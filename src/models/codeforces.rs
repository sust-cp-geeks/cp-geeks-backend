use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// --- codeforces api response wrappers ---

// top-level wrapper for all cf api responses
#[derive(Debug, Deserialize)]
pub struct CfApiResponse<T> {
    pub status: String,
    pub result: Option<T>,
    pub comment: Option<String>,
}

// from user.info — one entry per handle
#[derive(Debug, Deserialize, Serialize)]
pub struct CfUserInfo {
    pub handle: String,
    pub rating: Option<i32>,
    pub rank: Option<String>,
    #[serde(rename = "maxRating")]
    pub max_rating: Option<i32>,
    #[serde(rename = "maxRank")]
    pub max_rank: Option<String>,
}

// from user.status — one entry per submission
#[derive(Debug, Deserialize)]
pub struct CfSubmission {
    pub id: i64,
    pub verdict: Option<String>,
    #[serde(rename = "creationTimeSeconds")]
    pub creation_time_seconds: i64,
    pub problem: CfProblem,
}

#[derive(Debug, Deserialize)]
pub struct CfProblem {
    #[serde(rename = "contestId")]
    pub contest_id: Option<i32>,
    pub index: Option<String>,
    pub name: String,
    pub rating: Option<i32>,
}

// from user.rating — one entry per rated contest
#[derive(Debug, Deserialize, Serialize)]
pub struct CfRatingChange {
    #[serde(rename = "contestId")]
    pub contest_id: i32,
    #[serde(rename = "contestName")]
    pub contest_name: String,
    pub handle: String,
    pub rank: i32,
    #[serde(rename = "oldRating")]
    pub old_rating: i32,
    #[serde(rename = "newRating")]
    pub new_rating: i32,
    #[serde(rename = "ratingUpdateTimeSeconds")]
    pub rating_update_time_seconds: i64,
}

// --- our api response shapes ---

// solve counts grouped by difficulty bucket for a time period
#[derive(Debug, Serialize)]
pub struct SolveCountPeriod {
    pub total: usize,
    pub buckets: BTreeMap<String, usize>,
}

// all solve counts across time periods
#[derive(Debug, Serialize)]
pub struct SolveCounts {
    pub last_1_month: SolveCountPeriod,
    pub last_6_months: SolveCountPeriod,
    pub last_1_year: SolveCountPeriod,
}

// a single contest performance entry
#[derive(Debug, Serialize)]
pub struct ContestPerformance {
    pub contest_name: String,
    pub rank: i32,
    pub old_rating: i32,
    pub new_rating: i32,
    pub rating_change: i32,
    pub date: String,
}

// full profile stats response
#[derive(Debug, Serialize)]
pub struct CfProfileStats {
    pub codeforces_handle: String,
    pub current_rating: Option<i32>,
    pub current_rank: Option<String>,
    pub max_rating: Option<i32>,
    pub max_rank: Option<String>,
    pub solve_counts: SolveCounts,
    pub recent_contests: Vec<ContestPerformance>,
}

// leaderboard row
#[derive(Debug, Serialize)]
pub struct LeaderboardEntry {
    pub rank: i32,
    pub user_id: i32,
    pub name: String,
    pub codeforces_handle: String,
    pub current_rating: Option<i32>,
    pub current_rank: Option<String>,
}
