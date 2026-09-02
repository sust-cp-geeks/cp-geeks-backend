use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::app_state::AppState;
use crate::errors::{require_admin_or_manager, AppError};
use crate::models::event::{
    CreateEventInput, Event, EventResponse, TeamInput, TeamMemberWithProfile, TeamWithMembers,
    UpdateEventInput,
};
use crate::utils::jwt::Claims;
use crate::validation::validate_string;

// helper rows for the join queries
#[derive(sqlx::FromRow)]
struct TeamRow {
    team_id: i32,
    event_id: Option<i32>,
    coach_name: Option<String>,
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    member_id: i32,
    team_id: Option<i32>,
    reg_number: String,
    user_id: Option<i32>,
    name: Option<String>,
}

// assembles teams + members for a set of event ids using batch queries instead of N+1
async fn build_event_responses(
    pool: &sqlx::PgPool,
    events: Vec<Event>,
) -> Result<Vec<EventResponse>, AppError> {
    if events.is_empty() {
        return Ok(Vec::new());
    }

    let event_ids: Vec<i32> = events.iter().map(|e| e.event_id).collect();

    // one query for ALL teams across all events
    let teams = sqlx::query_as::<_, TeamRow>(
        "SELECT team_id, event_id, coach_name FROM teams WHERE event_id = ANY($1) ORDER BY team_id ASC",
    )
    .bind(&event_ids)
    .fetch_all(pool)
    .await?;

    let team_ids: Vec<i32> = teams.iter().map(|t| t.team_id).collect();

    // one query for ALL members across all teams, joined with users for profile info
    let members = sqlx::query_as::<_, MemberRow>(
        r#"SELECT tm.member_id, tm.team_id, tm.reg_number, u.user_id, u.name
           FROM team_members tm
           LEFT JOIN users u ON tm.reg_number = u.reg_number
           WHERE tm.team_id = ANY($1)
           ORDER BY tm.member_id ASC"#,
    )
    .bind(&team_ids)
    .fetch_all(pool)
    .await?;

    // group members by team_id
    let mut members_by_team: HashMap<i32, Vec<TeamMemberWithProfile>> = HashMap::new();
    for m in members {
        members_by_team
            .entry(m.team_id.unwrap_or(0))
            .or_default()
            .push(TeamMemberWithProfile {
                member_id: m.member_id,
                reg_number: m.reg_number,
                user_id: m.user_id,
                name: m.name,
            });
    }

    // group teams by event_id
    let mut teams_by_event: HashMap<i32, Vec<TeamWithMembers>> = HashMap::new();
    for t in teams {
        let members = members_by_team.remove(&t.team_id).unwrap_or_default();
        teams_by_event
            .entry(t.event_id.unwrap_or(0))
            .or_default()
            .push(TeamWithMembers {
                team_id: t.team_id,
                coach_name: t.coach_name,
                members,
            });
    }

    // assemble final response
    let response = events
        .into_iter()
        .map(|e| {
            let teams = teams_by_event.remove(&e.event_id).unwrap_or_default();
            EventResponse {
                event_id: e.event_id,
                title: e.title,
                description: e.description,
                vjudge_contest_ids: e.vjudge_contest_ids,
                merged_handles: e.merged_handles.map(|j| j.0),
                teams,
            }
        })
        .collect();

    Ok(response)
}

// list all events with their teams and members (3 queries total, not N+1)
pub async fn get_events(
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let events = sqlx::query_as::<_, Event>("SELECT * FROM events /* v3 */ ORDER BY created_at DESC")
        .fetch_all(&state.pool)
        .await?;

    let response = build_event_responses(&state.pool, events).await?;

    Ok(Json(json!({
        "success": true,
        "count": response.len(),
        "data": response
    })))
}

// get a single event with its teams and members
pub async fn get_event(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let event = sqlx::query_as::<_, Event>("SELECT * FROM events /* v3 */ WHERE event_id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let event = event.ok_or(AppError::NotFound("Event not found".to_string()))?;

    let mut response = build_event_responses(&state.pool, vec![event]).await?;
    let event_data = response.pop().unwrap();

    Ok(Json(json!({"success": true, "data": event_data})))
}

// create a new event (admin/manager only)
pub async fn create_event(
    claims: Claims,
    State(state): State<AppState>,
    Json(body): Json<CreateEventInput>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    require_admin_or_manager(&claims)?;

    validate_string(&body.title, "Title", 1, 255)?;
    validate_string(&body.description, "Description", 1, 10000)?;

    let event = sqlx::query_as::<_, Event>(
        r#"INSERT INTO events (title, description, vjudge_contest_ids, merged_handles)
           VALUES ($1, $2, $3, $4) RETURNING * /* v3 */"#,
    )
    .bind(&body.title)
    .bind(&body.description)
    .bind(&body.vjudge_contest_ids)
    .bind(body.merged_handles.map(sqlx::types::Json))
    .fetch_one(&state.pool)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "success": true,
            "message": "Event created",
            "data": event
        })),
    ))
}

// update an event (admin/manager only) — partial update with merge
pub async fn update_event(
    claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<UpdateEventInput>,
) -> Result<Json<Value>, AppError> {
    require_admin_or_manager(&claims)?;

    let existing = sqlx::query_as::<_, Event>("SELECT * FROM events /* v3 */ WHERE event_id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?;

    let existing = existing.ok_or(AppError::NotFound("Event not found".to_string()))?;

    let new_title = body.title.unwrap_or(existing.title);
    let new_description = body.description.unwrap_or(existing.description);

    let new_vjudge_contest_ids = body.vjudge_contest_ids.or(existing.vjudge_contest_ids);
    let new_merged_handles = body.merged_handles.map(sqlx::types::Json).or(existing.merged_handles);

    validate_string(&new_title, "Title", 1, 255)?;
    validate_string(&new_description, "Description", 1, 10000)?;

    let event = sqlx::query_as::<_, Event>(
        r#"UPDATE events SET title = $1, description = $2, vjudge_contest_ids = $3, merged_handles = $4
           WHERE event_id = $5 RETURNING * /* v3 */"#,
    )
    .bind(&new_title)
    .bind(&new_description)
    .bind(&new_vjudge_contest_ids)
    .bind(&new_merged_handles)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({
        "success": true,
        "message": "Event updated",
        "data": event
    })))
}

// delete an event (admin/manager only)
pub async fn delete_event(
    claims: Claims,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    require_admin_or_manager(&claims)?;

    let result = sqlx::query("DELETE FROM events WHERE event_id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Event not found".to_string()));
    }

    Ok(Json(json!({
        "success": true,
        "message": format!("Event {} deleted", id)
    })))
}

// add a team to an event (admin/manager only)
pub async fn add_team(
    claims: Claims,
    State(state): State<AppState>,
    Path(event_id): Path<i32>,
    Json(body): Json<TeamInput>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    require_admin_or_manager(&claims)?;

    if body.members.len() != 3 {
        return Err(AppError::BadRequest(
            "A team must have exactly 3 members".to_string(),
        ));
    }

    // check the event exists first — otherwise the foreign key blows up as a 500
    let event_exists =
        sqlx::query_scalar::<_, i32>("SELECT event_id FROM events WHERE event_id = $1")
            .bind(event_id)
            .fetch_optional(&state.pool)
            .await?;

    if event_exists.is_none() {
        return Err(AppError::NotFound("Event not found".to_string()));
    }

    let mut tx = state.pool.begin().await?;

    let team_id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO teams (event_id, coach_name) VALUES ($1, $2) RETURNING team_id",
    )
    .bind(event_id)
    .bind(&body.coach_name)
    .fetch_one(&mut *tx)
    .await?;

    for reg_number in &body.members {
        validate_string(reg_number, "Registration number", 1, 50)?;
        sqlx::query("INSERT INTO team_members (team_id, reg_number) VALUES ($1, $2)")
            .bind(team_id)
            .bind(reg_number)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "success": true,
            "message": "Team added successfully"
        })),
    ))
}

// update a team (admin/manager only) — replaces all members
pub async fn update_team(
    claims: Claims,
    State(state): State<AppState>,
    Path((event_id, team_id)): Path<(i32, i32)>,
    Json(body): Json<TeamInput>,
) -> Result<Json<Value>, AppError> {
    require_admin_or_manager(&claims)?;

    if body.members.len() != 3 {
        return Err(AppError::BadRequest(
            "A team must have exactly 3 members".to_string(),
        ));
    }

    let mut tx = state.pool.begin().await?;

    // scope by event too — otherwise any team could be edited through any
    // event's url, since the event_id in the path was never checked
    let result =
        sqlx::query("UPDATE teams SET coach_name = $1 WHERE team_id = $2 AND event_id = $3")
            .bind(&body.coach_name)
            .bind(team_id)
            .bind(event_id)
            .execute(&mut *tx)
            .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Team not found".to_string()));
    }

    // delete existing members and insert new ones
    sqlx::query("DELETE FROM team_members WHERE team_id = $1")
        .bind(team_id)
        .execute(&mut *tx)
        .await?;

    for reg_number in &body.members {
        validate_string(reg_number, "Registration number", 1, 50)?;
        sqlx::query("INSERT INTO team_members (team_id, reg_number) VALUES ($1, $2)")
            .bind(team_id)
            .bind(reg_number)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    Ok(Json(json!({
        "success": true,
        "message": "Team updated successfully"
    })))
}

// delete a team (admin/manager only)
pub async fn delete_team(
    claims: Claims,
    State(state): State<AppState>,
    Path((event_id, team_id)): Path<(i32, i32)>,
) -> Result<Json<Value>, AppError> {
    require_admin_or_manager(&claims)?;

    // scope by event so a team can't be deleted through another event's url
    let result = sqlx::query("DELETE FROM teams WHERE team_id = $1 AND event_id = $2")
        .bind(team_id)
        .bind(event_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Team not found".to_string()));
    }

    Ok(Json(json!({
        "success": true,
        "message": format!("Team {} deleted", team_id)
    })))
}
