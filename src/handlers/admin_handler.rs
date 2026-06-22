use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::{require_admin, AppError};
use crate::models::user::User;
use crate::utils::jwt::Claims;

#[derive(Debug, Deserialize)]
pub struct UserFilter {
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StatusUpdateInput {
    pub reason: Option<String>,
}

// get all users, or filter by status like ?status=pending
pub async fn admin_list_users(
    claims: Claims,
    State(state): State<AppState>,
    Query(filter): Query<UserFilter>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    let users = match &filter.status {
        Some(status) => {
            sqlx::query_as::<_, User>("SELECT * FROM users WHERE status = $1 ORDER BY user_id DESC")
                .bind(status)
                .fetch_all(&state.pool)
                .await?
        }
        None => {
            sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY user_id DESC")
                .fetch_all(&state.pool)
                .await?
        }
    };

    Ok(Json(json!({
        "success": true,
        "count": users.len(),
        "data": users
    })))
}

// get a single user's detailed profile
pub async fn admin_get_user(
    claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE user_id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let user = user.ok_or(AppError::NotFound("User not found".to_string()))?;

    Ok(Json(json!({"success": true, "data": user})))
}

// approve a user so they can log in
pub async fn admin_approve_user(
    claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE user_id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let user = user.ok_or(AppError::NotFound("User not found".to_string()))?;

    if user.status.as_deref() != Some("pending") {
        return Err(AppError::BadRequest(format!(
            "Cannot approve user with status '{:?}'",
            user.status
        )));
    }

    let updated = sqlx::query_as::<_, User>(
        "UPDATE users SET status = 'active' WHERE user_id = $1 RETURNING *",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({
        "success": true,
        "message": format!("User '{}' has been approved", updated.name),
        "data": updated
    })))
}

// reject a pending user
pub async fn admin_reject_user(
    claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<StatusUpdateInput>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE user_id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let user = user.ok_or(AppError::NotFound("User not found".to_string()))?;

    if user.status.as_deref() != Some("pending") {
        return Err(AppError::BadRequest(format!(
            "Cannot reject user with status '{:?}'",
            user.status
        )));
    }

    let updated = sqlx::query_as::<_, User>(
        "UPDATE users SET status = 'rejected' WHERE user_id = $1 RETURNING *",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    let message = match &body.reason {
        Some(reason) => format!("User '{}' rejected. Reason: {}", updated.name, reason),
        None => format!("User '{}' has been rejected", updated.name),
    };

    Ok(Json(json!({
        "success": true,
        "message": message,
        "data": updated
    })))
}

// ban an already active user
pub async fn admin_ban_user(
    claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<StatusUpdateInput>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    // safety measure to stop admins locking themselves out
    if claims.user_id == id {
        return Err(AppError::BadRequest("You cannot ban yourself".to_string()));
    }

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE user_id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let user = user.ok_or(AppError::NotFound("User not found".to_string()))?;

    if user.status.as_deref() == Some("rejected") {
        return Err(AppError::BadRequest(
            "User is already rejected/banned".to_string(),
        ));
    }

    let updated = sqlx::query_as::<_, User>(
        "UPDATE users SET status = 'rejected' WHERE user_id = $1 RETURNING *",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    let message = match &body.reason {
        Some(reason) => format!("User '{}' banned. Reason: {}", updated.name, reason),
        None => format!("User '{}' has been banned", updated.name),
    };

    Ok(Json(json!({
        "success": true,
        "message": message,
        "data": updated
    })))
}
