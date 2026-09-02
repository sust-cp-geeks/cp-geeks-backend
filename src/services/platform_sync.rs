use std::time::Duration;

use sqlx::PgPool;

use std::collections::{HashMap, HashSet};

use serde_json::json;

use crate::services::atcoder;

pub const ATCODER: &str = "atcoder";
pub const CODEFORCES: &str = "codeforces";

// how often a full pass runs. a pass is one polite request pair per member, so
// with a few hundred members it takes minutes — nowhere near a request path.
const SYNC_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

// give the server a moment to finish starting before the first pass
const STARTUP_DELAY: Duration = Duration::from_secs(30);

struct Target {
    user_id: i32,
    handle: String,
}

// pulls one member's atcoder standing and writes it, replacing their history
async fn sync_one(pool: &PgPool, target: &Target) -> Result<(), String> {
    let history = atcoder::fetch_history(&target.handle)
        .await
        .map_err(|e| format!("{e:?}"))?;

    // an unrated round still reports the standing rating, so the last entry is
    // the current one either way
    let rating = history.last().map(|e| e.new_rating);
    let max_rating = history.iter().map(|e| e.new_rating).max();
    let rank = rating.map(atcoder::rank_title);

    tokio::time::sleep(atcoder::POLITE_DELAY).await;

    // a missing solve count should not throw away a good rating
    let solved = match atcoder::fetch_solved_count(&target.handle).await {
        Ok(n) => Some(n as i32),
        Err(e) => {
            tracing::warn!("solve count unavailable for {}: {:?}", target.handle, e);
            None
        }
    };

    // per-difficulty charts, the same three windows the codeforces profile
    // shows. only a year of submissions is needed, which keeps this to a few
    // pages even for an active member — and it is off the request path anyway.
    let solve_counts = match build_solve_counts(&target.handle).await {
        Ok(counts) => Some(counts),
        Err(e) => {
            tracing::warn!("solve counts unavailable for {}: {:?}", target.handle, e);
            None
        }
    };

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    sqlx::query(
        r#"INSERT INTO platform_profiles
             (user_id, platform, handle, rating, max_rating, rank_title,
              solved_count, solve_counts, synced_at, sync_error)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW(), NULL)
           ON CONFLICT (user_id, platform) DO UPDATE SET
             handle = $3, rating = $4, max_rating = $5, rank_title = $6,
             solved_count = $7, solve_counts = $8, synced_at = NOW(),
             sync_error = NULL"#,
    )
    .bind(target.user_id)
    .bind(ATCODER)
    .bind(&target.handle)
    .bind(rating)
    .bind(max_rating)
    .bind(rank)
    .bind(solved)
    .bind(solve_counts)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    // replace rather than merge: a renamed handle would otherwise leave the
    // previous account's contests behind
    sqlx::query("DELETE FROM platform_rating_history WHERE user_id = $1 AND platform = $2")
        .bind(target.user_id)
        .bind(ATCODER)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    for entry in &history {
        let ended_at = chrono::DateTime::parse_from_rfc3339(&entry.end_time)
            .map(|d| d.naive_utc())
            .ok();

        sqlx::query(
            r#"INSERT INTO platform_rating_history
                 (user_id, platform, contest_id, contest_name, old_rating,
                  new_rating, place, performance, is_rated, ended_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               ON CONFLICT (user_id, platform, contest_id) DO NOTHING"#,
        )
        .bind(target.user_id)
        .bind(ATCODER)
        .bind(&entry.contest_screen_name)
        .bind(&entry.contest_name)
        .bind(entry.old_rating)
        .bind(entry.new_rating)
        .bind(entry.place)
        .bind(entry.performance)
        .bind(entry.is_rated)
        .bind(ended_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

// unique accepted problems per difficulty band, over the three windows the
// codeforces profile uses
async fn build_solve_counts(handle: &str) -> Result<serde_json::Value, String> {
    let now = chrono::Utc::now().timestamp();
    let year_ago = now - 365 * 24 * 60 * 60;

    let subs = atcoder::fetch_submissions_since(handle, year_ago)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let difficulty = atcoder::fetch_problem_difficulties()
        .await
        .map_err(|e| format!("{e:?}"))?;

    let window = |after: i64| {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut buckets: HashMap<&str, usize> = atcoder::DIFFICULTY_BUCKETS
            .iter()
            .map(|(l, _, _)| (*l, 0))
            .collect();
        let mut total = 0usize;

        for s in &subs {
            if s.result != "AC" || s.epoch_second < after {
                continue;
            }
            // a problem solved twice counts once
            if !seen.insert(s.problem_id.as_str()) {
                continue;
            }
            // only problems with a difficulty estimate are counted, so that
            // total always equals the sum of the bars — the same rule the
            // codeforces chart follows
            if let Some(d) = difficulty.get(&s.problem_id) {
                for (label, min, max) in atcoder::DIFFICULTY_BUCKETS {
                    if d >= min && d <= max {
                        *buckets.entry(label).or_insert(0) += 1;
                        total += 1;
                        break;
                    }
                }
            }
        }

        json!({ "total": total, "buckets": buckets })
    };

    Ok(json!({
        "last_1_month": window(now - 30 * 24 * 60 * 60),
        "last_6_months": window(now - 180 * 24 * 60 * 60),
        "last_1_year": window(year_ago),
    }))
}

// records why a member could not be synced, so a bad handle is visible rather
// than the member simply being absent from the leaderboard
async fn record_failure(pool: &PgPool, target: &Target, reason: &str) {
    record_failure_for(pool, ATCODER, target, reason).await;
}

// note what is absent from the update list: rating, max_rating, rank_title and
// the solve counts are left exactly as they were. a failed pass marks the row
// stale and says why, and the leaderboard keeps serving the last real numbers.
async fn record_failure_for(pool: &PgPool, platform: &str, target: &Target, reason: &str) {
    let _ = sqlx::query(
        r#"INSERT INTO platform_profiles
             (user_id, platform, handle, synced_at, sync_error)
           VALUES ($1, $2, $3, NOW(), $4)
           ON CONFLICT (user_id, platform) DO UPDATE SET
             handle = $3, synced_at = NOW(), sync_error = $4"#,
    )
    .bind(target.user_id)
    .bind(platform)
    .bind(&target.handle)
    .bind(reason)
    .execute(pool)
    .await;
}

// refreshes a single member in the background
//
// called when someone sets or changes their handle: waiting up to six hours for
// the next full pass to see your own data is not a reasonable first impression.
// spawned rather than awaited so the request that triggered it returns at once.
pub fn sync_user_soon(pool: PgPool, user_id: i32) {
    tokio::spawn(async move {
        let row = sqlx::query_as::<_, (i32, String)>(
            r#"SELECT user_id, atcoder_handle FROM users
               WHERE user_id = $1 AND atcoder_handle IS NOT NULL AND atcoder_handle <> ''"#,
        )
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten();

        let Some((user_id, handle)) = row else {
            // handle was cleared — drop whatever we had for them
            let _ =
                sqlx::query("DELETE FROM platform_profiles WHERE user_id = $1 AND platform = $2")
                    .bind(user_id)
                    .bind(ATCODER)
                    .execute(&pool)
                    .await;
            let _ = sqlx::query(
                "DELETE FROM platform_rating_history WHERE user_id = $1 AND platform = $2",
            )
            .bind(user_id)
            .bind(ATCODER)
            .execute(&pool)
            .await;
            return;
        };

        let target = Target { user_id, handle };
        tracing::info!("atcoder: refreshing {} on demand", target.handle);
        match sync_one(&pool, &target).await {
            Ok(()) => tracing::info!("atcoder: {} refreshed", target.handle),
            Err(reason) => {
                tracing::warn!("atcoder: refresh failed for {}: {}", target.handle, reason);
                record_failure(&pool, &target, &reason).await;
            }
        }
    });
}

// one pass over everyone who has given us an atcoder handle
pub async fn sync_atcoder(pool: &PgPool) -> (usize, usize) {
    let targets = sqlx::query_as::<_, (i32, String)>(
        r#"SELECT user_id, atcoder_handle FROM users
           WHERE atcoder_handle IS NOT NULL AND atcoder_handle <> ''
           ORDER BY user_id"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(user_id, handle)| Target { user_id, handle })
    .collect::<Vec<_>>();

    if targets.is_empty() {
        return (0, 0);
    }

    // pull the shared lists here rather than letting the first profile view of
    // the day pay for a megabyte download inside its request
    if let Err(e) = atcoder::fetch_contest_list().await {
        tracing::warn!("could not warm the atcoder contest list: {:?}", e);
    }

    tracing::info!("atcoder sync starting for {} members", targets.len());
    let (mut ok, mut failed) = (0, 0);

    for target in &targets {
        match sync_one(pool, target).await {
            Ok(()) => ok += 1,
            Err(reason) => {
                tracing::warn!("atcoder sync failed for {}: {}", target.handle, reason);
                record_failure(pool, target, &reason).await;
                failed += 1;
            }
        }
        // the courtesy delay upstream asks for, between members as well as
        // between the two calls each member needs
        tokio::time::sleep(atcoder::POLITE_DELAY).await;
    }

    tracing::info!("atcoder sync finished: {} ok, {} failed", ok, failed);
    (ok, failed)
}

// codeforces takes every handle in one user.info call, so a whole club costs a
// single request. that is why this pass has no per-member delay and no partial
// failures: either the call lands or nobody is updated this round.
pub async fn sync_codeforces(pool: &PgPool) -> (usize, usize) {
    let targets = sqlx::query_as::<_, (i32, String)>(
        r#"SELECT user_id, codeforces_handle FROM users
           WHERE codeforces_handle IS NOT NULL AND codeforces_handle <> ''
           ORDER BY user_id"#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(user_id, handle)| Target { user_id, handle })
    .collect::<Vec<_>>();

    if targets.is_empty() {
        return (0, 0);
    }

    tracing::info!("codeforces sync starting for {} members", targets.len());

    let handles = targets
        .iter()
        .map(|t| t.handle.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let url = format!("https://codeforces.com/api/user.info?handles={handles}");

    let fetched = async {
        let response = crate::services::http::codeforces()
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("could not reach codeforces: {e}"))?;
        let body = response
            .json::<crate::models::codeforces::CfApiResponse<
                Vec<crate::models::codeforces::CfUserInfo>,
            >>()
            .await
            .map_err(|e| format!("could not parse user.info: {e}"))?;
        if body.status != "OK" {
            return Err(body
                .comment
                .unwrap_or_else(|| "codeforces returned a non-OK status".to_string()));
        }
        Ok(body.result.unwrap_or_default())
    }
    .await;

    let users = match fetched {
        Ok(u) => u,
        Err(reason) => {
            // leave the stored rows alone. a codeforces outage should cost the
            // leaderboard its freshness, never its contents
            tracing::warn!("codeforces sync failed: {}", reason);
            for target in &targets {
                record_failure_for(pool, CODEFORCES, target, &reason).await;
            }
            return (0, targets.len());
        }
    };

    let by_handle: HashMap<String, &crate::models::codeforces::CfUserInfo> = users
        .iter()
        .map(|u| (u.handle.to_lowercase(), u))
        .collect();

    let (mut ok, mut failed) = (0, 0);
    for target in &targets {
        let Some(info) = by_handle.get(&target.handle.to_lowercase()) else {
            // asked for and not returned means the handle no longer exists
            record_failure_for(pool, CODEFORCES, target, "handle not found on codeforces").await;
            failed += 1;
            continue;
        };

        let written = sqlx::query(
            r#"INSERT INTO platform_profiles
                 (user_id, platform, handle, rating, max_rating, rank_title,
                  synced_at, sync_error)
               VALUES ($1, $2, $3, $4, $5, $6, NOW(), NULL)
               ON CONFLICT (user_id, platform) DO UPDATE SET
                 handle = $3, rating = $4, max_rating = $5, rank_title = $6,
                 synced_at = NOW(), sync_error = NULL"#,
        )
        .bind(target.user_id)
        .bind(CODEFORCES)
        .bind(&info.handle)
        .bind(info.rating)
        .bind(info.max_rating)
        .bind(info.rank.as_deref())
        .execute(pool)
        .await;

        match written {
            Ok(_) => ok += 1,
            Err(e) => {
                tracing::warn!("could not store codeforces row for {}: {}", target.handle, e);
                failed += 1;
            }
        }
    }

    tracing::info!("codeforces sync finished: {} ok, {} failed", ok, failed);
    (ok, failed)
}

// background loop; nothing on the request path ever calls atcoder or codeforces
pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        tokio::time::sleep(STARTUP_DELAY).await;
        loop {
            sync_codeforces(&pool).await;
            sync_atcoder(&pool).await;
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });
}
