use anyhow::{anyhow, Result};
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use tracing::warn;

pub struct FlashLoanSafety;

impl FlashLoanSafety {
    /// Verify that the transaction instructions are ordered correctly for a flash loan
    /// 1. Borrow Instruction
    /// 2. Arbitrage Logic (Swap A -> B -> A)
    /// 3. Repay Instruction
    pub fn verify_instruction_order(
        instructions: &[Instruction],
        borrower_program_id: &Pubkey,
    ) -> Result<()> {
        if instructions.len() < 3 {
            return Err(anyhow!(
                "Flash loan transaction must have at least 3 instructions (Borrow, Swap, Repay)"
            ));
        }

        let first_ix = &instructions[0];
        let last_ix = &instructions[instructions.len() - 1];

        if first_ix.program_id != *borrower_program_id {
            return Err(anyhow!("First instruction must be the flash loan borrow"));
        }

        if last_ix.program_id != *borrower_program_id {
            return Err(anyhow!("Last instruction must be the flash loan repay"));
        }

        Ok(())
    }

    /// Check if the detected opportunity profit exceeds the flash loan fee
    pub fn check_profitability(
        estimated_profit: u64,
        flash_loan_fee: u64,
        network_fee: u64,
    ) -> Result<()> {
        let total_cost = flash_loan_fee + network_fee;

        if estimated_profit <= total_cost {
            warn!(
                "Flash loan safety check failed: Profit {} <= Total Cost {} (Fee: {}, Network: {})",
                estimated_profit, total_cost, flash_loan_fee, network_fee
            );
            return Err(anyhow!(
                "Opportunity not profitable enough to cover flash loan costs"
            ));
        }

        Ok(())
    }
}
