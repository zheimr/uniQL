pub mod prometheus;
pub mod victorialogs;

use serde_json::Value;

/// Maximum number of retries for transient failures (connection errors, 5xx).
pub const MAX_RETRIES: u32 = 1;
/// Delay between retries.
pub const RETRY_DELAY_MS: u64 = 200;

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

/// Check if an HTTP error is retryable (connection failure or server error).
pub fn is_retryable_reqwest_error(err: &reqwest::Error) -> bool {
    err.is_connect() || err.is_timeout() || err.status().is_some_and(|s| s.is_server_error())
}
