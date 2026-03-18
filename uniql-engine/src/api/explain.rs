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
