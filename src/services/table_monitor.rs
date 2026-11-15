//! The Kickertool source (ADR 0012): polls the selected table and puts its
//! current match on the table. Scores are entered, not polled; tournament.io
//! has none.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;
use tokio::task::AbortHandle;
use tokio::time::{Duration, interval};

use super::{KickertoolApiService, MatchStore, Settings};
use crate::competition::{Competition, MatchFormat};
use crate::models::{Match, Table, Tournament, Translations};

#[derive(Clone)]
pub struct TableMonitor {
    api_service: Arc<KickertoolApiService>,
    store: MatchStore,
    settings: Arc<RwLock<Settings>>,
    translations_path: PathBuf,
    task_handle: Arc<Mutex<Option<AbortHandle>>>,
}

impl TableMonitor {
    pub fn new(
        api_service: Arc<KickertoolApiService>,
        store: MatchStore,
        settings: Arc<RwLock<Settings>>,
        translations_path: PathBuf,
    ) -> Self {
        Self {
            api_service,
            store,
            settings,
            translations_path,
            task_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start_monitoring(&self, tournament: Tournament, table_id: String) {
        self.stop_monitoring().await;

        let refresh_interval = self.settings.read().await.refresh_interval;
        tracing::info!(
            "Started monitoring table {} in tournament {} (refresh: {}s)",
            table_id,
            tournament.name,
            refresh_interval
        );

        // Read on every start, so edits apply when the table is reselected.
        let translations = Translations::load(&self.translations_path).unwrap_or_else(|e| {
            tracing::warn!("Ignoring {}: {:#}", self.translations_path.display(), e);
            Translations::bundled()
        });

        let poll = Poll {
            api: Arc::clone(&self.api_service),
            store: self.store.clone(),
            tournament,
            table_id,
            translations,
        };
        let task = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(u64::from(refresh_interval)));
            // The match last put on the table. Only a new one replaces the
            // match, so entered scores survive polls and a match loaded
            // elsewhere (e.g. by the tablet) is not overwritten.
            let mut last_match: Option<String> = None;
            loop {
                ticker.tick().await;
                if let Err(e) = poll.run(&mut last_match).await {
                    tracing::warn!("Kickertool poll failed: {e:#}");
                }
            }
        });

        *self
            .task_handle
            .lock()
            .expect("table monitor lock poisoned") = Some(task.abort_handle());
    }

    pub async fn stop_monitoring(&self) {
        let handle = self
            .task_handle
            .lock()
            .expect("table monitor lock poisoned")
            .take();
        if let Some(handle) = handle {
            handle.abort();
            tracing::info!("Stopped monitoring");
        }
    }
}

struct Poll {
    api: Arc<KickertoolApiService>,
    store: MatchStore,
    tournament: Tournament,
    table_id: String,
    translations: Translations,
}

impl Poll {
    async fn run(&self, last_match: &mut Option<String>) -> anyhow::Result<()> {
        let tournament = &self.tournament;
        let table: Table = self
            .api
            .get(&format!(
                "tournaments/{}/courts/{}",
                tournament.id, self.table_id
            ))
            .await?;

        // A free table keeps its own last match on air, with its final score.
        // A match from another table or tournament goes.
        let Some(match_id) = table.current_match_id.clone() else {
            tracing::debug!("No active match");
            let own = self.store.current().await.is_some_and(|m| {
                is_kickertool_table(&m.competition, &tournament.id, &self.table_id)
            });
            if !own && self.store.scoreboard().is_some() {
                self.store.replace(None).await;
            }
            return Ok(());
        };
        if last_match.as_deref() == Some(match_id.as_str()) {
            return Ok(());
        }

        let match_data: Match = self
            .api
            .get(&format!(
                "tournaments/{}/matches/{}",
                tournament.id, match_id
            ))
            .await?;
        tracing::info!(
            "New match on the table: {} vs {}",
            match_data.team_a(),
            match_data.team_b()
        );

        // Sets are entered by hand (ADR 0012).
        let table_match =
            match_data.to_table_match(&table, tournament, &self.translations, MatchFormat::Sets {});
        self.store.replace(Some(table_match)).await;
        *last_match = Some(match_id);
        Ok(())
    }
}

/// Whether a match came from this Kickertool table.
pub fn is_kickertool_table(competition: &Competition, tournament: &str, table: &str) -> bool {
    matches!(
        competition,
        Competition::Kickertool { tournament_id, table_id, .. }
            if tournament_id == tournament && table_id == table
    )
}

/// Whether a match came from this Kickertool tournament.
pub fn is_kickertool_tournament(competition: &Competition, tournament: &str) -> bool {
    matches!(
        competition,
        Competition::Kickertool { tournament_id, .. } if tournament_id == tournament
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_origin_by_tournament_and_table() {
        let c = Competition::Kickertool {
            tournament_id: "t1".into(),
            table_id: "c1".into(),
            match_id: "m1".into(),
        };
        assert!(is_kickertool_table(&c, "t1", "c1"));
        assert!(!is_kickertool_table(&c, "t1", "c2"));
        assert!(!is_kickertool_table(&c, "t2", "c1"));
        assert!(is_kickertool_tournament(&c, "t1"));
        assert!(!is_kickertool_tournament(&c, "t2"));
        assert!(!is_kickertool_tournament(&Competition::Manual, "t1"));
    }
}
