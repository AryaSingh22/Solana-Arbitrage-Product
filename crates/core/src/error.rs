//! Comprehensive error types for the Solana Arbitrage system
//!
//! Provides structured, categorized errors with severity classification
//! for production-grade error handling and alerting.

use thiserror::Error;

/// Main error type for the arbitrage system
#[derive(Error, Debug)]
pub enum ArbitrageError {
    // ── RPC / Network Errors ────────────────────────────────────────
    #[error("DEX connection error: {0}")]
    DexConnection(String),

    #[error("RPC request failed: {0}")]
    RpcError(String),

    #[error("RPC connection timeout after {timeout_ms}ms")]
    RpcTimeout { timeout_ms: u64 },

    #[error("RPC rate limit exceeded: {0}")]
    RpcRateLimit(String),

    // ── Price Fetching Errors ───────────────────────────────────────
    #[error("Price fetch error: {0}")]
    PriceFetch(String),

    #[error("Price fetch failed for {pair}: {reason}")]
    PriceFetchDetailed { pair: String, reason: String },

    #[error("No price available for {0}")]
    PriceNotAvailable(String),

    #[error("Stale price data: {pair} is {age_seconds}s old (max {max_age}s)")]
    StalePriceData {
        pair: String,
        age_seconds: u64,
        max_age: u64,
    },

    // ── Opportunity Errors ──────────────────────────────────────────
    #[error("Opportunity validation failed: {0}")]
    InvalidOpportunity(String),

    // ── Execution Errors ────────────────────────────────────────────
    #[error("Transaction simulation failed: {0}")]
    SimulationFailed(String),

    #[error("Transaction submission failed: {0}")]
    SubmissionFailed(String),

    #[error("Transaction confirmation timeout after {timeout_secs}s")]
    ConfirmationTimeout { timeout_secs: u64 },

    #[error("Transaction error: {0}")]
    Transaction(String),

    // ── Flash Loan Errors ───────────────────────────────────────────
    #[error("Flash loan amount {amount} exceeds maximum {max}")]
    FlashLoanAmountExceeded { amount: u64, max: u64 },

    #[error("Flash loan simulation failed: {0}")]
    FlashLoanSimulationFailed(String),

    #[error("Insufficient liquidity for flash loan: need {need}, available {available}")]
    InsufficientFlashLoanLiquidity { need: u64, available: u64 },

    #[error("Insufficient liquidity: {0}")]
    InsufficientLiquidity(String),

    #[error("Flash loan reserve not configured for mint: {0}")]
    FlashLoanReserveNotFound(String),

    // ── Risk Management Errors ──────────────────────────────────────
    #[error("Circuit breaker is open: {reason}")]
    CircuitBreakerOpen { reason: String },

    #[error("Position size ${size} exceeds limit ${limit}")]
    PositionSizeExceeded { size: f64, limit: f64 },

    #[error("Daily loss limit reached: ${current:.2} / ${limit:.2}")]
    DailyLossLimitReached { current: f64, limit: f64 },

    #[error("VaR exceeded: {current:.2}% > {limit:.2}%")]
    VarExceeded { current: f64, limit: f64 },

    #[error("Slippage exceeded: expected {expected}%, got {actual}%")]
    SlippageExceeded { expected: f64, actual: f64 },

    // ── Strategy Errors ─────────────────────────────────────────────
    #[error("Strategy '{strategy}' failed: {reason}")]
    StrategyError { strategy: String, reason: String },

    // ── Jupiter API Errors ──────────────────────────────────────────
    #[error("Jupiter API error: {0}")]
    JupiterApiError(String),

    #[error("Jupiter quote failed: {0}")]
    JupiterQuoteFailed(String),

    // ── WebSocket Errors ────────────────────────────────────────────
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("WebSocket connection failed: {0}")]
    WebSocketConnectionFailed(String),

    #[error("WebSocket message parse error: {0}")]
    WebSocketParseError(String),

    // ── Infrastructure Errors ───────────────────────────────────────
    #[error("Database error: {0}")]
    Database(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP request error: {0}")]
    Http(String),

    #[error("Redis error: {0}")]
    Redis(String),

    #[error("Rate limited by {0}")]
    RateLimited(String),

    // ── Configuration Errors ────────────────────────────────────────
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Missing required configuration: {0}")]
    MissingConfig(String),

    #[error("Invalid public key: {0}")]
    InvalidPubkey(String),

    // ── General Errors ──────────────────────────────────────────────
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

// ── Severity & Classification ───────────────────────────────────────

/// Error severity level for logging and alerting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Transient issue, will retry automatically
    Warning,
    /// Operational error, logged and tracked
    Error,
    /// Must trigger alerts, may pause trading
    Critical,
}

impl ArbitrageError {
    /// Returns true if this error is retryable (transient network/timeout issues)
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ArbitrageError::RpcTimeout { .. }
                | ArbitrageError::RpcRateLimit(_)
                | ArbitrageError::RpcError(_)
                | ArbitrageError::WebSocketConnectionFailed(_)
                | ArbitrageError::ConfirmationTimeout { .. }
                | ArbitrageError::RateLimited(_)
                | ArbitrageError::PriceFetch(_)
        )
    }

    /// Returns true if this error is critical (should trigger alerts)
    pub fn is_critical(&self) -> bool {
        matches!(
            self,
            ArbitrageError::DailyLossLimitReached { .. }
                | ArbitrageError::VarExceeded { .. }
                | ArbitrageError::FlashLoanSimulationFailed(_)
                | ArbitrageError::CircuitBreakerOpen { .. }
        )
    }

    /// Returns severity level for logging and alerting
    pub fn severity(&self) -> ErrorSeverity {
        if self.is_critical() {
            ErrorSeverity::Critical
        } else if self.is_retryable() {
            ErrorSeverity::Warning
        } else {
            ErrorSeverity::Error
        }
    }
}

// ── From implementations for optional dependencies ──────────────────

#[cfg(feature = "http")]
impl From<reqwest::Error> for ArbitrageError {
    fn from(e: reqwest::Error) -> Self {
        ArbitrageError::Http(e.to_string())
    }
}

#[cfg(feature = "cache")]
impl From<redis::RedisError> for ArbitrageError {
    fn from(e: redis::RedisError) -> Self {
        ArbitrageError::Redis(e.to_string())
    }
}

/// Result type alias for arbitrage operations
pub type ArbitrageResult<T> = Result<T, ArbitrageError>;

// ── Retry Logic ─────────────────────────────────────────────────────

/// Retry a fallible async operation with exponential backoff.
///
/// # Arguments
/// * `f` - Closure returning a `Future<Output = ArbitrageResult<T>>`
/// * `max_attempts` - Maximum number of attempts (first try + retries)
/// * `base_delay` - Initial delay before first retry
///
/// # Behaviour
/// Only retries errors where `ArbitrageError::is_retryable()` returns true.
/// Non-retryable errors are returned immediately.
///
/// # Example
/// ```no_run
/// use solana_arb_core::error::retry_with_backoff;
/// use std::time::Duration;
/// let result = retry_with_backoff(|| async { Ok(42u32) }, 3, Duration::from_millis(500));
/// ```
pub async fn retry_with_backoff<F, Fut, T>(
    mut f: F,
    max_attempts: u32,
    base_delay: std::time::Duration,
) -> ArbitrageResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ArbitrageResult<T>>,
{
    let mut attempt = 0u32;
    loop {
        match f().await {
            Ok(value) => return Ok(value),
            Err(e) if !e.is_retryable() || attempt + 1 >= max_attempts => {
                return Err(e);
            }
            Err(e) => {
                attempt += 1;
                let delay = base_delay * 2u32.pow(attempt - 1);
                tracing::warn!(
                    error = %e,
                    attempt = attempt,
                    next_retry_ms = delay.as_millis() as u64,
                    "Retryable error — backing off"
                );
                tokio::time::sleep(delay).await;
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod retry_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn test_retry_succeeds_on_second_attempt() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();

        let result = retry_with_backoff(
            || {
                let c = calls_clone.clone();
                async move {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    if n == 0 {
                        Err(ArbitrageError::RpcTimeout { timeout_ms: 100 })
                    } else {
                        Ok(42u32)
                    }
                }
            },
            3,
            Duration::from_millis(1),
        )
        .await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_non_retryable_error_returned_immediately() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();

        let result: ArbitrageResult<u32> = retry_with_backoff(
            || {
                let c = calls_clone.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err(ArbitrageError::Config("bad config".to_string()))
                }
            },
            3,
            Duration::from_millis(1),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1); // Only called once
    }

    #[tokio::test]
    async fn test_exhaust_retries_returns_last_error() {
        let result: ArbitrageResult<u32> = retry_with_backoff(
            || async { Err(ArbitrageError::RpcTimeout { timeout_ms: 100 }) },
            3,
            Duration::from_millis(1),
        )
        .await;

        assert!(matches!(
            result,
            Err(ArbitrageError::RpcTimeout { timeout_ms: 100 })
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retryable_errors() {
        assert!(ArbitrageError::RpcTimeout { timeout_ms: 5000 }.is_retryable());
        assert!(ArbitrageError::RpcRateLimit("429".into()).is_retryable());
        assert!(ArbitrageError::RateLimited("Jupiter".into()).is_retryable());

        // Non-retryable
        assert!(!ArbitrageError::InvalidOpportunity("bad".into()).is_retryable());
        assert!(!ArbitrageError::Config("missing".into()).is_retryable());
    }

    #[test]
    fn test_critical_errors() {
        assert!(ArbitrageError::DailyLossLimitReached {
            current: 500.0,
            limit: 100.0
        }
        .is_critical());
        assert!(ArbitrageError::CircuitBreakerOpen {
            reason: "test".into()
        }
        .is_critical());

        // Non-critical
        assert!(!ArbitrageError::RpcTimeout { timeout_ms: 5000 }.is_critical());
    }

    #[test]
    fn test_severity_classification() {
        assert_eq!(
            ArbitrageError::RpcTimeout { timeout_ms: 5000 }.severity(),
            ErrorSeverity::Warning
        );
        assert_eq!(
            ArbitrageError::DailyLossLimitReached {
                current: 500.0,
                limit: 100.0
            }
            .severity(),
            ErrorSeverity::Critical
        );
        assert_eq!(
            ArbitrageError::InvalidOpportunity("test".into()).severity(),
            ErrorSeverity::Error
        );
    }

    #[test]
    fn test_error_display() {
        let e = ArbitrageError::StalePriceData {
            pair: "SOL/USDC".into(),
            age_seconds: 120,
            max_age: 60,
        };
        assert!(e.to_string().contains("SOL/USDC"));
        assert!(e.to_string().contains("120"));
    }
}
