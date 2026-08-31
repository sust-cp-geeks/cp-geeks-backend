pub mod app_state;
pub mod config;
pub mod errors;
pub mod handlers;
pub mod middleware;
pub mod models;
pub mod routes;
pub mod services;
pub mod utils;
pub mod validation;

use crate::app_state::AppState;
use axum::Router;
use http::Method;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

// fallback origins when CORS_ALLOWED_ORIGINS isn't set
const DEFAULT_ALLOWED_ORIGINS: &[&str] = &[
    "http://localhost:5173",
    "http://localhost:4173",
    "http://localhost:3000",
];

// builds the cors layer from CORS_ALLOWED_ORIGINS so deploys don't need a code change
fn build_cors() -> CorsLayer {
    let configured = std::env::var("CORS_ALLOWED_ORIGINS").unwrap_or_default();

    let raw_origins: Vec<String> = if configured.trim().is_empty() {
        DEFAULT_ALLOWED_ORIGINS
            .iter()
            .map(|o| o.to_string())
            .collect()
    } else {
        configured
            .split(',')
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect()
    };

    let mut origins = Vec::new();
    for origin in raw_origins {
        // HeaderValue accepts almost anything, so check the shape ourselves
        let well_formed = (origin.starts_with("http://") || origin.starts_with("https://"))
            && !origin.contains(char::is_whitespace);

        match origin.parse::<http::HeaderValue>() {
            Ok(value) if well_formed => origins.push(value),
            // skip bad entries instead of taking the whole server down
            _ => tracing::warn!("ignoring invalid CORS origin: {:?}", origin),
        }
    }

    tracing::info!("cors allowed origins: {:?}", origins);

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any)
}

// waits for the process to be asked to stop, so in-flight requests finish
// instead of being cut off mid-registration on every deploy
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl-c");
    };

    // what docker, systemd and most hosts actually send
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received — finishing in-flight requests");
}

#[tokio::main]
async fn main() {
    // a malformed line makes dotenvy stop, silently dropping every variable
    // after it — worth a loud warning rather than a mystery later
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("warning: could not fully load .env: {e}");
    }

    // RUST_LOG overrides this — the default shows request logs from TraceLayer
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,tower_http=debug"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // fail fast here instead of panicking mid-request later
    if std::env::var("JWT_SECRET").is_err() {
        panic!("JWT_SECRET must be set in .env file");
    }

    // db pool for neon postgres
    let pool = config::database::connect().await;

    // automatically run any pending database migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to apply database migrations");

    let state = AppState::new(pool);

    let app = Router::new()
        .nest("/api/auth", routes::auth_routes::routes())
        .nest("/api/users", routes::user_routes::routes())
        .nest("/api/admin", routes::admin_routes::routes())
        .nest("/api/contests", routes::contest_routes::routes())
        .nest("/api/announcements", routes::announcement_routes::routes())
        .nest("/api/events", routes::event_routes::routes())
        .nest("/api/cf", routes::codeforces_routes::routes())
        .nest("/api/ranker", routes::ranker_routes::routes())
        .nest("/api/problems", routes::problem_routes::routes())
        .route(
            "/api/health",
            axum::routing::get(handlers::health_handler::health_check),
        )
        .with_state(state)
        .layer(build_cors())
        .layer(TraceLayer::new_for_http());

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("server running at http://{}", addr);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .unwrap();
}
