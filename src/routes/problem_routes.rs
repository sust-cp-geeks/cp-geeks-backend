use crate::{app_state::AppState, handlers::problem_handler::*};
use axum::{
    routing::{get, post},
    Router,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(get_problems))
        .route("/sections", post(create_section))
        .route("/subsections", post(create_subsection))
        .route("/items", post(create_item))
}
