use sqlx::postgres::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::models::ranker::RankerResponse;

// how long an /analyze result stays downloadable as a pdf
const SESSION_TTL: Duration = Duration::from_secs(6 * 60 * 60);

// hard cap so the cache can't grow forever
const MAX_SESSIONS: usize = 500;

pub struct CachedResult {
    pub stored_at: Instant,
    pub response: RankerResponse,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    results_cache: Arc<Mutex<HashMap<String, CachedResult>>>,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            results_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // a poisoned lock just means another request panicked — the cache is still fine
    fn cache(&self) -> MutexGuard<'_, HashMap<String, CachedResult>> {
        self.results_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    // stores a result for pdf download, clearing out expired ones first
    pub fn store_result(&self, session_id: String, response: RankerResponse) {
        let mut cache = self.cache();
        let now = Instant::now();

        cache.retain(|_, entry| now.duration_since(entry.stored_at) < SESSION_TTL);

        // still too many after that — drop the oldest
        while cache.len() >= MAX_SESSIONS {
            let oldest = cache
                .iter()
                .min_by_key(|(_, entry)| entry.stored_at)
                .map(|(key, _)| key.clone());
            match oldest {
                Some(key) => {
                    cache.remove(&key);
                }
                None => break,
            }
        }

        cache.insert(
            session_id,
            CachedResult {
                stored_at: now,
                response,
            },
        );
    }

    // returns a cached result if it exists and hasn't expired
    pub fn get_result(&self, session_id: &str) -> Option<RankerResponse> {
        let mut cache = self.cache();
        let entry = cache.get(session_id)?;

        if Instant::now().duration_since(entry.stored_at) >= SESSION_TTL {
            cache.remove(session_id);
            return None;
        }

        Some(entry.response.clone())
    }
}
