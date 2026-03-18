//! CORRELATE Engine — Cross-signal result correlation
//!
//! Takes results from multiple backends and joins them on shared fields + time window.
//! Strategies: TimeFieldJoin (default), FieldJoin, TraceIdJoin

use crate::executor::BackendResult;
use crate::normalize_result::NormalizedResult;
use crate::planner::CorrelationPlan;
use serde_json::Value;

/// Maximum number of correlated events to prevent memory explosion.
/// 100K metrics × 100K logs with high cardinality join keys could produce
/// billions of rows without this guard.
const MAX_CORRELATED_EVENTS: usize = 10_000;

#[derive(Debug)]
pub struct CorrelatedResult {
    pub events: Vec<CorrelatedEvent>,
    pub metadata: CorrelationMetadata,
}

#[derive(Debug, serde::Serialize)]
pub struct CorrelatedEvent {
    pub timestamp: Option<String>,
    pub join_fields: serde_json::Map<String, Value>,
    pub signals: serde_json::Map<String, Value>,
}

#[derive(Debug, serde::Serialize)]
pub struct CorrelationMetadata {
    pub strategy: String,
    pub join_fields: Vec<String>,
    pub time_window: Option<String>,
    pub metrics_count: usize,
    pub logs_count: usize,
    pub correlated_count: usize,
}

/// Correlate results from multiple backends (legacy path, kept as fallback).
#[allow(dead_code)]
pub fn correlate(
    results: &[(String, BackendResult)],
    plan: &CorrelationPlan,
) -> CorrelatedResult {
    // Separate by signal type
    let mut metrics_entries: Vec<FlatEntry> = Vec::new();
    let mut logs_entries: Vec<FlatEntry> = Vec::new();

    for (signal_type, result) in results {
        let entries = flatten_result(result, &plan.join_fields);
        match signal_type.as_str() {
            "metrics" => metrics_entries.extend(entries),
            "logs" => logs_entries.extend(entries),
            _ => logs_entries.extend(entries), // treat unknown as logs
        }
    }

    // Time window in seconds (parse duration string)
    let window_secs = plan.time_window.as_ref()
        .map(|w| parse_duration_secs(w))
        .unwrap_or(60.0);

    let skew_secs = plan.skew_tolerance.as_ref()
        .map(|s| parse_duration_secs(s))
        .unwrap_or(0.0);

    let total_window = window_secs + skew_secs;

    // Join: for each metrics entry, find matching logs entries
    let mut correlated_events = Vec::new();

    for m in &metrics_entries {
        for l in &logs_entries {
            // Check join field match
            let fields_match = plan.join_fields.iter().all(|f| {
                let mv = m.fields.get(f);
                let lv = l.fields.get(f);
                match (mv, lv) {
                    (Some(a), Some(b)) => a == b,
                    _ => false,
                }
            });

            if !fields_match {
                continue;
            }

            // Check time window
            let time_match = match (m.timestamp_epoch, l.timestamp_epoch) {
                (Some(mt), Some(lt)) => (mt - lt).abs() <= total_window,
                _ => true, // if no timestamps, match by field only
            };

            if !time_match {
                continue;
            }

            let mut join_fields = serde_json::Map::new();
            for f in &plan.join_fields {
                if let Some(v) = m.fields.get(f).or_else(|| l.fields.get(f)) {
                    join_fields.insert(f.clone(), Value::String(v.clone()));
                }
            }

            let mut signals = serde_json::Map::new();
            signals.insert("metrics".to_string(), m.raw.clone());
            signals.insert("logs".to_string(), l.raw.clone());

            correlated_events.push(CorrelatedEvent {
                timestamp: m.timestamp.clone().or_else(|| l.timestamp.clone()),
                join_fields,
                signals,
            });

            if correlated_events.len() >= MAX_CORRELATED_EVENTS {
                break;
            }
        }
        if correlated_events.len() >= MAX_CORRELATED_EVENTS {
            break;
        }
    }

    CorrelatedResult {
        metadata: CorrelationMetadata {
            strategy: "TimeFieldJoin".to_string(),
            join_fields: plan.join_fields.clone(),
            time_window: plan.time_window.clone(),
            metrics_count: metrics_entries.len(),
            logs_count: logs_entries.len(),
            correlated_count: correlated_events.len(),
        },
        events: correlated_events,
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

struct FlatEntry {
    timestamp: Option<String>,
    timestamp_epoch: Option<f64>,
    fields: std::collections::HashMap<String, String>,
    raw: Value,
}

/// Flatten a BackendResult into individual entries with extracted join fields.
#[allow(dead_code)]
fn flatten_result(result: &BackendResult, _join_fields: &[String]) -> Vec<FlatEntry> {
    let mut entries = Vec::new();

    match result.backend_type.as_str() {
        "prometheus" => {
            // Prometheus result format: { data: { result: [ { metric: {}, value: [ts, val] } ] } }
            if let Some(results_arr) = result.data
                .get("data")
                .and_then(|d| d.get("result"))
                .and_then(|r| r.as_array())
            {
                for item in results_arr {
                    let mut fields = std::collections::HashMap::new();
                    if let Some(metric) = item.get("metric").and_then(|m| m.as_object()) {
                        for (k, v) in metric {
                            if let Some(s) = v.as_str() {
                                fields.insert(k.clone(), s.to_string());
                            }
                        }
                    }

                    let (ts, ts_epoch) = if let Some(val) = item.get("value").and_then(|v| v.as_array()) {
                        let epoch = val.first().and_then(|t| t.as_f64());
                        (epoch.map(|e| format!("{}", e)), epoch)
                    } else {
                        (None, None)
                    };

                    entries.push(FlatEntry {
                        timestamp: ts,
                        timestamp_epoch: ts_epoch,
                        fields,
                        raw: item.clone(),
                    });
                }
            }
        }
        "victorialogs" => {
            // VictoriaLogs: { result: [ { _msg, _time, _stream, field1, field2, ... } ] }
            if let Some(results_arr) = result.data
                .get("result")
                .and_then(|r| r.as_array())
            {
                for item in results_arr {
                    let mut fields = std::collections::HashMap::new();
                    if let Some(obj) = item.as_object() {
                        for (k, v) in obj {
                            if let Some(s) = v.as_str() {
                                fields.insert(k.clone(), s.to_string());
                            }
                        }
                    }

                    let ts = item.get("_time").and_then(|t| t.as_str()).map(|s| s.to_string());
                    let ts_epoch = ts.as_ref().and_then(|t| crate::normalize_result::parse_timestamp_to_epoch(t));

                    entries.push(FlatEntry {
                        timestamp: ts,
                        timestamp_epoch: ts_epoch,
                        fields,
                        raw: item.clone(),
                    });
                }
            }
        }
        _ => {}
    }

    entries
}

/// Parse a duration string (e.g., "60s", "5m", "1h") to seconds.
fn parse_duration_secs(s: &str) -> f64 {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("ms") {
        num.parse::<f64>().unwrap_or(0.0) / 1000.0
    } else if let Some(num) = s.strip_suffix('s') {
        num.parse::<f64>().unwrap_or(0.0)
    } else if let Some(num) = s.strip_suffix('m') {
        num.parse::<f64>().unwrap_or(0.0) * 60.0
    } else if let Some(num) = s.strip_suffix('h') {
        num.parse::<f64>().unwrap_or(0.0) * 3600.0
    } else if let Some(num) = s.strip_suffix('d') {
        num.parse::<f64>().unwrap_or(0.0) * 86400.0
    } else {
        s.parse::<f64>().unwrap_or(60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize_result::{NormalizedResult, NormalizedRow};
    use std::collections::HashMap;

    fn make_correlation_plan(join_fields: Vec<&str>, time_window: Option<&str>) -> CorrelationPlan {
        CorrelationPlan {
            join_fields: join_fields.into_iter().map(|s| s.to_string()).collect(),
            time_window: time_window.map(|s| s.to_string()),
            skew_tolerance: None,
        }
    }

    fn make_backend_result(backend_type: &str, data: Value) -> BackendResult {
        BackendResult {
            data,
            backend_name: format!("{}_backend", backend_type),
            backend_type: backend_type.to_string(),
            native_query: "test".to_string(),
            execute_time_ms: 1,
        }
    }

    fn make_normalized_row(ts_epoch: Option<f64>, labels: Vec<(&str, &str)>, value: Option<&str>) -> NormalizedRow {
        let mut label_map = HashMap::new();
        for (k, v) in labels {
            label_map.insert(k.to_string(), v.to_string());
        }
        NormalizedRow {
            timestamp: ts_epoch.map(|e| format!("{}", e)),
            timestamp_epoch: ts_epoch,
            labels: label_map,
            value: value.map(|s| s.to_string()),
            raw: serde_json::json!({"test": true}),
        }
    }

    fn make_normalized_result(signal_type: &str, rows: Vec<NormalizedRow>) -> NormalizedResult {
        NormalizedResult {
            rows,
            backend_name: "test".to_string(),
            backend_type: "test".to_string(),
            signal_type: signal_type.to_string(),
        }
    }

    // ─── parse_duration_secs ────────────────────────────────────────────

    #[test]
    fn parse_duration_seconds() {
        assert!((parse_duration_secs("60s") - 60.0).abs() < 0.001);
    }

    #[test]
    fn parse_duration_minutes() {
        assert!((parse_duration_secs("5m") - 300.0).abs() < 0.001);
    }

    #[test]
    fn parse_duration_hours() {
        assert!((parse_duration_secs("1h") - 3600.0).abs() < 0.001);
    }

    #[test]
    fn parse_duration_days() {
        assert!((parse_duration_secs("7d") - 604800.0).abs() < 0.001);
    }

    #[test]
    fn parse_duration_milliseconds() {
        assert!((parse_duration_secs("500ms") - 0.5).abs() < 0.001);
    }

    #[test]
    fn parse_duration_bare_number_defaults_to_seconds() {
        assert!((parse_duration_secs("120") - 120.0).abs() < 0.001);
    }

    #[test]
    fn parse_duration_invalid_defaults_to_60() {
        assert!((parse_duration_secs("abc") - 60.0).abs() < 0.001);
    }

    #[test]
    fn parse_duration_whitespace_trimmed() {
        assert!((parse_duration_secs("  30s  ") - 30.0).abs() < 0.001);
    }

    // ─── correlate (legacy path) ────────────────────────────────────────

    #[test]
    fn correlate_basic_field_match() {
        // Use matching timestamps close together
        let metrics = make_backend_result("prometheus", serde_json::json!({
            "data": {"result": [
                {"metric": {"host": "srv-01"}, "value": [1000.0, "42"]}
            ]}
        }));
        let logs = make_backend_result("victorialogs", serde_json::json!({
            "result": [
                {"host": "srv-01", "_msg": "error"}
            ]
        }));

        // Use large time window so timestamps don't matter (log has no _time so epoch=None → always matches)
        let plan = make_correlation_plan(vec!["host"], Some("60s"));
        let result = correlate(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );

        assert_eq!(result.metadata.strategy, "TimeFieldJoin");
        assert!(result.events.len() > 0, "Should correlate on matching host (no timestamp = field-only match)");
        assert_eq!(result.events[0].join_fields.get("host").unwrap(), "srv-01");
        assert!(result.events[0].signals.contains_key("metrics"));
        assert!(result.events[0].signals.contains_key("logs"));
    }

    #[test]
    fn correlate_no_field_match() {
        let metrics = make_backend_result("prometheus", serde_json::json!({
            "data": {"result": [
                {"metric": {"host": "srv-01"}, "value": [1000.0, "1"]}
            ]}
        }));
        let logs = make_backend_result("victorialogs", serde_json::json!({
            "result": [
                {"host": "srv-99", "_msg": "error"}
            ]
        }));

        let plan = make_correlation_plan(vec!["host"], Some("60s"));
        let result = correlate(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn correlate_time_window_outside() {
        let metrics = make_backend_result("prometheus", serde_json::json!({
            "data": {"result": [
                {"metric": {"host": "srv-01"}, "value": [1000.0, "1"]}
            ]}
        }));
        let logs = make_backend_result("victorialogs", serde_json::json!({
            "result": [
                {"host": "srv-01", "_msg": "err", "_time": "2026-03-17T20:31:36Z"}
            ]
        }));

        // Very small time window — they won't match because epoch values are very different
        let plan = make_correlation_plan(vec!["host"], Some("1s"));
        let result = correlate(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        // The metrics entry has epoch ~1000 and the log has epoch ~1774054296
        // With 1s window they should not match
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn correlate_empty_results() {
        let metrics = make_backend_result("prometheus", serde_json::json!({"data": {"result": []}}));
        let logs = make_backend_result("victorialogs", serde_json::json!({"result": []}));

        let plan = make_correlation_plan(vec!["host"], Some("60s"));
        let result = correlate(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        assert_eq!(result.events.len(), 0);
        assert_eq!(result.metadata.metrics_count, 0);
        assert_eq!(result.metadata.logs_count, 0);
    }

    #[test]
    fn correlate_metadata_counts() {
        let metrics = make_backend_result("prometheus", serde_json::json!({
            "data": {"result": [
                {"metric": {"host": "a"}, "value": [100.0, "1"]},
                {"metric": {"host": "b"}, "value": [100.0, "2"]},
            ]}
        }));
        let logs = make_backend_result("victorialogs", serde_json::json!({
            "result": [{"host": "a", "_msg": "err"}]
        }));

        let plan = make_correlation_plan(vec!["host"], Some("99999999s"));
        let result = correlate(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        assert_eq!(result.metadata.metrics_count, 2);
        assert_eq!(result.metadata.logs_count, 1);
    }

    // ─── correlate_normalized ───────────────────────────────────────────

    #[test]
    fn correlate_normalized_basic_match() {
        let metrics_rows = vec![
            make_normalized_row(Some(1000.0), vec![("host", "srv-01")], Some("42")),
        ];
        let logs_rows = vec![
            make_normalized_row(Some(1005.0), vec![("host", "srv-01")], Some("error")),
        ];

        let metrics = make_normalized_result("metrics", metrics_rows);
        let logs = make_normalized_result("logs", logs_rows);

        let plan = make_correlation_plan(vec!["host"], Some("60s"));
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );

        assert_eq!(result.metadata.strategy, "HashTimeWindowJoin");
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].join_fields.get("host").unwrap(), "srv-01");
    }

    #[test]
    fn correlate_normalized_no_match_field() {
        let metrics = make_normalized_result("metrics", vec![
            make_normalized_row(Some(1000.0), vec![("host", "a")], None),
        ]);
        let logs = make_normalized_result("logs", vec![
            make_normalized_row(Some(1000.0), vec![("host", "b")], None),
        ]);

        let plan = make_correlation_plan(vec!["host"], Some("60s"));
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn correlate_normalized_outside_time_window() {
        let metrics = make_normalized_result("metrics", vec![
            make_normalized_row(Some(1000.0), vec![("host", "a")], None),
        ]);
        let logs = make_normalized_result("logs", vec![
            make_normalized_row(Some(2000.0), vec![("host", "a")], None),
        ]);

        let plan = make_correlation_plan(vec!["host"], Some("10s"));
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        assert_eq!(result.events.len(), 0);
    }

    #[test]
    fn correlate_normalized_multiple_matches() {
        let metrics = make_normalized_result("metrics", vec![
            make_normalized_row(Some(1000.0), vec![("host", "a")], Some("10")),
            make_normalized_row(Some(1100.0), vec![("host", "a")], Some("20")),
        ]);
        let logs = make_normalized_result("logs", vec![
            make_normalized_row(Some(1050.0), vec![("host", "a")], Some("err1")),
        ]);

        let plan = make_correlation_plan(vec!["host"], Some("60s"));
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        // Both metrics entries are within 60s of the log entry
        assert_eq!(result.events.len(), 2);
    }

    #[test]
    fn correlate_normalized_composite_join_key() {
        let metrics = make_normalized_result("metrics", vec![
            make_normalized_row(Some(1000.0), vec![("host", "a"), ("job", "api")], None),
            make_normalized_row(Some(1000.0), vec![("host", "a"), ("job", "web")], None),
        ]);
        let logs = make_normalized_result("logs", vec![
            make_normalized_row(Some(1000.0), vec![("host", "a"), ("job", "api")], None),
        ]);

        let plan = make_correlation_plan(vec!["host", "job"], Some("60s"));
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        // Only the host=a, job=api combo should match
        assert_eq!(result.events.len(), 1);
    }

    #[test]
    fn correlate_normalized_skew_tolerance() {
        let metrics = make_normalized_result("metrics", vec![
            make_normalized_row(Some(1000.0), vec![("host", "a")], None),
        ]);
        let logs = make_normalized_result("logs", vec![
            make_normalized_row(Some(1070.0), vec![("host", "a")], None),
        ]);

        // 60s window + 15s skew = 75s total — entry at 1070 is within 75s of 1000
        let mut plan = make_correlation_plan(vec!["host"], Some("60s"));
        plan.skew_tolerance = Some("15s".to_string());
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        assert_eq!(result.events.len(), 1);
    }

    #[test]
    fn correlate_normalized_empty() {
        let metrics = make_normalized_result("metrics", vec![]);
        let logs = make_normalized_result("logs", vec![]);
        let plan = make_correlation_plan(vec!["host"], Some("60s"));
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        assert_eq!(result.events.len(), 0);
        assert_eq!(result.metadata.correlated_count, 0);
    }

    #[test]
    fn correlate_normalized_no_timestamps_match_by_field() {
        let metrics = make_normalized_result("metrics", vec![
            make_normalized_row(None, vec![("host", "a")], None),
        ]);
        let logs = make_normalized_result("logs", vec![
            make_normalized_row(None, vec![("host", "a")], None),
        ]);

        let plan = make_correlation_plan(vec!["host"], Some("60s"));
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        // No timestamps, so should match by field only
        assert_eq!(result.events.len(), 1);
    }

    #[test]
    fn correlate_normalized_default_time_window() {
        let metrics = make_normalized_result("metrics", vec![
            make_normalized_row(Some(1000.0), vec![("host", "a")], None),
        ]);
        let logs = make_normalized_result("logs", vec![
            make_normalized_row(Some(1050.0), vec![("host", "a")], None),
        ]);

        // No time_window → defaults to 60s
        let plan = make_correlation_plan(vec!["host"], None);
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        assert_eq!(result.events.len(), 1);
    }

    #[test]
    fn correlate_normalized_cardinality_limit() {
        // Create enough entries to exceed MAX_CORRELATED_EVENTS
        // 200 metrics × 200 logs with same host = 40,000 potential matches
        // Should be capped at MAX_CORRELATED_EVENTS (10,000)
        let metrics_rows: Vec<NormalizedRow> = (0..200)
            .map(|i| make_normalized_row(Some(1000.0 + i as f64), vec![("host", "a")], Some("val")))
            .collect();
        let logs_rows: Vec<NormalizedRow> = (0..200)
            .map(|i| make_normalized_row(Some(1000.0 + i as f64), vec![("host", "a")], Some("log")))
            .collect();

        let metrics = make_normalized_result("metrics", metrics_rows);
        let logs = make_normalized_result("logs", logs_rows);

        // Very wide time window ensures all match
        let plan = make_correlation_plan(vec!["host"], Some("999999s"));
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("logs".to_string(), logs)],
            &plan,
        );
        assert!(result.events.len() <= super::MAX_CORRELATED_EVENTS,
            "Should be capped at {}, got {}", super::MAX_CORRELATED_EVENTS, result.events.len());
    }

    #[test]
    fn correlate_normalized_unknown_signal_treated_as_logs() {
        let metrics = make_normalized_result("metrics", vec![
            make_normalized_row(Some(1000.0), vec![("host", "a")], None),
        ]);
        let unknown = make_normalized_result("traces", vec![
            make_normalized_row(Some(1000.0), vec![("host", "a")], None),
        ]);

        let plan = make_correlation_plan(vec!["host"], Some("60s"));
        let result = correlate_normalized(
            &[("metrics".to_string(), metrics), ("traces".to_string(), unknown)],
            &plan,
        );
        // "traces" should be treated as logs
        assert_eq!(result.events.len(), 1);
    }
}

/// Correlate using pre-normalized results.
/// Accepts NormalizedResult instead of raw BackendResult, avoiding re-parsing.
pub fn correlate_normalized(
    results: &[(String, NormalizedResult)],
    plan: &CorrelationPlan,
) -> CorrelatedResult {
    let mut metrics_entries: Vec<FlatEntry> = Vec::new();
    let mut logs_entries: Vec<FlatEntry> = Vec::new();

    for (signal_type, normalized) in results {
        let entries: Vec<FlatEntry> = normalized.rows.iter().map(|row| {
            FlatEntry {
                timestamp: row.timestamp.clone(),
                timestamp_epoch: row.timestamp_epoch,
                fields: row.labels.clone(),
                raw: row.raw.clone(),
            }
        }).collect();

        match signal_type.as_str() {
            "metrics" => metrics_entries.extend(entries),
            "logs" => logs_entries.extend(entries),
            _ => logs_entries.extend(entries),
        }
    }

    let window_secs = plan.time_window.as_ref()
        .map(|w| parse_duration_secs(w))
        .unwrap_or(60.0);

    let skew_secs = plan.skew_tolerance.as_ref()
        .map(|s| parse_duration_secs(s))
        .unwrap_or(0.0);

    let total_window = window_secs + skew_secs;

    // ── Hash-Partitioned Time-Windowed Join ────────────────────────────
    // Phase 1: Build hash map from smaller side (metrics) keyed by join fields
    let build_key = |entry: &FlatEntry| -> String {
        plan.join_fields.iter()
            .map(|f| entry.fields.get(f).map(|v| v.as_str()).unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\x00") // null separator for composite key
    };

    let mut metrics_map: std::collections::HashMap<String, Vec<&FlatEntry>> =
        std::collections::HashMap::new();
    for entry in &metrics_entries {
        let key = build_key(entry);
        metrics_map.entry(key).or_default().push(entry);
    }

    // Phase 2: Sort each bucket by timestamp for binary search
    for bucket in metrics_map.values_mut() {
        bucket.sort_by(|a, b| {
            a.timestamp_epoch.unwrap_or(0.0)
                .partial_cmp(&b.timestamp_epoch.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // Phase 3: Probe with logs — O(1) hash lookup + O(log k) binary search per entry
    let mut correlated_events = Vec::new();

    for l in &logs_entries {
        let key = build_key(l);
        let Some(bucket) = metrics_map.get(&key) else { continue };

        let log_ts = l.timestamp_epoch.unwrap_or(0.0);

        // Binary search for time window: find metrics entries within [log_ts - window, log_ts + window]
        let lo = log_ts - total_window;
        let hi = log_ts + total_window;

        // Find first entry >= lo
        let start_idx = bucket.partition_point(|e| {
            e.timestamp_epoch.unwrap_or(0.0) < lo
        });

        for m in &bucket[start_idx..] {
            let m_ts = m.timestamp_epoch.unwrap_or(0.0);
            if m_ts > hi { break; } // past window, done

            // If either side has no timestamp, match by field only
            let time_ok = match (m.timestamp_epoch, l.timestamp_epoch) {
                (Some(_), Some(_)) => true, // already within window from binary search
                _ => true, // no timestamps, match by field only
            };

            if !time_ok { continue; }

            let mut join_fields = serde_json::Map::new();
            for f in &plan.join_fields {
                if let Some(v) = m.fields.get(f).or_else(|| l.fields.get(f)) {
                    join_fields.insert(f.clone(), Value::String(v.clone()));
                }
            }

            let mut signals = serde_json::Map::new();
            signals.insert("metrics".to_string(), m.raw.clone());
            signals.insert("logs".to_string(), l.raw.clone());

            correlated_events.push(CorrelatedEvent {
                timestamp: m.timestamp.clone().or_else(|| l.timestamp.clone()),
                join_fields,
                signals,
            });

            if correlated_events.len() >= MAX_CORRELATED_EVENTS {
                break;
            }
        }
        if correlated_events.len() >= MAX_CORRELATED_EVENTS {
            break;
        }
    }

    CorrelatedResult {
        metadata: CorrelationMetadata {
            strategy: "HashTimeWindowJoin".to_string(),
            join_fields: plan.join_fields.clone(),
            time_window: plan.time_window.clone(),
            metrics_count: metrics_entries.len(),
            logs_count: logs_entries.len(),
            correlated_count: correlated_events.len(),
        },
        events: correlated_events,
    }
}
