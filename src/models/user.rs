use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// what another member is allowed to see. the full User row carries the id card
// storage keys, a pending email mid-change and session bookkeeping — none of it
// anyone else's business, and none of it used by the frontend. selecting columns
// explicitly also means a column added later is private until we say otherwise.
#[derive(Debug, FromRow, Serialize)]
pub struct PublicUser {
    pub user_id: i32,
    pub reg_number: String,
    pub name: String,
    pub email: String,
    pub vjudge_handle: Option<String>,
    pub codeforces_handle: Option<String>,
    pub atcoder_handle: Option<String>,
    pub is_admin: Option<bool>,
    pub is_manager: Option<bool>,
    pub status: Option<String>,
}

pub const PUBLIC_USER_COLUMNS: &str = "user_id, reg_number, name, email, vjudge_handle, \
     codeforces_handle, atcoder_handle, is_admin, is_manager, status";

#[derive(Debug, FromRow, Serialize)]
pub struct User {
    pub user_id: i32,
    pub reg_number: String,
    pub name: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub vjudge_handle: Option<String>,
    pub codeforces_handle: Option<String>,
    pub atcoder_handle: Option<String>,
    pub is_admin: Option<bool>,
    pub is_manager: Option<bool>,
    pub status: Option<String>,
    pub id_card_path: Option<String>,
    pub id_card_front_path: Option<String>,
    pub id_card_back_path: Option<String>,
    pub pending_email: Option<String>,
    pub sessions_valid_from: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterInput {
    pub reg_number: String,
    pub name: String,
    pub email: String,
    pub password: String,
    pub codeforces_handle: Option<String>,
    pub vjudge_handle: Option<String>,
    pub atcoder_handle: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfile {
    pub name: Option<String>,
    pub vjudge_handle: Option<String>,
    pub codeforces_handle: Option<String>,
    pub atcoder_handle: Option<String>,
}
