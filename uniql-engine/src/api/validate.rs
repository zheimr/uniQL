use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ValidateRequest {
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    pub signals: Vec<String>,
    pub clauses: String,
    pub warnings: Vec<String>,
}

pub async fn handle_validate(
    Json(req): Json<ValidateRequest>,
) -> Result<Json<ValidateResponse>, (StatusCode, Json<ValidateResponse>)> {
    // Parse
    let ast = match uniql_core::parse(&req.query) {
        Ok(ast) => ast,
        Err(e) => {
            return Ok(Json(ValidateResponse {
                valid: false,
                error: Some(e.to_string()),
                hint: Some("Check your UNIQL syntax.".to_string()),
                signals: vec![],
                clauses: String::new(),
                warnings: vec![],
            }));
        }
    };

    // Semantic validation
    let warnings = match uniql_core::semantic::validate(&ast) {
        Ok(w) => w,
        Err(e) => {
            return Ok(Json(ValidateResponse {
                valid: false,
                error: Some(e.message),
                hint: e.hint,
                signals: ast.inferred_signal_types().iter().map(|s| format!("{:?}", s)).collect(),
                clauses: ast.clause_summary(),
                warnings: vec![],
            }));
        }
    };

    Ok(Json(ValidateResponse {
        valid: true,
        error: None,
        hint: None,
        signals: ast.inferred_signal_types().iter().map(|s| format!("{:?}", s)).collect(),
        clauses: ast.clause_summary(),
        warnings: warnings.iter().map(|w| w.message.clone()).collect(),
    }))
}
