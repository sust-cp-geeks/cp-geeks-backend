use axum::{extract::State, Json};
use serde_json::Value;

use crate::{
    app_state::AppState,
    errors::{require_admin, AppError},
    models::problem::*,
    utils::jwt::Claims,
    validation::{validate_string, validate_url},
};
use std::collections::HashMap;

pub async fn get_problems(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let sections = sqlx::query_as::<_, DbProblemSection>(
        "SELECT id, name, description, created_at FROM problem_sections ORDER BY id ASC",
    )
    .fetch_all(&state.pool)
    .await?;

    let subsections = sqlx::query_as::<_, DbProblemSubsection>(
        "SELECT id, section_id, name, description, created_at FROM problem_subsections ORDER BY id ASC",
    )
    .fetch_all(&state.pool)
    .await?;

    let items = sqlx::query_as::<_, DbProblemItem>(
        "SELECT id, subsection_id, item_type, title, url, platform, created_at FROM problem_items ORDER BY id ASC",
    )
    .fetch_all(&state.pool)
    .await?;

    let mut items_by_sub: HashMap<i32, Vec<ProblemItem>> = HashMap::new();
    for item in items {
        if let Some(sub_id) = item.subsection_id {
            items_by_sub.entry(sub_id).or_default().push(ProblemItem {
                id: item.id,
                item_type: item.item_type,
                title: item.title,
                url: item.url,
                platform: item.platform,
            });
        }
    }

    let mut subs_by_sec: HashMap<i32, Vec<ProblemSubsection>> = HashMap::new();
    for sub in subsections {
        if let Some(sec_id) = sub.section_id {
            let sub_items = items_by_sub.remove(&sub.id).unwrap_or_default();
            subs_by_sec
                .entry(sec_id)
                .or_default()
                .push(ProblemSubsection {
                    id: sub.id,
                    name: sub.name,
                    items: sub_items,
                });
        }
    }

    let mut result = Vec::new();
    for sec in sections {
        let sec_subs = subs_by_sec.remove(&sec.id).unwrap_or_default();
        result.push(ProblemSection {
            id: sec.id,
            name: sec.name,
            description: sec.description,
            subsections: sec_subs,
        });
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "data": result
    })))
}

pub async fn create_section(
    claims: Claims,
    State(state): State<AppState>,
    Json(payload): Json<CreateSectionReq>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    validate_string(&payload.name, "Name", 1, 255)?;

    sqlx::query("INSERT INTO problem_sections (name, description) VALUES ($1, $2)")
        .bind(payload.name)
        .bind(payload.description)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn create_subsection(
    claims: Claims,
    State(state): State<AppState>,
    Json(payload): Json<CreateSubsectionReq>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    validate_string(&payload.name, "Name", 1, 255)?;

    // check the section exists first — otherwise the foreign key blows up as a 500
    let section_exists =
        sqlx::query_scalar::<_, i32>("SELECT id FROM problem_sections WHERE id = $1")
            .bind(payload.section_id)
            .fetch_optional(&state.pool)
            .await?;

    if section_exists.is_none() {
        return Err(AppError::NotFound("Section not found".to_string()));
    }

    sqlx::query(
        "INSERT INTO problem_subsections (section_id, name, description) VALUES ($1, $2, $3)",
    )
    .bind(payload.section_id)
    .bind(payload.name)
    .bind(payload.description)
    .execute(&state.pool)
    .await?;

    Ok(Json(serde_json::json!({ "success": true })))
}

pub async fn create_item(
    claims: Claims,
    State(state): State<AppState>,
    Json(payload): Json<CreateItemReq>,
) -> Result<Json<Value>, AppError> {
    require_admin(&claims)?;

    validate_string(&payload.title, "Title", 1, 255)?;
    validate_string(&payload.item_type, "Item type", 1, 50)?;
    validate_url(&payload.url, "URL")?;

    // check the subsection exists first — otherwise the foreign key blows up as a 500
    let subsection_exists =
        sqlx::query_scalar::<_, i32>("SELECT id FROM problem_subsections WHERE id = $1")
            .bind(payload.subsection_id)
            .fetch_optional(&state.pool)
            .await?;

    if subsection_exists.is_none() {
        return Err(AppError::NotFound("Subsection not found".to_string()));
    }

    sqlx::query(
        "INSERT INTO problem_items (subsection_id, item_type, title, url, platform) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(payload.subsection_id)
    .bind(payload.item_type)
    .bind(payload.title)
    .bind(payload.url)
    .bind(payload.platform)
    .execute(&state.pool)
    .await?;

    Ok(Json(serde_json::json!({ "success": true })))
}
