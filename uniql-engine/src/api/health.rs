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
