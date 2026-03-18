use crate::api::metrics::EngineMetrics;
use crate::cache::QueryCache;
use crate::config::EngineConfig;

pub struct AppState {
    pub config: EngineConfig,
    pub cache: QueryCache,
    pub metrics: EngineMetrics,
}
