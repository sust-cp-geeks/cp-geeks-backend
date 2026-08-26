use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::errors::AppError;

// the categories a post may carry
//
// kept as a fixed list so the feed can be filtered reliably — free text drifts
// into "Update" / "update" / "Updates" as separate values, and any filter or
// dropdown built from them then silently misses posts.
// changing this list is safe: it is validated in the api, not in the database.
pub const CATEGORIES: &[&str] = &["Contest", "Result", "Notice", "Update", "General"];

// accepts any casing and returns the canonical spelling, so "contest" and
// "CONTEST" both store as "Contest". an empty value means "no category".
pub fn normalize_category(value: Option<&str>) -> Result<Option<String>, AppError> {
    let raw = match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) => v,
        None => return Ok(None),
    };

    CATEGORIES
        .iter()
        .find(|c| c.eq_ignore_ascii_case(raw))
        .map(|c| Some(c.to_string()))
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Invalid category '{}' — must be one of: {}",
                raw,
                CATEGORIES.join(", ")
            ))
        })
}

#[derive(Debug, FromRow, Serialize)]
pub struct Announcement {
    pub post_id: i32,
    pub author_id: Option<i32>,
    pub title: String,
    pub content: String,
    pub category: Option<String>,
    pub event_date: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    // null until the post is edited
    pub updated_at: Option<NaiveDateTime>,
    // joined from users so the feed can show a byline without a second request.
    // stays None if the author's account was deleted.
    #[sqlx(default)]
    pub author_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAnnouncement {
    pub title: String,
    pub content: String,
    pub category: Option<String>,
    pub event_date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAnnouncement {
    pub title: Option<String>,
    pub content: Option<String>,
    pub category: Option<String>,
    pub event_date: Option<String>,
}
