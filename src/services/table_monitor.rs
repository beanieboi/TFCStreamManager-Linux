use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, interval};

use super::{KickertoolApiService, LogCallback, OverlayStateManager, Settings, log};
use crate::models::{Match, Table, Tournament};

pub struct TableMonitor {
    api_service: Arc<KickertoolApiService>,
    overlay_state: OverlayStateManager,
    settings: Arc<RwLock<Settings>>,
    log_callback: LogCallback,
    tournament: Arc<RwLock<Option<Tournament>>>,
    table_id: Arc<RwLock<Option<String>>>,
    shutdown_tx: Arc<RwLock<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl TableMonitor {
    pub fn new(
        api_service: Arc<KickertoolApiService>,
        overlay_state: OverlayStateManager,
        settings: Arc<RwLock<Settings>>,
        log_callback: LogCallback,
    ) -> Self {
        Self {
            api_service,
            overlay_state,
            settings,
            log_callback,
            tournament: Arc::new(RwLock::new(None)),
            table_id: Arc::new(RwLock::new(None)),
            shutdown_tx: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn start_monitoring(&self, tournament: Tournament, table_id: String) {
        // Stop any existing monitoring
        self.stop_monitoring().await;

        let refresh_interval = {
            let settings = self.settings.read().await;
            settings.refresh_interval
        };

        {
            let mut t = self.tournament.write().await;
            *t = Some(tournament.clone());
        }
        {
            let mut tid = self.table_id.write().await;
            *tid = Some(table_id.clone());
        }
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        {
            let mut tx = self.shutdown_tx.write().await;
            *tx = Some(shutdown_tx);
        }

        log(
            &self.log_callback,
            "TableMonitor",
            format!(
                "Started monitoring table {} in tournament {} (refresh: {}s)",
                table_id, tournament.name, refresh_interval
            ),
        );

        let api_service = Arc::clone(&self.api_service);
        let overlay_state = self.overlay_state.clone();
        let settings = Arc::clone(&self.settings);
        let log_callback = Arc::clone(&self.log_callback);
        let tournament_arc = Arc::clone(&self.tournament);
        let table_id_arc = Arc::clone(&self.table_id);
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(refresh_interval as u64));

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let tournament_opt = tournament_arc.read().await.clone();
                        let table_id_opt = table_id_arc.read().await.clone();

                        if let (Some(tournament), Some(table_id)) = (tournament_opt, table_id_opt)
                            && let Err(e) = fetch_current_match(
                                &api_service,
                                &overlay_state,
                                &settings,
                                &log_callback,
                                &tournament,
                                &table_id,
                            ).await {
                            log(&log_callback, "TableMonitor", format!("Error: {}", e));
                        }
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }

            log(&log_callback, "TableMonitor", "Stopped monitoring");
        });
    }

    pub async fn stop_monitoring(&self) {
        let mut tx = self.shutdown_tx.write().await;
        if let Some(sender) = tx.take() {
            let _ = sender.send(());
        }
    }
}

async fn fetch_current_match(
    api_service: &KickertoolApiService,
    overlay_state: &OverlayStateManager,
    settings: &Arc<RwLock<Settings>>,
    log_callback: &LogCallback,
    tournament: &Tournament,
    table_id: &str,
) -> anyhow::Result<()> {
    let table: Table = api_service
        .get(&format!(
            "tournaments/{}/courts/{}",
            tournament.id, table_id
        ))
        .await?;

    let current_match_id = match &table.current_match_id {
        Some(id) => id,
        None => {
            log(log_callback, "TableMonitor", "No active match");
            overlay_state.reset().await;
            return Ok(());
        }
    };

    let match_data: Match = api_service
        .get(&format!(
            "tournaments/{}/matches/{}",
            tournament.id, current_match_id
        ))
        .await?;

    log(
        log_callback,
        "TableMonitor",
        format!(
            "Match state: {} Players: {} vs {} Started: {}",
            match_data.state,
            match_data.team_a(),
            match_data.team_b(),
            match_data
                .start_time
                .map(|t| t.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| "-".to_string())
        ),
    );

    let current_settings = settings.read().await;
    overlay_state
        .update_from_match(&match_data, &table, tournament, &current_settings)
        .await;

    Ok(())
}

impl Clone for TableMonitor {
    fn clone(&self) -> Self {
        Self {
            api_service: Arc::clone(&self.api_service),
            overlay_state: self.overlay_state.clone(),
            settings: Arc::clone(&self.settings),
            log_callback: Arc::clone(&self.log_callback),
            tournament: Arc::clone(&self.tournament),
            table_id: Arc::clone(&self.table_id),
            shutdown_tx: Arc::clone(&self.shutdown_tx),
        }
    }
}
