use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::errors::AppError;
use crate::services::http;

const ATCODER_BASE: &str = "https://atcoder.jp";
const KENKOOOO_BASE: &str = "https://kenkoooo.com/atcoder";

// kenkoooo's docs ask for more than a second between calls. atcoder.jp itself
// publishes no figure, so it gets the same courtesy.
pub const POLITE_DELAY: Duration = Duration::from_millis(1200);

// the contest list is a megabyte and identical for everyone
const CONTEST_LIST_TTL: Duration = Duration::from_secs(6 * 60 * 60);

// --- what the two upstreams return ---

// atcoder.jp/users/{handle}/history/json — one entry per contest entered.
// this is the official endpoint behind their own rating graph.
#[derive(Debug, Clone, Deserialize)]
pub struct AtcoderHistoryEntry {
    #[serde(rename = "IsRated")]
    pub is_rated: bool,
    #[serde(rename = "Place")]
    pub place: i32,
    #[serde(rename = "OldRating")]
    pub old_rating: i32,
    #[serde(rename = "NewRating")]
    pub new_rating: i32,
    #[serde(rename = "Performance")]
    pub performance: i32,
    #[serde(rename = "ContestScreenName")]
    pub contest_screen_name: String,
    #[serde(rename = "ContestName")]
    pub contest_name: String,
    #[serde(rename = "EndTime")]
    pub end_time: String,
}

// kenkoooo v3/user_info — small, and the only cheap source of a solve count
#[derive(Debug, Deserialize)]
pub struct AtcoderUserInfo {
    pub accepted_count: i64,
}

// kenkoooo resources/contests.json — every contest that has ever run
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AtcoderContest {
    pub id: String,
    pub title: String,
    pub start_epoch_second: i64,
    pub rate_change: String,
}

// --- fetching ---

// a handle that does not exist still answers 200 [] on the history endpoint,
// which is indistinguishable from a real member who has never competed.
// the profile page returns a real 404, so ask that instead.
pub async fn validate_handle(handle: &str) -> Result<(), AppError> {
    let url = format!("{}/users/{}", ATCODER_BASE, handle);
    let response = http::codeforces().get(&url).send().await.map_err(|e| {
        tracing::error!("failed to reach atcoder: {}", e);
        AppError::InternalError("Could not reach AtCoder".to_string())
    })?;

    if response.status().as_u16() == 404 {
        return Err(AppError::BadRequest(format!(
            "No AtCoder user found with handle '{}'",
            handle
        )));
    }
    if !response.status().is_success() {
        tracing::error!("atcoder returned {} for {}", response.status(), handle);
        return Err(AppError::InternalError(
            "Could not verify AtCoder handle".to_string(),
        ));
    }
    Ok(())
}

// full rating history, newest last (atcoder returns oldest first)
pub async fn fetch_history(handle: &str) -> Result<Vec<AtcoderHistoryEntry>, AppError> {
    let url = format!("{}/users/{}/history/json", ATCODER_BASE, handle);
    let response = http::codeforces().get(&url).send().await.map_err(|e| {
        tracing::error!("atcoder history request failed for {}: {}", handle, e);
        AppError::InternalError("Could not reach AtCoder".to_string())
    })?;

    if !response.status().is_success() {
        return Err(AppError::InternalError(format!(
            "AtCoder returned {} for {}",
            response.status(),
            handle
        )));
    }

    response
        .json::<Vec<AtcoderHistoryEntry>>()
        .await
        .map_err(|e| {
            tracing::error!("could not parse atcoder history for {}: {}", handle, e);
            AppError::InternalError("Failed to parse AtCoder response".to_string())
        })
}

// total accepted problems; not fatal if it fails, so callers treat it as optional
pub async fn fetch_solved_count(handle: &str) -> Result<i64, AppError> {
    let url = format!("{}/atcoder-api/v3/user_info?user={}", KENKOOOO_BASE, handle);
    let response = http::codeforces().get(&url).send().await.map_err(|e| {
        tracing::error!("kenkoooo user_info failed for {}: {}", handle, e);
        AppError::InternalError("Could not reach AtCoder Problems".to_string())
    })?;

    let info = response.json::<AtcoderUserInfo>().await.map_err(|e| {
        tracing::error!("could not parse kenkoooo user_info for {}: {}", handle, e);
        AppError::InternalError("Failed to parse AtCoder Problems response".to_string())
    })?;

    Ok(info.accepted_count)
}

type ContestCache = Mutex<Option<(Instant, Vec<AtcoderContest>)>>;
static CONTEST_CACHE: OnceLock<ContestCache> = OnceLock::new();

fn contest_cache() -> &'static ContestCache {
    CONTEST_CACHE.get_or_init(|| Mutex::new(None))
}

// every contest ever run, newest first — shared by every member, so it is
// fetched once and held rather than pulled per profile view
pub async fn fetch_contest_list() -> Result<Vec<AtcoderContest>, AppError> {
    {
        let cached = contest_cache().lock().unwrap_or_else(|p| p.into_inner());
        if let Some((at, list)) = cached.as_ref() {
            if at.elapsed() < CONTEST_LIST_TTL {
                return Ok(list.clone());
            }
        }
    }

    let url = format!("{}/resources/contests.json", KENKOOOO_BASE);
    // the storage client, because this is a megabyte — the api client's 10s
    // budget truncates it and the parse fails
    let response = http::storage().get(&url).send().await.map_err(|e| {
        tracing::error!("atcoder contest list request failed: {}", e);
        AppError::InternalError("Could not reach AtCoder Problems".to_string())
    })?;

    let mut list = response.json::<Vec<AtcoderContest>>().await.map_err(|e| {
        tracing::error!("could not parse atcoder contest list: {}", e);
        AppError::InternalError("Failed to parse AtCoder Problems response".to_string())
    })?;

    // most of the 6000+ entries are Daily Training and Weekday Beta rounds,
    // which carry rate_change "-" and nobody treats as contests. keeping them
    // would bury the real ABC/ARC/AGC rounds under practice sessions.
    list.retain(|c| rating_range(&c.rate_change).is_some());
    list.sort_by_key(|c| std::cmp::Reverse(c.start_epoch_second));
    tracing::info!("cached {} atcoder contests", list.len());
    *contest_cache().lock().unwrap_or_else(|p| p.into_inner()) =
        Some((Instant::now(), list.clone()));

    Ok(list)
}

// who a round was rated for, read from the contest list's rate_change field
//
// atcoder states this as data — "~ 1999", "1200 ~ 2799", "All" — so unlike
// codeforces there is no need to guess a division from the contest title.
// returns None for an unrated round: a practice session nobody can be said to
// have missed.
pub fn rating_range(rate_change: &str) -> Option<(i32, i32)> {
    let text = rate_change.trim();
    if text.is_empty() || text == "-" || text == "\u{00d7}" {
        return None;
    }
    if text.eq_ignore_ascii_case("all") {
        return Some((i32::MIN, i32::MAX));
    }

    let (low, high) = text.split_once('~')?;
    let min = match low.trim() {
        "" => i32::MIN,
        v => v.parse().ok()?,
    };
    let max = match high.trim() {
        "" => i32::MAX,
        v => v.parse().ok()?,
    };
    Some((min, max))
}

// whether a member of this rating was allowed to compete rated
pub fn is_eligible(rate_change: &str, rating: Option<i32>) -> bool {
    match (rating_range(rate_change), rating) {
        (Some((min, max)), Some(r)) => r >= min && r <= max,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

// difficulty bands, using atcoder's own colours rather than the 500-wide
// ranges codeforces uses — these are the bands their community actually talks in
pub const DIFFICULTY_BUCKETS: &[(&str, i32, i32)] = &[
    ("0-399", i32::MIN, 399),
    ("400-799", 400, 799),
    ("800-1199", 800, 1199),
    ("1200-1599", 1200, 1599),
    ("1600-1999", 1600, 1999),
    ("2000-2399", 2000, 2399),
    ("2400+", 2400, i32::MAX),
];

// one accepted submission as kenkoooo reports it
#[derive(Debug, Clone, Deserialize)]
pub struct AtcoderSubmission {
    pub epoch_second: i64,
    pub problem_id: String,
    pub result: String,
}

#[derive(Debug, Deserialize)]
struct ProblemModel {
    difficulty: Option<i32>,
}

type ModelCache = Mutex<Option<(Instant, std::collections::HashMap<String, i32>)>>;
static MODEL_CACHE: OnceLock<ModelCache> = OnceLock::new();

fn model_cache() -> &'static ModelCache {
    MODEL_CACHE.get_or_init(|| Mutex::new(None))
}

// problem_id -> estimated difficulty, shared by everyone and about a megabyte,
// so it is fetched once rather than per member
pub async fn fetch_problem_difficulties() -> Result<std::collections::HashMap<String, i32>, AppError>
{
    {
        let cached = model_cache().lock().unwrap_or_else(|p| p.into_inner());
        if let Some((at, map)) = cached.as_ref() {
            if at.elapsed() < CONTEST_LIST_TTL {
                return Ok(map.clone());
            }
        }
    }

    let url = format!("{}/resources/problem-models.json", KENKOOOO_BASE);
    // likewise about a megabyte
    let response = http::storage().get(&url).send().await.map_err(|e| {
        tracing::error!("problem-models request failed: {}", e);
        AppError::InternalError("Could not reach AtCoder Problems".to_string())
    })?;

    let raw = response
        .json::<std::collections::HashMap<String, ProblemModel>>()
        .await
        .map_err(|e| {
            tracing::error!("could not parse problem-models: {}", e);
            AppError::InternalError("Failed to parse AtCoder Problems response".to_string())
        })?;

    // not every problem has been modelled yet
    let map: std::collections::HashMap<String, i32> = raw
        .into_iter()
        .filter_map(|(id, m)| m.difficulty.map(|d| (id, d)))
        .collect();

    tracing::info!("cached difficulty for {} atcoder problems", map.len());
    *model_cache().lock().unwrap_or_else(|p| p.into_inner()) = Some((Instant::now(), map.clone()));

    Ok(map)
}

// accepted submissions since `from_second`, following kenkoooo's paging
//
// the endpoint returns at most 500 per call and pages by timestamp. only a
// year is ever needed for the charts, which keeps this to a few calls even for
// a very active member.
pub async fn fetch_submissions_since(
    handle: &str,
    from_second: i64,
) -> Result<Vec<AtcoderSubmission>, AppError> {
    const PAGE: usize = 500;
    const MAX_PAGES: usize = 12;

    let mut all: Vec<AtcoderSubmission> = Vec::new();
    let mut cursor = from_second;

    for _ in 0..MAX_PAGES {
        let url = format!(
            "{}/atcoder-api/v3/user/submissions?user={}&from_second={}",
            KENKOOOO_BASE, handle, cursor
        );
        let response = http::codeforces().get(&url).send().await.map_err(|e| {
            tracing::error!("submissions request failed for {}: {}", handle, e);
            AppError::InternalError("Could not reach AtCoder Problems".to_string())
        })?;

        let page = response
            .json::<Vec<AtcoderSubmission>>()
            .await
            .map_err(|e| {
                tracing::error!("could not parse submissions for {}: {}", handle, e);
                AppError::InternalError("Failed to parse AtCoder Problems response".to_string())
            })?;

        let count = page.len();
        // advance past the last row; +1 avoids re-reading it forever
        if let Some(last) = page.last() {
            cursor = last.epoch_second + 1;
        }
        all.extend(page);

        if count < PAGE {
            break;
        }
        tokio::time::sleep(POLITE_DELAY).await;
    }

    Ok(all)
}

// atcoder colour bands, the equivalent of codeforces rank titles
pub fn rank_title(rating: i32) -> &'static str {
    match rating {
        r if r >= 2800 => "Red",
        r if r >= 2400 => "Orange",
        r if r >= 2000 => "Yellow",
        r if r >= 1600 => "Blue",
        r if r >= 1200 => "Cyan",
        r if r >= 800 => "Green",
        r if r >= 400 => "Brown",
        _ => "Gray",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_titles_follow_the_atcoder_colour_bands() {
        assert_eq!(rank_title(0), "Gray");
        assert_eq!(rank_title(399), "Gray");
        assert_eq!(rank_title(400), "Brown");
        assert_eq!(rank_title(1199), "Green");
        assert_eq!(rank_title(1200), "Cyan");
        assert_eq!(rank_title(2000), "Yellow");
        assert_eq!(rank_title(2800), "Red");
    }

    // the shapes below are copied from live responses; if either upstream
    // changes its field names these stop deserialising, which is the point
    #[test]
    fn history_entries_deserialise_from_the_official_shape() {
        let raw = r#"[{"IsRated":true,"Place":2,"OldRating":0,"NewRating":2720,
            "Performance":3920,"InnerPerformance":3920,
            "ContestScreenName":"agc004.contest.atcoder.jp",
            "ContestName":"AtCoder Grand Contest 004","ContestNameEn":"",
            "EndTime":"2016-09-04T22:50:00+09:00"}]"#;
        let parsed: Vec<AtcoderHistoryEntry> = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].new_rating, 2720);
        assert_eq!(parsed[0].place, 2);
        assert!(parsed[0].is_rated);
        assert_eq!(parsed[0].contest_screen_name, "agc004.contest.atcoder.jp");
    }

    #[test]
    fn rate_change_parses_every_shape_atcoder_uses() {
        // taken from the live contest list
        assert_eq!(rating_range("All"), Some((i32::MIN, i32::MAX)));
        assert_eq!(rating_range("~ 1999"), Some((i32::MIN, 1999)));
        assert_eq!(rating_range("1200 ~"), Some((1200, i32::MAX)));
        assert_eq!(rating_range("1200 ~ 2799"), Some((1200, 2799)));
        // "-" marks Daily Training and Weekday Beta, which are not contests
        assert_eq!(rating_range("-"), None);
        assert_eq!(rating_range(""), None);
    }

    #[test]
    fn eligibility_uses_the_published_band() {
        // a 3797 red player cannot enter an ABC rated
        assert!(!is_eligible("~ 1999", Some(3797)));
        assert!(is_eligible("~ 1999", Some(1500)));
        // ARC tops out below them too
        assert!(!is_eligible("1200 ~ 2799", Some(3797)));
        assert!(is_eligible("1200 ~ 2799", Some(2000)));
        // a beginner is below the floor
        assert!(!is_eligible("1200 ~ 2799", Some(800)));
        // heuristic contests are open to everyone
        assert!(is_eligible("All", Some(3797)));
        // an unrated round is never something you can be said to have missed
        assert!(!is_eligible("-", Some(1500)));
        // unknown rating: do not accuse them of skipping it
        assert!(is_eligible("~ 1999", None));
    }

    #[test]
    fn contest_list_entries_deserialise() {
        let raw = r#"[{"id":"abc321","start_epoch_second":1695996000,
            "duration_second":6000,"title":"AtCoder Beginner Contest 321",
            "rate_change":" ~ 1999"}]"#;
        let parsed: Vec<AtcoderContest> = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed[0].id, "abc321");
        assert_eq!(parsed[0].start_epoch_second, 1695996000);
    }

    #[test]
    fn user_info_deserialises() {
        let raw = r#"{"user_id":"tourist","accepted_count":1057,
            "accepted_count_rank":4421,"rated_point_sum":688543.0,
            "rated_point_sum_rank":677}"#;
        let parsed: AtcoderUserInfo = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.accepted_count, 1057);
    }
}
