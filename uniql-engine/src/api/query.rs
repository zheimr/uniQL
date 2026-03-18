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
