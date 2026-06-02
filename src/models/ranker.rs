use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- vjudge api response ---

// raw json from vjudge.net/contest/rank/single/{id}
#[derive(Debug, Deserialize)]
pub struct VjudgeContest {
    pub id: u64,
    pub title: String,
    pub length: i64, // contest duration in milliseconds
    pub participants: HashMap<String, serde_json::Value>,
    pub submissions: Vec<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MergedHandle {
    pub name: String,
    pub handles: Vec<String>,
}

// --- ranker request ---

#[derive(Debug, Deserialize)]
pub struct RankerRequest {
    pub title: String,
    pub contest_ids: Vec<u64>,
    pub problem_weights: Option<Vec<Option<Vec<f64>>>>,
    pub custom_titles: Option<Vec<String>>,
    pub merged_handles: Option<Vec<MergedHandle>>,
}

// --- ranker response ---

#[derive(Debug, Clone, Serialize)]
pub struct ContestResult {
    pub contest_name: String,
    pub solved: usize,
    pub upsolved: usize,
    pub penalty: i64,
    pub score: f64,
    pub participated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RankedParticipant {
    pub rank: i32,
    pub handle: String,
    pub total_score: f64,
    pub problems_solved: usize,
    pub total_upsolved: usize,
    pub total_penalty: i64,
    pub contests_participated: usize,
    pub contest_details: Vec<ContestResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RankerResponse {
    pub title: String,
    pub contest_ids: Vec<u64>,
    pub total_contests: usize,
    pub total_participants: usize,
    pub rankings: Vec<RankedParticipant>,
}
