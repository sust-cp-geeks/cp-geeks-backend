use axum::{
    extract::{Path, State},
    Json,
};
use chrono::NaiveDateTime;
use serde_json::{json, Value};
use sqlx::FromRow;

use crate::app_state::AppState;
use crate::errors::AppError;
use crate::models::codeforces::{AttendanceSummary, CfProfileStats, LeaderboardEntry, SolveCounts};
use crate::services::codeforces;
use crate::services::platform_sync::CODEFORCES;
use crate::utils::jwt::Claims;

// what the background sync leaves behind, and all we can serve when codeforces
// itself is unreachable
#[derive(Debug, FromRow)]
struct StoredCfProfile {
    handle: String,
    rating: Option<i32>,
    max_rating: Option<i32>,
    rank_title: Option<String>,
    synced_at: Option<NaiveDateTime>,
    sync_error: Option<String>,
}

// get codeforces profile stats for a registered user
pub async fn get_cf_stats(
    _claims: Claims,
    State(state): State<AppState>,
    Path(user_id): Path<i32>,
) -> Result<Json<Value>, AppError> {
    // look up the user's cf handle from our database
    let handle = sqlx::query_scalar::<_, Option<String>>(
        "SELECT codeforces_handle FROM users WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;

    // outer none = no such user, inner none = user has no handle set
    let handle = handle
        .flatten()
        .filter(|h| !h.trim().is_empty())
        .ok_or(AppError::NotFound(
            "User not found or has no Codeforces handle".to_string(),
        ))?;

    // the live call is the good path: it carries submissions, rating history and
    // attendance, none of which we store for codeforces.
    match codeforces::build_profile_stats(&handle).await {
        Ok(stats) => Ok(Json(json!({"success": true, "data": stats}))),

        // but codeforces goes down, and when it did the whole profile page turned
        // into an error — including the rating we already had on disk. serve what
        // the sync stored and let the page say it is stale.
        Err(live_err) => {
            tracing::warn!(
                "codeforces profile for user {} ({}) falling back to stored data: {:?}",
                user_id,
                handle,
                live_err
            );

            let stored = sqlx::query_as::<_, StoredCfProfile>(
                r#"SELECT handle, rating, max_rating, rank_title, synced_at, sync_error
                   FROM platform_profiles
                   WHERE user_id = $1 AND platform = $2"#,
            )
            .bind(user_id)
            .bind(CODEFORCES)
            .fetch_optional(&state.pool)
            .await?;

            // nothing synced yet means there is genuinely nothing to show, so the
            // original upstream error is the honest answer
            let stored = match stored {
                Some(p) => p,
                None => return Err(live_err),
            };

            Ok(Json(
                json!({"success": true, "data": degraded_profile(stored)}),
            ))
        }
    }
}

// the stored row rendered in the shape the live call would have returned, so the
// frontend renders one profile component either way and only has to branch on
// `stale`. kept separate from the handler so the mapping is testable without a
// database or a codeforces outage.
fn degraded_profile(stored: StoredCfProfile) -> CfProfileStats {
    CfProfileStats {
        codeforces_handle: stored.handle,
        current_rating: stored.rating,
        current_rank: stored.rank_title,
        max_rating: stored.max_rating,
        // codeforces sends the peak title next to the peak rating and we never
        // stored it. leave it unset rather than infer a band.
        max_rank: None,
        // these three are derived from the submission and rating history, which
        // we only ever hold for atcoder — for codeforces they are live-only
        solve_counts: SolveCounts::default(),
        recent_contests: Vec::new(),
        contest_attendance: Vec::new(),
        attendance_summary: AttendanceSummary::default(),
        stale: true,
        synced_at: stored.synced_at,
        sync_error: stored.sync_error,
    }
}

// community leaderboard — all active users ranked by cf rating
pub async fn get_leaderboard(
    _claims: Claims,
    State(state): State<AppState>,
) -> Result<Json<Value>, AppError> {
    // read what the background sync stored. this used to ask codeforces for
    // every rating while the page was loading, so a codeforces outage emptied
    // the leaderboard entirely — now an outage only means the numbers are as
    // fresh as the last successful pass.
    let rows = sqlx::query_as::<_, (i32, String, String, Option<i32>, Option<String>)>(
        r#"SELECT u.user_id, u.name, u.codeforces_handle,
                  p.rating, p.rank_title
           FROM users u
           LEFT JOIN platform_profiles p
             ON p.user_id = u.user_id AND p.platform = 'codeforces'
           WHERE u.status IN ('active', 'pending', 'pending_verification')
             AND u.codeforces_handle IS NOT NULL
             AND u.codeforces_handle != ''
           ORDER BY u.name ASC"#,
    )
    .fetch_all(&state.pool)
    .await?;

    // rated members rank by rating; everyone still unrated shares the last place
    let (mut rated, unrated): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .partition(|(_, _, _, rating, _)| rating.is_some());

    rated.sort_by_key(|a| std::cmp::Reverse(a.3));

    let mut leaderboard: Vec<LeaderboardEntry> = rated
        .into_iter()
        .enumerate()
        .map(
            |(i, (user_id, name, handle, rating, rank_title))| LeaderboardEntry {
                rank: (i + 1) as i32,
                user_id,
                name,
                codeforces_handle: handle,
                current_rating: rating,
                current_rank: rank_title,
            },
        )
        .collect();

    let unrated_rank = (leaderboard.len() + 1) as i32;
    for (user_id, name, handle, _, _) in unrated {
        leaderboard.push(LeaderboardEntry {
            rank: unrated_rank,
            user_id,
            name,
            codeforces_handle: handle,
            current_rating: None,
            current_rank: None,
        });
    }

    Ok(Json(json!({
        "success": true,
        "count": leaderboard.len(),
        "data": leaderboard
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> StoredCfProfile {
        StoredCfProfile {
            handle: "tourist".to_string(),
            rating: Some(3800),
            max_rating: Some(4009),
            rank_title: Some("legendary grandmaster".to_string()),
            synced_at: NaiveDateTime::parse_from_str("2026-09-07 06:00:00", "%Y-%m-%d %H:%M:%S")
                .ok(),
            sync_error: None,
        }
    }

    #[test]
    fn degraded_profile_is_flagged_stale() {
        // the whole point: the frontend must be able to tell that a rating it is
        // about to print may be six hours old
        assert!(degraded_profile(stored()).stale);
    }

    #[test]
    fn degraded_profile_keeps_what_the_sync_stored() {
        let p = degraded_profile(stored());
        assert_eq!(p.codeforces_handle, "tourist");
        assert_eq!(p.current_rating, Some(3800));
        assert_eq!(p.max_rating, Some(4009));
        assert_eq!(p.current_rank.as_deref(), Some("legendary grandmaster"));
        assert!(p.synced_at.is_some());
    }

    #[test]
    fn degraded_profile_leaves_live_only_sections_empty() {
        // serving zeroes here would read as "solved nothing recently" rather than
        // "we could not ask", which is why the response carries `stale` at all
        let p = degraded_profile(stored());
        assert!(p.recent_contests.is_empty());
        assert!(p.contest_attendance.is_empty());
        assert_eq!(p.attendance_summary.total_contests, 0);
        assert_eq!(p.solve_counts.last_1_year.total, 0);
        // never guessed from max_rating
        assert!(p.max_rank.is_none());
    }

    #[test]
    fn degraded_profile_surfaces_a_sync_error() {
        // a handle that no longer exists and an outage both land here, and they
        // need opposite advice, so the reason has to survive the mapping
        let mut s = stored();
        s.sync_error = Some("handle not found".to_string());
        assert_eq!(
            degraded_profile(s).sync_error.as_deref(),
            Some("handle not found")
        );
    }
}
