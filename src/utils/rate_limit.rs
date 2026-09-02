use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::errors::AppError;

// --- limits ---
// auth flows are keyed by email rather than ip on purpose: campus wifi puts a
// lot of students behind one address, and the attacks we care about target a
// specific account anyway

// wrong otp guesses before the code is burned and a new one is needed
pub const OTP_MAX_ATTEMPTS: usize = 5;
pub const OTP_ATTEMPT_WINDOW: Duration = Duration::from_secs(15 * 60);

// failed logins per email
pub const LOGIN_MAX_ATTEMPTS: usize = 10;
pub const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);

// otp emails per address — covers register, resend and forgot-password together
pub const OTP_SEND_MAX: usize = 5;
pub const OTP_SEND_WINDOW: Duration = Duration::from_secs(60 * 60);

// ranker analyze per ip — each call fans out to vjudge, so keep it modest
pub const ANALYZE_MAX: usize = 10;
pub const ANALYZE_WINDOW: Duration = Duration::from_secs(5 * 60);

// stop the map itself from becoming a memory leak
const MAX_TRACKED_KEYS: usize = 10_000;

// sliding-window counter shared across handlers
#[derive(Clone)]
pub struct RateLimiter {
    hits: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            hits: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // a poisoned lock just means another request panicked — the map is still fine
    fn hits(&self) -> MutexGuard<'_, HashMap<String, Vec<Instant>>> {
        self.hits
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    // records a hit and fails if the key is already over its limit
    pub fn check(&self, key: &str, limit: usize, window: Duration) -> Result<(), AppError> {
        let now = Instant::now();
        let mut hits = self.hits();

        // sweep expired keys when the map grows — cheap and keeps memory flat
        if hits.len() > MAX_TRACKED_KEYS {
            hits.retain(|_, times| times.iter().any(|t| now.duration_since(*t) < window));
        }

        let times = hits.entry(key.to_string()).or_default();
        times.retain(|t| now.duration_since(*t) < window);

        if times.len() >= limit {
            // oldest hit decides when a slot frees up again
            let retry_in = times
                .first()
                .map(|t| window.saturating_sub(now.duration_since(*t)))
                .unwrap_or(window);

            return Err(AppError::TooManyRequests(format!(
                "Too many attempts. Please try again in {} seconds.",
                retry_in.as_secs().max(1)
            )));
        }

        times.push(now);
        Ok(())
    }

    // how many hits a key has recorded inside the window
    pub fn count(&self, key: &str, window: Duration) -> usize {
        let now = Instant::now();
        let mut hits = self.hits();
        match hits.get_mut(key) {
            Some(times) => {
                times.retain(|t| now.duration_since(*t) < window);
                times.len()
            }
            None => 0,
        }
    }

    // clears a key — used after a success so honest users don't stay penalised
    pub fn reset(&self, key: &str) {
        self.hits().remove(key);
    }
}

// keys are namespaced so the same email can't collide across flows
pub fn otp_attempt_key(email: &str) -> String {
    format!("otp_attempt:{}", email.trim().to_lowercase())
}

pub fn otp_send_key(email: &str) -> String {
    format!("otp_send:{}", email.trim().to_lowercase())
}

pub fn login_key(email: &str) -> String {
    format!("login:{}", email.trim().to_lowercase())
}

pub fn analyze_key(ip: &str) -> String {
    format!("analyze:{}", ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(60);

    #[test]
    fn allows_up_to_the_limit_then_refuses() {
        let limiter = RateLimiter::new();
        for i in 1..=3 {
            assert!(limiter.check("a@b.com", 3, WINDOW).is_ok(), "attempt {i}");
        }
        assert!(matches!(
            limiter.check("a@b.com", 3, WINDOW),
            Err(AppError::TooManyRequests(_))
        ));
    }

    #[test]
    fn the_refusal_says_how_long_to_wait() {
        let limiter = RateLimiter::new();
        limiter.check("a@b.com", 1, WINDOW).unwrap();
        let err = limiter.check("a@b.com", 1, WINDOW).unwrap_err();
        let msg = format!("{err:?}");
        assert!(msg.contains("second"), "should name a wait: {msg}");
    }

    #[test]
    fn keys_are_counted_separately() {
        // one member being locked out must not lock out anyone else
        let limiter = RateLimiter::new();
        limiter.check("a@b.com", 1, WINDOW).unwrap();
        assert!(limiter.check("a@b.com", 1, WINDOW).is_err());
        assert!(limiter.check("other@b.com", 1, WINDOW).is_ok());
    }

    #[test]
    fn reset_clears_a_key() {
        // a successful login clears the failed-login counter
        let limiter = RateLimiter::new();
        limiter.check("a@b.com", 1, WINDOW).unwrap();
        assert!(limiter.check("a@b.com", 1, WINDOW).is_err());
        limiter.reset("a@b.com");
        assert!(limiter.check("a@b.com", 1, WINDOW).is_ok());
    }

    #[test]
    fn hits_outside_the_window_stop_counting() {
        let limiter = RateLimiter::new();
        let tiny = Duration::from_millis(50);
        limiter.check("a@b.com", 1, tiny).unwrap();
        assert!(limiter.check("a@b.com", 1, tiny).is_err());
        std::thread::sleep(Duration::from_millis(80));
        assert!(
            limiter.check("a@b.com", 1, tiny).is_ok(),
            "the old hit should have aged out"
        );
    }
}
