use crate::app_state::AppState;
use crate::handlers::codeforces_handler;
use axum::{routing::get, Router};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/profile/{user_id}", get(codeforces_handler::get_cf_stats))
        .route("/leaderboard", get(codeforces_handler::get_leaderboard))
}
