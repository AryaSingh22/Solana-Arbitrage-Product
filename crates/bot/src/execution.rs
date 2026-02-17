//! Execution Module
//!
//! Handles fetching quotes and swap instructions from aggregators (Jupiter).
//! Implements HTTP-based execution path with priority fees, retry logic,
//! and balance checking for production-ready trading.

use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
use base64::Engine;
use reqwest::Client;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_rpc_client_api::config::RpcSendTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::transaction::VersionedTransaction;
use std::collections::HashMap;
use tracing::{debug, error, info, warn};

use crate::wallet::Wallet;
use solana_arb_core::jito::JitoClient;
use solana_arb_core::types::TradeResult;
use solana_arb_core::ArbitrageOpportunity;

use crate::flash_loan_tx_builder::FlashLoanTxBuilder;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use std::str::FromStr;

const JUPITER_API_URL: &str = "https://quote-api.jup.ag/v6";

// Token Mints (Mainnet)
pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
pub const RAY_MINT: &str = "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R";
pub const ORCA_MINT: &str = "orcaEKTdK7LKz57vaAYr9QeNsVEPfiu6QeMU1kektZE";

/// Configuration for trade execution parameters.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ExecutionConfig {
    /// Priority fee to add to transactions (in micro-lamports).
    pub priority_fee_micro_lamports: u64,
    /// Compute unit limit for transactions.
    pub compute_unit_limit: u32,
    /// Slippage tolerance in basis points.
    pub slippage_bps: u64,
    /// Maximum number of retries for failed transactions.
    pub max_retries: u32,
    /// RPC commitment level (e.g., "confirmed", "finalized").
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

use solana_arb_core::alt::AltManager;
use solana_arb_core::rate_limiter::RateLimiter;
use std::sync::Arc;

/// Main execution component responsible for processing trades.
///
/// Handles interaction with Jupiter API for swap quotes and instructions,
/// builds transactions, and submits them to the Solana network.
#[derive(Debug)]
#[allow(dead_code)]
pub struct Executor {
    /// HTTP client for making API requests.
    client: Client,
    /// Cache of token mint addresses.
    token_map: HashMap<String, String>,
    /// Execution configuration.
    config: ExecutionConfig,
    /// Builder for flash loan transactions.
    flash_loan_builder: FlashLoanTxBuilder,
    /// Whether flash loans are enabled.
    flash_loans_enabled: bool,
    /// Optional Address Lookup Table (ALT) manager.
    alt_manager: Option<Arc<AltManager>>,
    /// Rate limiter for RPC requests.
    pub rpc_rate_limiter: Option<Arc<RateLimiter>>,
    /// Rate limiter for Jupiter API requests.
    pub jupiter_rate_limiter: Option<Arc<RateLimiter>>,
}

/// Request body for Jupiter /swap endpoint (full transaction mode)
#[derive(Debug, Serialize)]
struct SwapRequest {
    #[serde(rename = "userPublicKey")]
    user_public_key: String,
    #[serde(rename = "quoteResponse")]
    quote_response: serde_json::Value,
    #[serde(rename = "computeUnitPriceMicroLamports")]
    compute_unit_price_micro_lamports: Option<u64>,
}

/// Response from Jupiter /swap endpoint
#[derive(Debug, Deserialize)]
struct SwapResponse {
    #[serde(rename = "swapTransaction")]
    swap_transaction: String,
}

/// Request body for Jupiter /swap-instructions endpoint (structured instructions mode)
#[derive(Debug, Serialize)]
struct SwapInstructionsRequest {
    #[serde(rename = "userPublicKey")]
    user_public_key: String,
    #[serde(rename = "quoteResponse")]
    quote_response: serde_json::Value,
    #[serde(rename = "wrapAndUnwrapSol")]
    wrap_and_unwrap_sol: bool,
    #[serde(rename = "computeUnitPriceMicroLamports")]
    compute_unit_price_micro_lamports: Option<u64>,
}

/// Response from Jupiter /swap-instructions endpoint
#[derive(Debug, Deserialize)]
struct SwapInstructionsResponse {
    #[serde(rename = "setupInstructions", default)]
    setup_instructions: Vec<JupiterInstruction>,
    #[serde(rename = "swapInstruction")]
    swap_instruction: JupiterInstruction,
    #[serde(rename = "cleanupInstruction")]
    cleanup_instruction: Option<JupiterInstruction>,
    #[serde(rename = "addressLookupTableAddresses", default)]
    address_lookup_table_addresses: Vec<String>,
}

/// A single instruction as returned by Jupiter's API
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JupiterInstruction {
    #[serde(rename = "programId")]
    pub program_id: String,
    #[serde(default)]
    pub accounts: Vec<JupiterAccountMeta>,
    pub data: String,
}

/// Account metadata for a Jupiter instruction
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JupiterAccountMeta {
    pub pubkey: String,
    #[serde(rename = "isSigner")]
    pub is_signer: bool,
    #[serde(rename = "isWritable")]
    pub is_writable: bool,
}

#[allow(dead_code)]
impl Executor {
    /// Creates a new Executor with default configuration.
    pub fn new() -> Self {
        Self::with_config(ExecutionConfig::default())
    }


    /// Creates a new Executor with the specified configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - The execution configuration to use.
    pub fn with_config(config: ExecutionConfig) -> Self {
        let is_devnet = config.rpc_commitment == "devnet"
            || std::env::var("SOLANA_RPC_URL")
                .unwrap_or_default()
                .contains("devnet");

        let mut token_map = HashMap::new();
        if is_devnet {
            // Devnet Mints
            // Solend Devnet USDC: zVzi5VAf4qMEwzv7NXECVx5v2pQ7xnqVVjCXZwS9XzA
            token_map.insert("SOL".to_string(), SOL_MINT.to_string());
            token_map.insert(
                "USDC".to_string(),
                "zVzi5VAf4qMEwzv7NXECVx5v2pQ7xnqVVjCXZwS9XzA".to_string(),
            );
            // Other mints (RAY, ORCA) might not work on Devnet or use different addresses.
            // Leaving them pointing to Mainnet but users should be aware.
            token_map.insert("RAY".to_string(), RAY_MINT.to_string());
            token_map.insert("ORCA".to_string(), ORCA_MINT.to_string());
        } else {
            token_map.insert("SOL".to_string(), SOL_MINT.to_string());
            token_map.insert("USDC".to_string(), USDC_MINT.to_string());
            token_map.insert("RAY".to_string(), RAY_MINT.to_string());
            token_map.insert("ORCA".to_string(), ORCA_MINT.to_string());
        }

        let wallet = crate::wallet::Wallet::new().expect("Failed to load wallet for executor");
        let keypair = if let Some(kp) = wallet.signer() {
            Keypair::from_bytes(&kp.to_bytes()).expect("Failed to clone keypair")
        } else {
            Keypair::new()
        };

        Self {
            client: Client::new(),
            token_map,
            config: config.clone(),
            flash_loan_builder: FlashLoanTxBuilder::new(keypair, is_devnet),
            flash_loans_enabled: std::env::var("ENABLE_FLASH_LOANS").unwrap_or("false".to_string())
                == "true",
            alt_manager: None,
            rpc_rate_limiter: None,
            jupiter_rate_limiter: None,
        }
    }

    /// Sets the address lookup table manager for optimizing transaction size.
    pub fn set_alt_manager(&mut self, manager: Arc<AltManager>) {
        self.alt_manager = Some(manager);
    }
    
    /// Configures rate limiters for the executor.
    pub fn set_rate_limiters(
        &mut self,
        rpc: Option<Arc<RateLimiter>>,
        jupiter: Option<Arc<RateLimiter>>,
    ) {
        self.rpc_rate_limiter = rpc;
        self.jupiter_rate_limiter = jupiter;
    }

    /// Fetches a swap quote from the Jupiter API.
    ///
    /// # Arguments
    ///
    /// * `input_mint` - Mint address of the token to swap from
    /// * `output_mint` - Mint address of the token to swap to
    /// * `amount` - Amount of input token in atomic units
    pub async fn get_quote(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount: u64,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/quote?inputMint={}&outputMint={}&amount={}&slippageBps={}",
            JUPITER_API_URL, input_mint, output_mint, amount, self.config.slippage_bps
        );

        debug!("Fetching quote from {}", url);
        let response = self.client.get(&url).send().await?;
        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(anyhow!("Jupiter quote failed: {}", err_text));
        }
        let quote: serde_json::Value = response.json().await?;
        Ok(quote)
    }

    /// Checks the SOL balance of the provided wallet.
    pub async fn check_balance(&self, wallet: &Wallet, rpc_url: &str) -> Result<u64> {
        let client = RpcClient::new(rpc_url.to_string());
        let pubkey = Pubkey::from_str(&wallet.pubkey())
            .map_err(|e| anyhow!("Invalid wallet pubkey: {}", e))?;
        Ok(client.get_balance(&pubkey).await?)
    }

    /// Executes an arbitrage trade.
    ///
    /// Decides whether to use a flash loan based on trade size and configuration.
    ///
    /// # Arguments
    ///
    /// * `wallet` - The wallet to sign the transaction
    /// * `opp` - The arbitrage opportunity details
    /// * `amount_usd` - The trade size in USD
    /// * `submit` - If true, submits the transaction; otherwise, simulates
    /// * `rpc_url` - The RPC URL to use
    /// * `jito_client` - Optional Jito client for MEV protection
    pub async fn execute(
        &self,
        wallet: &Wallet,
        opp: &ArbitrageOpportunity,
        amount_usd: Decimal,
        submit: bool,
        rpc_url: &str,
        jito_client: Option<&JitoClient>,
    ) -> Result<TradeResult> {
        let flash_loan_threshold = Decimal::from(1000);
        let use_flash_loan = self.flash_loans_enabled && amount_usd > flash_loan_threshold;

        if use_flash_loan {
            return self
                .execute_with_flash_loan(wallet, opp, amount_usd, submit, rpc_url, jito_client)
                .await;
        }

        self.execute_standard(wallet, opp, amount_usd, submit, rpc_url, jito_client)
            .await
    }

    /// Executes a standard (non-flash-loan) arbitrage trade.
    ///
    /// Fetches a quote, gets swap instructions, checks balance, and submits the transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_standard(
        &self,
        wallet: &Wallet,
        opp: &ArbitrageOpportunity,
        amount_usd: Decimal,
        submit: bool,
        rpc_url: &str,
        jito_client: Option<&JitoClient>,
    ) -> Result<TradeResult> {
        let (input_token, output_token) = (&opp.pair.quote, &opp.pair.base);

        let amount_atoms = (amount_usd * Decimal::from(1_000_000))
            .to_u64()
            .unwrap_or(1_000_000);

        let quote = match self
            .get_quote(input_token, output_token, amount_atoms)
            .await
        {
            Ok(q) => {
                if let Some(out_amount) = q.get("outAmount") {
                    info!(
                        "📊 Quote: {} {} → {} {} (slippage: {}bps)",
                        amount_atoms,
                        input_token,
                        out_amount,
                        output_token,
                        self.config.slippage_bps
                    );
                }
                q
            }
            Err(e) => {
                warn!("Failed to get quote from Jupiter: {}", e);
                return Ok(TradeResult {
                    opportunity_id: opp.id,
                    signature: None,
                    success: false,
                    actual_profit: Decimal::ZERO,
                    executed_at: chrono::Utc::now(),
                    error: Some(format!("Failed to get quote: {}", e)),
                });
            }
        };

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
        let response = self
            .client
            .post(format!("{}/swap", JUPITER_API_URL))
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
                if let Ok(balance) = self.check_balance(wallet, rpc_url).await {
                    let min_balance = 10_000_000;
                    if balance < min_balance {
                        return Ok(TradeResult {
                            opportunity_id: opp.id,
                            signature: None,
                            success: false,
                            actual_profit: Decimal::ZERO,
                            executed_at: chrono::Utc::now(),
                            error: Some("Insufficient SOL balance".to_string()),
                        });
                    }
                }


                match self.submit_with_retry(
                    wallet,
                    &swap_resp.swap_transaction,
                    rpc_url,
                    jito_client,
                ).await {
                    Ok(signature) => {
                        info!("✅ Swap submitted: {}", signature);
                        Ok(TradeResult {
                            opportunity_id: opp.id,
                            signature: Some(signature),
                            success: true,
                            actual_profit: opp.estimated_profit_usd.unwrap_or_default(),
                            executed_at: chrono::Utc::now(),
                            error: None,
                        })
                    }
                    Err(e) => Ok(TradeResult {
                        opportunity_id: opp.id,
                        signature: None,
                        success: false,
                        actual_profit: Decimal::ZERO,
                        executed_at: chrono::Utc::now(),
                        error: Some(format!("Submission failed: {}", e)),
                    }),
                }
            } else {
                info!("📝 [SIMULATION] Transaction would be signed and sent here.");
                Ok(TradeResult {
                    opportunity_id: opp.id,
                    signature: Some("simulated_signature".to_string()),
                    success: true,
                    actual_profit: opp.estimated_profit_usd.unwrap_or_default(),
                    executed_at: chrono::Utc::now(),
                    error: None,
                })
            }
        } else {
            let error_text = response.text().await?;
            warn!("Failed to get swap transaction: {}", error_text);
            Ok(TradeResult {
                opportunity_id: opp.id,
                signature: None,
                success: false,
                actual_profit: Decimal::ZERO,
                executed_at: chrono::Utc::now(),
                error: Some(format!("Failed to get swap transaction: {}", error_text)),
            })
        }
    }

    /// Submits a transaction with exponential backoff retry logic.
    async fn submit_with_retry(
        &self,
        wallet: &Wallet,
        encoded_tx: &str,
        rpc_url: &str,
        jito_client: Option<&JitoClient>,
    ) -> Result<String> {
        let mut last_error = None;
        
        for attempt in 0..self.config.max_retries {
            // Apply rate limit before attempt
            if let Some(limiter) = &self.rpc_rate_limiter {
                limiter.acquire().await;
            }

            match self.submit_swap_transaction(wallet, encoded_tx, rpc_url, jito_client).await {
                Ok(sig) => return Ok(sig),
                Err(e) => {
                    let delay_ms = 500 * 2u64.pow(attempt);
                    warn!(
                        "⚠️ Transaction attempt {}/{} failed: {}. Retrying in {}ms...",
                        attempt + 1,
                        self.config.max_retries,
                        e,
                        delay_ms
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("All retry attempts exhausted")))
    }

    async fn submit_swap_transaction(
        &self,
        wallet: &Wallet,
        encoded_tx: &str,
        rpc_url: &str,
        jito_client: Option<&JitoClient>,
    ) -> Result<String> {
        let signer = wallet
            .signer()
            .ok_or_else(|| anyhow!("No keypair available for signing"))?;

        let tx_bytes = BASE64_ENGINE.decode(encoded_tx)?;
        let tx: VersionedTransaction = bincode::deserialize(&tx_bytes)?;
        let signed_tx = VersionedTransaction::try_new(tx.message, &[signer])?;

        if let Some(jito) = jito_client {
            let signed_tx_bytes = bincode::serialize(&signed_tx)?;
            let signed_tx_base64 = BASE64_ENGINE.encode(signed_tx_bytes);

            let bundle_id = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(jito.send_bundle(&signed_tx_base64))
            })?;

            info!("🚀 Sent via Jito! Bundle ID: {}", bundle_id);
            return Ok(bundle_id);
        }

        let commitment = self.parse_commitment();
        let client = RpcClient::new_with_commitment(rpc_url.to_string(), commitment);

        let config = RpcSendTransactionConfig {
            skip_preflight: true,
            ..Default::default()
        };

        let signature = client.send_transaction_with_config(&signed_tx, config).await?;

        info!(
            "📡 Transaction sent: {}. Waiting for confirmation...",
            signature
        );
        match client.confirm_transaction_with_spinner(
            &signature,
            &client.get_latest_blockhash().await?,
            commitment,
        ).await {
            Ok(_) => {
                info!("✅ Transaction confirmed: {}", signature);
            }
            Err(e) => {
                error!("⚠️ Transaction sent but confirmation uncertain: {}", e);
            }
        }

        Ok(signature.to_string())
    }

    fn parse_commitment(&self) -> CommitmentConfig {
        match self.config.rpc_commitment.as_str() {
            "processed" => CommitmentConfig::processed(),
            "finalized" => CommitmentConfig::finalized(),
            _ => CommitmentConfig::confirmed(),
        }
    }

    /// Execute a flash loan arbitrage trade using Jupiter's `/swap-instructions` API.
    ///
    /// Instead of calling `/swap` to get a full serialized transaction and manually
    /// deserializing it (fragile), this calls `/swap-instructions` which returns
    /// structured JSON instructions that can be directly converted to
    /// `solana_sdk::Instruction`.
    pub async fn execute_with_flash_loan(
        &self,
        wallet: &Wallet,
        opp: &ArbitrageOpportunity,
        amount_usd: Decimal,
        submit: bool,
        rpc_url: &str,
        _jito_client: Option<&JitoClient>,
    ) -> Result<TradeResult> {
        info!(
            "⚡ Executing FLASH LOAN trade for opportunity: {} (amount: {} USD)",
            opp.id, amount_usd
        );

        // 1. Resolve mint address
        let input_mint_str = self
            .token_map
            .get(&opp.pair.base)
            .ok_or_else(|| anyhow!("Unknown base token: {}", opp.pair.base))?;
        let output_mint_str = self
            .token_map
            .get(&opp.pair.quote)
            .ok_or_else(|| anyhow!("Unknown quote token: {}", opp.pair.quote))?;
        let input_mint = Pubkey::from_str(input_mint_str)?;

        // 2. Convert USD amount to token atoms
        let decimals = if opp.pair.base == "USDC" { 6 } else { 9 };
        let amount_atoms = (amount_usd * Decimal::from(10u64.pow(decimals)))
            .to_u64()
            .unwrap_or(0);

        if amount_atoms == 0 {
            return Err(anyhow!("Invalid flash loan amount: zero atoms"));
        }

        // 3. Get quote from Jupiter
        let quote = self
            .get_quote(input_mint_str, output_mint_str, amount_atoms)
            .await?;

        if let Some(out_amount) = quote.get("outAmount") {
            debug!(
                "📊 Flash loan quote: {} {} → {} {}",
                amount_atoms, input_mint_str, out_amount, output_mint_str
            );
        }

        // 4. Get structured swap instructions (NOT full transaction)
        let swap_instructions_resp = self
            .get_swap_instructions(&wallet.pubkey(), &quote)
            .await?;

        info!(
            "📋 Received swap instructions: {} setup + 1 swap + {} cleanup",
            swap_instructions_resp.setup_instructions.len(),
            if swap_instructions_resp.cleanup_instruction.is_some() { 1 } else { 0 }
        );

        // 5. Convert Jupiter instructions → solana_sdk::Instruction
        let mut swap_instructions = Vec::new();

        for jup_ix in &swap_instructions_resp.setup_instructions {
            swap_instructions.push(Self::convert_jupiter_instruction(jup_ix)?);
        }

        swap_instructions.push(
            Self::convert_jupiter_instruction(&swap_instructions_resp.swap_instruction)?
        );

        if let Some(cleanup) = &swap_instructions_resp.cleanup_instruction {
            swap_instructions.push(Self::convert_jupiter_instruction(cleanup)?);
        }

        // 6. Resolve Address Lookup Tables (if any)
        let lookup_tables = if !swap_instructions_resp.address_lookup_table_addresses.is_empty() {
            if let Some(alt_manager) = &self.alt_manager {
                let table_pubkeys: Vec<Pubkey> = swap_instructions_resp
                    .address_lookup_table_addresses
                    .iter()
                    .filter_map(|addr| Pubkey::from_str(addr).ok())
                    .collect();
                alt_manager.get_tables(&table_pubkeys).await?
            } else {
                warn!("ALTs returned by Jupiter but AltManager not configured; proceeding without");
                vec![]
            }
        } else {
            vec![]
        };

        // 7. Build flash loan transaction via FlashLoanTxBuilder
        let rpc_client_instance = RpcClient::new(rpc_url.to_string());
        let recent_blockhash = rpc_client_instance.get_latest_blockhash().await?;

        let tx = self
            .flash_loan_builder
            .build_transaction(
                opp,
                amount_atoms,
                &input_mint,
                swap_instructions,
                &lookup_tables,
                recent_blockhash,
            )
            .map_err(|e| anyhow!("Failed to build flash loan tx: {}", e))?;

        // 8. Simulate transaction before submission
        if submit {
            debug!("🔍 Simulating flash loan transaction...");
            let sim_result = rpc_client_instance.simulate_transaction(&tx).await?;

            if let Some(err) = sim_result.value.err {
                return Err(anyhow!(
                    "Flash loan simulation failed: {:?}. Logs: {:?}",
                    err,
                    sim_result.value.logs.unwrap_or_default()
                ));
            }

            let compute_units = sim_result.value.units_consumed.unwrap_or(0);
            if compute_units > 1_400_000 {
                return Err(anyhow!(
                    "Compute units {} exceed limit 1,400,000",
                    compute_units
                ));
            }

            info!(
                "✅ Simulation passed (compute units: {})",
                compute_units
            );
        }

        // 9. Submit or simulate
        let signature = if submit {
            let client = RpcClient::new(rpc_url.to_string());
            let sig = client.send_and_confirm_transaction(&tx).await?;
            info!("✅ Flash loan transaction confirmed: {}", sig);
            sig.to_string()
        } else {
            info!("📝 [SIMULATION] Flash loan transaction would be submitted here.");
            "simulated_flash_loan_tx".to_string()
        };

        Ok(TradeResult {
            opportunity_id: opp.id,
            signature: Some(signature),
            success: true,
            actual_profit: opp.estimated_profit_usd.unwrap_or(Decimal::ZERO),
            executed_at: chrono::Utc::now(),
            error: None,
        })
    }

    /// Call Jupiter's `/swap-instructions` endpoint to get structured swap instructions.
    ///
    /// This returns individual instructions (setup, swap, cleanup) instead of a
    /// full serialized transaction, making it safe to embed inside a flash loan tx.
    async fn get_swap_instructions(
        &self,
        user_pubkey: &str,
        quote: &serde_json::Value,
    ) -> Result<SwapInstructionsResponse> {
        let req = SwapInstructionsRequest {
            user_public_key: user_pubkey.to_string(),
            quote_response: quote.clone(),
            wrap_and_unwrap_sol: true,
            compute_unit_price_micro_lamports: None, // Handled by FlashLoanTxBuilder
        };

        let response = self
            .client
            .post(format!("{}/swap-instructions", JUPITER_API_URL))
            .json(&req)
            .send()
            .await?;

        if !response.status().is_success() {
            let err_text = response.text().await?;
            return Err(anyhow!("Jupiter /swap-instructions failed: {}", err_text));
        }

        let resp: SwapInstructionsResponse = response.json().await?;
        Ok(resp)
    }

    /// Convert a Jupiter API instruction into a `solana_sdk::Instruction`.
    ///
    /// Jupiter's `/swap-instructions` endpoint returns instructions as JSON with
    /// base64-encoded data and string pubkeys. This converts them to native SDK types.
    pub fn convert_jupiter_instruction(
        jupiter_ix: &JupiterInstruction,
    ) -> Result<solana_sdk::instruction::Instruction> {
        let program_id = Pubkey::from_str(&jupiter_ix.program_id)
            .map_err(|e| anyhow!("Invalid program ID '{}': {}", jupiter_ix.program_id, e))?;

        let accounts = jupiter_ix
            .accounts
            .iter()
            .map(|acc| {
                let pubkey = Pubkey::from_str(&acc.pubkey)
                    .map_err(|e| anyhow!("Invalid account pubkey '{}': {}", acc.pubkey, e))?;
                Ok(solana_sdk::instruction::AccountMeta {
                    pubkey,
                    is_signer: acc.is_signer,
                    is_writable: acc.is_writable,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let data = BASE64_ENGINE
            .decode(&jupiter_ix.data)
            .map_err(|e| anyhow!("Invalid instruction data (base64): {}", e))?;

        Ok(solana_sdk::instruction::Instruction {
            program_id,
            accounts,
            data,
        })
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_jupiter_instruction_valid() {
        let jup_ix = JupiterInstruction {
            program_id: "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4".to_string(),
            accounts: vec![
                JupiterAccountMeta {
                    pubkey: "So11111111111111111111111111111111111111112".to_string(),
                    is_signer: false,
                    is_writable: true,
                },
            ],
            data: BASE64_ENGINE.encode(b"test_data"),
        };

        let result = Executor::convert_jupiter_instruction(&jup_ix);
        assert!(result.is_ok(), "Conversion should succeed");

        let ix = result.unwrap();
        assert_eq!(
            ix.program_id.to_string(),
            "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"
        );
        assert_eq!(ix.accounts.len(), 1);
        assert!(ix.accounts[0].is_writable);
        assert!(!ix.accounts[0].is_signer);
        assert_eq!(ix.data, b"test_data");
    }

    #[test]
    fn test_convert_jupiter_instruction_invalid_program_id() {
        let jup_ix = JupiterInstruction {
            program_id: "not-a-valid-pubkey!!!".to_string(),
            accounts: vec![],
            data: BASE64_ENGINE.encode(b"data"),
        };

        let result = Executor::convert_jupiter_instruction(&jup_ix);
        assert!(result.is_err(), "Should fail with invalid program ID");
    }

    #[test]
    fn test_convert_jupiter_instruction_invalid_base64() {
        let jup_ix = JupiterInstruction {
            program_id: "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4".to_string(),
            accounts: vec![],
            data: "not valid base64!!!@#$".to_string(),
        };

        let result = Executor::convert_jupiter_instruction(&jup_ix);
        assert!(result.is_err(), "Should fail with invalid base64 data");
    }

    #[test]
    fn test_convert_jupiter_instruction_empty_accounts() {
        let jup_ix = JupiterInstruction {
            program_id: "11111111111111111111111111111111".to_string(),
            accounts: vec![],
            data: BASE64_ENGINE.encode(b""),
        };

        let result = Executor::convert_jupiter_instruction(&jup_ix);
        assert!(result.is_ok(), "Should handle zero accounts");
        assert_eq!(result.unwrap().accounts.len(), 0);
    }

    #[test]
    fn test_convert_jupiter_instruction_multiple_accounts() {
        let jup_ix = JupiterInstruction {
            program_id: "11111111111111111111111111111111".to_string(),
            accounts: vec![
                JupiterAccountMeta {
                    pubkey: "So11111111111111111111111111111111111111112".to_string(),
                    is_signer: true,
                    is_writable: true,
                },
                JupiterAccountMeta {
                    pubkey: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                    is_signer: false,
                    is_writable: false,
                },
            ],
            data: BASE64_ENGINE.encode(b"swap"),
        };

        let result = Executor::convert_jupiter_instruction(&jup_ix);
        assert!(result.is_ok());
        let ix = result.unwrap();
        assert_eq!(ix.accounts.len(), 2);
        assert!(ix.accounts[0].is_signer);
        assert!(!ix.accounts[1].is_signer);
    }
}
