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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Json;

    async fn validate_query(query: &str) -> ValidateResponse {
        let req = ValidateRequest { query: query.to_string() };
        match handle_validate(Json(req)).await {
            Ok(Json(resp)) => resp,
            Err((_, Json(resp))) => resp,
        }
    }

    #[tokio::test]
    async fn valid_simple_metrics_query() {
        let resp = validate_query("FROM metrics WHERE __name__ = \"up\"").await;
        assert!(resp.valid);
        assert!(resp.error.is_none());
        assert!(!resp.signals.is_empty());
    }

    #[tokio::test]
    async fn valid_show_table_query() {
        let resp = validate_query("SHOW table FROM metrics WHERE __name__ = \"cpu\"").await;
        assert!(resp.valid);
    }

    #[tokio::test]
    async fn valid_show_count_query() {
        let resp = validate_query("SHOW count FROM metrics WHERE __name__ = \"up\"").await;
        assert!(resp.valid);
    }

    #[tokio::test]
    async fn valid_logs_query() {
        let resp = validate_query("FROM logs WHERE job = \"nginx\"").await;
        assert!(resp.valid);
    }

    #[tokio::test]
    async fn valid_query_with_within() {
        let resp = validate_query("FROM metrics WHERE __name__ = \"up\" WITHIN last 5m").await;
        assert!(resp.valid);
    }

    #[tokio::test]
    async fn valid_query_with_compute() {
        let resp = validate_query("FROM metrics WHERE __name__ = \"requests\" COMPUTE rate(value, 1m)").await;
        assert!(resp.valid);
    }

    #[tokio::test]
    async fn valid_query_with_group_by() {
        let resp = validate_query("FROM metrics WHERE __name__ = \"cpu\" COMPUTE avg(cpu) GROUP BY host").await;
        assert!(resp.valid);
    }

    #[tokio::test]
    async fn invalid_syntax_returns_error() {
        let resp = validate_query("NOT A VALID QUERY !!!").await;
        assert!(!resp.valid);
        assert!(resp.error.is_some());
    }

    #[tokio::test]
    async fn empty_query_handled() {
        let resp = validate_query("").await;
        // Empty query may be valid (parsed as empty) or invalid depending on parser
        // Just ensure it doesn't panic
        let _ = resp.valid;
    }

    #[tokio::test]
    async fn invalid_query_has_hint() {
        let resp = validate_query("SELCT * FROM foo").await;
        assert!(!resp.valid);
        assert!(resp.hint.is_some() || resp.error.is_some());
    }

    #[tokio::test]
    async fn valid_query_returns_clauses() {
        let resp = validate_query("FROM metrics WHERE __name__ = \"up\"").await;
        assert!(resp.valid);
        assert!(!resp.clauses.is_empty());
    }

    #[tokio::test]
    async fn native_query_handled() {
        // NATIVE passthrough may or may not pass semantic validation
        let resp = validate_query("NATIVE(\"promql\", \"rate(up[5m])\")").await;
        // Just verify it doesn't panic — the parser should handle it
        let _ = resp.valid;
    }

    #[tokio::test]
    async fn valid_correlate_query() {
        let resp = validate_query("FROM metrics, logs CORRELATE ON host WITHIN 60s").await;
        assert!(resp.valid);
        // Should detect multiple signals
        assert!(resp.signals.len() >= 2);
    }

    #[tokio::test]
    async fn valid_where_operators() {
        let resp = validate_query("FROM metrics WHERE __name__ = \"cpu\" AND host != \"test\"").await;
        assert!(resp.valid);
    }

    #[tokio::test]
    async fn valid_parse_clause() {
        let resp = validate_query("FROM logs WHERE job = \"nginx\" PARSE json").await;
        assert!(resp.valid);
    }
}
