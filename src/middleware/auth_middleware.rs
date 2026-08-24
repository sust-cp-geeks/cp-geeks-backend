use axum::{
    extract::FromRequestParts,
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde_json::json;

use crate::app_state::AppState;
use crate::utils::jwt::Claims;

// my custom error type so axum knows how to respond when auth fails
pub struct AuthError {
    pub message: String,
    pub status: StatusCode,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "success": false,
            "error": self.message
        }));
        (self.status, body).into_response()
    }
}

// Extractor that automatically verifies the JWT
impl FromRequestParts<AppState> for Claims {
    type Rejection = AuthError;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // get the "Authorization" header from the request
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(AuthError {
                message: "Missing authorization header".to_string(),
                status: StatusCode::UNAUTHORIZED,
            })?;

        // strip out the "Bearer " part to just get the jwt token
        let token = auth_header.strip_prefix("Bearer ").ok_or(AuthError {
            message: "Invalid authorization format. Use: Bearer <token>".to_string(),
            status: StatusCode::UNAUTHORIZED,
        })?;

        let jwt_secret = std::env::var("JWT_SECRET").map_err(|_| {
            tracing::error!("JWT_SECRET is not set — cannot verify tokens");
            AuthError {
                message: "Server authentication is misconfigured".to_string(),
                status: StatusCode::INTERNAL_SERVER_ERROR,
            }
        })?;

        let decoded = decode::<Claims>(
            token,
            &DecodingKey::from_secret(jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|e| {
            let message = match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
                    "Token has expired. Please login again."
                }
                jsonwebtoken::errors::ErrorKind::InvalidToken => "Invalid token.",
                _ => "Token verification failed.",
            };
            AuthError {
                message: message.to_string(),
                status: StatusCode::UNAUTHORIZED,
            }
        })?;

        Ok(decoded.claims)
    }
}
