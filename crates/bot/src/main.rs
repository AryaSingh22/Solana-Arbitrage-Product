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
mod jito;

use solana_arb_core::{
    arbitrage::ArbitrageDetector,
    config::Config,
    dex::{DexManager, jupiter::JupiterProvider, raydium::RaydiumProvider, orca::OrcaProvider},
    pathfinder::PathFinder,
    risk::{RiskManager, RiskConfig, TradeDecision, TradeOutcome},
    flash_loan::{FlashLoanProvider, MockFlashLoanProvider},
    history::HistoryRecorder,
    DexType, TokenPair,
};
use wallet::Wallet;
use execution::Executor;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use crate::execution::{SOL_MINT, USDC_MINT, RAY_MINT, ORCA_MINT};

/// Trading bot state
struct BotState {
    detector: ArbitrageDetector,
    path_finder: PathFinder,
    risk_manager: RiskManager,
    dex_manager: DexManager,
    executor: Executor,
    wallet: Wallet,
    flash_loan_provider: Box<dyn FlashLoanProvider>,
    history_recorder: HistoryRecorder,
    is_running: bool,
    dry_run: bool,
    rpc_url: String,
    max_price_age_seconds: i64,
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

        // Initialize Flash Loan Provider
        let flash_loan_provider = Box::new(MockFlashLoanProvider::new("Solend-Mock"));
        info!("🏦 Initialized Flash Loan Provider: {}", flash_loan_provider.name());

        let temp_session_id = format!("SESSION-{}", Utc::now().format("%Y%m%d-%H%M%S"));
        let history_file = if dry_run { "data/history-sim.jsonl" } else { "data/history-live.jsonl" };
        let history_recorder = HistoryRecorder::new(history_file, &temp_session_id);
        info!("📜 Trade history will be saved to: {}", history_file);

        Self {
            detector: ArbitrageDetector::default(),
            path_finder: PathFinder::new(4),
            risk_manager: RiskManager::new(risk_config),
            dex_manager,
            executor: Executor::with_config(execution::ExecutionConfig {
                priority_fee_micro_lamports: config.priority_fee_micro_lamports,
                compute_unit_limit: config.compute_unit_limit,
                slippage_bps: config.slippage_bps,
                max_retries: config.max_retries,
                rpc_commitment: config.rpc_commitment.clone(),
            }),
            wallet: Wallet::new().expect("Failed to load wallet"),
            flash_loan_provider,
            history_recorder,
            is_running: true,
            dry_run,
            rpc_url: config.solana_rpc_url.clone(),
            max_price_age_seconds: config.max_price_age_seconds,
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
                use rand::seq::SliceRandom;
                use rand::Rng;
                let mut rng = rand::thread_rng();

                // 80% chance to find an opportunity
                if rng.gen_bool(0.8) {
                    if let Some(pair) = pairs.choose(&mut rng) {
                        let dexs = vec![DexType::Raydium, DexType::Orca, DexType::Jupiter];
                        
                        // Pick two different DEXs
                        let buy_dex = dexs.choose(&mut rng).unwrap();
                        let mut sell_dex = dexs.choose(&mut rng).unwrap();
                        while sell_dex == buy_dex {
                            sell_dex = dexs.choose(&mut rng).unwrap();
                        }

                        let profit_basis = rng.gen_range(50..450); // 0.50 to 4.50
                        let profit_pct = Decimal::new(profit_basis, 2);
                        let size = Decimal::from(rng.gen_range(50..500));
                        let est_profit = (size * profit_pct) / Decimal::from(100);

                        info!(
                            "🧪 Synthetic arbitrage injected: {} on {:?} -> {:?}, profit {}% (${})",
                            pair, buy_dex, sell_dex, profit_pct, est_profit.round_dp(2)
                        );
                        
                        let synthetic_opp = solana_arb_core::ArbitrageOpportunity {
                            id: solana_arb_core::Uuid::new_v4(),
                            pair: pair.clone(),
                            buy_dex: buy_dex.clone(),
                            sell_dex: sell_dex.clone(),
                            buy_price: Decimal::new(100, 0), // Dummy
                            sell_price: Decimal::new(100, 0) + (Decimal::new(100, 0) * profit_pct / Decimal::from(100)),
                            gross_profit_pct: profit_pct,
                            net_profit_pct: profit_pct,
                            estimated_profit_usd: Some(est_profit),
                            recommended_size: Some(size),
                            detected_at: Utc::now(),
                            expired_at: None,
                        };
                        opps.push(synthetic_opp);
                    }
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
        let max_age = state.max_price_age_seconds;
        state.detector.clear_stale_prices(max_age);

        // Update pathfinder
        state.path_finder.clear();
        for price in &prices {
            state.path_finder.add_price(price);
        }
    }

    validate_dex_coverage(&prices, pairs);

    Ok(())
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
            .filter(|dex| seen.map_or(true, |set| !set.contains(dex)))
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

/// Execute a trade (or simulate in dry-run mode)
async fn execute_trade(
    state: &Arc<RwLock<BotState>>,
    opp: &solana_arb_core::ArbitrageOpportunity,
) {
    let pair_symbol = opp.pair.symbol();
    
    // We need to release the read lock before acquiring write lock later, 
    // AND calling async execution which shouldn't hold locks if possible.
    // However, Executor is stateless (HttpClient) so we can clone data needed.

    let (is_dry_run, decision, rpc_url) = {
        let state = state.read().await;
        let decision = state.risk_manager.can_trade(&pair_symbol, Decimal::from(100));
        (state.dry_run, decision, state.rpc_url.clone())
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

    // Check Flash Loan Viability
    let flash_loan_quote = {
        let state_read = state.read().await;
        if let Some(mint) = resolve_mint(&opp.pair.base) { // Assume borrowing base asset
             match state_read.flash_loan_provider.get_quote(mint, size).await {
                Ok(quote) => {
                    let total_profit_usd = (size * opp.net_profit_pct) / Decimal::from(100);
                    // Assuming quote.fee is in same denomination as amount (base currency)
                    // We need to convert fee to USD to compare with profit, or profit to base.
                    // Simplified: fee is in base token.
                    // If base is SOL ($100), fee 0.09% = 0.0009 SOL.
                    // Profit is % of size.
                    
                    let fee_pct = (quote.fee / size) * Decimal::from(100);
                    
                    if opp.net_profit_pct > fee_pct {
                         info!(
                            "⚡ Flash Loan Viable! Borrowing {} {} costs {} {} ({:.4}%) - Net edge: {:.4}%",
                            size, opp.pair.base, quote.fee, opp.pair.base, fee_pct, opp.net_profit_pct - fee_pct
                        );
                        Some(quote)
                    } else {
                        debug!("Flash Loan fee too high: {:.4}% > {:.4}% profit", fee_pct, opp.net_profit_pct);
                        None
                    }
                },
                Err(e) => {
                    warn!("Failed to get flash loan quote: {}", e);
                    None
                }
             }
        } else {
            None
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
            if let Err(e) = state_read
                .executor
                .execute(&state_read.wallet, opp, size, false, &rpc_url)
                .await
            {
                warn!("Simulation execution failed: {}", e);
            }
        }

        // Record simulation history
        {
            let state_read = state.read().await;
            let est_profit = (size * opp.net_profit_pct) / Decimal::from(100);
            state_read.history_recorder.record_trade(
                opp,
                size,
                est_profit,
                true,
                None,
                None,
                true
            );
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
            state_read
                .executor
                .execute(&state_read.wallet, opp, size, true, &rpc_url)
                .await
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
                
                // Record history
                {
                    let state_read = state.read().await;
                    let est_profit = (size * opp.net_profit_pct) / Decimal::from(100);
                    state_read.history_recorder.record_trade(
                        opp,
                        size,
                        est_profit,
                        true,
                        Some(tx_signature.to_string()),
                        None,
                        false
                    );
                }

                let mut state = state.write().await;
                state.risk_manager.record_trade(outcome);
            }
            Err(e) => {
                error!("❌ Trade failed: {}", e);
                
                // Record failure history
                {
                    let state_read = state.read().await;
                    state_read.history_recorder.record_trade(
                        opp,
                        size,
                        Decimal::ZERO,
                        false,
                        None,
                        Some(e.to_string()),
                        false
                    );
                }

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
    // Load config first
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "solana_arb_bot=info".into())
        )
        .init();
    
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
    info!("   Priority fee: {} µL/CU", config.priority_fee_micro_lamports);
    info!("   Slippage tolerance: {} bps", config.slippage_bps);
    info!("   RPC commitment: {}", config.rpc_commitment);
    info!("   Max retries: {}", config.max_retries);
    info!("   RPC URL: {}", config.solana_rpc_url);

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
