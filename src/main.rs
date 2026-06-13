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

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    // db pool for neon postgres
    let pool = config::database::connect().await;
    let state = AppState {
        pool,
        results_cache: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    };

    // cors setup for frontend
    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost:5173".parse::<http::HeaderValue>().unwrap(),
            "http://localhost:4173".parse::<http::HeaderValue>().unwrap(),
            "https://cp-geeks-frontend.vercel.app".parse::<http::HeaderValue>().unwrap(),
        ])
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(Any);

    let app = Router::new()
        .nest("/api/auth", routes::auth_routes::routes())
        .nest("/api/users", routes::user_routes::routes())
        .nest("/api/admin", routes::admin_routes::routes())
        .nest("/api/contests", routes::contest_routes::routes())
        .nest("/api/announcements", routes::announcement_routes::routes())
        .nest("/api/events", routes::event_routes::routes())
        .nest("/api/cf", routes::codeforces_routes::routes())
        .nest("/api/ranker", routes::ranker_routes::routes())
        .route(
            "/api/health",
            axum::routing::get(handlers::health_handler::health_check),
        )
        .with_state(state)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap();
    tracing::info!("server running at http://localhost:8080");
    axum::serve(listener, app).await.unwrap();
}