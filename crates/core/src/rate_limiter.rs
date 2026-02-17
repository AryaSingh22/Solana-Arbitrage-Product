//! Token-bucket rate limiter for RPC and API calls
//!
//! Prevents exceeding rate limits on external services like Solana RPC,
//! Jupiter API, and Jito block engine.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Rate limiter using a sliding window approach
#[derive(Debug)]
pub struct RateLimiter {
    state: Arc<Mutex<RateLimiterState>>,
    max_requests: usize,
    window: Duration,
}

#[derive(Debug)]
struct RateLimiterState {
    /// Timestamps of recent requests within the current window
    timestamps: Vec<Instant>,
}

impl RateLimiter {
    /// Create a new rate limiter
    ///
    /// # Arguments
    /// * `max_requests` - Maximum requests allowed per window
    /// * `window` - Time window duration
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(RateLimiterState {
                timestamps: Vec::with_capacity(max_requests),
            })),
            max_requests,
            window,
        }
    }

    /// Create a rate limiter for the given requests per second
    pub fn per_second(requests_per_second: usize) -> Self {
        Self::new(requests_per_second, Duration::from_secs(1))
    }

    /// Wait until a request slot is available, then acquire it
    ///
    /// This will block (async) if the rate limit has been reached.
    pub async fn acquire(&self) {
        loop {
            let wait_time = {
                let mut state = self.state.lock().await;
                let now = Instant::now();

                // Remove expired timestamps
                state
                    .timestamps
                    .retain(|t| now.duration_since(*t) < self.window);

                if state.timestamps.len() < self.max_requests {
                    // Slot available
                    state.timestamps.push(now);
                    return;
                }

                // Calculate how long to wait for the oldest request to expire
                if let Some(oldest) = state.timestamps.first() {
                    let elapsed = now.duration_since(*oldest);
                    if elapsed < self.window {
                        self.window - elapsed
                    } else {
                        Duration::from_millis(1) // Tiny wait, then retry
                    }
                } else {
                    Duration::from_millis(1)
                }
            };

            tokio::time::sleep(wait_time).await;
        }
    }

    /// Try to acquire a slot without waiting
    ///
    /// Returns `true` if a slot was acquired, `false` if rate limited.
    pub async fn try_acquire(&self) -> bool {
        let mut state = self.state.lock().await;
        let now = Instant::now();

        state
            .timestamps
            .retain(|t| now.duration_since(*t) < self.window);

        if state.timestamps.len() < self.max_requests {
            state.timestamps.push(now);
            true
        } else {
            false
        }
    }

    /// Get current request count within the window
    pub async fn current_count(&self) -> usize {
        let state = self.state.lock().await;
        let now = Instant::now();
        state
            .timestamps
            .iter()
            .filter(|t| now.duration_since(**t) < self.window)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_within_limit() {
        let limiter = RateLimiter::per_second(5);

        for _ in 0..5 {
            assert!(limiter.try_acquire().await);
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::per_second(2);

        assert!(limiter.try_acquire().await);
        assert!(limiter.try_acquire().await);
        assert!(!limiter.try_acquire().await); // Over limit
    }

    #[tokio::test]
    async fn test_rate_limiter_current_count() {
        let limiter = RateLimiter::per_second(10);

        limiter.acquire().await;
        limiter.acquire().await;
        limiter.acquire().await;

        assert_eq!(limiter.current_count().await, 3);
    }

    #[tokio::test]
    async fn test_rate_limiter_window_expiry() {
        let limiter = RateLimiter::new(2, Duration::from_millis(50));

        assert!(limiter.try_acquire().await);
        assert!(limiter.try_acquire().await);
        assert!(!limiter.try_acquire().await); // Full

        // Wait for window to expire
        tokio::time::sleep(Duration::from_millis(60)).await;

        assert!(limiter.try_acquire().await); // Should work now
    }
}
