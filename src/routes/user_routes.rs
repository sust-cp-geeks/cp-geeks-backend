use crate::app_state::AppState;
use crate::handlers::user_handler;
use axum::{routing::get, Router};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/me", get(user_handler::get_me).put(user_handler::update_me))
        .route("/search", get(user_handler::search_users))
        .route("/{id}", get(user_handler::get_user))
}
