use anyhow::Result;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use super::{LogCallback, OverlayMode, OverlayStateManager, Settings, SettingsService, log};
use crate::models::{OverlayContent, ScoreUpdate};

struct AppState {
    overlay_state: OverlayStateManager,
    settings_service: Arc<SettingsService>,
    settings: Arc<RwLock<Settings>>,
    log_callback: LogCallback,
}

pub struct WebServer {
    port: u16,
    overlay_state: OverlayStateManager,
    settings_service: Arc<SettingsService>,
    settings: Arc<RwLock<Settings>>,
    log_callback: LogCallback,
    runtime: Arc<Runtime>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl WebServer {
    pub fn new(
        port: u16,
        overlay_state: OverlayStateManager,
        settings_service: Arc<SettingsService>,
        settings: Arc<RwLock<Settings>>,
        log_callback: LogCallback,
        runtime: Arc<Runtime>,
    ) -> Self {
        Self {
            port,
            overlay_state,
            settings_service,
            settings,
            log_callback,
            runtime,
            shutdown_tx: None,
            handle: None,
        }
    }

    pub fn start(&mut self) -> Result<()> {
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let state = Arc::new(AppState {
            overlay_state: self.overlay_state.clone(),
            settings_service: Arc::clone(&self.settings_service),
            settings: Arc::clone(&self.settings),
            log_callback: Arc::clone(&self.log_callback),
        });

        let app = Router::new()
            .route("/", get(serve_overlay))
            .route("/scores", post(handle_scores))
            .layer(CorsLayer::permissive())
            .with_state(state);

        let port = self.port;
        let log_callback = Arc::clone(&self.log_callback);

        // Use the runtime handle to spawn the server task
        let handle = self.runtime.spawn(async move {
            let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    log(
                        &log_callback,
                        "WebServer",
                        format!("Failed to bind to port {}: {}", port, e),
                    );
                    return;
                }
            };

            log(
                &log_callback,
                "WebServer",
                format!("Started on port {}", port),
            );

            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();

            log(&log_callback, "WebServer", "Server stopped");
        });

        self.shutdown_tx = Some(shutdown_tx);
        self.handle = Some(handle);

        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

async fn serve_overlay(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let settings = state.settings.read().await;
    let overlay_path = state.settings_service.get_overlay_path(&settings);

    let template = match std::fs::read_to_string(&overlay_path) {
        Ok(content) => content,
        Err(e) => {
            log(
                &state.log_callback,
                "WebServer",
                format!("Error reading HTML file: {}", e),
            );
            return Html(
                "<html><body><h1>Error: HTML file not found</h1></body></html>".to_string(),
            );
        }
    };

    let content = state.overlay_state.get_content().await;
    let rendered = render_template(&template, &content, settings.refresh_interval);

    Html(rendered)
}

async fn handle_scores(
    State(state): State<Arc<AppState>>,
    Json(scores): Json<ScoreUpdate>,
) -> impl IntoResponse {
    log(
        &state.log_callback,
        "WebServer",
        format!("Received score update: {:?}", scores),
    );

    let mode = state.overlay_state.get_mode().await;
    if mode != OverlayMode::Remote {
        log(
            &state.log_callback,
            "WebServer",
            "Ignoring score update - not in Remote mode",
        );
        return StatusCode::FORBIDDEN;
    }

    let old_content = state.overlay_state.get_content().await;
    let new_content = OverlayContent {
        table: old_content.table,
        team_a: scores.team_a_name,
        team_b: scores.team_b_name,
        score_a: scores.team_a_score.to_string(),
        score_b: scores.team_b_score.to_string(),
        team_a_player: scores.team_a_player,
        team_b_player: scores.team_b_player,
        tournament_name: scores.event_name,
        ..old_content
    };

    state.overlay_state.set_content(new_content).await;
    log(
        &state.log_callback,
        "WebServer",
        format!(
            "Updated scores to {}:{}",
            scores.team_a_score, scores.team_b_score
        ),
    );

    StatusCode::OK
}

fn render_template(template: &str, content: &OverlayContent, refresh_interval: u32) -> String {
    template
        .replace("{{table}}", &content.table.name)
        .replace("{{tournamentName}}", &content.tournament_name)
        .replace("{{teamA}}", &content.team_a)
        .replace("{{teamB}}", &content.team_b)
        .replace("{{teamAPlayer}}", &content.team_a_player)
        .replace("{{teamBPlayer}}", &content.team_b_player)
        .replace("{{scoreName}}", &content.score_name)
        .replace("{{scoreA}}", &content.score_a)
        .replace("{{scoreB}}", &content.score_b)
        .replace("{{setsA}}", &content.sets_a)
        .replace("{{setsB}}", &content.sets_b)
        .replace("{{started}}", &content.start_time)
        .replace("{{state}}", &content.state)
        .replace("{{roundName}}", &content.round_name)
        .replace("{{groupName}}", &content.group_name)
        .replace("{{disciplineName}}", &content.discipline_name)
        .replace("{{setsName}}", &content.sets_name)
        .replace("{{refreshInterval}}", &refresh_interval.to_string())
}
