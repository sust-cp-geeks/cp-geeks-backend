use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::AppError;
use crate::models::user::{PublicUser, UpdateProfile, User, PUBLIC_USER_COLUMNS};
use crate::services::{atcoder, codeforces};
use crate::utils::jwt::Claims;
use crate::validation::validate_string;

// get my profile
pub async fn get_me(
    claims: Claims,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE user_id = $1")
        .bind(claims.user_id)
        .fetch_optional(&state.pool)
        .await?;

    let user = user.ok_or(AppError::NotFound("User not found".to_string()))?;

    Ok(Json(json!({
        "success": true,
        "data": user
    })))
}

// update my profile (name, vjudge_handle, codeforces_handle)
pub async fn update_me(
    claims: Claims,
    State(state): State<AppState>,
    Json(body): Json<UpdateProfile>,
) -> Result<Json<Value>, AppError> {
    let existing = sqlx::query_as::<_, User>("SELECT * FROM users WHERE user_id = $1")
        .bind(claims.user_id)
        .fetch_optional(&state.pool)
        .await?;

    let existing = existing.ok_or(AppError::NotFound("User not found".to_string()))?;

    // merge: use new value if provided, keep existing if not
    // same limits register enforces — without these an oversized value just hit
    // the VARCHAR limit and came back as a 500
    let new_name = match body.name {
        Some(name) => {
            validate_string(&name, "Name", 2, 100)?;
            name.trim().to_string()
        }
        None => existing.name,
    };

    // an empty handle means "clear it" — every read path already treats an empty
    // handle as no handle, so store null instead of an empty string
    let new_vjudge = match body.vjudge_handle {
        Some(handle) if handle.trim().is_empty() => None,
        Some(handle) => {
            validate_string(&handle, "VJudge handle", 1, 100)?;
            Some(handle.trim().to_string())
        }
        None => existing.vjudge_handle,
    };

    let new_codeforces = match body.codeforces_handle {
        Some(handle) if handle.trim().is_empty() => None,
        Some(handle) => {
            validate_string(&handle, "Codeforces handle", 1, 50)?;
            // validate the new handle exists on codeforces.com
            codeforces::validate_handle(handle.trim()).await?;
            Some(handle.trim().to_string())
        }
        None => existing.codeforces_handle,
    };

    let previous_atcoder = existing.atcoder_handle.clone();
    let new_atcoder = match body.atcoder_handle {
        Some(handle) if handle.trim().is_empty() => None,
        Some(handle) => {
            validate_string(&handle, "AtCoder handle", 1, 100)?;
            atcoder::validate_handle(handle.trim()).await?;
            Some(handle.trim().to_string())
        }
        None => existing.atcoder_handle,
    };

    let user = sqlx::query_as::<_, User>(
        r#"UPDATE users
           SET name = $1, vjudge_handle = $2, codeforces_handle = $3, atcoder_handle = $4
           WHERE user_id = $5
           RETURNING *"#,
    )
    .bind(&new_name)
    .bind(&new_vjudge)
    .bind(&new_codeforces)
    .bind(&new_atcoder)
    .bind(claims.user_id)
    .fetch_one(&state.pool)
    .await?;

    // a changed handle means the stored atcoder data is now for someone else,
    // so refresh it rather than leaving it stale until the next scheduled pass
    if new_atcoder != previous_atcoder {
        crate::services::platform_sync::sync_user_soon(state.pool.clone(), claims.user_id);
    }

    Ok(Json(json!({
        "success": true,
        "message": "Profile updated successfully",
        "data": user
    })))
}

use axum::extract::{Path, Query};
use std::collections::HashMap;

// public get user profile by id
pub async fn get_user(
    _claims: Claims,
    Path(id): Path<i32>,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let user = sqlx::query_as::<_, PublicUser>(&format!(
        "SELECT {PUBLIC_USER_COLUMNS} FROM users WHERE user_id = $1"
    ))
    .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let user = user.ok_or(AppError::NotFound("User not found".to_string()))?;

    Ok(Json(json!({
        "success": true,
        "data": user
    })))
}

// search users by name
pub async fn search_users(
    _claims: Claims,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let name_query = params
        .get("name")
        .map(|s| format!("%{}%", s))
        .unwrap_or_else(|| "%".to_string());

    let users = sqlx::query_as::<_, PublicUser>(&format!(
        "SELECT {PUBLIC_USER_COLUMNS} FROM users WHERE name ILIKE $1 LIMIT 10"
    ))
    .bind(name_query)
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(json!({
        "success": true,
        "data": users
    })))
}
