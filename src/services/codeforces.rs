use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::errors::AppError;
use crate::models::codeforces::{
    AttendanceSummary, CfApiResponse, CfContestListItem, CfProfileStats, CfRatingChange,
    CfSubmission, CfUserInfo, ContestAttendance, ContestPerformance, SolveCountPeriod, SolveCounts,
};
use crate::services::http;

const CF_API_BASE: &str = "https://codeforces.com/api";

// difficulty bucket labels (500 gap)
const BUCKETS: &[(&str, i32, i32)] = &[
    // starts at 500 because codeforces problems begin at 800 — a 0-499 row was
    // permanently empty. the range still opens at 0 so that a lower-rated
    // problem, if one ever appeared, is counted rather than dropped: `total` is
    // the sum of these buckets, so anything unbucketed would vanish from it too.
    ("500-999", 0, 999),
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

    let response = http::codeforces().get(&url).send().await.map_err(|e| {
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
        let msg = body
            .comment
            .unwrap_or_else(|| "Handle not found".to_string());
        return Err(AppError::BadRequest(format!(
            "Invalid Codeforces handle: {}",
            msg
        )));
    }

    body.result
        .and_then(|mut users| {
            if users.is_empty() {
                None
            } else {
                Some(users.remove(0))
            }
        })
        .ok_or_else(|| AppError::BadRequest("Codeforces handle not found".to_string()))
}

// fetches all submissions for a handle from the cf api
async fn fetch_submissions(handle: &str) -> Result<Vec<CfSubmission>, AppError> {
    let url = format!("{}/user.status?handle={}", CF_API_BASE, handle);

    let response = http::codeforces().get(&url).send().await.map_err(|e| {
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

    let response = http::codeforces().get(&url).send().await.map_err(|e| {
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
fn count_solves_by_bucket(submissions: &[CfSubmission], after_timestamp: i64) -> SolveCountPeriod {
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
// the newest N contests are the only ones worth showing — a member cannot
// meaningfully have missed something from before they joined
const MAX_ATTENDANCE_ROWS: usize = 100;

// contest.list is the same 400 KB for every user and changes about daily, so it
// is fetched once and shared rather than per profile view
const CONTEST_LIST_TTL: Duration = Duration::from_secs(6 * 60 * 60);
type ContestListCache = Mutex<Option<(Instant, Vec<CfContestListItem>)>>;
static CONTEST_LIST_CACHE: OnceLock<ContestListCache> = OnceLock::new();

fn contest_cache() -> &'static ContestListCache {
    CONTEST_LIST_CACHE.get_or_init(|| Mutex::new(None))
}

// every finished contest on codeforces, newest first
async fn fetch_contest_list() -> Result<Vec<CfContestListItem>, AppError> {
    {
        let cached = contest_cache().lock().unwrap_or_else(|p| p.into_inner());
        if let Some((fetched_at, list)) = cached.as_ref() {
            if fetched_at.elapsed() < CONTEST_LIST_TTL {
                return Ok(list.clone());
            }
        }
    }

    let url = format!("{}/contest.list?gym=false", CF_API_BASE);
    let response = http::codeforces().get(&url).send().await.map_err(|e| {
        tracing::error!("failed to reach codeforces contest.list: {}", e);
        AppError::InternalError("Could not reach Codeforces API".to_string())
    })?;

    let body = response
        .json::<CfApiResponse<Vec<CfContestListItem>>>()
        .await
        .map_err(|e| {
            tracing::error!("failed to parse contest.list: {}", e);
            AppError::InternalError("Failed to parse Codeforces response".to_string())
        })?;

    if body.status != "OK" {
        return Err(AppError::InternalError(
            "Codeforces rejected the contest list request".to_string(),
        ));
    }

    let mut list: Vec<CfContestListItem> = body
        .result
        .unwrap_or_default()
        .into_iter()
        .filter(|c| c.phase == "FINISHED" && c.start_time_seconds.is_some())
        .collect();
    // newest first
    list.sort_by_key(|c| std::cmp::Reverse(c.start_time_seconds.unwrap_or(0)));

    tracing::info!("cached {} finished codeforces contests", list.len());
    *contest_cache().lock().unwrap_or_else(|p| p.into_inner()) =
        Some((Instant::now(), list.clone()));

    Ok(list)
}

// who a round is open to, read off the contest name
//
// codeforces encodes this only in the title, so this is string matching by
// necessity. the thresholds below are the current rules; they have shifted over
// the years, so an old contest may be judged by today's cutoff. participation
// always overrides this, which keeps a parsing miss from hiding a real entry.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DivisionRule {
    Open,
    Div1,
    Div2,
    Div3,
    Div4,
    // unrated rounds never appear in user.rating, so we cannot tell whether
    // they turned up — better to exclude than to accuse them of skipping it
    Unrated,
}

// matches "Div. 2", "Div.2" and "Div 2"
fn mentions_division(name: &str, digit: char) -> bool {
    ["div. ", "div.", "div "]
        .iter()
        .any(|p| name.contains(&format!("{}{}", p, digit)))
}

fn division_rule(name: &str) -> DivisionRule {
    let n = name.to_lowercase();

    if n.contains("unrated") {
        return DivisionRule::Unrated;
    }
    // combined rounds name both divisions, so this has to come first
    if mentions_division(&n, '1') && mentions_division(&n, '2') {
        return DivisionRule::Open;
    }
    if mentions_division(&n, '1') {
        return DivisionRule::Div1;
    }
    // educational rounds read "(Rated for Div. 2)", which lands here
    if mentions_division(&n, '2') {
        return DivisionRule::Div2;
    }
    if mentions_division(&n, '3') {
        return DivisionRule::Div3;
    }
    if mentions_division(&n, '4') {
        return DivisionRule::Div4;
    }
    // global rounds, April Fools, Kotlin Heroes and the like are open to all
    DivisionRule::Open
}

// their rating on the day of a given contest, from the surrounding history
fn rating_at(timestamp: i64, history: &[CfRatingChange]) -> Option<i32> {
    history
        .iter()
        .filter(|rc| rc.rating_update_time_seconds <= timestamp)
        .max_by_key(|rc| rc.rating_update_time_seconds)
        .map(|rc| rc.new_rating)
        // before their first rated contest they still had a starting rating
        .or_else(|| {
            history
                .iter()
                .min_by_key(|rc| rc.rating_update_time_seconds)
                .map(|rc| rc.old_rating)
        })
}

fn is_eligible(rule: DivisionRule, rating: Option<i32>) -> bool {
    let rating = match rating {
        Some(r) => r,
        None => return true,
    };
    match rule {
        DivisionRule::Unrated => false,
        DivisionRule::Open => true,
        DivisionRule::Div1 => rating >= 1900,
        DivisionRule::Div2 => rating < 2100,
        DivisionRule::Div3 => rating < 1600,
        DivisionRule::Div4 => rating < 1400,
    }
}

// pairs the full contest list against what they actually competed in
//
// only contests from their first rated appearance onward are considered — a
// student who joined this year has not "missed" a contest from 2015
fn build_attendance(
    contests: &[CfContestListItem],
    rating_history: &[CfRatingChange],
) -> (Vec<ContestAttendance>, AttendanceSummary) {
    let by_contest: HashMap<i32, &CfRatingChange> = rating_history
        .iter()
        .map(|rc| (rc.contest_id, rc))
        .collect();

    // their first rated contest marks the start of a meaningful window
    let joined_at = rating_history
        .iter()
        .map(|rc| rc.rating_update_time_seconds)
        .min();

    let rows: Vec<ContestAttendance> = contests
        .iter()
        .filter(|c| match (joined_at, c.start_time_seconds) {
            // allow a day of slack: the rating update lands after the contest starts
            (Some(joined), Some(start)) => start >= joined - 86_400,
            _ => false,
        })
        .take(MAX_ATTENDANCE_ROWS)
        .map(|c| {
            let entry = by_contest.get(&c.id);
            let start = c.start_time_seconds.unwrap_or(0);
            // turning up proves eligibility, whatever the name parsing decided
            let eligible = entry.is_some()
                || is_eligible(division_rule(&c.name), rating_at(start, rating_history));
            let date = c
                .start_time_seconds
                .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
                .map(|d| d.format("%Y-%m-%dT%H:%M:%S").to_string())
                .unwrap_or_default();

            ContestAttendance {
                contest_id: c.id,
                contest_name: c.name.clone(),
                date,
                participated: entry.is_some(),
                eligible,
                rank: entry.map(|e| e.rank),
                old_rating: entry.map(|e| e.old_rating),
                new_rating: entry.map(|e| e.new_rating),
                rating_change: entry.map(|e| e.new_rating - e.old_rating),
            }
        })
        .collect();

    let participated = rows.iter().filter(|r| r.participated).count();
    let ineligible = rows.iter().filter(|r| !r.eligible).count();
    let summary = AttendanceSummary {
        total_contests: rows.len(),
        participated,
        // only a contest they could have entered counts as missed
        missed: rows.len() - participated - ineligible,
        ineligible,
    };

    (rows, summary)
}

pub async fn build_profile_stats(handle: &str) -> Result<CfProfileStats, AppError> {
    // fetch in parallel for speed — the contest list is usually a cache hit
    let (user_info, submissions, rating_history, contest_list) = tokio::try_join!(
        validate_handle(handle),
        fetch_submissions(handle),
        fetch_rating_history(handle),
        fetch_contest_list(),
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

    let (contest_attendance, attendance_summary) = build_attendance(&contest_list, &rating_history);

    Ok(CfProfileStats {
        codeforces_handle: handle.to_string(),
        current_rating: user_info.rating,
        current_rank: user_info.rank,
        max_rating: user_info.max_rating,
        max_rank: user_info.max_rank,
        solve_counts,
        recent_contests,
        contest_attendance,
        attendance_summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rated(contest_id: i32, at: i64, old: i32, new: i32) -> CfRatingChange {
        CfRatingChange {
            contest_id,
            contest_name: format!("Contest {contest_id}"),
            handle: "tester".to_string(),
            rank: 100,
            old_rating: old,
            new_rating: new,
            rating_update_time_seconds: at,
        }
    }

    fn listed(id: i32, name: &str, at: i64) -> CfContestListItem {
        CfContestListItem {
            id,
            name: name.to_string(),
            phase: "FINISHED".to_string(),
            start_time_seconds: Some(at),
        }
    }

    #[test]
    fn combined_rounds_are_open_to_everyone() {
        // must be checked before Div. 1, or a pupil is wrongly excluded
        assert_eq!(
            division_rule("Spectral::Cup 2026 Round 3 (Codeforces Round 1110, Div. 1 + Div. 2)"),
            DivisionRule::Open
        );
    }

    #[test]
    fn divisions_are_read_from_the_contest_name() {
        assert_eq!(
            division_rule("Codeforces Round 1116 (Div. 1)"),
            DivisionRule::Div1
        );
        assert_eq!(
            division_rule("Codeforces Round 1118 (Div. 2)"),
            DivisionRule::Div2
        );
        assert_eq!(
            division_rule("Codeforces Round 1114 (Div. 3)"),
            DivisionRule::Div3
        );
        assert_eq!(
            division_rule("Codeforces Round 1090 (Div. 4)"),
            DivisionRule::Div4
        );
        // educational rounds say "Rated for Div. 2"
        assert_eq!(
            division_rule("Educational Codeforces Round 193 (Rated for Div. 2)"),
            DivisionRule::Div2
        );
        // no marker at all — global rounds, april fools, kotlin heroes
        assert_eq!(division_rule("Hello 2026"), DivisionRule::Open);
        assert_eq!(
            division_rule("Kotlin Heroes: Episode 14"),
            DivisionRule::Open
        );
    }

    #[test]
    fn unrated_rounds_are_not_counted_against_anyone() {
        // participation in an unrated round never reaches user.rating, so we
        // cannot tell whether they showed up
        assert_eq!(
            division_rule("2025 ICPC Asia Taichung Regional Contest (Unrated, Online Mirror)"),
            DivisionRule::Unrated
        );
        assert!(!is_eligible(DivisionRule::Unrated, Some(1500)));
    }

    #[test]
    fn eligibility_follows_the_rating_thresholds() {
        assert!(!is_eligible(DivisionRule::Div1, Some(1899)));
        assert!(is_eligible(DivisionRule::Div1, Some(1900)));
        assert!(is_eligible(DivisionRule::Div2, Some(2099)));
        assert!(!is_eligible(DivisionRule::Div2, Some(2100)));
        assert!(is_eligible(DivisionRule::Div3, Some(1599)));
        assert!(!is_eligible(DivisionRule::Div3, Some(1600)));
        assert!(is_eligible(DivisionRule::Div4, Some(1399)));
        assert!(!is_eligible(DivisionRule::Div4, Some(1400)));
        assert!(is_eligible(DivisionRule::Open, Some(0)));
    }

    #[test]
    fn rating_is_taken_from_the_day_of_the_contest_not_today() {
        let history = vec![rated(1, T0, 1200, 1300), rated(2, T0 + 7 * DAY, 1300, 1950)];
        // before anything happened — their starting rating
        assert_eq!(rating_at(T0 - DAY, &history), Some(1200));
        // between the two
        assert_eq!(rating_at(T0 + DAY, &history), Some(1300));
        // after both
        assert_eq!(rating_at(T0 + 30 * DAY, &history), Some(1950));
    }

    // real epoch seconds: the window allows a day of slack because a rating
    // update lands after its contest starts, so toy timestamps a few seconds
    // apart would all fall inside it
    const T0: i64 = 1_700_000_000;
    const DAY: i64 = 86_400;

    #[test]
    fn window_starts_at_their_first_rated_contest() {
        // a member who joined this year has not "missed" a 2015 round
        let history = vec![rated(2, T0, 1200, 1250)];
        let contests = vec![
            listed(3, "Codeforces Round C (Div. 2)", T0 + 7 * DAY),
            listed(2, "Codeforces Round B (Div. 2)", T0),
            listed(1, "Codeforces Round A (Div. 2)", T0 - 30 * DAY), // before they joined
        ];
        let (rows, summary) = build_attendance(&contests, &history);
        assert_eq!(rows.len(), 2, "the pre-join contest must be excluded");
        assert!(rows.iter().all(|r| r.contest_id != 1));
        assert_eq!(summary.total_contests, 2);
    }

    #[test]
    fn attendance_splits_into_attended_missed_and_ineligible() {
        let history = vec![rated(10, T0, 1200, 1282)];
        let contests = vec![
            listed(30, "Codeforces Round 1116 (Div. 1)", T0 + 14 * DAY), // 1282 cannot enter
            listed(20, "Codeforces Round 1118 (Div. 2)", T0 + 7 * DAY),  // eligible, skipped
            listed(10, "Codeforces Round 1100 (Div. 2)", T0),            // attended
        ];
        let (rows, summary) = build_attendance(&contests, &history);

        assert_eq!(summary.participated, 1);
        assert_eq!(summary.missed, 1);
        assert_eq!(summary.ineligible, 1);
        assert_eq!(summary.total_contests, 3);
        // missed must never include contests they could not enter
        assert_eq!(
            summary.participated + summary.missed + summary.ineligible,
            summary.total_contests
        );

        let attended = rows.iter().find(|r| r.contest_id == 10).unwrap();
        assert!(attended.participated && attended.eligible);
        assert_eq!(attended.rating_change, Some(82));

        let missed = rows.iter().find(|r| r.contest_id == 20).unwrap();
        assert!(!missed.participated && missed.eligible);
        assert_eq!(missed.rank, None, "missed rows carry no rank");

        let div1 = rows.iter().find(|r| r.contest_id == 30).unwrap();
        assert!(!div1.participated && !div1.eligible);
    }

    #[test]
    fn taking_part_overrides_the_name_parser() {
        // if the parser misreads a title, a real entry must still show as
        // attended rather than being hidden as ineligible
        let history = vec![rated(1, T0, 1200, 1210)];
        let contests = vec![listed(1, "Some Round (Unrated, Online Mirror)", T0)];
        let (rows, summary) = build_attendance(&contests, &history);
        assert!(rows[0].participated);
        assert!(rows[0].eligible, "participation implies eligibility");
        assert_eq!(summary.ineligible, 0);
    }

    #[test]
    fn a_member_who_never_competed_gets_an_empty_timeline() {
        let (rows, summary) = build_attendance(&[listed(1, "Round (Div. 2)", T0)], &[]);
        assert!(rows.is_empty());
        assert_eq!(summary.total_contests, 0);
    }
}
