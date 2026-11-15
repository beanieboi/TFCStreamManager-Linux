use super::{MatchEntry, Table, Tournament, Translations};
use crate::competition::{Competition, Labels, MatchFormat, Side, TableMatch, Team};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Match {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub entries: Vec<MatchEntry>,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub encounters: Vec<serde_json::Value>,
    #[serde(default, rename = "disciplineName")]
    pub discipline_name: String,
    #[serde(default, rename = "roundName")]
    pub round_name: String,
    #[serde(default, rename = "groupName")]
    pub group_name: String,
    #[serde(default, rename = "startTime")]
    pub start_time: Option<DateTime<Utc>>,
}

impl Match {
    /// The Kickertool match as the match on the table (ADR 0012): labels,
    /// entries and players from the API; the format is the operator's.
    pub fn to_table_match(
        &self,
        table: &Table,
        tournament: &Tournament,
        translations: &Translations,
        format: MatchFormat,
    ) -> TableMatch {
        let labels = Labels {
            tournament: tournament.name.clone(),
            round: self.round_label(translations),
            group: self.group_label(translations),
            discipline: self.discipline_label(translations),
            table: table.name.clone(),
        };
        let players = self.entries.iter().map(|e| e.players.len()).max();
        let singles = players == Some(1);
        let mut table_match = TableMatch::single_game(
            Competition::Kickertool {
                tournament_id: tournament.id.clone(),
                table_id: table.id.clone(),
                match_id: self.id.clone(),
            },
            labels,
            Team::named(self.team_a()),
            Team::named(self.team_b()),
            singles,
            format,
        );
        for (side, entry) in [
            (Side::Home, self.entries.first()),
            (Side::Away, self.entries.last()),
        ] {
            let names = entry
                .map(|e| e.players.iter().map(|p| p.name.clone()).collect())
                .unwrap_or_default();
            let slot = table_match.units[0].slot(side);
            table_match.prefill_lineup(side, slot, names);
        }
        table_match
    }

    pub fn team_a(&self) -> &str {
        self.entries.first().map(|e| e.name.as_str()).unwrap_or("")
    }

    pub fn team_b(&self) -> &str {
        self.entries.last().map(|e| e.name.as_str()).unwrap_or("")
    }

    /// The round as shown on the overlay.
    pub fn round_label(&self, translations: &Translations) -> String {
        translations.round(&self.round_name)
    }

    /// The group as shown on the overlay.
    pub fn group_label(&self, translations: &Translations) -> String {
        translations.group(&self.group_name)
    }

    /// The discipline as shown on the overlay. Kickertool names unnamed
    /// disciplines "[D1] Discipline 1" (or "Disziplin 1"); those are
    /// replaced by the translation for the number of players per team.
    pub fn discipline_label(&self, translations: &Translations) -> String {
        if !is_default_discipline_name(&self.discipline_name) {
            return self.discipline_name.clone();
        }
        let players = self.entries.iter().map(|e| e.players.len()).max();
        players
            .and_then(|n| translations.unnamed_discipline(n))
            .map_or_else(|| self.discipline_name.clone(), str::to_owned)
    }
}

/// `[D<n>] Discipline <n>` or `[D<n>] Disziplin <n>`, the name Kickertool
/// gives a discipline that was never named.
fn is_default_discipline_name(name: &str) -> bool {
    let Some((tag, label)) = name
        .strip_prefix("[D")
        .and_then(|rest| rest.split_once("] "))
    else {
        return false;
    };
    let number = label
        .strip_prefix("Discipline ")
        .or_else(|| label.strip_prefix("Disziplin "));
    let is_number = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    is_number(tag) && number.is_some_and(is_number)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, players: &[&str]) -> MatchEntry {
        MatchEntry {
            id: String::new(),
            name: name.into(),
            players: players
                .iter()
                .map(|p| MatchEntry {
                    id: String::new(),
                    name: (*p).into(),
                    players: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn kickertool_match_becomes_the_match_on_the_table() {
        let mut m = make_match(vec![]);
        m.entries = vec![entry("Alpha", &["Ann", "Bo"]), entry("Beta", &["Cy", "Di"])];
        m.round_name = "FINALS-1-2".into();
        m.group_name = "Qualification".into();
        m.discipline_name = "[D1] Discipline 1".into();
        let table = Table {
            id: "t1".into(),
            name: "Tisch 1".into(),
            ..Default::default()
        };
        let tournament = Tournament {
            id: "tour1".into(),
            name: "Summer Cup".into(),
            disciplines: vec![],
            date: None,
            state: "running".into(),
            num_players: 0,
            num_teams: 0,
        };
        let format = MatchFormat::Sets {};
        let tm = m.to_table_match(&table, &tournament, &Translations::fixture(), format);

        assert_eq!(tm.labels.tournament, "Summer Cup");
        assert_eq!(tm.labels.round, "Semi");
        assert_eq!(tm.labels.group, "Quali");
        assert_eq!(tm.labels.discipline, "Doubles");
        assert_eq!(tm.labels.table, "Tisch 1");
        assert_eq!(tm.match_format(), Some(format));
        assert_eq!(tm.units.len(), 1);
        assert_eq!(
            tm.competition,
            Competition::Kickertool {
                tournament_id: "tour1".into(),
                table_id: "t1".into(),
                match_id: "m1".into()
            }
        );
        let sb = tm.scoreboard();
        assert_eq!(sb.home.team, "Alpha");
        assert_eq!(sb.away.team, "Beta");
        let names: Vec<_> = sb.home.players.iter().map(|p| p.display_name()).collect();
        assert_eq!(names, ["Ann", "Bo"]);
        assert!(!sb.unit.singles);
    }

    #[test]
    fn one_player_per_entry_is_singles() {
        let mut m = make_match(vec![]);
        m.entries = vec![entry("Ann", &["Ann"]), entry("Bo", &["Bo"])];
        let tournament = Tournament {
            id: String::new(),
            name: String::new(),
            disciplines: vec![],
            date: None,
            state: String::new(),
            num_players: 0,
            num_teams: 0,
        };
        let tm = m.to_table_match(
            &Table::default(),
            &tournament,
            &Translations::fixture(),
            MatchFormat::Race {
                target: 10,
                segment: 0,
            },
        );
        assert!(tm.scoreboard().unit.singles);
        assert_eq!(tm.units.len(), 1);
    }

    fn make_match(entries: Vec<&str>) -> Match {
        Match {
            id: "m1".into(),
            entries: entries
                .into_iter()
                .map(|name| MatchEntry {
                    id: String::new(),
                    name: name.into(),
                    players: vec![],
                })
                .collect(),
            state: "running".into(),
            encounters: vec![],
            discipline_name: String::new(),
            round_name: String::new(),
            group_name: String::new(),
            start_time: None,
        }
    }

    #[test]
    fn team_a_and_b_from_entries() {
        let m = make_match(vec!["Alpha", "Beta"]);
        assert_eq!(m.team_a(), "Alpha");
        assert_eq!(m.team_b(), "Beta");
    }

    #[test]
    fn team_a_and_b_empty_entries() {
        let m = make_match(vec![]);
        assert_eq!(m.team_a(), "");
        assert_eq!(m.team_b(), "");
    }

    #[test]
    fn team_a_and_b_single_entry() {
        let m = make_match(vec!["Solo"]);
        // With one entry, first == last
        assert_eq!(m.team_a(), "Solo");
        assert_eq!(m.team_b(), "Solo");
    }

    #[test]
    fn deserialize_from_json() {
        let json = r#"{
            "id": "abc123",
            "entries": [
                {"id": "e1", "name": "Team A"},
                {"id": "e2", "name": "Team B"}
            ],
            "state": "running",
            "disciplineName": "Singles",
            "roundName": "Round 1",
            "groupName": "Group A",
            "startTime": "2025-01-15T10:30:00Z"
        }"#;
        let m: Match = serde_json::from_str(json).unwrap();
        assert_eq!(m.id, "abc123");
        assert_eq!(m.entries.len(), 2);
        assert_eq!(m.discipline_name, "Singles");
        assert_eq!(m.round_name, "Round 1");
        assert_eq!(m.group_name, "Group A");
        assert!(m.start_time.is_some());
    }

    fn t() -> Translations {
        Translations::fixture()
    }

    fn with_players(discipline: &str, players: &[usize]) -> Match {
        let mut m = make_match(vec!["A"; players.len()]);
        m.discipline_name = discipline.into();
        for (entry, &n) in m.entries.iter_mut().zip(players) {
            entry.players = vec![MatchEntry::default(); n];
        }
        m
    }

    #[test]
    fn default_discipline_name_falls_back_by_player_count() {
        assert_eq!(
            with_players("[D1] Discipline 1", &[2, 2]).discipline_label(&t()),
            "Doubles"
        );
        assert_eq!(
            with_players("[D1] Disziplin 1", &[1, 1]).discipline_label(&t()),
            "Singles"
        );
        assert_eq!(
            with_players("[D2] Discipline 2", &[1, 2]).discipline_label(&t()),
            "Doubles"
        );
        assert_eq!(
            with_players("[D1] Discipline 1", &[0, 0]).discipline_label(&t()),
            "[D1] Discipline 1"
        );
    }

    #[test]
    fn named_discipline_is_kept() {
        for name in [
            "[HD] Herren-Doppel",
            "[D1] Damen",
            "Discipline 1",
            "[DD] Disziplin 1",
            "",
        ] {
            assert_eq!(with_players(name, &[2, 2]).discipline_label(&t()), name);
        }
    }

    #[test]
    fn deserializes_nested_players() {
        let json = r#"{"disciplineName": "[D1] Disziplin 1", "entries": [
            {"id": "t1", "name": "A / B", "entries": [{"id": "a", "name": "A"}, {"id": "b", "name": "B"}]},
            {"id": "t2", "name": "C / D", "entries": [{"id": "c", "name": "C"}, {"id": "d", "name": "D"}]}
        ]}"#;
        let m: Match = serde_json::from_str(json).unwrap();
        assert_eq!(m.entries[0].players[1].name, "B");
        assert_eq!(m.discipline_label(&t()), "Doubles");
    }

    #[test]
    fn deserialize_with_defaults() {
        let json = r#"{}"#;
        let m: Match = serde_json::from_str(json).unwrap();
        assert_eq!(m.id, "");
        assert!(m.entries.is_empty());
        assert!(m.start_time.is_none());
    }

    #[test]
    fn serialize_roundtrip() {
        let m = make_match(vec!["A", "B"]);
        let json = serde_json::to_string(&m).unwrap();
        let m2: Match = serde_json::from_str(&json).unwrap();
        assert_eq!(m2.team_a(), "A");
        assert_eq!(m2.team_b(), "B");
    }
}
