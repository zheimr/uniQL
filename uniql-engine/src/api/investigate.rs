//! Investigation Packs — Alert-driven multi-query execution
//!
//! When an alert fires, AETHERIS (or any caller) sends:
//!   POST /v1/investigate { "pack": "high_cpu", "params": { "host": "srv-01" } }
//!
//! The engine looks up the pack, substitutes params, executes all queries in parallel,
//! and returns a unified investigation context.
//!
//! Loose coupling: AETHERIS only knows the HTTP API, never imports UNIQL internals.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::engine::AppState;
use crate::executor::{prometheus::PrometheusExecutor, victorialogs::VictoriaLogsExecutor};
use crate::planner;
use super::ErrorResponse;

// ─── Request / Response ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct InvestigateRequest {
    /// Pack name: "high_cpu", "error_spike", "latency_degradation", or "custom"
    pub pack: String,
    /// Parameters to substitute into pack queries (e.g., host, service, start_time)
    #[serde(default)]
    pub params: HashMap<String, String>,
    /// Custom queries (when pack = "custom")
    #[serde(default)]
    pub queries: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct InvestigateResponse {
    pub status: String,
    pub pack: String,
    pub results: Vec<InvestigateResult>,
    pub total_time_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct InvestigateResult {
    pub name: String,
    pub query: String,
    pub native_query: Option<String>,
    pub status: String,
    pub data: serde_json::Value,
    pub execute_time_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ─── Built-in Packs ──────────────────────────────────────────────────────────

fn get_pack_queries(pack: &str) -> Option<Vec<(&'static str, &'static str)>> {
    match pack {
        "high_cpu" => Some(vec![
            ("host_cpu_trend", "SHOW timeseries FROM victoria WHERE __name__ = \"vsphere_host_cpu_usage_average\" AND esxhostname = \"$host\" WITHIN last 30m"),
            ("vm_cpu_on_host", "SHOW timeseries FROM victoria WHERE __name__ = \"vsphere_vm_cpu_usage_average\" AND esxhostname = \"$host\" WITHIN last 30m"),
            ("host_memory", "SHOW timeseries FROM victoria WHERE __name__ = \"vsphere_host_mem_usage_average\" AND esxhostname = \"$host\" WITHIN last 30m"),
        ]),
        "error_spike" => Some(vec![
            ("soc_event_rate", "SHOW timeseries FROM victoria WHERE __name__ = \"soc_events_processed_total\" WITHIN last 1h"),
            ("error_logs", "SHOW table FROM vlogs WHERE job = \"$service\" WITHIN last 30m"),
            ("api_errors", "SHOW timeseries FROM victoria WHERE __name__ = \"http_requests_total\" AND status =~ \"5..\" WITHIN last 1h"),
        ]),
        "latency_degradation" => Some(vec![
            ("api_latency", "SHOW timeseries FROM victoria WHERE __name__ = \"http_request_duration_seconds_sum\" AND job = \"$service\" WITHIN last 1h"),
            ("api_requests", "SHOW timeseries FROM victoria WHERE __name__ = \"http_requests_total\" AND job = \"$service\" WITHIN last 1h"),
            ("slow_logs", "SHOW table FROM vlogs WHERE job = \"$service\" WITHIN last 30m"),
        ]),
        "link_down" => Some(vec![
            ("device_status", "SHOW timeseries FROM victoria WHERE __name__ = \"snmpv2_device_up\" AND hostname = \"$host\" WITHIN last 30m"),
            ("interface_status", "SHOW timeseries FROM victoria WHERE __name__ = \"snmpv2_if_oper_status\" AND hostname = \"$host\" WITHIN last 30m"),
            ("firewall_logs", "SHOW table FROM vlogs WHERE job = \"fortigate\" WITHIN last 30m"),
        ]),
        _ => None,
    }
}

/// Substitute $param placeholders in a query string.
fn substitute_params(query: &str, params: &HashMap<String, String>) -> String {
    let mut result = query.to_string();
    for (key, value) in params {
        result = result.replace(&format!("${}", key), value);
    }
    result
}

// ─── Handler ─────────────────────────────────────────────────────────────────

pub async fn handle_investigate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InvestigateRequest>,
) -> Result<Json<InvestigateResponse>, (StatusCode, Json<ErrorResponse>)> {
    let total_start = Instant::now();

    // Resolve queries: built-in pack or custom
    let named_queries: Vec<(String, String)> = if req.pack == "custom" {
        req.queries.iter().enumerate()
            .map(|(i, q)| (format!("query_{}", i + 1), q.clone()))
            .collect()
    } else {
        let pack_templates = get_pack_queries(&req.pack).ok_or_else(|| {
            (StatusCode::BAD_REQUEST, Json(ErrorResponse {
                status: "error".to_string(),
                error: format!("Unknown investigation pack '{}'. Available: high_cpu, error_spike, latency_degradation, link_down, custom", req.pack),
                hint: Some("Use pack='custom' with a 'queries' array for ad-hoc investigation.".to_string()),
            }))
        })?;

        pack_templates.into_iter()
            .map(|(name, tmpl)| (name.to_string(), substitute_params(tmpl, &req.params)))
            .collect()
    };

    // Execute all queries in parallel
    let futures: Vec<_> = named_queries.into_iter().map(|(name, query)| {
        let state = state.clone();
        async move {
            let start = Instant::now();

            // Parse + plan
            let ast = match uniql_core::prepare(&query) {
                Ok(ast) => ast,
                Err(e) => {
                    return InvestigateResult {
                        name,
                        query,
                        native_query: None,
                        status: "error".to_string(),
                        data: serde_json::Value::Null,
                        execute_time_ms: start.elapsed().as_millis() as u64,
                        error: Some(e.to_string()),
                    };
                }
            };

            let plan = match planner::plan(&ast, &state.config) {
                Ok(p) => p,
                Err(e) => {
                    return InvestigateResult {
                        name,
                        query,
                        native_query: None,
                        status: "error".to_string(),
                        data: serde_json::Value::Null,
                        execute_time_ms: start.elapsed().as_millis() as u64,
                        error: Some(e.message),
                    };
                }
            };

            // Execute first sub-query
            if plan.sub_queries.is_empty() {
                return InvestigateResult {
                    name,
                    query,
                    native_query: None,
                    status: "error".to_string(),
                    data: serde_json::Value::Null,
                    execute_time_ms: start.elapsed().as_millis() as u64,
                    error: Some("No sub-queries in plan".to_string()),
                };
            }

            let sq = &plan.sub_queries[0];
            let native = sq.native_query.clone();

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
                        .query_range(&sq.native_query, 100, &sq.time_start, &sq.time_end).await
                }
                _ => {
                    return InvestigateResult {
                        name,
                        query,
                        native_query: Some(native),
                        status: "error".to_string(),
                        data: serde_json::Value::Null,
                        execute_time_ms: start.elapsed().as_millis() as u64,
                        error: Some(format!("Unsupported backend: {}", sq.backend_type)),
                    };
                }
            };

            match result {
                Ok(r) => InvestigateResult {
                    name,
                    query,
                    native_query: Some(native),
                    status: "success".to_string(),
                    data: r.data,
                    execute_time_ms: start.elapsed().as_millis() as u64,
                    error: None,
                },
                Err(e) => InvestigateResult {
                    name,
                    query,
                    native_query: Some(native),
                    status: "error".to_string(),
                    data: serde_json::Value::Null,
                    execute_time_ms: start.elapsed().as_millis() as u64,
                    error: Some(e.to_string()),
                },
            }
        }
    }).collect();

    let results = futures::future::join_all(futures).await;

    Ok(Json(InvestigateResponse {
        status: "success".to_string(),
        pack: req.pack,
        results,
        total_time_ms: total_start.elapsed().as_millis() as u64,
    }))
}
