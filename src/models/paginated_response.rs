use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub data: T,
    #[serde(default, rename = "totalCount")]
    pub total_count: i32,
    #[serde(default)]
    pub limit: i32,
    #[serde(default)]
    pub offset: i32,
}

impl<T> PaginatedResponse<T> {
    pub fn has_more(&self) -> bool {
        self.offset + self.limit < self.total_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_more_true() {
        let resp = PaginatedResponse {
            data: vec![1, 2, 3],
            total_count: 100,
            limit: 10,
            offset: 0,
        };
        assert!(resp.has_more());
    }

    #[test]
    fn has_more_false_at_end() {
        let resp = PaginatedResponse {
            data: vec![1, 2],
            total_count: 12,
            limit: 10,
            offset: 10,
        };
        assert!(!resp.has_more());
    }

    #[test]
    fn has_more_false_exact() {
        let resp = PaginatedResponse {
            data: vec![1],
            total_count: 10,
            limit: 10,
            offset: 0,
        };
        assert!(!resp.has_more());
    }

    #[test]
    fn deserialize_from_json() {
        let json = r#"{"data": [1, 2, 3], "totalCount": 50, "limit": 10, "offset": 0}"#;
        let resp: PaginatedResponse<Vec<i32>> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data, vec![1, 2, 3]);
        assert_eq!(resp.total_count, 50);
        assert_eq!(resp.limit, 10);
        assert_eq!(resp.offset, 0);
        assert!(resp.has_more());
    }
}
