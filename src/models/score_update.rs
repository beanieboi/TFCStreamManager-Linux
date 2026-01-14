use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreUpdate {
    #[serde(default, rename = "teamAScore")]
    pub team_a_score: i32,
    #[serde(default, rename = "teamBScore")]
    pub team_b_score: i32,
    #[serde(default, rename = "teamAName")]
    pub team_a_name: String,
    #[serde(default, rename = "teamBName")]
    pub team_b_name: String,
    #[serde(default, rename = "teamAPlayer")]
    pub team_a_player: String,
    #[serde(default, rename = "teamBPlayer")]
    pub team_b_player: String,
    #[serde(default, rename = "eventName")]
    pub event_name: String,
}
