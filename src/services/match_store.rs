//! The match on the table (ADR 0006, 0007, 0012), the one state: mutations
//! go through here, are persisted, and every change is published (SSE).

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};

use crate::competition::{
    CommandError, Event, Labels, MatchFormat, Scoreboard, Side, Slot, TableMatch,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("no match on the table")]
    NoMatch,
    #[error(transparent)]
    Command(#[from] CommandError),
}

#[derive(Clone)]
pub struct MatchStore {
    inner: Arc<Inner>,
}

struct Inner {
    day: Mutex<Option<TableMatch>>,
    tx: watch::Sender<Option<Scoreboard>>,
    path: Option<PathBuf>,
}

impl MatchStore {
    /// Restores the match day persisted at `path`, if any.
    pub fn open(path: PathBuf) -> Self {
        let day = match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str::<TableMatch>(&json)
                .inspect_err(|e| tracing::warn!("Ignoring unreadable {}: {}", path.display(), e))
                .ok(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                tracing::warn!("Failed to read {}: {}", path.display(), e);
                None
            }
        };
        Self::build(day, Some(path))
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::build(None, None)
    }

    fn build(day: Option<TableMatch>, path: Option<PathBuf>) -> Self {
        let tx = watch::Sender::new(day.as_ref().map(TableMatch::scoreboard));
        Self {
            inner: Arc::new(Inner {
                day: Mutex::new(day),
                tx,
                path,
            }),
        }
    }

    /// Yields the current scoreboard first, then every change.
    pub fn subscribe(&self) -> watch::Receiver<Option<Scoreboard>> {
        self.inner.tx.subscribe()
    }

    pub fn scoreboard(&self) -> Option<Scoreboard> {
        self.inner.tx.borrow().clone()
    }

    pub async fn current(&self) -> Option<TableMatch> {
        self.inner.day.lock().await.clone()
    }

    /// Replaces the match day. The previous log is discarded.
    pub async fn replace(&self, day: Option<TableMatch>) {
        match &day {
            Some(m) => tracing::info!(
                "On the table: {} {} – {} ({})",
                m.competition.key(),
                m.home.name,
                m.away.name,
                m.labels.title()
            ),
            None => tracing::info!("Table cleared"),
        }
        let mut guard = self.inner.day.lock().await;
        *guard = day;
        self.commit(&guard);
    }

    pub async fn apply(
        &self,
        event: Event,
        expected_revision: u64,
    ) -> Result<Scoreboard, StoreError> {
        self.mutate(|day| day.apply(event, expected_revision)).await
    }

    pub async fn set_lineup(
        &self,
        side: Side,
        slot: Slot,
        player_ids: Vec<String>,
        expected_revision: u64,
    ) -> Result<Scoreboard, StoreError> {
        self.mutate(|day| day.set_lineup(side, slot, player_ids, expected_revision))
            .await
    }

    pub async fn set_setup(
        &self,
        labels: Option<Labels>,
        home: Option<String>,
        away: Option<String>,
        expected_revision: u64,
    ) -> Result<Scoreboard, StoreError> {
        self.mutate(|day| day.set_setup(labels, home, away, expected_revision))
            .await
    }

    pub async fn set_format(
        &self,
        format: MatchFormat,
        expected_revision: u64,
    ) -> Result<Scoreboard, StoreError> {
        self.mutate(|day| day.set_format(format, expected_revision))
            .await
    }

    /// Runs a command against the current revision, for server-side inputs
    /// that hold no revision of their own (the `/scores` adapter).
    pub async fn mutate_current(
        &self,
        f: impl FnOnce(&mut TableMatch, u64) -> Result<(), CommandError>,
    ) -> Result<Scoreboard, StoreError> {
        self.mutate(|day| {
            let revision = day.revision();
            f(day, revision)
        })
        .await
    }

    async fn mutate(
        &self,
        f: impl FnOnce(&mut TableMatch) -> Result<(), CommandError>,
    ) -> Result<Scoreboard, StoreError> {
        let mut guard = self.inner.day.lock().await;
        let day = guard.as_mut().ok_or(StoreError::NoMatch)?;
        f(day)?;
        let scoreboard = day.scoreboard();
        self.commit(&guard);
        Ok(scoreboard)
    }

    /// Persists and publishes. Called with the lock held, so writes and
    /// notifications happen in mutation order.
    fn commit(&self, day: &Option<TableMatch>) {
        if let Some(path) = &self.inner.path
            && let Err(e) = persist(path, day)
        {
            tracing::error!(
                "Failed to save the match on the table to {}: {}",
                path.display(),
                e
            );
        }
        self.inner
            .tx
            .send_replace(day.as_ref().map(TableMatch::scoreboard));
    }
}

fn persist(path: &PathBuf, day: &Option<TableMatch>) -> std::io::Result<()> {
    let Some(day) = day else {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(day)?)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::competition::{Competition, Format, Labels, parse_modus, test_support::team};

    pub(crate) fn sample_day() -> TableMatch {
        day()
    }

    fn day() -> TableMatch {
        TableMatch::new(
            Labels {
                tournament: "Test".into(),
                ..Default::default()
            },
            team("h", &[("1", "A")]),
            team("a", &[("2", "B")]),
            Format::mtfv_race(),
            parse_modus("D1D1,D2D2").unwrap(),
            Competition::Manual,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn apply_without_match_day() {
        let store = MatchStore::in_memory();
        assert_eq!(store.apply(Event::Undo, 0).await, Err(StoreError::NoMatch));
    }

    #[tokio::test]
    async fn changes_are_published() {
        let store = MatchStore::in_memory();
        let mut rx = store.subscribe();
        assert!(rx.borrow_and_update().is_none());

        store.replace(Some(day())).await;
        assert!(rx.has_changed().unwrap());
        assert_eq!(rx.borrow_and_update().as_ref().unwrap().revision, 0);

        let sb = store
            .apply(Event::Goal { side: Side::Home }, 0)
            .await
            .unwrap();
        assert_eq!(sb.home.score, 1);
        assert_eq!(rx.borrow_and_update().as_ref(), Some(&sb));
    }

    #[tokio::test]
    async fn stale_revision_is_a_conflict_and_not_published() {
        let store = MatchStore::in_memory();
        store.replace(Some(day())).await;
        store
            .apply(Event::Goal { side: Side::Home }, 0)
            .await
            .unwrap();
        let mut rx = store.subscribe();
        rx.borrow_and_update();

        let err = store
            .apply(Event::Goal { side: Side::Home }, 0)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            StoreError::Command(CommandError::Conflict { current: 1, got: 0 })
        );
        assert!(!rx.has_changed().unwrap());
    }

    #[tokio::test]
    async fn persists_and_restores() {
        let dir = std::env::temp_dir().join(format!("tfc-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("table_match.json");

        let store = MatchStore::open(path.clone());
        assert!(store.scoreboard().is_none());
        store.replace(Some(day())).await;
        store
            .apply(Event::Goal { side: Side::Away }, 0)
            .await
            .unwrap();

        let restored = MatchStore::open(path.clone());
        let sb = restored.scoreboard().unwrap();
        assert_eq!((sb.revision, sb.away.score), (1, 1));

        restored.replace(None).await;
        assert!(!path.exists());
        assert!(MatchStore::open(path).scoreboard().is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
