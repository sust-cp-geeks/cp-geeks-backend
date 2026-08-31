use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, FromRow, Serialize)]
pub struct Event {
    pub event_id: i32,
    pub title: String,
    pub description: String,
    pub created_at: Option<NaiveDateTime>,
    pub vjudge_contest_ids: Option<Vec<i64>>,
    pub merged_handles: Option<sqlx::types::Json<serde_json::Value>>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct Team {
    pub team_id: i32,
    pub event_id: Option<i32>,
    pub coach_name: Option<String>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct TeamMember {
    pub member_id: i32,
    pub team_id: Option<i32>,
    pub reg_number: String,
}

// input for creating an event
#[derive(Debug, Deserialize)]
pub struct CreateEventInput {
    pub title: String,
    pub description: String,
    pub vjudge_contest_ids: Option<Vec<i64>>,
    pub merged_handles: Option<serde_json::Value>,
}

// input for updating an event (all fields optional for partial updates)
#[derive(Debug, Deserialize)]
pub struct UpdateEventInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub vjudge_contest_ids: Option<Vec<i64>>,
    pub merged_handles: Option<serde_json::Value>,
}

// input for creating or updating a team
#[derive(Debug, Deserialize)]
pub struct TeamInput {
    pub coach_name: Option<String>,
    pub members: Vec<String>, // exactly 3 registration numbers
}

// aggregated response structs for nested event data
#[derive(Debug, Serialize)]
pub struct TeamMemberWithProfile {
    pub member_id: i32,
    pub reg_number: String,
    pub user_id: Option<i32>,
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TeamWithMembers {
    pub team_id: i32,
    pub coach_name: Option<String>,
    pub members: Vec<TeamMemberWithProfile>,
}

#[derive(Debug, Serialize)]
pub struct EventResponse {
    pub event_id: i32,
    pub title: String,
    pub description: String,
    pub vjudge_contest_ids: Option<Vec<i64>>,
    pub merged_handles: Option<serde_json::Value>,
    pub teams: Vec<TeamWithMembers>,
}
