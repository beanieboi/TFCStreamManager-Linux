use super::MatchEntry;
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
    pub fn team_a(&self) -> &str {
        self.entries.first().map(|e| e.name.as_str()).unwrap_or("")
    }

    pub fn team_b(&self) -> &str {
        self.entries.last().map(|e| e.name.as_str()).unwrap_or("")
    }
}
