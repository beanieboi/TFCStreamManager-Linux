use super::{Match, Table, Tournament};
use crate::services::Settings;

pub const DEFAULT_SCORE_NAME: &str = "G";
pub const DEFAULT_SETS_NAME: &str = "S";
pub const DEFAULT_ZERO: &str = "0";
pub const DEFAULT_DASH: &str = "-";

/// Returns the value as an owned String if enabled, otherwise empty String.
fn when_enabled(enabled: bool, value: &str) -> String {
    if enabled {
        value.to_owned()
    } else {
        String::new()
    }
}

#[derive(Debug, Clone, Default)]
pub struct OverlayContent {
    pub table: Table,
    pub tournament_name: String,
    pub team_a: String,
    pub team_b: String,
    pub score_name: String,
    pub score_a: String,
    pub score_b: String,
    pub sets_name: String,
    pub sets_a: String,
    pub sets_b: String,
    pub state: String,
    pub start_time: String,
    pub round_name: String,
    pub group_name: String,
    pub discipline_name: String,
    pub team_a_player: String,
    pub team_b_player: String,
}

impl OverlayContent {
    pub fn empty() -> Self {
        Self {
            table: Table {
                name: "No table".to_owned(),
                ..Default::default()
            },
            tournament_name: DEFAULT_DASH.to_owned(),
            team_a: DEFAULT_DASH.to_owned(),
            team_b: DEFAULT_DASH.to_owned(),
            score_name: DEFAULT_SCORE_NAME.to_owned(),
            score_a: DEFAULT_DASH.to_owned(),
            score_b: DEFAULT_DASH.to_owned(),
            sets_name: DEFAULT_SETS_NAME.to_owned(),
            sets_a: DEFAULT_ZERO.to_owned(),
            sets_b: DEFAULT_ZERO.to_owned(),
            state: DEFAULT_DASH.to_owned(),
            start_time: DEFAULT_DASH.to_owned(),
            round_name: DEFAULT_DASH.to_owned(),
            group_name: DEFAULT_DASH.to_owned(),
            discipline_name: DEFAULT_DASH.to_owned(),
            team_a_player: String::new(),
            team_b_player: String::new(),
        }
    }

    pub fn from_match(
        match_data: &Match,
        table: &Table,
        tournament: &Tournament,
        settings: &Settings,
    ) -> Self {
        let show_score = settings.show_score;
        let show_sets = settings.show_sets;

        Self {
            table: table.clone(),
            tournament_name: tournament.name.clone(),
            team_a: match_data.team_a().to_string(),
            team_b: match_data.team_b().to_string(),
            score_name: when_enabled(show_score, DEFAULT_SCORE_NAME),
            score_a: when_enabled(show_score, DEFAULT_ZERO),
            score_b: when_enabled(show_score, DEFAULT_ZERO),
            sets_name: when_enabled(show_sets, DEFAULT_SETS_NAME),
            sets_a: when_enabled(show_sets, DEFAULT_ZERO),
            sets_b: when_enabled(show_sets, DEFAULT_ZERO),
            state: match_data.state.clone(),
            start_time: match_data
                .start_time
                .map(|t| t.format("%H:%M:%S").to_string())
                .unwrap_or_else(|| DEFAULT_DASH.to_owned()),
            round_name: match_data.round_name.clone(),
            group_name: match_data.group_name.clone(),
            discipline_name: match_data.discipline_name.clone(),
            team_a_player: String::new(),
            team_b_player: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::MatchEntry;

    #[test]
    fn empty_has_dash_defaults() {
        let c = OverlayContent::empty();
        assert_eq!(c.tournament_name, "-");
        assert_eq!(c.team_a, "-");
        assert_eq!(c.team_b, "-");
        assert_eq!(c.score_name, "G");
        assert_eq!(c.sets_name, "S");
        assert_eq!(c.score_a, "-");
        assert_eq!(c.sets_a, "0");
        assert_eq!(c.table.name, "No table");
    }

    #[test]
    fn from_match_basic() {
        let match_data = Match {
            id: "m1".into(),
            entries: vec![
                MatchEntry {
                    id: "e1".into(),
                    name: "Team Alpha".into(),
                },
                MatchEntry {
                    id: "e2".into(),
                    name: "Team Beta".into(),
                },
            ],
            state: "running".into(),
            encounters: vec![],
            discipline_name: "Open Singles".into(),
            round_name: "Quarterfinal".into(),
            group_name: "Group A".into(),
            start_time: None,
        };
        let table = Table {
            id: "t1".into(),
            number: 1,
            name: "Main Table".into(),
            current_match_id: Some("m1".into()),
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
        let settings = Settings {
            show_score: true,
            show_sets: true,
            ..Default::default()
        };

        let c = OverlayContent::from_match(&match_data, &table, &tournament, &settings);
        assert_eq!(c.team_a, "Team Alpha");
        assert_eq!(c.team_b, "Team Beta");
        assert_eq!(c.tournament_name, "Summer Cup");
        assert_eq!(c.table.name, "Main Table");
        assert_eq!(c.score_name, "G");
        assert_eq!(c.sets_name, "S");
        assert_eq!(c.score_a, "0");
        assert_eq!(c.discipline_name, "Open Singles");
        assert_eq!(c.round_name, "Quarterfinal");
        assert_eq!(c.start_time, "-"); // no start_time
    }

    #[test]
    fn from_match_hides_score_and_sets() {
        let match_data = Match {
            id: String::new(),
            entries: vec![],
            state: String::new(),
            encounters: vec![],
            discipline_name: String::new(),
            round_name: String::new(),
            group_name: String::new(),
            start_time: None,
        };
        let table = Table::default();
        let tournament = Tournament {
            id: String::new(),
            name: String::new(),
            disciplines: vec![],
            date: None,
            state: String::new(),
            num_players: 0,
            num_teams: 0,
        };
        let settings = Settings {
            show_score: false,
            show_sets: false,
            ..Default::default()
        };

        let c = OverlayContent::from_match(&match_data, &table, &tournament, &settings);
        assert_eq!(c.score_name, "");
        assert_eq!(c.score_a, "");
        assert_eq!(c.score_b, "");
        assert_eq!(c.sets_name, "");
        assert_eq!(c.sets_a, "");
        assert_eq!(c.sets_b, "");
    }

    #[test]
    fn from_match_with_start_time() {
        let match_data = Match {
            id: String::new(),
            entries: vec![],
            state: String::new(),
            encounters: vec![],
            discipline_name: String::new(),
            round_name: String::new(),
            group_name: String::new(),
            start_time: Some("2025-06-15T14:30:00Z".parse().unwrap()),
        };
        let table = Table::default();
        let tournament = Tournament {
            id: String::new(),
            name: String::new(),
            disciplines: vec![],
            date: None,
            state: String::new(),
            num_players: 0,
            num_teams: 0,
        };
        let settings = Settings::default();

        let c = OverlayContent::from_match(&match_data, &table, &tournament, &settings);
        assert_eq!(c.start_time, "14:30:00");
    }
}
