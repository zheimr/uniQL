//! CORRELATE Engine — Cross-signal result correlation
//!
//! Takes results from multiple backends and joins them on shared fields + time window.
//! Strategies: TimeFieldJoin (default), FieldJoin, TraceIdJoin

use crate::executor::BackendResult;
use crate::normalize_result::NormalizedResult;
use crate::planner::CorrelationPlan;
use serde_json::Value;

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
