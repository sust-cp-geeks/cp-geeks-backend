use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::{require_admin, AppError};
use crate::models::user::User;
use crate::services::storage;
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

// short-lived links so an admin can look at a pending student's id card
// without the bucket ever being public
pub async fn admin_get_id_card(
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

    let (front, back) = match (&user.id_card_front_path, &user.id_card_back_path) {
        (Some(f), Some(b)) => (f, b),
        _ => {
            return Err(AppError::NotFound(
                "This user has no ID card on file".to_string(),
            ))
        }
    };

    tracing::info!("admin {} viewed id card of user {}", claims.user_id, id);

    Ok(Json(json!({
        "success": true,
        "data": {
            "front_url": storage::view_url(front).await?,
            "back_url": storage::view_url(back).await?,
            "expires_in_seconds": 300
        }
    })))
}

// once a decision is made the photos have served their purpose, so drop them —
// keeps storage flat and avoids holding identity documents we no longer need
async fn discard_id_card(pool: &sqlx::PgPool, user: &User) {
    let mut removed = false;
    for key in [&user.id_card_front_path, &user.id_card_back_path]
        .into_iter()
        .flatten()
    {
        storage::delete_quietly(key).await;
        removed = true;
    }

    if removed {
        let cleared = sqlx::query(
            "UPDATE users SET id_card_front_path = NULL, id_card_back_path = NULL WHERE user_id = $1",
        )
        .bind(user.user_id)
        .execute(pool)
        .await;

        if let Err(e) = cleared {
            tracing::warn!("could not clear id card paths for {}: {}", user.user_id, e);
        }
    }
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

    discard_id_card(&state.pool, &updated).await;

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

    discard_id_card(&state.pool, &updated).await;

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
