use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;
use tokio::task::AbortHandle;
use tokio::time::{Duration, interval};

use super::{KickertoolApiService, LogCallback, OverlayStateManager, Settings, log};
use crate::models::{Match, Table, Tournament};

pub struct TableMonitor {
    api_service: Arc<KickertoolApiService>,
    overlay_state: OverlayStateManager,
    settings: Arc<RwLock<Settings>>,
    log_callback: LogCallback,
    task_handle: Arc<Mutex<Option<AbortHandle>>>,
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
            task_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start_monitoring(&self, tournament: Tournament, table_id: String) {
        self.stop_monitoring().await;

        let refresh_interval = {
            let settings = self.settings.read().await;
            settings.refresh_interval
        };

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
        let task = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(refresh_interval as u64));

            loop {
                ticker.tick().await;

                if let Err(e) = fetch_current_match(
                    &api_service,
                    &overlay_state,
                    &settings,
                    &log_callback,
                    &tournament,
                    &table_id,
                )
                .await
                {
                    log(&log_callback, "TableMonitor", format!("Error: {}", e));
                }
            }
        });

        let mut task_handle = self
            .task_handle
            .lock()
            .expect("table monitor lock poisoned");
        *task_handle = Some(task.abort_handle());
    }

    pub async fn stop_monitoring(&self) {
        let mut task_handle = self
            .task_handle
            .lock()
            .expect("table monitor lock poisoned");
        if let Some(handle) = task_handle.take() {
            handle.abort();
            log(&self.log_callback, "TableMonitor", "Stopped monitoring");
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
            task_handle: Arc::clone(&self.task_handle),
        }
    }
}
