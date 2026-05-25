use axum::{extract::{Path, State}, Json};
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
    let handle = sqlx::query_scalar::<_, String>(
        "SELECT codeforces_handle FROM users WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await?;

    let handle = handle.ok_or(AppError::NotFound(
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
    // fetch all active users who have a cf handle, ordered by handle
    let rows = sqlx::query_as::<_, (i32, String, String)>(
        r#"SELECT user_id, name, codeforces_handle
           FROM users
           WHERE status IN ('active', 'pending', 'pending_verification')
             AND codeforces_handle IS NOT NULL
             AND codeforces_handle != ''
           ORDER BY name ASC"#,
    )
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok(Json(json!({
            "success": true,
            "count": 0,
            "data": []
        })));
    }

    // batch-fetch ratings from cf api using semicolon-separated handles
    let handles: Vec<&str> = rows.iter().map(|(_, _, h)| h.as_str()).collect();
    let handles_param = handles.join(";");
    let url = format!(
        "https://codeforces.com/api/user.info?handles={}",
        handles_param
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    let response = client.get(&url).send().await.map_err(|e| {
        tracing::error!("failed to reach codeforces api for leaderboard: {}", e);
        AppError::InternalError("Could not reach Codeforces API".to_string())
    })?;

    let body = response
        .json::<crate::models::codeforces::CfApiResponse<Vec<crate::models::codeforces::CfUserInfo>>>()
        .await
        .map_err(|e| {
            tracing::error!("failed to parse cf user.info for leaderboard: {}", e);
            AppError::InternalError("Failed to parse Codeforces response".to_string())
        })?;

    let cf_users = body.result.unwrap_or_default();

    // build a handle -> (rating, rank) lookup map
    let rating_map: std::collections::HashMap<String, (Option<i32>, Option<String>)> = cf_users
        .iter()
        .map(|u| (u.handle.to_lowercase(), (u.rating, u.rank.clone())))
        .collect();

    // split into rated and unrated
    let mut rated: Vec<(i32, String, String, i32, Option<String>)> = Vec::new();
    let mut unrated: Vec<(i32, String, String)> = Vec::new();

    for (user_id, name, handle) in &rows {
        match rating_map.get(&handle.to_lowercase()) {
            Some((Some(r), rank_opt)) => rated.push((*user_id, name.clone(), handle.clone(), *r, rank_opt.clone())),
            _ => unrated.push((*user_id, name.clone(), handle.clone())),
        }
    }

    // sort rated users by rating descending
    rated.sort_by(|a, b| b.3.cmp(&a.3));

    // rated users get sequential ranks 1, 2, 3, ...
    let mut leaderboard: Vec<LeaderboardEntry> = rated
        .into_iter()
        .enumerate()
        .map(|(i, (user_id, name, handle, rating, rank_opt))| LeaderboardEntry {
            rank: (i + 1) as i32,
            user_id,
            name,
            codeforces_handle: handle,
            current_rating: Some(rating),
            current_rank: rank_opt,
        })
        .collect();

    // all unrated users share the same last rank
    let unrated_rank = (leaderboard.len() + 1) as i32;
    for (user_id, name, handle) in unrated {
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
