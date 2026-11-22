//! HTTP API for the match on the table (ADR 0006, 0012).
//!
//! Reads: `GET /match` (setup), `GET /scoreboard`, `GET /events` (SSE).
//! Commands, each with the revision the client last saw (a stale one is
//! answered with 409 and the current scoreboard): `POST /intents` (score),
//! `PUT /lineups`, `PUT /setup` (labels, team names), `PUT /format`.
//! Starting a match: `POST /match` (an MTFV Begegnung or a Frei match),
//! `DELETE /match`, `GET /mtfv/fixtures`.
//! Tracker phone health: `POST /trackers`, `GET /trackers` (see `trackers`).

use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{self, KeepAlive, Sse},
    },
    routing::{get, post, put},
};
use chrono::NaiveDate;
use futures_util::{Stream, StreamExt, stream};
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};

use super::match_store::{MatchStore, StoreError};
use super::{Settings, Trackers};
use crate::competition::{
    CommandError, Competition, Event, Labels, MatchFormat, Side, Slot, TableMatch, Team,
};
use crate::mtfv::{FormatRules, MtfvClient, fixtures_on};

/// What the match day API works on: the store and the competition source.
#[derive(Clone)]
pub struct MatchServices {
    pub store: MatchStore,
    pub mtfv: Arc<MtfvClient>,
    /// For the MTFV race target.
    pub settings: Arc<RwLock<Settings>>,
    /// Health of the tracker phones. Here because the phones use this API;
    /// their reports never touch the match.
    pub trackers: Trackers,
}

struct ApiState {
    store: MatchStore,
    mtfv: Arc<MtfvClient>,
    settings: Arc<RwLock<Settings>>,
    shutdown: watch::Receiver<bool>,
}

/// Routes to nest under `/api`. Open SSE streams end when `shutdown` turns
/// true, so a graceful shutdown does not wait for them forever.
pub fn router(services: MatchServices, shutdown: watch::Receiver<bool>) -> Router {
    Router::new()
        .route(
            "/match",
            get(get_match).post(load_match).delete(clear_match),
        )
        .route("/scoreboard", get(get_scoreboard))
        .route("/events", get(events))
        .route("/intents", post(post_intent))
        .route("/lineups", put(put_lineup))
        .route("/setup", put(put_setup))
        .route("/format", put(put_format))
        .route("/mtfv/fixtures", get(mtfv_fixtures))
        .with_state(Arc::new(ApiState {
            store: services.store,
            mtfv: services.mtfv,
            settings: services.settings,
            shutdown,
        }))
        .merge(super::trackers::router(services.trackers))
}

fn error(status: StatusCode, message: impl ToString) -> Response {
    (status, Json(json!({ "error": message.to_string() }))).into_response()
}

impl IntoResponse for StoreError {
    fn into_response(self) -> Response {
        match self {
            StoreError::NoMatch => error(StatusCode::NOT_FOUND, self),
            StoreError::Command(CommandError::Conflict { .. }) => error(StatusCode::CONFLICT, self),
            StoreError::Command(_) => error(StatusCode::UNPROCESSABLE_ENTITY, self),
        }
    }
}

/// A 409 carries the current scoreboard, so the client needs no refetch.
fn command_result(
    store: &MatchStore,
    result: Result<crate::competition::Scoreboard, StoreError>,
) -> Response {
    match result {
        Ok(scoreboard) => Json(scoreboard).into_response(),
        Err(e @ StoreError::Command(CommandError::Conflict { .. })) => (
            StatusCode::CONFLICT,
            Json(json!({ "error": e.to_string(), "scoreboard": store.scoreboard() })),
        )
            .into_response(),
        Err(e) => e.into_response(),
    }
}

async fn get_match(State(state): State<Arc<ApiState>>) -> Response {
    match state.store.current().await {
        Some(day) => Json(day).into_response(),
        None => StoreError::NoMatch.into_response(),
    }
}

async fn get_scoreboard(State(state): State<Arc<ApiState>>) -> Response {
    match state.store.scoreboard() {
        Some(scoreboard) => Json(scoreboard).into_response(),
        None => StoreError::NoMatch.into_response(),
    }
}

/// One `scoreboard` event with the current state on connect, then one per
/// change. `null` means no match day is loaded.
async fn events(
    State(state): State<Arc<ApiState>>,
) -> Sse<impl Stream<Item = Result<sse::Event, Infallible>>> {
    let updates = stream::unfold(
        (state.store.subscribe(), true),
        |(mut rx, first)| async move {
            if !first {
                rx.changed().await.ok()?;
            }
            let data = serde_json::to_string(&*rx.borrow_and_update()).ok()?;
            Some((
                Ok(sse::Event::default().event("scoreboard").data(data)),
                (rx, false),
            ))
        },
    );
    let mut shutdown = state.shutdown.clone();
    let stopped = async move {
        let _ = shutdown.wait_for(|stop| *stop).await;
    };
    Sse::new(updates.take_until(stopped)).keep_alive(KeepAlive::default())
}

#[derive(Deserialize)]
struct Intent {
    revision: u64,
    #[serde(flatten)]
    event: Event,
}

async fn post_intent(State(state): State<Arc<ApiState>>, Json(intent): Json<Intent>) -> Response {
    let result = state.store.apply(intent.event, intent.revision).await;
    command_result(&state.store, result)
}

#[derive(Deserialize)]
struct LineupChange {
    revision: u64,
    side: Side,
    slot: Slot,
    players: Vec<String>,
}

async fn put_lineup(
    State(state): State<Arc<ApiState>>,
    Json(change): Json<LineupChange>,
) -> Response {
    let result = state
        .store
        .set_lineup(change.side, change.slot, change.players, change.revision)
        .await;
    command_result(&state.store, result)
}

#[derive(Deserialize)]
struct SetupChange {
    revision: u64,
    labels: Option<Labels>,
    home: Option<String>,
    away: Option<String>,
}

async fn put_setup(
    State(state): State<Arc<ApiState>>,
    Json(change): Json<SetupChange>,
) -> Response {
    let result = state
        .store
        .set_setup(change.labels, change.home, change.away, change.revision)
        .await;
    command_result(&state.store, result)
}

#[derive(Deserialize)]
struct FormatChange {
    revision: u64,
    format: MatchFormat,
}

async fn put_format(
    State(state): State<Arc<ApiState>>,
    Json(change): Json<FormatChange>,
) -> Response {
    let result = state.store.set_format(change.format, change.revision).await;
    command_result(&state.store, result)
}

#[derive(Deserialize)]
struct FixturesQuery {
    date: Option<NaiveDate>,
}

async fn mtfv_fixtures(
    State(state): State<Arc<ApiState>>,
    Query(query): Query<FixturesQuery>,
) -> Response {
    let date = query
        .date
        .unwrap_or_else(|| chrono::Local::now().date_naive());
    match state.mtfv.season().await {
        Ok(season) => Json(fixtures_on(&season, date)).into_response(),
        Err(e) => error(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StartMatch {
    Mtfv {
        veranstaltung_id: u32,
        begegnung_id: u32,
    },
    Frei {
        #[serde(default)]
        labels: Labels,
        #[serde(default)]
        home: String,
        #[serde(default)]
        away: String,
        #[serde(default)]
        singles: bool,
        #[serde(default)]
        format: MatchFormat,
    },
}

/// Puts a new match on the table: a Begegnung from mtfv.de, or a Frei match.
async fn load_match(State(state): State<Arc<ApiState>>, Json(start): Json<StartMatch>) -> Response {
    let (veranstaltung_id, begegnung_id) = match start {
        StartMatch::Mtfv {
            veranstaltung_id,
            begegnung_id,
        } => (veranstaltung_id, begegnung_id),
        StartMatch::Frei {
            labels,
            home,
            away,
            singles,
            format,
        } => {
            let table_match = TableMatch::single_game(
                Competition::Manual,
                labels,
                Team::named(home),
                Team::named(away),
                singles,
                format,
            );
            state.store.replace(Some(table_match)).await;
            return created(&state.store);
        }
    };
    // From the season overview: the Begegnung exists there before any
    // result is entered (ADR 0012).
    let season = match state.mtfv.season().await {
        Ok(season) => season,
        Err(e) => return error(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    };
    let Some(league) = season.iter().find(|l| l.id() == veranstaltung_id) else {
        return error(
            StatusCode::NOT_FOUND,
            format!("league {veranstaltung_id} is not in the season"),
        );
    };
    let rules = {
        let settings = state.settings.read().await;
        FormatRules::race_to(settings.race_target, settings.race_segment)
    };
    let day = match league.table_match(begegnung_id, &rules) {
        Ok(day) => day,
        Err(e) => return error(StatusCode::UNPROCESSABLE_ENTITY, e),
    };
    state.store.replace(Some(day)).await;
    created(&state.store)
}

fn created(store: &MatchStore) -> Response {
    match store.scoreboard() {
        Some(scoreboard) => (StatusCode::CREATED, Json(scoreboard)).into_response(),
        None => StoreError::NoMatch.into_response(),
    }
}

async fn clear_match(State(state): State<Arc<ApiState>>) -> StatusCode {
    state.store.replace(None).await;
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::Request;
    use tower::ServiceExt;

    use crate::competition::{
        Competition, Format, Labels, TableMatch, parse_modus, test_support::team,
    };

    fn app() -> (Router, MatchStore, watch::Sender<bool>) {
        let store = MatchStore::in_memory();
        let (tx, rx) = watch::channel(false);
        let mtfv = Arc::new(MtfvClient::with_base_url("http://127.0.0.1:9").unwrap());
        (
            router(
                MatchServices {
                    store: store.clone(),
                    mtfv,
                    settings: Default::default(),
                    trackers: Default::default(),
                },
                rx,
            ),
            store,
            tx,
        )
    }

    async fn load(store: &MatchStore) {
        let day = TableMatch::new(
            Labels {
                tournament: "Test".into(),
                ..Default::default()
            },
            team("h", &[("1", "A"), ("2", "B")]),
            team("a", &[("3", "C")]),
            Format::mtfv_race(),
            parse_modus("D1D1,E1E1").unwrap(),
            Competition::Mtfv {
                veranstaltung_id: 71,
                begegnung_id: 1,
                teamspiel_ids: vec![],
            },
        )
        .unwrap();
        store.replace(Some(day)).await;
    }

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

    #[tokio::test]
    async fn reads_are_404_without_match_day() {
        let (app, _, _) = app();
        assert_eq!(
            call(&app, "GET", "/scoreboard", None).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(&app, "GET", "/match", None).await.0,
            StatusCode::NOT_FOUND
        );
        let (status, _) = call(
            &app,
            "POST",
            "/intents",
            Some(r#"{"revision":0,"type":"undo"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn goal_then_stale_retry_conflicts() {
        let (app, store, _) = app();
        load(&store).await;
        let goal = r#"{"revision":0,"type":"goal","side":"home"}"#;

        let (status, body) = call(&app, "POST", "/intents", Some(goal)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["revision"], 1);
        assert_eq!(body["home"]["score"], 1);

        let (status, body) = call(&app, "POST", "/intents", Some(goal)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["scoreboard"]["revision"], 1);
        assert_eq!(body["scoreboard"]["home"]["score"], 1);
    }

    #[tokio::test]
    async fn invalid_intent_is_422() {
        let (app, store, _) = app();
        load(&store).await;
        let (status, body) = call(
            &app,
            "POST",
            "/intents",
            Some(r#"{"revision":0,"type":"undo"}"#),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["error"], "nothing to undo");
    }

    #[tokio::test]
    async fn lineup_is_set_and_resolved() {
        let (app, store, _) = app();
        load(&store).await;
        let (status, body) = call(
            &app,
            "PUT",
            "/lineups",
            Some(r#"{"revision":0,"side":"home","slot":"D1","players":["1","2"]}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["home"]["players"][1]["first_name"], "B");

        // Not on the roster: a typed name (ADR 0012).
        let (status, body) = call(
            &app,
            "PUT",
            "/lineups",
            Some(r#"{"revision":1,"side":"away","slot":"D1","players":["Max Muster"]}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["away"]["players"][0]["first_name"], "Max Muster");

        let (status, _) = call(
            &app,
            "PUT",
            "/lineups",
            Some(r#"{"revision":2,"side":"away","slot":"E1","players":["A","B"]}"#),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn frei_match_setup_format_and_correction() {
        let (app, store, _) = app();
        let (status, body) = call(
            &app,
            "POST",
            "/match",
            Some(
                &json!({"home": "A", "away": "B", "format": {"kind": "race", "target": 5}})
                    .to_string(),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["competition"], "manual");
        assert_eq!(
            body["match_format"],
            json!({"kind": "race", "target": 5, "segment": 0})
        );

        let (status, body) = call(
            &app,
            "PUT",
            "/setup",
            Some(
                &json!({"revision": 0, "labels": {"tournament": "Cup"}, "home": "Ann"}).to_string(),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["labels"]["tournament"], "Cup");
        assert_eq!(body["home"]["team"], "Ann");
        assert_eq!(body["away"]["team"], "B");

        let (status, body) = call(
            &app,
            "POST",
            "/intents",
            Some(&json!({"revision": 1, "type": "correct", "home": 4, "away": 2}).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            (body["home"]["score"].clone(), body["away"]["score"].clone()),
            (json!(4), json!(2))
        );

        let (status, body) = call(
            &app,
            "PUT",
            "/format",
            Some(&json!({"revision": 2, "format": {"kind": "race", "target": 4}}).to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["outcome"], "home", "4:2 wins a race to 4");

        let (status, _) = call(
            &app,
            "PUT",
            "/lineups",
            Some(
                &json!({"revision": 3, "side": "home", "slot": "D1", "players": ["Max Muster"]})
                    .to_string(),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<_> = store
            .scoreboard()
            .unwrap()
            .home
            .players
            .iter()
            .map(|p| p.display_name())
            .collect();
        assert_eq!(names, ["Max Muster"]);
    }

    #[tokio::test]
    async fn format_of_an_mtfv_match_is_fixed() {
        let (app, store, _) = app();
        load(&store).await;
        let revision = store.scoreboard().unwrap().revision;
        let (status, _) = call(
            &app,
            "PUT",
            "/format",
            Some(
                &json!({"revision": revision, "format": {"kind": "race", "target": 4}}).to_string(),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn clear_removes_match_day() {
        let (app, store, _) = app();
        load(&store).await;
        assert_eq!(
            call(&app, "DELETE", "/match", None).await.0,
            StatusCode::NO_CONTENT
        );
        assert!(store.scoreboard().is_none());
    }

    #[tokio::test]
    async fn sse_sends_current_state_then_changes_and_ends_on_shutdown() {
        let (app, store, shutdown) = app();
        load(&store).await;
        let res = app
            .oneshot(Request::get("/events").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.headers()["content-type"], "text/event-stream");
        let mut body = res.into_body().into_data_stream();

        let first = String::from_utf8(body.next().await.unwrap().unwrap().to_vec()).unwrap();
        assert!(
            first.starts_with("event: scoreboard\ndata: {\"revision\":0"),
            "{first}"
        );

        store
            .apply(Event::Goal { side: Side::Away }, 0)
            .await
            .unwrap();
        let second = String::from_utf8(body.next().await.unwrap().unwrap().to_vec()).unwrap();
        assert!(second.contains("\"revision\":1"), "{second}");

        store.replace(None).await;
        let third = String::from_utf8(body.next().await.unwrap().unwrap().to_vec()).unwrap();
        assert_eq!(third, "event: scoreboard\ndata: null\n\n");

        shutdown.send(true).unwrap();
        assert!(body.next().await.is_none());
    }
}
