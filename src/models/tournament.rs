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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display() {
        let t = Tournament {
            id: "1".into(),
            name: "World Cup 2025".into(),
            disciplines: vec![],
            date: None,
            state: "running".into(),
            num_players: 16,
            num_teams: 8,
        };
        assert_eq!(t.to_string(), "World Cup 2025");
    }

    #[test]
    fn deserialize_from_api_json() {
        let json = r#"{
            "id": "abc",
            "name": "Test Tournament",
            "state": "running",
            "numPlayers": 32,
            "numTeams": 16,
            "disciplines": [
                {"_id": "d1", "shortName": "OS", "name": "Open Singles", "modes": ["se"], "entryType": "player"}
            ],
            "date": "2025-06-01T09:00:00Z"
        }"#;
        let t: Tournament = serde_json::from_str(json).unwrap();
        assert_eq!(t.id, "abc");
        assert_eq!(t.name, "Test Tournament");
        assert_eq!(t.num_players, 32);
        assert_eq!(t.num_teams, 16);
        assert_eq!(t.disciplines.len(), 1);
        assert_eq!(t.disciplines[0].short_name, "OS");
        assert!(t.date.is_some());
    }

    #[test]
    fn deserialize_minimal() {
        let json = r#"{"id": "x", "name": "Minimal"}"#;
        let t: Tournament = serde_json::from_str(json).unwrap();
        assert_eq!(t.id, "x");
        assert!(t.disciplines.is_empty());
        assert_eq!(t.num_players, 0);
    }
}
