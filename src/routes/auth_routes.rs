use crate::app_state::AppState;
use crate::handlers::auth_handler;
use axum::{
    routing::{get, post},
    Router,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/register", post(auth_handler::register))
        .route("/login", post(auth_handler::login))
        .route("/verify-otp", post(auth_handler::verify_otp_handler))
        .route("/resend-otp", post(auth_handler::resend_otp_handler))
        .route("/forgot-password", post(auth_handler::forgot_password))
        .route("/reset-password", post(auth_handler::reset_password))
        .route("/status", get(auth_handler::account_status))
        .route("/change-email", post(auth_handler::request_email_change))
        .route(
            "/change-email/verify",
            post(auth_handler::confirm_email_change),
        )
}
