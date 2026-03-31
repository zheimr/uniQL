pub mod explain;
pub mod health;
pub mod investigate;
pub mod metrics;
pub mod query;
pub mod schema;
pub mod validate;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub query: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_format() -> String {
    "json".to_string()
}
fn default_limit() -> u32 {
    100
}

#[derive(Debug, Serialize)]
pub struct QueryResponse {
    pub status: String,
    pub data: serde_json::Value,
    pub metadata: QueryMetadata,
}

#[derive(Debug, Serialize)]
pub struct QueryMetadata {
    pub query_id: String,
    pub parse_time_us: u64,
    pub transpile_time_us: u64,
    pub execute_time_ms: u64,
    pub total_time_ms: u64,
    pub backend: String,
    pub backend_type: String,
    pub native_query: String,
    pub signal_type: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub status: String,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}
