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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_match(entries: Vec<&str>) -> Match {
        Match {
            id: "m1".into(),
            entries: entries
                .into_iter()
                .map(|name| MatchEntry {
                    id: String::new(),
                    name: name.into(),
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
