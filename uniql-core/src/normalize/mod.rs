//! UNIQL Normalizer — Pre-computes durations, aggregations, HAVING, GROUP BY.
//!
//! Runs after binding, before transpilation.
//! Eliminates 3x duplicated logic across transpilers:
//!   - Duration extraction from COMPUTE args
//!   - Percentile shorthand resolution (p99 → quantile 0.99)
//!   - GROUP BY qualified ident resolution
//!   - HAVING formatting (fixes LogsQL hardcoded "count(*)" bug)

use crate::ast::*;
use crate::bind::BoundQuery;
use crate::config;

// ─── Normalized Query ────────────────────────────────────────────────────────

/// Output of the normalizer: bound query enriched with pre-computed metadata.
#[derive(Debug, Clone)]
pub struct NormalizedQuery {
    pub bound: BoundQuery,
    pub duration: Option<NormalizedDuration>,
    pub aggregation: Option<NormalizedAggregation>,
    pub group_by_labels: Vec<String>,
    pub having: Option<NormalizedHaving>,
}

/// A parsed and validated duration.
#[derive(Debug, Clone)]
pub struct NormalizedDuration {
    pub raw: String,
    pub seconds: f64,
}

/// Resolved aggregation info from COMPUTE clause.
#[derive(Debug, Clone)]
pub struct NormalizedAggregation {
    pub func_name: String,
    pub is_range_function: bool,
    pub quantile_value: Option<String>,
}

/// Pre-computed HAVING expression, using the actual aggregate function name.
/// Fixes the LogsQL bug that hardcoded "count(*)".
#[derive(Debug, Clone)]
pub struct NormalizedHaving {
    /// The actual aggregate function used (e.g., "count", "rate", "avg")
    pub aggregate_func: Option<String>,
    /// The comparison operator as a string
    pub op: String,
    /// The threshold value
    pub value: String,
    /// Full expression for backends that need it (e.g., LogsQL filter pipe)
    pub lhs: Option<String>,
}

// ─── Duration Parsing ────────────────────────────────────────────────────────

/// Parse a duration string into seconds.
/// Supports: ms, s, m, h, d
pub fn parse_duration(raw: &str) -> Result<NormalizedDuration, String> {
    let raw = raw.trim();
    let err = || format!("Invalid duration: {}", raw);

    let seconds = if let Some(num) = raw.strip_suffix("ms") {
        num.parse::<f64>().map_err(|_| err())? / 1000.0
    } else if let Some(num) = raw.strip_suffix('s') {
        num.parse::<f64>().map_err(|_| err())?
    } else if let Some(num) = raw.strip_suffix('m') {
        num.parse::<f64>().map_err(|_| err())? * 60.0
    } else if let Some(num) = raw.strip_suffix('h') {
        num.parse::<f64>().map_err(|_| err())? * 3600.0
    } else if let Some(num) = raw.strip_suffix('d') {
        num.parse::<f64>().map_err(|_| err())? * 86400.0
    } else {
        return Err(format!("Invalid duration suffix: {}", raw));
    };

    Ok(NormalizedDuration {
        raw: raw.to_string(),
        seconds,
    })
}

// ─── Normalization ───────────────────────────────────────────────────────────

/// Normalize a bound query: extract durations, resolve aggregations, pre-compute HAVING.
pub fn normalize(bound: BoundQuery) -> Result<NormalizedQuery, String> {
    let query = &bound.query;

    // Extract duration from WITHIN clause
    let mut duration = match &query.within {
        Some(WithinClause::Last(d)) => Some(parse_duration(d)?),
        _ => None,
    };

    // Extract aggregation from COMPUTE clause
    let mut aggregation = None;
    if let Some(ref compute) = query.compute {
        if let Some(func) = compute.functions.first() {
            let func_name = func.name.to_lowercase();

            // Extract duration from COMPUTE args (overrides WITHIN if present)
            for arg in &func.args {
                if let Expr::DurationLit(d) = arg {
                    duration = Some(parse_duration(d)?);
                }
            }

            // Resolve percentile shorthand
            let quantile_value = config::quantile_for_percentile(&func_name)
                .map(|q| q.to_string());

            aggregation = Some(NormalizedAggregation {
                func_name: func_name.clone(),
                is_range_function: config::is_range_function(&func_name),
                quantile_value,
            });
        }
    }

    // Resolve GROUP BY labels
    let group_by_labels = if let Some(ref gb) = query.group_by {
        gb.fields.iter().map(|field| {
            match field {
                Expr::Ident(name) => name.clone(),
                Expr::QualifiedIdent(parts) => parts.last().cloned().unwrap_or_default(),
                _ => String::new(),
            }
        }).filter(|s| !s.is_empty()).collect()
    } else {
        Vec::new()
    };

    // Pre-compute HAVING
    let having = query.having.as_ref().map(|h| {
        normalize_having(&h.condition, aggregation.as_ref())
    });

    Ok(NormalizedQuery {
        bound,
        duration,
        aggregation,
        group_by_labels,
        having,
    })
}

/// Normalize a HAVING expression.
/// Uses the actual aggregate function name instead of hardcoding "count(*)".
fn normalize_having(expr: &Expr, agg: Option<&NormalizedAggregation>) -> NormalizedHaving {
    let agg_func = agg.map(|a| a.func_name.clone());

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
                BinaryOp::Mod => "%",
                _ => "==",
            };

            let is_agg_ref = matches!(left.as_ref(), Expr::Ident(n) if config::is_aggregate_function(n));
            let lhs = if is_agg_ref {
                None // implicit: aggregate result is the LHS
            } else {
                Some(having_value_to_string(left))
            };

            let value = having_value_to_string(right);

            NormalizedHaving {
                aggregate_func: agg_func,
                op: op_str.to_string(),
                value,
                lhs,
            }
        }
        _ => NormalizedHaving {
            aggregate_func: agg_func,
            op: String::new(),
            value: String::new(),
            lhs: None,
        },
    }
}

fn having_value_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Ident(name) => name.clone(),
        Expr::NumberLit(n) => format!("{}", n),
        Expr::BinaryOp { left, op, right } => {
            let l = having_value_to_string(left);
            let r = having_value_to_string(right);
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

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bind;

    fn prepare_and_normalize(input: &str) -> NormalizedQuery {
        let ast = crate::prepare(input).unwrap();
        let bound = bind::bind(&ast).unwrap();
        normalize(bound).unwrap()
    }

    #[test]
    fn test_parse_duration_seconds() {
        let d = parse_duration("30s").unwrap();
        assert_eq!(d.raw, "30s");
        assert!((d.seconds - 30.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_duration_minutes() {
        let d = parse_duration("5m").unwrap();
        assert!((d.seconds - 300.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_duration_hours() {
        let d = parse_duration("1h").unwrap();
        assert!((d.seconds - 3600.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_duration_millis() {
        let d = parse_duration("500ms").unwrap();
        assert!((d.seconds - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_parse_duration_days() {
        let d = parse_duration("1d").unwrap();
        assert!((d.seconds - 86400.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert!(parse_duration("abc").is_err());
    }

    #[test]
    fn test_normalize_rate_with_duration() {
        let nq = prepare_and_normalize(
            "FROM metrics WHERE __name__ = \"http_requests_total\" COMPUTE rate(value, 5m)"
        );
        assert!(nq.aggregation.is_some());
        let agg = nq.aggregation.unwrap();
        assert_eq!(agg.func_name, "rate");
        assert!(agg.is_range_function);
        assert!(nq.duration.is_some());
        assert!((nq.duration.unwrap().seconds - 300.0).abs() < 0.001);
    }

    #[test]
    fn test_normalize_percentile() {
        let nq = prepare_and_normalize(
            "FROM metrics WHERE __name__ = \"http_request_duration_seconds_bucket\" COMPUTE p99(value)"
        );
        let agg = nq.aggregation.unwrap();
        assert_eq!(agg.func_name, "p99");
        assert_eq!(agg.quantile_value, Some("0.99".to_string()));
    }

    #[test]
    fn test_normalize_group_by() {
        let nq = prepare_and_normalize(
            "FROM metrics WHERE __name__ = \"http_requests_total\" COMPUTE rate(value, 5m) GROUP BY service, region"
        );
        assert_eq!(nq.group_by_labels, vec!["service", "region"]);
    }

    #[test]
    fn test_normalize_having() {
        let nq = prepare_and_normalize(
            "FROM metrics WHERE __name__ = \"http_requests_total\" COMPUTE rate(value, 5m) GROUP BY service HAVING rate > 0.01"
        );
        assert!(nq.having.is_some());
        let having = nq.having.unwrap();
        assert_eq!(having.op, ">");
        assert_eq!(having.value, "0.01");
        assert!(having.lhs.is_none()); // "rate" is aggregate ref → implicit
        assert_eq!(having.aggregate_func, Some("rate".to_string()));
    }

    #[test]
    fn test_normalize_having_uses_actual_aggregate() {
        // This is the bug fix: LogsQL used to hardcode "count(*)" regardless of actual aggregate
        let nq = prepare_and_normalize(
            "FROM logs WHERE service = \"api\" COMPUTE count() GROUP BY level HAVING count > 100"
        );
        let having = nq.having.unwrap();
        assert_eq!(having.aggregate_func, Some("count".to_string()));
        assert_eq!(having.op, ">");
        assert_eq!(having.value, "100");
    }

    #[test]
    fn test_normalize_within_duration() {
        let nq = prepare_and_normalize(
            "FROM logs WHERE service = \"api\" WITHIN last 15m"
        );
        assert!(nq.duration.is_some());
        assert!((nq.duration.unwrap().seconds - 900.0).abs() < 0.001);
    }

    #[test]
    fn test_normalize_compute_duration_overrides_within() {
        let nq = prepare_and_normalize(
            "FROM metrics WHERE __name__ = \"http_requests_total\" WITHIN last 1h COMPUTE rate(value, 5m)"
        );
        // COMPUTE duration (5m) should be extracted
        assert!(nq.duration.is_some());
        assert_eq!(nq.duration.unwrap().raw, "5m");
    }

    #[test]
    fn test_parse_duration_zero() {
        let d = parse_duration("0s").unwrap();
        assert!((d.seconds - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_duration_whitespace() {
        let d = parse_duration("  5m  ").unwrap();
        assert!((d.seconds - 300.0).abs() < 0.001);
    }

    #[test]
    fn test_parse_duration_no_number() {
        assert!(parse_duration("s").is_err());
        assert!(parse_duration("m").is_err());
    }

    #[test]
    fn test_normalize_no_compute() {
        let nq = prepare_and_normalize(
            "FROM logs WHERE service = \"api\""
        );
        assert!(nq.aggregation.is_none());
        assert!(nq.having.is_none());
        assert!(nq.group_by_labels.is_empty());
    }

    #[test]
    fn test_normalize_group_by_qualified() {
        let nq = prepare_and_normalize(
            "FROM metrics WHERE __name__ = \"cpu\" COMPUTE avg(value) GROUP BY labels.env"
        );
        assert_eq!(nq.group_by_labels, vec!["env"]);
    }
}
