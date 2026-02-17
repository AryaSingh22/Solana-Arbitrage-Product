use async_trait::async_trait;
use solana_arb_core::{
    types::{ArbitrageOpportunity, PriceData},
    ArbitrageResult,
};

pub mod latency;
pub mod statistical;
pub mod plugin;

pub use latency::LatencyArbitrage;
pub use statistical::StatisticalArbitrage;
pub use plugin::*;

/// Trait for trading strategies
#[async_trait]
pub trait Strategy: Send + Sync {
    /// Unique name of the strategy
    fn name(&self) -> &'static str;

    /// Analyze price data and generate arbitrage opportunities
    async fn analyze(&self, prices: &[PriceData]) -> ArbitrageResult<Vec<ArbitrageOpportunity>>;

    /// Update internal state with new market data (e.g., for moving averages)
    async fn update_state(&self, price: &PriceData) -> ArbitrageResult<()>;
}
