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

/// Mock Flash Loan Provider for testing and simulation
#[derive(Debug, Clone)]
pub struct MockFlashLoanProvider {
    pub name: String,
}

impl MockFlashLoanProvider {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

#[async_trait]
impl FlashLoanProvider for MockFlashLoanProvider {
    fn name(&self) -> &'static str {
        "MockProvider"
    }

    async fn get_quote(&self, token_mint: Pubkey, amount: Decimal) -> ArbitrageResult<FlashLoanQuote> {
        // Flat 0.09% fee (9 basis points), typical for Solend/marginfi
        // 9 / 10000 = 0.0009
        let fee_rate = Decimal::new(9, 4); 
        let fee = amount * fee_rate;
        
        Ok(FlashLoanQuote {
            provider_name: self.name.clone(),
            token_mint,
            amount,
            fee,
        })
    }

    fn build_borrow_ix(&self, _quote: &FlashLoanQuote) -> ArbitrageResult<Vec<Instruction>> {
        // Return empty instructions for mock
        Ok(vec![])
    }

    fn build_repay_ix(&self, _quote: &FlashLoanQuote) -> ArbitrageResult<Vec<Instruction>> {
        // Return empty instructions for mock
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_mock_flash_loan() {
        let provider = MockFlashLoanProvider::new("Solend-Mock");
        let amount = Decimal::from_str("100.0").unwrap();
        // Use a dummy pubkey
        let token = Pubkey::new_from_array([1; 32]);

        let quote = provider.get_quote(token, amount).await.expect("Failed to get quote");

        assert_eq!(quote.provider_name, "Solend-Mock");
        assert_eq!(quote.amount, amount);
        // 0.09% of 100.0 is 0.09
        assert_eq!(quote.fee, Decimal::from_str("0.09").unwrap());
    }
}
