use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::{require_admin_or_manager, AppError};
use crate::models::announcement::{
    normalize_category, Announcement, CreateAnnouncement, UpdateAnnouncement, CATEGORIES,
};
use crate::utils::jwt::Claims;
use crate::validation::{parse_datetime, validate_string};

// lets the frontend populate a category dropdown without duplicating the list
pub async fn get_categories() -> Json<Value> {
    Json(json!({ "success": true, "data": CATEGORIES }))
}

// every read goes through this so the byline is always present
const SELECT_WITH_AUTHOR: &str = r#"
    SELECT a.post_id, a.author_id, a.title, a.content, a.category,
           a.event_date, a.created_at, a.updated_at, u.name AS author_name
    FROM announcements a
    LEFT JOIN users u ON a.author_id = u.user_id
"#;

// re-reads one post through the join, so create and update return the same
// shape as a plain read
async fn fetch_one(pool: &sqlx::PgPool, id: i32) -> Result<Announcement, AppError> {
    let sql = format!("{} WHERE a.post_id = $1", SELECT_WITH_AUTHOR);
    sqlx::query_as::<_, Announcement>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound("Announcement not found".to_string()))
}

// list all announcements, newest first
pub async fn get_announcements(
    _claims: Claims,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let sql = format!("{} ORDER BY a.created_at DESC", SELECT_WITH_AUTHOR);
    let announcements = sqlx::query_as::<_, Announcement>(&sql)
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
    let announcement = fetch_one(&state.pool, id).await?;

    Ok(Json(json!({"success": true, "data": announcement})))
}

// create a new announcement (admin only), author_id from jwt
pub async fn create_announcement(
    claims: Claims,
    State(state): State<AppState>,
    Json(body): Json<CreateAnnouncement>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    require_admin_or_manager(&claims)?;

    validate_string(&body.title, "Title", 1, 255)?;
    validate_string(&body.content, "Content", 1, 10000)?;

    let event_date = match &body.event_date {
        Some(d) => parse_datetime(d, "event_date")?,
        None => None,
    };
    let category = normalize_category(body.category.as_deref())?;

    let post_id = sqlx::query_scalar::<_, i32>(
        r#"INSERT INTO announcements (author_id, title, content, category, event_date)
           VALUES ($1, $2, $3, $4, $5) RETURNING post_id"#,
    )
    .bind(claims.user_id)
    .bind(&body.title)
    .bind(&body.content)
    .bind(&category)
    .bind(event_date)
    .fetch_one(&state.pool)
    .await?;

    let announcement = fetch_one(&state.pool, post_id).await?;

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
    require_admin_or_manager(&claims)?;

    let existing = fetch_one(&state.pool, id).await?;

    let new_title = body.title.unwrap_or(existing.title);
    let new_content = body.content.unwrap_or(existing.content);
    let new_category = match body.category.as_deref() {
        Some(c) => normalize_category(Some(c))?,
        None => existing.category,
    };
    let new_event_date = match body.event_date {
        Some(d) => parse_datetime(&d, "event_date")?,
        None => existing.event_date,
    };

    sqlx::query(
        r#"UPDATE announcements
           SET title = $1, content = $2, category = $3, event_date = $4, updated_at = NOW()
           WHERE post_id = $5"#,
    )
    .bind(&new_title)
    .bind(&new_content)
    .bind(&new_category)
    .bind(new_event_date)
    .bind(id)
    .execute(&state.pool)
    .await?;

    let announcement = fetch_one(&state.pool, id).await?;

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
    require_admin_or_manager(&claims)?;

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
