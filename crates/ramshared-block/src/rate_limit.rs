//! Request rate limiting to protect against resource exhaustion and request flooding.
//! Uses a token bucket algorithm.

use std::time::{Instant, Duration};

/// Token bucket for rate limiting requests from clients.
pub struct RateLimiter {
    capacity: u32,
    tokens: u32,
    refill_rate_per_sec: u32,
    last_refill: Instant,
}

impl RateLimiter {
    /// Creates a new `RateLimiter` with the given capacity and refill rate (tokens/second).
    pub fn new(capacity: u32, refill_rate_per_sec: u32) -> Self {
        Self {
            capacity,
            tokens: capacity,
            refill_rate_per_sec,
            last_refill: Instant::now(),
        }
    }

    /// Acquires a single token. Returns true if successful, false if rate limited.
    pub fn acquire(&mut self) -> bool {
        self.refill();
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }

    /// Refills the token bucket based on elapsed time.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let tokens_to_add = (elapsed.as_secs_f64() * self.refill_rate_per_sec as f64) as u32;
        if tokens_to_add > 0 {
            self.tokens = std::cmp::min(self.capacity, self.tokens + tokens_to_add);
            // Advance last_refill by the exact amount of time used to generate the tokens
            // to prevent fractional time accumulation loss.
            let time_for_tokens = Duration::from_secs_f64(tokens_to_add as f64 / self.refill_rate_per_sec as f64);
            self.last_refill += time_for_tokens;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_acquires_tokens_up_to_capacity() {
        let mut limiter = RateLimiter::new(3, 1);
        assert!(limiter.acquire());
        assert!(limiter.acquire());
        assert!(limiter.acquire());
        assert!(!limiter.acquire()); // 4th should fail (capacity 3)
    }

    #[test]
    fn rate_limiter_refills_tokens() {
        let mut limiter = RateLimiter::new(1, 100);
        assert!(limiter.acquire());
        assert!(!limiter.acquire());
        std::thread::sleep(std::time::Duration::from_millis(20));
        limiter.refill(); // Forcing a refill, normally called inside acquire
        assert!(limiter.acquire());
    }
}
