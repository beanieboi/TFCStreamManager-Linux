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
