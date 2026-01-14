use super::Discipline;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tournament {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub disciplines: Vec<Discipline>,
    #[serde(default)]
    pub date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub state: String,
    #[serde(default, rename = "numPlayers")]
    pub num_players: i32,
    #[serde(default, rename = "numTeams")]
    pub num_teams: i32,
}

impl std::fmt::Display for Tournament {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}
