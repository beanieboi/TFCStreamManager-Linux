use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreUpdate {
    #[serde(rename = "teamAScore")]
    pub team_a_score: Option<i32>,
    #[serde(rename = "teamBScore")]
    pub team_b_score: Option<i32>,
    #[serde(rename = "teamAName")]
    pub team_a_name: Option<String>,
    #[serde(rename = "teamBName")]
    pub team_b_name: Option<String>,
    #[serde(rename = "teamAPlayer")]
    pub team_a_player: Option<String>,
    #[serde(rename = "teamBPlayer")]
    pub team_b_player: Option<String>,
    #[serde(rename = "eventName")]
    pub event_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_score_update() {
        let json = r#"{
            "teamAScore": 5,
            "teamBScore": 3,
            "teamAName": "Foo FC",
            "teamBName": "Bar United",
            "teamAPlayer": "Alice",
            "teamBPlayer": "Bob",
            "eventName": "Finals"
        }"#;
        let su: ScoreUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(su.team_a_score, Some(5));
        assert_eq!(su.team_b_score, Some(3));
        assert_eq!(su.team_a_name.as_deref(), Some("Foo FC"));
        assert_eq!(su.team_b_name.as_deref(), Some("Bar United"));
        assert_eq!(su.team_a_player.as_deref(), Some("Alice"));
        assert_eq!(su.team_b_player.as_deref(), Some("Bob"));
        assert_eq!(su.event_name.as_deref(), Some("Finals"));
    }

    #[test]
    fn deserialize_with_missing_fields() {
        let json = r#"{}"#;
        let su: ScoreUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(su.team_a_score, None);
        assert_eq!(su.team_b_score, None);
        assert_eq!(su.team_a_name, None);
    }

    #[test]
    fn deserialize_partial_update() {
        let json = r#"{"teamAScore": 3, "teamBScore": 1}"#;
        let su: ScoreUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(su.team_a_score, Some(3));
        assert_eq!(su.team_b_score, Some(1));
        assert_eq!(su.team_a_name, None);
        assert_eq!(su.event_name, None);
    }

    #[test]
    fn serialize_roundtrip() {
        let su = ScoreUpdate {
            team_a_score: Some(10),
            team_b_score: Some(7),
            team_a_name: Some("A".into()),
            team_b_name: Some("B".into()),
            team_a_player: Some("P1".into()),
            team_b_player: Some("P2".into()),
            event_name: Some("Cup".into()),
        };
        let json = serde_json::to_string(&su).unwrap();
        let su2: ScoreUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(su2.team_a_score, Some(10));
        assert_eq!(su2.team_b_score, Some(7));
    }
}
