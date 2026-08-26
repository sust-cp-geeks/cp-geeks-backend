use crate::app_state::AppState;
use crate::handlers::admin_handler;
use axum::{
    routing::{delete, get, put},
    Router,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/users", get(admin_handler::admin_list_users))
        .route("/users/{id}", get(admin_handler::admin_get_user))
        .route("/users/{id}/id-card", get(admin_handler::admin_get_id_card))
        .route(
            "/users/{id}/approve",
            put(admin_handler::admin_approve_user),
        )
        .route("/users/{id}/reject", put(admin_handler::admin_reject_user))
        .route("/users/{id}/ban", put(admin_handler::admin_ban_user))
        .route(
            "/users/{id}/reactivate",
            put(admin_handler::admin_reactivate_user),
        )
        .route("/users/{id}/email", put(admin_handler::admin_update_email))
        .route("/users/{id}", delete(admin_handler::admin_delete_user))
}
