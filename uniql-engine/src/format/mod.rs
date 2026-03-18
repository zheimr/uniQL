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
