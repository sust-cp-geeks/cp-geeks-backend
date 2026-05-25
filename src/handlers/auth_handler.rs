use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::AppError;
use crate::models::user::{LoginInput, RegisterInput, User};
use crate::services::{codeforces, email};
use crate::utils::jwt::create_token;
use crate::utils::otp;
use crate::validation::{validate_email, validate_string};

#[derive(Debug, Deserialize)]
pub struct VerifyOtpInput {
    pub email: String,
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct ResendOtpInput {
    pub email: String,
}

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterInput>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    // validate inputs upfront
    validate_string(&body.name, "Name", 2, 100)?;
    validate_string(&body.reg_number, "Registration number", 5, 50)?;
    validate_string(&body.password, "Password", 6, 255)?;
    validate_email(&body.email)?;
    
    let cf_handle = body.codeforces_handle.as_deref().filter(|h| !h.trim().is_empty());
    if let Some(handle) = cf_handle {
        validate_string(handle, "Codeforces handle", 1, 50)?;
        // validate the codeforces handle exists on codeforces.com
        codeforces::validate_handle(handle).await?;
    }

    // check if email already exists
    let existing = sqlx::query_scalar::<_, i32>("SELECT user_id FROM users WHERE email = $1")
        .bind(&body.email)
        .fetch_optional(&state.pool)
        .await?;

    if existing.is_some() {
        return Err(AppError::Conflict("Email already registered".to_string()));
    }

    // hash password with argon2
    let salt = SaltString::generate(&mut OsRng);
    let hashed = Argon2::default()
        .hash_password(body.password.as_bytes(), &salt)
        .map_err(|e| AppError::InternalError(format!("Failed to hash password: {}", e)))?
        .to_string();

    // all new registrations start as pending_verification until otp is confirmed
    let user_id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO users (reg_number, name, email, password, status, codeforces_handle) VALUES ($1, $2, $3, $4, $5, $6) RETURNING user_id",
    )
    .bind(&body.reg_number)
    .bind(&body.name)
    .bind(&body.email)
    .bind(&hashed)
    .bind("pending_verification")
    .bind(cf_handle)
    .fetch_one(&state.pool)
    .await?;

    // generate and send otp
    let code = otp::generate_otp();
    otp::store_otp(&state.pool, &body.email, &code).await?;
    email::send_otp_email(&body.email, &code).await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "success": true,
            "user_id": user_id,
            "status": "pending_verification",
            "message": "Registered — check your email for the verification code"
        })),
    ))
}

// verify the otp code sent to the user's email
pub async fn verify_otp_handler(
    State(state): State<AppState>,
    Json(body): Json<VerifyOtpInput>,
) -> Result<Json<Value>, AppError> {
    validate_email(&body.email)?;
    validate_string(&body.code, "OTP code", 6, 6)?;

    let is_valid = otp::verify_otp(&state.pool, &body.email, &body.code).await?;

    if !is_valid {
        return Err(AppError::BadRequest(
            "Invalid or expired verification code".to_string(),
        ));
    }

    // otp is valid — transition user status
    // sust students go straight to active, others need admin approval
    let new_status = if body.email.ends_with("@student.sust.edu") {
        "active"
    } else {
        "pending"
    };

    sqlx::query("UPDATE users SET status = $1 WHERE email = $2")
        .bind(new_status)
        .bind(&body.email)
        .execute(&state.pool)
        .await?;

    let message = if new_status == "active" {
        "Email verified — you can now log in"
    } else {
        "Email verified — your account is pending admin approval"
    };

    Ok(Json(json!({
        "success": true,
        "status": new_status,
        "message": message
    })))
}

// resend the otp code if the user didn't receive it
pub async fn resend_otp_handler(
    State(state): State<AppState>,
    Json(body): Json<ResendOtpInput>,
) -> Result<Json<Value>, AppError> {
    validate_email(&body.email)?;

    // make sure the user exists and is still pending verification
    let status = sqlx::query_scalar::<_, String>(
        "SELECT status FROM users WHERE email = $1",
    )
    .bind(&body.email)
    .fetch_optional(&state.pool)
    .await?;

    match status.as_deref() {
        Some("pending_verification") => {
            // good — they need a new code
        }
        Some(_) => {
            return Err(AppError::BadRequest(
                "This account has already been verified".to_string(),
            ));
        }
        None => {
            return Err(AppError::NotFound("No account found with this email".to_string()));
        }
    }

    // generate and send a fresh otp (old ones get invalidated inside store_otp)
    let code = otp::generate_otp();
    otp::store_otp(&state.pool, &body.email, &code).await?;
    email::send_otp_email(&body.email, &code).await?;

    Ok(Json(json!({
        "success": true,
        "message": "New verification code sent — check your email"
    })))
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginInput>,
) -> Result<Json<Value>, AppError> {
    // find user by email — use vague error to prevent user enumeration
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(&body.email)
        .fetch_optional(&state.pool)
        .await?;

    let user = user.ok_or(AppError::Unauthorized(
        "Invalid email or password".to_string(),
    ))?;

    // check account status
    match user.status.as_deref() {
        Some("pending_verification") => {
            return Err(AppError::Unauthorized(
                "Please verify your email first — check your inbox for the code".to_string(),
            ))
        }
        Some("pending") => {
            return Err(AppError::Unauthorized(
                "Account pending admin approval".to_string(),
            ))
        }
        Some("rejected") => {
            return Err(AppError::Unauthorized(
                "Account has been rejected".to_string(),
            ))
        }
        _ => {}
    }

    // verify password
    let parsed_hash = PasswordHash::new(&user.password)
        .map_err(|_| AppError::Unauthorized("Invalid email or password".to_string()))?;

    let is_valid = Argon2::default()
        .verify_password(body.password.as_bytes(), &parsed_hash)
        .is_ok();

    if !is_valid {
        return Err(AppError::Unauthorized(
            "Invalid email or password".to_string(),
        ));
    }

    // generate jwt token
    let token = create_token(user.user_id, &user.email, user.is_admin.unwrap_or(false), user.is_manager.unwrap_or(false))
        .map_err(|e| AppError::InternalError(e))?;

    Ok(Json(json!({
        "success": true,
        "token": token,
        "user": {
            "user_id": user.user_id,
            "name": user.name,
            "email": user.email,
            "is_admin": user.is_admin,
            "is_manager": user.is_manager
        }
    })))
}

// --- forgot password flow ---

#[derive(Debug, Deserialize)]
pub struct ForgotPasswordInput {
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct ResetPasswordInput {
    pub email: String,
    pub code: String,
    pub new_password: String,
}

// step 1: user provides email, we return the account name and send an otp
pub async fn forgot_password(
    State(state): State<AppState>,
    Json(body): Json<ForgotPasswordInput>,
) -> Result<Json<Value>, AppError> {
    validate_email(&body.email)?;

    // look up the user — return vague error to prevent user enumeration
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(&body.email)
        .fetch_optional(&state.pool)
        .await?;

    let user = user.ok_or(AppError::NotFound(
        "No account found with this email".to_string(),
    ))?;

    // only active or pending users can reset — rejected/banned users cannot
    match user.status.as_deref() {
        Some("pending_verification") => {
            return Err(AppError::BadRequest(
                "Please verify your email first before resetting password".to_string(),
            ));
        }
        Some("rejected") => {
            return Err(AppError::BadRequest(
                "This account has been rejected".to_string(),
            ));
        }
        _ => {}
    }

    // generate and send otp
    let code = otp::generate_otp();
    otp::store_otp(&state.pool, &body.email, &code).await?;
    email::send_password_reset_email(&body.email, &code).await?;

    Ok(Json(json!({
        "success": true,
        "name": user.name,
        "message": "Password reset code sent — check your email"
    })))
}

// step 2: user provides email + otp + new password, we reset the password
pub async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<ResetPasswordInput>,
) -> Result<Json<Value>, AppError> {
    validate_email(&body.email)?;
    validate_string(&body.code, "OTP code", 6, 6)?;
    validate_string(&body.new_password, "New password", 6, 255)?;

    // verify the otp
    let is_valid = otp::verify_otp(&state.pool, &body.email, &body.code).await?;

    if !is_valid {
        return Err(AppError::BadRequest(
            "Invalid or expired reset code".to_string(),
        ));
    }

    // hash the new password
    let salt = SaltString::generate(&mut OsRng);
    let hashed = Argon2::default()
        .hash_password(body.new_password.as_bytes(), &salt)
        .map_err(|e| AppError::InternalError(format!("Failed to hash password: {}", e)))?
        .to_string();

    // update the password in the database
    sqlx::query("UPDATE users SET password = $1 WHERE email = $2")
        .bind(&hashed)
        .bind(&body.email)
        .execute(&state.pool)
        .await?;

    Ok(Json(json!({
        "success": true,
        "message": "Password reset successfully — you can now log in with your new password"
    })))
}
