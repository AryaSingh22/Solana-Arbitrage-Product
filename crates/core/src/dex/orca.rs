//! Orca DEX Provider
//! 
//! Orca is a popular AMM DEX on Solana with Whirlpools for concentrated liquidity.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::{ArbitrageError, ArbitrageResult, DexType, PriceData, TokenPair};
use super::{DexProvider, PriceStream};

const ORCA_WHIRLPOOL_API: &str = "https://api.mainnet.orca.so/v1/whirlpool/list";

/// Orca DEX provider implementation
pub struct OrcaProvider {
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct OrcaWhirlpoolList {
    whirlpools: Vec<OrcaWhirlpool>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OrcaWhirlpool {
    address: String,
    token_a: OrcaToken,
    token_b: OrcaToken,
    price: f64,
    volume_24h: Option<f64>,
    tvl: Option<f64>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OrcaToken {
    mint: String,
    symbol: String,
    decimals: u8,
}

impl OrcaProvider {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for OrcaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DexProvider for OrcaProvider {
    fn dex_type(&self) -> DexType {
        DexType::Orca
    }

    async fn get_price(&self, pair: &TokenPair) -> ArbitrageResult<PriceData> {
        let response: OrcaWhirlpoolList = self.client
            .get(ORCA_WHIRLPOOL_API)
            .send()
            .await?
            .json()
            .await?;

        let whirlpool = response.whirlpools.iter()
            .find(|w| {
                (w.token_a.symbol == pair.base && w.token_b.symbol == pair.quote) ||
                (w.token_a.symbol == pair.quote && w.token_b.symbol == pair.base)
            })
            .ok_or_else(|| ArbitrageError::PriceFetch(format!("Pair {} not found on Orca", pair)))?;

        let mut price = Decimal::try_from(whirlpool.price)
            .map_err(|e| ArbitrageError::PriceFetch(format!("Invalid price: {}", e)))?;

        // Invert if tokens are reversed
        if whirlpool.token_a.symbol == pair.quote {
            price = Decimal::ONE / price;
        }

        // Orca Whirlpools have variable spreads, estimate ~0.3%
        let spread = price * Decimal::new(30, 5); // 0.03% each side
        let bid = price - spread;
        let ask = price + spread;

        let mut price_data = PriceData::new(DexType::Orca, pair.clone(), bid, ask);
        
        if let Some(vol) = whirlpool.volume_24h {
            price_data.volume_24h = Some(Decimal::try_from(vol).unwrap_or_default());
        }
        if let Some(tvl) = whirlpool.tvl {
            price_data.liquidity = Some(Decimal::try_from(tvl).unwrap_or_default());
        }

        Ok(price_data)
    }

    async fn subscribe(&self, pairs: Vec<TokenPair>) -> ArbitrageResult<PriceStream> {
        let (tx, rx) = mpsc::channel(100);
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                if let Ok(response) = client.get(ORCA_WHIRLPOOL_API).send().await {
                    if let Ok(data) = response.json::<OrcaWhirlpoolList>().await {
                        for pair in &pairs {
                            if let Some(whirlpool) = data.whirlpools.iter().find(|w| {
                                (w.token_a.symbol == pair.base && w.token_b.symbol == pair.quote) ||
                                (w.token_a.symbol == pair.quote && w.token_b.symbol == pair.base)
                            }) {
                                if let Ok(mut price) = Decimal::try_from(whirlpool.price) {
                                    if whirlpool.token_a.symbol == pair.quote {
                                        price = Decimal::ONE / price;
                                    }

                                    let spread = price * Decimal::new(30, 5);
                                    let bid = price - spread;
                                    let ask = price + spread;

                                    let mut price_data = PriceData::new(
                                        DexType::Orca,
                                        pair.clone(),
                                        bid,
                                        ask,
                                    );
                                    
                                    if let Some(vol) = whirlpool.volume_24h {
                                        price_data.volume_24h = Decimal::try_from(vol).ok();
                                    }
                                    if let Some(tvl) = whirlpool.tvl {
                                        price_data.liquidity = Decimal::try_from(tvl).ok();
                                    }

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
        let response = self.client.get(ORCA_WHIRLPOOL_API).send().await?;
        Ok(response.status().is_success())
    }
}
