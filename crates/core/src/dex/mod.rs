//! DEX Provider implementations
//!
//! This module contains the trait definition and implementations for
//! connecting to various Solana DEXs and fetching price data.

#[cfg(feature = "http")]
pub mod jupiter;
#[cfg(feature = "http")]
pub mod orca;
#[cfg(feature = "http")]
pub mod raydium;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{ArbitrageResult, DexType, PriceData, TokenPair};

/// Stream of price updates from a DEX
pub type PriceStream = mpsc::Receiver<PriceData>;

/// Trait for DEX price data providers
#[async_trait]
pub trait DexProvider: Send + Sync {
    /// Returns the DEX type this provider connects to
    fn dex_type(&self) -> DexType;

    /// Returns the trading fee percentage for this DEX
    fn fee_percentage(&self) -> rust_decimal::Decimal {
        self.dex_type().fee_percentage()
    }

    /// Get the current price for a specific trading pair
    async fn get_price(&self, pair: &TokenPair) -> ArbitrageResult<PriceData>;

    /// Get prices for multiple trading pairs
    async fn get_prices(&self, pairs: &[TokenPair]) -> ArbitrageResult<Vec<PriceData>> {
        let mut prices = Vec::with_capacity(pairs.len());
        for pair in pairs {
            match self.get_price(pair).await {
                Ok(price) => prices.push(price),
                Err(e) => {
                    tracing::warn!("Failed to get price for {}: {}", pair, e);
                }
            }
        }
        Ok(prices)
    }

    /// Subscribe to real-time price updates for the given pairs
    async fn subscribe(&self, pairs: Vec<TokenPair>) -> ArbitrageResult<PriceStream>;

    /// Check if the provider is connected and healthy
    async fn health_check(&self) -> ArbitrageResult<bool>;
}

/// Manager for multiple DEX providers.
///
/// Aggregates multiple DEX implementations to allow unified price fetching
/// and interaction across the Solana ecosystem.
pub struct DexManager {
    providers: Vec<std::sync::Arc<dyn DexProvider>>,
}

impl DexManager {
    /// Creates a new, empty DexManager.
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Registers a new DEX provider.
    pub fn add_provider(&mut self, provider: std::sync::Arc<dyn DexProvider>) {
        self.providers.push(provider);
    }

    /// Returns a slice of all registered providers.
    pub fn providers(&self) -> &[std::sync::Arc<dyn DexProvider>] {
        &self.providers
    }

    /// Fetches prices for a given pair from all registered providers.
    ///
    /// Useful for price discovery and cross-exchange comparison.
    pub async fn get_all_prices(&self, pair: &TokenPair) -> Vec<PriceData> {
        let mut prices = Vec::new();
        for provider in &self.providers {
            tracing::info!("➡️ Calling price fetch for DEX: {:?}", provider.dex_type());
            match provider.get_price(pair).await {
                Ok(price) => {
                    tracing::info!(
                        "⬅️ DEX {:?} returned price for {}",
                        provider.dex_type(),
                        pair
                    );
                    prices.push(price);
                }
                Err(e) => {
                    tracing::warn!("❌ DEX {:?} fetch error: {}", provider.dex_type(), e);
                }
            }
        }
        prices
    }
}

impl Default for DexManager {
    fn default() -> Self {
        Self::new()
    }
}
