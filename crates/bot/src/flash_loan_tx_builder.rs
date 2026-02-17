use solana_arb_core::ArbitrageOpportunity;
use solana_sdk::address_lookup_table::AddressLookupTableAccount;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::message::{v0, VersionedMessage};
use solana_sdk::transaction::VersionedTransaction;
use solana_sdk::{
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};

#[derive(Debug)]
pub struct FlashLoanTxBuilder {
    payer: Keypair,
    solend_program_id: Pubkey,
    is_devnet: bool,
}

impl FlashLoanTxBuilder {
    pub const SOLEND_PROGRAM_MAINNET: &'static str = "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo";
    pub const SOLEND_PROGRAM_DEVNET: &'static str = "ALend7Ketfx5bxh6ghsCDXAoDrhvEmsXT3cynB6aPLgx";
    pub const FEE_BPS: u64 = 3; // 0.03%

    pub fn new(payer: Keypair, is_devnet: bool) -> Self {
        let program_id_str = if is_devnet {
            Self::SOLEND_PROGRAM_DEVNET
        } else {
            Self::SOLEND_PROGRAM_MAINNET
        };

        Self {
            payer,
            // Safety: SOLEND_PROGRAM_MAINNET and SOLEND_PROGRAM_DEVNET are valid base58 pubkeys
            solend_program_id: program_id_str
                .parse()
                .expect("Solend program ID constants must be valid pubkeys"),
            is_devnet,
        }
    }

    /// Build complete flash loan transaction (V0 with ALT support)
    pub fn build_transaction(
        &self,
        opportunity: &ArbitrageOpportunity,
        borrow_amount: u64,
        token_mint: &Pubkey,
        swap_instructions: Vec<Instruction>,
        lookup_tables: &[AddressLookupTableAccount],
        recent_blockhash: solana_sdk::hash::Hash,
    ) -> Result<VersionedTransaction, Box<dyn std::error::Error>> {
        let mut all_instructions = Vec::new();

        // 1. Compute budget
        all_instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(1_400_000));

        let priority_fee = self.calculate_priority_fee(opportunity, borrow_amount);
        all_instructions.push(ComputeBudgetInstruction::set_compute_unit_price(
            priority_fee,
        ));

        // 2. Get/Create ATA for Payer
        let ata = spl_associated_token_account::get_associated_token_address(
            &self.payer.pubkey(),
            token_mint,
        );

        // Create ATA idempotent (if it doesn't exist)
        all_instructions.push(
            spl_associated_token_account::instruction::create_associated_token_account_idempotent(
                &self.payer.pubkey(),
                &self.payer.pubkey(),
                token_mint,
                &spl_token::id(),
            ),
        );

        // 3. Flash borrow from Solend to ATA
        all_instructions.push(self.build_flash_borrow_instruction(
            borrow_amount,
            token_mint,
            &ata,
        )?);

        // 4. Add all swap instructions
        all_instructions.extend(swap_instructions);

        // 5. Flash repay (amount + fee) from ATA
        let repay_amount = self.calculate_repay_amount(borrow_amount);
        all_instructions.push(self.build_flash_repay_instruction(
            repay_amount,
            token_mint,
            &ata,
        )?);

        // Build V0 Message with ALTs
        let message = v0::Message::try_compile(
            &self.payer.pubkey(),
            &all_instructions,
            lookup_tables,
            recent_blockhash,
        )?;

        // Build Versioned Transaction
        let transaction =
            VersionedTransaction::try_new(VersionedMessage::V0(message), &[&self.payer])?;

        Ok(transaction)
    }

    fn calculate_priority_fee(
        &self,
        _opportunity: &ArbitrageOpportunity,
        borrow_amount: u64,
    ) -> u64 {
        // 5% of expected profit as priority fee
        // heuristic: profit ~ 0.5% of amount
        // profit_amt = amount * 0.005
        // fee = profit_amt * 0.05 = amount * 0.00025
        let fee = (borrow_amount as f64 * 0.00025) as u64;

        // Cap at reasonable limits, min 50k micro-lamports
        fee.clamp(50_000, 1_000_000)
    }

    fn calculate_repay_amount(&self, borrowed: u64) -> u64 {
        // Solend fee: 0.03% (3 basis points)
        borrowed + (borrowed * Self::FEE_BPS / 10000)
    }

    fn build_flash_borrow_instruction(
        &self,
        amount: u64,
        token_mint: &Pubkey,
        destination: &Pubkey,
    ) -> Result<Instruction, Box<dyn std::error::Error>> {
        let reserve = self.get_solend_reserve(token_mint)?;

        // FlashBorrow: [139, 141, 178, 175, 49, 45, 115, 42]
        let mut data = vec![139, 141, 178, 175, 49, 45, 115, 42];
        data.extend_from_slice(&amount.to_le_bytes());

        let accounts = vec![
            solana_sdk::instruction::AccountMeta::new(reserve.liquidity_supply_pubkey, false),
            solana_sdk::instruction::AccountMeta::new(*destination, false),
            solana_sdk::instruction::AccountMeta::new_readonly(reserve.reserve_pubkey, false),
            solana_sdk::instruction::AccountMeta::new_readonly(reserve.lending_market, false),
            solana_sdk::instruction::AccountMeta::new_readonly(spl_token::id(), false),
        ];

        Ok(Instruction {
            program_id: self.solend_program_id,
            accounts,
            data,
        })
    }

    fn build_flash_repay_instruction(
        &self,
        amount: u64,
        token_mint: &Pubkey,
        source: &Pubkey,
    ) -> Result<Instruction, Box<dyn std::error::Error>> {
        let reserve = self.get_solend_reserve(token_mint)?;

        // FlashRepay: [92, 159, 112, 159, 84, 26, 25, 187]
        let mut data = vec![92, 159, 112, 159, 84, 26, 25, 187];
        data.extend_from_slice(&amount.to_le_bytes());

        let accounts = vec![
            solana_sdk::instruction::AccountMeta::new(*source, false),
            solana_sdk::instruction::AccountMeta::new(reserve.liquidity_supply_pubkey, false),
            solana_sdk::instruction::AccountMeta::new(reserve.reserve_pubkey, false),
            solana_sdk::instruction::AccountMeta::new_readonly(reserve.lending_market, false),
            solana_sdk::instruction::AccountMeta::new_readonly(self.payer.pubkey(), true),
            solana_sdk::instruction::AccountMeta::new_readonly(spl_token::id(), false),
        ];

        Ok(Instruction {
            program_id: self.solend_program_id,
            accounts,
            data,
        })
    }

    fn get_solend_reserve(
        &self,
        token_mint: &Pubkey,
    ) -> Result<SolendReserve, Box<dyn std::error::Error>> {
        if self.is_devnet {
            return self.get_solend_reserve_devnet(token_mint);
        }

        // Hardcoded Solend reserves (mainnet)
        let usdc_mint: Pubkey = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".parse()?;
        let sol_mint: Pubkey = "So11111111111111111111111111111111111111112".parse()?;

        if token_mint == &usdc_mint {
            Ok(SolendReserve {
                reserve_pubkey: "BgxfHJDzm44T7XG68MYKx7YisTjZu73tVovyZSjJMpmw".parse()?,
                liquidity_supply_pubkey: "8SheGtsopRUDzdiD6v6BR9a6bqZ9QwywYQY99Fp5meNf".parse()?,
                lending_market: "4UpD2fh7xH3VP9QQaXtsS1YY3bxzWhtfpks7FatyKvdY".parse()?,
            })
        } else if token_mint == &sol_mint {
            Ok(SolendReserve {
                reserve_pubkey: "8PbodeaosQP19SjYFx855UMqWxH2HynZLdBXmsrbac36".parse()?,
                liquidity_supply_pubkey: "8UviNr47S8eL6J3WfDxMRa3hvLta1VDJwNWqsDgtN3Cv".parse()?,
                lending_market: "4UpD2fh7xH3VP9QQaXtsS1YY3bxzWhtfpks7FatyKvdY".parse()?,
            })
        } else {
            Err(
                "Unsupported token mint for flash loans (Only SOL/USDC supported in Phase 11)"
                    .into(),
            )
        }
    }

    fn get_solend_reserve_devnet(
        &self,
        token_mint: &Pubkey,
    ) -> Result<SolendReserve, Box<dyn std::error::Error>> {
        // Devnet Reserves
        // Using Solend Devnet USDC faucet mint: zVzi5VAf4qMEwzv7NXECVx5v2pQ7xnqVVjCXZwS9XzA
        // Using Standard Wrapped SOL: So11111111111111111111111111111111111111112

        let usdc_devnet_mint: Pubkey = "zVzi5VAf4qMEwzv7NXECVx5v2pQ7xnqVVjCXZwS9XzA".parse()?;
        let sol_mint: Pubkey = "So11111111111111111111111111111111111111112".parse()?;

        if token_mint == &usdc_devnet_mint {
            // USDC Reserve
            Ok(SolendReserve {
                reserve_pubkey: "FNNkz4RCQezSSS71rW2tvqZH1LCkTzaiG7Nd1LeA5x5y".parse()?,
                liquidity_supply_pubkey: "HixjFJoeD2ggqKgFHQxrcJFjVvE5nXKuUPYNijFg7Kc5".parse()?,
                lending_market: "GvjoVKNjBvQcFaSKUW1gTE7DxhSpjHbE69umVR5nPuQp".parse()?,
            })
        } else if token_mint == &sol_mint {
            // SOL Reserve
            Ok(SolendReserve {
                reserve_pubkey: "5VVLD7BQp8y3bTgyF5ezm1ResyMTR3PhYsT4iHFU8Sxz".parse()?,
                liquidity_supply_pubkey: "furd3XUtjXZ2gRvSsoUts9A5m8cMJNqdsyR2Rt8vY9s".parse()?,
                lending_market: "GvjoVKNjBvQcFaSKUW1gTE7DxhSpjHbE69umVR5nPuQp".parse()?,
            })
        } else {
            Err(format!("Unsupported Devnet token mint: {}", token_mint).into())
        }
    }
}

struct SolendReserve {
    reserve_pubkey: Pubkey,
    liquidity_supply_pubkey: Pubkey,
    lending_market: Pubkey,
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::Keypair;

    #[test]
    fn test_devnet_builder_init() {
        let payer = Keypair::new();
        let builder = FlashLoanTxBuilder::new(payer, true);
        assert!(builder.is_devnet);
        assert_eq!(
            builder.solend_program_id.to_string(),
            FlashLoanTxBuilder::SOLEND_PROGRAM_DEVNET
        );
    }

    #[test]
    fn test_get_solend_reserve_devnet() {
        let payer = Keypair::new();
        let builder = FlashLoanTxBuilder::new(payer, true);
        let usdc_devnet_mint: Pubkey = "zVzi5VAf4qMEwzv7NXECVx5v2pQ7xnqVVjCXZwS9XzA"
            .parse()
            .unwrap();
        let reserve = builder.get_solend_reserve(&usdc_devnet_mint).unwrap();
        assert_eq!(
            reserve.reserve_pubkey.to_string(),
            "FNNkz4RCQezSSS71rW2tvqZH1LCkTzaiG7Nd1LeA5x5y"
        );
    }
}
