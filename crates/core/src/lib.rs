//! Solana Arbitrage Core Library
//! 
//! This crate provides shared types, DEX integrations, and arbitrage detection
//! for the Solana Arbitrage Dashboard system.

pub mod types;
pub mod error;
pub mod dex;
pub mod arbitrage;
pub mod config;
pub mod pathfinder;
pub mod risk;
pub mod flash_loan;
pub mod history;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod simulation_logs;

pub use types::*;
pub use error::*;
