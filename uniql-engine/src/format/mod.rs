//! Response Formatter — Applies SHOW clause and format parameter to results.
//!
//! SHOW table → tabular structure
//! SHOW count → single number
//! Default JSON passthrough (backward compatible)
//! Applies limit

use serde_json::Value;

/// Format specification for response shaping.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FormatSpec {
    pub show_format: Option<String>,   // from SHOW clause: "table", "count", "timeseries", etc.
    pub output_format: String,         // from request: "json" (default)
    pub limit: u32,
}

impl Default for FormatSpec {
    fn default() -> Self {
        FormatSpec {
            show_format: None,
            output_format: "json".to_string(),
            limit: 100,
        }
    }
}

/// Format response data based on the SHOW clause and format parameter.
pub fn format_response(data: &Value, spec: &FormatSpec) -> Value {
    let data = apply_limit(data, spec.limit);

    match spec.show_format.as_deref() {
        Some("table") => format_as_table(&data),
        Some("count") => format_as_count(&data),
        _ => data, // default: JSON passthrough
    }
}

/// Apply result limit to the response data.
fn apply_limit(data: &Value, limit: u32) -> Value {
    let limit = limit as usize;

    // Try Prometheus format: data.result[]
    if let Some(results) = data
        .get("data")
        .and_then(|d| d.get("result"))
        .and_then(|r| r.as_array())
    {
        if results.len() > limit {
            let mut cloned = data.clone();
            if let Some(result_arr) = cloned
                .get_mut("data")
                .and_then(|d| d.get_mut("result"))
                .and_then(|r| r.as_array_mut())
            {
                result_arr.truncate(limit);
            }
            return cloned;
        }
    }

    // Try VictoriaLogs format: result[]
    if let Some(results) = data.get("result").and_then(|r| r.as_array()) {
        if results.len() > limit {
            let mut cloned = data.clone();
            if let Some(result_arr) = cloned
                .get_mut("result")
                .and_then(|r| r.as_array_mut())
            {
                result_arr.truncate(limit);
            }
            return cloned;
        }
    }

    // Try correlated format: correlated_events[]
    if let Some(events) = data.get("correlated_events").and_then(|e| e.as_array()) {
        if events.len() > limit {
            let mut cloned = data.clone();
            if let Some(events_arr) = cloned
                .get_mut("correlated_events")
                .and_then(|e| e.as_array_mut())
            {
                events_arr.truncate(limit);
            }
            return cloned;
        }
    }

    data.clone()
}

/// Format result as a table structure.
fn format_as_table(data: &Value) -> Value {
    // Extract rows from the result
    let rows: Vec<Value> = extract_rows(data);
    if rows.is_empty() {
        return serde_json::json!({
            "format": "table",
            "columns": [],
            "rows": [],
        });
    }

    // Collect all unique column names from all rows
    let mut columns = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in &rows {
        if let Some(obj) = row.as_object() {
            for key in obj.keys() {
                if seen.insert(key.clone()) {
                    columns.push(key.clone());
                }
            }
        }
    }

    // Build tabular rows
    let table_rows: Vec<Vec<Value>> = rows.iter().map(|row| {
        columns.iter().map(|col| {
            row.get(col).cloned().unwrap_or(Value::Null)
        }).collect()
    }).collect();

    serde_json::json!({
        "format": "table",
        "columns": columns,
        "rows": table_rows,
    })
}

/// Format result as a single count value.
fn format_as_count(data: &Value) -> Value {
    let rows = extract_rows(data);
    serde_json::json!({
        "format": "count",
        "count": rows.len(),
    })
}

/// Extract result rows from various data formats.
fn extract_rows(data: &Value) -> Vec<Value> {
    // Prometheus: data.result[]
    if let Some(results) = data
        .get("data")
        .and_then(|d| d.get("result"))
        .and_then(|r| r.as_array())
    {
        return results.clone();
    }

    // VictoriaLogs: result[]
    if let Some(results) = data.get("result").and_then(|r| r.as_array()) {
        return results.clone();
    }

    // Correlated: correlated_events[]
    if let Some(events) = data.get("correlated_events").and_then(|e| e.as_array()) {
        return events.clone();
    }

    // Array at top level
    if let Some(arr) = data.as_array() {
        return arr.clone();
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ─── format_response ────────────────────────────────────────────────

    #[test]
    fn format_response_default_passthrough() {
        let data = json!({"data": {"result": [{"metric": {"__name__": "up"}, "value": [1, "1"]}]}});
        let spec = FormatSpec::default();
        let result = format_response(&data, &spec);
        assert_eq!(result, data);
    }

    #[test]
    fn format_response_show_table() {
        let data = json!({"data": {"result": [
            {"metric": {"host": "a"}, "value": [1, "10"]},
            {"metric": {"host": "b"}, "value": [2, "20"]},
        ]}});
        let spec = FormatSpec { show_format: Some("table".to_string()), ..Default::default() };
        let result = format_response(&data, &spec);
        assert_eq!(result["format"], "table");
        assert!(result["columns"].as_array().unwrap().len() > 0);
        assert_eq!(result["rows"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn format_response_show_count() {
        let data = json!({"data": {"result": [
            {"metric": {"host": "a"}},
            {"metric": {"host": "b"}},
            {"metric": {"host": "c"}},
        ]}});
        let spec = FormatSpec { show_format: Some("count".to_string()), ..Default::default() };
        let result = format_response(&data, &spec);
        assert_eq!(result["format"], "count");
        assert_eq!(result["count"], 3);
    }

    #[test]
    fn format_response_unknown_show_format_passthrough() {
        let data = json!({"foo": "bar"});
        let spec = FormatSpec { show_format: Some("unknown".to_string()), ..Default::default() };
        let result = format_response(&data, &spec);
        assert_eq!(result, data);
    }

    // ─── apply_limit ────────────────────────────────────────────────────

    #[test]
    fn apply_limit_prometheus_format() {
        let data = json!({"data": {"result": [
            {"metric": {"a": "1"}},
            {"metric": {"a": "2"}},
            {"metric": {"a": "3"}},
        ]}});
        let result = apply_limit(&data, 2);
        assert_eq!(result["data"]["result"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn apply_limit_prometheus_no_truncate_when_under() {
        let data = json!({"data": {"result": [{"metric": {"a": "1"}}]}});
        let result = apply_limit(&data, 10);
        assert_eq!(result["data"]["result"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn apply_limit_victorialogs_format() {
        let data = json!({"result": [
            {"_msg": "log1"}, {"_msg": "log2"}, {"_msg": "log3"}
        ]});
        let result = apply_limit(&data, 1);
        assert_eq!(result["result"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn apply_limit_victorialogs_no_truncate() {
        let data = json!({"result": [{"_msg": "log1"}]});
        let result = apply_limit(&data, 100);
        assert_eq!(result["result"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn apply_limit_correlated_format() {
        let data = json!({"correlated_events": [
            {"ts": "1"}, {"ts": "2"}, {"ts": "3"}, {"ts": "4"}
        ]});
        let result = apply_limit(&data, 2);
        assert_eq!(result["correlated_events"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn apply_limit_correlated_no_truncate() {
        let data = json!({"correlated_events": [{"ts": "1"}]});
        let result = apply_limit(&data, 10);
        assert_eq!(result["correlated_events"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn apply_limit_unknown_format_passthrough() {
        let data = json!({"foo": "bar"});
        let result = apply_limit(&data, 1);
        assert_eq!(result, data);
    }

    // ─── format_as_table ────────────────────────────────────────────────

    #[test]
    fn format_as_table_prometheus() {
        let data = json!({"data": {"result": [
            {"metric": {"host": "a"}, "value": [1, "10"]},
        ]}});
        let result = format_as_table(&data);
        assert_eq!(result["format"], "table");
        let cols = result["columns"].as_array().unwrap();
        assert!(cols.iter().any(|c| c == "metric"));
        assert_eq!(result["rows"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn format_as_table_empty_result() {
        let data = json!({"data": {"result": []}});
        let result = format_as_table(&data);
        assert_eq!(result["format"], "table");
        assert_eq!(result["columns"].as_array().unwrap().len(), 0);
        assert_eq!(result["rows"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn format_as_table_victorialogs() {
        let data = json!({"result": [
            {"_msg": "hello", "host": "srv1"},
            {"_msg": "world", "host": "srv2", "extra": "field"},
        ]});
        let result = format_as_table(&data);
        assert_eq!(result["format"], "table");
        let cols = result["columns"].as_array().unwrap();
        // Should have all unique columns from all rows
        assert!(cols.len() >= 2);
        assert_eq!(result["rows"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn format_as_table_correlated() {
        let data = json!({"correlated_events": [
            {"timestamp": "1", "host": "srv1"},
        ]});
        let result = format_as_table(&data);
        assert_eq!(result["format"], "table");
        assert_eq!(result["rows"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn format_as_table_no_recognizable_format() {
        let data = json!({"random": "data"});
        let result = format_as_table(&data);
        assert_eq!(result["format"], "table");
        assert_eq!(result["rows"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn format_as_table_top_level_array() {
        let data = json!([{"a": 1}, {"b": 2}]);
        let result = format_as_table(&data);
        assert_eq!(result["format"], "table");
        assert_eq!(result["rows"].as_array().unwrap().len(), 2);
    }

    // ─── format_as_count ────────────────────────────────────────────────

    #[test]
    fn format_as_count_prometheus() {
        let data = json!({"data": {"result": [{"m": 1}, {"m": 2}]}});
        let result = format_as_count(&data);
        assert_eq!(result["format"], "count");
        assert_eq!(result["count"], 2);
    }

    #[test]
    fn format_as_count_empty() {
        let data = json!({"data": {"result": []}});
        let result = format_as_count(&data);
        assert_eq!(result["count"], 0);
    }

    #[test]
    fn format_as_count_victorialogs() {
        let data = json!({"result": [{"a": 1}, {"a": 2}, {"a": 3}]});
        let result = format_as_count(&data);
        assert_eq!(result["count"], 3);
    }

    #[test]
    fn format_as_count_no_results() {
        let data = json!({"unrelated": true});
        let result = format_as_count(&data);
        assert_eq!(result["count"], 0);
    }

    // ─── extract_rows ───────────────────────────────────────────────────

    #[test]
    fn extract_rows_prometheus() {
        let data = json!({"data": {"result": [{"x": 1}, {"x": 2}]}});
        assert_eq!(extract_rows(&data).len(), 2);
    }

    #[test]
    fn extract_rows_victorialogs() {
        let data = json!({"result": [{"_msg": "a"}]});
        assert_eq!(extract_rows(&data).len(), 1);
    }

    #[test]
    fn extract_rows_correlated() {
        let data = json!({"correlated_events": [{"ts": "1"}]});
        assert_eq!(extract_rows(&data).len(), 1);
    }

    #[test]
    fn extract_rows_top_level_array() {
        let data = json!([1, 2, 3]);
        assert_eq!(extract_rows(&data).len(), 3);
    }

    #[test]
    fn extract_rows_empty() {
        let data = json!({"unknown": true});
        assert_eq!(extract_rows(&data).len(), 0);
    }

    // ─── FormatSpec defaults ────────────────────────────────────────────

    #[test]
    fn format_spec_defaults() {
        let spec = FormatSpec::default();
        assert!(spec.show_format.is_none());
        assert_eq!(spec.output_format, "json");
        assert_eq!(spec.limit, 100);
    }

    // ─── Integration: limit + table ─────────────────────────────────────

    #[test]
    fn format_response_limit_then_table() {
        let data = json!({"data": {"result": [
            {"metric": {"h": "a"}, "value": [1, "1"]},
            {"metric": {"h": "b"}, "value": [2, "2"]},
            {"metric": {"h": "c"}, "value": [3, "3"]},
        ]}});
        let spec = FormatSpec {
            show_format: Some("table".to_string()),
            output_format: "json".to_string(),
            limit: 2,
        };
        let result = format_response(&data, &spec);
        assert_eq!(result["format"], "table");
        // After limit=2, only 2 rows should be in the table
        assert_eq!(result["rows"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn format_response_limit_then_count() {
        let data = json!({"result": [{"a": 1}, {"a": 2}, {"a": 3}]});
        let spec = FormatSpec {
            show_format: Some("count".to_string()),
            output_format: "json".to_string(),
            limit: 2,
        };
        let result = format_response(&data, &spec);
        assert_eq!(result["format"], "count");
        assert_eq!(result["count"], 2); // limit applied first, then count
    }
}
