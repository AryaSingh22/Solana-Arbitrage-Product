//! Raydium DEX Provider
//! 
//! Raydium is one of the largest AMM DEXs on Solana.
//! This provider fetches pool data and calculates prices.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::{ArbitrageError, ArbitrageResult, DexType, PriceData, TokenPair};
use super::{DexProvider, PriceStream};

const RAYDIUM_API: &str = "https://api.raydium.io/v2/main/pairs";

/// Raydium DEX provider implementation
pub struct RaydiumProvider {
    client: reqwest::Client,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RaydiumPair {
    name: String,
    amm_id: String,
    lp_mint: String,
    base_mint: String,
    quote_mint: String,
    price: f64,
    volume_24h: f64,
    liquidity: f64,
}

impl RaydiumProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Parse a pair name into base and quote tokens
    #[allow(dead_code)]
    fn parse_pair_name(name: &str) -> Option<(String, String)> {
        let parts: Vec<&str> = name.split('-').collect();
        if parts.len() == 2 {
            Some((parts[0].to_string(), parts[1].to_string()))
        } else {
            None
        }
    }
}

impl Default for RaydiumProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DexProvider for RaydiumProvider {
    fn dex_type(&self) -> DexType {
        DexType::Raydium
    }

    async fn get_price(&self, pair: &TokenPair) -> ArbitrageResult<PriceData> {
        let pairs: Vec<RaydiumPair> = self.client
            .get(RAYDIUM_API)
            .send()
            .await?
            .json()
            .await?;

        let target_name = format!("{}-{}", pair.base, pair.quote);
        let reverse_name = format!("{}-{}", pair.quote, pair.base);

        let raydium_pair = pairs.iter()
            .find(|p| p.name == target_name || p.name == reverse_name)
            .ok_or_else(|| ArbitrageError::PriceFetch(format!("Pair {} not found on Raydium", pair)))?;

        let mut price = Decimal::try_from(raydium_pair.price)
            .map_err(|e| ArbitrageError::PriceFetch(format!("Invalid price: {}", e)))?;

        // If we found the reverse pair, invert the price
        if raydium_pair.name == reverse_name {
            price = Decimal::ONE / price;
        }

        // Raydium AMM typically has ~0.25% spread
        let spread = price * Decimal::new(25, 5); // 0.025% each side
        let bid = price - spread;
        let ask = price + spread;

        let mut price_data = PriceData::new(DexType::Raydium, pair.clone(), bid, ask);
        price_data.volume_24h = Some(Decimal::try_from(raydium_pair.volume_24h).unwrap_or_default());
        price_data.liquidity = Some(Decimal::try_from(raydium_pair.liquidity).unwrap_or_default());

        Ok(price_data)
    }

    async fn subscribe(&self, pairs: Vec<TokenPair>) -> ArbitrageResult<PriceStream> {
        let (tx, rx) = mpsc::channel(100);
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                if let Ok(response) = client.get(RAYDIUM_API).send().await {
                    if let Ok(all_pairs) = response.json::<Vec<RaydiumPair>>().await {
                        for pair in &pairs {
                            let target_name = format!("{}-{}", pair.base, pair.quote);
                            let reverse_name = format!("{}-{}", pair.quote, pair.base);

                            if let Some(raydium_pair) = all_pairs.iter()
                                .find(|p| p.name == target_name || p.name == reverse_name)
                            {
                                if let Ok(mut price) = Decimal::try_from(raydium_pair.price) {
                                    if raydium_pair.name == reverse_name {
                                        price = Decimal::ONE / price;
                                    }

                                    let spread = price * Decimal::new(25, 5);
                                    let bid = price - spread;
                                    let ask = price + spread;

                                    let mut price_data = PriceData::new(
                                        DexType::Raydium,
                                        pair.clone(),
                                        bid,
                                        ask,
                                    );
                                    price_data.volume_24h = Decimal::try_from(raydium_pair.volume_24h).ok();
                                    price_data.liquidity = Decimal::try_from(raydium_pair.liquidity).ok();

                                    if tx.send(price_data).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }

                // Poll every 500ms
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        });

        Ok(rx)
    }

    async fn health_check(&self) -> ArbitrageResult<bool> {
        let response = self.client.get(RAYDIUM_API).send().await?;
        Ok(response.status().is_success())
    }
}
