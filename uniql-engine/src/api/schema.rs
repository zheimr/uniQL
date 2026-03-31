//! Schema introspection — discover available metrics, labels, and log fields.
//!
//! Queries backends for metadata so users can explore what's available.

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;
use std::sync::Arc;

use super::ErrorResponse;
use crate::engine::AppState;
use crate::executor::prometheus::PrometheusExecutor;

#[derive(Debug, Serialize)]
pub struct SchemaResponse {
    pub metrics: Vec<String>,
    pub labels: Vec<String>,
    pub label_values: std::collections::HashMap<String, Vec<String>>,
    pub backends: Vec<String>,
    pub total_time_ms: u64,
}

/// GET /v1/schema — Discover available metrics and labels from backends
pub async fn handle_schema(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SchemaResponse>, (StatusCode, Json<ErrorResponse>)> {
    let start = std::time::Instant::now();
    let mut metrics = Vec::new();
    let mut labels = Vec::new();
    let mut label_values = std::collections::HashMap::new();
    let mut backends = Vec::new();

    for bc in &state.config.backends {
        if bc.backend_type == "prometheus" || bc.backend_type == "victoriametrics" {
            backends.push(bc.name.clone());
            let executor = PrometheusExecutor::new(&bc.name, &bc.url);

            // Fetch metric names: GET /api/v1/label/__name__/values
            if let Ok(result) = executor
                .query("count({__name__=~\".+\"}) by (__name__)")
                .await
            {
                if let Some(data) = result
                    .data
                    .get("data")
                    .and_then(|d| d.get("result"))
                    .and_then(|r| r.as_array())
                {
                    for item in data.iter().take(200) {
                        if let Some(name) = item
                            .get("metric")
                            .and_then(|m| m.get("__name__"))
                            .and_then(|n| n.as_str())
                        {
                            if !metrics.contains(&name.to_string()) {
                                metrics.push(name.to_string());
                            }
                        }
                    }
                }
            }

            // Fetch common label names from "up" metric
            if let Ok(result) = executor.query("up").await {
                if let Some(data) = result
                    .data
                    .get("data")
                    .and_then(|d| d.get("result"))
                    .and_then(|r| r.as_array())
                {
                    for item in data {
                        if let Some(metric) = item.get("metric").and_then(|m| m.as_object()) {
                            for key in metric.keys() {
                                if key != "__name__" && !labels.contains(key) {
                                    labels.push(key.clone());
                                }
                            }
                        }
                    }
                    // Collect values for "job" label
                    let job_values: Vec<String> = data
                        .iter()
                        .filter_map(|item| {
                            item.get("metric")?
                                .get("job")?
                                .as_str()
                                .map(|s| s.to_string())
                        })
                        .collect::<std::collections::HashSet<_>>()
                        .into_iter()
                        .collect();
                    if !job_values.is_empty() {
                        label_values.insert("job".to_string(), job_values);
                    }
                }
            }
        }
    }

    metrics.sort();
    labels.sort();

    Ok(Json(SchemaResponse {
        metrics,
        labels,
        label_values,
        backends,
        total_time_ms: start.elapsed().as_millis() as u64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BackendConfig, EngineConfig};
    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn schema_returns_metrics_and_labels() {
        let server = MockServer::start().await;
        // Mock count by __name__
        Mock::given(matchers::path("/api/v1/query"))
            .and(matchers::query_param_contains("query", "count"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success",
                "data": { "resultType": "vector", "result": [
                    {"metric": {"__name__": "up"}, "value": [1, "14"]},
                    {"metric": {"__name__": "cpu_usage"}, "value": [1, "100"]},
                ]}
            })))
            .mount(&server)
            .await;

        // Mock up metric
        Mock::given(matchers::path("/api/v1/query"))
            .and(matchers::query_param("query", "up"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success",
                "data": { "resultType": "vector", "result": [
                    {"metric": {"__name__": "up", "job": "api", "instance": "localhost:9090"}, "value": [1, "1"]},
                ]}
            })))
            .mount(&server).await;

        let config = EngineConfig {
            listen: "0.0.0.0:0".to_string(),
            backends: vec![BackendConfig {
                name: "vm".to_string(),
                backend_type: "prometheus".to_string(),
                url: server.uri(),
            }],
            api_keys: vec![],
            cors_origins: vec![],
        };
        let state = Arc::new(AppState {
            config,
            cache: crate::cache::QueryCache::new(100, 15),
            metrics: crate::api::metrics::EngineMetrics::new(),
            rate_limiter: crate::rate_limit::RateLimiter::new(100),
        });

        let result = handle_schema(State(state)).await;
        assert!(result.is_ok());
        let Json(resp) = result.unwrap();
        assert!(resp.metrics.contains(&"up".to_string()));
        assert!(resp.labels.contains(&"job".to_string()));
        assert!(resp.backends.contains(&"vm".to_string()));
    }
}
