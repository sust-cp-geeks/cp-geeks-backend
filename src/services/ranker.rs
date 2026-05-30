use std::collections::HashMap;

use crate::errors::AppError;
use crate::models::ranker::{
    ContestResult, RankedParticipant, RankerRequest, RankerResponse, VjudgeContest,
};
use crate::services::vjudge;

// per-problem stats for a single participant in a single contest
struct ProblemAttempt {
    solved: bool,
    wrong_attempts: i64,
    solve_time_secs: i64,
}

// processes a single vjudge contest into per-user scores
fn process_contest(
    contest: &VjudgeContest,
    weights: &Option<Vec<f64>>,
) -> HashMap<String, ContestResult> {
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

    for sub in &contest.submissions {
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

        // skip upsolves — only count submissions during the contest
        // vjudge api: submission time is in SECONDS, contest.length is in MILLISECONDS
        let contest_duration_secs = contest.length / 1000;
        if time_secs > contest_duration_secs {
            continue;
        }

        if prob_idx < 0 {
            continue;
        }
        let prob_idx = prob_idx as usize;

        let problems = user_problems
            .entry(uid.clone())
            .or_insert_with(|| {
                (0..num_problems)
                    .map(|_| ProblemAttempt {
                        solved: false,
                        wrong_attempts: 0,
                        solve_time_secs: 0,
                    })
                    .collect()
            });

        if prob_idx >= problems.len() {
            continue;
        }

        // skip if already solved
        if problems[prob_idx].solved {
            continue;
        }

        if verdict == 1 {
            problems[prob_idx].solved = true;
            problems[prob_idx].solve_time_secs = time_secs;
        } else {
            problems[prob_idx].wrong_attempts += 1;
        }
    }

    // compute scores for each user
    let mut results: HashMap<String, ContestResult> = HashMap::new();

    for (uid, problems) in &user_problems {
        let handle = match id_to_handle.get(uid) {
            Some(h) => h.clone(),
            None => continue,
        };

        let mut solved_count = 0usize;
        let mut penalty = 0i64;
        let mut score = 0.0f64;

        for (i, p) in problems.iter().enumerate() {
            if p.solved {
                solved_count += 1;

                // icpc penalty: solve_time_minutes + 20 * wrong_attempts
                let time_min = p.solve_time_secs / 60;
                penalty += time_min + 20 * p.wrong_attempts;

                // weighted score (default weight = 1.0)
                let weight = weights
                    .as_ref()
                    .and_then(|w| w.get(i))
                    .copied()
                    .unwrap_or(1.0);
                score += weight;
            }
        }

        results.insert(
            handle,
            ContestResult {
                contest_name: contest.title.clone(),
                solved: solved_count,
                penalty,
                score,
            },
        );
    }

    results
}

// main ranking function: fetches all contests, merges, ranks
pub async fn analyze(request: &RankerRequest) -> Result<RankerResponse, AppError> {
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

    // merge all participants across all contests
    // key = vjudge handle (lowercase for dedup), value = aggregated stats
    let mut merged: HashMap<String, (String, f64, usize, i64, Vec<ContestResult>)> =
        HashMap::new();

    for (i, contest) in contests.iter().enumerate() {
        let weights = request
            .problem_weights
            .as_ref()
            .and_then(|pw| pw.get(i))
            .cloned()
            .flatten();

        let contest_results = process_contest(contest, &weights);

        for (handle, result) in contest_results {
            let key = handle.to_lowercase();
            let entry = merged.entry(key).or_insert_with(|| {
                (handle.clone(), 0.0, 0, 0, Vec::new())
            });

            entry.1 += result.score;
            entry.2 += result.solved;
            entry.3 += result.penalty;
            entry.4.push(result);
        }
    }

    // sort: total_score desc, then penalty asc, then solved desc
    let mut participants: Vec<_> = merged.into_values().collect();
    participants.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.3.cmp(&b.3))
            .then(b.2.cmp(&a.2))
    });

    // assign ranks (equal score + penalty = same rank)
    let mut rankings: Vec<RankedParticipant> = Vec::new();
    let mut current_rank = 1;

    for (i, (handle, score, solved, penalty, details)) in participants.into_iter().enumerate() {
        if i > 0 {
            let prev = &rankings[i - 1];
            if (score - prev.total_score).abs() > f64::EPSILON
                || penalty != prev.total_penalty
            {
                current_rank = (i + 1) as i32;
            }
        }

        rankings.push(RankedParticipant {
            rank: current_rank,
            handle,
            total_score: score,
            problems_solved: solved,
            total_penalty: penalty,
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
