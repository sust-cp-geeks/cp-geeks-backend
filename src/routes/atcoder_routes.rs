use crate::app_state::AppState;
use crate::handlers::atcoder_handler;
use axum::{routing::get, Router};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/profile/{user_id}",
            get(atcoder_handler::get_atcoder_stats),
        )
        .route(
            "/leaderboard",
            get(atcoder_handler::get_atcoder_leaderboard),
        )
}
