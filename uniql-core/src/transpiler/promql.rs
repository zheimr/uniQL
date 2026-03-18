//! UNIQL → PromQL/MetricsQL Transpiler
//!
//! Converts a UNIQL AST into a valid PromQL query string.
//! Supports metric selection, label filtering, rate, aggregations, GROUP BY, HAVING.

use crate::ast::*;
use crate::bind::{BoundCondition, BoundOrGroup};
use crate::config;
use crate::normalize::NormalizedQuery;
use super::{Transpiler, TranspileOutput, TranspileError, BackendType};

// ─── Trait Implementation ─────────────────────────────────────────────────────

pub struct PromQLTranspiler;

impl Transpiler for PromQLTranspiler {
    fn name(&self) -> &str {
        "promql"
    }

    fn supported_signals(&self) -> &[SignalType] {
        &[SignalType::Metrics]
    }

    fn transpile(&self, query: &Query) -> Result<TranspileOutput, TranspileError> {
        let native = transpile(query)?;
        Ok(TranspileOutput {
            native_query: native,
            target_signal: SignalType::Metrics,
            backend_type: BackendType::Prometheus,
        })
    }

    fn transpile_normalized(
        &self,
        normalized: &NormalizedQuery,
    ) -> Result<TranspileOutput, TranspileError> {
        let native = transpile_from_normalized(normalized)?;
        Ok(TranspileOutput {
            native_query: native,
            target_signal: SignalType::Metrics,
            backend_type: BackendType::Prometheus,
        })
    }
}

// ─── Transpiler ────────────────────────────────────────────────────────────────

struct PromQLBuilder {
    metric_name: Option<String>,
    label_matchers: Vec<LabelMatcher>,
    native_fragments: Vec<String>,
    range_duration: Option<String>,
    aggregation: Option<AggregationInfo>,
    group_by_labels: Vec<String>,
    having_expr: Option<String>,
}

struct LabelMatcher {
    label: String,
    op: String,    // =, !=, =~, !~
    value: String,
}

struct AggregationInfo {
    func_name: String,
    wrapper_func: Option<String>,
}

impl PromQLBuilder {
    fn new() -> Self {
        PromQLBuilder {
            metric_name: None,
            label_matchers: Vec::new(),
            native_fragments: Vec::new(),
            range_duration: None,
            aggregation: None,
            group_by_labels: Vec::new(),
            having_expr: None,
        }
    }

    fn build(&self) -> Result<String, TranspileError> {
        // If there are native fragments and nothing else, return native directly
        if !self.native_fragments.is_empty() && self.label_matchers.is_empty()
            && self.metric_name.is_none() && self.aggregation.is_none()
        {
            return Ok(self.native_fragments.join(" "));
        }

        // Build metric selector
        let metric = self.metric_name.as_deref().unwrap_or("");
        let mut matchers = self.build_label_matchers();

        // Append native fragments as raw matchers inside {}
        for frag in &self.native_fragments {
            if !matchers.is_empty() {
                matchers.push_str(", ");
            }
            matchers.push_str(frag);
        }

        let selector = if matchers.is_empty() {
            metric.to_string()
        } else if metric.is_empty() {
            format!("{{{}}}", matchers)
        } else {
            format!("{}{{{}}}", metric, matchers)
        };

        // Wrap with range + function if needed
        let mut query = match &self.aggregation {
            Some(agg) => {
                let range_selector = match &self.range_duration {
                    Some(d) => format!("{}[{}]", selector, d),
                    None => selector.clone(),
                };

                let inner = match agg.func_name.as_str() {
                    "rate" => {
                        let duration = self.range_duration.as_deref().unwrap_or(config::DEFAULT_RANGE_DURATION);
                        format!("rate({}[{}])", selector, duration)
                    }
                    "irate" => {
                        let duration = self.range_duration.as_deref().unwrap_or(config::DEFAULT_RANGE_DURATION);
                        format!("irate({}[{}])", selector, duration)
                    }
                    "increase" => {
                        let duration = self.range_duration.as_deref().unwrap_or(config::DEFAULT_RANGE_DURATION);
                        format!("increase({}[{}])", selector, duration)
                    }
                    "count" => {
                        if self.range_duration.is_some() {
                            format!("count_over_time({})", range_selector)
                        } else {
                            format!("count({})", selector)
                        }
                    }
                    "avg" => selector.clone(),
                    "sum" => selector.clone(),
                    "min" => selector.clone(),
                    "max" => selector.clone(),
                    "p50" | "p90" | "p95" | "p99" => selector.clone(),
                    "histogram_quantile" => selector.clone(),
                    _ => return Err(TranspileError::UnknownFunction(agg.func_name.clone())),
                };

                // Wrap with outer aggregation
                match agg.wrapper_func.as_deref().or(Some(agg.func_name.as_str())) {
                    Some("rate") | Some("irate") | Some("increase") | Some("count") => inner,
                    Some("avg") => {
                        if self.group_by_labels.is_empty() {
                            format!("avg({})", inner)
                        } else {
                            format!(
                                "avg by ({}) ({})",
                                self.group_by_labels.join(", "),
                                inner
                            )
                        }
                    }
                    Some("sum") => {
                        if self.group_by_labels.is_empty() {
                            format!("sum({})", inner)
                        } else {
                            format!(
                                "sum by ({}) ({})",
                                self.group_by_labels.join(", "),
                                inner
                            )
                        }
                    }
                    Some("min") => {
                        if self.group_by_labels.is_empty() {
                            format!("min({})", inner)
                        } else {
                            format!(
                                "min by ({}) ({})",
                                self.group_by_labels.join(", "),
                                inner
                            )
                        }
                    }
                    Some("max") => {
                        if self.group_by_labels.is_empty() {
                            format!("max({})", inner)
                        } else {
                            format!(
                                "max by ({}) ({})",
                                self.group_by_labels.join(", "),
                                inner
                            )
                        }
                    }
                    Some(name @ ("p50" | "p75" | "p90" | "p95" | "p99" | "p999")) => {
                        let quantile = config::quantile_for_percentile(name)
                            .unwrap_or("0.99");
                        let duration = self.range_duration.as_deref().unwrap_or(config::DEFAULT_RANGE_DURATION);
                        format!(
                            "histogram_quantile({}, rate({}[{}]))",
                            quantile, selector, duration
                        )
                    }
                    _ => inner,
                }
            }
            None => {
                // No aggregation, just selector
                if let Some(ref d) = self.range_duration {
                    format!("{}[{}]", selector, d)
                } else {
                    selector
                }
            }
        };

        // Add GROUP BY as outer aggregation if rate/irate/increase + group by
        if !self.group_by_labels.is_empty() {
            if let Some(ref agg) = self.aggregation {
                match agg.func_name.as_str() {
                    "rate" | "irate" | "increase" => {
                        query = format!(
                            "sum by ({}) ({})",
                            self.group_by_labels.join(", "),
                            query
                        );
                    }
                    "count" => {
                        if self.range_duration.is_some() {
                            query = format!(
                                "sum by ({}) ({})",
                                self.group_by_labels.join(", "),
                                query
                            );
                        }
                    }
                    _ => {} // already handled above
                }
            }
        }

        // Add HAVING as threshold filter
        if let Some(ref having) = self.having_expr {
            query = format!("{} {}", query, having);
        }

        Ok(query)
    }

    fn build_label_matchers(&self) -> String {
        self.label_matchers
            .iter()
            .map(|m| format!("{}{}\"{}\"", m.label, m.op, m.value))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

// ─── AST → PromQL ──────────────────────────────────────────────────────────────

pub fn transpile(query: &Query) -> Result<String, TranspileError> {
    // Validate signal types
    if let Some(ref from) = query.from {
        for source in &from.sources {
            match source.signal_type {
                SignalType::Metrics | SignalType::Unknown(_) => {}
                ref other => return Err(TranspileError::UnsupportedSignalType {
                    backend: "promql".to_string(),
                    signal: other.clone(),
                }),
            }
        }
    }

    // Reject CORRELATE
    if query.correlate.is_some() {
        return Err(TranspileError::CorrelateNotSupported);
    }

    let mut builder = PromQLBuilder::new();

    // Extract WHERE conditions into label matchers
    if let Some(ref wc) = query.where_clause {
        extract_conditions(&wc.condition, &mut builder)?;
    }

    // Extract WITHIN as range duration
    if let Some(WithinClause::Last(d)) = &query.within {
        builder.range_duration = Some(d.clone());
    }

    // Extract COMPUTE
    if let Some(ref compute) = query.compute {
        if compute.functions.len() > 1 {
            return Err(TranspileError::UnsupportedExpression(
                "PromQL supports only one aggregation per query. Split into multiple queries.".to_string()
            ));
        }
        if let Some(func) = compute.functions.first() {
            let func_name = func.name.to_lowercase();

            // Extract duration from args if present
            for arg in &func.args {
                if let Expr::DurationLit(d) = arg {
                    builder.range_duration = Some(d.clone());
                }
            }

            builder.aggregation = Some(AggregationInfo {
                func_name,
                wrapper_func: None,
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

    // Extract HAVING as comparison
    if let Some(ref having) = query.having {
        builder.having_expr = Some(expr_to_promql_filter(&having.condition));
    }

    builder.build()
}

fn extract_conditions(expr: &Expr, builder: &mut PromQLBuilder) -> Result<(), TranspileError> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOp::And,
            right,
        } => {
            extract_conditions(left, builder)?;
            extract_conditions(right, builder)?;
        }

        Expr::BinaryOp {
            left,
            op: BinaryOp::Or,
            right,
        } => {
            // OR on same label → regex matcher: service="a" OR service="b" → service=~"a|b"
            let or_values = collect_or_values(expr);
            if let Some((label, values)) = or_values {
                if label == "__name__" {
                    // OR on metric names → regex __name__ matcher
                    let regex = values.join("|");
                    builder.label_matchers.push(LabelMatcher {
                        label: "__name__".to_string(),
                        op: "=~".to_string(),
                        value: regex,
                    });
                } else {
                    let regex = values.join("|");
                    builder.label_matchers.push(LabelMatcher {
                        label,
                        op: "=~".to_string(),
                        value: regex,
                    });
                }
            } else {
                // OR on different labels — extract both sides independently
                extract_conditions(left, builder)?;
                extract_conditions(right, builder)?;
            }
        }

        Expr::BinaryOp {
            left,
            op,
            right,
        } => {
            let label = expr_to_label(left);
            let value = expr_to_value(right);

            if let (Some(label), Some(value)) = (label, value) {
                // Special case: __name__ = "metric_name" → set metric name
                if label == "__name__" {
                    if let BinaryOp::Eq = op {
                        builder.metric_name = Some(value);
                        return Ok(());
                    }
                }

                let op_str = match op {
                    BinaryOp::Eq => "=",
                    BinaryOp::Neq => "!=",
                    BinaryOp::RegexMatch => "=~",
                    BinaryOp::RegexNoMatch => "!~",
                    _ => "=", // fallback
                };

                builder.label_matchers.push(LabelMatcher {
                    label,
                    op: op_str.to_string(),
                    value,
                });
            }
        }

        Expr::StringMatch {
            expr,
            op,
            pattern,
        } => {
            let label = expr_to_label(expr);
            if let Some(label) = label {
                let regex_pattern = match op {
                    StringMatchOp::Contains => format!(".*{}.*", pattern),
                    StringMatchOp::StartsWith => format!("{}.*", pattern),
                    StringMatchOp::Matches => pattern.clone(),
                };
                builder.label_matchers.push(LabelMatcher {
                    label,
                    op: "=~".to_string(),
                    value: regex_pattern,
                });
            }
        }

        Expr::InList { expr, list, .. } => {
            let label = expr_to_label(expr);
            if let Some(label) = label {
                let values: Vec<String> = list
                    .iter()
                    .filter_map(|e| match e {
                        Expr::StringLit(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
                let regex = values.join("|");
                builder.label_matchers.push(LabelMatcher {
                    label,
                    op: "=~".to_string(),
                    value: regex,
                });
            }
        }

        _ => {}
    }

    Ok(())
}

/// Collect OR'd equality conditions on the same label.
/// e.g. service = "a" OR service = "b" → Some(("service", ["a", "b"]))
/// Returns None if the OR branches don't share the same label or aren't simple equalities.
fn collect_or_values(expr: &Expr) -> Option<(String, Vec<String>)> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOp::Or,
            right,
        } => {
            let left_vals = collect_or_values(left);
            let right_vals = collect_or_values(right);
            match (left_vals, right_vals) {
                (Some((l_label, mut l_vals)), Some((r_label, r_vals))) if l_label == r_label => {
                    l_vals.extend(r_vals);
                    Some((l_label, l_vals))
                }
                _ => None,
            }
        }
        Expr::BinaryOp {
            left,
            op: BinaryOp::Eq,
            right,
        } => {
            let label = expr_to_label(left);
            let value = expr_to_value(right);
            match (label, value) {
                (Some(l), Some(v)) => Some((l, vec![v])),
                _ => None,
            }
        }
        _ => None,
    }
}

fn expr_to_label(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name) => Some(name.clone()),
        Expr::QualifiedIdent(parts) => {
            // metrics.__name__ → __name__, labels.env → env
            if parts.len() >= 2 {
                // Skip the signal type prefix
                Some(parts[1..].join("."))
            } else {
                Some(parts.join("."))
            }
        }
        _ => None,
    }
}

fn expr_to_value(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLit(s) => Some(s.clone()),
        Expr::NumberLit(n) => Some(format!("{}", n)),
        Expr::Ident(s) => Some(s.clone()),
        _ => None,
    }
}

/// Convert a HAVING expression to a PromQL threshold comparison.
/// HAVING rate > 0.01 → "> 0.01" (the aggregate result is the LHS implicitly)
/// HAVING count * 100 / total > 5 → "> 5" for simple cases,
///   or full arithmetic expression when both sides are complex.
fn expr_to_promql_filter(expr: &Expr) -> String {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            let op_str = match op {
                BinaryOp::Gt => ">",
                BinaryOp::Lt => "<",
                BinaryOp::Gte => ">=",
                BinaryOp::Lte => "<=",
                BinaryOp::Eq => "==",
                BinaryOp::Neq => "!=",
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                _ => "==",
            };
            // For comparison operators, check if LHS is a simple aggregate ref
            // If so, emit only "op value" since the aggregation result is implicit in PromQL
            match op {
                BinaryOp::Gt | BinaryOp::Lt | BinaryOp::Gte | BinaryOp::Lte
                | BinaryOp::Eq | BinaryOp::Neq => {
                    if is_aggregate_ref(left) {
                        let r = expr_to_promql_value(right);
                        format!("{} {}", op_str, r)
                    } else {
                        let l = expr_to_promql_value(left);
                        let r = expr_to_promql_value(right);
                        format!("{} {} {}", l, op_str, r)
                    }
                }
                _ => {
                    let l = expr_to_promql_value(left);
                    let r = expr_to_promql_value(right);
                    format!("{} {} {}", l, op_str, r)
                }
            }
        }
        _ => expr_to_promql_value(expr),
    }
}

fn expr_to_promql_value(expr: &Expr) -> String {
    match expr {
        Expr::Ident(name) => name.clone(),
        Expr::NumberLit(n) => format!("{}", n),
        Expr::BinaryOp { left, op, right } => {
            let l = expr_to_promql_value(left);
            let r = expr_to_promql_value(right);
            let op_str = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                _ => "",
            };
            format!("{} {} {}", l, op_str, r)
        }
        _ => String::new(),
    }
}

/// Check if an expression is a reference to an aggregate function result
/// (e.g., "rate", "count", "avg" — simple idents that match COMPUTE function names)
fn is_aggregate_ref(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name) => config::is_aggregate_function(name),
        _ => false,
    }
}

// ─── Normalized Path ─────────────────────────────────────────────────────────

/// Transpile using pre-computed binder/normalizer data.
/// Reads BoundConditions directly instead of re-walking the AST.
fn transpile_from_normalized(nq: &NormalizedQuery) -> Result<String, TranspileError> {
    let query = &nq.bound.query;

    // Validate signal types
    if let Some(ref from) = query.from {
        for source in &from.sources {
            match source.signal_type {
                SignalType::Metrics | SignalType::Unknown(_) => {}
                ref other => return Err(TranspileError::UnsupportedSignalType {
                    backend: "promql".to_string(),
                    signal: other.clone(),
                }),
            }
        }
    }

    if query.correlate.is_some() {
        return Err(TranspileError::CorrelateNotSupported);
    }

    let mut builder = PromQLBuilder::new();

    // Use bound conditions directly
    for cond in &nq.bound.conditions {
        match cond {
            BoundCondition::MetricName(name) => {
                builder.metric_name = Some(name.clone());
            }
            BoundCondition::StreamLabel { name, op, value } => {
                builder.label_matchers.push(LabelMatcher {
                    label: name.clone(),
                    op: op.as_promql_str().to_string(),
                    value: value.clone(),
                });
            }
            BoundCondition::FieldFilter { name, op, value } => {
                builder.label_matchers.push(LabelMatcher {
                    label: name.clone(),
                    op: op.as_promql_str().to_string(),
                    value: value.clone(),
                });
            }
            BoundCondition::OrGroup(BoundOrGroup { field, values }) => {
                let regex = values.join("|");
                if field == "__name__" {
                    builder.label_matchers.push(LabelMatcher {
                        label: "__name__".to_string(),
                        op: "=~".to_string(),
                        value: regex,
                    });
                } else {
                    builder.label_matchers.push(LabelMatcher {
                        label: field.clone(),
                        op: "=~".to_string(),
                        value: regex,
                    });
                }
            }
            BoundCondition::InList { name, values, .. } => {
                let regex = values.join("|");
                builder.label_matchers.push(LabelMatcher {
                    label: name.clone(),
                    op: "=~".to_string(),
                    value: regex,
                });
            }
            BoundCondition::FieldStringMatch { name, match_op, pattern } => {
                let regex_pattern = match match_op {
                    StringMatchOp::Contains => format!(".*{}.*", pattern),
                    StringMatchOp::StartsWith => format!("{}.*", pattern),
                    StringMatchOp::Matches => pattern.clone(),
                };
                builder.label_matchers.push(LabelMatcher {
                    label: name.clone(),
                    op: "=~".to_string(),
                    value: regex_pattern,
                });
            }
            BoundCondition::ContentFilter { .. } => {
                // PromQL doesn't have content filters — skip
            }
            BoundCondition::Native { backend, query } => {
                match backend.as_deref() {
                    None | Some("promql") | Some("metricsql") | Some("prometheus") => {
                        // Native fragment injected directly as a raw label matcher
                        builder.native_fragments.push(query.clone());
                    }
                    Some(other) => {
                        return Err(TranspileError::UnsupportedExpression(
                            format!("NATIVE('{}', ...) cannot be transpiled to PromQL", other),
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

    // Use normalized aggregation
    if let Some(ref agg) = nq.aggregation {
        builder.aggregation = Some(AggregationInfo {
            func_name: agg.func_name.clone(),
            wrapper_func: None,
        });
    }

    // Use normalized GROUP BY
    builder.group_by_labels = nq.group_by_labels.clone();

    // Use normalized HAVING
    if let Some(ref having) = nq.having {
        let having_str = if let Some(ref full) = having.full_expr {
            full.clone()
        } else if let Some(ref lhs) = having.lhs {
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

    /// Helper: assert old and new paths produce identical output.
    fn assert_mirror(input: &str) {
        let old = transpile_query(input).unwrap();
        let new = transpile_normalized_query(input).unwrap();
        assert_eq!(old, new, "Mirror mismatch for: {}", input);
    }

    #[test]
    fn test_simple_metric_select() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND job = \"api\"",
        )
        .unwrap();
        assert_eq!(result, "http_requests_total{job=\"api\"}");
    }

    #[test]
    fn test_rate() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" WITHIN last 5m COMPUTE rate(value, 5m)",
        )
        .unwrap();
        assert_eq!(result, "rate(http_requests_total[5m])");
    }

    #[test]
    fn test_rate_with_labels() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND env = \"prod\" WITHIN last 5m COMPUTE rate(value, 5m)",
        )
        .unwrap();
        assert_eq!(result, "rate(http_requests_total{env=\"prod\"}[5m])");
    }

    #[test]
    fn test_rate_with_group_by() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" COMPUTE rate(value, 5m) GROUP BY service",
        )
        .unwrap();
        assert_eq!(
            result,
            "sum by (service) (rate(http_requests_total[5m]))"
        );
    }

    #[test]
    fn test_avg_aggregation() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"node_cpu_seconds_total\" COMPUTE avg(value) GROUP BY instance",
        )
        .unwrap();
        assert_eq!(
            result,
            "avg by (instance) (node_cpu_seconds_total)"
        );
    }

    #[test]
    fn test_regex_match() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND service =~ \"api.*\"",
        )
        .unwrap();
        assert_eq!(
            result,
            "http_requests_total{service=~\"api.*\"}"
        );
    }

    #[test]
    fn test_in_list_to_regex() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND service IN [\"nginx\", \"envoy\"]",
        )
        .unwrap();
        assert_eq!(
            result,
            "http_requests_total{service=~\"nginx|envoy\"}"
        );
    }

    #[test]
    fn test_contains_to_regex() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND path CONTAINS \"api\"",
        )
        .unwrap();
        assert_eq!(
            result,
            "http_requests_total{path=~\".*api.*\"}"
        );
    }

    #[test]
    fn test_starts_with_to_regex() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND host STARTS WITH \"prod-\"",
        )
        .unwrap();
        assert_eq!(
            result,
            "http_requests_total{host=~\"prod-.*\"}"
        );
    }

    #[test]
    fn test_having_filter() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" COMPUTE rate(value, 5m) GROUP BY service HAVING rate > 0.01",
        )
        .unwrap();
        assert_eq!(
            result,
            "sum by (service) (rate(http_requests_total[5m])) > 0.01"
        );
    }

    #[test]
    fn test_p99() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_request_duration_seconds_bucket\" WITHIN last 5m COMPUTE p99(value)",
        )
        .unwrap();
        assert_eq!(
            result,
            "histogram_quantile(0.99, rate(http_request_duration_seconds_bucket[5m]))"
        );
    }

    #[test]
    fn test_rejects_logs() {
        let result = transpile_query("FROM logs WHERE service = \"api\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_rejects_correlate() {
        let result = transpile_query(
            "FROM metrics, logs CORRELATE ON service WITHIN 30s",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_labels() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND job = \"api\" AND env = \"prod\" AND region = \"us-east\"",
        )
        .unwrap();
        assert_eq!(
            result,
            "http_requests_total{job=\"api\", env=\"prod\", region=\"us-east\"}"
        );
    }

    #[test]
    fn test_aetheris_snmp_query() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"ifInErrors\" AND device_type = \"router\" WITHIN last 5m COMPUTE rate(value, 5m) GROUP BY host",
        )
        .unwrap();
        assert_eq!(
            result,
            "sum by (host) (rate(ifInErrors{device_type=\"router\"}[5m]))"
        );
    }

    #[test]
    fn test_or_same_label_to_regex() {
        // Use parentheses to isolate OR on same label
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND (service = \"nginx\" OR service = \"envoy\")",
        )
        .unwrap();
        // OR on same label → regex matcher
        assert!(result.contains("service=~\"nginx|envoy\"") || result.contains("service=~\"envoy|nginx\""));
    }

    #[test]
    fn test_having_comparison_only() {
        let result = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" COMPUTE rate(value, 5m) GROUP BY service HAVING rate > 0.01",
        )
        .unwrap();
        // HAVING should produce valid PromQL: "> 0.01" not "rate > 0.01"
        assert!(result.ends_with("> 0.01"));
        assert!(!result.contains("rate > 0.01") || result.contains(") > 0.01"));
    }

    // ─── Mirror Tests: assert old == new path ────────────────────────────────

    #[test]
    fn test_mirror_simple_metric() {
        assert_mirror(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND job = \"api\""
        );
    }

    #[test]
    fn test_mirror_rate() {
        assert_mirror(
            "FROM metrics WHERE __name__ = \"http_requests_total\" WITHIN last 5m COMPUTE rate(value, 5m)"
        );
    }

    #[test]
    fn test_mirror_rate_with_labels() {
        assert_mirror(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND env = \"prod\" WITHIN last 5m COMPUTE rate(value, 5m)"
        );
    }

    #[test]
    fn test_mirror_rate_group_by() {
        assert_mirror(
            "FROM metrics WHERE __name__ = \"http_requests_total\" COMPUTE rate(value, 5m) GROUP BY service"
        );
    }

    #[test]
    fn test_mirror_avg() {
        assert_mirror(
            "FROM metrics WHERE __name__ = \"node_cpu_seconds_total\" COMPUTE avg(value) GROUP BY instance"
        );
    }

    #[test]
    fn test_mirror_in_list() {
        assert_mirror(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND service IN [\"nginx\", \"envoy\"]"
        );
    }

    #[test]
    fn test_mirror_contains() {
        assert_mirror(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND path CONTAINS \"api\""
        );
    }

    #[test]
    fn test_mirror_having() {
        assert_mirror(
            "FROM metrics WHERE __name__ = \"http_requests_total\" COMPUTE rate(value, 5m) GROUP BY service HAVING rate > 0.01"
        );
    }

    #[test]
    fn test_mirror_p99() {
        assert_mirror(
            "FROM metrics WHERE __name__ = \"http_request_duration_seconds_bucket\" WITHIN last 5m COMPUTE p99(value)"
        );
    }

    #[test]
    fn test_mirror_multiple_labels() {
        assert_mirror(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND job = \"api\" AND env = \"prod\" AND region = \"us-east\""
        );
    }

    #[test]
    fn test_mirror_or_regex() {
        // OR on same label → regex
        let old = transpile_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND (service = \"nginx\" OR service = \"envoy\")"
        ).unwrap();
        let new = transpile_normalized_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND (service = \"nginx\" OR service = \"envoy\")"
        ).unwrap();
        assert_eq!(old, new);
    }

    // ─── NATIVE Tests ────────────────────────────────────────────────────

    #[test]
    fn test_native_full_query() {
        let result = transpile_normalized_query(
            "FROM metrics WHERE NATIVE(\"rate(up{job='api'}[5m])\")"
        ).unwrap();
        assert_eq!(result, "rate(up{job='api'}[5m])");
    }

    #[test]
    fn test_native_partial_in_where() {
        let result = transpile_normalized_query(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND NATIVE(\"job=~'api.*'\")"
        ).unwrap();
        assert!(result.contains("job=~'api.*'"), "Got: {}", result);
        assert!(result.contains("http_requests_total"), "Got: {}", result);
    }

    #[test]
    fn test_native_with_backend() {
        let result = transpile_normalized_query(
            "FROM metrics WHERE NATIVE(\"promql\", \"up{job='api'}\")"
        ).unwrap();
        assert_eq!(result, "up{job='api'}");
    }

    #[test]
    fn test_native_wrong_backend() {
        let result = transpile_normalized_query(
            "FROM metrics WHERE NATIVE(\"logql\", \"something\")"
        );
        assert!(result.is_err());
    }
}
