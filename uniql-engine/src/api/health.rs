use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::Arc;

use crate::engine::AppState;
use crate::executor::prometheus::PrometheusExecutor;
use crate::executor::victorialogs::VictoriaLogsExecutor;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub backends: Vec<BackendHealth>,
}

#[derive(Debug, Serialize)]
pub struct BackendHealth {
    pub name: String,
    #[serde(rename = "type")]
    pub backend_type: String,
    pub url: String,
    pub status: String,
}

pub async fn handle_health(
    State(state): State<Arc<AppState>>,
) -> Json<HealthResponse> {
    let mut backends = Vec::new();

    for bc in &state.config.backends {
        let reachable = match bc.backend_type.as_str() {
            "prometheus" | "victoriametrics" => {
                let executor = PrometheusExecutor::new(&bc.name, &bc.url);
                executor.health().await.unwrap_or(false)
            }
            "victorialogs" => {
                let executor = VictoriaLogsExecutor::new(&bc.name, &bc.url);
                executor.health().await.unwrap_or(false)
            }
            _ => false,
        };

        backends.push(BackendHealth {
            name: bc.name.clone(),
            backend_type: bc.backend_type.clone(),
            url: bc.url.clone(),
            status: if reachable { "reachable".to_string() } else { "unreachable".to_string() },
        });
    }

    let all_ok = backends.iter().all(|b| b.status == "reachable");

    Json(HealthResponse {
        status: if all_ok { "ok".to_string() } else { "degraded".to_string() },
        version: "0.3.0".to_string(),
        backends,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EngineConfig, BackendConfig};
    use crate::engine::AppState;
    use wiremock::{MockServer, Mock, matchers, ResponseTemplate};

    #[tokio::test]
    async fn health_all_backends_up() {
        let prom_server = MockServer::start().await;
        Mock::given(matchers::path("/health")).respond_with(ResponseTemplate::new(200)).mount(&prom_server).await;

        let vlogs_server = MockServer::start().await;
        Mock::given(matchers::path("/health")).respond_with(ResponseTemplate::new(200)).mount(&vlogs_server).await;

        let config = EngineConfig {
            listen: "0.0.0.0:0".to_string(),
            backends: vec![
                BackendConfig { name: "vm".to_string(), backend_type: "prometheus".to_string(), url: prom_server.uri() },
                BackendConfig { name: "vl".to_string(), backend_type: "victorialogs".to_string(), url: vlogs_server.uri() },
            ],
            api_keys: vec![],
            cors_origins: vec![],
        };
        let state = Arc::new(AppState { config });
        let Json(resp) = handle_health(State(state)).await;
        assert_eq!(resp.status, "ok");
        assert_eq!(resp.version, "0.3.0");
        assert_eq!(resp.backends.len(), 2);
        assert!(resp.backends.iter().all(|b| b.status == "reachable"));
    }

    #[tokio::test]
    async fn health_degraded_when_backend_down() {
        let prom_server = MockServer::start().await;
        Mock::given(matchers::path("/health")).respond_with(ResponseTemplate::new(200)).mount(&prom_server).await;

        let config = EngineConfig {
            listen: "0.0.0.0:0".to_string(),
            backends: vec![
                BackendConfig { name: "vm".to_string(), backend_type: "prometheus".to_string(), url: prom_server.uri() },
                BackendConfig { name: "vl".to_string(), backend_type: "victorialogs".to_string(), url: "http://127.0.0.1:1".to_string() },
            ],
            api_keys: vec![],
            cors_origins: vec![],
        };
        let state = Arc::new(AppState { config });
        let Json(resp) = handle_health(State(state)).await;
        assert_eq!(resp.status, "degraded");
        let vm = resp.backends.iter().find(|b| b.name == "vm").unwrap();
        let vl = resp.backends.iter().find(|b| b.name == "vl").unwrap();
        assert_eq!(vm.status, "reachable");
        assert_eq!(vl.status, "unreachable");
    }

    #[tokio::test]
    async fn health_no_backends() {
        let config = EngineConfig {
            listen: "0.0.0.0:0".to_string(),
            backends: vec![],
            api_keys: vec![],
            cors_origins: vec![],
        };
        let state = Arc::new(AppState { config });
        let Json(resp) = handle_health(State(state)).await;
        assert_eq!(resp.status, "ok"); // no backends = vacuously true
        assert_eq!(resp.backends.len(), 0);
    }

    #[tokio::test]
    async fn health_unknown_backend_type() {
        let config = EngineConfig {
            listen: "0.0.0.0:0".to_string(),
            backends: vec![
                BackendConfig { name: "custom".to_string(), backend_type: "elasticsearch".to_string(), url: "http://localhost:9200".to_string() },
            ],
            api_keys: vec![],
            cors_origins: vec![],
        };
        let state = Arc::new(AppState { config });
        let Json(resp) = handle_health(State(state)).await;
        assert_eq!(resp.status, "degraded");
        assert_eq!(resp.backends[0].status, "unreachable");
    }
}
