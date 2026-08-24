use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::{require_admin, AppError};
use crate::models::contest::{Contest, CreateContest, UpdateContest};
use crate::utils::jwt::Claims;
use crate::validation::{parse_datetime, validate_string, validate_url};

// list all contests, newest first
pub async fn get_contests(
    _claims: Claims,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let contests = sqlx::query_as::<_, Contest>("SELECT * FROM contests ORDER BY created_at DESC")
        .fetch_all(&state.pool)
        .await?;

    Ok(Json(json!({
        "success": true,
        "count": contests.len(),
        "data": contests
    })))
}

// get a single contest by id
pub async fn get_contest(
    _claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let contest = sqlx::query_as::<_, Contest>("SELECT * FROM contests WHERE contest_no = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let contest = contest.ok_or(AppError::NotFound("Contest not found".to_string()))?;

    Ok(Json(json!({"success": true, "data": contest})))
}

// create a new contest (admin only)
pub async fn create_contest(
    claims: Claims,
    State(state): State<AppState>,
    Json(body): Json<CreateContest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    require_admin(&claims)?;

    validate_string(&body.title, "Title", 1, 255)?;
    validate_string(&body.contest_link, "Contest link", 1, 255)?;
    validate_url(&body.contest_link, "Contest link")?;

    let contest_date = match &body.contest_date {
        Some(d) => parse_datetime(d, "contest_date")?,
        None => None,
    };

    let contest = sqlx::query_as::<_, Contest>(
        r#"INSERT INTO contests (title, contest_link, contest_date)
           VALUES ($1, $2, $3) RETURNING *"#,
    )
    .bind(&body.title)
    .bind(&body.contest_link)
    .bind(contest_date)
    .fetch_one(&state.pool)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "success": true,
            "message": "Contest created",
            "data": contest
        })),
    ))
}

// update an existing contest (admin only)
pub async fn update_contest(
    claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateContest>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    let existing = sqlx::query_as::<_, Contest>("SELECT * FROM contests WHERE contest_no = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let existing = existing.ok_or(AppError::NotFound("Contest not found".to_string()))?;

    let new_title = body.title.unwrap_or(existing.title);
    let new_link = body.contest_link.unwrap_or(existing.contest_link);
    let new_date = match body.contest_date {
        Some(d) => parse_datetime(&d, "contest_date")?,
        None => existing.contest_date,
    };

    let contest = sqlx::query_as::<_, Contest>(
        r#"UPDATE contests
           SET title = $1, contest_link = $2, contest_date = $3
           WHERE contest_no = $4
           RETURNING *"#,
    )
    .bind(&new_title)
    .bind(&new_link)
    .bind(new_date)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({
        "success": true,
        "message": "Contest updated",
        "data": contest
    })))
}

// delete a contest (admin only)
pub async fn delete_contest(
    claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    let result = sqlx::query("DELETE FROM contests WHERE contest_no = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Contest not found".to_string()));
    }

    Ok(Json(json!({
        "success": true,
        "message": format!("Contest {} deleted", id)
    })))
}
