use wasm_bindgen::prelude::*;

/// Parse a UNIQL query and return the AST as JSON (with macro expansion)
#[wasm_bindgen]
pub fn parse(input: &str) -> Result<String, JsError> {
    let ast = uniql_core::prepare(input).map_err(|e| JsError::new(&e.to_string()))?;
    serde_json::to_string_pretty(&ast).map_err(|e| JsError::new(&e.to_string()))
}

/// Transpile UNIQL to PromQL (includes expand + validate)
#[wasm_bindgen]
pub fn to_promql(input: &str) -> Result<String, JsError> {
    uniql_core::to_promql(input).map_err(|e| JsError::new(&e.to_string()))
}

/// Transpile UNIQL to LogQL (includes expand + validate)
#[wasm_bindgen]
pub fn to_logql(input: &str) -> Result<String, JsError> {
    uniql_core::to_logql(input).map_err(|e| JsError::new(&e.to_string()))
}

/// Transpile UNIQL to LogsQL (includes expand + validate)
#[wasm_bindgen]
pub fn to_logsql(input: &str) -> Result<String, JsError> {
    uniql_core::to_logsql(input).map_err(|e| JsError::new(&e.to_string()))
}

/// Validate a UNIQL query — returns JSON with valid/errors/warnings
#[wasm_bindgen]
pub fn validate(input: &str) -> Result<String, JsError> {
    let ast = match uniql_core::parse(input) {
        Ok(ast) => ast,
        Err(e) => {
            return Ok(serde_json::json!({
                "valid": false,
                "error": e.to_string(),
                "signals": [],
                "clauses": ""
            })
            .to_string());
        }
    };

    let expanded = match uniql_core::expand::expand(&ast) {
        Ok(e) => e,
        Err(e) => {
            return Ok(serde_json::json!({
                "valid": false,
                "error": e.to_string(),
                "signals": ast.inferred_signal_types().iter().map(|s| format!("{:?}", s)).collect::<Vec<_>>(),
                "clauses": ast.clause_summary()
            }).to_string());
        }
    };

    let warnings = match uniql_core::semantic::validate(&expanded) {
        Ok(w) => w,
        Err(e) => {
            return Ok(serde_json::json!({
                "valid": false,
                "error": e.to_string(),
                "hint": e.hint,
                "signals": expanded.inferred_signal_types().iter().map(|s| format!("{:?}", s)).collect::<Vec<_>>(),
                "clauses": expanded.clause_summary()
            }).to_string());
        }
    };

    Ok(serde_json::json!({
        "valid": true,
        "signals": expanded.inferred_signal_types().iter().map(|s| format!("{:?}", s)).collect::<Vec<_>>(),
        "clauses": expanded.clause_summary(),
        "warnings": warnings.iter().map(|w| &w.message).collect::<Vec<_>>()
    }).to_string())
}

/// Explain a UNIQL query — returns execution plan JSON (which backend, native query per target)
#[wasm_bindgen]
pub fn explain(input: &str) -> Result<String, JsError> {
    let ast = match uniql_core::prepare(input) {
        Ok(ast) => ast,
        Err(e) => {
            return Ok(serde_json::json!({
                "error": e.to_string(),
                "steps": []
            })
            .to_string());
        }
    };

    let signals: Vec<String> = ast
        .inferred_signal_types()
        .iter()
        .map(|s| format!("{:?}", s))
        .collect();
    let clauses = ast.clause_summary();

    let mut steps = Vec::<serde_json::Value>::new();
    steps.push(serde_json::json!({
        "step": 1,
        "action": "parse",
        "detail": format!("UNIQL → AST ({})", clauses),
    }));

    // Transpile to each backend
    let mut step_num = 2u32;

    // PromQL
    if let Ok(pql) = uniql_core::to_promql(input) {
        steps.push(serde_json::json!({
            "step": step_num,
            "action": "transpile_promql",
            "detail": "Metrics → PromQL → VictoriaMetrics",
            "native_query": pql,
            "backend": "prometheus",
        }));
        step_num += 1;
    }

    // LogsQL
    if let Ok(lsql) = uniql_core::to_logsql(input) {
        steps.push(serde_json::json!({
            "step": step_num,
            "action": "transpile_logsql",
            "detail": "Logs → LogsQL → VictoriaLogs",
            "native_query": lsql,
            "backend": "victorialogs",
        }));
        step_num += 1;
    }

    // LogQL
    if let Ok(lql) = uniql_core::to_logql(input) {
        steps.push(serde_json::json!({
            "step": step_num,
            "action": "transpile_logql",
            "detail": "Logs → LogQL → Loki",
            "native_query": lql,
            "backend": "loki",
        }));
        step_num += 1;
    }

    steps.push(serde_json::json!({
        "step": step_num,
        "action": "execute",
        "detail": "Execute native query against backend (readonly)",
    }));

    Ok(serde_json::json!({
        "signals": signals,
        "clauses": clauses,
        "steps": steps,
    })
    .to_string())
}

/// Autocomplete suggestions for UNIQL query at cursor position
#[wasm_bindgen]
pub fn autocomplete(input: &str, cursor: usize) -> Result<String, JsError> {
    let before = if cursor <= input.len() {
        &input[..cursor]
    } else {
        input
    };
    let last_token = before
        .split_whitespace()
        .last()
        .unwrap_or("")
        .to_uppercase();

    let keywords = vec![
        "SHOW",
        "FROM",
        "WHERE",
        "WITHIN",
        "COMPUTE",
        "GROUP",
        "BY",
        "HAVING",
        "CORRELATE",
        "ON",
        "PARSE",
        "DEFINE",
        "AS",
        "AND",
        "OR",
        "NOT",
        "IN",
        "CONTAINS",
        "MATCHES",
        "STARTS_WITH",
    ];
    let show_formats = vec!["timeseries", "table", "count", "timeline", "heatmap"];
    let signals = vec!["metrics", "logs", "traces", "events"];
    let backends = ["victoria", "vlogs", "loki"];
    let within_hints = vec!["last", "today", "this_week"];
    let functions = vec![
        "count", "sum", "avg", "min", "max", "rate", "p50", "p90", "p95", "p99",
    ];

    let suggestions: Vec<&str> = if before.trim().is_empty() {
        vec!["SHOW", "FROM", "DEFINE"]
    } else if last_token == "SHOW" {
        show_formats.clone()
    } else if last_token == "FROM" {
        let mut s: Vec<&str> = signals.clone();
        s.extend(backends.iter());
        s
    } else if last_token == "WITHIN" {
        within_hints.clone()
    } else if last_token == "COMPUTE" {
        functions.clone()
    } else if last_token == "PARSE" {
        vec!["json", "logfmt", "pattern", "regexp"]
    } else {
        let partial = last_token.to_lowercase();
        keywords
            .iter()
            .chain(show_formats.iter())
            .chain(signals.iter())
            .chain(backends.iter())
            .chain(functions.iter())
            .filter(|k| k.to_lowercase().starts_with(&partial))
            .copied()
            .collect()
    };

    Ok(serde_json::json!({
        "suggestions": suggestions,
        "cursor": cursor,
        "token": last_token,
    })
    .to_string())
}
