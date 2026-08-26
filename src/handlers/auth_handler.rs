use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::{FromRequest, Multipart, Query, Request, State};
use axum::http::{header::CONTENT_TYPE, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::app_state::AppState;
use crate::errors::AppError;
use crate::handlers::admin_handler::discard_id_card;
use crate::models::user::{LoginInput, RegisterInput, User};
use crate::services::{codeforces, email, image_upload, storage};
use crate::utils::jwt::create_token;
use crate::utils::otp;
use crate::utils::rate_limit;
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

// the two sides of a student id, already validated and re-encoded
struct IdCardImages {
    front: Vec<u8>,
    back: Vec<u8>,
}

// registration accepts either json (students with a working @student.sust.edu
// address) or multipart (everyone else, who must attach their id card), so the
// existing json clients keep working unchanged
async fn parse_register_request(
    request: Request,
    state: &AppState,
) -> Result<(RegisterInput, Option<IdCardImages>), AppError> {
    let is_multipart = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("multipart/form-data"))
        .unwrap_or(false);

    if !is_multipart {
        let Json(body) = Json::<RegisterInput>::from_request(request, state).await?;
        return Ok((body, None));
    }

    let mut multipart = Multipart::from_request(request, state)
        .await
        .map_err(|e| AppError::BadRequest(format!("Invalid multipart body: {}", e)))?;

    let mut fields: HashMap<String, String> = HashMap::new();
    let mut front: Option<Vec<u8>> = None;
    let mut back: Option<Vec<u8>> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("Could not read uploaded form: {}", e)))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "id_card_front" | "id_card_back" => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("Could not read {}: {}", name, e)))?;
                if name == "id_card_front" {
                    front = Some(bytes.to_vec());
                } else {
                    back = Some(bytes.to_vec());
                }
            }
            _ => {
                let text = field.text().await.map_err(|e| {
                    AppError::BadRequest(format!("Could not read field {}: {}", name, e))
                })?;
                fields.insert(name, text);
            }
        }
    }

    let required = |key: &str| -> Result<String, AppError> {
        fields
            .get(key)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or_else(|| AppError::BadRequest(format!("{} is required", key)))
    };
    let optional = |key: &str| -> Option<String> {
        fields
            .get(key)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };

    let body = RegisterInput {
        reg_number: required("reg_number")?,
        name: required("name")?,
        email: required("email")?,
        password: required("password")?,
        codeforces_handle: optional("codeforces_handle"),
        vjudge_handle: optional("vjudge_handle"),
    };

    let images = match (front, back) {
        (Some(f), Some(b)) => Some(IdCardImages { front: f, back: b }),
        (None, None) => None,
        // one side alone is never useful — say so rather than half-accepting it
        _ => {
            return Err(AppError::BadRequest(
                "Both id_card_front and id_card_back are required".to_string(),
            ))
        }
    };

    Ok((body, images))
}

// verifies an otp and counts wrong guesses
// a 6-digit code is only 1e6 options, so without a cap it can just be guessed —
// after OTP_MAX_ATTEMPTS we burn the live codes and make them request a new one
async fn verify_otp_guarded(
    state: &AppState,
    email: &str,
    code: &str,
    label: &str,
) -> Result<(), AppError> {
    let key = rate_limit::otp_attempt_key(email);

    // refuse before touching the db once the allowance is gone
    state.limiter.check(
        &key,
        rate_limit::OTP_MAX_ATTEMPTS,
        rate_limit::OTP_ATTEMPT_WINDOW,
    )?;

    let is_valid = otp::verify_otp(&state.pool, email, code).await?;

    if !is_valid {
        // that guess counted — once the allowance runs out, kill the live codes
        if state.limiter.count(&key, rate_limit::OTP_ATTEMPT_WINDOW) >= rate_limit::OTP_MAX_ATTEMPTS
        {
            otp::invalidate_otps(&state.pool, email).await?;
            tracing::warn!("otp attempt limit hit for {} — codes invalidated", email);
        }
        return Err(AppError::BadRequest(format!(
            "Invalid or expired {}",
            label
        )));
    }

    // honest user got it right, don't leave them counted against
    state.limiter.reset(&key);
    Ok(())
}

// drops images that were uploaded for a registration that then failed
async fn discard_uploads(keys: &Option<(String, String)>) {
    if let Some((front, back)) = keys {
        storage::delete_quietly(front).await;
        storage::delete_quietly(back).await;
    }
}

// students with a working university address skip manual review
fn is_student_email(email: &str) -> bool {
    email.trim().to_lowercase().ends_with("@student.sust.edu")
}

pub async fn register(
    State(state): State<AppState>,
    request: Request,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let (body, id_card) = parse_register_request(request, &state).await?;

    // validate inputs upfront
    validate_string(&body.name, "Name", 2, 100)?;
    validate_string(&body.reg_number, "Registration number", 5, 50)?;
    validate_string(&body.password, "Password", 6, 255)?;
    validate_email(&body.email)?;

    // caps otp mail per address so registration can't be used to bomb an inbox
    state.limiter.check(
        &rate_limit::otp_send_key(&body.email),
        rate_limit::OTP_SEND_MAX,
        rate_limit::OTP_SEND_WINDOW,
    )?;

    let cf_handle = body
        .codeforces_handle
        .as_deref()
        .filter(|h| !h.trim().is_empty());
    if let Some(handle) = cf_handle {
        validate_string(handle, "Codeforces handle", 1, 50)?;
        // validate the codeforces handle exists on codeforces.com
        codeforces::validate_handle(handle).await?;
    }

    let vjudge_handle = body
        .vjudge_handle
        .as_deref()
        .map(|h| h.trim())
        .filter(|h| !h.is_empty());
    if let Some(handle) = vjudge_handle {
        validate_string(handle, "VJudge handle", 1, 100)?;
    }

    // anyone without a university address has to prove who they are, since an
    // admin will be approving them by hand
    let id_card = match id_card {
        Some(images) => Some(images),
        None if is_student_email(&body.email) => None,
        None => {
            return Err(AppError::BadRequest(
                "Registering without an @student.sust.edu email requires id_card_front and id_card_back photos".to_string(),
            ))
        }
    };

    // resize and re-encode before creating anything, so a bad photo fails the
    // request early instead of leaving a half-made account behind
    let id_card = match id_card {
        Some(images) => Some(IdCardImages {
            front: image_upload::process(&images.front, "ID card front")?,
            back: image_upload::process(&images.back, "ID card back")?,
        }),
        None => None,
    };

    if id_card.is_some() && !storage::is_configured() {
        tracing::error!("id card upload attempted but R2 is not configured");
        return Err(AppError::InternalError(
            "File storage is not configured — contact an admin".to_string(),
        ));
    }

    // check if email already exists
    let existing = sqlx::query_scalar::<_, i32>("SELECT user_id FROM users WHERE email = $1")
        .bind(&body.email)
        .fetch_optional(&state.pool)
        .await?;

    if existing.is_some() {
        return Err(AppError::Conflict("Email already registered".to_string()));
    }

    // reg_number is unique too — check it here so duplicates give 409, not 500
    let existing_reg =
        sqlx::query_scalar::<_, i32>("SELECT user_id FROM users WHERE reg_number = $1")
            .bind(&body.reg_number)
            .fetch_optional(&state.pool)
            .await?;

    if existing_reg.is_some() {
        return Err(AppError::Conflict(
            "Registration number already registered".to_string(),
        ));
    }

    // hash password with argon2
    let salt = SaltString::generate(&mut OsRng);
    let hashed = Argon2::default()
        .hash_password(body.password.as_bytes(), &salt)
        .map_err(|e| AppError::InternalError(format!("Failed to hash password: {}", e)))?
        .to_string();

    // Everything that can fail against an outside service happens before the
    // account row is written, so a failure leaves nothing behind. Previously
    // the row was inserted first and the OTP email sent last — a bounced email
    // stranded an account that the unique email/reg_number constraints then
    // blocked the student from re-creating.
    let id_card_keys = id_card.as_ref().map(|_| image_upload::new_object_keys());

    if let (Some(images), Some((front_key, back_key))) = (id_card, &id_card_keys) {
        storage::upload(front_key, images.front, image_upload::STORED_CONTENT_TYPE).await?;
        if let Err(e) =
            storage::upload(back_key, images.back, image_upload::STORED_CONTENT_TYPE).await
        {
            storage::delete_quietly(front_key).await;
            return Err(e);
        }
    }

    // generate and send otp — a send failure now costs only the uploaded images
    let code = otp::generate_otp();
    otp::store_otp(&state.pool, &body.email, &code).await?;
    if let Err(e) = email::send_otp_email(&body.email, &code).await {
        discard_uploads(&id_card_keys).await;
        return Err(e);
    }

    // all new registrations start as pending_verification until otp is confirmed
    let (front_path, back_path) = match &id_card_keys {
        Some((f, b)) => (Some(f.as_str()), Some(b.as_str())),
        None => (None, None),
    };

    let inserted = sqlx::query_scalar::<_, i32>(
        "INSERT INTO users (reg_number, name, email, password, status, codeforces_handle, vjudge_handle, id_card_front_path, id_card_back_path) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING user_id",
    )
    .bind(&body.reg_number)
    .bind(&body.name)
    .bind(&body.email)
    .bind(&hashed)
    .bind("pending_verification")
    .bind(cf_handle)
    .bind(vjudge_handle)
    .bind(front_path)
    .bind(back_path)
    .fetch_one(&state.pool)
    .await;

    let user_id = match inserted {
        Ok(id) => id,
        Err(e) => {
            discard_uploads(&id_card_keys).await;
            // two people racing on the same email or reg_number get past the
            // checks above and collide here — that's a conflict, not a 500
            if let sqlx::Error::Database(db) = &e {
                if db.code().as_deref() == Some("23505") {
                    return Err(AppError::Conflict(
                        "Email or registration number already registered".to_string(),
                    ));
                }
            }
            return Err(e.into());
        }
    };

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

    verify_otp_guarded(&state, &body.email, &body.code, "verification code").await?;

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

    state.limiter.check(
        &rate_limit::otp_send_key(&body.email),
        rate_limit::OTP_SEND_MAX,
        rate_limit::OTP_SEND_WINDOW,
    )?;

    // make sure the user exists and is still pending verification
    let status = sqlx::query_scalar::<_, String>("SELECT status FROM users WHERE email = $1")
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
            return Err(AppError::NotFound(
                "No account found with this email".to_string(),
            ));
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
    // cap password guesses per account
    let login_key = rate_limit::login_key(&body.email);
    state.limiter.check(
        &login_key,
        rate_limit::LOGIN_MAX_ATTEMPTS,
        rate_limit::LOGIN_WINDOW,
    )?;

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

    // logged in fine — don't leave successful attempts counted against them
    state.limiter.reset(&login_key);

    // generate jwt token
    let token = create_token(
        user.user_id,
        &user.email,
        user.is_admin.unwrap_or(false),
        user.is_manager.unwrap_or(false),
    )
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

#[derive(Debug, Deserialize)]
pub struct ChangeEmailInput {
    pub current_email: String,
    pub password: String,
    pub new_email: String,
}

#[derive(Debug, Deserialize)]
pub struct ChangeEmailVerifyInput {
    pub new_email: String,
    pub code: String,
}

// step 1 of changing an address: prove you own the account, then we mail a code
// to the address you want to move to
//
// this takes a password rather than a token on purpose — a student waiting on
// manual approval is exactly who needs this, and 'pending' accounts can't log
// in, so they have no token to present
pub async fn request_email_change(
    State(state): State<AppState>,
    Json(body): Json<ChangeEmailInput>,
) -> Result<Json<Value>, AppError> {
    validate_email(&body.current_email)?;
    validate_email(&body.new_email)?;

    let new_email = body.new_email.trim().to_string();
    if new_email.eq_ignore_ascii_case(body.current_email.trim()) {
        return Err(AppError::BadRequest(
            "That is already your email address".to_string(),
        ));
    }

    // this endpoint exists so a manually-registered student can move onto their
    // university address once it arrives — nothing else is a valid target
    if !is_student_email(&new_email) {
        return Err(AppError::BadRequest(
            "You can only change to an @student.sust.edu address".to_string(),
        ));
    }

    // same cap as a failed login, since this also checks a password
    let login_key = rate_limit::login_key(&body.current_email);
    state.limiter.check(
        &login_key,
        rate_limit::LOGIN_MAX_ATTEMPTS,
        rate_limit::LOGIN_WINDOW,
    )?;
    // and the usual cap on how much mail one address can trigger
    state.limiter.check(
        &rate_limit::otp_send_key(&new_email),
        rate_limit::OTP_SEND_MAX,
        rate_limit::OTP_SEND_WINDOW,
    )?;

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(body.current_email.trim())
        .fetch_optional(&state.pool)
        .await?;

    let user = user.ok_or(AppError::Unauthorized(
        "Invalid email or password".to_string(),
    ))?;

    let parsed = PasswordHash::new(&user.password)
        .map_err(|_| AppError::Unauthorized("Invalid email or password".to_string()))?;
    if Argon2::default()
        .verify_password(body.password.as_bytes(), &parsed)
        .is_err()
    {
        return Err(AppError::Unauthorized(
            "Invalid email or password".to_string(),
        ));
    }

    if user.status.as_deref() == Some("rejected") {
        return Err(AppError::Forbidden(
            "This account has been rejected".to_string(),
        ));
    }

    // taken as a live address, or already claimed by someone mid-change
    let taken = sqlx::query_scalar::<_, i32>(
        "SELECT user_id FROM users WHERE (email = $1 OR pending_email = $1) AND user_id <> $2",
    )
    .bind(&new_email)
    .bind(user.user_id)
    .fetch_optional(&state.pool)
    .await?;

    if taken.is_some() {
        return Err(AppError::Conflict(
            "That email is already in use".to_string(),
        ));
    }

    state.limiter.reset(&login_key);

    sqlx::query("UPDATE users SET pending_email = $1 WHERE user_id = $2")
        .bind(&new_email)
        .bind(user.user_id)
        .execute(&state.pool)
        .await?;

    // the code goes to the NEW address — that is what proves they own it
    let code = otp::generate_otp();
    otp::store_otp(&state.pool, &new_email, &code).await?;
    if let Err(e) = email::send_otp_email(&new_email, &code).await {
        sqlx::query("UPDATE users SET pending_email = NULL WHERE user_id = $1")
            .bind(user.user_id)
            .execute(&state.pool)
            .await
            .ok();
        return Err(e);
    }

    Ok(Json(json!({
        "success": true,
        "message": format!("Verification code sent to {} — enter it to finish the change", new_email)
    })))
}

// step 2: the code proves they can read the new inbox, so make the swap
pub async fn confirm_email_change(
    State(state): State<AppState>,
    Json(body): Json<ChangeEmailVerifyInput>,
) -> Result<Json<Value>, AppError> {
    validate_email(&body.new_email)?;
    validate_string(&body.code, "OTP code", 6, 6)?;

    let new_email = body.new_email.trim().to_string();

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE pending_email = $1")
        .bind(&new_email)
        .fetch_optional(&state.pool)
        .await?;

    let user = user.ok_or(AppError::BadRequest(
        "No pending email change for that address".to_string(),
    ))?;

    verify_otp_guarded(&state, &new_email, &body.code, "verification code").await?;

    // someone else may have claimed the address while this change was pending
    let taken = sqlx::query_scalar::<_, i32>("SELECT user_id FROM users WHERE email = $1")
        .bind(&new_email)
        .fetch_optional(&state.pool)
        .await?;

    if taken.is_some() {
        sqlx::query("UPDATE users SET pending_email = NULL WHERE user_id = $1")
            .bind(user.user_id)
            .execute(&state.pool)
            .await
            .ok();
        return Err(AppError::Conflict(
            "That email is already in use".to_string(),
        ));
    }

    // the target is always a university address, and entering the code proves
    // they can read it — that covers both email verification and the manual
    // review, so the account is active whichever state it was waiting in
    let was_waiting = matches!(
        user.status.as_deref(),
        Some("pending_verification") | Some("pending")
    );
    let new_status = "active";

    let updated = sqlx::query_as::<_, User>(
        r#"UPDATE users
           SET email = $1, pending_email = NULL, status = $2
           WHERE user_id = $3
           RETURNING *"#,
    )
    .bind(&new_email)
    .bind(new_status)
    .bind(user.user_id)
    .fetch_one(&state.pool)
    .await?;

    // an approved account no longer needs the id card on file
    discard_id_card(&state.pool, &updated).await;

    tracing::info!(
        "user {} moved to a university address (was waiting: {})",
        updated.user_id,
        was_waiting
    );

    let message = if was_waiting {
        "Email updated and your account is now active — you can log in"
    } else {
        "Email updated — use the new address to log in"
    };

    Ok(Json(json!({
        "success": true,
        "email": updated.email,
        "status": updated.status,
        "message": message
    })))
}

#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    pub email: String,
}

// lets someone waiting on manual approval see where they stand without being
// able to log in yet — returns the status only, never any personal data
pub async fn account_status(
    State(state): State<AppState>,
    Query(query): Query<StatusQuery>,
) -> Result<Json<Value>, AppError> {
    validate_email(&query.email)?;

    // public and unauthenticated, so keep it from being used to sweep addresses
    state.limiter.check(
        &rate_limit::login_key(&query.email),
        rate_limit::LOGIN_MAX_ATTEMPTS,
        rate_limit::LOGIN_WINDOW,
    )?;

    let status = sqlx::query_scalar::<_, String>("SELECT status FROM users WHERE email = $1")
        .bind(&query.email)
        .fetch_optional(&state.pool)
        .await?;

    let status = status.ok_or(AppError::NotFound(
        "No account found with this email".to_string(),
    ))?;

    let message = match status.as_str() {
        "pending_verification" => "Check your email for the verification code",
        "pending" => "Your account is waiting for an admin to review it",
        "active" => "Your account is active — you can log in",
        "rejected" => "Your account was not approved",
        _ => "Unknown account status",
    };

    Ok(Json(json!({
        "success": true,
        "status": status,
        "message": message
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

    state.limiter.check(
        &rate_limit::otp_send_key(&body.email),
        rate_limit::OTP_SEND_MAX,
        rate_limit::OTP_SEND_WINDOW,
    )?;

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
    verify_otp_guarded(&state, &body.email, &body.code, "reset code").await?;

    // hash the new password
    let salt = SaltString::generate(&mut OsRng);
    let hashed = Argon2::default()
        .hash_password(body.new_password.as_bytes(), &salt)
        .map_err(|e| AppError::InternalError(format!("Failed to hash password: {}", e)))?
        .to_string();

    // update the password, and cut every session issued before now — the usual
    // reason to reset is that someone else may be logged in
    sqlx::query("UPDATE users SET password = $1, sessions_valid_from = NOW() WHERE email = $2")
        .bind(&hashed)
        .bind(&body.email)
        .execute(&state.pool)
        .await?;

    Ok(Json(json!({
        "success": true,
        "message": "Password reset successfully — you can now log in with your new password"
    })))
}
