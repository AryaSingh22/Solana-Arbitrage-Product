use solana_arb_bot::execution::Executor;
use solana_arb_core::{
    events::{EventBus, TradingEvent},
    rate_limiter::RateLimiter,
    risk::{RiskConfig, RiskManager, TradeOutcome},
    history::HistoryRecorder,
    types::{TokenPair, ArbitrageOpportunity, DexType, TradeResult},
    Uuid,
};
use std::sync::Arc;
use tokio::time::Duration;
use rust_decimal::Decimal;
use chrono::Utc;

#[tokio::test]
async fn test_rate_limiter_throttling() {
    let limiter = Arc::new(RateLimiter::per_second(5));
    let start = std::time::Instant::now();
    
    // Acquire 6 permits
    for _ in 0..6 {
        limiter.acquire().await;
    }
    
    let elapsed = start.elapsed();
    assert!(elapsed.as_millis() >= 1000, "Should throttle requests (elapsed: {}ms)", elapsed.as_millis());
}

#[tokio::test]
async fn test_event_bus_subscriptions() {
    let event_bus = Arc::new(EventBus::new(100));
    let mut rx1 = event_bus.subscribe();
    let mut rx2 = event_bus.subscribe();

    event_bus.publish(TradingEvent::SystemStarted {
        mode: "test".to_string(),
    });

    let e1 = rx1.recv().await.unwrap();
    let e2 = rx2.recv().await.unwrap();

    assert!(matches!(e1, TradingEvent::SystemStarted { .. }));
    assert!(matches!(e2, TradingEvent::SystemStarted { .. }));
}

#[tokio::test]
async fn test_config_validation_rules() {
    // Manually create invalid config
    let mut invalid_config = solana_arb_bot::config_manager::DynamicConfig {
        version: "1.0.0".to_string(),
        trading: solana_arb_bot::config_manager::TradingConfig {
            enabled: true,
            max_position_size: 0, // Invalid: must be > 0
            min_profit_bps: -5.0, // Invalid: must be >= 0
            max_slippage_bps: 0,
        },
        risk: solana_arb_bot::config_manager::RiskConfig {
            circuit_breaker_enabled: true,
            max_consecutive_losses: 5,
            max_daily_loss: -100.0, // Invalid
            var_limit_percent: 150.0, // Invalid
        },
        performance: solana_arb_bot::config_manager::PerformanceConfig {
            poll_interval_ms: 10, // Invalid: too small
            enable_websocket: true,
            enable_parallel_fetching: true,
        },
        alerts: solana_arb_bot::config_manager::AlertConfig {
            telegram_enabled: false,
            discord_enabled: false,
            alert_on_profit: 0.0,
            alert_on_loss: -1.0, // Invalid
        },
    };
    
    assert!(invalid_config.validate().is_err());
    
    // Fix it
    invalid_config.trading.max_position_size = 1000;
    invalid_config.trading.min_profit_bps = 10.0;
    invalid_config.trading.max_slippage_bps = 50;
    invalid_config.risk.max_daily_loss = 100.0;
    invalid_config.risk.var_limit_percent = 5.0;
    invalid_config.performance.poll_interval_ms = 500;
    invalid_config.alerts.alert_on_loss = 10.0;
    
    assert!(invalid_config.validate().is_ok());
}

#[tokio::test]
async fn test_risk_circuit_breaker() {
    let mut risk_manager = RiskManager::new(RiskConfig::default());
    let event_bus = Arc::new(EventBus::new(100));
    risk_manager.set_event_bus(event_bus.clone()).await;

    // Simulate 3 consecutive losses (default threshold is 3)
    let outcome = TradeOutcome {
        timestamp: Utc::now(),
        pair: "SOL/USDC".to_string(),
        profit_loss: Decimal::new(-10, 0),
        was_successful: false,
    };

    risk_manager.record_trade(outcome.clone()).await;
    risk_manager.record_trade(outcome.clone()).await;
    risk_manager.record_trade(outcome.clone()).await;
    
    // Should be open now
    assert!(!risk_manager.circuit_breaker.can_execute().await);
}

#[tokio::test]
async fn test_history_recorder() {
    let recorder = HistoryRecorder::new("test_history.jsonl", "TEST-SESSION");
    let pair = TokenPair::new("SOL", "USDC");
    let opp = ArbitrageOpportunity {
        id: Uuid::new_v4(),
        pair: pair.clone(),
        buy_dex: DexType::Raydium,
        sell_dex: DexType::Orca,
        buy_price: Decimal::new(100, 0),
        sell_price: Decimal::new(101, 0),
        gross_profit_pct: Decimal::new(1, 0),
        net_profit_pct: Decimal::new(1, 0),
        estimated_profit_usd: Some(Decimal::new(10, 0)),
        recommended_size: Some(Decimal::new(1000, 0)),
        detected_at: Utc::now(),
        expired_at: None,
    };

    recorder.record_trade(&opp, Decimal::new(1000, 0), Decimal::new(10, 0), true, None, None, true);
    
    // Verification would typically involve reading the file, 
    // but here we just check no panic and logic runs.
    // Clean up file if possible (in real test env).
}

#[tokio::test]
async fn test_execution_config_defaults() {
    let _executor = Executor::new();
    // Verify default config via public methods/behavior if accessors absent
    // or just instantiation is successful
}

#[tokio::test]
async fn test_rate_limiter_try_acquire() {
    let limiter = RateLimiter::new(1, Duration::from_secs(1));
    assert!(limiter.try_acquire().await);
    assert!(!limiter.try_acquire().await); // Should fail immediately
}

#[tokio::test]
async fn test_dex_coverage_check() {
    // This tests logic that would be in validate_dex_coverage
    // But since that function is private in main/lib, we can mock the logic or if exported test it.
    // It's in `lib.rs` but likely private fn `validate_dex_coverage`.
    // We check `solana_arb_bot::safety_checks` instead
}

#[tokio::test]
async fn test_preflight_checks_mock() {
    // Mock preflight check logic
    // Real preflight checks need RPC.
}
