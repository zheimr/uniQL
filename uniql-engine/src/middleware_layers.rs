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
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
