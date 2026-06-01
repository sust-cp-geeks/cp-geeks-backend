use axum::{routing::{get, post}, Router};
use crate::app_state::AppState;
use crate::handlers::ranker_handler;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/analyze", post(ranker_handler::analyze))
        .route("/pdf/{session_id}", get(ranker_handler::download_pdf))
        .route("/contest-title/{id}", get(ranker_handler::get_contest_title))
}
