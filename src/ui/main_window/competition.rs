//! What is played (ADR 0012): the competition buttons and the setup panels
//! that put a match on the table.

use gtk4::Stack;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use super::{
    CompetitionButtons, KickertoolControls, MainWindow, MtfvControls, RaceControls, ServerContext,
};
use crate::competition::{
    Competition, Format, Labels, MatchFormat, SlotKind, TableMatch, Team, kinds_to_letters,
    race_units,
};
use crate::mtfv::{FormatRules, SeasonLeague, by_spieltag};

/// The id of a separator row in the Begegnung list.
const SEPARATOR: &str = "-";
use crate::services::{CompetitionKind, FreiKind, overlay_key};

impl MainWindow {
    pub(super) fn connect_competition_buttons(
        buttons: &CompetitionButtons,
        stack: &Stack,
        kickertool: &KickertoolControls,
        mtfv: &MtfvControls,
        context: Rc<ServerContext>,
    ) {
        for (button, kind) in [
            (&buttons.kickertool, CompetitionKind::Kickertool),
            (&buttons.mtfv, CompetitionKind::Mtfv),
            (&buttons.manual, CompetitionKind::Manual),
        ] {
            let stack = stack.clone();
            let refresh = kickertool.refresh_button.clone();
            let reload_season = mtfv.reload_button.clone();
            let context = Rc::clone(&context);
            button.connect_toggled(move |button| {
                if !button.is_active() {
                    return;
                }
                stack.set_visible_child_name(kind.key());
                Self::select_competition(&context, kind);
                match kind {
                    CompetitionKind::Kickertool => refresh.emit_clicked(),
                    CompetitionKind::Mtfv => reload_season.emit_clicked(),
                    CompetitionKind::Manual => {}
                }
            });
        }
    }

    /// The competition is a setting. Choosing one does not replace the match
    /// on the table; its setup panel does.
    fn select_competition(context: &ServerContext, kind: CompetitionKind) {
        tracing::info!("Competition: {}", kind.key());
        if kind != CompetitionKind::Kickertool {
            context
                .runtime
                .block_on(context.table_monitor.stop_monitoring());
        }
        context
            .runtime
            .block_on(async { context.settings.write().await.competition = Some(kind) });
        Self::save_settings(context);
        Self::show_overlays(context);
    }

    pub(super) fn restore_competition(buttons: &CompetitionButtons, context: &ServerContext) {
        let kind = context
            .runtime
            .block_on(async { context.settings.read().await.competition });
        let button = match kind {
            Some(CompetitionKind::Kickertool) => &buttons.kickertool,
            Some(CompetitionKind::Mtfv) => &buttons.mtfv,
            Some(CompetitionKind::Manual) => &buttons.manual,
            None => return,
        };
        button.set_active(true);
    }

    /// Keeps the score panel and the overlay selection on the match on the
    /// table, whoever changed it. tokio's watch channel needs no tokio
    /// runtime, so this runs on the GTK main loop.
    pub(super) fn follow_match_on_the_table(context: Rc<ServerContext>) {
        let mut rx = context.match_services.store.subscribe();
        glib::spawn_future_local(async move {
            // The selection key, not only the competition: an MTFV race and
            // an MTFV set league need different templates.
            let mut key = None;
            loop {
                let scoreboard = rx.borrow_and_update().clone();
                Self::update_score_panel(&context, scoreboard.as_ref());
                let now = scoreboard.as_ref().map(overlay_key);
                if now != key {
                    key = now;
                    Self::show_overlays(&context);
                } else {
                    // A new Kickertool match starts in sets.
                    Self::apply_template_format(&context);
                }
                if rx.changed().await.is_err() {
                    break;
                }
            }
        });
    }

    fn frei_labels(context: &ServerContext) -> Labels {
        let f = &context.frei;
        Labels {
            tournament: f.tournament.text().to_string(),
            round: f.round.text().to_string(),
            group: f.group.text().to_string(),
            discipline: f.discipline.text().to_string(),
            table: String::new(),
        }
    }

    fn frei_kind(context: &ServerContext) -> FreiKind {
        let f = &context.frei;
        if f.singles.is_active() {
            FreiKind::Singles
        } else if f.race.is_active() {
            FreiKind::Race
        } else {
            FreiKind::Doubles
        }
    }

    /// Team names for a race; Einzel and Doppel have no fields of their own
    /// here, their players are entered with the score.
    fn show_frei_kind(context: &ServerContext, kind: FreiKind) {
        let f = &context.frei;
        let race = kind == FreiKind::Race;
        for widget in [
            f.team_label.upcast_ref::<gtk4::Widget>(),
            f.team_a.upcast_ref(),
            f.team_b.upcast_ref(),
        ] {
            widget.set_visible(race);
        }
        show_race(&f.race_settings, race);
    }

    /// The team names of a race; Einzel and Doppel get theirs from the
    /// players, entered with the score.
    fn frei_teams(context: &ServerContext, kind: FreiKind) -> Option<(String, String)> {
        let f = &context.frei;
        (kind == FreiKind::Race).then(|| {
            (
                f.team_a.text().trim().to_owned(),
                f.team_b.text().trim().to_owned(),
            )
        })
    }

    /// A new Frei match of the chosen kind, from the form.
    fn frei_match(context: &ServerContext, kind: FreiKind) -> TableMatch {
        let format = match kind {
            FreiKind::Race => {
                let (target, segment) = context.race_settings();
                MatchFormat::Race { target, segment }
            }
            FreiKind::Singles | FreiKind::Doubles => MatchFormat::Sets {},
        };
        let (home, away) = Self::frei_teams(context, kind).unwrap_or_default();
        if kind == FreiKind::Race {
            // The games chosen for the day, one per segment (D1 D2 E1 …).
            let kinds = context
                .runtime
                .block_on(async { context.settings.read().await.race_game_kinds() });
            if let Ok(table_match) = TableMatch::new(
                Self::frei_labels(context),
                Team::named(home.clone()),
                Team::named(away.clone()),
                format.format(),
                race_units(&kinds),
                Competition::Manual,
            ) {
                return table_match;
            }
        }
        TableMatch::single_game(
            Competition::Manual,
            Self::frei_labels(context),
            Team::named(home),
            Team::named(away),
            kind == FreiKind::Singles,
            format,
        )
    }

    /// The kind of the Frei match on the table; `None` for another
    /// competition.
    fn frei_kind_on_table(context: &ServerContext) -> Option<FreiKind> {
        let sb = context.match_services.store.scoreboard()?;
        let kind = match sb.format {
            Format::Race { .. } => FreiKind::Race,
            _ if sb.unit.singles => FreiKind::Singles,
            _ => FreiKind::Doubles,
        };
        (sb.competition == "manual").then_some(kind)
    }

    /// "Neustart" puts a new Frei match on the table. "Auf den Tisch" does
    /// the same unless a Frei match of the same kind is there: then only its
    /// names change and the score stays.
    pub(super) fn connect_frei_panel(context: Rc<ServerContext>) {
        let f = &context.frei;
        for button in [&f.singles, &f.doubles, &f.race] {
            let ctx = Rc::clone(&context);
            button.connect_toggled(move |button| {
                if !button.is_active() {
                    return;
                }
                let kind = Self::frei_kind(&ctx);
                Self::show_frei_kind(&ctx, kind);
                ctx.runtime
                    .block_on(async { ctx.settings.write().await.frei_kind = kind });
                Self::save_settings(&ctx);
                Self::show_overlays(&ctx);
            });
        }

        let ctx = Rc::clone(&context);
        context.frei.new_button.connect_clicked(move |_| {
            let table_match = Self::frei_match(&ctx, Self::frei_kind(&ctx));
            ctx.runtime
                .block_on(ctx.match_services.store.replace(Some(table_match)));
        });

        let ctx = Rc::clone(&context);
        context.frei.apply_button.connect_clicked(move |_| {
            let kind = Self::frei_kind(&ctx);
            if Self::frei_kind_on_table(&ctx) != Some(kind) {
                ctx.frei.new_button.emit_clicked();
            } else {
                let labels = Self::frei_labels(&ctx);
                let (home, away) = Self::frei_teams(&ctx, kind).unzip();
                Self::run_command(&ctx, |m, revision| {
                    m.set_setup(Some(labels), home, away, revision)
                });
            }
            // Everything on the form goes to the table: also the players
            // typed in below.
            Self::put_players_on_table(&ctx);
        });

        let kind = context
            .runtime
            .block_on(async { context.settings.read().await.frei_kind });
        let button = match kind {
            FreiKind::Singles => &f.singles,
            FreiKind::Doubles => &f.doubles,
            FreiKind::Race => &f.race,
        };
        button.set_active(true);
        Self::connect_race_controls(&context, &context.frei.race_settings);
        Self::refresh_race_controls(&context);
        Self::show_frei_kind(&context, kind);
    }

    /// The leagues of the mtfv.de season overview. The chosen league shows
    /// its format; one of its Begegnungen, played or not, is put on the
    /// table (ADR 0012). The overview switches to a new season on its own.
    pub(super) fn connect_mtfv_panel(mtfv: &MtfvControls, context: Rc<ServerContext>) {
        // Rows with the separator id are drawn as a line between Spieltage.
        mtfv.fixture_combo.set_row_separator_func(|model, iter| {
            model
                .get_value(iter, 1)
                .get::<Option<String>>()
                .ok()
                .flatten()
                .is_some_and(|id| id == SEPARATOR)
        });
        let season: Rc<RefCell<Vec<SeasonLeague>>> = Rc::default();

        Self::connect_race_controls(&context, &mtfv.race);

        // Reload the season overview.
        let league_combo = mtfv.league_combo.clone();
        let ctx = Rc::clone(&context);
        let leagues = Rc::clone(&season);
        mtfv.reload_button.connect_clicked(move |_| {
            let client = std::sync::Arc::clone(&ctx.match_services.mtfv);
            let combo = league_combo.clone();
            let leagues = Rc::clone(&leagues);
            let ctx2 = Rc::clone(&ctx);
            Self::run_async_into_ui(
                &ctx.runtime,
                async move { client.season().await },
                move |result| match result {
                    Ok(loaded) => {
                        let saved = ctx2
                            .runtime
                            .block_on(async { ctx2.settings.read().await.mtfv_league });
                        *leagues.borrow_mut() = loaded;
                        combo.remove_all();
                        for league in leagues.borrow().iter() {
                            combo.append(Some(&league.id().to_string()), league.name());
                        }
                        let saved = saved.map(|id| id.to_string());
                        if !saved
                            .as_deref()
                            .is_some_and(|id| combo.set_active_id(Some(id)))
                        {
                            combo.set_active(Some(0));
                        }
                        ctx2.score.error.set_text("");
                    }
                    Err(e) => {
                        tracing::warn!("MTFV season overview not loaded: {e:#}");
                        ctx2.score.error.set_text(&format!("mtfv.de: {e:#}"));
                    }
                },
            );
        });

        // A league shows its format and its Begegnungen.
        let ctx = Rc::clone(&context);
        let leagues = Rc::clone(&season);
        let (games_label, race) = (mtfv.games_label.clone(), mtfv.race.clone());
        let (fixture_combo, load_button) = (mtfv.fixture_combo.clone(), mtfv.load_button.clone());
        mtfv.league_combo.connect_changed(move |combo| {
            fixture_combo.remove_all();
            let leagues = leagues.borrow();
            let Some(league) = combo
                .active_id()
                .and_then(|id| leagues.iter().find(|l| l.id().to_string() == id.as_str()))
            else {
                games_label.set_visible(false);
                show_race(&race, false);
                load_button.set_sensitive(false);
                return;
            };
            ctx.runtime
                .block_on(async { ctx.settings.write().await.mtfv_league = Some(league.id()) });
            Self::save_settings(&ctx);

            // A race league shows the race settings for the day and its
            // games from mtfv.de (read only); a set league its games.
            show_race(&race, league.is_race());
            games_label.set_visible(!league.is_race());
            games_label.set_text(&games_text(&league.games()));
            clear(&race.games);
            for (game, _) in league.games() {
                let button = gtk4::ToggleButton::with_label(&game);
                button.set_active(game.starts_with('E'));
                button.set_sensitive(false);
                button.set_tooltip_text(Some("Von mtfv.de"));
                race.games.append(&button);
            }

            // Spieltage to come first, a line between Spieltage; the first
            // entry (the next Begegnung) is preselected.
            let today = chrono::Local::now().date_naive();
            let groups = by_spieltag(league.fixtures(), today);
            for (i, group) in groups.iter().enumerate() {
                if i > 0 {
                    fixture_combo.append(Some(SEPARATOR), "");
                }
                for f in group {
                    let id = format!("{}:{}", f.veranstaltung_id, f.begegnung_id);
                    let text = format!(
                        "{} · {} · {} – {}",
                        f.spieltag,
                        f.time.format("%d.%m. %H:%M"),
                        f.home,
                        f.away
                    );
                    fixture_combo.append(Some(&id), &text);
                }
            }
            fixture_combo.set_active((!groups.is_empty()).then_some(0));
            load_button.set_sensitive(!groups.is_empty());
        });

        // Put the chosen Begegnung on the table, built from the overview.
        let combo = mtfv.fixture_combo.clone();
        mtfv.load_button.connect_clicked(move |_| {
            let Some((veranstaltung_id, begegnung_id)) = combo.active_id().and_then(|id| {
                let (v, b) = id.split_once(':')?;
                Some((v.parse::<u32>().ok()?, b.parse::<u32>().ok()?))
            }) else {
                return;
            };
            let leagues = season.borrow();
            let Some(league) = leagues.iter().find(|l| l.id() == veranstaltung_id) else {
                return;
            };
            let (target, segment) = context.race_settings();
            match league.table_match(begegnung_id, &FormatRules::race_to(target, segment)) {
                Ok(table_match) => {
                    context
                        .runtime
                        .block_on(context.match_services.store.replace(Some(table_match)));
                    context.score.error.set_text("");
                }
                Err(e) => {
                    tracing::error!("MTFV Begegnung {begegnung_id} not loaded: {e}");
                    context
                        .score
                        .error
                        .set_text(&format!("Begegnung nicht geladen: {e}"));
                }
            }
        });
    }

    /// The race settings of both panels: one setting for the day, changed
    /// in either. A change applies to the race on the table too.
    fn connect_race_controls(context: &Rc<ServerContext>, race: &RaceControls) {
        for spin in [&race.target, &race.segment] {
            let ctx = Rc::clone(context);
            let race = race.clone();
            spin.connect_value_changed(move |_| {
                if REFRESHING.get() {
                    return;
                }
                let target = race.target.value() as u16;
                let segment = race.segment.value() as u16;
                ctx.runtime.block_on(async {
                    let mut settings = ctx.settings.write().await;
                    settings.race_target = target;
                    settings.race_segment = segment;
                });
                Self::save_settings(&ctx);
                Self::refresh_race_controls(&ctx);
                let race_on_table = ctx.match_services.store.scoreboard().is_some_and(|s| {
                    matches!(s.format, Format::Race { .. }) && s.competition != "kickertool"
                });
                if race_on_table {
                    Self::run_command(&ctx, |m, revision| m.set_race(target, segment, revision));
                }
                // A Kickertool race under a race template.
                Self::apply_template_format(&ctx);
            });
        }
    }

    /// Both panels show the race settings for the day; Frei also its games.
    pub(super) fn refresh_race_controls(context: &Rc<ServerContext>) {
        let (target, segment) = context.race_settings();
        REFRESHING.set(true);
        for race in [&context.frei.race_settings, &context.mtfv_race] {
            race.target.set_value(f64::from(target));
            race.segment.set_value(f64::from(segment));
            race.segments_label
                .set_text(&segments_text(target, segment));
        }
        REFRESHING.set(false);
        Self::fill_frei_games(context);
    }

    /// One button per segment of a Frei race, numbered in order (D1 D2 E1
    /// …); a click switches between Doppel and Einzel.
    fn fill_frei_games(context: &Rc<ServerContext>) {
        let race = &context.frei.race_settings;
        clear(&race.games);
        let kinds = context
            .runtime
            .block_on(async { context.settings.read().await.race_game_kinds() });
        for (i, unit) in race_units(&kinds).iter().enumerate() {
            // Einzel is pressed in, Doppel out.
            let button = gtk4::ToggleButton::with_label(&unit.home.to_string());
            button.set_active(kinds[i] == SlotKind::Singles);
            button.set_tooltip_text(Some("Gedrückt: Einzel, sonst Doppel"));
            let ctx = Rc::clone(context);
            let kinds = kinds.clone();
            button.connect_toggled(move |_| {
                let mut kinds = kinds.clone();
                kinds[i] = match kinds[i] {
                    SlotKind::Doubles => SlotKind::Singles,
                    SlotKind::Singles => SlotKind::Doubles,
                };
                let letters = kinds_to_letters(&kinds);
                ctx.runtime
                    .block_on(async { ctx.settings.write().await.race_games = letters });
                Self::save_settings(&ctx);
                Self::fill_frei_games(&ctx);
            });
            race.games.append(&button);
        }
    }
}

/// "→ 7 Segmente" for a race of 42 in sixes.
fn segments_text(target: u16, segment: u16) -> String {
    if segment == 0 {
        String::new()
    } else {
        format!("→ {} Segmente", target.div_ceil(segment))
    }
}

/// "5 Spiele à 2 Sätze: D1 E1 D2 E2 D3".
fn games_text(games: &[(String, usize)]) -> String {
    let names: Vec<_> = games.iter().map(|(name, _)| name.as_str()).collect();
    let sets = games.iter().map(|(_, sets)| *sets).max().unwrap_or(1);
    format!(
        "{} Spiele à {} Sätze: {}",
        games.len(),
        sets,
        names.join(" ")
    )
}

impl ServerContext {
    /// Names for what is shown, from `translations.toml` (read each time,
    /// so edits apply without a restart).
    pub(super) fn translations(&self) -> crate::models::Translations {
        let path = self.settings_service.translations_path();
        crate::models::Translations::load(&path)
            .unwrap_or_else(|_| crate::models::Translations::bundled())
    }

    /// Race target and goals per segment from the settings.
    pub(super) fn race_settings(&self) -> (u16, u16) {
        self.runtime.block_on(async {
            let settings = self.settings.read().await;
            (settings.race_target, settings.race_segment)
        })
    }
}

thread_local! {
    /// Set while the race spins are filled from the settings.
    static REFRESHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn clear(container: &gtk4::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn show_race(race: &RaceControls, visible: bool) {
    race.settings_row.set_visible(visible);
    race.games_label.set_visible(visible);
    race.games.set_visible(visible);
}
