use axum::http::HeaderMap;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// A small in-memory, fixed-window rate limiter keyed by an arbitrary string
/// (typically `"<route>:<client_ip>"` or `"<route>:<client_ip>:<email>"`).
///
/// This deliberately does not attempt to be a general-purpose HTTP middleware:
/// different sensitive endpoints (CEO admin login, owner login, owner
/// registration) need different limits and different keying strategies (e.g.
/// owner login should key on IP *and* email so a shared office IP doesn't
/// lock out every other tenant), so it's applied inline at the top of each
/// handler via [`RateLimiter::check`].
pub struct RateLimiter {
    buckets: parking_lot::Mutex<HashMap<String, Vec<Instant>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: parking_lot::Mutex::new(HashMap::new()),
        }
    }

    /// Records an attempt for `key` and returns `Ok(())` if it is allowed
    /// under `max_attempts` within the trailing `window`, or `Err(retry_after_secs)`
    /// if the caller should be rejected with `429 Too Many Requests`.
    pub fn check(&self, key: &str, max_attempts: usize, window: Duration) -> Result<(), u64> {
        let now = Instant::now();
        let mut buckets = self.buckets.lock();
        let entry = buckets.entry(key.to_string()).or_default();
        entry.retain(|&t| now.duration_since(t) < window);

        if entry.len() >= max_attempts {
            let oldest = entry[0];
            let retry_after = window.saturating_sub(now.duration_since(oldest));
            return Err(retry_after.as_secs().max(1));
        }

        entry.push(now);
        Ok(())
    }

    /// Clears the counter for `key`, e.g. after a successful login so a
    /// legitimate user isn't penalized by earlier mistyped attempts.
    pub fn reset(&self, key: &str) {
        self.buckets.lock().remove(key);
    }

    /// Drops buckets with no attempts inside `max_idle`, bounding memory
    /// growth from many distinct keys (e.g. IP-spoofed spam). Intended to be
    /// called periodically from a background task.
    pub fn cleanup(&self, max_idle: Duration) {
        let now = Instant::now();
        let mut buckets = self.buckets.lock();
        buckets.retain(|_, attempts| {
            attempts.retain(|&t| now.duration_since(t) < max_idle);
            !attempts.is_empty()
        });
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Best-effort client IP resolution: uses the LAST `X-Forwarded-For` entry
/// (appended by the closest proxy, e.g. Render's edge — attacker-controlled
/// leftmost entries can't spoof it) and falls back to the raw TCP peer
/// address from `ConnectInfo`.
pub fn client_ip(headers: &HeaderMap, connect_info: Option<SocketAddr>) -> String {
    if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(last) = xff.split(',').next_back() {
            let ip = last.trim();
            if !ip.is_empty() {
                return ip.to_string();
            }
        }
    }
    connect_info
        .map(|s| s.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Builds the standard 429 JSON error response for a rejected request.
pub fn too_many_requests(
    retry_after_secs: u64,
) -> (
    axum::http::StatusCode,
    [(axum::http::HeaderName, String); 1],
    axum::Json<serde_json::Value>,
) {
    (
        axum::http::StatusCode::TOO_MANY_REQUESTS,
        [(axum::http::header::RETRY_AFTER, retry_after_secs.to_string())],
        axum::Json(serde_json::json!({
            "error": "Too many attempts. Please wait before trying again.",
            "code": "RATE_LIMITED",
            "retry_after_seconds": retry_after_secs
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_the_limit_then_blocks() {
        let limiter = RateLimiter::new();
        let window = Duration::from_secs(60);
        for _ in 0..5 {
            assert!(limiter.check("k", 5, window).is_ok());
        }
        let err = limiter.check("k", 5, window).unwrap_err();
        assert!(err >= 1);
    }

    #[test]
    fn distinct_keys_are_independent() {
        let limiter = RateLimiter::new();
        let window = Duration::from_secs(60);
        for _ in 0..3 {
            assert!(limiter.check("a", 3, window).is_ok());
        }
        assert!(limiter.check("a", 3, window).is_err());
        // A different key must not be affected by "a"'s bucket.
        assert!(limiter.check("b", 3, window).is_ok());
    }

    #[test]
    fn reset_clears_the_bucket() {
        let limiter = RateLimiter::new();
        let window = Duration::from_secs(60);
        for _ in 0..3 {
            assert!(limiter.check("k", 3, window).is_ok());
        }
        assert!(limiter.check("k", 3, window).is_err());
        limiter.reset("k");
        assert!(limiter.check("k", 3, window).is_ok());
    }
}
