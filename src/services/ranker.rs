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
        if let Some(obj) = info.as_object() {
            if let Some(handle) = obj.get("name").and_then(|v| v.as_str()) {
                id_to_handle.insert(uid.clone(), handle.to_string());
            }
        } else if let Some(arr) = info.as_array() {
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

    // vjudge api: contest.length is in milliseconds, submission times are in seconds
    let contest_duration_secs = contest.length / 1000;

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

        let entry = user_participated.entry(uid.clone()).or_insert(false);
        if time_secs <= contest_duration_secs {
            *entry = true;
        }

        let problems = user_problems.entry(uid.clone()).or_insert_with(|| {
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
            let mut total_solve_time_secs = 0i64;
            let mut total_wrong_attempts_for_solved = 0i64;

            for (i, p) in problems.iter().enumerate() {
                if p.solved_during {
                    solved_count += 1;
                    total_solve_time_secs += p.solve_time_secs;
                    total_wrong_attempts_for_solved += p.wrong_attempts_during;

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
            penalty = (total_solve_time_secs / 60) + (20 * total_wrong_attempts_for_solved);
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

// One person's totals across every contest in the run. This was an eight-field
// tuple, which meant the sort read `b.3.cmp(&a.3)` with a comment beside it
// explaining that 3 meant solved — a wrong index there would have been silent
// and would have reordered the standings.
// Decides the standings. Solved first, then penalty, then upsolved, and handle
// last so that two genuinely tied rows do not swap places between runs — a
// leaderboard that reorders itself on refresh looks broken even when it is not.
fn standings_order(a: &Tally, b: &Tally) -> std::cmp::Ordering {
    b.solved
        .cmp(&a.solved)
        .then(a.penalty.cmp(&b.penalty))
        .then(b.upsolved.cmp(&a.upsolved))
        .then_with(|| a.handle.to_lowercase().cmp(&b.handle.to_lowercase()))
}

struct Tally {
    handle: String,
    real_name: String,
    score: f64,
    solved: usize,
    upsolved: usize,
    penalty: i64,
    contests_participated: usize,
    details: Vec<ContestResult>,
}

pub async fn analyze(
    pool: &sqlx::PgPool,
    request: &RankerRequest,
) -> Result<RankerResponse, AppError> {
    if request.contest_ids.is_empty() {
        return Err(AppError::BadRequest(
            "At least one contest ID is required".to_string(),
        ));
    }

    // build vjudge_handle -> real_name map from the database
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT LOWER(vjudge_handle), name FROM users WHERE vjudge_handle IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    let handle_to_name: HashMap<String, String> = rows.into_iter().collect();

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

    // Collect all merged constituent handles into a HashSet to filter them out of unique handles
    let mut merged_constituent_handles = std::collections::HashSet::new();
    if let Some(merges) = &request.merged_handles {
        for merge in merges {
            for h in &merge.handles {
                merged_constituent_handles.insert(h.to_lowercase());
            }
        }
    }

    // merge all participants across all contests
    // key = vjudge handle (lowercase for dedup), value = original handle
    let mut unique_handles: HashMap<String, String> = HashMap::new();
    for results in &contest_results_list {
        for (lowercase_handle, (original_handle, _)) in results {
            if !merged_constituent_handles.contains(lowercase_handle) {
                unique_handles.insert(lowercase_handle.clone(), original_handle.clone());
            }
        }
    }

    let mut participants: Vec<Tally> = Vec::new();

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

        let real_name = handle_to_name
            .get(lowercase_handle)
            .cloned()
            .unwrap_or_else(|| "unregistered".to_string());

        participants.push(Tally {
            handle: original_handle.clone(),
            real_name,
            score: total_score,
            solved: total_solved,
            upsolved: total_upsolved,
            penalty: total_penalty,
            contests_participated,
            details,
        });
    }

    // Now process the merged handles
    if let Some(merges) = &request.merged_handles {
        for merge in merges {
            let mut total_score = 0.0;
            let mut total_solved = 0;
            let mut total_upsolved = 0;
            let mut total_penalty = 0;
            let mut contests_participated = 0;
            let mut details = Vec::new();

            for (i, contest) in contests.iter().enumerate() {
                let results = &contest_results_list[i];
                let mut contest_solved = 0;
                let mut contest_upsolved = 0;
                let mut contest_penalty = 0;
                let mut contest_score = 0.0;
                let mut contest_participated = false;

                for h in &merge.handles {
                    let lowercase_h = h.to_lowercase();
                    if let Some((_, res)) = results.get(&lowercase_h) {
                        contest_solved += res.solved;
                        contest_upsolved += res.upsolved;
                        contest_penalty += res.penalty;
                        contest_score += res.score;
                        if res.participated {
                            contest_participated = true;
                        }
                    }
                }

                let contest_title = request
                    .custom_titles
                    .as_ref()
                    .and_then(|ct| ct.get(i))
                    .filter(|t| !t.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| contest.title.clone());

                details.push(ContestResult {
                    contest_name: contest_title,
                    solved: contest_solved,
                    upsolved: contest_upsolved,
                    penalty: contest_penalty,
                    score: contest_score,
                    participated: contest_participated,
                });

                total_score += contest_score;
                total_solved += contest_solved;
                total_upsolved += contest_upsolved;
                total_penalty += contest_penalty;
                if contest_participated {
                    contests_participated += 1;
                }
            }

            // for merged handles, display comma-separated vjudge handles
            let merged_handle_display = merge.handles.join(",");

            participants.push(Tally {
                handle: merged_handle_display,
                real_name: merge.name.clone(),
                score: total_score,
                solved: total_solved,
                upsolved: total_upsolved,
                penalty: total_penalty,
                contests_participated,
                details,
            });
        }
    }

    // sort: total solved desc, then penalty asc, then upsolved desc
    // handle asc at the end so tied rows don't shuffle around between runs
    participants.sort_by(standings_order);

    // assign ranks (equal solved + penalty + upsolved = same rank)
    let mut rankings: Vec<RankedParticipant> = Vec::new();
    let mut current_rank = 1;

    for (
        i,
        Tally {
            handle,
            real_name,
            score,
            solved,
            upsolved,
            penalty,
            contests_participated,
            details,
        },
    ) in participants.into_iter().enumerate()
    {
        if i > 0 {
            let prev = &rankings[i - 1];
            if solved != prev.problems_solved
                || penalty != prev.total_penalty
                || upsolved != prev.total_upsolved
            {
                current_rank = (i + 1) as i32;
            }
        }

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

#[cfg(test)]
mod tests {
    use super::{standings_order, Tally};
    use std::cmp::Ordering;

    fn tally(handle: &str, solved: usize, penalty: i64, upsolved: usize) -> Tally {
        Tally {
            handle: handle.to_string(),
            real_name: handle.to_string(),
            score: 0.0,
            solved,
            upsolved,
            penalty,
            contests_participated: 1,
            details: Vec::new(),
        }
    }

    #[test]
    fn more_solved_wins_regardless_of_penalty() {
        let more = tally("a", 5, 9999, 0);
        let fewer = tally("b", 4, 0, 0);
        assert_eq!(standings_order(&more, &fewer), Ordering::Less);
    }

    #[test]
    fn equal_solved_is_broken_by_lower_penalty() {
        let quick = tally("a", 5, 100, 0);
        let slow = tally("b", 5, 200, 0);
        assert_eq!(standings_order(&quick, &slow), Ordering::Less);
    }

    #[test]
    fn upsolving_breaks_a_tie_on_solved_and_penalty() {
        let upsolver = tally("a", 5, 100, 3);
        let neither = tally("b", 5, 100, 0);
        assert_eq!(standings_order(&upsolver, &neither), Ordering::Less);
    }

    // without this a genuinely tied pair could swap places between two runs of
    // the same input, which reads as a bug to anyone refreshing the page
    #[test]
    fn a_real_tie_falls_back_to_handle_so_the_order_is_stable() {
        let x = tally("alice", 5, 100, 2);
        let y = tally("bob", 5, 100, 2);
        assert_eq!(standings_order(&x, &y), Ordering::Less);
        assert_eq!(standings_order(&y, &x), Ordering::Greater);
    }

    #[test]
    fn handle_comparison_ignores_case() {
        let upper = tally("Zoe", 5, 100, 0);
        let lower = tally("adam", 5, 100, 0);
        assert_eq!(standings_order(&upper, &lower), Ordering::Greater);
    }

    #[test]
    fn sorting_a_field_puts_them_in_the_expected_order() {
        let mut field = [
            tally("carol", 4, 50, 0),
            tally("alice", 5, 300, 1),
            tally("dave", 5, 300, 0),
            tally("bob", 5, 100, 0),
        ];
        field.sort_by(standings_order);
        let order: Vec<&str> = field.iter().map(|t| t.handle.as_str()).collect();
        // bob leads on penalty; alice beats dave on upsolved; carol solved fewer
        assert_eq!(order, vec!["bob", "alice", "dave", "carol"]);
    }
}
