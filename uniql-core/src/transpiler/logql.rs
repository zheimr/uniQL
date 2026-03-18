//! UNIQL → LogQL (Grafana Loki) Transpiler
//!
//! Converts a UNIQL AST into a valid LogQL query string.
//! Supports stream selectors, line filters, parser stages, metric queries.

use crate::ast::*;
use crate::bind::{self, BoundCondition, BoundOrGroup};
use crate::config;
use crate::normalize::NormalizedQuery;
use super::{Transpiler, TranspileOutput, TranspileError, BackendType};

// ─── Trait Implementation ─────────────────────────────────────────────────────

pub struct LogQLTranspiler;

impl Transpiler for LogQLTranspiler {
    fn name(&self) -> &str {
        "logql"
    }

    fn supported_signals(&self) -> &[SignalType] {
        &[SignalType::Logs]
    }

    fn transpile(&self, query: &Query) -> Result<TranspileOutput, TranspileError> {
        let native = transpile(query)?;
        Ok(TranspileOutput {
            native_query: native,
            target_signal: SignalType::Logs,
            backend_type: BackendType::Loki,
        })
    }

    fn transpile_normalized(
        &self,
        normalized: &NormalizedQuery,
    ) -> Result<TranspileOutput, TranspileError> {
        let native = transpile_from_normalized(normalized)?;
        Ok(TranspileOutput {
            native_query: native,
            target_signal: SignalType::Logs,
            backend_type: BackendType::Loki,
        })
    }
}

// ─── LogQL Builder ────────────────────────────────────────────────────────────

struct LogQLBuilder {
    stream_selectors: Vec<StreamSelector>,
    line_filters: Vec<LineFilter>,
    parser_stage: Option<String>,
    label_filters: Vec<String>,
    aggregation: Option<LogQLAggregation>,
    group_by_labels: Vec<String>,
    range_duration: Option<String>,
    having_expr: Option<String>,
}

struct StreamSelector {
    label: String,
    op: String,
    value: String,
}

struct LineFilter {
    op: String,   // |= , != , |~ , !~
    pattern: String,
}

struct LogQLAggregation {
    func_name: String,
}

impl LogQLBuilder {
    fn new() -> Self {
        LogQLBuilder {
            stream_selectors: Vec::new(),
            line_filters: Vec::new(),
            parser_stage: None,
            label_filters: Vec::new(),
            aggregation: None,
            group_by_labels: Vec::new(),
            range_duration: None,
            having_expr: None,
        }
    }

    fn build(&self) -> Result<String, TranspileError> {
        // Build stream selector: {service="api", job="nginx"}
        let stream = if self.stream_selectors.is_empty() {
            "{}".to_string()
        } else {
            let selectors: Vec<String> = self.stream_selectors
                .iter()
                .map(|s| format!("{}{}\"{}\"", s.label, s.op, s.value))
                .collect();
            format!("{{{}}}", selectors.join(", "))
        };

        // Build pipeline: |= "error" | json | level = "error"
        let mut pipeline = String::new();

        for lf in &self.line_filters {
            pipeline.push_str(&format!(" {} \"{}\"", lf.op, lf.pattern));
        }

        if let Some(ref parser) = self.parser_stage {
            pipeline.push_str(&format!(" | {}", parser));
        }

        for lf in &self.label_filters {
            pipeline.push_str(&format!(" | {}", lf));
        }

        let log_query = format!("{}{}", stream, pipeline);

        // If there's an aggregation, wrap as metric query
        if let Some(ref agg) = self.aggregation {
            let duration = self.range_duration.as_deref()
                .unwrap_or(config::DEFAULT_RANGE_DURATION);

            let metric_inner = match agg.func_name.as_str() {
                "rate" => format!("rate({}[{}])", log_query, duration),
                "count" | "count_over_time" => {
                    format!("count_over_time({}[{}])", log_query, duration)
                }
                "sum_over_time" => format!("sum_over_time({}[{}])", log_query, duration),
                "avg_over_time" => format!("avg_over_time({}[{}])", log_query, duration),
                "bytes_rate" => format!("bytes_rate({}[{}])", log_query, duration),
                _ => format!("count_over_time({}[{}])", log_query, duration),
            };

            // Wrap with GROUP BY if present
            let mut result = if !self.group_by_labels.is_empty() {
                let labels = self.group_by_labels.join(", ");
                format!("sum by ({}) ({})", labels, metric_inner)
            } else {
                metric_inner
            };

            // Append HAVING as threshold comparison
            if let Some(ref having) = self.having_expr {
                result = format!("{} {}", result, having);
            }

            Ok(result)
        } else {
            Ok(log_query)
        }
    }
}

// ─── AST → LogQL ──────────────────────────────────────────────────────────────

pub fn transpile(query: &Query) -> Result<String, TranspileError> {
    // Validate signal types
    if let Some(ref from) = query.from {
        for source in &from.sources {
            match source.signal_type {
                SignalType::Logs | SignalType::Unknown(_) => {}
                ref other => return Err(TranspileError::UnsupportedSignalType {
                    backend: "logql".to_string(),
                    signal: other.clone(),
                }),
            }
        }
    }

    if query.correlate.is_some() {
        return Err(TranspileError::CorrelateNotSupported);
    }

    let mut builder = LogQLBuilder::new();

    // Extract WHERE conditions
    if let Some(ref wc) = query.where_clause {
        extract_log_conditions(&wc.condition, &mut builder)?;
    }

    // Extract WITHIN as range duration
    if let Some(WithinClause::Last(d)) = &query.within {
        builder.range_duration = Some(d.clone());
    }

    // Extract PARSE clause
    if let Some(ref parse) = query.parse {
        builder.parser_stage = Some(match &parse.mode {
            ParseMode::Json => "json".to_string(),
            ParseMode::Logfmt => "logfmt".to_string(),
            ParseMode::Pattern(pat) => format!("pattern \"{}\"", pat),
            ParseMode::Regexp(pat) => format!("regexp \"{}\"", pat),
        });
    }

    // Extract COMPUTE
    if let Some(ref compute) = query.compute {
        if compute.functions.len() > 1 {
            return Err(TranspileError::UnsupportedExpression(
                "LogQL supports only one aggregation per query. Split into multiple queries.".to_string()
            ));
        }
        if let Some(func) = compute.functions.first() {
            let func_name = func.name.to_lowercase();

            // Extract duration from args
            for arg in &func.args {
                if let Expr::DurationLit(d) = arg {
                    builder.range_duration = Some(d.clone());
                }
            }

            builder.aggregation = Some(LogQLAggregation {
                func_name,
            });
        }
    }

    // Extract GROUP BY
    if let Some(ref gb) = query.group_by {
        for field in &gb.fields {
            match field {
                Expr::Ident(name) => builder.group_by_labels.push(name.clone()),
                Expr::QualifiedIdent(parts) => {
                    builder.group_by_labels.push(parts.last().unwrap().clone())
                }
                _ => {}
            }
        }
    }

    // Extract HAVING as threshold comparison
    if let Some(ref having) = query.having {
        builder.having_expr = Some(having_to_comparison(&having.condition));
    }

    builder.build()
}

/// Convert HAVING expression to a comparison string (e.g., "> 0.01")
fn having_to_comparison(expr: &Expr) -> String {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            let op_str = match op {
                BinaryOp::Gt => ">",
                BinaryOp::Lt => "<",
                BinaryOp::Gte => ">=",
                BinaryOp::Lte => "<=",
                BinaryOp::Eq => "==",
                BinaryOp::Neq => "!=",
                _ => ">",
            };
            // If LHS is aggregate ref, emit only "op value"
            if matches!(left.as_ref(), Expr::Ident(n) if config::is_aggregate_function(n)) {
                let r = expr_to_value(right);
                format!("{} {}", op_str, r)
            } else {
                let l = expr_to_value(left);
                let r = expr_to_value(right);
                format!("{} {} {}", l, op_str, r)
            }
        }
        _ => String::new(),
    }
}

fn expr_to_value(expr: &Expr) -> String {
    match expr {
        Expr::Ident(name) => name.clone(),
        Expr::NumberLit(n) => format!("{}", n),
        _ => String::new(),
    }
}

fn extract_log_conditions(expr: &Expr, builder: &mut LogQLBuilder) -> Result<(), TranspileError> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOp::And,
            right,
        } => {
            extract_log_conditions(left, builder)?;
            extract_log_conditions(right, builder)?;
        }

        Expr::BinaryOp { left, op, right } => {
            let label = expr_to_label_name(left);
            let value = expr_to_string_value(right);

            if let (Some(label), Some(value)) = (label, value) {
                // Decide: stream selector vs label filter
                // Simple labels go to stream selector, post-parse labels go to label filter
                if is_stream_label(&label) {
                    let op_str = match op {
                        BinaryOp::Eq => "=",
                        BinaryOp::Neq => "!=",
                        BinaryOp::RegexMatch => "=~",
                        BinaryOp::RegexNoMatch => "!~",
                        _ => "=",
                    };
                    builder.stream_selectors.push(StreamSelector {
                        label,
                        op: op_str.to_string(),
                        value,
                    });
                } else {
                    // Post-parse label filter
                    let op_str = match op {
                        BinaryOp::Eq => "=",
                        BinaryOp::Neq => "!=",
                        BinaryOp::Gt => ">",
                        BinaryOp::Lt => "<",
                        BinaryOp::Gte => ">=",
                        BinaryOp::Lte => "<=",
                        _ => "=",
                    };
                    builder.label_filters.push(format!("{} {} \"{}\"", label, op_str, value));
                }
            }
        }

        Expr::StringMatch { expr, op, pattern } => {
            let label = expr_to_label_name(expr);
            if let Some(label) = label {
                if label == "message" || label == "_msg" || label == "msg" || label == "content" {
                    // Message content → line filter
                    match op {
                        StringMatchOp::Contains => {
                            builder.line_filters.push(LineFilter {
                                op: "|=".to_string(),
                                pattern: pattern.clone(),
                            });
                        }
                        StringMatchOp::Matches => {
                            builder.line_filters.push(LineFilter {
                                op: "|~".to_string(),
                                pattern: pattern.clone(),
                            });
                        }
                        StringMatchOp::StartsWith => {
                            builder.line_filters.push(LineFilter {
                                op: "|~".to_string(),
                                pattern: format!("^{}", pattern),
                            });
                        }
                    }
                } else {
                    // Non-message field → label filter with regex
                    match op {
                        StringMatchOp::Contains => {
                            builder.label_filters.push(
                                format!("{} =~ \".*{}.*\"", label, pattern)
                            );
                        }
                        StringMatchOp::Matches => {
                            builder.label_filters.push(
                                format!("{} =~ \"{}\"", label, pattern)
                            );
                        }
                        StringMatchOp::StartsWith => {
                            builder.label_filters.push(
                                format!("{} =~ \"{}.*\"", label, pattern)
                            );
                        }
                    }
                }
            }
        }

        _ => {}
    }

    Ok(())
}

/// Determine if a label is a stream-level label (goes into {}) or post-parse label
fn is_stream_label(name: &str) -> bool {
    matches!(
        name,
        "job" | "service" | "namespace" | "container" | "pod" | "host"
        | "instance" | "env" | "environment" | "cluster" | "region"
        | "app" | "component" | "filename"
    )
}

fn expr_to_label_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name) => Some(name.clone()),
        Expr::QualifiedIdent(parts) => {
            if parts.len() >= 2 {
                Some(parts[1..].join("."))
            } else {
                Some(parts.join("."))
            }
        }
        _ => None,
    }
}

fn expr_to_string_value(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLit(s) => Some(s.clone()),
        Expr::NumberLit(n) => Some(format!("{}", n)),
        Expr::Ident(s) => Some(s.clone()),
        _ => None,
    }
}

// ─── Normalized Path ─────────────────────────────────────────────────────────

/// Transpile using pre-computed binder/normalizer data.
/// Uses BoundConditions directly: stream labels, content filters, field filters unified.
fn transpile_from_normalized(nq: &NormalizedQuery) -> Result<String, TranspileError> {
    let query = &nq.bound.query;

    // Validate signal types
    if let Some(ref from) = query.from {
        for source in &from.sources {
            match source.signal_type {
                SignalType::Logs | SignalType::Unknown(_) => {}
                ref other => return Err(TranspileError::UnsupportedSignalType {
                    backend: "logql".to_string(),
                    signal: other.clone(),
                }),
            }
        }
    }

    if query.correlate.is_some() {
        return Err(TranspileError::CorrelateNotSupported);
    }

    let mut builder = LogQLBuilder::new();

    // Use bound conditions directly
    for cond in &nq.bound.conditions {
        match cond {
            BoundCondition::StreamLabel { name, op, value } => {
                builder.stream_selectors.push(StreamSelector {
                    label: name.clone(),
                    op: op.as_promql_str().to_string(),
                    value: value.clone(),
                });
            }
            BoundCondition::FieldFilter { name, op, value } => {
                let op_str = op.as_comparison_str();
                builder.label_filters.push(format!("{} {} \"{}\"", name, op_str, value));
            }
            BoundCondition::ContentFilter { match_op, pattern } => {
                match match_op {
                    StringMatchOp::Contains => {
                        builder.line_filters.push(LineFilter {
                            op: "|=".to_string(),
                            pattern: pattern.clone(),
                        });
                    }
                    StringMatchOp::Matches => {
                        builder.line_filters.push(LineFilter {
                            op: "|~".to_string(),
                            pattern: pattern.clone(),
                        });
                    }
                    StringMatchOp::StartsWith => {
                        builder.line_filters.push(LineFilter {
                            op: "|~".to_string(),
                            pattern: format!("^{}", pattern),
                        });
                    }
                }
            }
            BoundCondition::OrGroup(BoundOrGroup { field, values }) => {
                // OR on same field → regex stream selector or label filter
                let regex = values.join("|");
                if bind::is_stream_label(field) {
                    builder.stream_selectors.push(StreamSelector {
                        label: field.clone(),
                        op: "=~".to_string(),
                        value: regex,
                    });
                } else {
                    builder.label_filters.push(format!("{} =~ \"{}\"", field, regex));
                }
            }
            BoundCondition::FieldStringMatch { name, match_op, pattern } => {
                match match_op {
                    StringMatchOp::Contains => {
                        builder.label_filters.push(format!("{} =~ \".*{}.*\"", name, pattern));
                    }
                    StringMatchOp::Matches => {
                        builder.label_filters.push(format!("{} =~ \"{}\"", name, pattern));
                    }
                    StringMatchOp::StartsWith => {
                        builder.label_filters.push(format!("{} =~ \"{}.*\"", name, pattern));
                    }
                }
            }
            BoundCondition::MetricName(_) | BoundCondition::InList { .. } => {
                // Not applicable to LogQL log queries
            }
            BoundCondition::Native { backend, query } => {
                match backend.as_deref() {
                    None | Some("logql") | Some("loki") => {
                        builder.label_filters.push(query.clone());
                    }
                    Some(other) => {
                        return Err(TranspileError::UnsupportedExpression(
                            format!("NATIVE('{}', ...) cannot be transpiled to LogQL", other),
                        ));
                    }
                }
            }
        }
    }

    // Use normalized duration
    if let Some(ref d) = nq.duration {
        builder.range_duration = Some(d.raw.clone());
    }

    // PARSE clause
    if let Some(ref parse) = query.parse {
        builder.parser_stage = Some(match &parse.mode {
            ParseMode::Json => "json".to_string(),
            ParseMode::Logfmt => "logfmt".to_string(),
            ParseMode::Pattern(pat) => format!("pattern \"{}\"", pat),
            ParseMode::Regexp(pat) => format!("regexp \"{}\"", pat),
        });
    }

    // Use normalized aggregation
    if let Some(ref agg) = nq.aggregation {
        builder.aggregation = Some(LogQLAggregation {
            func_name: agg.func_name.clone(),
        });
    }

    // Use normalized GROUP BY
    builder.group_by_labels = nq.group_by_labels.clone();

    // Use normalized HAVING
    if let Some(ref having) = nq.having {
        let having_str = if let Some(ref lhs) = having.lhs {
            format!("{} {} {}", lhs, having.op, having.value)
        } else {
            format!("{} {}", having.op, having.value)
        };
        builder.having_expr = Some(having_str);
    }

    builder.build()
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::parser;
    use crate::transpiler::TranspileError;

    fn transpile_query(input: &str) -> Result<String, TranspileError> {
        let tokens = lexer::tokenize(input).unwrap();
        let ast = parser::parse(tokens).unwrap();
        transpile(&ast)
    }

    fn transpile_normalized_query(input: &str) -> Result<String, TranspileError> {
        let nq = crate::prepare_normalized(input).unwrap();
        transpile_from_normalized(&nq)
    }

    fn assert_mirror(input: &str) {
        let old = transpile_query(input).unwrap();
        let new = transpile_normalized_query(input).unwrap();
        assert_eq!(old, new, "Mirror mismatch for: {}", input);
    }

    #[test]
    fn test_simple_log_query() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\""
        ).unwrap();
        assert_eq!(result, "{service=\"api\"}");
    }

    #[test]
    fn test_log_with_contains() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\" AND message CONTAINS \"error\""
        ).unwrap();
        assert_eq!(result, "{service=\"api\"} |= \"error\"");
    }

    #[test]
    fn test_log_with_regex_match() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\" AND message MATCHES \"error.*5[0-9]{2}\""
        ).unwrap();
        assert_eq!(result, "{service=\"api\"} |~ \"error.*5[0-9]{2}\"");
    }

    #[test]
    fn test_log_with_json_parse() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\" PARSE json"
        ).unwrap();
        assert_eq!(result, "{service=\"api\"} | json");
    }

    #[test]
    fn test_log_with_parse_and_filter() {
        // Second WHERE overrides first in parser — use combined condition
        let result = transpile_query(
            "FROM logs WHERE job = \"varlogs\" AND level = \"error\" PARSE json"
        ).unwrap();
        assert_eq!(result, "{job=\"varlogs\"} | json | level = \"error\"");
    }

    #[test]
    fn test_log_with_pattern_parse() {
        let result = transpile_query(
            "FROM logs WHERE service = \"nginx\" PARSE pattern \"<ip> - <method> <path> <status>\""
        ).unwrap();
        assert!(result.contains("| pattern"));
    }

    #[test]
    fn test_log_metric_rate() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\" WITHIN last 5m COMPUTE rate(count, 5m)"
        ).unwrap();
        assert_eq!(result, "rate({service=\"api\"}[5m])");
    }

    #[test]
    fn test_log_metric_count() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\" WITHIN last 1h COMPUTE count()"
        ).unwrap();
        assert!(result.contains("count_over_time({service=\"api\"}"));
    }

    #[test]
    fn test_log_metric_group_by() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\" COMPUTE rate(count, 5m) GROUP BY level"
        ).unwrap();
        assert_eq!(result, "sum by (level) (rate({service=\"api\"}[5m]))");
    }

    #[test]
    fn test_rejects_metrics() {
        let result = transpile_query("FROM metrics WHERE service = \"api\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_stream_selectors() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\" AND env = \"prod\" AND namespace = \"default\""
        ).unwrap();
        assert!(result.contains("service=\"api\""));
        assert!(result.contains("env=\"prod\""));
        assert!(result.contains("namespace=\"default\""));
    }

    #[test]
    fn test_log_contains_and_parse() {
        let result = transpile_query(
            "FROM logs WHERE service = \"syslog-collector\" AND message CONTAINS \"link down\" WITHIN last 15m"
        ).unwrap();
        assert_eq!(result, "{service=\"syslog-collector\"} |= \"link down\"");
    }

    // ─── Mirror Tests ────────────────────────────────────────────────────────

    #[test]
    fn test_mirror_simple_log() {
        assert_mirror("FROM logs WHERE service = \"api\"");
    }

    #[test]
    fn test_mirror_log_contains() {
        assert_mirror("FROM logs WHERE service = \"api\" AND message CONTAINS \"error\"");
    }

    #[test]
    fn test_mirror_log_json_parse() {
        assert_mirror("FROM logs WHERE service = \"api\" PARSE json");
    }

    #[test]
    fn test_mirror_log_rate() {
        assert_mirror("FROM logs WHERE service = \"api\" WITHIN last 5m COMPUTE rate(count, 5m)");
    }

    #[test]
    fn test_mirror_log_group_by() {
        assert_mirror("FROM logs WHERE service = \"api\" COMPUTE rate(count, 5m) GROUP BY level");
    }

    #[test]
    fn test_mirror_multiple_selectors() {
        assert_mirror("FROM logs WHERE service = \"api\" AND env = \"prod\" AND namespace = \"default\"");
    }

    #[test]
    fn test_mirror_log_contains_syslog() {
        assert_mirror(
            "FROM logs WHERE service = \"syslog-collector\" AND message CONTAINS \"link down\" WITHIN last 15m"
        );
    }

    // ─── NATIVE Tests ────────────────────────────────────────────────────

    #[test]
    fn test_native_in_logql() {
        let result = transpile_normalized_query(
            "FROM logs WHERE service = \"api\" AND NATIVE(\"status_code >= 500\")"
        ).unwrap();
        assert!(result.contains("service=\"api\""), "Got: {}", result);
        assert!(result.contains("status_code >= 500"), "Got: {}", result);
    }

    #[test]
    fn test_native_wrong_backend_logql() {
        let result = transpile_normalized_query(
            "FROM logs WHERE NATIVE(\"promql\", \"up{job='api'}\")"
        );
        assert!(result.is_err());
    }
}
