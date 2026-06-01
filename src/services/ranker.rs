use std::collections::HashMap;

use crate::errors::AppError;
use crate::models::ranker::{
    ContestResult, RankedParticipant, RankerRequest, RankerResponse, VjudgeContest,
};
use crate::services::vjudge;

// per-problem stats for a single participant in a single contest
struct ProblemAttempt {
    solved_during: bool,
    solved_after: bool,
    wrong_attempts_during: i64,
    solve_time_secs: i64,
}

// processes a single vjudge contest into per-user scores
fn process_contest(
    contest: &VjudgeContest,
    weights: &Option<Vec<f64>>,
    title: &str,
) -> HashMap<String, (String, ContestResult)> {
    // build user_id -> handle mapping
    let mut id_to_handle: HashMap<String, String> = HashMap::new();
    for (uid, info) in &contest.participants {
        if let Some(arr) = info.as_array() {
            if let Some(handle) = arr.first().and_then(|v| v.as_str()) {
                id_to_handle.insert(uid.clone(), handle.to_string());
            }
        }
    }

    // find the total number of problems by scanning all submissions
    let max_prob_idx = contest
        .submissions
        .iter()
        .filter_map(|s| s.get(1).and_then(|v| v.as_i64()))
        .max()
        .unwrap_or(0) as usize;
    let num_problems = max_prob_idx + 1;

    // build per-user, per-problem attempt tracking
    // submission format: [user_id, problem_index, verdict, time_ms]
    let mut user_problems: HashMap<String, Vec<ProblemAttempt>> = HashMap::new();
    let mut user_participated: HashMap<String, bool> = HashMap::new();

    // Initialize tracking for all registered participants
    for uid in id_to_handle.keys() {
        user_problems.insert(
            uid.clone(),
            (0..num_problems)
                .map(|_| ProblemAttempt {
                    solved_during: false,
                    solved_after: false,
                    wrong_attempts_during: 0,
                    solve_time_secs: 0,
                })
                .collect(),
        );
        user_participated.insert(uid.clone(), false);
    }

    // Sort submissions by time ascending to process chronologically
    let mut sorted_submissions = contest.submissions.clone();
    sorted_submissions.sort_by_key(|s| s.get(3).and_then(|v| v.as_i64()).unwrap_or(0));

    for sub in &sorted_submissions {
        let uid = match sub.first() {
            Some(v) => {
                if let Some(n) = v.as_i64() {
                    n.to_string()
                } else if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    continue;
                }
            }
            None => continue,
        };
        let prob_idx = sub.get(1).and_then(|v| v.as_i64()).unwrap_or(-1);
        let verdict = sub.get(2).and_then(|v| v.as_i64()).unwrap_or(0);
        let time_secs = sub.get(3).and_then(|v| v.as_i64()).unwrap_or(0);

        if prob_idx < 0 {
            continue;
        }
        let prob_idx = prob_idx as usize;

        let contest_duration_secs = contest.length / 1000;
        let entry = user_participated.entry(uid.clone()).or_insert(false);
        if time_secs <= contest_duration_secs {
            *entry = true;
        }

        let problems = user_problems
            .entry(uid.clone())
            .or_insert_with(|| {
                (0..num_problems)
                    .map(|_| ProblemAttempt {
                        solved_during: false,
                        solved_after: false,
                        wrong_attempts_during: 0,
                        solve_time_secs: 0,
                    })
                    .collect()
            });

        if prob_idx >= problems.len() {
            continue;
        }

        // skip if already solved during the contest
        if problems[prob_idx].solved_during {
            continue;
        }

        if time_secs <= contest_duration_secs {
            if verdict == 1 {
                problems[prob_idx].solved_during = true;
                problems[prob_idx].solve_time_secs = time_secs;
            } else {
                problems[prob_idx].wrong_attempts_during += 1;
            }
        } else {
            // skip if already solved after the contest
            if problems[prob_idx].solved_after {
                continue;
            }
            if verdict == 1 {
                problems[prob_idx].solved_after = true;
            }
        }
    }

    // compute scores for each user
    let mut results: HashMap<String, (String, ContestResult)> = HashMap::new();

    for (uid, problems) in &user_problems {
        let handle = match id_to_handle.get(uid) {
            Some(h) => h.clone(),
            None => continue,
        };

        let participated = user_participated.get(uid).copied().unwrap_or(false);

        let mut solved_count = 0usize;
        let mut upsolved_count = 0usize;
        let mut penalty = 0i64;
        let mut score = 0.0f64;

        if participated {
            for (i, p) in problems.iter().enumerate() {
                if p.solved_during {
                    solved_count += 1;

                    // icpc penalty: solve_time_minutes + 20 * wrong_attempts
                    let time_min = p.solve_time_secs / 60;
                    penalty += time_min + 20 * p.wrong_attempts_during;

                    // weighted score (default weight = 1.0)
                    let weight = weights
                        .as_ref()
                        .and_then(|w| w.get(i))
                        .copied()
                        .unwrap_or(1.0);
                    score += weight;
                } else if p.solved_after {
                    upsolved_count += 1;
                }
            }
        } else {
            // Not participated: solve count 0, penalty 0.
            // Any solved problems count as upsolved.
            for p in problems {
                if p.solved_during || p.solved_after {
                    upsolved_count += 1;
                }
            }
        }

        let lowercase_handle = handle.to_lowercase();
        results.insert(
            lowercase_handle,
            (
                handle,
                ContestResult {
                    contest_name: title.to_string(),
                    solved: solved_count,
                    upsolved: upsolved_count,
                    penalty,
                    score,
                    participated,
                },
            ),
        );
    }

    results
}

// main ranking function: fetches all contests, merges, ranks
pub async fn analyze(pool: &sqlx::PgPool, request: &RankerRequest) -> Result<RankerResponse, AppError> {
    if request.contest_ids.is_empty() {
        return Err(AppError::BadRequest(
            "At least one contest ID is required".to_string(),
        ));
    }

    // fetch all contests in parallel
    let futures: Vec<_> = request
        .contest_ids
        .iter()
        .map(|id| vjudge::fetch_contest(*id))
        .collect();

    let contests: Vec<VjudgeContest> = futures::future::try_join_all(futures).await?;

    // process all contests
    let mut contest_results_list = Vec::new();
    for (i, contest) in contests.iter().enumerate() {
        let weights = request
            .problem_weights
            .as_ref()
            .and_then(|pw| pw.get(i))
            .cloned()
            .flatten();

        let contest_title = request
            .custom_titles
            .as_ref()
            .and_then(|ct| ct.get(i))
            .filter(|t| !t.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| contest.title.clone());

        let contest_results = process_contest(contest, &weights, &contest_title);
        contest_results_list.push(contest_results);
    }

    // merge all participants across all contests
    // key = vjudge handle (lowercase for dedup), value = original handle
    let mut unique_handles: HashMap<String, String> = HashMap::new();
    for results in &contest_results_list {
        for (lowercase_handle, (original_handle, _)) in results {
            unique_handles.insert(lowercase_handle.clone(), original_handle.clone());
        }
    }

    let mut participants: Vec<(String, f64, usize, usize, i64, usize, Vec<ContestResult>)> = Vec::new();

    for (lowercase_handle, original_handle) in &unique_handles {
        let mut total_score = 0.0;
        let mut total_solved = 0;
        let mut total_upsolved = 0;
        let mut total_penalty = 0;
        let mut contests_participated = 0;
        let mut details = Vec::new();

        for (i, contest) in contests.iter().enumerate() {
            let results = &contest_results_list[i];

            if let Some((_, res)) = results.get(lowercase_handle) {
                total_score += res.score;
                total_solved += res.solved;
                total_upsolved += res.upsolved;
                total_penalty += res.penalty;
                if res.participated {
                    contests_participated += 1;
                }
                details.push(res.clone());
            } else {
                let contest_title = request
                    .custom_titles
                    .as_ref()
                    .and_then(|ct| ct.get(i))
                    .filter(|t| !t.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| contest.title.clone());

                // not in this contest -> default placeholder
                details.push(ContestResult {
                    contest_name: contest_title,
                    solved: 0,
                    upsolved: 0,
                    penalty: 0,
                    score: 0.0,
                    participated: false,
                });
            }
        }

        participants.push((
            original_handle.clone(),
            total_score,
            total_solved,
            total_upsolved,
            total_penalty,
            contests_participated,
            details,
        ));
    }

    // Fetch all users to map vjudge handles to real names
    use sqlx::Row;
    let db_users = sqlx::query("SELECT name, vjudge_handle FROM users WHERE vjudge_handle IS NOT NULL")
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(format!("Failed to fetch users: {}", e)))?;

    let mut handle_to_name: HashMap<String, String> = HashMap::new();
    for row in db_users {
        let name: String = row.get("name");
        let vjudge_handle: Option<String> = row.get("vjudge_handle");
        if let Some(handle) = vjudge_handle {
            handle_to_name.insert(handle.to_lowercase(), name);
        }
    }

    // sort: total solved desc, then penalty asc, then upsolved desc
    participants.sort_by(|a, b| {
        b.2.cmp(&a.2) // solved desc
            .then(a.4.cmp(&b.4)) // penalty asc
            .then(b.3.cmp(&a.3)) // upsolved desc
    });

    // assign ranks (equal solved + penalty + upsolved = same rank)
    let mut rankings: Vec<RankedParticipant> = Vec::new();
    let mut current_rank = 1;

    for (i, (handle, score, solved, upsolved, penalty, contests_participated, details)) in participants.into_iter().enumerate() {
        if i > 0 {
            let prev = &rankings[i - 1];
            if solved != prev.problems_solved
                || penalty != prev.total_penalty
                || upsolved != prev.total_upsolved
            {
                current_rank = (i + 1) as i32;
            }
        }

        let real_name = handle_to_name.get(&handle.to_lowercase()).cloned().unwrap_or_else(|| "unregistered".to_string());

        rankings.push(RankedParticipant {
            rank: current_rank,
            real_name,
            handle,
            total_score: score,
            problems_solved: solved,
            total_upsolved: upsolved,
            total_penalty: penalty,
            contests_participated,
            contest_details: details,
        });
    }

    Ok(RankerResponse {
        title: request.title.clone(),
        contest_ids: request.contest_ids.clone(),
        total_contests: contests.len(),
        total_participants: rankings.len(),
        rankings,
    })
}
