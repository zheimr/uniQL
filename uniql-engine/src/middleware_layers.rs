//! UNIQL Middleware Layers
//!
//! - Request ID generation (x-request-id header)
//! - API key authentication (x-api-key header)
//! - Request-level timeout (60s via tower-http TimeoutLayer)

use axum::extract::State;
use axum::http::{Request, HeaderValue, StatusCode};
use axum::body::Body;
use axum::middleware::Next;
use axum::response::{Response, IntoResponse};
use std::sync::Arc;

use crate::engine::AppState;

/// Middleware that adds an x-request-id header to every response.
pub async fn request_id(
    request: Request<Body>,
    next: Next,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();

    let mut response = next.run(request).await;
    if let Ok(val) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", val);
    }
    response
}

/// Middleware that logs query requests (audit trail).
pub async fn query_audit_log(
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let start = std::time::Instant::now();

    let response = next.run(request).await;

    let duration = start.elapsed();
    let status = response.status().as_u16();

    tracing::info!(
        method = %method,
        path = %path,
        status = status,
        duration_ms = duration.as_millis() as u64,
        "request"
    );

    response
}

/// Middleware that validates the x-api-key header.
/// If no API keys are configured, all requests are allowed (auth disabled).
/// Health endpoint is always exempt.
/// Uses constant-time comparison to prevent timing attacks.
pub async fn api_key_auth(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let keys = &state.config.api_keys;

    // If no keys configured, auth is disabled
    if keys.is_empty() {
        return next.run(request).await;
    }

    // Health endpoint is always exempt
    if request.uri().path() == "/health" {
        return next.run(request).await;
    }

    // Check x-api-key header
    let provided = request.headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok());

    match provided {
        Some(key) if keys.iter().any(|k| constant_time_eq(k.as_bytes(), key.as_bytes())) => {
            next.run(request).await
        }
        Some(_) => {
            (StatusCode::FORBIDDEN, "Invalid API key").into_response()
        }
        None => {
            (StatusCode::UNAUTHORIZED, "Missing x-api-key header").into_response()
        }
    }
}

/// Catch-all panic handler middleware.
/// If any downstream handler panics, this returns 500 instead of crashing the server.
pub async fn panic_recovery(
    request: Request<Body>,
    next: Next,
) -> Response {
    let result = std::panic::AssertUnwindSafe(next.run(request));

    match tokio::task::spawn(async move { result.await }).await {
        Ok(response) => response,
        Err(_) => {
            tracing::error!("Handler panicked — returning 500");
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        }
    }
}

/// Constant-time byte comparison to prevent timing attacks on API key validation.
pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::EngineConfig;
    use crate::engine::AppState;
    use axum::{routing::get, Router, middleware};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    use std::sync::Arc;

    fn test_state(api_keys: Vec<&str>) -> Arc<AppState> {
        Arc::new(AppState {
            config: EngineConfig {
                listen: "0.0.0.0:0".to_string(),
                backends: vec![],
                api_keys: api_keys.into_iter().map(|s| s.to_string()).collect(),
                cors_origins: vec![],
            },
        })
    }

    async fn ok_handler() -> &'static str { "ok" }

    // ─── constant_time_eq ────────────────────────────────────────

    #[test]
    fn ct_eq_same() {
        assert!(constant_time_eq(b"secret", b"secret"));
    }

    #[test]
    fn ct_eq_different() {
        assert!(!constant_time_eq(b"secret", b"wrong!"));
    }

    #[test]
    fn ct_eq_different_length() {
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    #[test]
    fn ct_eq_empty() {
        assert!(constant_time_eq(b"", b""));
    }

    // ─── request_id middleware ────────────────────────────────────

    #[tokio::test]
    async fn request_id_adds_header() {
        let app = Router::new()
            .route("/test", get(ok_handler))
            .layer(middleware::from_fn(request_id));

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        assert!(resp.headers().get("x-request-id").is_some());
    }

    // ─── api_key_auth middleware ──────────────────────────────────

    #[tokio::test]
    async fn auth_disabled_when_no_keys() {
        let state = test_state(vec![]);
        let app = Router::new()
            .route("/v1/query", get(ok_handler))
            .layer(middleware::from_fn_with_state(state.clone(), api_key_auth))
            .with_state(state);

        let req = Request::builder().uri("/v1/query").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn auth_health_exempt() {
        let state = test_state(vec!["secret123"]);
        let app = Router::new()
            .route("/health", get(ok_handler))
            .layer(middleware::from_fn_with_state(state.clone(), api_key_auth))
            .with_state(state);

        let req = Request::builder().uri("/health").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn auth_valid_key_accepted() {
        let state = test_state(vec!["secret123"]);
        let app = Router::new()
            .route("/v1/query", get(ok_handler))
            .layer(middleware::from_fn_with_state(state.clone(), api_key_auth))
            .with_state(state);

        let req = Request::builder()
            .uri("/v1/query")
            .header("x-api-key", "secret123")
            .body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn auth_invalid_key_rejected() {
        let state = test_state(vec!["secret123"]);
        let app = Router::new()
            .route("/v1/query", get(ok_handler))
            .layer(middleware::from_fn_with_state(state.clone(), api_key_auth))
            .with_state(state);

        let req = Request::builder()
            .uri("/v1/query")
            .header("x-api-key", "wrong")
            .body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 403);
    }

    #[tokio::test]
    async fn auth_missing_key_unauthorized() {
        let state = test_state(vec!["secret123"]);
        let app = Router::new()
            .route("/v1/query", get(ok_handler))
            .layer(middleware::from_fn_with_state(state.clone(), api_key_auth))
            .with_state(state);

        let req = Request::builder().uri("/v1/query").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 401);
    }

    // ─── query_audit_log middleware ───────────────────────────────

    #[tokio::test]
    async fn audit_log_passes_through() {
        let app = Router::new()
            .route("/test", get(ok_handler))
            .layer(middleware::from_fn(query_audit_log));

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
    }
}
