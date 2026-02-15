pub mod metrics;
pub mod safety;
pub mod solend;

use anyhow::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;

#[async_trait]
pub trait FlashLoanProvider: Send + Sync {
    /// Name of the provider (e.g., "Solend", "Mango")
    fn name(&self) -> &'static str;

    /// Create instructions to borrow funds
    fn borrow_instruction(&self, borrow_amount: u64, token_mint: &Pubkey) -> Result<Instruction>;

    /// Create instructions to repay funds (including fee)
    fn repay_instruction(&self, borrow_amount: u64, token_mint: &Pubkey) -> Result<Instruction>;

    /// Calculate the fee for a given borrow amount
    fn calculate_fee(&self, borrow_amount: u64) -> u64;

    /// Get a flash loan quote (fee and expected overhead)
    async fn get_quote(&self, token_mint: Pubkey, amount: Decimal) -> Result<FlashLoanQuote>;
}

#[derive(Debug, Clone)]
pub struct FlashLoanQuote {
    pub fee: Decimal,
    pub provider: String,
}
