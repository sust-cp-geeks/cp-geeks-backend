use crate::app_state::AppState;
use crate::handlers::auth_handler;
use crate::services::image_upload::MAX_UPLOAD_BYTES;
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};

// registration carries both sides of an id card. axum caps a request body at
// 2 MB by default — less than one photo we accept — so an upload died inside
// the multipart parser before our own size check could report anything useful.
// derived from MAX_UPLOAD_BYTES so raising the per-image limit raises this too,
// with a megabyte of slack for the text fields and multipart boundaries.
const REGISTER_BODY_LIMIT: usize = 2 * MAX_UPLOAD_BYTES + 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/register",
            post(auth_handler::register).layer(DefaultBodyLimit::max(REGISTER_BODY_LIMIT)),
        )
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
