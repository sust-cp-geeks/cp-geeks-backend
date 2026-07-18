use crate::app_state::AppState;
use crate::handlers::contest_handler;
use axum::{routing::get, Router};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(contest_handler::get_contests).post(contest_handler::create_contest),
        )
        .route(
            "/{id}",
            get(contest_handler::get_contest)
                .put(contest_handler::update_contest)
                .delete(contest_handler::delete_contest),
        )
}
