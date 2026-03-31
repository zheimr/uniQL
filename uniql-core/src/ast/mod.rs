//! UNIQL AST — Abstract Syntax Tree
//!
//! Typed AST nodes representing a parsed UNIQL query.
//! Signal-type aware: metrics, logs, traces, events.

use serde::Serialize;

// ─── Top-Level Query ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
pub struct Query {
    pub defines: Vec<DefineClause>,
    pub from: Option<FromClause>,
    pub where_clause: Option<WhereClause>,
    pub within: Option<WithinClause>,
    pub parse: Option<ParseClause>,
    pub compute: Option<ComputeClause>,
    pub group_by: Option<GroupByClause>,
    pub having: Option<HavingClause>,
    pub correlate: Option<CorrelateClause>,
    pub show: Option<ShowClause>,
}

impl Query {
    pub fn new() -> Self {
        Self::default()
    }

    /// Infer signal types from the FROM clause
    pub fn inferred_signal_types(&self) -> Vec<SignalType> {
        match &self.from {
            Some(from) => from.sources.iter().map(|s| s.signal_type.clone()).collect(),
            None => vec![],
        }
    }

    /// Summary of which clauses are present
    pub fn clause_summary(&self) -> String {
        let mut parts = Vec::new();
        if self.from.is_some() {
            parts.push("FROM");
        }
        if self.where_clause.is_some() {
            parts.push("WHERE");
        }
        if self.within.is_some() {
            parts.push("WITHIN");
        }
        if self.parse.is_some() {
            parts.push("PARSE");
        }
        if self.compute.is_some() {
            parts.push("COMPUTE");
        }
        if self.group_by.is_some() {
            parts.push("GROUP BY");
        }
        if self.having.is_some() {
            parts.push("HAVING");
        }
        if self.correlate.is_some() {
            parts.push("CORRELATE");
        }
        if self.show.is_some() {
            parts.push("SHOW");
        }
        parts.join(" → ")
    }
}

// ─── Signal Types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum SignalType {
    Metrics,
    Logs,
    Traces,
    Events,
    Unknown(String),
}

impl SignalType {
    pub fn parse_signal(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "metrics" | "metric" => SignalType::Metrics,
            "logs" | "log" => SignalType::Logs,
            "traces" | "trace" => SignalType::Traces,
            "events" | "event" => SignalType::Events,
            _ => SignalType::Unknown(s.to_string()),
        }
    }
}

// ─── FROM Clause ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct FromClause {
    pub sources: Vec<DataSource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataSource {
    pub signal_type: SignalType,
    pub backend_hint: Option<String>, // e.g., "victoria", "loki", "prometheus"
    pub alias: Option<String>,        // e.g., AS m
}

// ─── WHERE Clause ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct WhereClause {
    pub condition: Expr,
}

impl WhereClause {
    pub fn condition_count(&self) -> usize {
        count_conditions(&self.condition)
    }
}

fn count_conditions(expr: &Expr) -> usize {
    match expr {
        Expr::BinaryOp {
            left,
            right,
            op: BinaryOp::And | BinaryOp::Or,
        } => count_conditions(left) + count_conditions(right),
        Expr::Not(inner) => count_conditions(inner),
        _ => 1,
    }
}

// ─── Expressions ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub enum Expr {
    /// Simple identifier: service, host, __name__
    Ident(String),

    /// Qualified identifier: metrics.cpu, logs.level, labels.env
    QualifiedIdent(Vec<String>),

    /// String literal: "nginx", 'api'
    StringLit(String),

    /// Number literal: 42, 3.14
    NumberLit(f64),

    /// Duration literal: 5m, 1h, 500ms
    DurationLit(String),

    /// Binary operation: a = b, a > 5, a AND b
    BinaryOp {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },

    /// Unary NOT
    Not(Box<Expr>),

    /// Function call: rate(value, 1m), count(), avg(cpu)
    FunctionCall { name: String, args: Vec<Expr> },

    /// IN expression: service IN ["nginx", "envoy"]
    InList {
        expr: Box<Expr>,
        list: Vec<Expr>,
        negated: bool,
    },

    /// String match: message CONTAINS "error"
    StringMatch {
        expr: Box<Expr>,
        op: StringMatchOp,
        pattern: String,
    },

    /// Wildcard star: *
    Star,

    /// Native backend query passthrough: NATIVE("promql", "rate(up[5m])")
    /// backend is optional — if None, targets the current transpiler's backend
    Native {
        backend: Option<String>,
        query: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum BinaryOp {
    // Comparison
    Eq,           // =
    Neq,          // !=
    Gt,           // >
    Lt,           // <
    Gte,          // >=
    Lte,          // <=
    RegexMatch,   // =~
    RegexNoMatch, // !~

    // Logical
    And,
    Or,

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum StringMatchOp {
    Contains,
    StartsWith,
    Matches,
}

// ─── PARSE Clause ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ParseClause {
    pub mode: ParseMode,
    pub pattern: Option<String>, // pattern template or regex
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ParseMode {
    Json,            // PARSE json
    Logfmt,          // PARSE logfmt
    Pattern(String), // PARSE pattern "<ip> - <method> <status>"
    Regexp(String),  // PARSE regexp "(?P<status>\\d{3})"
}

// ─── WITHIN Clause ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub enum WithinClause {
    /// WITHIN last 5m
    Last(String),

    /// WITHIN "2025-03-01" TO "2025-03-10"
    Range { from: String, to: String },

    /// WITHIN today
    Today,

    /// WITHIN this_week
    ThisWeek,
}

// ─── COMPUTE Clause ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ComputeClause {
    pub functions: Vec<ComputeFunction>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComputeFunction {
    pub name: String,          // rate, avg, p99, count, sum
    pub args: Vec<Expr>,       // arguments
    pub alias: Option<String>, // AS error_rate
}

// ─── GROUP BY Clause ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct GroupByClause {
    pub fields: Vec<Expr>,
}

// ─── HAVING Clause ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct HavingClause {
    pub condition: Expr,
}

// ─── CORRELATE Clause ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct CorrelateClause {
    pub on_fields: Vec<String>,
    pub within: Option<String>,         // time window, e.g., "30s"
    pub skew_tolerance: Option<String>, // clock skew tolerance
}

// ─── DEFINE Clause ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DefineClause {
    pub name: String,
    pub params: Vec<String>, // empty for simple aliases
    pub body: Expr,          // the expression this definition expands to
}

// ─── SHOW Clause ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ShowClause {
    pub format: ShowFormat,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ShowFormat {
    Timeseries,
    Table,
    Timeline,
    Heatmap,
    Flamegraph,
    Count,
    Alert,
    Topology,
}

impl ShowFormat {
    pub fn parse_format(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "timeseries" => Some(ShowFormat::Timeseries),
            "table" => Some(ShowFormat::Table),
            "timeline" => Some(ShowFormat::Timeline),
            "heatmap" => Some(ShowFormat::Heatmap),
            "flamegraph" => Some(ShowFormat::Flamegraph),
            "count" => Some(ShowFormat::Count),
            "alert" => Some(ShowFormat::Alert),
            "topology" => Some(ShowFormat::Topology),
            _ => None,
        }
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_type_parsing() {
        assert_eq!(SignalType::parse_signal("metrics"), SignalType::Metrics);
        assert_eq!(SignalType::parse_signal("Logs"), SignalType::Logs);
        assert_eq!(SignalType::parse_signal("TRACES"), SignalType::Traces);
        assert_eq!(
            SignalType::parse_signal("custom"),
            SignalType::Unknown("custom".into())
        );
    }

    #[test]
    fn test_query_clause_summary() {
        let mut q = Query::new();
        q.from = Some(FromClause {
            sources: vec![DataSource {
                signal_type: SignalType::Metrics,
                backend_hint: None,
                alias: None,
            }],
        });
        q.where_clause = Some(WhereClause {
            condition: Expr::BinaryOp {
                left: Box::new(Expr::Ident("service".into())),
                op: BinaryOp::Eq,
                right: Box::new(Expr::StringLit("nginx".into())),
            },
        });
        q.show = Some(ShowClause {
            format: ShowFormat::Timeseries,
        });

        assert_eq!(q.clause_summary(), "FROM → WHERE → SHOW");
        assert_eq!(q.inferred_signal_types(), vec![SignalType::Metrics]);
    }

    #[test]
    fn test_condition_count() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Ident("a".into())),
                op: BinaryOp::Eq,
                right: Box::new(Expr::NumberLit(1.0)),
            }),
            op: BinaryOp::And,
            right: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Ident("b".into())),
                op: BinaryOp::Gt,
                right: Box::new(Expr::NumberLit(2.0)),
            }),
        };
        let wc = WhereClause { condition: expr };
        assert_eq!(wc.condition_count(), 2);
    }
}
