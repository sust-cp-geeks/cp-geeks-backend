use axum::{
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

// every possible error in our application
#[derive(Debug)]
pub enum AppError {
    // 400 — client sent bad data
    BadRequest(String),

    // 401 — not logged in or bad token
    Unauthorized(String),

    // 403 — logged in but not allowed
    Forbidden(String),

    // 404 — resource doesn't exist
    NotFound(String),

    // 409 — duplicate (email already exists, etc.)
    Conflict(String),

    // 500 — something went wrong on our side
    InternalError(String),
}

// teach axum how to convert AppError into an HTTP response
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            AppError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(json!({
            "success": false,
            "error": message
        }));

        (status, body).into_response()
    }
}

// auto-convert database errors so we can use ? with sqlx
impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!("database error: {}", e);
        AppError::InternalError(format!("Database error: {}", e))
    }
}

// auto-convert malformed json body errors
impl From<JsonRejection> for AppError {
    fn from(rejection: JsonRejection) -> Self {
        let message = match rejection {
            JsonRejection::JsonDataError(e) => format!("Invalid JSON data: {}", e),
            JsonRejection::JsonSyntaxError(e) => format!("Malformed JSON: {}", e),
            JsonRejection::MissingJsonContentType(_) => {
                "Missing Content-Type: application/json header".to_string()
            }
            _ => "Invalid request body".to_string(),
        };
        AppError::BadRequest(message)
    }
}

// helper: require the user to be an admin
pub fn require_admin(claims: &crate::utils::jwt::Claims) -> Result<(), AppError> {
    if !claims.is_admin {
        return Err(AppError::Forbidden("Admin access required".to_string()));
    }
    Ok(())
}

// helper: require the user to be an admin or manager
pub fn require_admin_or_manager(claims: &crate::utils::jwt::Claims) -> Result<(), AppError> {
    if !claims.is_admin && !claims.is_manager.unwrap_or(false) {
        return Err(AppError::Forbidden(
            "Admin or Manager access required".to_string(),
        ));
    }
    Ok(())
}
