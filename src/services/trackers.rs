//! Health reports of the tracker phones, one per table end, which read the
//! score counters with the camera.
//!
//! `POST /trackers`: a phone's report, about every 5 s. `GET /trackers`: the
//! latest report per phone with `last_seen_s`. Only kept in memory, and kept
//! apart from the match: a report never changes the score.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A phone that has not reported for this long is shown as offline.
pub const OFFLINE_AFTER: Duration = Duration::from_secs(15);
/// A phone that has not reported for this long is forgotten.
const FORGET_AFTER: Duration = Duration::from_secs(10 * 60);

/// What a phone reports. Everything but `id` may be missing or null, and
/// unknown fields are ignored: the phone app evolves on its own.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct TrackerReport {
    /// Stable per phone (Android ANDROID_ID).
    pub id: String,
    pub name: Option<String>,
    pub app_version: Option<String>,
    pub ip: Option<String>,
    pub debug_url: Option<String>,
    pub uptime_s: Option<u64>,
    pub battery: Option<Battery>,
    /// dBm.
    pub wifi_rssi: Option<i32>,
    /// The end of the table the phone found itself at: "A" or "B".
    pub table_end: Option<String>,
    /// The team this end's goal counter counts: "home" or "away".
    pub side: Option<String>,
    pub reading: Option<Reading>,
    pub camera: Option<Camera>,
    pub calibration: Option<String>,
    pub sync: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Battery {
    /// 0..100.
    pub level: Option<u8>,
    pub charging: Option<bool>,
    pub temperature_c: Option<f64>,
}

/// The counters as the camera reads them.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Reading {
    pub goals: Option<u32>,
    pub sets: Option<u32>,
    pub timeouts: Option<u32>,
    pub confidence: Option<f64>,
    #[serde(default)]
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Camera {
    pub fps: Option<f64>,
    pub zoom: Option<f64>,
    pub rotation: Option<i32>,
    pub exposure_ns: Option<u64>,
    pub iso: Option<u32>,
    pub stone_luma: Option<f64>,
    pub target_luma: Option<f64>,
    pub read_ms: Option<f64>,
}

impl TrackerReport {
    /// The name, or the id for a phone that sent none.
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

/// A phone's latest report and how long ago it arrived.
#[derive(Debug, Clone, Serialize)]
pub struct TrackerStatus {
    #[serde(flatten)]
    pub report: TrackerReport,
    #[serde(rename = "last_seen_s", serialize_with = "as_secs")]
    pub last_seen: Duration,
}

impl TrackerStatus {
    pub fn online(&self) -> bool {
        self.last_seen <= OFFLINE_AFTER
    }
}

fn as_secs<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(d.as_secs_f64())
}

/// The latest report per phone. Shared by the web server and the UI; the
/// lock is held only to copy, so a std mutex does on both sides.
#[derive(Clone, Default)]
pub struct Trackers {
    inner: Arc<Mutex<HashMap<String, (TrackerReport, Instant)>>>,
}

impl Trackers {
    pub fn report(&self, report: TrackerReport) {
        self.report_at(report, Instant::now());
    }

    /// The latest report per phone, by name then id.
    pub fn snapshot(&self) -> Vec<TrackerStatus> {
        self.snapshot_at(Instant::now())
    }

    fn report_at(&self, report: TrackerReport, now: Instant) {
        let mut phones = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        forget_old(&mut phones, now);
        // Every 5 s per phone is too much for the log; arriving and coming
        // back are what the operator needs to see.
        let ip = report.ip.as_deref().unwrap_or("?");
        match phones.get(&report.id) {
            None => tracing::info!(
                "Tracker {} ({}) reports from {ip}",
                report.label(),
                report.id
            ),
            Some((_, seen)) if now.duration_since(*seen) > OFFLINE_AFTER => tracing::info!(
                "Tracker {} ({}) is back after {} s, at {ip}",
                report.label(),
                report.id,
                now.duration_since(*seen).as_secs()
            ),
            Some(_) => {}
        }
        phones.insert(report.id.clone(), (report, now));
    }

    fn snapshot_at(&self, now: Instant) -> Vec<TrackerStatus> {
        let mut phones = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        forget_old(&mut phones, now);
        let mut all: Vec<_> = phones
            .values()
            .map(|(report, seen)| TrackerStatus {
                report: report.clone(),
                last_seen: now.duration_since(*seen),
            })
            .collect();
        drop(phones);
        all.sort_by(|a, b| (&a.report.name, &a.report.id).cmp(&(&b.report.name, &b.report.id)));
        all
    }
}

fn forget_old(phones: &mut HashMap<String, (TrackerReport, Instant)>, now: Instant) {
    phones.retain(|id, (report, seen)| {
        let keep = now.duration_since(*seen) <= FORGET_AFTER;
        if !keep {
            tracing::info!(
                "Tracker {} ({id}) forgotten, gone for 10 min",
                report.label()
            );
        }
        keep
    });
}

/// Routes to nest under `/api`.
pub fn router(trackers: Trackers) -> Router {
    Router::new()
        .route("/trackers", get(list).post(post_report))
        .with_state(trackers)
}

/// A missing `id` is already a 422 from the `Json` extractor; an empty one
/// would merge every such phone into one row.
async fn post_report(
    State(trackers): State<Trackers>,
    Json(report): Json<TrackerReport>,
) -> Response {
    if report.id.trim().is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "id is empty" })),
        )
            .into_response();
    }
    trackers.report(report);
    StatusCode::NO_CONTENT.into_response()
}

async fn list(State(trackers): State<Trackers>) -> Json<Vec<TrackerStatus>> {
    Json(trackers.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(body.map(|b| Body::from(b.to_owned())).unwrap_or_default())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    async fn post(app: &Router, body: serde_json::Value) -> StatusCode {
        call(app, "POST", "/trackers", Some(&body.to_string()))
            .await
            .0
    }

    fn full_report() -> serde_json::Value {
        json!({
            "id": "3f9a1c2b4d5e6f70",
            "name": "TrebleDroid_vanilla",
            "app_version": "0.1",
            "ip": "192.168.1.48",
            "debug_url": "http://192.168.1.48:8081/",
            "uptime_s": 1234,
            "battery": { "level": 87, "charging": true, "temperature_c": 31.5 },
            "wifi_rssi": -69,
            "table_end": "A",
            "side": "home",
            "reading": { "goals": 2, "sets": 0, "timeouts": 1, "confidence": 0.8, "problems": [] },
            "camera": { "fps": 24.9, "zoom": 2.75, "rotation": 180, "exposure_ns": 10000000,
                        "iso": 400, "stone_luma": 196.0, "target_luma": 200.0, "read_ms": 12.3 },
            "calibration": "done: rotation 180°, zoom 2.75",
            "sync": "sent home goals 2 sets 0"
        })
    }

    #[tokio::test]
    async fn full_report_is_listed_with_last_seen() {
        let app = router(Trackers::default());
        assert_eq!(post(&app, full_report()).await, StatusCode::NO_CONTENT);

        let (status, body) = call(&app, "GET", "/trackers", None).await;
        assert_eq!(status, StatusCode::OK);
        let phone = &body[0];
        assert_eq!(body.as_array().unwrap().len(), 1);
        assert_eq!(phone["id"], "3f9a1c2b4d5e6f70");
        assert_eq!(phone["battery"]["level"], 87);
        assert_eq!(phone["reading"]["timeouts"], 1);
        assert_eq!(phone["camera"]["rotation"], 180);
        assert_eq!(phone["debug_url"], "http://192.168.1.48:8081/");
        let last_seen = phone["last_seen_s"].as_f64().unwrap();
        assert!((0.0..5.0).contains(&last_seen), "{last_seen}");
    }

    #[tokio::test]
    async fn minimal_report_and_nulls_are_accepted() {
        let app = router(Trackers::default());
        assert_eq!(
            post(&app, json!({"id": "a", "name": "Pixel"})).await,
            StatusCode::NO_CONTENT
        );
        let nulls = json!({
            "id": "b", "battery": {"level": null, "charging": null, "temperature_c": null},
            "wifi_rssi": null, "table_end": null,
            "reading": {"goals": null, "sets": null, "timeouts": null, "confidence": null},
            "camera": {"fps": null}
        });
        assert_eq!(post(&app, nulls).await, StatusCode::NO_CONTENT);
        let (_, body) = call(&app, "GET", "/trackers", None).await;
        assert_eq!(body[1]["name"], "Pixel");
        assert!(body[1]["battery"].is_null());
    }

    #[tokio::test]
    async fn unknown_fields_are_ignored() {
        let app = router(Trackers::default());
        let mut report = full_report();
        report["thermal"] = json!({"state": "light"});
        report["battery"]["voltage_mv"] = json!(4100);
        assert_eq!(post(&app, report).await, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn missing_or_empty_id_is_422() {
        let app = router(Trackers::default());
        assert_eq!(
            post(&app, json!({"name": "Pixel"})).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            post(&app, json!({"id": " ", "name": "Pixel"})).await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(call(&app, "GET", "/trackers", None).await.1, json!([]));
    }

    #[tokio::test]
    async fn a_new_report_replaces_the_last_and_phones_are_sorted() {
        let app = router(Trackers::default());
        post(&app, json!({"id": "2", "name": "Pixel"})).await;
        post(&app, json!({"id": "1", "name": "Pixel"})).await;
        post(
            &app,
            json!({"id": "9", "name": "Galaxy", "reading": {"goals": 1}}),
        )
        .await;
        post(
            &app,
            json!({"id": "9", "name": "Galaxy", "reading": {"goals": 2}}),
        )
        .await;

        let (_, body) = call(&app, "GET", "/trackers", None).await;
        let ids: Vec<_> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["9", "1", "2"]);
        assert_eq!(body[0]["reading"]["goals"], 2);
    }

    #[test]
    fn phones_go_offline_then_are_forgotten() {
        let trackers = Trackers::default();
        let start = Instant::now();
        trackers.report_at(
            TrackerReport {
                id: "a".into(),
                ..Default::default()
            },
            start,
        );
        let at = |secs| trackers.snapshot_at(start + Duration::from_secs(secs));
        assert!(at(15)[0].online());
        assert!(!at(16)[0].online());
        assert_eq!(at(600).len(), 1);
        assert!(at(601).is_empty());
    }
}
