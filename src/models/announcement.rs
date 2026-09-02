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
    // pinned posts sort above everything else
    pub is_pinned: bool,
    // an arbitrary outbound link, plus the text the button should show
    pub link_url: Option<String>,
    pub link_label: Option<String>,
    // optional ties to something already in the system
    pub event_id: Option<i32>,
    pub contest_no: Option<i32>,
    // joined from users so the feed can show a byline without a second request.
    // stays None if the author's account was deleted.
    #[sqlx(default)]
    pub author_name: Option<String>,
    // joined so the frontend can label the tie without another request
    #[sqlx(default)]
    pub event_description: Option<String>,
    #[sqlx(default)]
    pub contest_title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAnnouncement {
    pub title: String,
    pub content: String,
    pub category: Option<String>,
    pub event_date: Option<String>,
    pub is_pinned: Option<bool>,
    pub link_url: Option<String>,
    pub link_label: Option<String>,
    pub event_id: Option<i32>,
    pub contest_no: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAnnouncement {
    pub title: Option<String>,
    pub content: Option<String>,
    pub category: Option<String>,
    pub event_date: Option<String>,
    pub is_pinned: Option<bool>,
    pub link_url: Option<String>,
    pub link_label: Option<String>,
    pub event_id: Option<i32>,
    pub contest_no: Option<i32>,
}

// filters for the list endpoint
#[derive(Debug, Deserialize)]
pub struct AnnouncementQuery {
    pub category: Option<String>,
    // only posts whose event_date is still ahead, soonest first
    pub upcoming: Option<bool>,
    pub limit: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_matching_ignores_case_and_returns_canonical_spelling() {
        for input in ["contest", "CONTEST", "Contest", "cOnTeSt"] {
            assert_eq!(
                normalize_category(Some(input)).unwrap(),
                Some("Contest".to_string()),
                "for {input}"
            );
        }
    }

    #[test]
    fn every_listed_category_is_accepted() {
        for c in CATEGORIES {
            assert_eq!(normalize_category(Some(c)).unwrap(), Some(c.to_string()));
        }
    }

    #[test]
    fn unknown_category_is_rejected_with_the_valid_list() {
        // "event" was in the old api docs as an example, so it will be sent
        let err = normalize_category(Some("event")).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("event"), "should name the bad value: {msg}");
        assert!(msg.contains("Contest"), "should list valid values: {msg}");
    }

    #[test]
    fn absent_and_empty_both_mean_no_category() {
        // this is the point: "" must not become a distinct stored value
        assert_eq!(normalize_category(None).unwrap(), None);
        assert_eq!(normalize_category(Some("")).unwrap(), None);
        assert_eq!(normalize_category(Some("   ")).unwrap(), None);
    }
}
