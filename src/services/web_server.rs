use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{
        Html, IntoResponse, Response,
        sse::{self, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures_util::{Stream, StreamExt, stream};
use std::convert::Infallible;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use super::{
    MatchServices, OverlayFeed, OverlayLibrary, OverlayTemplate, Settings, SettingsService,
    select_overlay,
};
use crate::competition::{
    CommandError, Competition, Event, Labels, MatchFormat, Side, TableMatch, Team,
};
use crate::models::ScoreUpdate;

struct AppState {
    overlay: OverlayFeed,
    overlays: OverlayLibrary,
    settings: Arc<RwLock<Settings>>,
    shutdown: tokio::sync::watch::Receiver<bool>,
}

pub struct WebServer {
    port: u16,
    overlay: OverlayFeed,
    settings_service: Arc<SettingsService>,
    settings: Arc<RwLock<Settings>>,
    match_services: MatchServices,
    runtime: Arc<Runtime>,
    shutdown_tx: Option<tokio::sync::watch::Sender<bool>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl WebServer {
    pub fn new(
        port: u16,
        overlay: OverlayFeed,
        settings_service: Arc<SettingsService>,
        settings: Arc<RwLock<Settings>>,
        match_services: MatchServices,
        runtime: Arc<Runtime>,
    ) -> Self {
        Self {
            port,
            overlay,
            settings_service,
            settings,
            match_services,
            runtime,
            shutdown_tx: None,
            handle: None,
        }
    }

    pub fn start(&mut self) -> Result<()> {
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);

        let state = Arc::new(AppState {
            overlay: self.overlay.clone(),
            overlays: self.settings_service.overlays(),
            settings: Arc::clone(&self.settings),
            shutdown: shutdown_rx.clone(),
        });

        // Template files are polled, so an edit reloads open overlay pages
        // without a restart.
        let overlays = state.overlays.clone();
        let feed = self.overlay.clone();
        let mut stop = shutdown_rx.clone();
        self.runtime.spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = stop.wait_for(|stop| *stop) => break,
                }
                let library = overlays.clone();
                if let Ok(fingerprint) =
                    tokio::task::spawn_blocking(move || library.fingerprint()).await
                {
                    feed.set_templates_fingerprint(fingerprint);
                }
            }
        });

        let app = app(state, self.match_services.clone(), shutdown_rx.clone());

        // Bind before returning, so a port in use is reported to the caller
        // instead of only to the log.
        let port = self.port;
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        let listener = self
            .runtime
            .block_on(tokio::net::TcpListener::bind(addr))
            .with_context(|| format!("port {port}"))?;
        tracing::info!("Started on port {}", port);

        let handle = self.runtime.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.wait_for(|stop| *stop).await;
                })
                .await
                .ok();

            tracing::info!("Server stopped");
        });

        self.shutdown_tx = Some(shutdown_tx);
        self.handle = Some(handle);

        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
    }
}

fn app(
    state: Arc<AppState>,
    match_services: MatchServices,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Router {
    let overlay_api = Router::new()
        .route("/overlay", get(overlay_events))
        .with_state(Arc::clone(&state));
    Router::new()
        .route("/", get(serve_selected_overlay))
        .route("/overlay/{id}", get(serve_overlay))
        .route("/overlay.js", get(serve_overlay_script))
        // Superseded by /api/intents; removed once the tablet uses the API.
        .route("/scores", post(handle_scores))
        .with_state(Arc::clone(&state))
        .nest(
            "/api",
            super::match_api::router(match_services, shutdown).merge(overlay_api),
        )
        .layer(middleware::from_fn(log_requests))
        .layer(CorsLayer::permissive())
}

/// One line per request: debug when it went well, warn for a client error,
/// error for a server error. SSE streams are logged when they end.
async fn log_requests(req: axum::http::Request<Body>, next: Next) -> impl IntoResponse {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let client = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("?")
        .to_owned();
    let started = std::time::Instant::now();
    let response = next.run(req).await;
    let status = response.status();
    let took = started.elapsed().as_millis();
    if status.is_server_error() {
        tracing::error!("{method} {uri} → {status} ({took} ms, {client})");
    } else if status.is_client_error() {
        tracing::warn!("{method} {uri} → {status} ({took} ms, {client})");
    } else {
        tracing::debug!("{method} {uri} → {status} ({took} ms, {client})");
    }
    response
}

/// The template selected for the competition of the match on the table, or
/// for the chosen competition while there is none (ADR 0010, 0012).
async fn serve_selected_overlay(State(state): State<Arc<AppState>>) -> Response {
    let settings = state.settings.read().await;
    let source = state
        .overlay
        .overlay_key()
        .or(settings.competition.map(|c| c.key().to_owned()));
    let choices = settings.overlay_choices.clone();
    drop(settings);
    let templates = state.overlays.templates();
    match select_overlay(&templates, source.as_deref(), &choices) {
        Some(template) => overlay_page(template, true),
        None => overlay_not_found(),
    }
}

/// `/overlay/<id>`, also accepted with `.html`.
async fn serve_overlay(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let id = id.strip_suffix(".html").unwrap_or(&id);
    match state.overlays.get(id) {
        Some(template) => overlay_page(&template, false),
        None => overlay_not_found(),
    }
}

/// The template as it is, with the overlay script added. `follow` pages
/// reload when the template selected for `/` changes.
fn overlay_page(template: &OverlayTemplate, follow: bool) -> Response {
    no_cache(Html(with_overlay_script(&template.html, follow)))
}

fn with_overlay_script(html: &str, follow: bool) -> String {
    let tag = if follow {
        r#"<script src="/overlay.js" data-follow></script>"#
    } else {
        r#"<script src="/overlay.js"></script>"#
    };
    match html.rfind("</body>") {
        Some(end) => format!("{}{tag}\n{}", &html[..end], &html[end..]),
        None => format!("{html}\n{tag}"),
    }
}

async fn serve_overlay_script(State(state): State<Arc<AppState>>) -> Response {
    no_cache((
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        state.overlays.script().into_owned(),
    ))
}

/// OBS keeps a browser source open for hours; never serve it a stale page.
fn no_cache(response: impl IntoResponse) -> Response {
    ([(header::CACHE_CONTROL, "no-cache")], response).into_response()
}

/// `overlay` with the view on connect and on every change (`null`: nothing
/// on air), `reload` with the template id when `/` serves another template,
/// `templates` when a template file was edited (every page reloads).
async fn overlay_events(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<sse::Event, Infallible>>> {
    let views = stream::unfold(
        (state.overlay.subscribe(), true),
        |(mut rx, first)| async move {
            if !first {
                rx.changed().await.ok()?;
            }
            let data = serde_json::to_string(&*rx.borrow_and_update()).ok()?;
            Some((
                Ok(sse::Event::default().event("overlay").data(data)),
                (rx, false),
            ))
        },
    );
    let mut selected = state.overlay.subscribe_selected_template();
    selected.borrow_and_update();
    let reloads = stream::unfold(selected, |mut rx| async move {
        rx.changed().await.ok()?;
        // An event without data is not dispatched by EventSource.
        let id = rx.borrow_and_update().clone().unwrap_or_else(|| "-".into());
        Some((Ok(sse::Event::default().event("reload").data(id)), rx))
    });
    let mut templates = state.overlay.subscribe_templates();
    templates.borrow_and_update();
    let edits = stream::unfold(templates, |mut rx| async move {
        rx.changed().await.ok()?;
        let fingerprint = rx.borrow_and_update().to_string();
        Some((
            Ok(sse::Event::default().event("templates").data(fingerprint)),
            rx,
        ))
    });
    let mut shutdown = state.shutdown.clone();
    let stopped = async move {
        let _ = shutdown.wait_for(|stop| *stop).await;
    };
    let events = stream::select(stream::select(views, reloads), edits);
    Sse::new(events.take_until(stopped)).keep_alive(KeepAlive::default())
}

fn overlay_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Html("<html><body><h1>Overlay nicht gefunden</h1></body></html>"),
    )
        .into_response()
}

/// The old tablet protocol, as an adapter onto the match on the table
/// (ADR 0012): names become entered team names and labels, scores a
/// correction. Without a match it starts a Frei match. Removed once the
/// tablet uses `/api`.
async fn handle_scores(
    State(state): State<Arc<AppState>>,
    Json(scores): Json<ScoreUpdate>,
) -> StatusCode {
    tracing::debug!("Received score update: {:?}", scores);
    let store = state.overlay.store();
    if store.current().await.is_none() {
        store
            .replace(Some(TableMatch::single_game(
                Competition::Manual,
                Labels::default(),
                Team::named(""),
                Team::named(""),
                false,
                MatchFormat::Sets {},
            )))
            .await;
    }
    let result = store
        .mutate_current(|m, revision| apply_score_update(m, scores, revision))
        .await;
    match result {
        Ok(_) => StatusCode::OK,
        Err(e) => {
            tracing::warn!("Score update rejected: {e}");
            StatusCode::UNPROCESSABLE_ENTITY
        }
    }
}

fn apply_score_update(
    m: &mut TableMatch,
    scores: ScoreUpdate,
    revision: u64,
) -> Result<(), CommandError> {
    let given = |v: Option<String>| v.filter(|s| !s.trim().is_empty());
    let labels = given(scores.event_name).map(|tournament| Labels {
        tournament,
        ..m.labels.clone()
    });
    m.set_setup(
        labels,
        given(scores.team_a_name),
        given(scores.team_b_name),
        revision,
    )?;
    for (side, players) in [
        (Side::Home, given(scores.team_a_player)),
        (Side::Away, given(scores.team_b_player)),
    ] {
        if let Some(players) = players {
            let slot = m.current_slot(side);
            let names = players.split(" / ").map(str::to_owned).collect();
            m.set_lineup(side, slot, names, m.revision())?;
        }
    }
    let current = m.scoreboard();
    let score =
        |v: Option<i32>, now: u16| v.map_or(now, |v| v.clamp(0, i32::from(u16::MAX)) as u16);
    let home = score(scores.team_a_score, current.home.score);
    let away = score(scores.team_b_score, current.away.score);
    if (home, away) != (current.home.score, current.away.score) {
        m.apply(Event::Correct { home, away }, m.revision())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mtfv::MtfvClient;
    use crate::services::MatchStore;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_app(overlay: OverlayFeed, settings: Settings) -> Router {
        // Never stopped. A dropped sender would end the SSE streams at once.
        let (tx, shutdown) = tokio::sync::watch::channel(false);
        std::mem::forget(tx);
        let services = MatchServices {
            store: overlay.store().clone(),
            mtfv: Arc::new(MtfvClient::with_base_url("http://127.0.0.1:9").unwrap()),
            settings: Default::default(),
            trackers: Default::default(),
        };
        let state = Arc::new(AppState {
            overlay,
            overlays: crate::services::overlays::tests::test_library(),
            settings: Arc::new(RwLock::new(settings)),
            shutdown: shutdown.clone(),
        });
        app(state, services, shutdown)
    }

    fn overlay_manager() -> OverlayFeed {
        OverlayFeed::new(MatchStore::in_memory(), "/nonexistent".into())
    }

    fn frei_match() -> TableMatch {
        TableMatch::single_game(
            Competition::Manual,
            Labels::default(),
            Team::named("A"),
            Team::named("B"),
            false,
            crate::competition::MatchFormat::default(),
        )
    }

    async fn get_page(app: Router, uri: &str) -> (StatusCode, String) {
        let response = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn overlays_by_name() {
        let app = test_app(overlay_manager(), Settings::default());
        let (status, body) = get_page(app.clone(), "/overlay/mtfv_race_42").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(r#"content="MTFV Race 42""#));
        let (status, _) = get_page(app.clone(), "/overlay/mtfv_race_42.html").await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = get_page(app, "/overlay/missing").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn root_serves_the_selection_for_the_competition_on_the_table() {
        let overlay = overlay_manager();
        let mut settings = Settings::default();
        settings
            .overlay_choices
            .insert("manual:race".into(), "mtfv_race_42".into());
        settings.competition = Some(crate::services::CompetitionKind::Kickertool);
        let app = test_app(overlay.clone(), settings);
        let name = |body: String| {
            ["Turnier Einzel", "MTFV Race 42"]
                .into_iter()
                .find(|n| body.contains(&format!(r#"name="tfc:name" content="{n}""#)))
        };

        // No match: the chosen competition decides.
        assert_eq!(
            name(get_page(app.clone(), "/").await.1),
            Some("Turnier Einzel")
        );
        // A Frei race on the table: its competition and kind decide.
        let race = TableMatch::single_game(
            Competition::Manual,
            Labels::default(),
            Team::named("A"),
            Team::named("B"),
            false,
            crate::competition::MatchFormat::Race {
                target: 10,
                segment: 0,
            },
        );
        overlay.store().replace(Some(race)).await;
        assert_eq!(name(get_page(app, "/").await.1), Some("MTFV Race 42"));
    }

    #[tokio::test]
    async fn pages_get_the_overlay_script_and_raw_placeholders() {
        let app = test_app(overlay_manager(), Settings::default());
        let (_, root) = get_page(app.clone(), "/").await;
        assert!(root.contains(r#"<script src="/overlay.js" data-follow></script>"#));
        assert!(root.contains("{{teamA}}"), "the script fills placeholders");
        let (_, named) = get_page(app.clone(), "/overlay/player_overlay").await;
        assert!(named.contains(r#"<script src="/overlay.js"></script>"#));
        let (status, script) = get_page(app, "/overlay.js").await;
        assert_eq!(status, StatusCode::OK);
        assert!(script.contains("/api/overlay"));
    }

    #[test]
    fn script_goes_before_the_last_body_end() {
        assert_eq!(
            with_overlay_script("<body>x</body></html>", false),
            "<body>x<script src=\"/overlay.js\"></script>\n</body></html>"
        );
        assert_eq!(
            with_overlay_script("x", true),
            "x\n<script src=\"/overlay.js\" data-follow></script>"
        );
    }

    /// Reads SSE frames until `count` events of the given names arrived.
    async fn read_events(body: Body, count: usize) -> Vec<(String, String)> {
        let mut stream = body.into_data_stream();
        let mut text = String::new();
        let mut events = Vec::new();
        while events.len() < count {
            let chunk = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
                .await
                .expect("event in time")
                .unwrap()
                .unwrap();
            text.push_str(std::str::from_utf8(&chunk).unwrap());
            while let Some(end) = text.find("\n\n") {
                let frame: String = text.drain(..end + 2).collect();
                let field = |name: &str| {
                    frame
                        .lines()
                        .find_map(|l| l.strip_prefix(name))
                        .map(str::to_owned)
                };
                if let (Some(event), Some(data)) = (field("event: "), field("data: ")) {
                    events.push((event, data));
                }
            }
        }
        events
    }

    #[tokio::test]
    async fn overlay_stream_pushes_views_and_reloads() {
        let overlay = overlay_manager();
        let app = test_app(overlay.clone(), Settings::default());
        let response = app
            .oneshot(Request::get("/api/overlay").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let pushes = tokio::spawn(read_events(response.into_body(), 3));
        tokio::task::yield_now().await;
        overlay.store().replace(Some(frei_match())).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        overlay.set_selected_template(Some("mtfv_race_42".into()));

        let events = pushes.await.unwrap();
        assert_eq!(events[0], ("overlay".into(), "null".into()));
        assert_eq!(events[1].0, "overlay");
        assert!(events[1].1.contains(r#""source":"manual""#));
        assert_eq!(events[2], ("reload".into(), "mtfv_race_42".into()));
    }

    async fn post_scores(app: Router, json: &str) -> StatusCode {
        app.oneshot(
            Request::post("/scores")
                .header("content-type", "application/json")
                .body(Body::from(json.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
    }

    #[tokio::test]
    async fn tracker_reports_are_served_under_api_and_leave_the_match_alone() {
        let overlay = overlay_manager();
        let app = test_app(overlay.clone(), Settings::default());
        let status = app
            .clone()
            .oneshot(
                Request::post("/api/trackers")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"id":"a","name":"Pixel","reading":{"goals":3}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status();
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, body) = get_page(app, "/api/trackers").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(r#""name":"Pixel""#), "{body}");
        assert!(overlay.store().current().await.is_none());
    }

    #[tokio::test]
    async fn scores_start_a_frei_match_and_correct_the_score() {
        let overlay = overlay_manager();
        let app = test_app(overlay.clone(), Settings::default());
        let status = post_scores(
            app.clone(),
            r#"{"teamAName":"Halle","teamBName":"Jena","teamAScore":3,"teamBScore":1,
                "teamAPlayer":"Ann / Bo","eventName":"Liga"}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let sb = overlay.store().scoreboard().unwrap();
        assert_eq!(sb.competition, "manual");
        assert_eq!(
            (sb.home.team.as_str(), sb.away.team.as_str()),
            ("Halle", "Jena")
        );
        assert_eq!((sb.home.score, sb.away.score), (3, 1));
        assert_eq!(sb.labels.tournament, "Liga");
        assert_eq!(sb.home.players.len(), 2);

        // Only the score: names stay, and an unchanged score adds no event.
        post_scores(app.clone(), r#"{"teamAScore":4}"#).await;
        post_scores(app, r#"{"teamAScore":4}"#).await;
        let m = overlay.store().current().await.unwrap();
        assert_eq!(m.home.name, "Halle");
        assert_eq!(m.scoreboard().home.score, 4);
        assert_eq!(m.events().len(), 2);
    }

    #[tokio::test]
    async fn scores_apply_to_any_match_on_the_table() {
        let overlay = overlay_manager();
        overlay
            .store()
            .replace(Some(crate::services::match_store::tests::sample_day()))
            .await;
        let app = test_app(overlay.clone(), Settings::default());
        post_scores(app, r#"{"teamAScore":2,"teamBScore":0}"#).await;
        let sb = overlay.store().scoreboard().unwrap();
        assert_eq!(sb.competition, "manual");
        assert_eq!((sb.home.score, sb.away.score), (2, 0));
    }
}
