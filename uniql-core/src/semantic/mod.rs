//! UNIQL Semantic Validation
//!
//! Signal-type aware validation pass between parsing and transpilation.
//! Catches errors that are syntactically valid but semantically wrong:
//!   - PARSE on non-log sources
//!   - rate(value, ...) on logs without explicit count
//!   - CORRELATE required for multi-signal FROM
//!   - SHOW flamegraph only on traces

use crate::ast::*;

// ─── Validation Errors ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SemanticError {
    pub message: String,
    pub hint: Option<String>,
    pub clause: String,
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(ref hint) = self.hint {
            write!(f, "\n  Hint: {}", hint)?;
        }
        Ok(())
    }
}

impl std::error::Error for SemanticError {}

// ─── Validator ────────────────────────────────────────────────────────────────

pub fn validate(query: &Query) -> Result<Vec<SemanticWarning>, SemanticError> {
    let mut warnings = Vec::new();
    let signals = query.inferred_signal_types();

    // Rule 1: PARSE is only valid for log sources
    if query.parse.is_some()
        && !signals.is_empty()
        && !signals.iter().any(|s| matches!(s, SignalType::Logs | SignalType::Unknown(_)))
    {
        return Err(SemanticError {
            message: "PARSE is only valid for log sources".to_string(),
            hint: Some("PARSE json/logfmt/pattern/regexp is used with FROM logs. Remove PARSE or change source to FROM logs.".to_string()),
            clause: "PARSE".to_string(),
        });
    }

    // Rule 2: Multi-signal FROM requires CORRELATE
    if signals.len() > 1 && query.correlate.is_none() {
        return Err(SemanticError {
            message: format!(
                "Multi-signal query (FROM {:?}) requires a CORRELATE clause",
                signals
            ),
            hint: Some("Add CORRELATE ON <field> WITHIN <duration> to join signals.".to_string()),
            clause: "FROM".to_string(),
        });
    }

    // Rule 3: SHOW flamegraph only on traces
    if let Some(ref show) = query.show {
        if show.format == ShowFormat::Flamegraph
            && !signals.iter().any(|s| matches!(s, SignalType::Traces))
        {
            return Err(SemanticError {
                message: "SHOW flamegraph is only valid for trace sources".to_string(),
                hint: Some("Use FROM traces for flamegraph visualization, or try SHOW timeseries / SHOW timeline.".to_string()),
                clause: "SHOW".to_string(),
            });
        }
        if show.format == ShowFormat::Topology
            && !signals.iter().any(|s| matches!(s, SignalType::Traces))
        {
            warnings.push(SemanticWarning {
                message: "SHOW topology is designed for trace data. Results may be limited with other signal types.".to_string(),
                clause: "SHOW".to_string(),
            });
        }
    }

    // Rule 4: COMPUTE rate(value, ...) on logs is likely an error
    if let Some(ref compute) = query.compute {
        for func in &compute.functions {
            let fname = func.name.to_lowercase();
            if (fname == "rate" || fname == "irate" || fname == "increase") && signals.iter().all(|s| matches!(s, SignalType::Logs)) {
                // Check if first arg is "count" — that's valid for logs
                let first_arg_is_count = func.args.first().map(|a| {
                    matches!(a, Expr::Ident(name) if name.to_lowercase() == "count")
                }).unwrap_or(false);

                if !first_arg_is_count && !func.args.is_empty() {
                    warnings.push(SemanticWarning {
                        message: format!(
                            "COMPUTE {}() with value argument on log source may not produce expected results",
                            fname
                        ),
                        clause: "COMPUTE".to_string(),
                    });
                }
            }
        }
    }

    Ok(warnings)
}

#[derive(Debug, Clone)]
pub struct SemanticWarning {
    pub message: String,
    pub clause: String,
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::parser;

    fn validate_query(input: &str) -> Result<Vec<SemanticWarning>, SemanticError> {
        let tokens = lexer::tokenize(input).unwrap();
        let ast = parser::parse(tokens).unwrap();
        validate(&ast)
    }

    #[test]
    fn test_valid_metric_query() {
        let result = validate_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" COMPUTE rate(value, 5m)"
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_valid_log_query() {
        let result = validate_query(
            "FROM logs WHERE service = \"api\" PARSE json"
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_on_metrics_fails() {
        let result = validate_query(
            "FROM metrics WHERE service = \"api\" PARSE json"
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("PARSE is only valid for log sources"));
    }

    #[test]
    fn test_multi_signal_without_correlate_fails() {
        let result = validate_query(
            "FROM metrics, logs WHERE service = \"api\""
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("CORRELATE"));
    }

    #[test]
    fn test_multi_signal_with_correlate_ok() {
        let result = validate_query(
            "FROM metrics, logs WHERE service = \"api\" CORRELATE ON service WITHIN 30s"
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_flamegraph_on_metrics_fails() {
        let result = validate_query(
            "FROM metrics SHOW flamegraph"
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("flamegraph"));
    }

    #[test]
    fn test_rate_on_logs_warns() {
        let result = validate_query(
            "FROM logs WHERE service = \"api\" COMPUTE rate(value, 5m)"
        );
        assert!(result.is_ok());
        let warnings = result.unwrap();
        assert!(!warnings.is_empty());
    }

    #[test]
    fn test_rate_count_on_logs_no_warning() {
        let result = validate_query(
            "FROM logs WHERE service = \"api\" COMPUTE rate(count, 5m)"
        );
        assert!(result.is_ok());
        let warnings = result.unwrap();
        assert!(warnings.is_empty());
    }
}
