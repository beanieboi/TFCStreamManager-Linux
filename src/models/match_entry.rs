use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}
