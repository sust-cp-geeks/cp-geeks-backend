use crate::app_state::AppState;
use crate::handlers::announcement_handler;
use axum::{routing::get, Router};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(announcement_handler::get_announcements)
                .post(announcement_handler::create_announcement),
        )
        .route("/categories", get(announcement_handler::get_categories))
        .route(
            "/{id}",
            get(announcement_handler::get_announcement)
                .put(announcement_handler::update_announcement)
                .delete(announcement_handler::delete_announcement),
        )
}
