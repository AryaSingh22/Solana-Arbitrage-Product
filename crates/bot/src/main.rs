//! Solana Arbitrage Trading Bot
//!
//! Automated trading bot that executes arbitrage opportunities.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use rust_decimal::Decimal;
use chrono::Utc;

mod wallet;
mod execution;

use solana_arb_core::{
    arbitrage::ArbitrageDetector,
    config::Config,
    dex::{DexManager, jupiter::JupiterProvider, raydium::RaydiumProvider, orca::OrcaProvider},
    pathfinder::PathFinder,
    risk::{RiskManager, RiskConfig, TradeDecision, TradeOutcome},
    TokenPair,
};
use wallet::Wallet;
use execution::Executor;

/// Trading bot state
struct BotState {
    detector: ArbitrageDetector,
    path_finder: PathFinder,
    risk_manager: RiskManager,
    dex_manager: DexManager,
    executor: Executor,
    wallet: Wallet,
    is_running: bool,
    dry_run: bool,
}

impl BotState {
    fn new(config: &Config, dry_run: bool) -> Self {
        let risk_config = RiskConfig {
            max_position_size: Decimal::from(1000),
            max_total_exposure: Decimal::from(5000),
            max_daily_loss: Decimal::from(100),
            min_profit_threshold: config.min_profit_threshold.try_into().unwrap_or(Decimal::new(5, 3)),
            ..Default::default()
        };

        let mut dex_manager = DexManager::new();
        
        // Register DEX providers
        dex_manager.add_provider(Box::new(JupiterProvider::new()));
        info!("🔌 Registered DEX provider: Jupiter");
        
        dex_manager.add_provider(Box::new(RaydiumProvider::new()));
        info!("🔌 Registered DEX provider: Raydium");
        
        dex_manager.add_provider(Box::new(OrcaProvider::new()));
        info!("🔌 Registered DEX provider: Orca");
        
        info!("🔌 DexManager initialized with {} providers", dex_manager.providers().len());

        Self {
            detector: ArbitrageDetector::default(),
            path_finder: PathFinder::new(4),
            risk_manager: RiskManager::new(risk_config),
            dex_manager,
            executor: Executor::new(),
            wallet: Wallet::new().expect("Failed to load wallet"),
            is_running: true,
            dry_run,
        }
    }
}

/// Main trading loop
async fn run_trading_loop(state: Arc<RwLock<BotState>>, pairs: Vec<TokenPair>) {
    info!("🤖 Trading bot started");
    
    let mut tick = 0u64;
    
    loop {
        info!("🔎 Scanning markets for arbitrage opportunities...");
        // Check if still running
        {
            let state = state.read().await;
            if !state.is_running {
                info!("Bot stopped");
                break;
            }
        }

        tick += 1;
        
        // Every 10 ticks, log status
        if tick % 10 == 0 {
            let state = state.read().await;
            let status = state.risk_manager.status();
            info!(
                "📊 Status - Exposure: ${:.2}, P&L: ${:.2}, Trades: {}, Paused: {}",
                status.total_exposure,
                status.daily_pnl,
                status.trades_today,
                status.is_paused
            );
        }

        // Collect prices
        if let Err(e) = collect_prices(&state, &pairs).await {
            warn!("Failed to collect prices: {}", e);
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }

        // Find and evaluate opportunities
        let opportunities = {
            let state = state.read().await;
            
            // Simple arbitrage opportunities
            let mut opps = state.detector.find_all_opportunities();
            
            // Also check triangular paths
            let paths = state.path_finder.find_all_profitable_paths();
            
            debug!("Found {} simple opportunities, {} triangular paths", opps.len(), paths.len());
            
            // 🧪 Inject synthetic arbitrage in DRY_RUN mode for demo
            if state.dry_run {
                let prices = state.detector.get_prices();
                let price_list: Vec<_> = prices.values().collect();
                
                if price_list.len() >= 2 {
                    let buy = price_list[0];
                    let sell = price_list[1];
                    let synthetic_profit_pct = Decimal::new(12, 2); // 0.12%
                    
                    info!(
                        "🧪 Synthetic arbitrage injected: Buy on {:?}, sell on {:?}, profit {:.2}%",
                        buy.dex,
                        sell.dex,
                        synthetic_profit_pct
                    );
                    
                    let synthetic_opp = solana_arb_core::ArbitrageOpportunity {
                        id: solana_arb_core::Uuid::new_v4(),
                        pair: buy.pair.clone(),
                        buy_dex: buy.dex.clone(),
                        sell_dex: sell.dex.clone(),
                        buy_price: buy.ask,
                        sell_price: sell.bid,
                        gross_profit_pct: synthetic_profit_pct,
                        net_profit_pct: synthetic_profit_pct,
                        estimated_profit_usd: Some(Decimal::new(50, 2)), // $0.50 demo
                        recommended_size: Some(Decimal::from(100)),
                        detected_at: Utc::now(),
                        expired_at: None,
                    };
                    opps.push(synthetic_opp);
                }
            }
            
            opps
        };

        // Execute best opportunity if profitable
        for opp in opportunities.iter().take(1) {
            let should_execute = {
                let state = state.read().await;
                
                // Check profit threshold
                if opp.net_profit_pct < Decimal::new(5, 3) {
                    false
                } else {
                    // Check risk manager
                    let decision = state.risk_manager.can_trade(
                        &opp.pair.symbol(),
                        Decimal::from(100), // Base size
                    );
                    matches!(decision, TradeDecision::Approved { .. } | TradeDecision::Reduced { .. })
                }
            };

            if should_execute {
                execute_trade(&state, opp).await;
            }
        }

        // Sleep before next cycle
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Collect prices from all DEXs
async fn collect_prices(
    state: &Arc<RwLock<BotState>>,
    pairs: &[TokenPair],
) -> Result<(), Box<dyn std::error::Error>> {
    let prices = {
        let state = state.read().await;
        let mut all_prices = Vec::new();
        
        for pair in pairs {
            let pair_prices = state.dex_manager.get_all_prices(pair).await;
            info!("💓 Price heartbeat for {} — {} prices collected", pair, pair_prices.len());
            all_prices.extend(pair_prices);
        }
        
        all_prices
    };

    info!("📈 Received price data from DEX ({} prices)", prices.len());

    // Update state
    {
        let mut state = state.write().await;
        
        // Update detector
        state.detector.update_prices(prices.clone());
        
        // Update pathfinder
        state.path_finder.clear();
        for price in &prices {
            state.path_finder.add_price(price);
        }
    }

    Ok(())
}

/// Execute a trade (or simulate in dry-run mode)
async fn execute_trade(
    state: &Arc<RwLock<BotState>>,
    opp: &solana_arb_core::ArbitrageOpportunity,
) {
    let pair_symbol = opp.pair.symbol();
    
    // We need to release the read lock before acquiring write lock later, 
    // AND calling async execution which shouldn't hold locks if possible.
    // However, Executor is stateless (HttpClient) so we can clone data needed.

    let (is_dry_run, decision) = {
        let state = state.read().await;
        let decision = state.risk_manager.can_trade(&pair_symbol, Decimal::from(100));
        (state.dry_run, decision)
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

    if is_dry_run {
        // Simulate trade
        info!(
            "🔵 [DRY RUN] Would execute: Buy {} on {}, Sell on {} | Size: ${} | Profit: {}%",
            pair_symbol,
            opp.buy_dex,
            opp.sell_dex,
            size,
            opp.net_profit_pct
        );

        // Fetch quote simulation (optional)
        {
            let state_read = state.read().await;
            if let Err(e) = state_read.executor.execute(&state_read.wallet, opp, size).await {
                warn!("Simulation execution failed: {}", e);
            }
        }

        // Simulate successful outcome
        let outcome = TradeOutcome {
            timestamp: Utc::now(),
            pair: pair_symbol,
            profit_loss: size * opp.net_profit_pct / Decimal::from(100),
            was_successful: true,
        };

        let mut state = state.write().await;
        state.risk_manager.record_trade(outcome);
    } else {
        // Real execution via Jupiter API
        info!(
            "🟢 Executing: Buy {} on {}, Sell on {} | Size: ${} | Expected Profit: {}%",
            pair_symbol,
            opp.buy_dex,
            opp.sell_dex,
            size,
            opp.net_profit_pct
        );

        let result = {
             let state_read = state.read().await;
             state_read.executor.execute(&state_read.wallet, opp, size).await
        };

        match result {
            Ok(tx_signature) => {
                info!("✅ Trade submitted! Signature/Transaction: {:.20}...", tx_signature);
                
                // Record success
                 let outcome = TradeOutcome {
                    timestamp: Utc::now(),
                    pair: pair_symbol,
                    profit_loss: size * opp.net_profit_pct / Decimal::from(100), // Estimated
                    was_successful: true,
                };
                let mut state = state.write().await;
                state.risk_manager.record_trade(outcome);
            }
            Err(e) => {
                error!("❌ Trade failed: {}", e);
                // Record failure
                 let outcome = TradeOutcome {
                    timestamp: Utc::now(),
                    pair: pair_symbol,
                    profit_loss: Decimal::ZERO,
                    was_successful: false,
                };
                let mut state = state.write().await;
                state.risk_manager.record_trade(outcome);
            }
        }
    }
}

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "solana_arb_bot=info".into())
        )
        .init();

    // Load config
    dotenvy::dotenv().ok();
    
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

    // Check for dry-run mode
    let dry_run = std::env::var("DRY_RUN")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true); // Default to dry-run for safety

    if dry_run {
        info!("⚠️  Running in DRY RUN mode - no real trades will be executed");
    } else {
        warn!("⚠️  LIVE TRADING MODE - Real trades will be executed!");
    }

    // Define trading pairs
    let pairs = vec![
        TokenPair::new("SOL", "USDC"),
        TokenPair::new("RAY", "USDC"),
        TokenPair::new("ORCA", "USDC"),
        TokenPair::new("JUP", "USDC"),
    ];

    // Create bot state
    let state = Arc::new(RwLock::new(BotState::new(&config, dry_run)));

    // Run trading loop
    run_trading_loop(state, pairs).await;
}
