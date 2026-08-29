use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::{require_admin_or_manager, AppError};
use crate::models::announcement::{
    normalize_category, Announcement, AnnouncementQuery, CreateAnnouncement, UpdateAnnouncement,
    CATEGORIES,
};
use crate::utils::jwt::Claims;
use crate::validation::{parse_datetime, validate_string, validate_url};

// lets the frontend populate a category dropdown without duplicating the list
pub async fn get_categories() -> Json<Value> {
    Json(json!({ "success": true, "data": CATEGORIES }))
}

// every read goes through this so the byline is always present
const SELECT_WITH_AUTHOR: &str = r#"
    SELECT a.post_id, a.author_id, a.title, a.content, a.category,
           a.event_date, a.created_at, a.updated_at, a.is_pinned,
           a.link_url, a.link_label, a.event_id, a.contest_no,
           u.name AS author_name,
           e.description AS event_description,
           c.title AS contest_title
    FROM announcements a
    LEFT JOIN users u ON a.author_id = u.user_id
    LEFT JOIN events e ON a.event_id = e.event_id
    LEFT JOIN contests c ON a.contest_no = c.contest_no
"#;

// how many posts a single list request may return
const MAX_LIMIT: i64 = 100;
const DEFAULT_LIMIT: i64 = 50;

// an outbound link has to be a real http(s) url — this ends up as an anchor in
// the frontend, so javascript: and data: must never get through
fn clean_link(
    url: Option<&str>,
    label: Option<&str>,
) -> Result<(Option<String>, Option<String>), AppError> {
    let url = match url.map(str::trim).filter(|v| !v.is_empty()) {
        Some(u) => {
            validate_string(u, "Link URL", 1, 500)?;
            validate_url(u, "Link URL")?;
            Some(u.to_string())
        }
        None => None,
    };

    let label = match label.map(str::trim).filter(|v| !v.is_empty()) {
        Some(l) => {
            validate_string(l, "Link label", 1, 100)?;
            Some(l.to_string())
        }
        None => None,
    };

    // a label with nothing to point at is meaningless
    if url.is_none() && label.is_some() {
        return Err(AppError::BadRequest(
            "link_label needs a link_url".to_string(),
        ));
    }

    Ok((url, label))
}

// referencing something that isn't there should be a 404, not a foreign key 500
async fn check_relations(
    pool: &sqlx::PgPool,
    event_id: Option<i32>,
    contest_no: Option<i32>,
) -> Result<(), AppError> {
    if let Some(id) = event_id {
        let found = sqlx::query_scalar::<_, i32>("SELECT event_id FROM events WHERE event_id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
        found.ok_or(AppError::NotFound("Event not found".to_string()))?;
    }
    if let Some(no) = contest_no {
        let found =
            sqlx::query_scalar::<_, i32>("SELECT contest_no FROM contests WHERE contest_no = $1")
                .bind(no)
                .fetch_optional(pool)
                .await?;
        found.ok_or(AppError::NotFound("Contest not found".to_string()))?;
    }
    Ok(())
}

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
    Query(query): Query<AnnouncementQuery>,
) -> Result<Json<Value>, AppError> {
    // only ever assembled from validated pieces; every value is still bound
    let mut wheres: Vec<String> = Vec::new();
    let category = normalize_category(query.category.as_deref())?;
    if category.is_some() {
        wheres.push("a.category = $1".to_string());
    }

    let upcoming = query.upcoming.unwrap_or(false);
    if upcoming {
        wheres.push("a.event_date IS NOT NULL AND a.event_date >= NOW()".to_string());
    }

    let filter = if wheres.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", wheres.join(" AND "))
    };

    // pinned always rides at the top; "what's next" sorts soonest-first,
    // otherwise the feed is newest-first
    let order = if upcoming {
        "ORDER BY a.is_pinned DESC, a.event_date ASC"
    } else {
        "ORDER BY a.is_pinned DESC, a.created_at DESC"
    };

    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let sql = format!("{}{} {} LIMIT {}", SELECT_WITH_AUTHOR, filter, order, limit);

    let mut q = sqlx::query_as::<_, Announcement>(&sql);
    if let Some(c) = &category {
        q = q.bind(c);
    }
    let announcements = q.fetch_all(&state.pool).await?;

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
    let (link_url, link_label) = clean_link(body.link_url.as_deref(), body.link_label.as_deref())?;
    check_relations(&state.pool, body.event_id, body.contest_no).await?;

    let post_id = sqlx::query_scalar::<_, i32>(
        r#"INSERT INTO announcements
             (author_id, title, content, category, event_date,
              is_pinned, link_url, link_label, event_id, contest_no)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING post_id"#,
    )
    .bind(claims.user_id)
    .bind(&body.title)
    .bind(&body.content)
    .bind(&category)
    .bind(event_date)
    .bind(body.is_pinned.unwrap_or(false))
    .bind(&link_url)
    .bind(&link_label)
    .bind(body.event_id)
    .bind(body.contest_no)
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

    // sending a field replaces it, omitting it keeps what was there
    let (new_link_url, new_link_label) = match body.link_url.as_deref() {
        // an empty url clears the whole link, label included — otherwise the
        // leftover label would fail the "label needs a url" check
        Some(u) if u.trim().is_empty() => (None, None),
        Some(u) => clean_link(
            Some(u),
            body.link_label
                .as_deref()
                .or(existing.link_label.as_deref()),
        )?,
        // changing only the label keeps whatever url is already there
        None => match body.link_label.as_deref() {
            Some(l) => clean_link(existing.link_url.as_deref(), Some(l))?,
            None => (existing.link_url.clone(), existing.link_label.clone()),
        },
    };
    let new_pinned = body.is_pinned.unwrap_or(existing.is_pinned);
    let new_event_id = body.event_id.or(existing.event_id);
    let new_contest_no = body.contest_no.or(existing.contest_no);
    check_relations(&state.pool, body.event_id, body.contest_no).await?;

    sqlx::query(
        r#"UPDATE announcements
           SET title = $1, content = $2, category = $3, event_date = $4,
               is_pinned = $5, link_url = $6, link_label = $7,
               event_id = $8, contest_no = $9, updated_at = NOW()
           WHERE post_id = $10"#,
    )
    .bind(&new_title)
    .bind(&new_content)
    .bind(&new_category)
    .bind(new_event_date)
    .bind(new_pinned)
    .bind(&new_link_url)
    .bind(&new_link_label)
    .bind(new_event_id)
    .bind(new_contest_no)
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
