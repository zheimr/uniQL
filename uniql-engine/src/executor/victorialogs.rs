use super::{
    is_retryable_reqwest_error, BackendResult, ExecutionError, MAX_RETRIES, RETRY_DELAY_MS,
};
use reqwest::Client;
use std::time::Instant;

pub struct VictoriaLogsExecutor {
    client: Client,
    base_url: String,
    name: String,
}

impl VictoriaLogsExecutor {
    pub fn new(name: &str, base_url: &str) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        VictoriaLogsExecutor {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            name: name.to_string(),
        }
    }

    /// Execute a LogsQL query: GET /select/logsql/query
    pub async fn query(
        &self,
        logsql: &str,
        limit: u32,
        start: &str,
    ) -> Result<BackendResult, ExecutionError> {
        self.query_range(logsql, limit, start, "").await
    }

    /// Execute a LogsQL query with explicit time range (with retry on transient failures)
    pub async fn query_range(
        &self,
        logsql: &str,
        limit: u32,
        start: &str,
        end: &str,
    ) -> Result<BackendResult, ExecutionError> {
        let url = format!("{}/select/logsql/query", self.base_url);
        let timer = Instant::now();

        let mut params = vec![
            ("query", logsql.to_string()),
            ("limit", limit.to_string()),
            ("start", start.to_string()),
        ];
        if !end.is_empty() {
            params.push(("end", end.to_string()));
        }

        let resp = self
            .send_with_retry(|| self.client.get(&url).query(&params))
            .await?;

        let status = resp.status();
        let body_text = resp.text().await.map_err(|e| ExecutionError {
            message: format!("Failed to read response: {}", e),
            backend: self.name.clone(),
        })?;

        if !status.is_success() {
            return Err(ExecutionError {
                message: format!("Backend returned {}: {}", status, body_text),
                backend: self.name.clone(),
            });
        }

        // VictoriaLogs returns NDJSON (one JSON per line)
        let results: Vec<serde_json::Value> = body_text
            .lines()
            .filter(|l| !l.is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();

        Ok(BackendResult {
            data: serde_json::json!({
                "status": "success",
                "result_type": "logs",
                "result": results,
                "total": results.len(),
            }),
            backend_name: self.name.clone(),
            backend_type: "victorialogs".to_string(),
            native_query: logsql.to_string(),
            execute_time_ms: timer.elapsed().as_millis() as u64,
        })
    }

    /// Execute a stats query: GET /select/logsql/stats_query
    #[allow(dead_code)]
    pub async fn stats_query(
        &self,
        logsql: &str,
        start: &str,
    ) -> Result<BackendResult, ExecutionError> {
        self.query(logsql, 1000, start).await
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
    async fn send_with_retry<F>(
        &self,
        build_request: F,
    ) -> Result<reqwest::Response, ExecutionError>
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
            message: format!(
                "HTTP request failed after {} retries: {}",
                MAX_RETRIES,
                last_err.unwrap()
            ),
            backend: self.name.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn query_success_ndjson() {
        let server = MockServer::start().await;
        // VLogs returns NDJSON (one JSON per line)
        let body = r#"{"_msg":"log1","_time":"2026-03-18T10:00:00Z"}
{"_msg":"log2","_time":"2026-03-18T10:00:01Z"}"#;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/select/logsql/query"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let exec = VictoriaLogsExecutor::new("vlogs", &server.uri());
        let result = exec.query("*", 100, "-5m").await.unwrap();
        assert_eq!(result.backend_type, "victorialogs");
        assert_eq!(result.data["status"], "success");
        assert_eq!(result.data["result"].as_array().unwrap().len(), 2);
        assert_eq!(result.data["result"][0]["_msg"], "log1");
    }

    #[tokio::test]
    async fn query_range_with_end_param() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/select/logsql/query"))
            .and(matchers::query_param("end", "now"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"_msg":"ok"}"#))
            .mount(&server)
            .await;

        let exec = VictoriaLogsExecutor::new("vlogs", &server.uri());
        let result = exec.query_range("*", 10, "-1h", "now").await.unwrap();
        assert_eq!(result.data["result"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn query_empty_end_omitted() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/select/logsql/query"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&server)
            .await;

        let exec = VictoriaLogsExecutor::new("vlogs", &server.uri());
        let result = exec.query_range("*", 10, "-5m", "").await.unwrap();
        assert_eq!(result.data["result"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn query_backend_error() {
        let server = MockServer::start().await;
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/select/logsql/query"))
            .respond_with(ResponseTemplate::new(400).set_body_string("parse error"))
            .mount(&server)
            .await;

        let exec = VictoriaLogsExecutor::new("vlogs", &server.uri());
        let result = exec.query("bad query", 10, "-5m").await;
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

        let exec = VictoriaLogsExecutor::new("vlogs", &server.uri());
        assert!(exec.health().await.unwrap());
    }

    #[tokio::test]
    async fn health_check_unreachable() {
        let exec = VictoriaLogsExecutor::new("vlogs", "http://127.0.0.1:1");
        assert!(!exec.health().await.unwrap());
    }

    #[tokio::test]
    async fn query_connection_refused() {
        let exec = VictoriaLogsExecutor::new("vlogs", "http://127.0.0.1:1");
        let result = exec.query("*", 10, "-5m").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn query_partial_ndjson() {
        let server = MockServer::start().await;
        // Partial NDJSON — one valid line, one empty, one malformed
        let body = r#"{"_msg":"valid"}

not-json
{"_msg":"also valid"}"#;
        Mock::given(matchers::path("/select/logsql/query"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let exec = VictoriaLogsExecutor::new("vlogs", &server.uri());
        let result = exec.query("*", 100, "-5m").await.unwrap();
        // Should only parse valid JSON lines, skip malformed
        assert_eq!(result.data["result"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn query_large_result() {
        let server = MockServer::start().await;
        let lines: Vec<String> = (0..200)
            .map(|i| {
                format!(
                    r#"{{"_msg":"log {}","_time":"2026-03-18T10:00:{:02}Z"}}"#,
                    i,
                    i % 60
                )
            })
            .collect();
        Mock::given(matchers::path("/select/logsql/query"))
            .respond_with(ResponseTemplate::new(200).set_body_string(lines.join("\n")))
            .mount(&server)
            .await;

        let exec = VictoriaLogsExecutor::new("vlogs", &server.uri());
        let result = exec.query("*", 1000, "-1h").await.unwrap();
        assert_eq!(result.data["result"].as_array().unwrap().len(), 200);
        assert_eq!(result.data["total"], 200);
    }

    #[tokio::test]
    async fn query_records_metadata() {
        let server = MockServer::start().await;
        Mock::given(matchers::path("/select/logsql/query"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"_msg":"test"}"#))
            .mount(&server)
            .await;

        let exec = VictoriaLogsExecutor::new("my-vlogs", &server.uri());
        let result = exec
            .query("_stream:{job=\"api\"}", 50, "-10m")
            .await
            .unwrap();
        assert_eq!(result.backend_name, "my-vlogs");
        assert_eq!(result.backend_type, "victorialogs");
        assert_eq!(result.native_query, "_stream:{job=\"api\"}");
        assert!(result.execute_time_ms < 1000);
    }

    #[tokio::test]
    async fn stats_query_delegates_to_query() {
        let server = MockServer::start().await;
        Mock::given(matchers::path("/select/logsql/query"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"count":"42"}"#))
            .mount(&server)
            .await;

        let exec = VictoriaLogsExecutor::new("vlogs", &server.uri());
        let result = exec.stats_query("* | stats count()", "-1h").await.unwrap();
        assert_eq!(result.data["result"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn query_5xx_returns_error() {
        let server = MockServer::start().await;
        Mock::given(matchers::path("/select/logsql/query"))
            .respond_with(ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let exec = VictoriaLogsExecutor::new("vlogs", &server.uri());
        let result = exec.query("*", 10, "-5m").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("500"));
    }
}
