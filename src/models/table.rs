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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_with_name() {
        let t = Table {
            name: "Main Table".into(),
            number: 1,
            ..Default::default()
        };
        assert_eq!(t.to_string(), "Main Table");
    }

    #[test]
    fn display_without_name() {
        let t = Table {
            name: String::new(),
            number: 3,
            ..Default::default()
        };
        assert_eq!(t.to_string(), "Table 3");
    }

    #[test]
    fn deserialize_from_json() {
        let json = r#"{"id": "t1", "number": 2, "name": "Finals", "currentMatchId": "m1"}"#;
        let t: Table = serde_json::from_str(json).unwrap();
        assert_eq!(t.id, "t1");
        assert_eq!(t.number, 2);
        assert_eq!(t.name, "Finals");
        assert_eq!(t.current_match_id, Some("m1".into()));
    }

    #[test]
    fn deserialize_with_defaults() {
        let json = r#"{}"#;
        let t: Table = serde_json::from_str(json).unwrap();
        assert_eq!(t.number, 0);
        assert_eq!(t.name, "");
        assert_eq!(t.current_match_id, None);
    }

    #[test]
    fn serialize_roundtrip() {
        let t = Table {
            id: "t1".into(),
            number: 5,
            name: "VIP".into(),
            current_match_id: Some("m99".into()),
        };
        let json = serde_json::to_string(&t).unwrap();
        let t2: Table = serde_json::from_str(&json).unwrap();
        assert_eq!(t2.id, "t1");
        assert_eq!(t2.number, 5);
        assert_eq!(t2.current_match_id, Some("m99".into()));
    }
}
