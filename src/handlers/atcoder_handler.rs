use axum::{
    extract::{Path, State},
    Json,
};
use chrono::NaiveDateTime;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::FromRow;

use crate::app_state::AppState;
use crate::errors::AppError;
use crate::services::atcoder;
use crate::services::platform_sync::ATCODER;
use crate::utils::jwt::Claims;

// nothing here calls atcoder: the background sync fills these tables, so a
// profile view is a database read regardless of how slow the upstream is
#[derive(Debug, FromRow, Serialize)]
struct StoredProfile {
    handle: String,
    rating: Option<i32>,
    max_rating: Option<i32>,
    rank_title: Option<String>,
    solved_count: Option<i32>,
    solve_counts: Option<serde_json::Value>,
    synced_at: Option<NaiveDateTime>,
    sync_error: Option<String>,
}

#[derive(Debug, FromRow, Serialize)]
struct StoredHistory {
    contest_id: String,
    contest_name: String,
    old_rating: Option<i32>,
    new_rating: Option<i32>,
    place: Option<i32>,
    performance: Option<i32>,
    is_rated: bool,
    ended_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize)]
struct ContestAttendance {
    contest_id: String,
    contest_name: String,
    date: String,
    participated: bool,
    place: Option<i32>,
    performance: Option<i32>,
    rating_change: Option<i32>,
    is_rated: bool,
    // whether their rating at the time fell inside the round's rated band
    eligible: bool,
}

#[derive(Debug, Serialize)]
struct AttendanceSummary {
    total_contests: usize,
    participated: usize,
    missed: usize,
    ineligible: usize,
}

// same window rule as the codeforces view: only contests from their first
// appearance onward, since nobody can miss a round that predates their account
const MAX_ATTENDANCE_ROWS: usize = 100;

#[derive(Debug, FromRow, Serialize)]
struct LeaderboardRow {
    user_id: i32,
    name: String,
    handle: String,
    rating: Option<i32>,
    max_rating: Option<i32>,
    rank_title: Option<String>,
    solved_count: Option<i32>,
    synced_at: Option<NaiveDateTime>,
}

// atcoder profile for one member, served entirely from stored rows
pub async fn get_atcoder_stats(
    _claims: Claims,
    State(state): State<AppState>,
    Path(user_id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    let profile = sqlx::query_as::<_, StoredProfile>(
        r#"SELECT handle, rating, max_rating, rank_title, solved_count,
                  solve_counts, synced_at, sync_error
           FROM platform_profiles WHERE user_id = $1 AND platform = $2"#,
    )
    .bind(user_id)
    .bind(ATCODER)
    .fetch_optional(&state.pool)
    .await?;

    let profile = profile.ok_or(AppError::NotFound(
        "No AtCoder profile for this user yet — add a handle and wait for the next sync"
            .to_string(),
    ))?;

    let history = sqlx::query_as::<_, StoredHistory>(
        r#"SELECT contest_id, contest_name, old_rating, new_rating, place,
                  performance, is_rated, ended_at
           FROM platform_rating_history
           WHERE user_id = $1 AND platform = $2
           ORDER BY ended_at DESC"#,
    )
    .bind(user_id)
    .bind(ATCODER)
    .fetch_all(&state.pool)
    .await?;

    // the contest list comes from upstream, so it can be unavailable. that is
    // no reason to fail the whole profile — drop the attendance section and
    // still serve the rating, history and solve counts.
    let (attendance, summary) = match build_attendance(&history, profile.rating).await {
        Ok(pair) => pair,
        Err(e) => {
            tracing::warn!(
                "atcoder attendance unavailable for user {}: {:?}",
                user_id,
                e
            );
            (
                Vec::new(),
                AttendanceSummary {
                    total_contests: 0,
                    participated: 0,
                    missed: 0,
                    ineligible: 0,
                },
            )
        }
    };

    Ok(Json(json!({
        "success": true,
        "data": {
            "atcoder_handle": profile.handle,
            "current_rating": profile.rating,
            "current_rank": profile.rank_title,
            "max_rating": profile.max_rating,
            // same shape the codeforces profile returns, so the chart component
            // is reusable; the bands differ because atcoder uses its own
            "max_rank": profile.max_rating.map(atcoder::rank_title),
            "solved_count": profile.solved_count,
            "solve_counts": profile.solve_counts,
            "synced_at": profile.synced_at,
            // set when the last sync could not read this handle, so a stale or
            // wrong handle is visible instead of looking like no activity
            "sync_error": profile.sync_error,
            "recent_contests": history.iter().take(15).collect::<Vec<_>>(),
            "contest_attendance": attendance,
            "attendance_summary": summary,
        }
    })))
}

// pairs the cached contest list against what they actually entered
async fn build_attendance(
    history: &[StoredHistory],
    current_rating: Option<i32>,
) -> Result<(Vec<ContestAttendance>, AttendanceSummary), AppError> {
    let contests = atcoder::fetch_contest_list().await?;

    let entered: std::collections::HashMap<&str, &StoredHistory> =
        history.iter().map(|h| (h.contest_id.as_str(), h)).collect();

    // their first contest marks the start of a meaningful window
    let joined_at = history
        .iter()
        .filter_map(|h| h.ended_at)
        .min()
        .map(|d| d.and_utc().timestamp());

    let rows: Vec<ContestAttendance> = contests
        .iter()
        .filter(|c| match joined_at {
            Some(joined) => c.start_epoch_second >= joined - 86_400,
            None => false,
        })
        .take(MAX_ATTENDANCE_ROWS)
        .map(|c| {
            // history stores the screen name (agc004.contest.atcoder.jp); the
            // contest list uses the bare id (agc004)
            let key = format!("{}.contest.atcoder.jp", c.id);
            let entry = entered.get(key.as_str());
            let date = chrono::DateTime::from_timestamp(c.start_epoch_second, 0)
                .map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string())
                .unwrap_or_default();

            ContestAttendance {
                contest_id: c.id.clone(),
                contest_name: c.title.clone(),
                date,
                participated: entry.is_some(),
                place: entry.and_then(|e| e.place),
                performance: entry.and_then(|e| e.performance),
                rating_change: entry.and_then(|e| match (e.new_rating, e.old_rating) {
                    (Some(n), Some(o)) => Some(n - o),
                    _ => None,
                }),
                is_rated: entry.map(|e| e.is_rated).unwrap_or(true),
                // turning up proves it, whatever the band says
                eligible: entry.is_some() || atcoder::is_eligible(&c.rate_change, current_rating),
            }
        })
        .collect();

    let participated = rows.iter().filter(|r| r.participated).count();
    let ineligible = rows.iter().filter(|r| !r.eligible).count();
    let summary = AttendanceSummary {
        total_contests: rows.len(),
        participated,
        // only a round they could have entered counts as missed
        missed: rows.len() - participated - ineligible,
        ineligible,
    };

    Ok((rows, summary))
}

// community leaderboard, ranked by stored rating
pub async fn get_atcoder_leaderboard(
    _claims: Claims,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    let rows = sqlx::query_as::<_, LeaderboardRow>(
        r#"SELECT u.user_id, u.name, p.handle, p.rating, p.max_rating,
                  p.rank_title, p.solved_count, p.synced_at
           FROM platform_profiles p
           JOIN users u ON u.user_id = p.user_id
           WHERE p.platform = $1 AND p.sync_error IS NULL
           ORDER BY p.rating DESC NULLS LAST, u.name ASC"#,
    )
    .bind(ATCODER)
    .fetch_all(&state.pool)
    .await?;

    // rated members get sequential ranks; unrated ones share the final place
    let mut rank = 0;
    let data: Vec<Value> = rows
        .iter()
        .map(|r| {
            if r.rating.is_some() {
                rank += 1;
            }
            json!({
                "rank": if r.rating.is_some() { Some(rank) } else { None },
                "user_id": r.user_id,
                "name": r.name,
                "atcoder_handle": r.handle,
                "current_rating": r.rating,
                "max_rating": r.max_rating,
                "current_rank": r.rank_title,
                "solved_count": r.solved_count,
                "synced_at": r.synced_at,
            })
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "count": data.len(),
        "data": data
    })))
}
