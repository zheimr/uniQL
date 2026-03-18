pub mod prometheus;
pub mod victorialogs;

use serde_json::Value;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BackendResult {
    pub data: Value,
    pub backend_name: String,
    pub backend_type: String,
    pub native_query: String,
    pub execute_time_ms: u64,
}

#[derive(Debug)]
pub struct ExecutionError {
    pub message: String,
    pub backend: String,
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.backend, self.message)
    }
}
