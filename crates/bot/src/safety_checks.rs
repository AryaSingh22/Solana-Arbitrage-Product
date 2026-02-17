//! Safety Checks Module
//!
//! Provides pre-flight checks and ongoing safety validations for the trading bot.

use anyhow::{Result, anyhow};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_arb_core::config::Config;
use tracing::info;
use std::path::Path;

/// Runs a series of pre-flight checks to ensure the environment is safe for trading.
///
/// Checks include:
/// - RPC connectivity
/// - Configuration validity (dry-run, circuit breaker)
/// - Kill switch presence
pub async fn run_preflight_checks(
    rpc_client: &RpcClient,
    config: &Config
) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    
    // 1. Check RPC connection
    match rpc_client.get_slot().await {
        Ok(slot) => info!("✅ RPC connection OK (Slot: {})", slot),
        Err(e) => return Err(anyhow!("❌ RPC connection failed: {}", e))
    }
    
    // 2. Check balance (if wallet configured)
    // We assume the wallet pubkey is available or we need to derive it from private key in main
    // For now, we'll skip balance check here if we don't pass the pubkey, or we can pass pubkey separately.
    // However, the function signature in the prompt implies we might not have it or it's in config? 
    // Config doesn't have wallet_pubkey. Main does. 
    // I will modify the signature to accept wallet_pubkey.
    
    // 3. Check if in dry-run mode
    if config.dry_run {
        warnings.push("⚠️ Running in DRY-RUN mode (no real trades)".to_string());
    }
    
    // 4. Check circuit breaker config
    if !config.circuit_breaker_enabled {
        warnings.push("⚠️ Circuit breaker DISABLED - no safety limits".to_string());
    }
    
    // 5. Check if using premium RPC
    if config.solana_rpc_url.contains("devnet") || config.solana_rpc_url.contains("testnet") || config.solana_rpc_url.contains("api.mainnet-beta.solana.com") {
        warnings.push("⚠️ Using public/devnet RPC - not for high-frequency trading".to_string());
    }
    
    // 6. Check for kill switch file
    if Path::new(".kill").exists() {
         return Err(anyhow!("❌ Kill switch file (.kill) detected - aborting startup"));
    }

    Ok(warnings)
}
