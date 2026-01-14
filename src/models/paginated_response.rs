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
