//! The overlay feed (ADR 0011, 0012): the view of the match on the table, and
//! the template `/` serves.

use std::sync::{Arc, Weak};
use tokio::sync::watch;

use super::MatchStore;
use crate::competition::Scoreboard;
use crate::models::{OverlayView, Translations};
use std::path::PathBuf;

#[derive(Clone)]
pub struct OverlayFeed {
    inner: Arc<Inner>,
}

struct Inner {
    store: MatchStore,
    view: watch::Sender<Option<OverlayView>>,
    selected_template: watch::Sender<Option<String>>,
    /// Fingerprint of the template files; a change reloads open pages.
    templates: watch::Sender<u64>,
}

impl OverlayFeed {
    /// Must be called inside a tokio runtime: it follows the match store.
    /// Names (e.g. of MTFV games) come from `translations.toml`, read on
    /// every update so edits apply without a restart.
    pub fn new(store: MatchStore, translations: PathBuf) -> Self {
        let mut store_rx = store.subscribe();
        let view_of = move |scoreboard: Option<&Scoreboard>| {
            let translations = Translations::load(&translations).unwrap_or_else(|e| {
                tracing::warn!("Ignoring {}: {e:#}", translations.display());
                Translations::bundled()
            });
            scoreboard.map(|s| OverlayView::from_scoreboard(s, &translations))
        };
        let initial = view_of(store_rx.borrow_and_update().as_ref());
        let inner = Arc::new(Inner {
            store,
            view: watch::channel(initial).0,
            selected_template: watch::channel(None).0,
            templates: watch::channel(0).0,
        });

        let weak: Weak<Inner> = Arc::downgrade(&inner);
        tokio::spawn(async move {
            while store_rx.changed().await.is_ok() {
                let Some(inner) = weak.upgrade() else { break };
                let view = view_of(store_rx.borrow_and_update().as_ref());
                // A lineup change for a later game leaves the view as it is.
                inner.view.send_if_modified(|current| {
                    let changed = *current != view;
                    *current = view;
                    changed
                });
            }
        });

        Self { inner }
    }

    pub fn store(&self) -> &MatchStore {
        &self.inner.store
    }

    pub fn subscribe(&self) -> watch::Receiver<Option<OverlayView>> {
        self.inner.view.subscribe()
    }

    /// The competition of the match on the table, if there is one.
    #[cfg(test)]
    pub fn competition(&self) -> Option<&'static str> {
        self.inner.store.scoreboard().map(|s| s.competition)
    }

    /// The overlay selection key of the match on the table: its competition,
    /// and for MTFV also its format (`mtfv:race`, `mtfv:sets`), which the
    /// league decides. For Kickertool and Frei the template decides the
    /// format instead.
    pub fn overlay_key(&self) -> Option<String> {
        self.inner.store.scoreboard().map(|s| overlay_key(&s))
    }

    /// The template `/` serves. A change tells open `/` pages to reload.
    pub fn set_selected_template(&self, id: Option<String>) {
        self.inner.selected_template.send_if_modified(|current| {
            let changed = *current != id;
            *current = id;
            changed
        });
    }

    pub fn subscribe_selected_template(&self) -> watch::Receiver<Option<String>> {
        self.inner.selected_template.subscribe()
    }

    /// Records the template files' fingerprint. The first one only sets it;
    /// a later change tells every open page to reload.
    pub fn set_templates_fingerprint(&self, fingerprint: u64) {
        self.inner.templates.send_if_modified(|current| {
            let first = *current == 0;
            let changed = *current != fingerprint;
            *current = fingerprint;
            changed && !first
        });
    }

    pub fn subscribe_templates(&self) -> watch::Receiver<u64> {
        self.inner.templates.subscribe()
    }
}

/// The overlay selection key of a match: its competition with what decides
/// the template. For MTFV that is the league's format (`mtfv:race`,
/// `mtfv:sets`); for Kickertool the players per side
/// (`kickertool:singles`), as its format follows the template; for Frei the
/// kind chosen (`manual:singles`, `manual:doubles`, `manual:race`).
pub fn overlay_key(scoreboard: &crate::competition::Scoreboard) -> String {
    use crate::competition::Format;
    let players = if scoreboard.unit.singles {
        "singles"
    } else {
        "doubles"
    };
    match (scoreboard.competition, scoreboard.format) {
        ("mtfv", Format::Race { .. }) => "mtfv:race".to_owned(),
        ("mtfv", _) => "mtfv:sets".to_owned(),
        ("manual", Format::Race { .. }) => "manual:race".to_owned(),
        (competition, _) => format!("{competition}:{players}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::competition::{Event, Side};
    use crate::services::match_store::tests::sample_day;

    #[tokio::test]
    async fn the_view_follows_the_match_on_the_table() {
        let store = MatchStore::in_memory();
        let feed = OverlayFeed::new(store.clone(), "/nonexistent".into());
        let mut rx = feed.subscribe();
        assert!(rx.borrow_and_update().is_none());
        assert_eq!(feed.competition(), None);

        store.replace(Some(sample_day())).await;
        rx.wait_for(|v| v.is_some()).await.unwrap();
        assert_eq!(feed.competition(), Some("manual"));

        store
            .apply(Event::Goal { side: Side::Home }, 0)
            .await
            .unwrap();
        rx.wait_for(|v| v.as_ref().is_some_and(|v| v.fields["scoreA"] == "1"))
            .await
            .unwrap();

        store.replace(None).await;
        rx.wait_for(|v| v.is_none()).await.unwrap();
    }

    #[tokio::test]
    async fn starts_with_a_restored_match() {
        let store = MatchStore::in_memory();
        store.replace(Some(sample_day())).await;
        let feed = OverlayFeed::new(store, "/nonexistent".into());
        assert!(feed.subscribe().borrow().is_some());
    }

    #[tokio::test]
    async fn template_edits_notify_after_the_first_fingerprint() {
        let feed = OverlayFeed::new(MatchStore::in_memory(), "/nonexistent".into());
        let rx = feed.subscribe_templates();
        feed.set_templates_fingerprint(7);
        assert!(!rx.has_changed().unwrap(), "the first one is the baseline");
        feed.set_templates_fingerprint(7);
        assert!(!rx.has_changed().unwrap());
        feed.set_templates_fingerprint(8);
        assert!(rx.has_changed().unwrap());
    }

    #[test]
    fn the_key_names_what_decides_the_template() {
        use crate::competition::{Competition, Labels, MatchFormat, TableMatch, Team};
        let single = |competition, singles| {
            overlay_key(
                &TableMatch::single_game(
                    competition,
                    Labels::default(),
                    Team::named("A"),
                    Team::named("B"),
                    singles,
                    MatchFormat::Sets {},
                )
                .scoreboard(),
            )
        };
        let kickertool = || Competition::Kickertool {
            tournament_id: "t".into(),
            table_id: "c".into(),
            match_id: "m".into(),
        };
        assert_eq!(single(kickertool(), true), "kickertool:singles");
        assert_eq!(single(kickertool(), false), "kickertool:doubles");
        assert_eq!(single(Competition::Manual, true), "manual:singles");
        assert_eq!(
            overlay_key(&sample_day().scoreboard()),
            "manual:race",
            "a Frei race is a race, whoever plays"
        );
    }

    #[tokio::test]
    async fn selected_template_changes_only_on_a_new_id() {
        let feed = OverlayFeed::new(MatchStore::in_memory(), "/nonexistent".into());
        let mut rx = feed.subscribe_selected_template();
        feed.set_selected_template(Some("a".into()));
        assert!(rx.has_changed().unwrap());
        rx.borrow_and_update();
        feed.set_selected_template(Some("a".into()));
        assert!(!rx.has_changed().unwrap());
    }
}
