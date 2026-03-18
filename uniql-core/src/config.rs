//! UNIQL Configuration — Centralized constants and defaults
//!
//! All hardcoded values gathered here for easy tuning and future config file support.

/// Default range duration when no WITHIN clause or COMPUTE duration is specified.
pub const DEFAULT_RANGE_DURATION: &str = "5m";

/// Default backend when none is specified via CLI or API.
pub const DEFAULT_BACKEND: &str = "promql";

/// Maximum expression nesting depth to prevent stack overflow on malicious input.
pub const MAX_EXPR_DEPTH: usize = 64;

/// Maximum number of DEFINE expansions to prevent infinite loops.
pub const MAX_DEFINE_EXPANSIONS: usize = 256;

/// Maximum query string size in bytes (64 KB).
pub const MAX_QUERY_SIZE: usize = 65_536;

/// Quantile mapping for percentile shorthand functions.
/// COMPUTE p50(latency) → histogram_quantile(0.5, ...)
pub fn quantile_for_percentile(name: &str) -> Option<&'static str> {
    match name {
        "p50" => Some("0.5"),
        "p75" => Some("0.75"),
        "p90" => Some("0.9"),
        "p95" => Some("0.95"),
        "p99" => Some("0.99"),
        "p999" => Some("0.999"),
        _ => None,
    }
}

/// Check if a function name is a known aggregate function.
pub fn is_aggregate_function(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "rate" | "irate" | "increase"
        | "count" | "count_over_time"
        | "avg" | "sum" | "min" | "max"
        | "p50" | "p75" | "p90" | "p95" | "p99" | "p999"
        | "stddev" | "stdvar"
        | "topk" | "bottomk"
        | "histogram_quantile"
        | "predict_linear"
    )
}

/// Check if a function name is a range-vector function (requires [duration]).
pub fn is_range_function(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "rate" | "irate" | "increase"
        | "count_over_time" | "avg_over_time" | "sum_over_time"
        | "min_over_time" | "max_over_time"
        | "stddev_over_time" | "stdvar_over_time"
        | "predict_linear"
    )
}
