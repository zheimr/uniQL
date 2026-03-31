mod api;
mod cache;
mod config;
mod correlate;
mod engine;
mod executor;
mod format;
mod middleware_layers;
mod normalize_result;
mod planner;
mod rate_limit;

use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    middleware,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Config
    let config = config::EngineConfig::load();
    tracing::info!("UNIQL Engine v0.3.0");
    tracing::info!("Listen: {}", config.listen);
    for bc in &config.backends {
        tracing::info!("Backend: {} ({}) → {}", bc.name, bc.backend_type, bc.url);
    }
    if config.api_keys.is_empty() {
        tracing::warn!("API key auth disabled (no UNIQL_API_KEYS set)");
    } else {
        tracing::info!("API key auth enabled ({} keys)", config.api_keys.len());
    }

    let listen_addr = config.listen.clone();

    // Startup health check: probe all backends
    for bc in &config.backends {
        let reachable = match bc.backend_type.as_str() {
            "prometheus" | "victoriametrics" => {
                executor::prometheus::PrometheusExecutor::new(&bc.name, &bc.url)
                    .health()
                    .await
                    .unwrap_or(false)
            }
            "victorialogs" => executor::victorialogs::VictoriaLogsExecutor::new(&bc.name, &bc.url)
                .health()
                .await
                .unwrap_or(false),
            _ => false,
        };
        if reachable {
            tracing::info!(
                "Backend '{}' ({}) at {} — reachable",
                bc.name,
                bc.backend_type,
                bc.url
            );
        } else {
            tracing::warn!(
                "Backend '{}' ({}) at {} — UNREACHABLE (queries will fail)",
                bc.name,
                bc.backend_type,
                bc.url
            );
        }
    }

    // CORS: configurable origins or permissive
    let cors = if config.cors_origins.is_empty() {
        CorsLayer::permissive()
    } else {
        let origins: Vec<_> = config
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new().allow_origin(AllowOrigin::list(origins))
    };

    let cache = cache::QueryCache::new(1000, 15); // 1000 entries, 15s TTL
    let metrics = api::metrics::EngineMetrics::new();
    tracing::info!("Query cache: 1000 entries, 15s TTL");

    let rate_limiter = rate_limit::RateLimiter::new(100); // 100 req/s per IP
    tracing::info!("Rate limiter: 100 req/s per IP");

    let state = Arc::new(engine::AppState {
        config,
        cache,
        metrics,
        rate_limiter,
    });

    // Routes — layers applied bottom-up: last added = outermost
    let app = Router::new()
        .route("/v1/query", post(api::query::handle_query))
        .route("/v1/validate", post(api::validate::handle_validate))
        .route("/v1/explain", post(api::explain::handle_explain))
        .route(
            "/v1/investigate",
            post(api::investigate::handle_investigate),
        )
        .route("/health", get(api::health::handle_health))
        .route("/metrics", get(api::metrics::handle_metrics))
        .route("/v1/schema", get(api::schema::handle_schema))
        .layer(DefaultBodyLimit::max(256 * 1024)) // 256KB body limit
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(60),
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            middleware_layers::rate_limit,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            middleware_layers::api_key_auth,
        ))
        .layer(middleware::from_fn(middleware_layers::query_audit_log))
        .layer(middleware::from_fn(middleware_layers::request_id))
        .layer(middleware::from_fn(middleware_layers::panic_recovery))
        .with_state(state);

    // Start with graceful shutdown
    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .expect("Failed to bind");

    tracing::info!("UNIQL Engine ready on {}", listen_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");

    tracing::info!("UNIQL Engine shut down gracefully");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => { tracing::info!("Received Ctrl+C, shutting down..."); },
        _ = terminate => { tracing::info!("Received SIGTERM, shutting down..."); },
    }
}
