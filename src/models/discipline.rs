use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discipline {
    #[serde(rename = "_id", default)]
    pub id: String,
    #[serde(default, rename = "shortName")]
    pub short_name: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub modes: Vec<String>,
    #[serde(default, rename = "entryType")]
    pub entry_type: String,
}
