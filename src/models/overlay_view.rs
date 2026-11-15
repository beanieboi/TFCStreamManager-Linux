//! What is on air (ADR 0011): flat named fields that templates bind as
//! `{{placeholders}}`, pushed on `/api/overlay`. Derived from the match on
//! the table alone (ADR 0012).

use serde::Serialize;
use std::collections::BTreeMap;

use super::Translations;
use crate::competition::{Format, Outcome, Scoreboard, SideView};

/// Every field a view carries. Templates can rely on all of them being
/// present; a missing value is `""`.
pub const FIELDS: [&str; 32] = [
    "tournamentName",
    "table",
    "roundName",
    "groupName",
    "disciplineName",
    "state",
    "started",
    "teamA",
    "teamB",
    "teamAPlayer",
    "teamBPlayer",
    "teamAPlayer1",
    "teamAPlayer2",
    "teamBPlayer1",
    "teamBPlayer2",
    "scoreA",
    "scoreB",
    "setsA",
    "setsB",
    "slotA",
    "slotB",
    "gameName",
    "matchPointsA",
    "matchPointsB",
    "unitNumber",
    "unitCount",
    "raceTarget",
    "segmentNumber",
    "segmentCount",
    "setNumber",
    "setsInGame",
    "winner",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverlayView {
    /// The competition: `kickertool`, `mtfv` or `manual`.
    pub source: &'static str,
    /// `race` or `sets`.
    pub format: Option<&'static str>,
    pub fields: BTreeMap<&'static str, String>,
}

impl OverlayView {
    fn new(source: &'static str, format: Option<&'static str>) -> Self {
        Self {
            source,
            format,
            fields: FIELDS.iter().map(|&name| (name, String::new())).collect(),
        }
    }

    fn set(&mut self, name: &'static str, value: impl ToString) {
        debug_assert!(FIELDS.contains(&name), "unknown overlay field {name}");
        self.fields.insert(name, value.to_string());
    }

    pub fn from_scoreboard(scoreboard: &Scoreboard, translations: &Translations) -> Self {
        let format = match scoreboard.format {
            Format::Race { .. } => "race",
            Format::Sets { .. } | Format::ManualSets => "sets",
        };
        let mut view = Self::new(scoreboard.competition, Some(format));
        let labels = &scoreboard.labels;
        view.set("tournamentName", &labels.tournament);
        view.set("roundName", &labels.round);
        view.set("groupName", &labels.group);
        view.set("disciplineName", &labels.discipline);
        view.set("table", &labels.table);
        view.set(
            "state",
            if scoreboard.outcome.is_some() {
                "finished"
            } else {
                "running"
            },
        );
        view.set_side(
            &scoreboard.home,
            [
                "teamA",
                "teamAPlayer",
                "teamAPlayer1",
                "teamAPlayer2",
                "scoreA",
            ],
        );
        view.set_side(
            &scoreboard.away,
            [
                "teamB",
                "teamBPlayer",
                "teamBPlayer1",
                "teamBPlayer2",
                "scoreB",
            ],
        );
        view.set_optional("setsA", scoreboard.home.game_sets);
        view.set_optional("setsB", scoreboard.away.game_sets);
        // Slots name the games of an MTFV Begegnung; a single game has none.
        // A match of several games (MTFV, a Frei race) names the current one.
        if scoreboard.unit.games > 1 {
            view.set("slotA", scoreboard.home.slot);
            view.set("slotB", scoreboard.away.slot);
            view.set("gameName", translations.game(scoreboard.home.slot));
        }
        view.set("matchPointsA", scoreboard.home.match_points);
        view.set("matchPointsB", scoreboard.away.match_points);
        view.set("unitNumber", scoreboard.unit.index + 1);
        view.set("unitCount", scoreboard.unit.count);
        if let Format::Race { target, .. } = scoreboard.format {
            view.set("raceTarget", target);
        }
        if let Some((number, count)) = scoreboard.segment {
            view.set("segmentNumber", number);
            view.set("segmentCount", count);
        }
        view.set_optional("setNumber", scoreboard.unit.set_in_game);
        view.set_optional("setsInGame", scoreboard.unit.sets_in_game);
        match scoreboard.outcome {
            Some(Outcome::Home) => view.set("winner", &scoreboard.home.team),
            Some(Outcome::Away) => view.set("winner", &scoreboard.away.team),
            Some(Outcome::Draw) | None => {}
        }
        view
    }

    /// `players` holds all names joined; `first` and `second` one each, for
    /// templates that show doubles players on separate lines.
    fn set_side(
        &mut self,
        side: &SideView,
        [team, players, first, second, score]: [&'static str; 5],
    ) {
        self.set(team, &side.team);
        let names: Vec<_> = side.players.iter().map(|p| p.display_name()).collect();
        self.set(players, names.join(" / "));
        for (field, name) in [first, second].into_iter().zip(&names) {
            self.set(field, name);
        }
        self.set(score, side.score);
    }

    fn set_optional(&mut self, name: &'static str, value: Option<u16>) {
        if let Some(value) = value {
            self.set(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::competition::{Labels, Player, Slot, UnitView};

    fn side(team: &str, score: u16, players: &[(&str, &str)]) -> SideView {
        SideView {
            team: team.into(),
            score,
            match_points: score,
            game_sets: None,
            slot: Slot::doubles(2),
            players: players
                .iter()
                .map(|(first, last)| Player {
                    id: String::new(),
                    first_name: (*first).into(),
                    last_name: (*last).into(),
                })
                .collect(),
        }
    }

    fn race_scoreboard(outcome: Option<Outcome>) -> Scoreboard {
        Scoreboard {
            revision: 3,
            competition: "mtfv",
            title: "Landesliga".into(),
            labels: Labels {
                tournament: "Landesliga".into(),
                ..Default::default()
            },
            format: Format::mtfv_race(),
            match_format: None,
            segment: Some((2, 7)),
            home: side("Halle", 6, &[("Luca", "Lohr"), ("Anton", "Woschni")]),
            away: side("Jena", 3, &[]),
            unit: UnitView {
                index: 1,
                count: 7,
                singles: false,
                set_in_game: None,
                sets_in_game: None,
                game: 2,
                games: 7,
                game_over: false,
            },
            outcome,
            units: vec![],
        }
    }

    #[test]
    fn every_field_is_present() {
        let view = OverlayView::from_scoreboard(&race_scoreboard(None), &Translations::fixture());
        assert_eq!(view.fields.len(), FIELDS.len());
    }

    #[test]
    fn labels_map_to_placeholders_and_single_games_have_no_slot() {
        let mut scoreboard = race_scoreboard(None);
        scoreboard.competition = "kickertool";
        scoreboard.unit.games = 1;
        scoreboard.labels = Labels {
            tournament: "Cup".into(),
            round: "Finale".into(),
            group: "Vorrunde".into(),
            discipline: "Doppel".into(),
            table: "Tisch 1".into(),
        };
        let view = OverlayView::from_scoreboard(&scoreboard, &Translations::fixture());
        assert_eq!(view.source, "kickertool");
        let f = &view.fields;
        assert_eq!(f["tournamentName"], "Cup");
        assert_eq!(f["roundName"], "Finale");
        assert_eq!(f["groupName"], "Vorrunde");
        assert_eq!(f["disciplineName"], "Doppel");
        assert_eq!(f["table"], "Tisch 1");
        assert_eq!(f["slotA"], "");
    }

    #[test]
    fn race_scoreboard_maps_to_fields() {
        let view = OverlayView::from_scoreboard(&race_scoreboard(None), &Translations::fixture());
        assert_eq!(view.source, "mtfv");
        assert_eq!(view.format, Some("race"));
        let f = &view.fields;
        assert_eq!(f["tournamentName"], "Landesliga");
        assert_eq!(f["teamA"], "Halle");
        assert_eq!(f["teamAPlayer"], "Luca Lohr / Anton Woschni");
        assert_eq!(f["teamAPlayer1"], "Luca Lohr");
        assert_eq!(f["teamAPlayer2"], "Anton Woschni");
        assert_eq!(f["teamBPlayer1"], "");
        assert_eq!(f["teamBPlayer"], "");
        assert_eq!(f["scoreA"], "6");
        assert_eq!(f["scoreB"], "3");
        assert_eq!(f["setsA"], "");
        assert_eq!(f["slotA"], "D2");
        assert_eq!(f["gameName"], "Doubles 2", "named by translations.toml");
        assert_eq!(f["matchPointsB"], "3");
        assert_eq!(f["unitNumber"], "2");
        assert_eq!(f["unitCount"], "7");
        assert_eq!(f["raceTarget"], "42");
        assert_eq!(
            (f["segmentNumber"].as_str(), f["segmentCount"].as_str()),
            ("2", "7")
        );
        assert_eq!(f["setNumber"], "");
        assert_eq!(f["state"], "running");
        assert_eq!(f["winner"], "");
    }

    #[test]
    fn sets_scoreboard_has_sets_and_no_target() {
        let mut scoreboard = race_scoreboard(None);
        scoreboard.format = Format::Sets {
            sets: crate::competition::SetCount::Fixed(2),
            points: 5,
            win_by: 1,
            cap: None,
        };
        scoreboard.home.game_sets = Some(1);
        scoreboard.away.game_sets = Some(0);
        scoreboard.unit.set_in_game = Some(2);
        scoreboard.unit.sets_in_game = Some(2);
        let view = OverlayView::from_scoreboard(&scoreboard, &Translations::fixture());
        assert_eq!(view.format, Some("sets"));
        assert_eq!(view.fields["setsA"], "1");
        assert_eq!(view.fields["setsB"], "0");
        assert_eq!(view.fields["setNumber"], "2");
        assert_eq!(view.fields["setsInGame"], "2");
        assert_eq!(view.fields["raceTarget"], "");
    }

    #[test]
    fn winner_is_the_team_name() {
        let view = OverlayView::from_scoreboard(
            &race_scoreboard(Some(Outcome::Away)),
            &Translations::fixture(),
        );
        assert_eq!(view.fields["winner"], "Jena");
        assert_eq!(view.fields["state"], "finished");
        let draw = OverlayView::from_scoreboard(
            &race_scoreboard(Some(Outcome::Draw)),
            &Translations::fixture(),
        );
        assert_eq!(draw.fields["winner"], "");
        assert_eq!(draw.fields["state"], "finished");
    }
}
