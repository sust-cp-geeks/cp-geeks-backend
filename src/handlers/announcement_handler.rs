use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::NaiveDateTime;
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::{require_admin, AppError};
use crate::models::announcement::{Announcement, CreateAnnouncement, UpdateAnnouncement};
use crate::utils::jwt::Claims;
use crate::validation::validate_string;

// list all announcements, newest first
pub async fn get_announcements(
    _claims: Claims,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let announcements =
        sqlx::query_as::<_, Announcement>("SELECT * FROM announcements ORDER BY created_at DESC")
            .fetch_all(&state.pool)
            .await?;

    Ok(Json(json!({
        "success": true,
        "count": announcements.len(),
        "data": announcements
    })))
}

// get a single announcement by id
pub async fn get_announcement(
    _claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let announcement =
        sqlx::query_as::<_, Announcement>("SELECT * FROM announcements WHERE post_id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;

    let announcement =
        announcement.ok_or(AppError::NotFound("Announcement not found".to_string()))?;

    Ok(Json(json!({"success": true, "data": announcement})))
}

// create a new announcement (admin only), author_id from jwt
pub async fn create_announcement(
    claims: Claims,
    State(state): State<AppState>,
    Json(body): Json<CreateAnnouncement>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    require_admin(&claims)?;

    validate_string(&body.title, "Title", 1, 255)?;
    validate_string(&body.content, "Content", 1, 10000)?;

    let event_date = body
        .event_date
        .as_ref()
        .and_then(|d| NaiveDateTime::parse_from_str(d, "%Y-%m-%dT%H:%M:%S").ok());

    let announcement = sqlx::query_as::<_, Announcement>(
        r#"INSERT INTO announcements (author_id, title, content, category, event_date)
           VALUES ($1, $2, $3, $4, $5) RETURNING *"#,
    )
    .bind(claims.user_id)
    .bind(&body.title)
    .bind(&body.content)
    .bind(&body.category)
    .bind(event_date)
    .fetch_one(&state.pool)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "success": true,
            "message": "Announcement created",
            "data": announcement
        })),
    ))
}

// update an existing announcement (admin only)
pub async fn update_announcement(
    claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateAnnouncement>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    let existing =
        sqlx::query_as::<_, Announcement>("SELECT * FROM announcements WHERE post_id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;

    let existing = existing.ok_or(AppError::NotFound("Announcement not found".to_string()))?;

    let new_title = body.title.unwrap_or(existing.title);
    let new_content = body.content.unwrap_or(existing.content);
    let new_category = match body.category {
        Some(c) => Some(c),
        None => existing.category,
    };
    let new_event_date = match body.event_date {
        Some(d) => NaiveDateTime::parse_from_str(&d, "%Y-%m-%dT%H:%M:%S").ok(),
        None => existing.event_date,
    };

    let announcement = sqlx::query_as::<_, Announcement>(
        r#"UPDATE announcements
           SET title = $1, content = $2, category = $3, event_date = $4
           WHERE post_id = $5
           RETURNING *"#,
    )
    .bind(&new_title)
    .bind(&new_content)
    .bind(&new_category)
    .bind(new_event_date)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({
        "success": true,
        "message": "Announcement updated",
        "data": announcement
    })))
}

// delete an announcement (admin only)
pub async fn delete_announcement(
    claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    let result = sqlx::query("DELETE FROM announcements WHERE post_id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Announcement not found".to_string()));
    }

    Ok(Json(json!({
        "success": true,
        "message": format!("Announcement {} deleted", id)
    })))
}
