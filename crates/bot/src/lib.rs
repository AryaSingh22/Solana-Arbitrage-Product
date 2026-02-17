//! Solana Arbitrage Trading Bot
//!
//! Automated trading bot that executes arbitrage opportunities.

use anyhow::Result;
use chrono::Utc;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive; // Needed for from_f64
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use solana_arb_core::events::{EventBus, TradingEvent};

pub mod execution;
pub mod wallet;
// mod jito; // Migrated to core
pub mod api;
pub mod config_manager;
pub mod flash_loan_tx_builder;
pub mod logging;
pub mod metrics;
pub mod alerts;
pub mod safety_checks;
pub mod solend_config;

use crate::alerts::AlertManager;
use crate::config_manager::ConfigManager;
use crate::safety_checks::run_preflight_checks;
use axum::{routing::get, Json, Router};
use execution::{Executor, ORCA_MINT, RAY_MINT, SOL_MINT, USDC_MINT};
use serde_json::json;
use std::time::Instant;
use metrics::prometheus::MetricsCollector;
use solana_arb_core::{
    alt::AltManager,
    arbitrage::ArbitrageDetector,
    config::Config,
    dex::{jupiter::JupiterProvider, orca::OrcaProvider, raydium::RaydiumProvider, DexManager},
    history::HistoryRecorder,
    jito::JitoClient,
    pathfinding::PathFinder,
    pricing::parallel_fetcher::ParallelPriceFetcher,
    rate_limiter::RateLimiter,
    risk::{RiskConfig, RiskManager, TradeDecision, TradeOutcome},
    types::TradeResult,
    DexType, TokenPair,
};
use solana_arb_dex_plugins::{LifinityProvider, MeteoraProvider, PhoenixProvider};
use solana_arb_flash_loans::solend::SolendFlashLoan;
use solana_arb_flash_loans::FlashLoanProvider;
use solana_arb_strategies::{LatencyArbitrage, StatisticalArbitrage, Strategy};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use wallet::Wallet;

/// System health status
#[derive(Clone, Debug)]
pub struct SystemHealth {
    pub is_running: bool,
    pub last_opportunity_time: Option<Instant>,
    pub total_trades: u64,
    pub circuit_breaker_state: String,
    pub balance_usd: f64,
    pub start_time: Instant,
}

impl Default for SystemHealth {
    fn default() -> Self {
        Self {
            is_running: true,
            last_opportunity_time: None,
            total_trades: 0,
            circuit_breaker_state: "Closed".to_string(),
            balance_usd: 0.0,
            start_time: Instant::now(),
        }
    }
}

/// Trading bot state holding all component instances and shared data.
#[allow(dead_code)]
struct BotState {
    /// Service for detecting arbitrage opportunities.
    detector: ArbitrageDetector,
    /// Service for finding profitable paths.
    path_finder: PathFinder,
    /// Risk management system.
    risk_manager: RiskManager,
    /// Manager for decentralized exchanges.
    dex_manager: DexManager,
    /// Service for fetching token prices.
    price_fetcher: ParallelPriceFetcher,
    /// Component for executing trades.
    executor: Executor,
    /// Wallet for signing transactions.
    wallet: Wallet,
    /// Provider for flash loans.
    flash_loan_provider: Box<dyn FlashLoanProvider>,
    /// Recorder for trade history.
    history_recorder: HistoryRecorder,
    /// Optional Jito client for MEV protection.
    jito_client: Option<JitoClient>,
    /// Address Lookup Table (ALT) manager.
    alt_manager: Arc<AltManager>,
    /// List of active trading strategies.
    strategies: Vec<Box<dyn Strategy>>,
    /// Whether the bot is currently running.
    is_running: bool,
    /// Whether the bot is in dry-run mode.
    dry_run: bool,
    /// RPC URL for Solana connection.
    rpc_url: String,
    /// Maximum age of price data in seconds.
    max_price_age_seconds: i64,
    /// Metrics collector.
    metrics: Arc<MetricsCollector>,
    /// Alert manager for notifications.
    alert_manager: AlertManager,
    /// Shared system health status.
    system_health: Arc<RwLock<SystemHealth>>,
    /// Event bus for system-wide events.
    event_bus: Arc<EventBus>,
    /// Counter for consecutive errors.
    consecutive_errors: u32,
    /// Rate limiter for RPC requests.
    rpc_rate_limiter: Arc<RateLimiter>,
    /// Rate limiter for Jupiter API requests.
    jupiter_rate_limiter: Arc<RateLimiter>,
    /// Dynamic configuration manager.
    config_manager: Arc<ConfigManager>,
}

impl BotState {
    /// Creates a new BotState instance, initializing all components (DexManager, RiskManager, Executor, etc.).
    ///
    /// # Arguments
    ///
    /// * `config` - Application configuration
    /// * `dry_run` - Whether to run in simulation mode
    /// * `metrics` - Metrics collector
    /// * `alert_manager` - Alert manager
    /// * `system_health` - Shared system health status
    fn new(
        config: &Config,
        dry_run: bool,
        metrics: Arc<MetricsCollector>,
        alert_manager: AlertManager,
        system_health: Arc<RwLock<SystemHealth>>,
        config_manager: Arc<ConfigManager>,
    ) -> Self {
        let risk_config = RiskConfig {
            max_position_size: Decimal::from(1000),
            max_total_exposure: Decimal::from(5000),
            max_daily_loss: Decimal::from(100),
            min_profit_threshold: config
                .min_profit_threshold
                .try_into()
                .unwrap_or(Decimal::new(5, 3)),
            ..Default::default()
        };

        let mut dex_manager = DexManager::new();

        // Register DEX providers
        dex_manager.add_provider(Arc::new(JupiterProvider::new()));
        info!("🔌 Registered DEX provider: Jupiter");

        dex_manager.add_provider(Arc::new(RaydiumProvider::new()));
        info!("🔌 Registered DEX provider: Raydium");

        dex_manager.add_provider(Arc::new(OrcaProvider::new()));
        info!("🔌 Registered DEX provider: Orca");

        dex_manager.add_provider(Arc::new(LifinityProvider::new()));
        info!("🔌 Registered DEX provider: Lifinity");

        dex_manager.add_provider(Arc::new(MeteoraProvider::new()));
        info!("🔌 Registered DEX provider: Meteora");

        dex_manager.add_provider(Arc::new(PhoenixProvider::new()));
        info!("🔌 Registered DEX provider: Phoenix");

        info!(
            "🔌 DexManager initialized with {} providers",
            dex_manager.providers().len()
        );

        let price_fetcher = ParallelPriceFetcher::new(dex_manager.providers().to_vec());

        // Initialize Flash Loan Provider (Solend)
        // For now using USDC reserve placeholder - in prod this would be dynamic or config based
        // Safety: this is a valid base58-encoded Solana pubkey constant
        let usdc_reserve =
            Pubkey::from_str("BgxfHJDzm44T7XG68MYKx7YisTjZu73tVovyZSjJMpmw")
                .expect("Mainnet USDC reserve is a valid pubkey constant");
        let flash_loan_provider = Box::new(SolendFlashLoan::new(usdc_reserve));
        info!(
            "🏦 Initialized Flash Loan Provider: {}",
            flash_loan_provider.name()
        );

        let temp_session_id = format!("SESSION-{}", Utc::now().format("%Y%m%d-%H%M%S"));
        let history_file = if dry_run {
            "data/history-sim.jsonl"
        } else {
            "data/history-live.jsonl"
        };
        let history_recorder = HistoryRecorder::new(history_file, &temp_session_id);
        info!("📜 Trade history will be saved to: {}", history_file);

        // Initialize Jito Client (Optional)
        let jito_client = if std::env::var("USE_JITO").unwrap_or("false".to_string()) == "true" {
            let engine_url = std::env::var("JITO_BLOCK_ENGINE_URL")
                .unwrap_or("https://mainnet.block-engine.jito.wtf".to_string());
            let tip = std::env::var("JITO_TIP_LAMPORTS")
                .unwrap_or("100000".to_string())
                .parse()
                .unwrap_or(100000);
            info!(
                "🛡️ Jito MEV Protection enabled (Engine: {}, Tip: {} lamports)",
                engine_url, tip
            );
            Some(JitoClient::new(&engine_url, tip))
        } else {
            info!("⚠️ Jito MEV Protection DISABLED");
            None
        };

        // Initialize ALT Manager
        let alt_manager = Arc::new(AltManager::new(&config.solana_rpc_url));
        info!("📇 Address Lookup Table (ALT) Manager initialized");

        // Initialize Strategies
        let mut strategies: Vec<Box<dyn Strategy>> = Vec::new();

        // Statistical Arbitrage (Window: 20 ticks, Z-score: 2.0)
        strategies.push(Box::new(StatisticalArbitrage::new(20, Decimal::new(20, 1))));
        info!("🧠 Strategy initialized: Statistical Arbitrage");

        // Latency Arbitrage
        strategies.push(Box::new(LatencyArbitrage::new()));
        info!("🧠 Strategy initialized: Latency Arbitrage");

        let mut executor = Executor::with_config(execution::ExecutionConfig {
            priority_fee_micro_lamports: config.priority_fee_micro_lamports,
            compute_unit_limit: config.compute_unit_limit,
            slippage_bps: config.slippage_bps,
            max_retries: config.max_retries,
            rpc_commitment: config.rpc_commitment.clone(),
        });
        
        // Initialize Rate Limiters
        // RPC: 10 requests/second (conservative default)
        let rpc_rate_limiter = Arc::new(RateLimiter::per_second(10));
        // Jupiter: 5 requests/second (public API limit)
        let jupiter_rate_limiter = Arc::new(RateLimiter::per_second(5));

        executor.set_rate_limiters(
            Some(rpc_rate_limiter.clone()), 
            Some(jupiter_rate_limiter.clone())
        );

        executor.set_alt_manager(alt_manager.clone());

        Self {
            detector: ArbitrageDetector::default(),
            path_finder: PathFinder::new(4),
            risk_manager: RiskManager::new(risk_config),
            dex_manager,
            price_fetcher,
            executor,
            wallet: Wallet::new().expect("Failed to load wallet"),
            flash_loan_provider,
            history_recorder,
            jito_client,
            alt_manager,
            strategies,
            is_running: true,
            dry_run,
            rpc_url: config.solana_rpc_url.clone(),
            max_price_age_seconds: config.max_price_age_seconds,
            metrics,
            alert_manager,
            system_health,
            event_bus: Arc::new(EventBus::new(1000)),
            consecutive_errors: 0,
            rpc_rate_limiter,
            jupiter_rate_limiter,
            config_manager,
        }
    }
    


    /// Check risk parameters and calculate position size
    async fn check_risk_and_size(&self, opp: &solana_arb_core::ArbitrageOpportunity) -> (bool, TradeDecision, String) {
        let optimal_size = self.risk_manager.calculate_position_size(
            &opp.pair.symbol(),
            opp.net_profit_pct,
            Decimal::from(10000), // Assume high liquidity for now or get from opp
        );

        let decision = self
            .risk_manager
            .can_trade(&opp.pair.symbol(), optimal_size)
            .await;
            
        (self.dry_run, decision, self.rpc_url.clone())
    }

    /// Check if a flash loan is viable and return the quote if so
    async fn check_flash_loan(&self, opp: &solana_arb_core::ArbitrageOpportunity, size: Decimal) -> Option<solana_arb_flash_loans::FlashLoanQuote> {
        if let Some(mint) = resolve_mint(&opp.pair.base) {
            // Assume borrowing base asset
            match self.flash_loan_provider.get_quote(mint, size).await {
                Ok(quote) => {
                    // Simplified: fee is in base token.
                    let fee_pct = (quote.fee / size) * Decimal::from(100);

                    if opp.net_profit_pct > fee_pct {
                        info!(
                            "⚡ Flash Loan Viable! Borrowing {} {} costs {} {} ({:.4}%) - Net edge: {:.4}%",
                            size, opp.pair.base, quote.fee, opp.pair.base, fee_pct, opp.net_profit_pct - fee_pct
                        );
                        Some(quote)
                    } else {
                        debug!(
                            "Flash Loan fee too high: {:.4}% > {:.4}% profit",
                            fee_pct, opp.net_profit_pct
                        );
                        None
                    }
                }
                Err(e) => {
                    warn!("Failed to get flash loan quote: {}", e);
                    None
                }
            }
        } else {
            None
        }
    }

    /// Record trade outcome to all systems (Metrics, History, Risk, EventBus)
    async fn record_trade_outcome(
        &self,
        opp: &solana_arb_core::ArbitrageOpportunity,
        pair_symbol: &str,
        size: Decimal,
        outcome: &TradeResult,
        start_time: Instant,
    ) -> TradeOutcome { // Added return type
        let execution_time_ms = start_time.elapsed().as_millis() as u64;
        let success = outcome.success;

        // 1. Metrics
        let metrics = &self.metrics;
        if success {
            metrics.trades_successful.inc();
            metrics.trade_execution_time.observe(start_time.elapsed().as_secs_f64());
            if let Some(profit_f64) = opp.net_profit_pct.to_f64() {
                metrics.opportunity_profit.observe(profit_f64);
            }
        } else {
            metrics.trades_failed.inc();
        }

        // 2. EventBus
        let profit_usd = if success {
             opp.estimated_profit_usd.unwrap_or_default().to_f64().unwrap_or(0.0)
        } else {
             0.0
        };
        
        self.event_bus.publish(TradingEvent::TradeExecuted {
            id: opp.id.to_string(),
            pair: pair_symbol.to_string(),
            success,
            profit: profit_usd,
            execution_time_ms,
        });

        // 3. History Recorder
        let (est_profit, tx_sig, error_msg) = if success {
             (
                 (size * opp.net_profit_pct) / Decimal::from(100),
                 outcome.signature.clone(),
                 None
             )
        } else {
             (
                 Decimal::ZERO,
                 None,
                 outcome.error.clone().or(Some("Unknown error".to_string()))
             )
        };
        
        self.history_recorder.record_trade(
            opp,
            size,
            est_profit,
            success,
            tx_sig,
            error_msg,
            false,
        );

        // 4. Return outcome for Risk Manager
        TradeOutcome {
            timestamp: Utc::now(),
            pair: pair_symbol.to_string(),
            profit_loss: est_profit,
            was_successful: success,
        }
    }
}

/// Main trading loop that orchestrates price collection, opportunity detection, and execution.
///
/// Runs indefinitely until a stop signal is received or a critical error occurs.
async fn run_trading_loop(state: Arc<RwLock<BotState>>, pairs: Vec<TokenPair>) {
    info!("🤖 Trading bot started");

    // Publish startup event
    {
        let s = state.read().await;
        s.event_bus.publish(TradingEvent::SystemStarted {
            mode: if s.dry_run { "dry-run".to_string() } else { "live".to_string() },
        });
    }

    // Spawn event logger subscriber
    {
        let mut event_rx = state.read().await.event_bus.subscribe();
        tokio::spawn(async move {
            while let Ok(event) = event_rx.recv().await {
                match &event {
                    TradingEvent::TradeExecuted { id, success, profit, .. } => {
                        if *success {
                            tracing::info!(id, profit, "📈 Event: trade executed");
                        } else {
                            tracing::warn!(id, "📉 Event: trade failed");
                        }
                    }
                    TradingEvent::CircuitBreakerStateChanged { new_state, .. } => {
                        tracing::warn!(state = %new_state, "⚡ Event: circuit breaker changed");
                    }
                    TradingEvent::EmergencyStop { reason } => {
                        tracing::error!(reason, "🛑 Event: EMERGENCY STOP");
                    }
                    _ => {
                        tracing::debug!(event = ?event, "Event received");
                    }
                }
            }
        });
    }

    let mut tick = 0u64;
    let mut last_balance_check = Instant::now();

    loop {
        // 1. Check Kill Switch
        if std::path::Path::new(".kill").exists() {
            let state = state.read().await;
            state.alert_manager.send_critical("🛑 Kill switch (.kill) detected - shutting down").await;
            info!("Kill switch file detected - graceful shutdown");
            
            // Close all positions logic could go here
            
            // Update health
            let mut health = state.system_health.write().await;
            health.is_running = false;
            
            break;
        }

        // 2. Wrap main logic in async block for error handling
        let loop_result = async {
            // Check if still running (internal state)
            {
                let state = state.read().await;
                if !state.is_running {
                    return Ok::<_, anyhow::Error>(false); // Stop signal
                }
                
                // Fetch dynamic config
                let dynamic_config = state.config_manager.get().await;
                if !dynamic_config.trading.enabled {
                    info!("⏸️ Trading disabled via dynamic config. Sleeping...");
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    return Ok(true);
                }
            }

            tick += 1;

            // Every 10 ticks, log status
            if tick.is_multiple_of(10) {
                let state = state.read().await;
                let status = state.risk_manager.status().await;
                info!(
                    "📊 Status - Exposure: ${:.2}, VaR (95%): ${:.2}, P&L: ${:.2}, Trades: {}, Paused: {}",
                    status.total_exposure,
                    status.portfolio_var,
                    status.daily_pnl,
                    status.trades_today,
                    status.is_paused
                );
                
                // Update Health
                let mut health = state.system_health.write().await;
                health.circuit_breaker_state = if status.is_paused { "Open".to_string() } else { "Closed".to_string() };
                health.total_trades = status.trades_today as u64;
            }

            let start = std::time::Instant::now();

            // Collect prices
            let recent_prices = match collect_prices(&state, &pairs).await {
                Ok(p) => {
                    // Reset consecutive errors on success
                    state.write().await.consecutive_errors = 0;
                    p
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to collect prices: {}", e));
                }
            };

            {
                let state = state.read().await;
                state
                    .metrics
                    .price_fetch_latency
                    .observe(start.elapsed().as_secs_f64());
            }

            // Find and evaluate opportunities
            let opportunities = {
                let state = state.read().await;
                let mut opps = state.detector.find_all_opportunities();
                let _paths = state.path_finder.find_all_profitable_paths();

                state
                    .metrics
                    .opportunities_detected
                    .inc_by(opps.len() as u64);
                
                // Execute Strategies
                for strategy in &state.strategies {
                    if let Ok(strategy_opps) = strategy.analyze(&recent_prices).await {
                         opps.extend(strategy_opps);
                    }
                }
                opps
            };

            if !opportunities.is_empty() {
                let state_read = state.read().await;
                let mut health = state_read.system_health.write().await;
                health.last_opportunity_time = Some(Instant::now());
            }

            // Execute best opportunity
            for opp in opportunities.iter().take(1) {
                // ... (Execution logic same as before, calling execute_trade)
                 let should_execute = {
                    let state = state.read().await;
                    let config = state.config_manager.get().await;
                    let min_profit_bps = Decimal::from_f64(config.trading.min_profit_bps).unwrap_or_default();
                    let min_profit_pct = min_profit_bps / Decimal::from(100);

                    if opp.net_profit_pct < min_profit_pct {
                         debug!("Skipping opportunity: Profit {}% < Min {}%", opp.net_profit_pct, min_profit_pct);
                        false
                    } else {
                        let optimal_size = state.risk_manager.calculate_position_size(
                            &opp.pair.symbol(),
                            opp.net_profit_pct,
                            Decimal::from(10000),
                        );
                        let decision = state.risk_manager.can_trade(&opp.pair.symbol(), optimal_size).await;
                        matches!(decision, TradeDecision::Approved { .. } | TradeDecision::Reduced { .. })
                    }
                };

                if should_execute {
                    execute_trade(&state, opp).await;
                }
            }

            // Balance Check
            if last_balance_check.elapsed() > Duration::from_secs(600) {
                 last_balance_check = Instant::now();
                 // Logic to check balance
                 let (rpc_url, pubkey_str, alert_manager) = {
                     let state = state.read().await;
                     (state.rpc_url.clone(), state.wallet.pubkey(), state.alert_manager.clone())
                 };
                 
                 // Spawn check
                 let state_clone = state.clone();
                 tokio::spawn(async move {
                     use solana_rpc_client::nonblocking::rpc_client::RpcClient;
                     use solana_sdk::pubkey::Pubkey;
                     let client = RpcClient::new(rpc_url);
                     if let Ok(pubkey) = Pubkey::from_str(&pubkey_str) {
                         if let Ok(balance) = client.get_balance(&pubkey).await {
                             let balance_sol = balance as f64 / 1_000_000_000.0;
                             
                             // Get system_health Arc and drop state lock
                             let system_health = {
                                 let state = state_clone.read().await;
                                 state.system_health.clone()
                             };

                             // Update health
                             {
                                 let mut h = system_health.write().await;
                                 // Approximation: 1 SOL = $150 (should fetch real price)
                                 h.balance_usd = balance_sol * 150.0; 
                             }

                             if balance_sol < 0.1 {
                                 alert_manager.send_critical(&format!("⚠️ Low balance: {:.4} SOL", balance_sol)).await;
                             }
                         }
                     }
                 });
            }

            Ok(true) // Continue running
        }.await;

        match loop_result {
            Ok(should_continue) => {
                if !should_continue {
                    break;
                }
            }
            Err(e) => {
                // Classify error severity for appropriate response
                let err_str = e.to_string();
                let is_retryable = err_str.contains("timeout") || err_str.contains("rate limit");

                if err_str.contains("Daily loss") || err_str.contains("Circuit breaker") {
                    error!("🔴 CRITICAL error in main loop: {}", e);
                    let state_r = state.read().await;
                    state_r.alert_manager.send_critical(&format!("CRITICAL: {}", e)).await;
                } else if is_retryable {
                    warn!("⚠️ Retryable error in main loop: {}", e);
                } else {
                    error!("❌ Error in main loop: {}", e);
                }

                // Track consecutive errors
                let consecutive = {
                    let mut state_w = state.write().await;
                    state_w.consecutive_errors += 1;
                    state_w.consecutive_errors
                };

                // Exponential backoff based on consecutive error count
                let backoff = Duration::from_secs(2u64.pow(consecutive.min(5)));
                debug!(consecutive, backoff_secs = backoff.as_secs(), "Backing off");
                tokio::time::sleep(backoff).await;
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Collects recent price data from all registered DEX providers.
///
/// Updates the local state with new prices, clears stale data, and updates
/// strategy internal state.
async fn collect_prices(
    state: &Arc<RwLock<BotState>>,
    pairs: &[TokenPair],
) -> Result<Vec<solana_arb_core::PriceData>, Box<dyn std::error::Error>> {
    let prices = {
        let state = state.read().await;

        // Use parallel fetcher for all pairs at once!
        let all_prices = state.price_fetcher.fetch_all_prices(pairs).await;
        info!(
            "💓 Parallel fetch complete — {} prices collected",
            all_prices.len()
        );
        all_prices
    };

    info!("📈 Received price data from DEX ({} prices)", prices.len());

    // Update state
    {
        let mut state = state.write().await;

        // Update detector
        state.detector.update_prices(prices.clone());
        let max_age = state.max_price_age_seconds;
        state.detector.clear_stale_prices(max_age);

        // Update pathfinder
        state.path_finder.clear();
        for price in &prices {
            state.path_finder.add_price(price);
        }

        // Update risk manager volatility tracking
        state.risk_manager.update_prices(&prices);

        // Update strategies
        for strategy in &state.strategies {
            for price in &prices {
                if let Err(e) = strategy.update_state(price).await {
                    warn!("Strategy {} update failed: {}", strategy.name(), e);
                }
            }
        }
    }

    validate_dex_coverage(&prices, pairs);

    Ok(prices)
}

fn validate_dex_coverage(prices: &[solana_arb_core::PriceData], pairs: &[TokenPair]) {
    let mut coverage: std::collections::HashMap<String, std::collections::HashSet<DexType>> =
        std::collections::HashMap::new();

    for price in prices {
        coverage
            .entry(price.pair.symbol())
            .or_default()
            .insert(price.dex);
    }

    for pair in pairs {
        let seen = coverage.get(&pair.symbol());
        let missing: Vec<_> = DexType::all()
            .iter()
            .filter(|dex| seen.is_none_or(|set| !set.contains(dex)))
            .collect();

        if !missing.is_empty() {
            let missing_labels: Vec<_> = missing.iter().map(|dex| dex.display_name()).collect();
            warn!(
                "⚠️ Missing DEX coverage for {}: {}",
                pair,
                missing_labels.join(", ")
            );
        }
    }
}

/// Executes a detected arbitrage opportunity.
///
/// This function handles:
/// 1. Risk checks and position sizing
/// 2. Flash loan viability analysis
/// 3. Dry-run simulation (if enabled)
/// 4. Actual trade execution via the Executor
/// 5. Outcome recording (Metrics, History, Risk Manager)
async fn execute_trade(state: &Arc<RwLock<BotState>>, opp: &solana_arb_core::ArbitrageOpportunity) {
    let start_time = std::time::Instant::now();
    let pair_symbol = opp.pair.symbol();

    // We need to release the read lock before acquiring write lock later,
    // AND calling async execution which shouldn't hold locks if possible.
    // However, Executor is stateless (HttpClient) so we can clone data needed.

    let (is_dry_run, decision, rpc_url) = {
        let state = state.read().await;
        state.check_risk_and_size(opp).await
    };

    let size = match decision {
        TradeDecision::Approved { size } => size,
        TradeDecision::Reduced { new_size, reason } => {
            info!("Trade size reduced: {}", reason);
            new_size
        }
        TradeDecision::Rejected { reason } => {
            debug!("Trade rejected: {}", reason);
            return;
        }
    };

    // Record attempt
    {
        let state = state.read().await;
        state.metrics.trades_attempted.inc();
    }

    // Check Flash Loan Viability
    let _flash_loan_quote = {
        let state_read = state.read().await;
        state_read.check_flash_loan(opp, size).await
    };

    if is_dry_run {
        // Simulate trade
        info!(
            "🔵 [DRY RUN] Would execute: Buy {} on {}, Sell on {} | Size: ${} | Profit: {}%",
            pair_symbol, opp.buy_dex, opp.sell_dex, size, opp.net_profit_pct
        );

        // Fetch quote simulation (optional)
        {
            let state_read = state.read().await;
            if let Err(e) = state_read
                .executor
                .execute(&state_read.wallet, opp, size, false, &rpc_url, None)
                .await
            {
                warn!("Simulation execution failed: {}", e);
            }
        }

        // Record simulation history
        {
            let state_read = state.read().await;
            let est_profit = (size * opp.net_profit_pct) / Decimal::from(100);
            state_read
                .history_recorder
                .record_trade(opp, size, est_profit, true, None, None, true);
        }

        // Simulate successful outcome
        let outcome = TradeOutcome {
            timestamp: Utc::now(),
            pair: pair_symbol,
            profit_loss: size * opp.net_profit_pct / Decimal::from(100),
            was_successful: true,
        };

        let mut state = state.write().await;
        state.risk_manager.record_trade(outcome).await;
    } else {
        // Real execution via Jupiter API
        info!(
            "🟢 Executing: Buy {} on {}, Sell on {} | Size: ${} | Expected Profit: {}%",
            pair_symbol, opp.buy_dex, opp.sell_dex, size, opp.net_profit_pct
        );

        let result: Result<TradeResult> = {
            let state_read = state.read().await;
            state_read
                .executor
                .execute(
                    &state_read.wallet,
                    opp,
                    size,
                    true,
                    &rpc_url,
                    state_read.jito_client.as_ref(),
                )
                .await
        };

        match result {
            Ok(trade_result) => {
                if trade_result.success {
                    let tx_signature = trade_result.signature.as_deref().unwrap_or("unknown");
                    info!("✅ Trade submitted! Signature: {}", tx_signature);
                } else {
                    let error_msg = trade_result.error.as_deref().unwrap_or("Unknown error");
                    warn!("❌ Trade execution returned failure: {}", error_msg);
                }

                // Record outcome
                let outcome = {
                    let state_read = state.read().await;
                    state_read
                        .record_trade_outcome(opp, &pair_symbol, size, &trade_result, start_time)
                        .await
                };

                // Update Risk Manager
                // Update Risk Manager
                let mut state = state.write().await;
                state.risk_manager.record_trade(outcome).await;
            }
            Err(e) => {
                error!("❌ Trade failed (Executor Error): {}", e);

                // Construct failed TradeResult
                let failed_result = TradeResult {
                    opportunity_id: opp.id,
                    signature: None,
                    success: false,
                    actual_profit: Decimal::ZERO,
                    executed_at: Utc::now(),
                    error: Some(e.to_string()),
                };

                // Record outcome
                let outcome = {
                    let state_read = state.read().await;
                    state_read
                        .record_trade_outcome(opp, &pair_symbol, size, &failed_result, start_time)
                        .await
                };

                // Update Risk Manager
                let mut state = state.write().await;
                state.risk_manager.record_trade(outcome).await;
            }
        }
    }
}

pub async fn run_bot() {
    // Load config first
    dotenvy::dotenv().ok();

    // Initialize logging
    logging::setup();

    // Read MIN_PROFIT_THRESHOLD directly from environment at runtime
    let min_profit_threshold: f64 = std::env::var("MIN_PROFIT_THRESHOLD")
        .unwrap_or_else(|_| "0.5".to_string())
        .parse()
        .expect("Invalid MIN_PROFIT_THRESHOLD value");

    // Create config with runtime-loaded value
    let mut config = Config::from_env().unwrap_or_default();
    config.min_profit_threshold = min_profit_threshold;

    info!("🚀 Solana Arbitrage Bot starting...");
    info!("   Min profit threshold: {}%", min_profit_threshold);
    info!(
        "   Priority fee: {} µL/CU",
        config.priority_fee_micro_lamports
    );
    info!("   Slippage tolerance: {} bps", config.slippage_bps);
    info!("   RPC commitment: {}", config.rpc_commitment);
    info!("   Max retries: {}", config.max_retries);
    info!("   RPC URL: {}", config.solana_rpc_url);

    // Check for dry-run mode
    let dry_run = config.dry_run;

    // Initialize Alert Manager
    let alert_manager = AlertManager::new(
        config.telegram_webhook_url.clone(),
        config.discord_webhook_url.clone(),
    );

    // Alert on startup
    alert_manager.send_info(if dry_run {
        "🚀 ArbEngine-Pro started (Mode: DRY-RUN)"
    } else {
        "🚀 ArbEngine-Pro started (Mode: LIVE TRADING)"
    }).await;

    if dry_run {
        info!("⚠️  Running in DRY RUN mode - no real trades will be executed");
    } else {
        warn!("⚠️  LIVE TRADING MODE - Real trades will be executed!");
    }

    // Initialize RPC client for pre-flight checks
    let rpc_client = solana_rpc_client::nonblocking::rpc_client::RpcClient::new(config.solana_rpc_url.clone());

    // Run Pre-flight Checks
    info!("Running pre-flight safety checks...");
    if let Ok(warnings) = run_preflight_checks(&rpc_client, &config).await {
        if !warnings.is_empty() {
             let mut msg = String::from("⚠️ Pre-flight warnings:\n");
             for w in &warnings {
                 warn!("{}", w);
                 msg.push_str(&format!("- {}\n", w));
             }
             alert_manager.send_critical(&msg).await;
             
             // Wait for user or continue if safe
             if !dry_run {
                 info!("Waiting 10 seconds before continuing...");
                 tokio::time::sleep(Duration::from_secs(10)).await;
             }
        } else {
             info!("✅ All pre-flight checks passed");
        }
    } else {
         error!("❌ Pre-flight checks failed completely");
         // Maybe exit? For now just log
    }

    // Initialize System Health
    let system_health = Arc::new(RwLock::new(SystemHealth::default()));

    // Start Health Check Server
    let health_clone = system_health.clone();
    tokio::spawn(async move {
        let app = Router::new()
            .route("/health", get(|| async {
                Json(json!({
                    "status": "ok",
                    "timestamp": Utc::now().to_rfc3339()
                }))
            }))
            .route("/status", get(move || {
                let health = health_clone.clone();
                async move {
                    let h = health.read().await;
                    Json(json!({
                        "is_running": h.is_running,
                        "total_trades": h.total_trades,
                        "circuit_breaker": h.circuit_breaker_state,
                        "balance_usd": h.balance_usd,
                        "uptime_seconds": h.start_time.elapsed().as_secs()
                    }))
                }
            }));
        
        // Use a different port or 8080 as configured
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
        info!("🏥 Health check server running on http://{}", addr);
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                if let Err(e) = axum::serve(listener, app).await {
                    error!("Health check server error: {}", e);
                }
            }
            Err(e) => error!("Failed to bind health check server on {}: {}", addr, e),
        }
    });

    // Define trading pairs
    let pairs = vec![
        TokenPair::new("SOL", "USDC"),
        TokenPair::new("RAY", "USDC"),
        TokenPair::new("ORCA", "USDC"),
        TokenPair::new("JUP", "USDC"),
    ];

    // Initialize metrics
    let metrics = Arc::new(MetricsCollector::new().expect("Failed to initialize metrics"));

    // Start metrics server
    let metrics_clone = metrics.clone();
    // Default metrics port from config if possible, or 9090
    let metrics_port = config.metrics_port;
    tokio::spawn(async move {
        let app = api::metrics::metrics_routes(metrics_clone);
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], metrics_port));
        info!("📊 Metrics server running on http://{}/metrics", addr);
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                if let Err(e) = axum::serve(listener, app).await {
                    error!("Metrics server error: {}", e);
                }
            }
            Err(e) => error!("Failed to bind metrics server on {}: {}", addr, e),
        }
    });

    // Initialize Config Manager
    let config_path = "config/trading_config.json";
    let config_manager = Arc::new(ConfigManager::new(config_path)
        .unwrap_or_else(|e| {
            warn!("Failed to load dynamic config: {}. Using defaults.", e);
            // Panic or fallback? Since constructor failed (file read/parse), let's try to survive if possible or panic.
            // But ConfigManager::new implies loading.
            panic!("Critical: Failed to load {}: {}", config_path, e);
        }));

    // Start Config Watcher (Polling)
    let cm_clone = config_manager.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            if let Err(e) = cm_clone.reload().await {
                error!("Failed to reload configuration: {}", e);
            }
        }
    });

    // Create bot state
    let state = Arc::new(RwLock::new(BotState::new(
        &config,
        dry_run,
        metrics,
        alert_manager,
        system_health,
        config_manager,
    )));

    // Wire EventBus into RiskManager
    {
        let mut s = state.write().await;
        let event_bus = s.event_bus.clone();
        s.risk_manager.set_event_bus(event_bus).await;
    }

    // Run trading loop
    run_trading_loop(state, pairs).await;
}

/// Resolves a token symbol to its Mint Pubkey.
///
/// Returns `None` if the symbol is not recognized or the constant is invalid.
fn resolve_mint(symbol: &str) -> Option<Pubkey> {
    match symbol {
        "SOL" => Pubkey::from_str(SOL_MINT).ok(),
        "USDC" => Pubkey::from_str(USDC_MINT).ok(),
        "RAY" => Pubkey::from_str(RAY_MINT).ok(),
        "ORCA" => Pubkey::from_str(ORCA_MINT).ok(),
        "JUP" => None, // JUP mint not in constants yet, can add later or ignore
        _ => None,
    }
}
