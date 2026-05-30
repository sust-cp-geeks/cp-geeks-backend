use std::collections::BTreeMap;

use crate::errors::AppError;
use crate::models::codeforces::{
    CfApiResponse, CfProfileStats, CfRatingChange, CfSubmission, CfUserInfo, ContestPerformance,
    SolveCountPeriod, SolveCounts,
};

const CF_API_BASE: &str = "https://codeforces.com/api";

// difficulty bucket labels (500 gap)
const BUCKETS: &[(&str, i32, i32)] = &[
    ("0-499", 0, 499),
    ("500-999", 500, 999),
    ("1000-1499", 1000, 1499),
    ("1500-1999", 1500, 1999),
    ("2000-2499", 2000, 2499),
    ("2500-2999", 2500, 2999),
    ("3000+", 3000, i32::MAX),
];

// validates that a codeforces handle exists by calling user.info
// returns the user info on success, or an error if the handle doesn't exist
pub async fn validate_handle(handle: &str) -> Result<CfUserInfo, AppError> {
    let url = format!("{}/user.info?handles={}", CF_API_BASE, handle);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    let response = client.get(&url).send().await.map_err(|e| {
        tracing::error!("failed to reach codeforces api: {}", e);
        AppError::InternalError("Could not reach Codeforces API".to_string())
    })?;

    let body = response
        .json::<CfApiResponse<Vec<CfUserInfo>>>()
        .await
        .map_err(|e| {
            tracing::error!("failed to parse cf user.info response: {}", e);
            AppError::InternalError("Failed to parse Codeforces response".to_string())
        })?;

    if body.status != "OK" {
        let msg = body.comment.unwrap_or_else(|| "Handle not found".to_string());
        return Err(AppError::BadRequest(format!(
            "Invalid Codeforces handle: {}",
            msg
        )));
    }

    body.result
        .and_then(|mut users| if users.is_empty() { None } else { Some(users.remove(0)) })
        .ok_or_else(|| AppError::BadRequest("Codeforces handle not found".to_string()))
}

// fetches all submissions for a handle from the cf api
async fn fetch_submissions(handle: &str) -> Result<Vec<CfSubmission>, AppError> {
    let url = format!("{}/user.status?handle={}", CF_API_BASE, handle);

    let response = reqwest::get(&url).await.map_err(|e| {
        tracing::error!("failed to reach codeforces api: {}", e);
        AppError::InternalError("Could not reach Codeforces API".to_string())
    })?;

    let body = response
        .json::<CfApiResponse<Vec<CfSubmission>>>()
        .await
        .map_err(|e| {
            tracing::error!("failed to parse cf user.status response: {}", e);
            AppError::InternalError("Failed to parse Codeforces submissions".to_string())
        })?;

    Ok(body.result.unwrap_or_default())
}

// fetches rating change history for a handle from the cf api
async fn fetch_rating_history(handle: &str) -> Result<Vec<CfRatingChange>, AppError> {
    let url = format!("{}/user.rating?handle={}", CF_API_BASE, handle);

    let response = reqwest::get(&url).await.map_err(|e| {
        tracing::error!("failed to reach codeforces api: {}", e);
        AppError::InternalError("Could not reach Codeforces API".to_string())
    })?;

    let body = response
        .json::<CfApiResponse<Vec<CfRatingChange>>>()
        .await
        .map_err(|e| {
            tracing::error!("failed to parse cf user.rating response: {}", e);
            AppError::InternalError("Failed to parse Codeforces rating history".to_string())
        })?;

    Ok(body.result.unwrap_or_default())
}

// counts solved problems by difficulty bucket within a time window
// only counts unique accepted problems (deduplicates by contest_id + index)
fn count_solves_by_bucket(
    submissions: &[CfSubmission],
    after_timestamp: i64,
) -> SolveCountPeriod {
    let mut seen = std::collections::HashSet::new();
    let mut bucket_counts: BTreeMap<String, usize> = BTreeMap::new();

    // initialize all buckets to 0
    for (label, _, _) in BUCKETS {
        bucket_counts.insert(label.to_string(), 0);
    }

    for sub in submissions {
        // only count accepted solutions
        if sub.verdict.as_deref() != Some("OK") {
            continue;
        }
        // only count submissions within the time window
        if sub.creation_time_seconds < after_timestamp {
            continue;
        }
        // deduplicate by problem identity (contest_id + index)
        let key = format!(
            "{}-{}",
            sub.problem.contest_id.unwrap_or(0),
            sub.problem.index.as_deref().unwrap_or("")
        );
        if !seen.insert(key) {
            continue;
        }

        // place into the correct difficulty bucket
        if let Some(rating) = sub.problem.rating {
            for (label, min, max) in BUCKETS {
                if rating >= *min && rating <= *max {
                    *bucket_counts.entry(label.to_string()).or_insert(0) += 1;
                    break;
                }
            }
        }
    }

    let total: usize = bucket_counts.values().sum();
    SolveCountPeriod {
        total,
        buckets: bucket_counts,
    }
}

// builds the full profile stats by orchestrating all three cf api calls
pub async fn build_profile_stats(handle: &str) -> Result<CfProfileStats, AppError> {
    // fetch all three in parallel for speed
    let (user_info, submissions, rating_history) = tokio::try_join!(
        validate_handle(handle),
        fetch_submissions(handle),
        fetch_rating_history(handle),
    )?;

    let now = chrono::Utc::now().timestamp();
    let one_month_ago = now - (30 * 24 * 60 * 60);
    let six_months_ago = now - (180 * 24 * 60 * 60);
    let one_year_ago = now - (365 * 24 * 60 * 60);

    let solve_counts = SolveCounts {
        last_1_month: count_solves_by_bucket(&submissions, one_month_ago),
        last_6_months: count_solves_by_bucket(&submissions, six_months_ago),
        last_1_year: count_solves_by_bucket(&submissions, one_year_ago),
    };

    // take the last 15 contest performances (most recent first)
    let recent_contests: Vec<ContestPerformance> = rating_history
        .iter()
        .rev()
        .take(15)
        .map(|rc| {
            let dt = chrono::DateTime::from_timestamp(rc.rating_update_time_seconds, 0)
                .map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string())
                .unwrap_or_default();

            ContestPerformance {
                contest_name: rc.contest_name.clone(),
                rank: rc.rank,
                old_rating: rc.old_rating,
                new_rating: rc.new_rating,
                rating_change: rc.new_rating - rc.old_rating,
                date: dt,
            }
        })
        .collect();

    // .rev().take(15) gives most recent first — no additional reverse needed

    Ok(CfProfileStats {
        codeforces_handle: handle.to_string(),
        current_rating: user_info.rating,
        current_rank: user_info.rank,
        max_rating: user_info.max_rating,
        max_rank: user_info.max_rank,
        solve_counts,
        recent_contests,
    })
}
