use serde::{Deserialize, Serialize};
use chrono::NaiveDateTime;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DbProblemSection {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DbProblemSubsection {
    pub id: i32,
    pub section_id: Option<i32>,
    pub name: String,
    pub description: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DbProblemItem {
    pub id: i32,
    pub subsection_id: Option<i32>,
    pub item_type: String,
    pub title: String,
    pub url: String,
    pub platform: Option<String>,
    pub created_at: Option<NaiveDateTime>,
}

// Combined response
#[derive(Debug, Serialize, Deserialize)]
pub struct ProblemItem {
    pub id: i32,
    pub item_type: String,
    pub title: String,
    pub url: String,
    pub platform: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProblemSubsection {
    pub id: i32,
    pub name: String,
    pub items: Vec<ProblemItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProblemSection {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub subsections: Vec<ProblemSubsection>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSectionReq {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSubsectionReq {
    pub section_id: i32,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateItemReq {
    pub subsection_id: i32,
    pub item_type: String,
    pub title: String,
    pub url: String,
    pub platform: Option<String>,
}
