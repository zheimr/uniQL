use axum::{extract::State, http::StatusCode, Json};
use std::sync::Arc;
use std::time::Instant;

use super::{QueryRequest, QueryResponse, QueryMetadata, ErrorResponse};
use crate::engine::AppState;
use crate::format::{self, FormatSpec};
use crate::normalize_result;
use crate::planner;
use crate::correlate;
use crate::executor::{BackendResult, prometheus::PrometheusExecutor, victorialogs::VictoriaLogsExecutor};

pub async fn handle_query(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, (StatusCode, Json<ErrorResponse>)> {
    let total_start = Instant::now();
    state.metrics.queries_total.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // 0. Check cache
    if let Some(cached) = state.cache.get(&req.query).await {
        state.metrics.queries_cached.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let total_time_ms = total_start.elapsed().as_millis() as u64;
        return Ok(Json(QueryResponse {
            status: "success".to_string(),
            data: cached.data,
            metadata: QueryMetadata {
                query_id: uuid::Uuid::new_v4().to_string(),
                parse_time_us: 0,
                transpile_time_us: 0,
                execute_time_ms: 0,
                total_time_ms,
                backend: cached.backend,
                backend_type: cached.backend_type,
                native_query: cached.native_query,
                signal_type: cached.signal_type,
            },
        }));
    }

    // 1. Parse → Expand → Validate
    let parse_start = Instant::now();
    let ast = uniql_core::prepare(&req.query).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            status: "error".to_string(),
            error: e.to_string(),
            hint: Some("Check your UNIQL syntax.".to_string()),
        }))
    })?;
    let parse_time_us = parse_start.elapsed().as_micros() as u64;

    // 3. Plan — decompose into sub-queries
    let transpile_start = Instant::now();
    let plan = planner::plan(&ast, &state.config).map_err(|e| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse {
            status: "error".to_string(),
            error: e.message,
            hint: None,
        }))
    })?;
    let transpile_time_us = transpile_start.elapsed().as_micros() as u64;

    let is_multi_signal = plan.sub_queries.len() > 1;

    // 4. Execute sub-queries in parallel
    let execute_start = Instant::now();
    let mut results: Vec<(String, BackendResult)> = Vec::new();

    if is_multi_signal {
        // Parallel execution with tokio::join!
        let futures: Vec<_> = plan.sub_queries.iter().map(|sq| {
            let signal = sq.signal_type.clone();
            let backend_type = sq.backend_type.clone();
            let backend_name = sq.backend_name.clone();
            let backend_url = sq.backend_url.clone();
            let native_query = sq.native_query.clone();
            let time_start = sq.time_start.clone();
            let limit = req.limit;

            let has_time_range = sq.has_time_range;
            let time_end = sq.time_end.clone();
            let step = sq.step.clone();

            async move {
                let result = match backend_type.as_str() {
                    "prometheus" | "victoriametrics" => {
                        let executor = PrometheusExecutor::new(&backend_name, &backend_url);
                        if has_time_range {
                            executor.query_range(&native_query, &time_start, &time_end, &step).await
                        } else {
                            executor.query(&native_query).await
                        }
                    }
                    "victorialogs" => {
                        VictoriaLogsExecutor::new(&backend_name, &backend_url)
                            .query_range(&native_query, limit, &time_start, &time_end).await
                    }
                    _ => Err(crate::executor::ExecutionError {
                        message: format!("Unsupported backend: {}", backend_type),
                        backend: backend_name,
                    }),
                };
                (signal, result)
            }
        }).collect();

        let parallel_results = futures::future::join_all(futures).await;
        for (signal, result) in parallel_results {
            match result {
                Ok(r) => results.push((signal, r)),
                Err(e) => {
                    tracing::warn!("Sub-query failed for {}: {}", signal, e);
                    // Continue with partial results
                }
            }
        }
    } else {
        // Single signal — direct execution
        let sq = &plan.sub_queries[0];
        let result = match sq.backend_type.as_str() {
            "prometheus" | "victoriametrics" => {
                let executor = PrometheusExecutor::new(&sq.backend_name, &sq.backend_url);
                if sq.has_time_range {
                    executor.query_range(&sq.native_query, &sq.time_start, &sq.time_end, &sq.step).await
                } else {
                    executor.query(&sq.native_query).await
                }
            }
            "victorialogs" => {
                VictoriaLogsExecutor::new(&sq.backend_name, &sq.backend_url)
                    .query_range(&sq.native_query, req.limit, &sq.time_start, &sq.time_end).await
            }
            _ => {
                return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
                    status: "error".to_string(),
                    error: format!("Unsupported backend: {}", sq.backend_type),
                    hint: None,
                })));
            }
        }.map_err(|e| {
            (StatusCode::BAD_GATEWAY, Json(ErrorResponse {
                status: "error".to_string(),
                error: format!("Backend execution failed: {}", e),
                hint: Some(format!("Backend '{}' at {} may be unreachable.", sq.backend_name, sq.backend_url)),
            }))
        })?;

        results.push((sq.signal_type.clone(), result));
    }

    let execute_time_ms = execute_start.elapsed().as_millis() as u64;

    // 5. Normalize results
    let normalized_results: Vec<(String, normalize_result::NormalizedResult)> = results.iter()
        .map(|(signal, result)| {
            let normalizer = normalize_result::get_normalizer(&result.backend_type);
            (signal.clone(), normalizer.normalize(result, signal))
        })
        .collect();

    // 6. Correlate if multi-signal
    let (response_data, backend_summary, native_summary) = if is_multi_signal {
        if let Some(ref correlation_plan) = plan.correlation {
            let correlated = correlate::correlate_normalized(&normalized_results, correlation_plan);
            let data = serde_json::json!({
                "result_type": "correlated",
                "correlated_events": correlated.events,
                "correlation": correlated.metadata,
            });
            let backends: Vec<String> = plan.sub_queries.iter()
                .map(|sq| sq.backend_name.clone()).collect();
            let natives: Vec<String> = plan.sub_queries.iter()
                .map(|sq| sq.native_query.clone()).collect();
            (data, backends.join(" + "), natives.join(" | "))
        } else {
            // Multi-signal without correlation — merge raw results
            let mut merged = serde_json::Map::new();
            for (signal, result) in &results {
                merged.insert(signal.clone(), result.data.clone());
            }
            let backends: Vec<String> = plan.sub_queries.iter()
                .map(|sq| sq.backend_name.clone()).collect();
            let natives: Vec<String> = plan.sub_queries.iter()
                .map(|sq| sq.native_query.clone()).collect();
            (serde_json::Value::Object(merged), backends.join(" + "), natives.join(" | "))
        }
    } else {
        let r = &results[0].1;
        (r.data.clone(), r.backend_name.clone(), r.native_query.clone())
    };

    // 7. Apply formatter (SHOW clause + format parameter + limit)
    let format_spec = FormatSpec {
        show_format: plan.sub_queries.first().and_then(|sq| sq.show_format.clone()),
        output_format: req.format.clone(),
        limit: req.limit,
    };
    let response_data = format::format_response(&response_data, &format_spec);

    let total_time_ms = total_start.elapsed().as_millis() as u64;

    let signal_type_str = if is_multi_signal {
        plan.sub_queries.iter()
            .map(|sq| sq.signal_type.as_str())
            .collect::<Vec<_>>()
            .join("+")
    } else {
        plan.sub_queries[0].signal_type.clone()
    };

    let backend_type_str = if is_multi_signal {
        "multi".to_string()
    } else {
        plan.sub_queries[0].backend_type.clone()
    };

    // Cache the result
    state.cache.put(
        &req.query, response_data.clone(), &native_summary,
        &backend_summary, &backend_type_str, &signal_type_str,
    ).await;

    Ok(Json(QueryResponse {
        status: "success".to_string(),
        data: response_data,
        metadata: QueryMetadata {
            query_id: uuid::Uuid::new_v4().to_string(),
            parse_time_us,
            transpile_time_us,
            execute_time_ms,
            total_time_ms,
            backend: backend_summary,
            backend_type: backend_type_str,
            native_query: native_summary,
            signal_type: signal_type_str,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EngineConfig, BackendConfig};
    use crate::engine::AppState;
    use wiremock::{MockServer, Mock, matchers, ResponseTemplate};

    async fn setup_with_mock_backends() -> (Arc<AppState>, MockServer, MockServer) {
        let prom = MockServer::start().await;
        Mock::given(matchers::path("/api/v1/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success",
                "data": { "resultType": "vector", "result": [{"metric": {"__name__": "up", "job": "api"}, "value": [1000, "1"]}] }
            })))
            .mount(&prom)
            .await;

        let vlogs = MockServer::start().await;
        Mock::given(matchers::path("/select/logsql/query"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"_msg":"test log","_time":"2026-03-18T10:00:00Z","job":"fortigate"}"#
            ))
            .mount(&vlogs)
            .await;

        let config = EngineConfig {
            listen: "0.0.0.0:0".to_string(),
            backends: vec![
                BackendConfig { name: "victoria".to_string(), backend_type: "prometheus".to_string(), url: prom.uri() },
                BackendConfig { name: "vlogs".to_string(), backend_type: "victorialogs".to_string(), url: vlogs.uri() },
            ],
            api_keys: vec![],
            cors_origins: vec![],
        };
        (Arc::new(AppState { config, cache: crate::cache::QueryCache::new(100, 15), metrics: crate::api::metrics::EngineMetrics::new(), rate_limiter: crate::rate_limit::RateLimiter::new(100) }), prom, vlogs)
    }

    #[tokio::test]
    async fn query_metrics_success() {
        let (state, _prom, _vlogs) = setup_with_mock_backends().await;
        let req = QueryRequest { query: "FROM metrics WHERE __name__ = \"up\"".to_string(), format: "json".to_string(), limit: 100 };
        let result = handle_query(State(state), Json(req)).await;
        assert!(result.is_ok());
        let Json(resp) = result.unwrap();
        assert_eq!(resp.status, "success");
        assert_eq!(resp.metadata.signal_type, "metrics");
        assert_eq!(resp.metadata.backend_type, "prometheus");
    }

    #[tokio::test]
    async fn query_logs_success() {
        let (state, _prom, _vlogs) = setup_with_mock_backends().await;
        let req = QueryRequest { query: "FROM logs WHERE job = \"fortigate\"".to_string(), format: "json".to_string(), limit: 100 };
        let result = handle_query(State(state), Json(req)).await;
        assert!(result.is_ok());
        let Json(resp) = result.unwrap();
        assert_eq!(resp.status, "success");
        assert_eq!(resp.metadata.backend_type, "victorialogs");
    }

    #[tokio::test]
    async fn query_vlogs_routing() {
        let (state, _prom, _vlogs) = setup_with_mock_backends().await;
        let req = QueryRequest { query: "SHOW table FROM vlogs WHERE job = \"fortigate\"".to_string(), format: "json".to_string(), limit: 100 };
        let result = handle_query(State(state), Json(req)).await;
        assert!(result.is_ok());
        let Json(resp) = result.unwrap();
        assert_eq!(resp.metadata.backend_type, "victorialogs");
    }

    #[tokio::test]
    async fn query_invalid_syntax_returns_error() {
        let (state, _prom, _vlogs) = setup_with_mock_backends().await;
        let req = QueryRequest { query: "NOT VALID UNIQL!!!".to_string(), format: "json".to_string(), limit: 100 };
        let result = handle_query(State(state), Json(req)).await;
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn query_show_table_format() {
        let (state, _prom, _vlogs) = setup_with_mock_backends().await;
        let req = QueryRequest { query: "SHOW table FROM metrics WHERE __name__ = \"up\"".to_string(), format: "json".to_string(), limit: 100 };
        let result = handle_query(State(state), Json(req)).await;
        assert!(result.is_ok());
        let Json(resp) = result.unwrap();
        assert_eq!(resp.data["format"], "table");
    }

    #[tokio::test]
    async fn query_show_count_format() {
        let (state, _prom, _vlogs) = setup_with_mock_backends().await;
        let req = QueryRequest { query: "SHOW count FROM metrics WHERE __name__ = \"up\"".to_string(), format: "json".to_string(), limit: 100 };
        let result = handle_query(State(state), Json(req)).await;
        assert!(result.is_ok());
        let Json(resp) = result.unwrap();
        assert_eq!(resp.data["format"], "count");
    }

    #[tokio::test]
    async fn query_limit_applied() {
        let (state, _prom, _vlogs) = setup_with_mock_backends().await;
        let req = QueryRequest { query: "FROM metrics WHERE __name__ = \"up\"".to_string(), format: "json".to_string(), limit: 1 };
        let result = handle_query(State(state), Json(req)).await;
        assert!(result.is_ok());
        let Json(resp) = result.unwrap();
        // Original has 1 result, limit 1 should pass through
        assert!(resp.data["data"]["result"].as_array().unwrap().len() <= 1);
    }

    #[tokio::test]
    async fn query_metadata_populated() {
        let (state, _prom, _vlogs) = setup_with_mock_backends().await;
        let req = QueryRequest { query: "FROM metrics WHERE __name__ = \"up\"".to_string(), format: "json".to_string(), limit: 100 };
        let result = handle_query(State(state), Json(req)).await.unwrap().0;
        assert!(!result.metadata.query_id.is_empty());
        assert!(result.metadata.total_time_ms < 5000);
        assert!(!result.metadata.native_query.is_empty());
    }

    #[tokio::test]
    async fn query_multi_signal_correlate() {
        let (state, _prom, _vlogs) = setup_with_mock_backends().await;
        let req = QueryRequest {
            query: "FROM metrics, logs CORRELATE ON job WITHIN 60s".to_string(),
            format: "json".to_string(),
            limit: 100,
        };
        let result = handle_query(State(state), Json(req)).await;
        assert!(result.is_ok());
        let Json(resp) = result.unwrap();
        assert_eq!(resp.metadata.signal_type, "metrics+logs");
        assert_eq!(resp.metadata.backend_type, "multi");
        // Should have correlation structure
        assert!(resp.data.get("correlated_events").is_some() || resp.data.get("result_type").is_some());
    }

    #[tokio::test]
    async fn query_with_within_uses_range() {
        let (state, _prom, _vlogs) = setup_with_mock_backends().await;

        // Need to add query_range mock
        let prom2 = MockServer::start().await;
        Mock::given(matchers::path("/api/v1/query_range"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "success",
                "data": { "resultType": "matrix", "result": [] }
            })))
            .mount(&prom2)
            .await;

        let config = EngineConfig {
            listen: "0.0.0.0:0".to_string(),
            backends: vec![
                BackendConfig { name: "victoria".to_string(), backend_type: "prometheus".to_string(), url: prom2.uri() },
            ],
            api_keys: vec![],
            cors_origins: vec![],
        };
        let state = Arc::new(AppState { config, cache: crate::cache::QueryCache::new(100, 15), metrics: crate::api::metrics::EngineMetrics::new(), rate_limiter: crate::rate_limit::RateLimiter::new(100) });
        let req = QueryRequest { query: "FROM metrics WHERE __name__ = \"up\" WITHIN last 1h".to_string(), format: "json".to_string(), limit: 100 };
        let result = handle_query(State(state), Json(req)).await;
        assert!(result.is_ok());
    }
}
