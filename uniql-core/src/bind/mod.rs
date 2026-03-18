//! UNIQL Binder — Resolves identifiers, classifies conditions, unifies stream labels.
//!
//! Runs after expansion/validation, before transpilation.
//! Eliminates 3x duplicated logic across transpilers:
//!   - QualifiedIdent resolution (expr_to_label / expr_to_label_name / get_field_name)
//!   - Stream label whitelisting (is_stream_label / is_stream_field)
//!   - OR flattening (collect_or_values — was only in PromQL)

use crate::ast::*;

// ─── Bound Query ─────────────────────────────────────────────────────────────

/// Output of the binder: a query with all identifiers resolved and conditions classified.
#[derive(Debug, Clone)]
pub struct BoundQuery {
    pub query: Query,
    pub conditions: Vec<BoundCondition>,
    pub signal_type: SignalType,
}

/// A single resolved WHERE condition.
#[derive(Debug, Clone)]
pub enum BoundCondition {
    /// Stream-level label (goes into stream selector: {}, _stream:{})
    StreamLabel {
        name: String,
        op: BoundOp,
        value: String,
    },
    /// Field-level filter (post-parse label filter, field:value, etc.)
    FieldFilter {
        name: String,
        op: BoundOp,
        value: String,
    },
    /// Content/message filter (line filter in LogQL, _msg filter in LogsQL)
    ContentFilter {
        match_op: StringMatchOp,
        pattern: String,
    },
    /// Metric name (__name__ = "metric_name")
    MetricName(String),
    /// OR group on the same field flattened into a single condition
    OrGroup(BoundOrGroup),
    /// IN list on a field
    InList {
        name: String,
        values: Vec<String>,
        negated: bool,
    },
    /// Field-level string match (non-message fields with CONTAINS/STARTS WITH/MATCHES)
    FieldStringMatch {
        name: String,
        match_op: StringMatchOp,
        pattern: String,
    },
    /// Native backend query passthrough
    Native {
        backend: Option<String>,
        query: String,
    },
}

/// A flattened OR group: `service = "a" OR service = "b"` → one condition.
#[derive(Debug, Clone)]
pub struct BoundOrGroup {
    pub field: String,
    pub values: Vec<String>,
}

/// Resolved comparison operator.
#[derive(Debug, Clone, PartialEq)]
pub enum BoundOp {
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
    RegexMatch,
    RegexNoMatch,
}

impl BoundOp {
    pub fn from_binary_op(op: &BinaryOp) -> Self {
        match op {
            BinaryOp::Eq => BoundOp::Eq,
            BinaryOp::Neq => BoundOp::Neq,
            BinaryOp::Gt => BoundOp::Gt,
            BinaryOp::Lt => BoundOp::Lt,
            BinaryOp::Gte => BoundOp::Gte,
            BinaryOp::Lte => BoundOp::Lte,
            BinaryOp::RegexMatch => BoundOp::RegexMatch,
            BinaryOp::RegexNoMatch => BoundOp::RegexNoMatch,
            // Logical and arithmetic ops should not appear in condition LHS → Eq fallback.
            // AND/OR are split before reaching this point; arithmetic ops are rare in WHERE.
            BinaryOp::And | BinaryOp::Or
            | BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => BoundOp::Eq,
        }
    }

    pub fn as_promql_str(&self) -> &str {
        match self {
            BoundOp::Eq => "=",
            BoundOp::Neq => "!=",
            BoundOp::RegexMatch => "=~",
            BoundOp::RegexNoMatch => "!~",
            _ => "=",
        }
    }

    pub fn as_comparison_str(&self) -> &str {
        match self {
            BoundOp::Eq => "=",
            BoundOp::Neq => "!=",
            BoundOp::Gt => ">",
            BoundOp::Lt => "<",
            BoundOp::Gte => ">=",
            BoundOp::Lte => "<=",
            BoundOp::RegexMatch => "=~",
            BoundOp::RegexNoMatch => "!~",
        }
    }
}

// ─── Unified Stream Label Whitelist ──────────────────────────────────────────

/// Unified stream label whitelist.
/// These labels go into the stream selector ({} in PromQL/LogQL, _stream:{} in LogsQL).
/// Merges the previously inconsistent `is_stream_label` (LogQL) and `is_stream_field` (LogsQL).
/// LogsQL had `device_type` but not `filename`; LogQL had `filename` but not `device_type`.
/// The unified list includes both.
pub fn is_stream_label(name: &str) -> bool {
    matches!(
        name,
        "job" | "service" | "namespace" | "container" | "pod" | "host"
        | "instance" | "env" | "environment" | "cluster" | "region"
        | "app" | "component" | "filename" | "device_type"
    )
}

/// Check if a field name refers to message content (should become a content/line filter).
fn is_message_field(name: &str) -> bool {
    matches!(name, "message" | "_msg" | "msg" | "content")
}

// ─── Identifier Resolution ───────────────────────────────────────────────────

/// Resolve a label name from an expression.
/// Handles QualifiedIdent by stripping signal type prefix (e.g., metrics.__name__ → __name__).
/// Replaces: expr_to_label (PromQL), expr_to_label_name (LogQL), get_field_name (LogsQL).
pub fn resolve_label_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name) => Some(name.clone()),
        Expr::QualifiedIdent(parts) => {
            if parts.len() >= 2 {
                // Skip signal type prefix: metrics.__name__ → __name__, logs.level → level
                Some(parts[1..].join("."))
            } else {
                Some(parts.join("."))
            }
        }
        _ => None,
    }
}

/// Resolve a scalar value from an expression.
pub fn resolve_value(expr: &Expr) -> Option<String> {
    match expr {
        Expr::StringLit(s) => Some(s.clone()),
        Expr::NumberLit(n) => Some(format!("{}", n)),
        Expr::Ident(s) => Some(s.clone()),
        _ => None,
    }
}

// ─── Binding ─────────────────────────────────────────────────────────────────

/// Bind a prepared (parsed + expanded + validated) query.
/// Resolves identifiers, classifies conditions, validates prefix matches.
pub fn bind(query: &Query) -> Result<BoundQuery, String> {
    let signal_type = query.inferred_signal_types()
        .into_iter()
        .next()
        .unwrap_or(SignalType::Unknown("unknown".to_string()));

    let mut conditions = Vec::new();

    if let Some(ref wc) = query.where_clause {
        extract_bound_conditions(&wc.condition, &signal_type, &mut conditions)?;
    }

    Ok(BoundQuery {
        query: query.clone(),
        conditions,
        signal_type,
    })
}

fn extract_bound_conditions(
    expr: &Expr,
    signal_type: &SignalType,
    out: &mut Vec<BoundCondition>,
) -> Result<(), String> {
    match expr {
        Expr::BinaryOp { left, op: BinaryOp::And, right } => {
            extract_bound_conditions(left, signal_type, out)?;
            extract_bound_conditions(right, signal_type, out)?;
        }

        Expr::BinaryOp { left: _, op: BinaryOp::Or, right: _ } => {
            // Try to flatten OR on same field
            if let Some(or_group) = collect_or_values(expr) {
                out.push(BoundCondition::OrGroup(or_group));
            } else {
                // OR on different fields — extract both sides
                extract_or_branches(expr, signal_type, out)?;
            }
        }

        Expr::BinaryOp { left, op, right } => {
            // Detect arithmetic in WHERE: `cpu + mem > 100` has BinaryOp(Add) as LHS of Gt
            if contains_arithmetic(left) || contains_arithmetic(right) {
                return Err(format!(
                    "Arithmetic expressions in WHERE clause are not supported. Use COMPUTE for calculations."
                ));
            }

            // Handle reversed comparisons: `100 < cpu` → normalize to `cpu > 100`
            let (label, value, bound_op) = if resolve_label_name(left).is_some() {
                (resolve_label_name(left), resolve_value(right), BoundOp::from_binary_op(op))
            } else if resolve_label_name(right).is_some() {
                // Reversed: LHS is a value, RHS is a label → flip
                (resolve_label_name(right), resolve_value(left), flip_comparison(BoundOp::from_binary_op(op)))
            } else {
                (None, None, BoundOp::Eq)
            };

            if let (Some(label), Some(value)) = (label, value) {
                if label == "__name__" && bound_op == BoundOp::Eq {
                    out.push(BoundCondition::MetricName(value));
                } else if is_stream_label(&label) {
                    out.push(BoundCondition::StreamLabel {
                        name: label,
                        op: bound_op,
                        value,
                    });
                } else {
                    out.push(BoundCondition::FieldFilter {
                        name: label,
                        op: bound_op,
                        value,
                    });
                }
            }
        }

        Expr::StringMatch { expr: inner, op, pattern } => {
            let label = resolve_label_name(inner);
            if let Some(label) = label {
                if is_message_field(&label) {
                    out.push(BoundCondition::ContentFilter {
                        match_op: op.clone(),
                        pattern: pattern.clone(),
                    });
                } else {
                    out.push(BoundCondition::FieldStringMatch {
                        name: label,
                        match_op: op.clone(),
                        pattern: pattern.clone(),
                    });
                }
            }
        }

        Expr::InList { expr: inner, list, negated } => {
            let label = resolve_label_name(inner);
            if let Some(label) = label {
                let values: Vec<String> = list.iter()
                    .filter_map(|e| match e {
                        Expr::StringLit(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
                out.push(BoundCondition::InList {
                    name: label,
                    values,
                    negated: *negated,
                });
            }
        }

        Expr::Native { backend, query } => {
            out.push(BoundCondition::Native {
                backend: backend.clone(),
                query: query.clone(),
            });
        }

        _ => {}
    }

    Ok(())
}

fn extract_or_branches(
    expr: &Expr,
    signal_type: &SignalType,
    out: &mut Vec<BoundCondition>,
) -> Result<(), String> {
    match expr {
        Expr::BinaryOp { left, op: BinaryOp::Or, right } => {
            extract_or_branches(left, signal_type, out)?;
            extract_or_branches(right, signal_type, out)?;
        }
        _ => {
            extract_bound_conditions(expr, signal_type, out)?;
        }
    }
    Ok(())
}

/// Collect OR'd equality conditions on the same field into a BoundOrGroup.
/// e.g., `service = "a" OR service = "b"` → BoundOrGroup { field: "service", values: ["a", "b"] }
fn collect_or_values(expr: &Expr) -> Option<BoundOrGroup> {
    match expr {
        Expr::BinaryOp { left, op: BinaryOp::Or, right } => {
            let l = collect_or_values(left);
            let r = collect_or_values(right);
            match (l, r) {
                (Some(mut lg), Some(rg)) if lg.field == rg.field => {
                    lg.values.extend(rg.values);
                    Some(lg)
                }
                _ => None,
            }
        }
        Expr::BinaryOp { left, op: BinaryOp::Eq, right } => {
            let label = resolve_label_name(left);
            let value = resolve_value(right);
            match (label, value) {
                (Some(l), Some(v)) => Some(BoundOrGroup { field: l, values: vec![v] }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Check if an expression contains arithmetic operators (Add/Sub/Mul/Div/Mod).
fn contains_arithmetic(expr: &Expr) -> bool {
    match expr {
        Expr::BinaryOp { op, left, right, .. } => {
            matches!(op, BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod)
                || contains_arithmetic(left)
                || contains_arithmetic(right)
        }
        _ => false,
    }
}

/// Flip a comparison operator for reversed expressions.
/// `100 < cpu` → `cpu > 100` (Lt becomes Gt, etc.)
fn flip_comparison(op: BoundOp) -> BoundOp {
    match op {
        BoundOp::Gt => BoundOp::Lt,
        BoundOp::Lt => BoundOp::Gt,
        BoundOp::Gte => BoundOp::Lte,
        BoundOp::Lte => BoundOp::Gte,
        other => other, // Eq, Neq, Regex — symmetric
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_label_simple() {
        let expr = Expr::Ident("service".into());
        assert_eq!(resolve_label_name(&expr), Some("service".to_string()));
    }

    #[test]
    fn test_resolve_label_qualified() {
        let expr = Expr::QualifiedIdent(vec!["metrics".into(), "__name__".into()]);
        assert_eq!(resolve_label_name(&expr), Some("__name__".to_string()));
    }

    #[test]
    fn test_resolve_label_deep_qualified() {
        let expr = Expr::QualifiedIdent(vec!["logs".into(), "http".into(), "status".into()]);
        assert_eq!(resolve_label_name(&expr), Some("http.status".to_string()));
    }

    #[test]
    fn test_unified_stream_labels() {
        // From LogQL only
        assert!(is_stream_label("filename"));
        // From LogsQL only
        assert!(is_stream_label("device_type"));
        // Common
        assert!(is_stream_label("service"));
        assert!(is_stream_label("job"));
        assert!(is_stream_label("host"));
        // Not stream
        assert!(!is_stream_label("level"));
        assert!(!is_stream_label("status_code"));
    }

    #[test]
    fn test_bind_simple_metric() {
        let ast = crate::prepare("FROM metrics WHERE __name__ = \"http_requests_total\" AND job = \"api\"").unwrap();
        let bound = bind(&ast).unwrap();
        assert_eq!(bound.conditions.len(), 2);
        assert!(matches!(&bound.conditions[0], BoundCondition::MetricName(n) if n == "http_requests_total"));
        assert!(matches!(&bound.conditions[1], BoundCondition::StreamLabel { name, .. } if name == "job"));
    }

    #[test]
    fn test_bind_log_content_filter() {
        let ast = crate::prepare("FROM logs WHERE service = \"api\" AND message CONTAINS \"error\"").unwrap();
        let bound = bind(&ast).unwrap();
        assert_eq!(bound.conditions.len(), 2);
        assert!(matches!(&bound.conditions[0], BoundCondition::StreamLabel { name, .. } if name == "service"));
        assert!(matches!(&bound.conditions[1], BoundCondition::ContentFilter { pattern, .. } if pattern == "error"));
    }

    #[test]
    fn test_bind_or_flattening() {
        let ast = crate::prepare(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND (service = \"nginx\" OR service = \"envoy\")"
        ).unwrap();
        let bound = bind(&ast).unwrap();
        let or_group = bound.conditions.iter().find(|c| matches!(c, BoundCondition::OrGroup(_)));
        assert!(or_group.is_some());
        if let Some(BoundCondition::OrGroup(g)) = or_group {
            assert_eq!(g.field, "service");
            assert_eq!(g.values.len(), 2);
        }
    }

    #[test]
    fn test_bind_in_list() {
        let ast = crate::prepare(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND service IN [\"nginx\", \"envoy\"]"
        ).unwrap();
        let bound = bind(&ast).unwrap();
        let in_cond = bound.conditions.iter().find(|c| matches!(c, BoundCondition::InList { .. }));
        assert!(in_cond.is_some());
        if let Some(BoundCondition::InList { name, values, negated }) = in_cond {
            assert_eq!(name, "service");
            assert_eq!(values.len(), 2);
            assert!(!negated);
        }
    }

    #[test]
    fn test_bind_field_string_match() {
        let ast = crate::prepare(
            "FROM metrics WHERE __name__ = \"http_requests_total\" AND path CONTAINS \"api\""
        ).unwrap();
        let bound = bind(&ast).unwrap();
        let sm = bound.conditions.iter().find(|c| matches!(c, BoundCondition::FieldStringMatch { .. }));
        assert!(sm.is_some());
    }

    #[test]
    fn test_bound_op_promql_str() {
        assert_eq!(BoundOp::Eq.as_promql_str(), "=");
        assert_eq!(BoundOp::Neq.as_promql_str(), "!=");
        assert_eq!(BoundOp::RegexMatch.as_promql_str(), "=~");
    }

    #[test]
    fn test_bind_3_way_or() {
        let ast = crate::prepare(
            "FROM metrics WHERE __name__ = \"cpu\" AND (service = \"a\" OR service = \"b\" OR service = \"c\")"
        ).unwrap();
        let bound = bind(&ast).unwrap();
        let or_group = bound.conditions.iter().find(|c| matches!(c, BoundCondition::OrGroup(_)));
        assert!(or_group.is_some());
        if let Some(BoundCondition::OrGroup(g)) = or_group {
            assert_eq!(g.field, "service");
            assert_eq!(g.values.len(), 3);
        }
    }

    #[test]
    fn test_bind_cross_field_or() {
        // OR on different fields should NOT flatten into OrGroup
        let ast = crate::prepare(
            "FROM logs WHERE service = \"api\" OR host = \"prod-01\""
        ).unwrap();
        let bound = bind(&ast).unwrap();
        let or_group = bound.conditions.iter().find(|c| matches!(c, BoundCondition::OrGroup(_)));
        assert!(or_group.is_none(), "Cross-field OR should not create OrGroup");
        // Should produce individual conditions instead
        assert!(bound.conditions.len() >= 2);
    }

    #[test]
    fn test_bind_message_variants() {
        for msg_field in &["message", "_msg", "msg", "content"] {
            let query = format!(
                "FROM logs WHERE service = \"api\" AND {} CONTAINS \"error\"",
                msg_field
            );
            let ast = crate::prepare(&query).unwrap();
            let bound = bind(&ast).unwrap();
            let cf = bound.conditions.iter().find(|c| matches!(c, BoundCondition::ContentFilter { .. }));
            assert!(cf.is_some(), "Field '{}' should produce ContentFilter", msg_field);
        }
    }

    #[test]
    fn test_bind_non_stream_field() {
        let ast = crate::prepare(
            "FROM logs WHERE service = \"api\" AND level = \"error\""
        ).unwrap();
        let bound = bind(&ast).unwrap();
        let ff = bound.conditions.iter().find(|c| matches!(c, BoundCondition::FieldFilter { .. }));
        assert!(ff.is_some(), "level should produce FieldFilter, not StreamLabel");
    }

    #[test]
    fn test_reversed_comparison_number_lt_label() {
        // WHERE 100 < cpu → should become cpu > 100
        let ast = crate::prepare("FROM metrics WHERE __name__ = \"cpu\" AND 100 < usage").unwrap();
        let bound = bind(&ast).unwrap();
        let ff = bound.conditions.iter().find(|c| matches!(c, BoundCondition::FieldFilter { name, .. } if name == "usage"));
        assert!(ff.is_some(), "Reversed comparison should produce FieldFilter for 'usage'");
        if let Some(BoundCondition::FieldFilter { op, value, .. }) = ff {
            assert_eq!(*op, BoundOp::Gt, "100 < usage → usage > 100");
            assert_eq!(value, "100");
        }
    }

    #[test]
    fn test_reversed_comparison_eq_symmetric() {
        // WHERE "nginx" = service → service = "nginx"
        let ast = crate::prepare("FROM metrics WHERE __name__ = \"cpu\" AND \"nginx\" = service").unwrap();
        let bound = bind(&ast).unwrap();
        let sl = bound.conditions.iter().find(|c| matches!(c, BoundCondition::StreamLabel { name, .. } if name == "service"));
        assert!(sl.is_some(), "Reversed eq should produce StreamLabel for 'service'");
    }

    #[test]
    fn test_arithmetic_in_where_rejected() {
        let ast = crate::prepare("FROM metrics WHERE __name__ = \"cpu\" AND cpu + mem > 100").unwrap();
        let result = bind(&ast);
        assert!(result.is_err(), "Arithmetic in WHERE should be rejected");
        assert!(result.unwrap_err().contains("Arithmetic"));
    }

    #[test]
    fn test_flip_comparison() {
        assert_eq!(flip_comparison(BoundOp::Gt), BoundOp::Lt);
        assert_eq!(flip_comparison(BoundOp::Lt), BoundOp::Gt);
        assert_eq!(flip_comparison(BoundOp::Gte), BoundOp::Lte);
        assert_eq!(flip_comparison(BoundOp::Lte), BoundOp::Gte);
        assert_eq!(flip_comparison(BoundOp::Eq), BoundOp::Eq);
        assert_eq!(flip_comparison(BoundOp::Neq), BoundOp::Neq);
    }

    #[test]
    fn test_bound_op_comparison_str() {
        assert_eq!(BoundOp::Gt.as_comparison_str(), ">");
        assert_eq!(BoundOp::Lt.as_comparison_str(), "<");
        assert_eq!(BoundOp::Gte.as_comparison_str(), ">=");
        assert_eq!(BoundOp::Lte.as_comparison_str(), "<=");
    }
}
