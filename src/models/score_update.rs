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
        assert_eq!(su.team_a_score, 5);
        assert_eq!(su.team_b_score, 3);
        assert_eq!(su.team_a_name, "Foo FC");
        assert_eq!(su.team_b_name, "Bar United");
        assert_eq!(su.team_a_player, "Alice");
        assert_eq!(su.team_b_player, "Bob");
        assert_eq!(su.event_name, "Finals");
    }

    #[test]
    fn deserialize_with_defaults() {
        let json = r#"{}"#;
        let su: ScoreUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(su.team_a_score, 0);
        assert_eq!(su.team_b_score, 0);
        assert_eq!(su.team_a_name, "");
    }

    #[test]
    fn serialize_roundtrip() {
        let su = ScoreUpdate {
            team_a_score: 10,
            team_b_score: 7,
            team_a_name: "A".into(),
            team_b_name: "B".into(),
            team_a_player: "P1".into(),
            team_b_player: "P2".into(),
            event_name: "Cup".into(),
        };
        let json = serde_json::to_string(&su).unwrap();
        let su2: ScoreUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(su2.team_a_score, 10);
        assert_eq!(su2.team_b_score, 7);
    }
}
