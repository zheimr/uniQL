//! Prometheus metrics endpoint for self-monitoring.
//!
//! Exposes /metrics in Prometheus exposition format so UniQL engine
//! can be scraped by VictoriaMetrics/Prometheus.

use axum::extract::State;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::engine::AppState;

/// Global counters for engine metrics.
pub struct EngineMetrics {
    pub queries_total: AtomicU64,
    pub queries_cached: AtomicU64,
    pub queries_errors: AtomicU64,
    pub investigate_total: AtomicU64,
}

impl EngineMetrics {
    pub fn new() -> Self {
        EngineMetrics {
            queries_total: AtomicU64::new(0),
            queries_cached: AtomicU64::new(0),
            queries_errors: AtomicU64::new(0),
            investigate_total: AtomicU64::new(0),
        }
    }
}

/// GET /metrics — Prometheus exposition format
pub async fn handle_metrics(State(state): State<Arc<AppState>>) -> String {
    let cache_stats = state.cache.stats().await;
    let m = &state.metrics;

    let mut out = String::new();

    // Query counters
    out.push_str("# HELP uniql_queries_total Total queries processed\n");
    out.push_str("# TYPE uniql_queries_total counter\n");
    out.push_str(&format!(
        "uniql_queries_total {}\n",
        m.queries_total.load(Ordering::Relaxed)
    ));

    out.push_str("# HELP uniql_queries_cached Total cache hits\n");
    out.push_str("# TYPE uniql_queries_cached counter\n");
    out.push_str(&format!(
        "uniql_queries_cached {}\n",
        m.queries_cached.load(Ordering::Relaxed)
    ));

    out.push_str("# HELP uniql_queries_errors Total query errors\n");
    out.push_str("# TYPE uniql_queries_errors counter\n");
    out.push_str(&format!(
        "uniql_queries_errors {}\n",
        m.queries_errors.load(Ordering::Relaxed)
    ));

    out.push_str("# HELP uniql_investigate_total Total investigation packs run\n");
    out.push_str("# TYPE uniql_investigate_total counter\n");
    out.push_str(&format!(
        "uniql_investigate_total {}\n",
        m.investigate_total.load(Ordering::Relaxed)
    ));

    // Cache gauges
    out.push_str("# HELP uniql_cache_entries Current cache entries\n");
    out.push_str("# TYPE uniql_cache_entries gauge\n");
    out.push_str(&format!("uniql_cache_entries {}\n", cache_stats.active));

    out.push_str("# HELP uniql_cache_expired Expired cache entries pending eviction\n");
    out.push_str("# TYPE uniql_cache_expired gauge\n");
    out.push_str(&format!("uniql_cache_expired {}\n", cache_stats.expired));

    // Engine info
    out.push_str("# HELP uniql_info Engine information\n");
    out.push_str("# TYPE uniql_info gauge\n");
    out.push_str(&format!(
        "uniql_info{{version=\"0.3.0\",backends=\"{}\"}} 1\n",
        state.config.backends.len()
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::QueryCache;
    use crate::config::EngineConfig;
    use axum::extract::State;

    fn make_state() -> Arc<AppState> {
        Arc::new(AppState {
            config: EngineConfig::default(),
            cache: QueryCache::new(100, 15),
            metrics: EngineMetrics::new(),
            rate_limiter: crate::rate_limit::RateLimiter::new(100),
        })
    }

    #[tokio::test]
    async fn metrics_returns_prometheus_format() {
        let state = make_state();
        let output = handle_metrics(State(state)).await;
        assert!(output.contains("uniql_queries_total"));
        assert!(output.contains("uniql_cache_entries"));
        assert!(output.contains("uniql_info"));
        assert!(output.contains("# TYPE"));
        assert!(output.contains("# HELP"));
    }

    #[tokio::test]
    async fn metrics_counters_increment() {
        let state = make_state();
        state.metrics.queries_total.fetch_add(5, Ordering::Relaxed);
        state.metrics.queries_cached.fetch_add(2, Ordering::Relaxed);
        let output = handle_metrics(State(state)).await;
        assert!(output.contains("uniql_queries_total 5"));
        assert!(output.contains("uniql_queries_cached 2"));
    }
}
