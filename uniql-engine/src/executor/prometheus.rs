use super::{BackendResult, ExecutionError, MAX_RETRIES, RETRY_DELAY_MS, is_retryable_reqwest_error};
use reqwest::Client;
use std::time::Instant;

pub struct PrometheusExecutor {
    client: Client,
    base_url: String,
    name: String,
}

impl PrometheusExecutor {
    pub fn new(name: &str, base_url: &str) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        PrometheusExecutor {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            name: name.to_string(),
        }
    }

    /// Execute an instant query: GET /api/v1/query (with retry on transient failures)
    pub async fn query(&self, promql: &str) -> Result<BackendResult, ExecutionError> {
        let url = format!("{}/api/v1/query", self.base_url);
        let start = Instant::now();

        let resp = self.send_with_retry(|| {
            self.client.get(&url).query(&[("query", promql)])
        }).await?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await.map_err(|e| ExecutionError {
            message: format!("Failed to parse response: {}", e),
            backend: self.name.clone(),
        })?;

        if !status.is_success() {
            return Err(ExecutionError {
                message: format!("Backend returned {}: {}", status, body),
                backend: self.name.clone(),
            });
        }

        Ok(BackendResult {
            data: body,
            backend_name: self.name.clone(),
            backend_type: "prometheus".to_string(),
            native_query: promql.to_string(),
            execute_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Execute a range query: GET /api/v1/query_range (with retry on transient failures)
    pub async fn query_range(
        &self,
        promql: &str,
        start_ts: &str,
        end_ts: &str,
        step: &str,
    ) -> Result<BackendResult, ExecutionError> {
        let url = format!("{}/api/v1/query_range", self.base_url);
        let start = Instant::now();

        let resp = self.send_with_retry(|| {
            self.client.get(&url).query(&[
                ("query", promql),
                ("start", start_ts),
                ("end", end_ts),
                ("step", step),
            ])
        }).await?;

        let body: serde_json::Value = resp.json().await.map_err(|e| ExecutionError {
            message: format!("Failed to parse response: {}", e),
            backend: self.name.clone(),
        })?;

        Ok(BackendResult {
            data: body,
            backend_name: self.name.clone(),
            backend_type: "prometheus".to_string(),
            native_query: promql.to_string(),
            execute_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Health check: GET /health
    pub async fn health(&self) -> Result<bool, ExecutionError> {
        let url = format!("{}/health", self.base_url);
        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    /// Send an HTTP request with retry on transient failures.
    async fn send_with_retry<F>(&self, build_request: F) -> Result<reqwest::Response, ExecutionError>
    where
        F: Fn() -> reqwest::RequestBuilder,
    {
        let mut last_err = None;
        for attempt in 0..=MAX_RETRIES {
            match build_request().send().await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    if attempt < MAX_RETRIES && is_retryable_reqwest_error(&e) {
                        tracing::warn!(
                            backend = %self.name,
                            attempt = attempt + 1,
                            "Retryable error, retrying in {}ms: {}",
                            RETRY_DELAY_MS, e
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(RETRY_DELAY_MS)).await;
                        last_err = Some(e);
                        continue;
                    }
                    return Err(ExecutionError {
                        message: format!("HTTP request failed: {}", e),
                        backend: self.name.clone(),
                    });
                }
            }
        }
        Err(ExecutionError {
            message: format!("HTTP request failed after {} retries: {}", MAX_RETRIES, last_err.unwrap()),
            backend: self.name.clone(),
        })
    }
}
