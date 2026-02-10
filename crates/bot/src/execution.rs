//! Execution Module
//!
//! Handles fetching quotes and swap instructions from aggregators (Jupiter).
//! Implements HTTP-based execution path with priority fees, retry logic,
//! and balance checking for production-ready trading.

use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::HashMap;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcSendTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::message::{Message, VersionedMessage};
use solana_sdk::signature::Signer;
use solana_sdk::transaction::VersionedTransaction;
use tracing::{info, debug, warn, error};

use solana_arb_core::ArbitrageOpportunity;
use crate::wallet::Wallet;

const JUPITER_API_URL: &str = "https://quote-api.jup.ag/v6";

// Token Mints (Mainnet)
// Token Mints (Mainnet)
pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const RAY_MINT: &str = "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R";
pub const ORCA_MINT: &str = "orcaEKTdK7LKz57vaAYr9QeNsVEPfiu6QeMU1kektZE";

/// Execution configuration
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// Priority fee in micro-lamports per compute unit
    pub priority_fee_micro_lamports: u64,
    /// Compute unit limit per transaction
    pub compute_unit_limit: u32,
    /// Slippage tolerance in basis points
    pub slippage_bps: u64,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// RPC commitment level
    pub rpc_commitment: String,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            priority_fee_micro_lamports: 50_000,
            compute_unit_limit: 200_000,
            slippage_bps: 50,
            max_retries: 3,
            rpc_commitment: "confirmed".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Executor {
    client: Client,
    token_map: HashMap<String, String>,
    config: ExecutionConfig,
}

#[derive(Debug, Serialize)]
struct SwapRequest {
    #[serde(rename = "userPublicKey")]
    user_public_key: String,
    #[serde(rename = "quoteResponse")]
    quote_response: serde_json::Value,
    #[serde(rename = "computeUnitPriceMicroLamports")]
    compute_unit_price_micro_lamports: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SwapResponse {
    #[serde(rename = "swapTransaction")]
    swap_transaction: String, // Base64 encoded transaction
}

impl Executor {
    pub fn new() -> Self {
        Self::with_config(ExecutionConfig::default())
    }

    pub fn with_config(config: ExecutionConfig) -> Self {
        let mut token_map = HashMap::new();
        token_map.insert("SOL".to_string(), SOL_MINT.to_string());
        token_map.insert("USDC".to_string(), USDC_MINT.to_string());
        token_map.insert("RAY".to_string(), RAY_MINT.to_string());
        token_map.insert("ORCA".to_string(), ORCA_MINT.to_string());

        Self {
            client: Client::new(),
            token_map,
            config,
        }
    }

    /// Get quote from Jupiter with configurable slippage
    pub async fn get_quote(&self, input_token: &str, output_token: &str, amount: u64) -> Result<serde_json::Value> {
        let input_mint = self.token_map.get(input_token).cloned().unwrap_or_else(|| input_token.to_string());
        let output_mint = self.token_map.get(output_token).cloned().unwrap_or_else(|| output_token.to_string());

        let url = format!(
            "{}/quote?inputMint={}&outputMint={}&amount={}&slippageBps={}",
            JUPITER_API_URL, input_mint, output_mint, amount, self.config.slippage_bps
        );

        debug!("Fetching quote: {}", url);
        let response = self.client.get(&url).send().await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Jupiter quote failed: {}", error_text));
        }
        
        let quote: serde_json::Value = response.json().await?;
        Ok(quote)
    }

    /// Check wallet SOL balance before executing
    pub fn check_balance(&self, wallet: &Wallet, rpc_url: &str) -> Result<u64> {
        let signer = wallet
            .signer()
            .ok_or_else(|| anyhow!("No keypair available — cannot check balance"))?;
        
        let commitment = self.parse_commitment();
        let client = RpcClient::new_with_commitment(rpc_url.to_string(), commitment);
        let balance = client.get_balance(&signer.pubkey())?;
        
        info!("💰 Wallet balance: {} SOL", balance as f64 / 1_000_000_000.0);
        Ok(balance)
    }

    /// Execute a trade (simulated or real) with retry logic
    pub async fn execute(
        &self,
        wallet: &Wallet,
        opp: &ArbitrageOpportunity,
        amount_usd: Decimal,
        submit: bool,
        rpc_url: &str,
    ) -> Result<String> {
        let (input_token, output_token) = if opp.buy_dex.to_string() == "Jupiter" {
            (&opp.pair.quote, &opp.pair.base)
        } else {
            (&opp.pair.quote, &opp.pair.base)
        };

        // Amount in atoms (e.g. USDC = 6 decimals)
        let amount_atoms = (amount_usd * Decimal::from(1_000_000)).to_u64().unwrap_or(1_000_000);

        // 1. Get Quote
        let quote = match self.get_quote(input_token, output_token, amount_atoms).await {
            Ok(q) => {
                if let Some(out_amount) = q.get("outAmount") {
                    info!(
                        "📊 Quote: {} {} → {} {} (slippage: {}bps)",
                        amount_atoms, input_token, out_amount, output_token, self.config.slippage_bps
                    );
                }
                q
            },
            Err(e) => {
                warn!("Failed to get quote from Jupiter: {}", e);
                return Ok("Failed to get quote".to_string());
            }
        };

        // 2. Get Swap Transaction (Jupiter includes priority fee if we pass it)
        let swap_req = SwapRequest {
            user_public_key: wallet.pubkey(),
            quote_response: quote,
            compute_unit_price_micro_lamports: if submit {
                Some(self.config.priority_fee_micro_lamports)
            } else {
                None
            },
        };

        debug!("Requesting swap instruction...");
        let response = self.client.post(format!("{}/swap", JUPITER_API_URL))
            .json(&swap_req)
            .send()
            .await?;

        if response.status().is_success() {
            let swap_resp: SwapResponse = response.json().await?;
            info!(
                "✅ Received swap transaction (Base64 length: {})",
                swap_resp.swap_transaction.len()
            );

            if submit {
                // Check balance before submitting
                match self.check_balance(wallet, rpc_url) {
                    Ok(balance) => {
                        // Need at least 0.01 SOL for transaction fees
                        let min_balance = 10_000_000; // 0.01 SOL in lamports
                        if balance < min_balance {
                            return Err(anyhow!(
                                "Insufficient SOL balance: {} lamports (need at least {})",
                                balance, min_balance
                            ));
                        }
                    }
                    Err(e) => {
                        warn!("Could not check balance: {}. Proceeding anyway.", e);
                    }
                }

                // Submit with retry logic
                let signature = self.submit_with_retry(wallet, &swap_resp.swap_transaction, rpc_url)?;
                info!("✅ Swap submitted with priority fee {}µL/CU: {}", 
                    self.config.priority_fee_micro_lamports, signature);
                Ok(signature)
            } else {
                info!("📝 [SIMULATION] Transaction would be signed and sent here.");
                info!(
                    "   Priority fee: {} µL/CU | Compute limit: {} CU | Slippage: {} bps",
                    self.config.priority_fee_micro_lamports,
                    self.config.compute_unit_limit,
                    self.config.slippage_bps
                );
                Ok(swap_resp.swap_transaction)
            }
        } else {
            let error_text = response.text().await?;
            warn!("Failed to get swap transaction: {}", error_text);
            Ok("Failed".to_string())
        }
    }

    /// Submit a transaction with exponential backoff retry
    fn submit_with_retry(
        &self,
        wallet: &Wallet,
        encoded_tx: &str,
        rpc_url: &str,
    ) -> Result<String> {
        let mut last_error = None;

        for attempt in 0..self.config.max_retries {
            match self.submit_swap_transaction(wallet, encoded_tx, rpc_url) {
                Ok(sig) => return Ok(sig),
                Err(e) => {
                    let delay_ms = 500 * 2u64.pow(attempt);
                    warn!(
                        "⚠️ Transaction attempt {}/{} failed: {}. Retrying in {}ms...",
                        attempt + 1, self.config.max_retries, e, delay_ms
                    );
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("All retry attempts exhausted")))
    }

    /// Submit the swap transaction on-chain
    fn submit_swap_transaction(
        &self,
        wallet: &Wallet,
        encoded_tx: &str,
        rpc_url: &str,
    ) -> Result<String> {
        let signer = wallet
            .signer()
            .ok_or_else(|| anyhow!("No keypair available for signing"))?;

        let tx_bytes = BASE64_ENGINE.decode(encoded_tx)?;
        let tx: VersionedTransaction = bincode::deserialize(&tx_bytes)?;
        let signed_tx = VersionedTransaction::try_new(tx.message, &[signer])?;

        let commitment = self.parse_commitment();
        let client = RpcClient::new_with_commitment(rpc_url.to_string(), commitment);
        
        // Use skip_preflight for faster submission
        let config = RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        };
        
        let signature = client.send_transaction_with_config(&signed_tx, config)?;
        
        // Confirm separately
        info!("📡 Transaction sent: {}. Waiting for confirmation...", signature);
        match client.confirm_transaction_with_spinner(
            &signature,
            &client.get_latest_blockhash()?,
            commitment,
        ) {
            Ok(_) => {
                info!("✅ Transaction confirmed: {}", signature);
            }
            Err(e) => {
                error!("⚠️ Transaction sent but confirmation uncertain: {}", e);
                // Don't fail — the tx may still land
            }
        }
        
        Ok(signature.to_string())
    }

    /// Parse commitment level from config string
    fn parse_commitment(&self) -> CommitmentConfig {
        match self.config.rpc_commitment.as_str() {
            "processed" => CommitmentConfig::processed(),
            "finalized" => CommitmentConfig::finalized(),
            _ => CommitmentConfig::confirmed(),
        }
    }
}
