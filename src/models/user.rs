use serde::{Deserialize, Serialize};
use sqlx::FromRow;

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
    pub is_admin: Option<bool>,
    pub is_manager: Option<bool>,
    pub status: Option<String>,
    pub id_card_path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterInput {
    pub reg_number: String,
    pub name: String,
    pub email: String,
    pub password: String,
    pub codeforces_handle: Option<String>,
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
}
