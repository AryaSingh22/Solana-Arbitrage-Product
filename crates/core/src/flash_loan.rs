//! Flash Loan Provider Interface
//!
//! Defines the traits for interacting with flash loan providers on Solana.

use solana_sdk::pubkey::Pubkey;
use solana_sdk::instruction::Instruction;
use rust_decimal::Decimal;
use async_trait::async_trait;
use crate::error::ArbitrageResult;

#[derive(Debug, Clone)]
pub struct FlashLoanQuote {
    pub provider_name: String,
    pub token_mint: Pubkey,
    pub amount: Decimal,
    pub fee: Decimal,
}

#[async_trait]
pub trait FlashLoanProvider: Send + Sync {
    /// Get the provider's name (e.g., "Solend", "Marginfi")
    fn name(&self) -> &'static str;

    /// Get a quote for a flash loan
    async fn get_quote(&self, token_mint: Pubkey, amount: Decimal) -> ArbitrageResult<FlashLoanQuote>;

    /// Build instructions to borrow funds
    fn build_borrow_ix(&self, quote: &FlashLoanQuote) -> ArbitrageResult<Vec<Instruction>>;

    /// Build instructions to repay funds
    fn build_repay_ix(&self, quote: &FlashLoanQuote) -> ArbitrageResult<Vec<Instruction>>;
}
