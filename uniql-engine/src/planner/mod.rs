//! Query Planner — Decomposes multi-signal UNIQL queries into sub-queries per backend.
//!
//! Input:  AST with FROM metrics, logs ... CORRELATE ON host WITHIN 60s
//! Output: QueryPlan with separate sub-queries for each signal/backend

use uniql_core::ast::*;
use uniql_core::transpiler;
use crate::config::EngineConfig;

#[derive(Debug)]
pub struct QueryPlan {
    pub sub_queries: Vec<SubQuery>,
    pub correlation: Option<CorrelationPlan>,
}

#[derive(Debug)]
pub struct SubQuery {
    pub signal_type: String,
    pub backend_name: String,
    pub backend_url: String,
    pub backend_type: String,
    pub native_query: String,
    pub transpiler_name: String,
    pub time_start: String,
    pub time_end: String,
    pub has_time_range: bool,
    pub step: String,
    pub show_format: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CorrelationPlan {
    pub join_fields: Vec<String>,
    pub time_window: Option<String>,
    pub skew_tolerance: Option<String>,
}

#[derive(Debug)]
pub struct PlanError {
    pub message: String,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Plan error: {}", self.message)
    }
}

/// Decompose a multi-signal query into sub-queries.
/// Single-signal queries return a plan with one sub-query and no correlation.
pub fn plan(ast: &Query, config: &EngineConfig) -> Result<QueryPlan, PlanError> {
    let sources = match &ast.from {
        Some(from) => &from.sources,
        None => return Err(PlanError { message: "No FROM clause".to_string() }),
    };

    let mut sub_queries = Vec::new();

    for source in sources {
        let signal_str = signal_type_str(&source.signal_type);

        // Find backend
        let backend = config
            .find_backend(signal_str, source.backend_hint.as_deref())
            .ok_or_else(|| PlanError {
                message: format!("No backend for signal '{}' (hint: {:?})", signal_str, source.backend_hint),
            })?;

        // Determine transpiler — also handle Unknown("vlogs") routing to logsql
        let transpiler_name = match backend.backend_type.as_str() {
            "prometheus" | "victoriametrics" => "promql",
            "victorialogs" => "logsql",
            "loki" => "logql",
            other => other,
        };

        // Override signal type for vlogs/victorialogs Unknown variants → Logs
        let effective_signal_type = match &source.signal_type {
            SignalType::Unknown(s) if s == "vlogs" || s == "victorialogs" => SignalType::Logs,
            other => other.clone(),
        };

        // Build a per-signal AST: only include conditions relevant to this signal
        // Use effective_signal_type for vlogs→logs rewriting
        let effective_source = DataSource {
            signal_type: effective_signal_type.clone(),
            backend_hint: source.backend_hint.clone(),
            alias: source.alias.clone(),
        };
        let signal_ast = build_signal_ast(ast, &effective_source);

        // Try normalized transpile path first, fall back to legacy
        let transpiler_impl = transpiler::get_transpiler(transpiler_name)
            .ok_or_else(|| PlanError {
                message: format!("No transpiler for '{}'", transpiler_name),
            })?;

        // Bind first — propagate bind errors (arithmetic in WHERE, etc.)
        let bound = uniql_core::bind::bind(&signal_ast).map_err(|e| PlanError {
            message: format!("Bind error for {}: {}", signal_str, e),
        })?;

        let output = match uniql_core::normalize::normalize(bound) {
            Ok(normalized) => {
                transpiler_impl.transpile_normalized(&normalized).map_err(|e| PlanError {
                    message: format!("Transpile error for {}: {}", signal_str, e),
                })?
            }
            Err(_) => {
                // Normalize failed — fallback to legacy transpile (skip normalized path)
                transpiler_impl.transpile(&signal_ast).map_err(|e| PlanError {
                    message: format!("Transpile error for {}: {}", signal_str, e),
                })?
            }
        };

        let time_start = extract_time_start(ast);
        let has_time_range = ast.within.is_some();
        let time_end = extract_time_end(ast);
        let step = extract_step(ast);

        // Extract SHOW format for the formatter
        let show_format = ast.show.as_ref().map(|s| {
            match s.format {
                ShowFormat::Table => "table".to_string(),
                ShowFormat::Count => "count".to_string(),
                ShowFormat::Timeseries => "timeseries".to_string(),
                ShowFormat::Timeline => "timeline".to_string(),
                ShowFormat::Heatmap => "heatmap".to_string(),
                ShowFormat::Flamegraph => "flamegraph".to_string(),
                ShowFormat::Alert => "alert".to_string(),
                ShowFormat::Topology => "topology".to_string(),
            }
        });

        let effective_signal_str = signal_type_str(&effective_signal_type);
        sub_queries.push(SubQuery {
            signal_type: effective_signal_str.to_string(),
            backend_name: backend.name.clone(),
            backend_url: backend.url.clone(),
            backend_type: backend.backend_type.clone(),
            native_query: output.native_query,
            transpiler_name: transpiler_name.to_string(),
            time_start,
            time_end,
            has_time_range,
            step,
            show_format,
        });
    }

    // Extract correlation plan
    let correlation = ast.correlate.as_ref().map(|c| CorrelationPlan {
        join_fields: c.on_fields.clone(),
        time_window: c.within.clone(),
        skew_tolerance: c.skew_tolerance.clone(),
    });

    Ok(QueryPlan {
        sub_queries,
        correlation,
    })
}

/// Build a per-signal AST by filtering WHERE conditions for this signal's alias/prefix.
/// For now, we pass through the entire WHERE clause — the transpiler handles
/// extracting relevant conditions and ignoring signal-prefixed ones from other signals.
fn build_signal_ast(original: &Query, source: &DataSource) -> Query {
    let mut ast = Query::new();

    // Single source FROM
    ast.from = Some(FromClause {
        sources: vec![DataSource {
            signal_type: source.signal_type.clone(),
            backend_hint: source.backend_hint.clone(),
            alias: source.alias.clone(),
        }],
    });

    // Filter WHERE conditions relevant to this signal
    if let Some(ref wc) = original.where_clause {
        let alias = source.alias.as_deref();
        let signal_prefix = signal_type_str(&source.signal_type);
        let filtered = filter_conditions_for_signal(&wc.condition, alias, signal_prefix);
        if let Some(cond) = filtered {
            ast.where_clause = Some(WhereClause { condition: cond });
        }
    }

    // Pass through other clauses
    ast.within = original.within.clone();
    // PARSE only valid for log signals — don't pass to metrics/traces
    if matches!(source.signal_type, SignalType::Logs | SignalType::Unknown(_)) {
        ast.parse = original.parse.clone();
    }
    ast.compute = original.compute.clone();
    ast.group_by = original.group_by.clone();
    ast.having = original.having.clone();
    // No CORRELATE or SHOW in sub-queries
    ast.show = None;
    ast.correlate = None;

    ast
}

/// Filter WHERE conditions to only include those relevant to a given signal.
/// Conditions with a qualified ident (e.g., metrics.cpu, m.cpu) are matched by prefix/alias.
/// Conditions without a qualifier (e.g., service = "x") are included for all signals.
fn filter_conditions_for_signal(expr: &Expr, alias: Option<&str>, signal_prefix: &str) -> Option<Expr> {
    match expr {
        Expr::BinaryOp { left, op: BinaryOp::And, right } => {
            let l = filter_conditions_for_signal(left, alias, signal_prefix);
            let r = filter_conditions_for_signal(right, alias, signal_prefix);
            match (l, r) {
                (Some(l), Some(r)) => Some(Expr::BinaryOp {
                    left: Box::new(l),
                    op: BinaryOp::And,
                    right: Box::new(r),
                }),
                (Some(l), None) => Some(l),
                (None, Some(r)) => Some(r),
                (None, None) => None,
            }
        }
        Expr::BinaryOp { left, op: BinaryOp::Or, right } => {
            let l = filter_conditions_for_signal(left, alias, signal_prefix);
            let r = filter_conditions_for_signal(right, alias, signal_prefix);
            match (l, r) {
                (Some(l), Some(r)) => Some(Expr::BinaryOp {
                    left: Box::new(l),
                    op: BinaryOp::Or,
                    right: Box::new(r),
                }),
                _ => None, // OR requires both sides
            }
        }
        Expr::BinaryOp { left, .. } => {
            if condition_belongs_to_signal(left, alias, signal_prefix) {
                // Strip signal prefix from qualified idents
                Some(strip_signal_prefix(expr, alias, signal_prefix))
            } else {
                None
            }
        }
        Expr::StringMatch { expr: inner, .. } => {
            if condition_belongs_to_signal(inner, alias, signal_prefix) {
                Some(strip_signal_prefix(expr, alias, signal_prefix))
            } else {
                None
            }
        }
        // Unqualified conditions belong to all signals
        _ => Some(expr.clone()),
    }
}

/// Check if a condition's LHS belongs to the given signal (by alias or prefix).
/// Unqualified idents (e.g., `service`) belong to ALL signals.
fn condition_belongs_to_signal(expr: &Expr, alias: Option<&str>, signal_prefix: &str) -> bool {
    match expr {
        Expr::QualifiedIdent(parts) if parts.len() >= 2 => {
            let prefix = &parts[0];
            if let Some(a) = alias {
                prefix == a || prefix == signal_prefix
            } else {
                prefix == signal_prefix || prefix == "labels"
            }
        }
        // Unqualified → belongs to all
        Expr::Ident(_) => true,
        _ => true,
    }
}

/// Strip signal prefix from qualified identifiers.
/// e.g., metrics.__name__ → __name__, m.cpu → cpu
fn strip_signal_prefix(expr: &Expr, alias: Option<&str>, signal_prefix: &str) -> Expr {
    match expr {
        Expr::BinaryOp { left, op, right } => Expr::BinaryOp {
            left: Box::new(strip_signal_prefix(left, alias, signal_prefix)),
            op: op.clone(),
            right: Box::new(strip_signal_prefix(right, alias, signal_prefix)),
        },
        Expr::QualifiedIdent(parts) if parts.len() >= 2 => {
            let prefix = &parts[0];
            let is_our_prefix = if let Some(a) = alias {
                prefix == a || prefix == signal_prefix
            } else {
                prefix == signal_prefix || prefix == "labels"
            };
            if is_our_prefix {
                if parts.len() == 2 {
                    Expr::Ident(parts[1].clone())
                } else {
                    Expr::QualifiedIdent(parts[1..].to_vec())
                }
            } else {
                expr.clone()
            }
        }
        Expr::StringMatch { expr: inner, op, pattern } => Expr::StringMatch {
            expr: Box::new(strip_signal_prefix(inner, alias, signal_prefix)),
            op: op.clone(),
            pattern: pattern.clone(),
        },
        _ => expr.clone(),
    }
}

fn signal_type_str(st: &SignalType) -> &str {
    match st {
        SignalType::Metrics => "metrics",
        SignalType::Logs => "logs",
        SignalType::Traces => "traces",
        SignalType::Events => "events",
        SignalType::Unknown(s) => s.as_str(),
    }
}

fn extract_time_start(ast: &Query) -> String {
    match &ast.within {
        Some(WithinClause::Last(d)) => format!("-{}", d),
        Some(WithinClause::Range { from, .. }) => from.clone(),
        Some(WithinClause::Today) => "today".to_string(),
        Some(WithinClause::ThisWeek) => "-7d".to_string(),
        None => "-5m".to_string(),
    }
}

fn extract_time_end(ast: &Query) -> String {
    match &ast.within {
        Some(WithinClause::Range { to, .. }) => to.clone(),
        Some(_) => "now".to_string(),
        None => String::new(),
    }
}

fn extract_step(ast: &Query) -> String {
    match &ast.within {
        Some(WithinClause::Last(d)) => duration_to_step(d),
        Some(WithinClause::Range { .. }) => "1m".to_string(),
        Some(WithinClause::Today) => "1m".to_string(),
        Some(WithinClause::ThisWeek) => "5m".to_string(),
        None => String::new(),
    }
}

/// Calculate a reasonable step from a duration string.
/// Heuristic: step = duration / 250 data points, min 15s.
fn duration_to_step(d: &str) -> String {
    let secs = parse_duration_secs(d);
    let step_secs = (secs / 250).max(15);
    if step_secs >= 3600 {
        format!("{}h", step_secs / 3600)
    } else if step_secs >= 60 {
        format!("{}m", step_secs / 60)
    } else {
        format!("{}s", step_secs)
    }
}

fn parse_duration_secs(d: &str) -> u64 {
    let d = d.trim();
    if d.ends_with("ms") {
        return 1;
    }
    let (num_part, unit) = d.split_at(d.len().saturating_sub(1));
    let n: u64 = num_part.parse().unwrap_or(5);
    match unit {
        "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        "d" => n * 86400,
        "w" => n * 604800,
        _ => n * 60, // default minutes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;

    fn default_config() -> EngineConfig {
        EngineConfig::default()
    }

    fn plan_query(input: &str) -> Result<QueryPlan, PlanError> {
        let ast = uniql_core::prepare(input).unwrap();
        plan(&ast, &default_config())
    }

    // ─── Görev 1: VLogs Routing Tests ─────────────────────────

    #[test]
    fn test_vlogs_routes_to_victorialogs_backend() {
        let result = plan_query("SHOW table FROM vlogs WHERE job = \"fortigate\"").unwrap();
        assert_eq!(result.sub_queries.len(), 1);
        let sq = &result.sub_queries[0];
        assert_eq!(sq.backend_type, "victorialogs");
        assert_eq!(sq.backend_name, "vlogs");
        assert_eq!(sq.transpiler_name, "logsql");
        assert_eq!(sq.signal_type, "logs"); // effective signal rewritten
    }

    #[test]
    fn test_vlogs_produces_logsql_native_query() {
        let result = plan_query("SHOW table FROM vlogs WHERE job = \"fortigate\"").unwrap();
        let sq = &result.sub_queries[0];
        assert!(sq.native_query.contains("job"), "native_query: {}", sq.native_query);
    }

    #[test]
    fn test_logs_signal_routes_to_vlogs() {
        let result = plan_query("FROM logs WHERE service = \"api\"").unwrap();
        let sq = &result.sub_queries[0];
        assert_eq!(sq.backend_type, "victorialogs");
        assert_eq!(sq.signal_type, "logs");
    }

    #[test]
    fn test_metrics_still_routes_to_prometheus() {
        let result = plan_query("FROM metrics WHERE __name__ = \"up\"").unwrap();
        let sq = &result.sub_queries[0];
        assert_eq!(sq.backend_type, "prometheus");
        assert_eq!(sq.signal_type, "metrics");
    }

    // ─── Görev 2: WITHIN Time Range Tests ─────────────────────

    #[test]
    fn test_within_last_5m_sets_time_range() {
        let result = plan_query("FROM metrics WHERE __name__ = \"up\" WITHIN last 5m").unwrap();
        let sq = &result.sub_queries[0];
        assert!(sq.has_time_range);
        assert_eq!(sq.time_start, "-5m");
        assert_eq!(sq.time_end, "now");
        assert!(!sq.step.is_empty());
    }

    #[test]
    fn test_within_last_1h_calculates_step() {
        let result = plan_query("FROM metrics WHERE __name__ = \"up\" WITHIN last 1h").unwrap();
        let sq = &result.sub_queries[0];
        assert!(sq.has_time_range);
        assert_eq!(sq.time_start, "-1h");
        assert_eq!(sq.time_end, "now");
        // 3600 / 250 = 14.4 → max(14, 15) = 15s
        assert_eq!(sq.step, "15s");
    }

    #[test]
    fn test_within_last_24h_calculates_step() {
        let result = plan_query("FROM metrics WHERE __name__ = \"up\" WITHIN last 24h").unwrap();
        let sq = &result.sub_queries[0];
        assert_eq!(sq.time_start, "-24h");
        // 86400 / 250 = 345.6 → 5m
        assert_eq!(sq.step, "5m");
    }

    #[test]
    fn test_no_within_no_time_range() {
        let result = plan_query("FROM metrics WHERE __name__ = \"up\"").unwrap();
        let sq = &result.sub_queries[0];
        assert!(!sq.has_time_range);
        assert_eq!(sq.time_start, "-5m"); // default
        assert!(sq.time_end.is_empty());
    }

    // ─── Duration parsing tests ───────────────────────────────

    #[test]
    fn test_parse_duration_minutes() {
        assert_eq!(parse_duration_secs("5m"), 300);
        assert_eq!(parse_duration_secs("30m"), 1800);
    }

    #[test]
    fn test_parse_duration_hours() {
        assert_eq!(parse_duration_secs("1h"), 3600);
        assert_eq!(parse_duration_secs("24h"), 86400);
    }

    #[test]
    fn test_parse_duration_days() {
        assert_eq!(parse_duration_secs("7d"), 604800);
    }

    #[test]
    fn test_duration_to_step_5m() {
        assert_eq!(duration_to_step("5m"), "15s"); // 300/250=1.2, max(1,15)=15
    }

    #[test]
    fn test_duration_to_step_7d() {
        // 604800 / 250 = 2419 → 40m
        assert_eq!(duration_to_step("7d"), "40m");
    }
}
