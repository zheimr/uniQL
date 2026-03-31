//! UNIQL → LogsQL (VictoriaLogs) Transpiler
//!
//! Converts a UNIQL AST into a valid VictoriaLogs LogsQL query string.
//! VictoriaLogs uses a pipe-based query language with built-in fields:
//!   _msg (log message), _stream (stream labels), _time (timestamp)
//!
//! LogsQL reference: https://docs.victoriametrics.com/victorialogs/logsql/

use super::{BackendType, TranspileError, TranspileOutput, Transpiler};
use crate::ast::*;
use crate::bind::{self, BoundCondition, BoundOrGroup};
use crate::normalize::NormalizedQuery;

// ─── Trait Implementation ─────────────────────────────────────────────────────

pub struct LogsQLTranspiler;

impl Transpiler for LogsQLTranspiler {
    fn name(&self) -> &str {
        "logsql"
    }

    fn supported_signals(&self) -> &[SignalType] {
        &[SignalType::Logs]
    }

    fn transpile(&self, query: &Query) -> Result<TranspileOutput, TranspileError> {
        let native = transpile(query)?;
        Ok(TranspileOutput {
            native_query: native,
            target_signal: SignalType::Logs,
            backend_type: BackendType::VictoriaLogs,
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
            backend_type: BackendType::VictoriaLogs,
        })
    }
}

// ─── LogsQL Builder ───────────────────────────────────────────────────────────

struct LogsQLBuilder {
    /// Stream-level filters: _stream:{service="api"}
    stream_filters: Vec<String>,
    /// Word/phrase/regex filters on _msg
    msg_filters: Vec<String>,
    /// Field-level filters: field:value
    field_filters: Vec<String>,
    /// Pipe stages: | fields ..., | stats ...
    pipe_stages: Vec<String>,
    /// Time filter (handled by API, but can be in query)
    time_filter: Option<String>,
}

impl LogsQLBuilder {
    fn new() -> Self {
        LogsQLBuilder {
            stream_filters: Vec::new(),
            msg_filters: Vec::new(),
            field_filters: Vec::new(),
            pipe_stages: Vec::new(),
            time_filter: None,
        }
    }

    fn build(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Stream filters → _stream:{label="value", ...}
        if !self.stream_filters.is_empty() {
            let selectors = self.stream_filters.join(", ");
            parts.push(format!("_stream:{{{}}}", selectors));
        }

        // Message filters (bare words/phrases in filter section)
        for mf in &self.msg_filters {
            parts.push(mf.clone());
        }

        // Field filters
        for ff in &self.field_filters {
            parts.push(ff.clone());
        }

        // Time filter
        if let Some(ref tf) = self.time_filter {
            parts.push(tf.clone());
        }

        // Space-separated = implicit AND in LogsQL
        let mut query = parts.join(" ");

        // Pipe stages
        for stage in &self.pipe_stages {
            query = format!("{} | {}", query, stage);
        }

        if query.is_empty() {
            "*".to_string()
        } else {
            query
        }
    }
}

// ─── AST → LogsQL ────────────────────────────────────────────────────────────

pub fn transpile(query: &Query) -> Result<String, TranspileError> {
    // Validate signal types
    if let Some(ref from) = query.from {
        for source in &from.sources {
            match source.signal_type {
                SignalType::Logs | SignalType::Unknown(_) => {}
                ref other => {
                    return Err(TranspileError::UnsupportedSignalType {
                        backend: "logsql".to_string(),
                        signal: other.clone(),
                    })
                }
            }
        }
    }

    if query.correlate.is_some() {
        return Err(TranspileError::CorrelateNotSupported);
    }

    let mut builder = LogsQLBuilder::new();

    // Extract WHERE conditions
    if let Some(ref wc) = query.where_clause {
        extract_logsql_conditions(&wc.condition, &mut builder)?;
    }

    // Extract WITHIN as time filter
    if let Some(WithinClause::Last(d)) = &query.within {
        builder.time_filter = Some(format!("_time:{}", d));
    }

    // Extract PARSE clause → pipe stage
    if let Some(ref parse) = query.parse {
        match &parse.mode {
            ParseMode::Json => builder.pipe_stages.push("unpack_json".to_string()),
            ParseMode::Logfmt => builder.pipe_stages.push("unpack_logfmt".to_string()),
            ParseMode::Pattern(pat) => {
                builder.pipe_stages.push(format!("extract \"{}\"", pat));
            }
            ParseMode::Regexp(pat) => {
                builder
                    .pipe_stages
                    .push(format!("extract_regexp \"{}\"", pat));
            }
        }
    }

    // Extract COMPUTE → stats pipe
    if let Some(ref compute) = query.compute {
        if compute.functions.len() > 1 {
            return Err(TranspileError::UnsupportedExpression(
                "LogsQL supports only one aggregation per query. Split into multiple queries."
                    .to_string(),
            ));
        }
        if let Some(func) = compute.functions.first() {
            let func_name = func.name.to_lowercase();

            let stats_func = match func_name.as_str() {
                "count" | "count_over_time" => "count()".to_string(),
                "sum" | "sum_over_time" => {
                    if let Some(arg) = func.args.first() {
                        format!("sum({})", expr_to_field_name(arg))
                    } else {
                        "count()".to_string()
                    }
                }
                "avg" | "avg_over_time" => {
                    if let Some(arg) = func.args.first() {
                        format!("avg({})", expr_to_field_name(arg))
                    } else {
                        "avg()".to_string()
                    }
                }
                "min" => {
                    if let Some(arg) = func.args.first() {
                        format!("min({})", expr_to_field_name(arg))
                    } else {
                        "min()".to_string()
                    }
                }
                "max" => {
                    if let Some(arg) = func.args.first() {
                        format!("max({})", expr_to_field_name(arg))
                    } else {
                        "max()".to_string()
                    }
                }
                "rate" => "count()".to_string(), // rate approximation via count
                _ => format!("count() /* unsupported: {} */", func_name),
            };

            // Build stats pipe with GROUP BY
            if let Some(ref gb) = query.group_by {
                let group_fields: Vec<String> = gb.fields.iter().map(expr_to_field_name).collect();
                builder.pipe_stages.push(format!(
                    "stats by ({}) {}",
                    group_fields.join(", "),
                    stats_func
                ));
            } else {
                builder.pipe_stages.push(format!("stats {}", stats_func));
            }
        }
    }

    // Extract HAVING → filter pipe after stats
    if let Some(ref having) = query.having {
        let filter_expr = having_to_filter(&having.condition);
        if !filter_expr.is_empty() {
            builder.pipe_stages.push(format!("filter {}", filter_expr));
        }
    }

    Ok(builder.build())
}

/// Convert HAVING expression to a LogsQL filter pipe expression
fn having_to_filter(expr: &Expr) -> String {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            let op_str = match op {
                BinaryOp::Gt => ">",
                BinaryOp::Lt => "<",
                BinaryOp::Gte => ">=",
                BinaryOp::Lte => "<=",
                BinaryOp::Eq => "=",
                BinaryOp::Neq => "!=",
                _ => ">",
            };
            let l = having_value(left);
            let r = having_value(right);
            format!("{} {} {}", l, op_str, r)
        }
        _ => String::new(),
    }
}

fn having_value(expr: &Expr) -> String {
    match expr {
        Expr::Ident(name) => format!("\"count(*)\":{}", name),
        Expr::NumberLit(n) => n.to_string(),
        _ => String::new(),
    }
}

fn extract_logsql_conditions(
    expr: &Expr,
    builder: &mut LogsQLBuilder,
) -> Result<(), TranspileError> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOp::And,
            right,
        } => {
            extract_logsql_conditions(left, builder)?;
            extract_logsql_conditions(right, builder)?;
        }

        Expr::BinaryOp {
            left,
            op: BinaryOp::Or,
            right,
        } => {
            // OR → LogsQL uses "or" keyword
            let mut left_builder = LogsQLBuilder::new();
            let mut right_builder = LogsQLBuilder::new();
            extract_logsql_conditions(left, &mut left_builder)?;
            extract_logsql_conditions(right, &mut right_builder)?;
            let left_q = left_builder.build();
            let right_q = right_builder.build();
            builder
                .field_filters
                .push(format!("({} or {})", left_q, right_q));
        }

        Expr::BinaryOp { left, op, right } => {
            let label = get_field_name(left);
            let value = get_field_value(right);

            if let (Some(label), Some(value)) = (label, value) {
                if is_stream_field(&label) {
                    // Stream field → goes inside _stream:{...}
                    match op {
                        BinaryOp::Eq => {
                            builder
                                .stream_filters
                                .push(format!("{}=\"{}\"", label, value));
                        }
                        BinaryOp::Neq => {
                            builder
                                .stream_filters
                                .push(format!("{}!=\"{}\"", label, value));
                        }
                        BinaryOp::RegexMatch => {
                            builder
                                .stream_filters
                                .push(format!("{}=~\"{}\"", label, value));
                        }
                        BinaryOp::RegexNoMatch => {
                            builder
                                .stream_filters
                                .push(format!("{}!~\"{}\"", label, value));
                        }
                        _ => {
                            builder
                                .field_filters
                                .push(format!("{}:\"{}\"", label, value));
                        }
                    }
                } else if label == "level" || label == "log.level" || label == "severity" {
                    // Level field → direct filter
                    builder.field_filters.push(format!("{}:{}", label, value));
                } else {
                    // Generic field filter
                    match op {
                        BinaryOp::Eq => {
                            builder.field_filters.push(format!("{}:{}", label, value));
                        }
                        BinaryOp::Neq => {
                            builder.field_filters.push(format!("-{}:{}", label, value));
                        }
                        BinaryOp::Gt => {
                            builder.field_filters.push(format!("{}:>{}", label, value));
                        }
                        BinaryOp::Gte => {
                            builder.field_filters.push(format!("{}:>={}", label, value));
                        }
                        BinaryOp::Lt => {
                            builder.field_filters.push(format!("{}:<{}", label, value));
                        }
                        BinaryOp::Lte => {
                            builder.field_filters.push(format!("{}:<={}", label, value));
                        }
                        BinaryOp::RegexMatch => {
                            builder
                                .field_filters
                                .push(format!("{}:re({})", label, value));
                        }
                        _ => {
                            builder.field_filters.push(format!("{}:{}", label, value));
                        }
                    }
                }
            }
        }

        Expr::StringMatch { expr, op, pattern } => {
            let label = get_field_name(expr);
            if let Some(label) = label {
                if label == "message" || label == "_msg" || label == "msg" || label == "content" {
                    // Message content → _msg filter
                    match op {
                        StringMatchOp::Contains => {
                            builder.msg_filters.push(format!("\"{}\"", pattern));
                        }
                        StringMatchOp::Matches => {
                            builder.msg_filters.push(format!("re(\"{}\")", pattern));
                        }
                        StringMatchOp::StartsWith => {
                            builder.msg_filters.push(format!("re(\"^{}\")", pattern));
                        }
                    }
                } else {
                    // Field-level string match
                    match op {
                        StringMatchOp::Contains => {
                            builder
                                .field_filters
                                .push(format!("{}:\"{}\"", label, pattern));
                        }
                        StringMatchOp::Matches => {
                            builder
                                .field_filters
                                .push(format!("{}:re(\"{}\")", label, pattern));
                        }
                        StringMatchOp::StartsWith => {
                            builder
                                .field_filters
                                .push(format!("{}:re(\"^{}\")", label, pattern));
                        }
                    }
                }
            }
        }

        _ => {}
    }

    Ok(())
}

fn is_stream_field(name: &str) -> bool {
    matches!(
        name,
        "job"
            | "service"
            | "namespace"
            | "container"
            | "pod"
            | "host"
            | "instance"
            | "env"
            | "environment"
            | "cluster"
            | "region"
            | "app"
            | "component"
            | "device_type"
    )
}

fn get_field_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name) => Some(name.clone()),
        Expr::QualifiedIdent(parts) => {
            if parts.len() >= 2 {
                // Skip signal prefix: logs.level → level
                Some(parts[1..].join("."))
            } else {
                Some(parts.join("."))
            }
        }
        _ => None,
    }
}

fn get_field_value(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLit(s) => Some(s.clone()),
        Expr::NumberLit(n) => Some(format!("{}", n)),
        Expr::Ident(s) => Some(s.clone()),
        _ => None,
    }
}

fn expr_to_field_name(expr: &Expr) -> String {
    match expr {
        Expr::Ident(name) => name.clone(),
        Expr::QualifiedIdent(parts) => parts.last().cloned().unwrap_or_default(),
        _ => "_msg".to_string(),
    }
}

// ─── Normalized Path ─────────────────────────────────────────────────────────

/// Transpile using pre-computed binder/normalizer data.
/// Uses BoundConditions, fixes HAVING bug (no more hardcoded "count(*)").
fn transpile_from_normalized(nq: &NormalizedQuery) -> Result<String, TranspileError> {
    let query = &nq.bound.query;

    // Validate signal types
    if let Some(ref from) = query.from {
        for source in &from.sources {
            match source.signal_type {
                SignalType::Logs | SignalType::Unknown(_) => {}
                ref other => {
                    return Err(TranspileError::UnsupportedSignalType {
                        backend: "logsql".to_string(),
                        signal: other.clone(),
                    })
                }
            }
        }
    }

    if query.correlate.is_some() {
        return Err(TranspileError::CorrelateNotSupported);
    }

    let mut builder = LogsQLBuilder::new();

    // Use bound conditions directly
    for cond in &nq.bound.conditions {
        match cond {
            BoundCondition::StreamLabel { name, op, value } => {
                let op_str = match op {
                    bind::BoundOp::Eq => "=",
                    bind::BoundOp::Neq => "!=",
                    bind::BoundOp::RegexMatch => "=~",
                    bind::BoundOp::RegexNoMatch => "!~",
                    _ => "=",
                };
                builder
                    .stream_filters
                    .push(format!("{}{}\"{}\"", name, op_str, value));
            }
            BoundCondition::FieldFilter { name, op, value } => {
                if name == "level" || name == "log.level" || name == "severity" {
                    builder.field_filters.push(format!("{}:{}", name, value));
                } else {
                    match op {
                        bind::BoundOp::Eq => {
                            builder.field_filters.push(format!("{}:{}", name, value))
                        }
                        bind::BoundOp::Neq => {
                            builder.field_filters.push(format!("-{}:{}", name, value))
                        }
                        bind::BoundOp::Gt => {
                            builder.field_filters.push(format!("{}:>{}", name, value))
                        }
                        bind::BoundOp::Gte => {
                            builder.field_filters.push(format!("{}:>={}", name, value))
                        }
                        bind::BoundOp::Lt => {
                            builder.field_filters.push(format!("{}:<{}", name, value))
                        }
                        bind::BoundOp::Lte => {
                            builder.field_filters.push(format!("{}:<={}", name, value))
                        }
                        bind::BoundOp::RegexMatch => builder
                            .field_filters
                            .push(format!("{}:re({})", name, value)),
                        _ => builder.field_filters.push(format!("{}:{}", name, value)),
                    }
                }
            }
            BoundCondition::ContentFilter { match_op, pattern } => match match_op {
                StringMatchOp::Contains => {
                    builder.msg_filters.push(format!("\"{}\"", pattern));
                }
                StringMatchOp::Matches => {
                    builder.msg_filters.push(format!("re(\"{}\")", pattern));
                }
                StringMatchOp::StartsWith => {
                    builder.msg_filters.push(format!("re(\"^{}\")", pattern));
                }
            },
            BoundCondition::OrGroup(BoundOrGroup { field, values }) => {
                // OR → build "or" expression
                let parts: Vec<String> = values
                    .iter()
                    .map(|v| {
                        if bind::is_stream_label(field) {
                            format!("_stream:{{{}=\"{}\"}}", field, v)
                        } else {
                            format!("{}:{}", field, v)
                        }
                    })
                    .collect();
                builder
                    .field_filters
                    .push(format!("({})", parts.join(" or ")));
            }
            BoundCondition::FieldStringMatch {
                name,
                match_op,
                pattern,
            } => match match_op {
                StringMatchOp::Contains => {
                    builder
                        .field_filters
                        .push(format!("{}:\"{}\"", name, pattern));
                }
                StringMatchOp::Matches => {
                    builder
                        .field_filters
                        .push(format!("{}:re(\"{}\")", name, pattern));
                }
                StringMatchOp::StartsWith => {
                    builder
                        .field_filters
                        .push(format!("{}:re(\"^{}\")", name, pattern));
                }
            },
            BoundCondition::MetricName(_)
            | BoundCondition::InList { .. }
            | BoundCondition::CrossFieldOr { .. } => {
                // Not applicable to LogsQL
            }
            BoundCondition::Native { backend, query } => match backend.as_deref() {
                None | Some("logsql") | Some("victorialogs") | Some("vlogs") => {
                    builder.field_filters.push(query.clone());
                }
                Some(other) => {
                    return Err(TranspileError::UnsupportedExpression(format!(
                        "NATIVE('{}', ...) cannot be transpiled to LogsQL",
                        other
                    )));
                }
            },
        }
    }

    // Time filter from normalized duration (for WITHIN)
    if let Some(WithinClause::Last(d)) = &query.within {
        builder.time_filter = Some(format!("_time:{}", d));
    }

    // PARSE clause
    if let Some(ref parse) = query.parse {
        match &parse.mode {
            ParseMode::Json => builder.pipe_stages.push("unpack_json".to_string()),
            ParseMode::Logfmt => builder.pipe_stages.push("unpack_logfmt".to_string()),
            ParseMode::Pattern(pat) => builder.pipe_stages.push(format!("extract \"{}\"", pat)),
            ParseMode::Regexp(pat) => builder
                .pipe_stages
                .push(format!("extract_regexp \"{}\"", pat)),
        }
    }

    // COMPUTE → stats pipe (uses normalized aggregation)
    if let Some(ref agg) = nq.aggregation {
        let stats_func = match agg.func_name.as_str() {
            "count" | "count_over_time" => "count()".to_string(),
            "sum" | "sum_over_time" => {
                if let Some(ref compute) = query.compute {
                    if let Some(func) = compute.functions.first() {
                        if let Some(arg) = func.args.first() {
                            format!("sum({})", expr_to_field_name(arg))
                        } else {
                            "count()".to_string()
                        }
                    } else {
                        "count()".to_string()
                    }
                } else {
                    "count()".to_string()
                }
            }
            "avg" | "avg_over_time" => {
                if let Some(ref compute) = query.compute {
                    if let Some(func) = compute.functions.first() {
                        if let Some(arg) = func.args.first() {
                            format!("avg({})", expr_to_field_name(arg))
                        } else {
                            "avg()".to_string()
                        }
                    } else {
                        "avg()".to_string()
                    }
                } else {
                    "avg()".to_string()
                }
            }
            "min" => {
                if let Some(ref compute) = query.compute {
                    if let Some(func) = compute.functions.first() {
                        if let Some(arg) = func.args.first() {
                            format!("min({})", expr_to_field_name(arg))
                        } else {
                            "min()".to_string()
                        }
                    } else {
                        "min()".to_string()
                    }
                } else {
                    "min()".to_string()
                }
            }
            "max" => {
                if let Some(ref compute) = query.compute {
                    if let Some(func) = compute.functions.first() {
                        if let Some(arg) = func.args.first() {
                            format!("max({})", expr_to_field_name(arg))
                        } else {
                            "max()".to_string()
                        }
                    } else {
                        "max()".to_string()
                    }
                } else {
                    "max()".to_string()
                }
            }
            "rate" => "count()".to_string(),
            other => format!("count() /* unsupported: {} */", other),
        };

        // Build stats pipe with GROUP BY (uses normalized labels)
        if !nq.group_by_labels.is_empty() {
            builder.pipe_stages.push(format!(
                "stats by ({}) {}",
                nq.group_by_labels.join(", "),
                stats_func
            ));
        } else {
            builder.pipe_stages.push(format!("stats {}", stats_func));
        }
    }

    // HAVING → filter pipe (uses normalized HAVING — fixes hardcoded "count(*)" bug)
    if let Some(ref having) = nq.having {
        if let Some(ref full) = having.full_expr {
            // Compound HAVING — LogsQL doesn't support AND/OR in a single filter pipe.
            // Split AND into separate filter pipes; OR stays as single (VLogs supports `or` in filter).
            if having.op == "AND" {
                for part in full.split(" AND ") {
                    builder.pipe_stages.push(format!("filter {}", part.trim()));
                }
            } else {
                builder.pipe_stages.push(format!("filter {}", full));
            }
        } else if !having.op.is_empty() {
            // Simple HAVING — use aggregate function ref
            let stats_ref = having
                .aggregate_func
                .as_deref()
                .map(|f| match f {
                    "count" | "count_over_time" => "count()".to_string(),
                    "sum" => "sum()".to_string(),
                    "avg" => "avg()".to_string(),
                    "min" => "min()".to_string(),
                    "max" => "max()".to_string(),
                    "rate" => "count()".to_string(),
                    other => format!("{}()", other),
                })
                .unwrap_or_else(|| "count()".to_string());

            let lhs = if having.lhs.is_none() {
                format!("\"{}\"", stats_ref)
            } else {
                having.lhs.clone().unwrap()
            };
            let filter_expr = format!("{} {} {}", lhs, having.op, having.value);
            builder.pipe_stages.push(format!("filter {}", filter_expr));
        }
    }

    Ok(builder.build())
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
    fn test_simple_stream_filter() {
        let result = transpile_query("FROM logs WHERE service = \"api\"").unwrap();
        assert_eq!(result, "_stream:{service=\"api\"}");
    }

    #[test]
    fn test_message_contains() {
        let result =
            transpile_query("FROM logs WHERE service = \"api\" AND message CONTAINS \"error\"")
                .unwrap();
        assert_eq!(result, "_stream:{service=\"api\"} \"error\"");
    }

    #[test]
    fn test_message_regex() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\" AND message MATCHES \"error.*timeout\"",
        )
        .unwrap();
        assert_eq!(result, "_stream:{service=\"api\"} re(\"error.*timeout\")");
    }

    #[test]
    fn test_multiple_stream_filters() {
        let result =
            transpile_query("FROM logs WHERE service = \"api\" AND host = \"prod-01\"").unwrap();
        assert_eq!(result, "_stream:{service=\"api\", host=\"prod-01\"}");
    }

    #[test]
    fn test_with_time_filter() {
        let result = transpile_query("FROM logs WHERE service = \"api\" WITHIN last 5m").unwrap();
        assert!(result.contains("_time:5m"));
    }

    #[test]
    fn test_json_parse() {
        let result = transpile_query("FROM logs WHERE service = \"api\" PARSE json").unwrap();
        assert!(result.contains("| unpack_json"));
    }

    #[test]
    fn test_stats_count() {
        let result = transpile_query("FROM logs WHERE service = \"api\" COMPUTE count()").unwrap();
        assert!(result.contains("| stats count()"));
    }

    #[test]
    fn test_stats_group_by() {
        let result =
            transpile_query("FROM logs WHERE service = \"api\" COMPUTE count() GROUP BY level")
                .unwrap();
        assert!(result.contains("| stats by (level) count()"));
    }

    #[test]
    fn test_level_filter() {
        let result =
            transpile_query("FROM logs WHERE service = \"api\" AND level = \"error\"").unwrap();
        assert!(result.contains("_stream:{service=\"api\"}"));
        assert!(result.contains("level:error"));
    }

    #[test]
    fn test_rejects_metrics() {
        let result = transpile_query("FROM metrics WHERE service = \"api\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_aetheris_syslog() {
        let result = transpile_query(
            "FROM logs WHERE service = \"syslog-collector\" AND message CONTAINS \"link down\"",
        )
        .unwrap();
        assert_eq!(
            result,
            "_stream:{service=\"syslog-collector\"} \"link down\""
        );
    }

    #[test]
    fn test_negation_filter() {
        let result = transpile_query("FROM logs WHERE service != \"debug-service\"").unwrap();
        assert_eq!(result, "_stream:{service!=\"debug-service\"}");
    }

    // ─── Mirror Tests ────────────────────────────────────────────────────────

    #[test]
    fn test_mirror_simple_stream() {
        assert_mirror("FROM logs WHERE service = \"api\"");
    }

    #[test]
    fn test_mirror_message_contains() {
        assert_mirror("FROM logs WHERE service = \"api\" AND message CONTAINS \"error\"");
    }

    #[test]
    fn test_mirror_multiple_stream() {
        assert_mirror("FROM logs WHERE service = \"api\" AND host = \"prod-01\"");
    }

    #[test]
    fn test_mirror_time_filter() {
        assert_mirror("FROM logs WHERE service = \"api\" WITHIN last 5m");
    }

    #[test]
    fn test_mirror_json_parse() {
        assert_mirror("FROM logs WHERE service = \"api\" PARSE json");
    }

    #[test]
    fn test_mirror_stats_count() {
        assert_mirror("FROM logs WHERE service = \"api\" COMPUTE count()");
    }

    #[test]
    fn test_mirror_stats_group_by() {
        assert_mirror("FROM logs WHERE service = \"api\" COMPUTE count() GROUP BY level");
    }

    #[test]
    fn test_mirror_negation() {
        assert_mirror("FROM logs WHERE service != \"debug-service\"");
    }

    #[test]
    fn test_mirror_syslog() {
        assert_mirror(
            "FROM logs WHERE service = \"syslog-collector\" AND message CONTAINS \"link down\"",
        );
    }

    #[test]
    fn test_mirror_level_filter() {
        assert_mirror("FROM logs WHERE service = \"api\" AND level = \"error\"");
    }

    #[test]
    fn test_mirror_message_regex() {
        assert_mirror("FROM logs WHERE service = \"api\" AND message MATCHES \"error.*timeout\"");
    }

    // ─── Bug Fix Tests ───────────────────────────────────────────────────────

    #[test]
    fn test_normalized_having_uses_actual_aggregate() {
        // Bug fix: old path hardcoded "count(*)" in having_value;
        // normalized path uses actual aggregate function name.
        let nq = crate::prepare_normalized(
            "FROM logs WHERE service = \"api\" COMPUTE count() GROUP BY level HAVING count > 100",
        )
        .unwrap();
        let result = transpile_from_normalized(&nq).unwrap();
        // Should reference "count()" not "count(*)"
        assert!(result.contains("filter \"count()\""), "Got: {}", result);
    }

    // ─── NATIVE Tests ────────────────────────────────────────────────────

    // ─── Trait + Legacy Path Coverage ─────────────────────────────────

    #[test]
    fn test_trait_name() {
        let t = LogsQLTranspiler;
        assert_eq!(t.name(), "logsql");
    }

    #[test]
    fn test_trait_supported_signals() {
        let t = LogsQLTranspiler;
        assert_eq!(t.supported_signals(), &[SignalType::Logs]);
    }

    #[test]
    fn test_trait_transpile() {
        let tokens = lexer::tokenize("FROM logs WHERE service = \"api\"").unwrap();
        let ast = parser::parse(tokens).unwrap();
        let t = LogsQLTranspiler;
        let output = t.transpile(&ast).unwrap();
        assert_eq!(output.backend_type, super::BackendType::VictoriaLogs);
        assert!(output.native_query.contains("service"));
    }

    #[test]
    fn test_trait_transpile_normalized() {
        let nq = crate::prepare_normalized("FROM logs WHERE service = \"api\"").unwrap();
        let t = LogsQLTranspiler;
        let output = t.transpile_normalized(&nq).unwrap();
        assert_eq!(output.backend_type, super::BackendType::VictoriaLogs);
    }

    #[test]
    fn test_correlate_rejected() {
        let result = transpile_query("FROM logs WHERE service = \"api\"");
        assert!(result.is_ok());
        // CORRELATE is rejected at semantic level before reaching transpiler
    }

    // ─── PARSE Clause Variations ─────────────────────────────────────

    #[test]
    fn test_parse_logfmt() {
        let result = transpile_query("FROM logs WHERE service = \"api\" PARSE logfmt").unwrap();
        assert!(result.contains("| unpack_logfmt"), "Got: {}", result);
    }

    #[test]
    fn test_parse_pattern() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\" PARSE pattern \"<ip> - <method> <path>\"",
        )
        .unwrap();
        assert!(result.contains("| extract"), "Got: {}", result);
    }

    #[test]
    fn test_parse_regexp() {
        let result = transpile_query(
            "FROM logs WHERE service = \"api\" PARSE regexp \"(?P<status>\\\\d{3})\"",
        )
        .unwrap();
        assert!(result.contains("| extract_regexp"), "Got: {}", result);
    }

    // ─── COMPUTE Variations ──────────────────────────────────────────

    #[test]
    fn test_compute_sum() {
        let result =
            transpile_query("FROM logs WHERE service = \"api\" COMPUTE sum(bytes)").unwrap();
        assert!(result.contains("| stats sum(bytes)"), "Got: {}", result);
    }

    #[test]
    fn test_compute_avg() {
        let result =
            transpile_query("FROM logs WHERE service = \"api\" COMPUTE avg(duration)").unwrap();
        assert!(result.contains("| stats avg(duration)"), "Got: {}", result);
    }

    #[test]
    fn test_compute_min_max() {
        let min_result =
            transpile_query("FROM logs WHERE service = \"api\" COMPUTE min(latency)").unwrap();
        assert!(
            min_result.contains("| stats min(latency)"),
            "Got: {}",
            min_result
        );

        let max_result =
            transpile_query("FROM logs WHERE service = \"api\" COMPUTE max(latency)").unwrap();
        assert!(
            max_result.contains("| stats max(latency)"),
            "Got: {}",
            max_result
        );
    }

    #[test]
    fn test_compute_rate() {
        let result =
            transpile_query("FROM logs WHERE service = \"api\" COMPUTE rate(value)").unwrap();
        assert!(
            result.contains("| stats count()"),
            "Rate approximated as count. Got: {}",
            result
        );
    }

    #[test]
    fn test_or_condition() {
        let result =
            transpile_query("FROM logs WHERE (service = \"api\" OR service = \"web\")").unwrap();
        assert!(result.contains("or"), "Got: {}", result);
    }

    #[test]
    fn test_field_greater_than() {
        let result = transpile_query("FROM logs WHERE status = \"500\"").unwrap();
        assert!(result.contains("status:500"), "Got: {}", result);
    }

    #[test]
    fn test_regex_match_in_stream() {
        let result = transpile_query("FROM logs WHERE service =~ \"api.*\"").unwrap();
        assert!(result.contains("=~"), "Got: {}", result);
    }

    #[test]
    fn test_message_starts_with() {
        let result =
            transpile_query("FROM logs WHERE service = \"api\" AND message STARTS WITH \"ERROR\"")
                .unwrap();
        assert!(result.contains("re(\"^ERROR\")"), "Got: {}", result);
    }

    // ─── NATIVE Tests ────────────────────────────────────────────────────

    #[test]
    fn test_native_in_logsql() {
        let result = transpile_normalized_query(
            "FROM logs WHERE service = \"api\" AND NATIVE(\"status_code:>=500\")",
        )
        .unwrap();
        assert!(result.contains("service=\"api\""), "Got: {}", result);
        assert!(result.contains("status_code:>=500"), "Got: {}", result);
    }

    #[test]
    fn test_native_wrong_backend_logsql() {
        let result =
            transpile_normalized_query("FROM logs WHERE NATIVE(\"promql\", \"up{job='api'}\")");
        assert!(result.is_err());
    }
}
