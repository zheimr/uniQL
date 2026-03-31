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

use super::ErrorResponse;
use crate::engine::AppState;
use crate::executor::{prometheus::PrometheusExecutor, victorialogs::VictoriaLogsExecutor};
use crate::planner;

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
/// Values are sanitized to prevent UNIQL injection attacks.
fn substitute_params(query: &str, params: &HashMap<String, String>) -> String {
    let mut result = query.to_string();
    for (key, value) in params {
        let sanitized = sanitize_param_value(value);
        result = result.replace(&format!("${}", key), &sanitized);
    }
    result
}

/// Sanitize a parameter value to prevent injection.
/// Strips characters that could break out of quoted strings or inject UNIQL syntax.
/// Allowlist approach: only alphanumeric, dots, hyphens, underscores, colons, slashes.
fn sanitize_param_value(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/' | ' '))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── get_pack_queries ───────────────────────────────────────────────

    #[test]
    fn pack_high_cpu_exists() {
        let queries = get_pack_queries("high_cpu").unwrap();
        assert_eq!(queries.len(), 3);
        assert!(queries.iter().any(|(name, _)| *name == "host_cpu_trend"));
        assert!(queries.iter().any(|(name, _)| *name == "vm_cpu_on_host"));
        assert!(queries.iter().any(|(name, _)| *name == "host_memory"));
    }

    #[test]
    fn pack_error_spike_exists() {
        let queries = get_pack_queries("error_spike").unwrap();
        assert_eq!(queries.len(), 3);
        assert!(queries.iter().any(|(name, _)| *name == "soc_event_rate"));
        assert!(queries.iter().any(|(name, _)| *name == "error_logs"));
        assert!(queries.iter().any(|(name, _)| *name == "api_errors"));
    }

    #[test]
    fn pack_latency_degradation_exists() {
        let queries = get_pack_queries("latency_degradation").unwrap();
        assert_eq!(queries.len(), 3);
    }

    #[test]
    fn pack_link_down_exists() {
        let queries = get_pack_queries("link_down").unwrap();
        assert_eq!(queries.len(), 3);
    }

    #[test]
    fn pack_unknown_returns_none() {
        assert!(get_pack_queries("nonexistent_pack").is_none());
    }

    #[test]
    fn pack_custom_returns_none() {
        // "custom" is handled differently, not via get_pack_queries
        assert!(get_pack_queries("custom").is_none());
    }

    // ─── substitute_params ──────────────────────────────────────────────

    #[test]
    fn substitute_single_param() {
        let result = substitute_params(
            "WHERE host = \"$host\"",
            &HashMap::from([("host".to_string(), "srv-01".to_string())]),
        );
        assert_eq!(result, "WHERE host = \"srv-01\"");
    }

    #[test]
    fn substitute_multiple_params() {
        let mut params = HashMap::new();
        params.insert("host".to_string(), "srv-01".to_string());
        params.insert("service".to_string(), "nginx".to_string());
        let result = substitute_params("WHERE host = \"$host\" AND job = \"$service\"", &params);
        assert!(result.contains("srv-01"));
        assert!(result.contains("nginx"));
        assert!(!result.contains("$host"));
        assert!(!result.contains("$service"));
    }

    #[test]
    fn substitute_no_params() {
        let result = substitute_params("FROM metrics WHERE __name__ = \"up\"", &HashMap::new());
        assert_eq!(result, "FROM metrics WHERE __name__ = \"up\"");
    }

    #[test]
    fn substitute_param_not_in_query() {
        let mut params = HashMap::new();
        params.insert("notused".to_string(), "value".to_string());
        let result = substitute_params("FROM metrics", &params);
        assert_eq!(result, "FROM metrics");
    }

    #[test]
    fn substitute_param_appears_multiple_times() {
        let mut params = HashMap::new();
        params.insert("host".to_string(), "srv-01".to_string());
        let result = substitute_params("$host and $host", &params);
        assert_eq!(result, "srv-01 and srv-01");
    }

    // ─── Pack queries contain $param placeholders ───────────────────────

    #[test]
    fn high_cpu_queries_use_host_param() {
        let queries = get_pack_queries("high_cpu").unwrap();
        for (_, query) in &queries {
            assert!(
                query.contains("$host"),
                "Query should contain $host: {}",
                query
            );
        }
    }

    #[test]
    fn error_spike_queries_use_service_param() {
        let queries = get_pack_queries("error_spike").unwrap();
        let has_service = queries.iter().any(|(_, q)| q.contains("$service"));
        assert!(
            has_service,
            "At least one error_spike query should use $service"
        );
    }

    #[test]
    fn link_down_has_firewall_logs() {
        let queries = get_pack_queries("link_down").unwrap();
        let has_vlogs = queries.iter().any(|(_, q)| q.contains("vlogs"));
        assert!(
            has_vlogs,
            "link_down should include VLogs query for firewall logs"
        );
    }

    #[test]
    fn latency_queries_use_service_param() {
        let queries = get_pack_queries("latency_degradation").unwrap();
        let has_service = queries.iter().any(|(_, q)| q.contains("$service"));
        assert!(has_service, "latency_degradation should use $service");
    }

    #[test]
    fn link_down_queries_use_host_param() {
        let queries = get_pack_queries("link_down").unwrap();
        let has_host = queries.iter().any(|(_, q)| q.contains("$host"));
        assert!(has_host, "link_down should use $host");
    }

    #[test]
    fn all_packs_have_3_queries() {
        for pack in &[
            "high_cpu",
            "error_spike",
            "latency_degradation",
            "link_down",
        ] {
            let queries = get_pack_queries(pack).unwrap();
            assert_eq!(queries.len(), 3, "Pack '{}' should have 3 queries", pack);
        }
    }

    #[test]
    fn all_packs_have_unique_query_names() {
        for pack in &[
            "high_cpu",
            "error_spike",
            "latency_degradation",
            "link_down",
        ] {
            let queries = get_pack_queries(pack).unwrap();
            let names: Vec<&str> = queries.iter().map(|(n, _)| *n).collect();
            let unique: std::collections::HashSet<&str> = names.iter().copied().collect();
            assert_eq!(
                names.len(),
                unique.len(),
                "Pack '{}' has duplicate query names",
                pack
            );
        }
    }

    #[test]
    fn substitute_empty_value() {
        let mut params = HashMap::new();
        params.insert("host".to_string(), "".to_string());
        let result = substitute_params("host = \"$host\"", &params);
        assert_eq!(result, "host = \"\"");
    }

    #[test]
    fn substitute_special_chars_in_value() {
        let mut params = HashMap::new();
        params.insert("host".to_string(), "srv-01.example.com".to_string());
        let result = substitute_params("host = \"$host\"", &params);
        assert!(result.contains("srv-01.example.com"));
    }

    // ─── Injection prevention ─────────────────────────────────────────

    #[test]
    fn sanitize_strips_quotes() {
        let result = sanitize_param_value(r#"" OR service = "nginx"#);
        assert!(
            !result.contains('"'),
            "Quotes should be stripped: {}",
            result
        );
        assert!(
            !result.contains('='),
            "Operators should be stripped: {}",
            result
        );
    }

    #[test]
    fn sanitize_strips_dangerous_chars() {
        let result = sanitize_param_value(r#"test\path"#);
        assert!(!result.contains('\\'), "Backslashes stripped: {}", result);
        assert_eq!(result, "testpath");
    }

    #[test]
    fn sanitize_newlines_removed() {
        let result = sanitize_param_value("line1\nline2\rline3");
        assert!(!result.contains('\n'));
        assert!(!result.contains('\r'));
    }

    #[test]
    fn sanitize_normal_value_unchanged() {
        let result = sanitize_param_value("esxi-node01.example.com");
        assert_eq!(result, "esxi-node01.example.com");
    }

    #[test]
    fn sanitize_allows_safe_chars() {
        assert_eq!(sanitize_param_value("host-01_prod"), "host-01_prod");
        assert_eq!(sanitize_param_value("10.0.1.50:9090"), "10.0.1.50:9090");
        assert_eq!(sanitize_param_value("/api/v1"), "/api/v1");
    }

    #[test]
    fn substitute_with_injection_attempt() {
        let mut params = HashMap::new();
        params.insert("host".to_string(), r#"" OR service = "nginx"#.to_string());
        let result = substitute_params("WHERE host = \"$host\"", &params);
        // Dangerous chars (quotes, =, \) stripped, only safe chars remain
        assert!(
            !result.contains(r#"""#) || result.ends_with('"'),
            "No unmatched quotes: {}",
            result
        );
        // The sanitized value should not contain query operators
        let sanitized = sanitize_param_value(r#"" OR service = "nginx"#);
        assert!(!sanitized.contains('"'));
        assert!(!sanitized.contains('='));
    }

    #[test]
    fn all_pack_queries_contain_within() {
        // All AETHERIS packs should have WITHIN for time-bounded queries
        for pack in &[
            "high_cpu",
            "error_spike",
            "latency_degradation",
            "link_down",
        ] {
            let queries = get_pack_queries(pack).unwrap();
            for (name, query) in &queries {
                assert!(
                    query.contains("WITHIN"),
                    "Pack '{}' query '{}' missing WITHIN clause",
                    pack,
                    name
                );
            }
        }
    }

    #[test]
    fn pack_queries_are_valid_uniql() {
        // All pack templates (with params substituted) should be parseable
        let packs = vec![
            "high_cpu",
            "error_spike",
            "latency_degradation",
            "link_down",
        ];
        let mut params = HashMap::new();
        params.insert("host".to_string(), "test-host".to_string());
        params.insert("service".to_string(), "test-service".to_string());

        for pack in packs {
            let queries = get_pack_queries(pack).unwrap();
            for (name, tmpl) in &queries {
                let query = substitute_params(tmpl, &params);
                let result = uniql_core::parse(&query);
                assert!(
                    result.is_ok(),
                    "Pack '{}' query '{}' failed to parse: {:?}\nQuery: {}",
                    pack,
                    name,
                    result.err(),
                    query
                );
            }
        }
    }
}

// ─── Handler ─────────────────────────────────────────────────────────────────

pub async fn handle_investigate(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InvestigateRequest>,
) -> Result<Json<InvestigateResponse>, (StatusCode, Json<ErrorResponse>)> {
    let total_start = Instant::now();

    // Resolve queries: built-in pack or custom
    let named_queries: Vec<(String, String)> = if req.pack == "custom" {
        req.queries
            .iter()
            .enumerate()
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

        pack_templates
            .into_iter()
            .map(|(name, tmpl)| (name.to_string(), substitute_params(tmpl, &req.params)))
            .collect()
    };

    // Execute all queries in parallel
    let futures: Vec<_> = named_queries
        .into_iter()
        .map(|(name, query)| {
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
                            executor
                                .query_range(
                                    &sq.native_query,
                                    &sq.time_start,
                                    &sq.time_end,
                                    &sq.step,
                                )
                                .await
                        } else {
                            executor.query(&sq.native_query).await
                        }
                    }
                    "victorialogs" => {
                        VictoriaLogsExecutor::new(&sq.backend_name, &sq.backend_url)
                            .query_range(&sq.native_query, 100, &sq.time_start, &sq.time_end)
                            .await
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
        })
        .collect();

    let results = futures::future::join_all(futures).await;

    Ok(Json(InvestigateResponse {
        status: "success".to_string(),
        pack: req.pack,
        results,
        total_time_ms: total_start.elapsed().as_millis() as u64,
    }))
}
