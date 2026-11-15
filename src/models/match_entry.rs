use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatchEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// The players of a team entry. Kickertool nests them as `entries`.
    #[serde(default, rename = "entries")]
    pub players: Vec<MatchEntry>,
}
