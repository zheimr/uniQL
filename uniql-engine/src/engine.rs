use crate::api::metrics::EngineMetrics;
use crate::cache::QueryCache;
use crate::config::EngineConfig;
use crate::rate_limit::RateLimiter;

pub struct AppState {
    pub config: EngineConfig,
    pub cache: QueryCache,
    pub metrics: EngineMetrics,
    pub rate_limiter: RateLimiter,
}
