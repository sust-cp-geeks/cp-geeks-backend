use chrono::NaiveDateTime;
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

// from contest.list — one entry per contest that exists on codeforces
#[derive(Debug, Clone, Deserialize)]
pub struct CfContestListItem {
    pub id: i32,
    pub name: String,
    pub phase: String,
    #[serde(rename = "startTimeSeconds")]
    pub start_time_seconds: Option<i64>,
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
#[derive(Debug, Serialize, Default)]
pub struct SolveCountPeriod {
    pub total: usize,
    pub buckets: BTreeMap<String, usize>,
}

// all solve counts across time periods
#[derive(Debug, Serialize, Default)]
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

// one contest on the timeline, whether or not they showed up
#[derive(Debug, Serialize)]
pub struct ContestAttendance {
    pub contest_id: i32,
    pub contest_name: String,
    pub date: String,
    pub participated: bool,
    // false when they were not allowed to enter (a pupil cannot join Div. 1),
    // or when the contest was unrated so participation is undetectable.
    // the frontend should not shade an ineligible contest red.
    pub eligible: bool,
    // only present when participated — the frontend can show the delta inline
    pub rank: Option<i32>,
    pub old_rating: Option<i32>,
    pub new_rating: Option<i32>,
    pub rating_change: Option<i32>,
}

// how much of the timeline they turned up for
#[derive(Debug, Serialize, Default)]
pub struct AttendanceSummary {
    pub total_contests: usize,
    pub participated: usize,
    // eligible contests they did not enter — the only ones that are truly missed
    pub missed: usize,
    // contests they could never have entered, excluded from `missed`
    pub ineligible: usize,
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
    // every contest since their first rated one, newest first, each flagged
    // participated or missed
    pub contest_attendance: Vec<ContestAttendance>,
    pub attendance_summary: AttendanceSummary,
    // true when codeforces could not be reached and these numbers came out of
    // our own tables instead. the frontend needs to say so rather than present
    // a possibly days-old rating as current.
    pub stale: bool,
    // when the background sync last wrote these values. null on a live read,
    // because a live read is by definition current.
    pub synced_at: Option<NaiveDateTime>,
    // set when the last sync could not read this handle. distinguishes "codeforces
    // is down" from "this handle no longer exists", which look identical from the
    // outside but need opposite responses from the member.
    pub sync_error: Option<String>,
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
