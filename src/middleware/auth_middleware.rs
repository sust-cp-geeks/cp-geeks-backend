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
        state: &AppState,
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

        let claims = decoded.claims;

        // a valid signature isn't enough — the account may have had its sessions
        // cut since this token was issued (password reset, ban). without this the
        // token would keep working for the rest of its 7 days.
        let valid_from = sqlx::query_scalar::<_, Option<chrono::NaiveDateTime>>(
            "SELECT sessions_valid_from FROM users WHERE user_id = $1",
        )
        .bind(claims.user_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("could not check session validity: {}", e);
            AuthError {
                message: "Could not verify session".to_string(),
                status: StatusCode::INTERNAL_SERVER_ERROR,
            }
        })?;

        // outer None = the account is gone
        let valid_from = valid_from.ok_or(AuthError {
            message: "Account no longer exists".to_string(),
            status: StatusCode::UNAUTHORIZED,
        })?;

        if let Some(cutoff) = valid_from {
            // a token with no iat predates this check, so treat it as ancient
            let issued_at = claims.iat.unwrap_or(0);
            // iat is whole seconds but the cutoff has sub-second precision, and
            // .timestamp() truncates down — so <= rather than <, otherwise a ban
            // landing in the same second a token was issued wouldn't revoke it.
            // erring toward invalidating is the safe direction for a security check.
            if issued_at <= cutoff.and_utc().timestamp() {
                return Err(AuthError {
                    message: "Session expired. Please login again.".to_string(),
                    status: StatusCode::UNAUTHORIZED,
                });
            }
        }

        Ok(claims)
    }
}
