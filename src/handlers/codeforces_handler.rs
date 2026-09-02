use axum::{
    extract::{Path, State},
    Json,
};
use serde_json::{json, Value};

use crate::app_state::AppState;
use crate::errors::AppError;
use crate::models::codeforces::LeaderboardEntry;
use crate::services::codeforces;
use crate::utils::jwt::Claims;

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

    // fetch live stats from codeforces api
    let stats = codeforces::build_profile_stats(&handle).await?;

    Ok(Json(json!({"success": true, "data": stats})))
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
    let (mut rated, unrated): (Vec<_>, Vec<_>) =
        rows.into_iter().partition(|(_, _, _, rating, _)| rating.is_some());

    rated.sort_by(|a, b| b.3.cmp(&a.3));

    let mut leaderboard: Vec<LeaderboardEntry> = rated
        .into_iter()
        .enumerate()
        .map(|(i, (user_id, name, handle, rating, rank_title))| LeaderboardEntry {
            rank: (i + 1) as i32,
            user_id,
            name,
            codeforces_handle: handle,
            current_rating: rating,
            current_rank: rank_title,
        })
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
