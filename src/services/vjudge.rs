use crate::errors::AppError;
use crate::models::ranker::VjudgeContest;

const VJUDGE_RANK_URL: &str = "https://vjudge.net/contest/rank/single";

// fetches contest standings json from vjudge
pub async fn fetch_contest(contest_id: u64) -> Result<VjudgeContest, AppError> {
    let url = format!("{}/{}", VJUDGE_RANK_URL, contest_id);

    // vjudge blocks requests without a proper user-agent (403)
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) SUST-CP-Geeks/1.0")
        .build()
        .map_err(|e| {
            tracing::error!("failed to build http client: {}", e);
            AppError::InternalError("HTTP client error".to_string())
        })?;

    let response = client.get(&url).send().await.map_err(|e| {
        tracing::error!("failed to reach vjudge for contest {}: {}", contest_id, e);
        AppError::InternalError("Could not reach VJudge".to_string())
    })?;

    if !response.status().is_success() {
        return Err(AppError::BadRequest(format!(
            "VJudge contest {} not found or not accessible",
            contest_id
        )));
    }

    let contest = response.json::<VjudgeContest>().await.map_err(|e| {
        tracing::error!("failed to parse vjudge response for {}: {}", contest_id, e);
        AppError::InternalError("Failed to parse VJudge contest data".to_string())
    })?;

    Ok(contest)
}
