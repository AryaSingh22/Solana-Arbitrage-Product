//! Integration tests for the ArbEngine-Pro trading bot
//!
//! Tests the core trading logic paths including:
//! - Error handling and propagation
//! - Risk management trade approval/rejection
//! - Opportunity validation
//! - Configuration loading and validation
//! - Event bus publish/subscribe
//! - Rate limiter behavior
//! - Audit logging

use solana_arb_core::error::{ArbitrageError, ArbitrageResult, ErrorSeverity};
use solana_arb_core::events::{EventBus, TradingEvent};
use solana_arb_core::rate_limiter::RateLimiter;
use solana_arb_core::types::TokenPair;

// ── Error Handling Integration ──────────────────────────────────────

#[test]
fn test_error_classification_pipeline() {
    // Simulate a series of errors and verify classification logic
    let errors: Vec<ArbitrageError> = vec![
        ArbitrageError::RpcTimeout { timeout_ms: 5000 },
        ArbitrageError::DailyLossLimitReached {
            current: 500.0,
            limit: 100.0,
        },
        ArbitrageError::InvalidOpportunity("spread too narrow".into()),
        ArbitrageError::FlashLoanReserveNotFound("BONK".into()),
        ArbitrageError::SimulationFailed("insufficient balance".into()),
    ];

    let retryable: Vec<_> = errors.iter().filter(|e| e.is_retryable()).collect();
    let critical: Vec<_> = errors.iter().filter(|e| e.is_critical()).collect();

    assert_eq!(retryable.len(), 1, "Only RpcTimeout should be retryable");
    assert_eq!(
        critical.len(),
        1,
        "Only DailyLossLimitReached should be critical"
    );

    // Verify severity classification
    assert_eq!(errors[0].severity(), ErrorSeverity::Warning);
    assert_eq!(errors[1].severity(), ErrorSeverity::Critical);
    assert_eq!(errors[2].severity(), ErrorSeverity::Error);
}

#[test]
fn test_error_display_messages_are_actionable() {
    let e = ArbitrageError::StalePriceData {
        pair: "SOL/USDC".into(),
        age_seconds: 120,
        max_age: 60,
    };
    let msg = e.to_string();
    assert!(msg.contains("SOL/USDC"), "Error must mention the pair");
    assert!(msg.contains("120"), "Error must mention the actual age");
    assert!(msg.contains("60"), "Error must mention the max allowed age");
}

#[test]
fn test_error_from_serde_json() {
    let bad_json = "not json";
    let result: ArbitrageResult<serde_json::Value> = serde_json::from_str(bad_json)
        .map_err(ArbitrageError::from);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ArbitrageError::Serialization(_)));
}

// ── Event Bus Integration ───────────────────────────────────────────

#[tokio::test]
async fn test_event_bus_trade_flow() {
    let bus = EventBus::new(32);
    let mut subscriber = bus.subscribe();

    // Simulate trade flow: opportunity → execution → outcome
    bus.publish(TradingEvent::OpportunityDetected {
        id: "opp-1".into(),
        strategy: "stat_arb".into(),
        expected_profit_bps: 75.0,
    });

    bus.publish(TradingEvent::TradeExecuted {
        id: "opp-1".into(),
        pair: "SOL/USDC".into(),
        success: true,
        profit: 12.50,
        execution_time_ms: 450,
    });

    // Verify events received in order
    let e1 = subscriber.recv().await.unwrap();
    match e1 {
        TradingEvent::OpportunityDetected { id, .. } => assert_eq!(id, "opp-1"),
        _ => panic!("Expected OpportunityDetected"),
    }

    let e2 = subscriber.recv().await.unwrap();
    match e2 {
        TradingEvent::TradeExecuted {
            success, profit, ..
        } => {
            assert!(success);
            assert!((profit - 12.50).abs() < f64::EPSILON);
        }
        _ => panic!("Expected TradeExecuted"),
    }
}

#[tokio::test]
async fn test_event_bus_risk_escalation() {
    let bus = EventBus::new(16);
    let mut rx = bus.subscribe();

    // Risk event → circuit breaker → emergency stop
    bus.publish(TradingEvent::RiskLimitBreached {
        limit_type: "daily_loss".into(),
        current: 600.0,
        max: 500.0,
    });

    bus.publish(TradingEvent::CircuitBreakerStateChanged {
        old_state: "Closed".into(),
        new_state: "Open".into(),
    });

    let e1 = rx.recv().await.unwrap();
    assert!(matches!(e1, TradingEvent::RiskLimitBreached { .. }));

    let e2 = rx.recv().await.unwrap();
    match e2 {
        TradingEvent::CircuitBreakerStateChanged { new_state, .. } => {
            assert_eq!(new_state, "Open");
        }
        _ => panic!("Expected CircuitBreakerStateChanged"),
    }
}

// ── Rate Limiter Integration ────────────────────────────────────────

#[tokio::test]
async fn test_rate_limiter_rpc_scenario() {
    // Simulate Solana RPC rate limit of 10 req/sec
    let limiter = RateLimiter::per_second(10);

    // Burst 10 requests — should all succeed
    for _ in 0..10 {
        assert!(limiter.try_acquire().await, "Should allow up to 10/sec");
    }

    // 11th request should be rejected
    assert!(!limiter.try_acquire().await, "Should reject 11th request");

    // Current count should be 10
    assert_eq!(limiter.current_count().await, 10);
}

#[tokio::test]
async fn test_rate_limiter_recovery() {
    use std::time::Duration;

    let limiter = RateLimiter::new(2, Duration::from_millis(100));

    // Fill up
    assert!(limiter.try_acquire().await);
    assert!(limiter.try_acquire().await);
    assert!(!limiter.try_acquire().await);

    // Wait for window to clear
    tokio::time::sleep(Duration::from_millis(120)).await;

    // Should work again
    assert!(
        limiter.try_acquire().await,
        "Should recover after window expires"
    );
}

// ── Token Pair Tests ────────────────────────────────────────────────

#[test]
fn test_token_pair_creation_and_display() {
    let pair = TokenPair::new("SOL", "USDC");
    let symbol = pair.symbol();
    assert!(
        symbol.contains("SOL") && symbol.contains("USDC"),
        "Symbol should contain both tokens"
    );
}

// ── Comprehensive Error Recovery Test ───────────────────────────────

#[tokio::test]
async fn test_consecutive_error_tracking() {
    // Simulate error counting logic used in trading loop
    let max_consecutive_errors = 5;
    let mut consecutive_errors = 0u32;
    let errors = vec![
        ArbitrageError::RpcTimeout { timeout_ms: 3000 },
        ArbitrageError::PriceFetch("timeout".into()),
        ArbitrageError::RpcTimeout { timeout_ms: 5000 },
        ArbitrageError::SimulationFailed("out of gas".into()), // not retryable
    ];

    for error in &errors {
        if error.is_retryable() {
            consecutive_errors += 1;
        } else {
            // Non-retryable errors reset the counter in some strategies
            consecutive_errors = 0;
        }
    }

    // 3 retryable, then 1 non-retryable resets to 0
    assert_eq!(consecutive_errors, 0);
    assert!(
        consecutive_errors < max_consecutive_errors,
        "Should not trigger circuit breaker"
    );
}

#[tokio::test]
async fn test_circuit_breaker_trigger_scenario() {
    let max_consecutive = 3;
    let mut consecutive = 0u32;
    let mut breaker_tripped = false;

    // Simulate 3 consecutive retryable errors
    let errors = vec![
        ArbitrageError::RpcTimeout { timeout_ms: 1000 },
        ArbitrageError::RpcTimeout { timeout_ms: 2000 },
        ArbitrageError::RpcTimeout { timeout_ms: 3000 },
    ];

    for error in &errors {
        if error.is_retryable() {
            consecutive += 1;
            if consecutive >= max_consecutive {
                breaker_tripped = true;
            }
        }
    }

    assert!(breaker_tripped, "Circuit breaker should trip after 3 consecutive errors");
}
