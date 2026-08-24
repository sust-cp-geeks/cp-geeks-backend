use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub user_id: i32,
    pub email: String,
    pub is_admin: bool,
    pub is_manager: Option<bool>,
    pub exp: i64,
}

// helper to create a jwt token for a user that expires in 7 days
pub fn create_token(
    user_id: i32,
    email: &str,
    is_admin: bool,
    is_manager: bool,
) -> Result<String, String> {
    let secret = std::env::var("JWT_SECRET").map_err(|_| {
        tracing::error!("JWT_SECRET is not set — cannot issue tokens");
        "Server authentication is misconfigured".to_string()
    })?;

    let expiry = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(7))
        .expect("valid timestamp")
        .timestamp();

    let claims = Claims {
        user_id,
        email: email.to_string(),
        is_admin,
        is_manager: Some(is_manager),
        exp: expiry,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| format!("failed to create token: {}", e))
}
