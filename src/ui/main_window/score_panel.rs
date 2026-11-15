//! Live scores from the server UI, for every competition (ADR 0012). The
//! panel sends the same commands as the tablet. Goals are counted; sets are
//! entered by hand, because set rules (overtime in the last set) vary.

use gtk4::prelude::*;
use std::rc::Rc;

use super::{CounterRows, MainWindow, ServerContext};
use crate::competition::{
    CommandError, Competition, Event, Format, Outcome, Scoreboard, Side, TableMatch,
};

impl MainWindow {
    /// Runs a command on the match on the table and shows a refusal.
    pub(super) fn run_command(
        context: &ServerContext,
        f: impl FnOnce(&mut TableMatch, u64) -> Result<(), CommandError>,
    ) {
        let result = context
            .runtime
            .block_on(context.match_services.store.mutate_current(f));
        let message = match result {
            Ok(_) => String::new(),
            Err(e) => {
                tracing::debug!("Command refused: {e}");
                format!("Nicht möglich: {e}")
            }
        };
        context.score.error.set_text(&message);
    }

    /// Wires +1 and -1 per side of one counter.
    fn connect_counter(
        context: &Rc<ServerContext>,
        rows: &CounterRows,
        add: fn(Side) -> Event,
        sub: fn(&mut TableMatch, Side, u64) -> Result<(), CommandError>,
    ) {
        for side in [Side::Home, Side::Away] {
            let ctx = Rc::clone(context);
            rows.add_button(side).connect_clicked(move |_| {
                Self::run_command(&ctx, |m, revision| m.apply(add(side), revision));
            });
            let ctx = Rc::clone(context);
            rows.sub_button(side).connect_clicked(move |_| {
                Self::run_command(&ctx, |m, revision| sub(m, side, revision));
            });
        }
    }

    pub(super) fn connect_score_panel(context: Rc<ServerContext>) {
        let panel = &context.score;
        // -1 goal: a correction to one less, logged and undoable.
        Self::connect_counter(
            &context,
            &panel.goals,
            |side| Event::Goal { side },
            |m, side, revision| {
                let sb = m.scoreboard();
                match one_less(side, (sb.home.score, sb.away.score)) {
                    Some((home, away)) => m.apply(Event::Correct { home, away }, revision),
                    None => Ok(()),
                }
            },
        );
        // -1 set: in an MTFV game the last set is reopened; in a single
        // game the count is corrected.
        Self::connect_counter(
            &context,
            &panel.sets,
            |side| Event::SetWon { side },
            |m, side, revision| {
                if matches!(m.format, Format::Sets { .. }) {
                    return m.reopen_set(side, revision);
                }
                let sb = m.scoreboard();
                let sets = sb.home.game_sets.zip(sb.away.game_sets).unwrap_or_default();
                match one_less(side, sets) {
                    Some((home, away)) => m.apply(Event::CorrectSets { home, away }, revision),
                    None => Ok(()),
                }
            },
        );

        let ctx = Rc::clone(&context);
        panel.next_game_button.connect_clicked(move |_| {
            Self::run_command(&ctx, |m, revision| match m.next_game() {
                Some(unit) => m.apply(Event::Select { unit }, revision),
                None => Ok(()),
            });
        });

        let ctx = Rc::clone(&context);
        panel
            .players_button
            .connect_clicked(move |_| Self::put_players_on_table(&ctx));
    }

    /// The players typed in for the current game go to the table.
    pub(super) fn put_players_on_table(ctx: &Rc<ServerContext>) {
        let names = |entry: &gtk4::Entry| -> Vec<String> {
            entry
                .text()
                .split('/')
                .map(|n| n.trim().to_owned())
                .collect()
        };
        let home = names(&ctx.score.home_players);
        let away = names(&ctx.score.away_players);
        Self::run_command(ctx, |m, revision| {
            // A Frei Einzel or Doppel is known by its players: they are
            // also the name of the side.
            let frei_players =
                m.competition == Competition::Manual && !matches!(m.format, Format::Race { .. });
            let names = |players: &[String]| {
                players
                    .iter()
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join(" / ")
            };
            let (home_name, away_name) = (names(&home), names(&away));
            let slot = m.current_slot(Side::Home);
            m.set_lineup(Side::Home, slot, home, revision)?;
            let slot = m.current_slot(Side::Away);
            m.set_lineup(Side::Away, slot, away, m.revision())?;
            if frei_players {
                m.set_setup(None, Some(home_name), Some(away_name), m.revision())?;
            }
            Ok(())
        });
    }

    fn set_counter(rows: &CounterRows, values: Option<(u16, u16)>, enabled: bool) {
        for widget in &rows.widgets {
            widget.set_sensitive(enabled);
        }
        let text = |v: Option<u16>| v.map_or_else(|| "–".to_owned(), |v| v.to_string());
        rows.home_value.set_text(&text(values.map(|v| v.0)));
        rows.away_value.set_text(&text(values.map(|v| v.1)));
        // Nothing to take back at 0.
        let (home, away) = values.unwrap_or_default();
        rows.home_sub.set_sensitive(enabled && home > 0);
        rows.away_sub.set_sensitive(enabled && away > 0);
    }

    /// Shows the match on the table. Typed players and corrections are only
    /// overwritten when the game changes or the field is not being edited.
    pub(super) fn update_score_panel(context: &ServerContext, scoreboard: Option<&Scoreboard>) {
        let panel = &context.score;

        let Some(sb) = scoreboard else {
            panel.title.set_text("Kein Spiel auf dem Tisch");
            panel.home_team.set_text("Heim");
            panel.away_team.set_text("Gast");
            Self::set_counter(&panel.goals, None, false);
            Self::set_counter(&panel.sets, None, false);
            panel.next_game_button.set_visible(false);
            panel.players_button.set_sensitive(false);
            return;
        };

        let competition = match sb.competition {
            "kickertool" => "Kickertool",
            "mtfv" => "MTFV",
            _ => "Frei",
        };
        let labels = [
            sb.title.as_str(),
            sb.labels.discipline.as_str(),
            sb.labels.group.as_str(),
            sb.labels.table.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
        // A race shows its segment; an MTFV Begegnung in sets its game.
        let (format, game) = match sb.format {
            Format::Race { target, .. } => (
                Some(format!("Race to {target}")),
                sb.segment.map(|(number, count)| {
                    let segment = format!("Segment {number}/{count}");
                    if sb.unit.games > 1 {
                        format!("{segment} · {}", context.translations().game(sb.home.slot))
                    } else {
                        segment
                    }
                }),
            ),
            Format::Sets { .. } => (
                Some(format!(
                    "{} (Spiel {}/{}) · Satz {}/{}",
                    context.translations().game(sb.home.slot),
                    sb.unit.game,
                    sb.unit.games,
                    sb.unit.set_in_game.unwrap_or(1),
                    sb.unit.sets_in_game.unwrap_or(1)
                )),
                Some(if sb.unit.game_over {
                    format!(
                        "Spiel beendet · Sätze gesamt {}:{}",
                        sb.home.match_points, sb.away.match_points
                    )
                } else {
                    format!(
                        "Sätze gesamt {}:{}",
                        sb.home.match_points, sb.away.match_points
                    )
                }),
            ),
            Format::ManualSets => (None, None),
        };
        let outcome = match sb.outcome {
            Some(Outcome::Home) => Some(format!("Beendet, Sieg {}", sb.home.team)),
            Some(Outcome::Away) => Some(format!("Beendet, Sieg {}", sb.away.team)),
            Some(Outcome::Draw) => Some("Beendet, unentschieden".to_owned()),
            None => None,
        };
        let details = [format, game, outcome]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        let side = |side: &crate::competition::SideView, fallback: &str| {
            let players = side
                .players
                .iter()
                .map(|p| p.display_name())
                .collect::<Vec<_>>()
                .join(" / ");
            match (side.team.is_empty(), players.is_empty()) {
                (false, false) if side.team != players => format!("{} ({players})", side.team),
                (false, _) => side.team.clone(),
                (true, false) => players,
                (true, true) => fallback.to_owned(),
            }
        };
        let sides = format!("{} – {}", side(&sb.home, "Heim"), side(&sb.away, "Gast"));
        panel
            .title
            .set_text(&format!("{competition}: {labels}\n{sides}\n{details}"));

        let name = |team: &str, fallback: &str| {
            if team.is_empty() {
                fallback.to_owned()
            } else {
                team.to_owned()
            }
        };
        panel.home_team.set_text(&name(&sb.home.team, "Heim"));
        panel.away_team.set_text(&name(&sb.away.team, "Gast"));
        // A Frei Einzel or Doppel has no teams: its players are the sides.
        let teams = !(sb.competition == "manual" && !matches!(sb.format, Format::Race { .. }));
        panel.home_team.set_visible(teams);
        panel.away_team.set_visible(teams);
        let placeholder = if sb.unit.singles {
            "Name, Vorname"
        } else {
            "Name / Name"
        };
        panel.home_players.set_placeholder_text(Some(placeholder));
        panel.away_players.set_placeholder_text(Some(placeholder));

        // A finished MTFV game waits for "Nächstes Spiel".
        let running = sb.outcome.is_none() && !sb.unit.game_over;
        let sets_league = matches!(sb.format, Format::Sets { .. });
        Self::set_counter(&panel.goals, Some((sb.home.score, sb.away.score)), running);
        // Sets are entered by hand; a race has none.
        let sets = sb.home.game_sets.zip(sb.away.game_sets);
        let has_sets = !matches!(sb.format, Format::Race { .. });
        Self::set_counter(&panel.sets, sets, running && has_sets);
        panel.next_game_button.set_visible(sets_league);
        panel
            .next_game_button
            .set_sensitive(sets_league && sb.unit.game_over && sb.unit.game < sb.unit.games);
        panel.players_button.set_sensitive(true);

        // A new game (another match, or the next game of a Begegnung) shows
        // its players; within a game, typing is left alone.
        let game = format!(
            "{}|{}|{}|{}|{}",
            sb.competition, sb.title, sb.home.team, sb.away.team, sb.unit.index
        );
        let new_game = *panel.shown_game.borrow() != game;
        if new_game {
            *panel.shown_game.borrow_mut() = game;
        }
        let mut shown_players = panel.shown_players.borrow_mut();
        for (i, (entry, players)) in [
            (&panel.home_players, &sb.home.players),
            (&panel.away_players, &sb.away.players),
        ]
        .into_iter()
        .enumerate()
        {
            let names: Vec<_> = players.iter().map(|p| p.display_name()).collect();
            let names = names.join(" / ");
            if new_game || entry.text() == shown_players[i] {
                entry.set_text(&names);
            }
            shown_players[i] = names;
        }
    }
}

fn one_less(side: Side, (home, away): (u16, u16)) -> Option<(u16, u16)> {
    match side {
        Side::Home if home > 0 => Some((home - 1, away)),
        Side::Away if away > 0 => Some((home, away - 1)),
        _ => None,
    }
}
