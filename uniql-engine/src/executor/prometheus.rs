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

    #[cfg(test)]
    pub fn with_base_url(name: &str, base_url: &str) -> Self {
        Self::new(name, base_url)
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

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{MockServer, Mock, matchers, ResponseTemplate};

    #[tokio::test]
    async fn query_instant_success() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/api/v1/query"))
            .and(matchers::query_param("query", "up"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success",
                "data": { "resultType": "vector", "result": [{"metric": {"job": "api"}, "value": [1000, "1"]}] }
            })))
            .mount(&server)
            .await;

        let exec = PrometheusExecutor::new("test", &server.uri());
        let result = exec.query("up").await.unwrap();
        assert_eq!(result.backend_name, "test");
        assert_eq!(result.backend_type, "prometheus");
        assert_eq!(result.data["status"], "success");
        assert_eq!(result.data["data"]["result"][0]["value"][1], "1");
    }

    #[tokio::test]
    async fn query_range_success() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/api/v1/query_range"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success",
                "data": { "resultType": "matrix", "result": [{"metric": {"job": "api"}, "values": [[1000, "1"], [1015, "2"]]}] }
            })))
            .mount(&server)
            .await;

        let exec = PrometheusExecutor::new("test", &server.uri());
        let result = exec.query_range("up", "-1h", "now", "15s").await.unwrap();
        assert_eq!(result.data["data"]["resultType"], "matrix");
    }

    #[tokio::test]
    async fn query_backend_error_returns_error() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/api/v1/query"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "status": "error", "error": "bad query"
            })))
            .mount(&server)
            .await;

        let exec = PrometheusExecutor::new("test", &server.uri());
        let result = exec.query("bad{").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("400"));
    }

    #[tokio::test]
    async fn health_check_reachable() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/health"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let exec = PrometheusExecutor::new("test", &server.uri());
        assert!(exec.health().await.unwrap());
    }

    #[tokio::test]
    async fn health_check_unreachable() {
        let exec = PrometheusExecutor::new("test", "http://127.0.0.1:1");
        assert!(!exec.health().await.unwrap());
    }

    #[tokio::test]
    async fn query_connection_refused() {
        let exec = PrometheusExecutor::new("test", "http://127.0.0.1:1");
        let result = exec.query("up").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn query_range_backend_error() {
        let server = MockServer::start().await;
        Mock::given(matchers::path("/api/v1/query_range"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                "status": "error", "error": "internal server error"
            })))
            .mount(&server)
            .await;

        let exec = PrometheusExecutor::new("test", &server.uri());
        let result = exec.query_range("up", "-1h", "now", "15s").await;
        // query_range doesn't check status — returns raw body
        assert!(result.is_ok());
        assert_eq!(result.unwrap().data["status"], "error");
    }

    #[tokio::test]
    async fn query_malformed_json_response() {
        let server = MockServer::start().await;
        Mock::given(matchers::path("/api/v1/query"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let exec = PrometheusExecutor::new("test", &server.uri());
        let result = exec.query("up").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("parse"));
    }

    #[tokio::test]
    async fn query_empty_result_set() {
        let server = MockServer::start().await;
        Mock::given(matchers::path("/api/v1/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success",
                "data": { "resultType": "vector", "result": [] }
            })))
            .mount(&server)
            .await;

        let exec = PrometheusExecutor::new("test", &server.uri());
        let result = exec.query("nonexistent_metric").await.unwrap();
        assert_eq!(result.data["data"]["result"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn query_large_result_set() {
        let server = MockServer::start().await;
        let large_result: Vec<serde_json::Value> = (0..500).map(|i| serde_json::json!({
            "metric": {"__name__": "up", "instance": format!("host-{}", i)},
            "value": [1000, "1"]
        })).collect();
        Mock::given(matchers::path("/api/v1/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success",
                "data": { "resultType": "vector", "result": large_result }
            })))
            .mount(&server)
            .await;

        let exec = PrometheusExecutor::new("test", &server.uri());
        let result = exec.query("up").await.unwrap();
        assert_eq!(result.data["data"]["result"].as_array().unwrap().len(), 500);
    }

    #[tokio::test]
    async fn query_records_execution_time() {
        let server = MockServer::start().await;
        Mock::given(matchers::path("/api/v1/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success", "data": { "result": [] }
            })))
            .mount(&server)
            .await;

        let exec = PrometheusExecutor::new("test", &server.uri());
        let result = exec.query("up").await.unwrap();
        assert!(result.execute_time_ms < 1000); // should complete within 1s
        assert_eq!(result.native_query, "up");
    }

    #[tokio::test]
    async fn query_range_records_native_query() {
        let server = MockServer::start().await;
        Mock::given(matchers::path("/api/v1/query_range"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success", "data": { "result": [] }
            })))
            .mount(&server)
            .await;

        let exec = PrometheusExecutor::new("prom", &server.uri());
        let result = exec.query_range("rate(up[5m])", "-1h", "now", "15s").await.unwrap();
        assert_eq!(result.native_query, "rate(up[5m])");
        assert_eq!(result.backend_name, "prom");
    }
}
