use super::{BackendResult, ExecutionError};
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

    /// Execute a LogsQL query with explicit time range: GET /select/logsql/query
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

        let resp = self.client
            .get(&url)
            .query(&params)
            .send()
            .await
            .map_err(|e| ExecutionError {
                message: format!("HTTP request failed: {}", e),
                backend: self.name.clone(),
            })?;

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
        // Stats queries use the same endpoint, pipe syntax handles stats
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
}
