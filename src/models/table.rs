use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Table {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub number: i32,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "currentMatchId")]
    pub current_match_id: Option<String>,
}

impl std::fmt::Display for Table {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.name.is_empty() {
            write!(f, "Table {}", self.number)
        } else {
            write!(f, "{}", self.name)
        }
    }
}
