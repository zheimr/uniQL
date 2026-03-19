//! Token bucket rate limiter — per-IP request throttling.
//!
//! No external dependencies. Thread-safe via DashMap-style approach with tokio::sync::Mutex.

use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::Mutex;

/// Rate limiter configuration.
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    max_tokens: u32,
    refill_rate: f64, // tokens per second
}

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter.
    /// `max_requests_per_sec`: maximum burst size and sustained rate.
    pub fn new(max_requests_per_sec: u32) -> Self {
        RateLimiter {
            buckets: Mutex::new(HashMap::new()),
            max_tokens: max_requests_per_sec * 10, // 10 second burst window
            refill_rate: max_requests_per_sec as f64,
        }
    }

    /// Check if a request from `key` (IP address) is allowed.
    /// Returns Ok(remaining) or Err(retry_after_ms).
    pub async fn check(&self, key: &str) -> Result<u32, u64> {
        let mut buckets = self.buckets.lock().await;

        let now = Instant::now();
        let bucket = buckets.entry(key.to_string()).or_insert(TokenBucket {
            tokens: self.max_tokens as f64,
            last_refill: now,
        });

        // Refill tokens based on elapsed time
        let elapsed = bucket.last_refill.elapsed().as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * self.refill_rate).min(self.max_tokens as f64);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(bucket.tokens as u32)
        } else {
            // Calculate retry-after in milliseconds
            let deficit = 1.0 - bucket.tokens;
            let retry_after_ms = (deficit / self.refill_rate * 1000.0) as u64;
            Err(retry_after_ms.max(100))
        }
    }

    /// Clean up stale buckets (older than 60 seconds).
    pub async fn cleanup(&self) {
        let mut buckets = self.buckets.lock().await;
        buckets.retain(|_, b| b.last_refill.elapsed().as_secs() < 60);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allows_within_limit() {
        let limiter = RateLimiter::new(100);
        for _ in 0..50 {
            assert!(limiter.check("127.0.0.1").await.is_ok());
        }
    }

    #[tokio::test]
    async fn blocks_over_limit() {
        let limiter = RateLimiter::new(1); // 1 req/s, 10 burst
        // Exhaust burst
        for _ in 0..10 {
            let _ = limiter.check("127.0.0.1").await;
        }
        let result = limiter.check("127.0.0.1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn different_keys_independent() {
        let limiter = RateLimiter::new(1);
        for _ in 0..10 {
            let _ = limiter.check("ip1").await;
        }
        // ip1 exhausted, ip2 should still work
        assert!(limiter.check("ip2").await.is_ok());
    }

    #[tokio::test]
    async fn returns_remaining() {
        let limiter = RateLimiter::new(100);
        let result = limiter.check("127.0.0.1").await;
        assert!(result.is_ok());
        assert!(result.unwrap() > 0);
    }

    #[tokio::test]
    async fn returns_retry_after() {
        let limiter = RateLimiter::new(1);
        for _ in 0..10 {
            let _ = limiter.check("test").await;
        }
        let result = limiter.check("test").await;
        assert!(result.is_err());
        assert!(result.unwrap_err() >= 100); // at least 100ms
    }

    #[tokio::test]
    async fn cleanup_removes_stale() {
        let limiter = RateLimiter::new(100);
        let _ = limiter.check("old").await;
        limiter.cleanup().await;
        // Not stale yet (just created), should still be there
        let buckets = limiter.buckets.lock().await;
        assert!(buckets.contains_key("old"));
    }
}
