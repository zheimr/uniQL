//! Result Normalizer — Normalizes backend-specific JSON into a uniform schema.
//!
//! Moves backend-specific JSON parsing out of the correlator.
//! Implements proper timestamp parsing (replaces stub that returned None).

use crate::executor::BackendResult;
use serde_json::Value;

// ─── Normalized Result ───────────────────────────────────────────────────────

/// Uniform result format: all backends produce the same row schema.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NormalizedResult {
    pub rows: Vec<NormalizedRow>,
    pub backend_name: String,
    pub backend_type: String,
    pub signal_type: String,
}

/// A single result row with uniform fields.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NormalizedRow {
    pub timestamp: Option<String>,
    pub timestamp_epoch: Option<f64>,
    pub labels: std::collections::HashMap<String, String>,
    pub value: Option<String>,
    pub raw: Value,
}

// ─── Result Normalizer Trait ─────────────────────────────────────────────────

pub trait ResultNormalizer {
    fn normalize(&self, result: &BackendResult, signal_type: &str) -> NormalizedResult;
}

// ─── Prometheus Result Normalizer ────────────────────────────────────────────

pub struct PrometheusResultNormalizer;

impl ResultNormalizer for PrometheusResultNormalizer {
    fn normalize(&self, result: &BackendResult, signal_type: &str) -> NormalizedResult {
        let mut rows = Vec::new();

        if let Some(results_arr) = result
            .data
            .get("data")
            .and_then(|d| d.get("result"))
            .and_then(|r| r.as_array())
        {
            for item in results_arr {
                let mut labels = std::collections::HashMap::new();
                if let Some(metric) = item.get("metric").and_then(|m| m.as_object()) {
                    for (k, v) in metric {
                        if let Some(s) = v.as_str() {
                            labels.insert(k.clone(), s.to_string());
                        }
                    }
                }

                let (ts, ts_epoch, value) =
                    if let Some(val) = item.get("value").and_then(|v| v.as_array()) {
                        let epoch = val.first().and_then(|t| t.as_f64());
                        let val_str = val.get(1).and_then(|v| v.as_str()).map(|s| s.to_string());
                        (epoch.map(|e| format!("{}", e)), epoch, val_str)
                    } else {
                        (None, None, None)
                    };

                rows.push(NormalizedRow {
                    timestamp: ts,
                    timestamp_epoch: ts_epoch,
                    labels,
                    value,
                    raw: item.clone(),
                });
            }
        }

        NormalizedResult {
            rows,
            backend_name: result.backend_name.clone(),
            backend_type: result.backend_type.clone(),
            signal_type: signal_type.to_string(),
        }
    }
}

// ─── VictoriaLogs Result Normalizer ──────────────────────────────────────────

pub struct VictoriaLogsResultNormalizer;

impl ResultNormalizer for VictoriaLogsResultNormalizer {
    fn normalize(&self, result: &BackendResult, signal_type: &str) -> NormalizedResult {
        let mut rows = Vec::new();

        if let Some(results_arr) = result.data.get("result").and_then(|r| r.as_array()) {
            for item in results_arr {
                let mut labels = std::collections::HashMap::new();
                if let Some(obj) = item.as_object() {
                    for (k, v) in obj {
                        if let Some(s) = v.as_str() {
                            labels.insert(k.clone(), s.to_string());
                        }
                    }
                }

                let ts_str = item
                    .get("_time")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string());
                let ts_epoch = ts_str.as_ref().and_then(|t| parse_timestamp_to_epoch(t));
                let msg = item
                    .get("_msg")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string());

                rows.push(NormalizedRow {
                    timestamp: ts_str,
                    timestamp_epoch: ts_epoch,
                    labels,
                    value: msg,
                    raw: item.clone(),
                });
            }
        }

        NormalizedResult {
            rows,
            backend_name: result.backend_name.clone(),
            backend_type: result.backend_type.clone(),
            signal_type: signal_type.to_string(),
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Get the appropriate normalizer for a backend type.
pub fn get_normalizer(backend_type: &str) -> Box<dyn ResultNormalizer> {
    match backend_type {
        "prometheus" | "victoriametrics" => Box::new(PrometheusResultNormalizer),
        "victorialogs" => Box::new(VictoriaLogsResultNormalizer),
        _ => Box::new(PrometheusResultNormalizer), // default
    }
}

/// Parse an RFC3339 timestamp to epoch seconds.
/// Handles common formats: "2026-03-17T20:31:36.906Z", "2026-03-17T20:31:36Z"
/// Replaces the stub in correlate/mod.rs that returned None.
pub fn parse_timestamp_to_epoch(ts: &str) -> Option<f64> {
    // Parse RFC3339: YYYY-MM-DDThh:mm:ss[.frac]Z or with timezone offset
    let ts = ts.trim();

    // Find the 'T' separator
    let t_pos = ts.find('T')?;
    let date_part = &ts[..t_pos];
    let time_part = &ts[t_pos + 1..];

    // Parse date: YYYY-MM-DD
    let date_parts: Vec<&str> = date_part.split('-').collect();
    if date_parts.len() != 3 {
        return None;
    }
    let year: i64 = date_parts[0].parse().ok()?;
    let month: i64 = date_parts[1].parse().ok()?;
    let day: i64 = date_parts[2].parse().ok()?;

    // Strip timezone suffix to get time
    let (time_str, tz_offset_secs) = if let Some(stripped) = time_part.strip_suffix('Z') {
        (stripped, 0i64)
    } else if let Some(plus_pos) = time_part.rfind('+') {
        let tz = &time_part[plus_pos + 1..];
        let tz_parts: Vec<&str> = tz.split(':').collect();
        let hours: i64 = tz_parts.first().and_then(|h| h.parse().ok()).unwrap_or(0);
        let mins: i64 = tz_parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);
        (&time_part[..plus_pos], (hours * 3600 + mins * 60))
    } else if let Some(minus_pos) = time_part.rfind('-') {
        // Only if it looks like a timezone offset (after seconds)
        if minus_pos > 5 {
            let tz = &time_part[minus_pos + 1..];
            let tz_parts: Vec<&str> = tz.split(':').collect();
            let hours: i64 = tz_parts.first().and_then(|h| h.parse().ok()).unwrap_or(0);
            let mins: i64 = tz_parts.get(1).and_then(|m| m.parse().ok()).unwrap_or(0);
            (&time_part[..minus_pos], -(hours * 3600 + mins * 60))
        } else {
            (time_part, 0i64)
        }
    } else {
        (time_part, 0i64)
    };

    // Parse time: hh:mm:ss[.frac]
    let time_parts: Vec<&str> = time_str.split(':').collect();
    if time_parts.len() < 3 {
        return None;
    }
    let hour: i64 = time_parts[0].parse().ok()?;
    let minute: i64 = time_parts[1].parse().ok()?;
    let sec_part = time_parts[2];
    let (seconds, frac): (i64, f64) = if let Some(dot_pos) = sec_part.find('.') {
        let secs: i64 = sec_part[..dot_pos].parse().ok()?;
        let frac_str = &sec_part[dot_pos..]; // includes the dot
        let frac: f64 = frac_str.parse().unwrap_or(0.0);
        (secs, frac)
    } else {
        (sec_part.parse().ok()?, 0.0)
    };

    // Calculate days since epoch using a simplified algorithm
    // Based on Howard Hinnant's algorithm
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let yoe = y - era * 400;
    let m_adj = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m_adj + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;

    let total_secs = days * 86400 + hour * 3600 + minute * 60 + seconds - tz_offset_secs;
    Some(total_secs as f64 + frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamp_utc() {
        let epoch = parse_timestamp_to_epoch("2026-03-17T20:31:36Z").unwrap();
        // 2026-03-17T20:31:36Z should be a valid epoch
        assert!(epoch > 1_700_000_000.0);
        assert!(epoch < 2_000_000_000.0);
    }

    #[test]
    fn test_parse_timestamp_fractional() {
        let epoch = parse_timestamp_to_epoch("2026-03-17T20:31:36.906Z").unwrap();
        let epoch_int = parse_timestamp_to_epoch("2026-03-17T20:31:36Z").unwrap();
        assert!((epoch - epoch_int - 0.906).abs() < 0.001);
    }

    #[test]
    fn test_parse_timestamp_none_for_invalid() {
        assert!(parse_timestamp_to_epoch("not-a-timestamp").is_none());
        assert!(parse_timestamp_to_epoch("").is_none());
    }

    #[test]
    fn test_parse_known_epoch() {
        // 1970-01-01T00:00:00Z = epoch 0
        let epoch = parse_timestamp_to_epoch("1970-01-01T00:00:00Z").unwrap();
        assert!((epoch - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_known_epoch_2() {
        // 2000-01-01T00:00:00Z = epoch 946684800
        let epoch = parse_timestamp_to_epoch("2000-01-01T00:00:00Z").unwrap();
        assert!((epoch - 946684800.0).abs() < 1.0);
    }

    #[test]
    fn test_parse_timestamp_positive_tz_offset() {
        // 2000-01-01T03:00:00+03:00 should equal epoch 946684800 (same as midnight UTC)
        let epoch = parse_timestamp_to_epoch("2000-01-01T03:00:00+03:00").unwrap();
        assert!((epoch - 946684800.0).abs() < 1.0);
    }

    #[test]
    fn test_parse_timestamp_negative_tz_offset() {
        // 1999-12-31T19:00:00-05:00 should equal epoch 946684800
        let epoch = parse_timestamp_to_epoch("1999-12-31T19:00:00-05:00").unwrap();
        assert!((epoch - 946684800.0).abs() < 1.0);
    }

    #[test]
    fn test_parse_timestamp_whitespace_trimmed() {
        let epoch = parse_timestamp_to_epoch("  1970-01-01T00:00:00Z  ").unwrap();
        assert!((epoch - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_timestamp_missing_t_separator() {
        assert!(parse_timestamp_to_epoch("2026-03-17 20:31:36Z").is_none());
    }

    #[test]
    fn test_parse_timestamp_incomplete_time() {
        assert!(parse_timestamp_to_epoch("2026-03-17T20:31").is_none());
    }

    // ─── get_normalizer ─────────────────────────────────────────────────

    #[test]
    fn get_normalizer_prometheus() {
        let _n = get_normalizer("prometheus");
        let _n2 = get_normalizer("victoriametrics");
    }

    #[test]
    fn get_normalizer_victorialogs() {
        let _n = get_normalizer("victorialogs");
    }

    #[test]
    fn get_normalizer_unknown_defaults_to_prometheus() {
        let _n = get_normalizer("unknown_backend");
    }

    // ─── PrometheusResultNormalizer ─────────────────────────────────────

    #[test]
    fn prometheus_normalizer_basic() {
        let result = BackendResult {
            data: serde_json::json!({
                "data": {
                    "resultType": "vector",
                    "result": [
                        {
                            "metric": {"__name__": "up", "instance": "localhost:9090"},
                            "value": [1710000000.0, "1"]
                        }
                    ]
                }
            }),
            backend_name: "victoria".to_string(),
            backend_type: "prometheus".to_string(),
            native_query: "up".to_string(),
            execute_time_ms: 10,
        };

        let normalizer = PrometheusResultNormalizer;
        let normalized = normalizer.normalize(&result, "metrics");

        assert_eq!(normalized.backend_name, "victoria");
        assert_eq!(normalized.backend_type, "prometheus");
        assert_eq!(normalized.signal_type, "metrics");
        assert_eq!(normalized.rows.len(), 1);

        let row = &normalized.rows[0];
        assert_eq!(row.labels.get("__name__").unwrap(), "up");
        assert_eq!(row.labels.get("instance").unwrap(), "localhost:9090");
        assert_eq!(row.value.as_deref(), Some("1"));
        assert!((row.timestamp_epoch.unwrap() - 1710000000.0).abs() < 0.001);
    }

    #[test]
    fn prometheus_normalizer_empty_result() {
        let result = BackendResult {
            data: serde_json::json!({"data": {"result": []}}),
            backend_name: "vm".to_string(),
            backend_type: "prometheus".to_string(),
            native_query: "nonexistent".to_string(),
            execute_time_ms: 1,
        };
        let normalized = PrometheusResultNormalizer.normalize(&result, "metrics");
        assert_eq!(normalized.rows.len(), 0);
    }

    #[test]
    fn prometheus_normalizer_no_data_field() {
        let result = BackendResult {
            data: serde_json::json!({"status": "error"}),
            backend_name: "vm".to_string(),
            backend_type: "prometheus".to_string(),
            native_query: "bad".to_string(),
            execute_time_ms: 1,
        };
        let normalized = PrometheusResultNormalizer.normalize(&result, "metrics");
        assert_eq!(normalized.rows.len(), 0);
    }

    #[test]
    fn prometheus_normalizer_multiple_results() {
        let result = BackendResult {
            data: serde_json::json!({
                "data": {
                    "result": [
                        {"metric": {"host": "a"}, "value": [100.0, "10"]},
                        {"metric": {"host": "b"}, "value": [200.0, "20"]},
                        {"metric": {"host": "c"}, "value": [300.0, "30"]},
                    ]
                }
            }),
            backend_name: "vm".to_string(),
            backend_type: "prometheus".to_string(),
            native_query: "cpu".to_string(),
            execute_time_ms: 5,
        };
        let normalized = PrometheusResultNormalizer.normalize(&result, "metrics");
        assert_eq!(normalized.rows.len(), 3);
        assert_eq!(normalized.rows[0].labels.get("host").unwrap(), "a");
        assert_eq!(normalized.rows[1].value.as_deref(), Some("20"));
        assert!((normalized.rows[2].timestamp_epoch.unwrap() - 300.0).abs() < 0.001);
    }

    #[test]
    fn prometheus_normalizer_no_value_field() {
        let result = BackendResult {
            data: serde_json::json!({
                "data": {"result": [{"metric": {"host": "x"}}]}
            }),
            backend_name: "vm".to_string(),
            backend_type: "prometheus".to_string(),
            native_query: "q".to_string(),
            execute_time_ms: 1,
        };
        let normalized = PrometheusResultNormalizer.normalize(&result, "metrics");
        assert_eq!(normalized.rows.len(), 1);
        assert!(normalized.rows[0].timestamp.is_none());
        assert!(normalized.rows[0].value.is_none());
    }

    // ─── VictoriaLogsResultNormalizer ───────────────────────────────────

    #[test]
    fn victorialogs_normalizer_basic() {
        let result = BackendResult {
            data: serde_json::json!({
                "result": [
                    {
                        "_msg": "Connection refused",
                        "_time": "2026-03-17T20:31:36Z",
                        "host": "srv-01",
                        "job": "nginx"
                    }
                ]
            }),
            backend_name: "vlogs".to_string(),
            backend_type: "victorialogs".to_string(),
            native_query: "error".to_string(),
            execute_time_ms: 5,
        };

        let normalizer = VictoriaLogsResultNormalizer;
        let normalized = normalizer.normalize(&result, "logs");

        assert_eq!(normalized.backend_name, "vlogs");
        assert_eq!(normalized.signal_type, "logs");
        assert_eq!(normalized.rows.len(), 1);

        let row = &normalized.rows[0];
        assert_eq!(row.value.as_deref(), Some("Connection refused"));
        assert_eq!(row.timestamp.as_deref(), Some("2026-03-17T20:31:36Z"));
        assert!(row.timestamp_epoch.is_some());
        assert_eq!(row.labels.get("host").unwrap(), "srv-01");
        assert_eq!(row.labels.get("job").unwrap(), "nginx");
    }

    #[test]
    fn victorialogs_normalizer_empty() {
        let result = BackendResult {
            data: serde_json::json!({"result": []}),
            backend_name: "vlogs".to_string(),
            backend_type: "victorialogs".to_string(),
            native_query: "q".to_string(),
            execute_time_ms: 1,
        };
        let normalized = VictoriaLogsResultNormalizer.normalize(&result, "logs");
        assert_eq!(normalized.rows.len(), 0);
    }

    #[test]
    fn victorialogs_normalizer_no_time_field() {
        let result = BackendResult {
            data: serde_json::json!({"result": [{"_msg": "hello"}]}),
            backend_name: "vlogs".to_string(),
            backend_type: "victorialogs".to_string(),
            native_query: "q".to_string(),
            execute_time_ms: 1,
        };
        let normalized = VictoriaLogsResultNormalizer.normalize(&result, "logs");
        assert_eq!(normalized.rows.len(), 1);
        assert!(normalized.rows[0].timestamp.is_none());
        assert!(normalized.rows[0].timestamp_epoch.is_none());
    }

    #[test]
    fn victorialogs_normalizer_no_msg_field() {
        let result = BackendResult {
            data: serde_json::json!({"result": [{"_time": "2026-03-17T20:31:36Z", "host": "x"}]}),
            backend_name: "vlogs".to_string(),
            backend_type: "victorialogs".to_string(),
            native_query: "q".to_string(),
            execute_time_ms: 1,
        };
        let normalized = VictoriaLogsResultNormalizer.normalize(&result, "logs");
        assert_eq!(normalized.rows.len(), 1);
        assert!(normalized.rows[0].value.is_none());
    }

    #[test]
    fn victorialogs_normalizer_multiple_results() {
        let result = BackendResult {
            data: serde_json::json!({
                "result": [
                    {"_msg": "a", "_time": "2026-03-17T10:00:00Z"},
                    {"_msg": "b", "_time": "2026-03-17T11:00:00Z"},
                ]
            }),
            backend_name: "vlogs".to_string(),
            backend_type: "victorialogs".to_string(),
            native_query: "q".to_string(),
            execute_time_ms: 3,
        };
        let normalized = VictoriaLogsResultNormalizer.normalize(&result, "logs");
        assert_eq!(normalized.rows.len(), 2);
        assert_eq!(normalized.rows[0].value.as_deref(), Some("a"));
        assert_eq!(normalized.rows[1].value.as_deref(), Some("b"));
    }

    #[test]
    fn victorialogs_normalizer_no_result_key() {
        let result = BackendResult {
            data: serde_json::json!({"status": "error"}),
            backend_name: "vlogs".to_string(),
            backend_type: "victorialogs".to_string(),
            native_query: "q".to_string(),
            execute_time_ms: 1,
        };
        let normalized = VictoriaLogsResultNormalizer.normalize(&result, "logs");
        assert_eq!(normalized.rows.len(), 0);
    }
}
