use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::engine::AppState;
use crate::planner;

#[derive(Debug, Deserialize)]
pub struct ExplainRequest {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct ExplainResponse {
    pub plan: ExecutionPlan,
}

#[derive(Debug, Serialize)]
pub struct ExecutionPlan {
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Serialize)]
pub struct PlanStep {
    pub step: u32,
    pub action: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
}

pub async fn handle_explain(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExplainRequest>,
) -> Result<Json<ExplainResponse>, (StatusCode, Json<super::ErrorResponse>)> {
    let mut steps = Vec::new();
    let mut step_num = 1u32;

    // Step 1: Parse + Expand + Validate (full pipeline)
    let ast = match uniql_core::prepare(&req.query) {
        Ok(ast) => {
            steps.push(PlanStep {
                step: step_num,
                action: "parse".to_string(),
                detail: "UNIQL → AST (parsed, expanded, validated)".to_string(),
                native_query: None,
                backend: None,
            });
            step_num += 1;
            ast
        }
        Err(e) => {
            steps.push(PlanStep {
                step: step_num,
                action: "parse".to_string(),
                detail: format!("FAILED: {}", e),
                native_query: None,
                backend: None,
            });
            return Ok(Json(ExplainResponse {
                plan: ExecutionPlan { steps },
            }));
        }
    };

    // Step 2: Plan using planner (handles multi-signal decomposition)
    let plan = match planner::plan(&ast, &state.config) {
        Ok(plan) => {
            let sub_count = plan.sub_queries.len();
            if sub_count > 1 {
                steps.push(PlanStep {
                    step: step_num,
                    action: "decompose".to_string(),
                    detail: format!("Split into {} sub-queries", sub_count),
                    native_query: None,
                    backend: None,
                });
                step_num += 1;
            }
            plan
        }
        Err(e) => {
            steps.push(PlanStep {
                step: step_num,
                action: "plan".to_string(),
                detail: format!("FAILED: {}", e),
                native_query: None,
                backend: None,
            });
            return Ok(Json(ExplainResponse { plan: ExecutionPlan { steps } }));
        }
    };

    // Step 3+: One step per sub-query (transpile + route)
    for sq in &plan.sub_queries {
        steps.push(PlanStep {
            step: step_num,
            action: format!("transpile_{}", sq.signal_type),
            detail: format!(
                "Signal '{}' → {} → backend '{}' ({})",
                sq.signal_type, sq.transpiler_name.to_uppercase(), sq.backend_name, sq.backend_url
            ),
            native_query: Some(sq.native_query.clone()),
            backend: Some(sq.backend_name.clone()),
        });
        step_num += 1;
    }

    // Execute step
    let exec_detail = if plan.sub_queries.len() > 1 {
        "Execute sub-queries in parallel (readonly)"
    } else {
        "Execute native query against backend (readonly)"
    };
    steps.push(PlanStep {
        step: step_num,
        action: "execute".to_string(),
        detail: exec_detail.to_string(),
        native_query: None,
        backend: None,
    });
    step_num += 1;

    // Correlate step (if multi-signal)
    if let Some(ref corr) = plan.correlation {
        steps.push(PlanStep {
            step: step_num,
            action: "correlate".to_string(),
            detail: format!(
                "Join on [{}] WITHIN {}",
                corr.join_fields.join(", "),
                corr.time_window.as_deref().unwrap_or("default")
            ),
            native_query: None,
            backend: None,
        });
    }

    Ok(Json(ExplainResponse { plan: ExecutionPlan { steps } }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::engine::AppState;
    use axum::extract::State;
    use axum::Json;
    use std::sync::Arc;

    fn make_state() -> Arc<AppState> {
        Arc::new(AppState {
            config: EngineConfig::default(),
            cache: crate::cache::QueryCache::new(100, 15),
            metrics: crate::api::metrics::EngineMetrics::new(),
        })
    }

    async fn explain_query(query: &str) -> ExplainResponse {
        let state = make_state();
        let req = ExplainRequest { query: query.to_string() };
        match handle_explain(State(state), Json(req)).await {
            Ok(Json(resp)) => resp,
            Err((_, Json(err))) => panic!("Explain returned error: {}", err.error),
        }
    }

    #[tokio::test]
    async fn explain_simple_metrics_query() {
        let resp = explain_query("FROM metrics WHERE __name__ = \"up\"").await;
        assert!(!resp.plan.steps.is_empty());
        // Should have at least parse + transpile + execute
        assert!(resp.plan.steps.len() >= 3);
        assert_eq!(resp.plan.steps[0].action, "parse");
    }

    #[tokio::test]
    async fn explain_has_native_query_in_transpile_step() {
        let resp = explain_query("FROM metrics WHERE __name__ = \"up\"").await;
        let transpile_step = resp.plan.steps.iter().find(|s| s.action.starts_with("transpile_"));
        assert!(transpile_step.is_some());
        let step = transpile_step.unwrap();
        assert!(step.native_query.is_some());
        assert!(step.backend.is_some());
    }

    #[tokio::test]
    async fn explain_has_execute_step() {
        let resp = explain_query("FROM metrics WHERE __name__ = \"up\"").await;
        let exec_step = resp.plan.steps.iter().find(|s| s.action == "execute");
        assert!(exec_step.is_some());
    }

    #[tokio::test]
    async fn explain_correlate_has_decompose_and_correlate_steps() {
        let resp = explain_query("FROM metrics, logs CORRELATE ON host WITHIN 60s").await;
        let decompose = resp.plan.steps.iter().find(|s| s.action == "decompose");
        let correlate = resp.plan.steps.iter().find(|s| s.action == "correlate");
        assert!(decompose.is_some(), "Should have a decompose step for multi-signal");
        assert!(correlate.is_some(), "Should have a correlate step");
    }

    #[tokio::test]
    async fn explain_correlate_step_has_join_info() {
        let resp = explain_query("FROM metrics, logs CORRELATE ON host WITHIN 60s").await;
        let correlate = resp.plan.steps.iter().find(|s| s.action == "correlate").unwrap();
        assert!(correlate.detail.contains("host"));
    }

    #[tokio::test]
    async fn explain_invalid_query_shows_parse_failure() {
        let state = make_state();
        let req = ExplainRequest { query: "INVALID!!!".to_string() };
        let resp = handle_explain(State(state), Json(req)).await.unwrap().0;
        assert_eq!(resp.plan.steps.len(), 1);
        assert_eq!(resp.plan.steps[0].action, "parse");
        assert!(resp.plan.steps[0].detail.contains("FAILED"));
    }

    #[tokio::test]
    async fn explain_logs_query() {
        let resp = explain_query("FROM logs WHERE job = \"nginx\"").await;
        assert!(resp.plan.steps.len() >= 3);
        let transpile = resp.plan.steps.iter().find(|s| s.action.starts_with("transpile_"));
        assert!(transpile.is_some());
    }

    #[tokio::test]
    async fn explain_native_query() {
        let resp = explain_query("NATIVE(\"promql\", \"rate(up[5m])\")").await;
        assert!(!resp.plan.steps.is_empty());
    }

    #[tokio::test]
    async fn explain_step_numbers_are_sequential() {
        let resp = explain_query("FROM metrics WHERE __name__ = \"up\" WITHIN last 5m").await;
        for (i, step) in resp.plan.steps.iter().enumerate() {
            assert_eq!(step.step, (i + 1) as u32);
        }
    }

    #[tokio::test]
    async fn explain_show_table_query() {
        let resp = explain_query("SHOW table FROM metrics WHERE __name__ = \"cpu\"").await;
        assert!(!resp.plan.steps.is_empty());
    }
}
