//! Jupiter DEX Provider
//! 
//! Jupiter is a DEX aggregator that routes trades through multiple DEXs
//! to find the best prices. We use their Price API for price data.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::{ArbitrageError, ArbitrageResult, DexType, PriceData, TokenPair};
use super::{DexProvider, PriceStream};

const JUPITER_PRICE_API: &str = "https://price.jup.ag/v6/price";

/// Jupiter DEX provider implementation
pub struct JupiterProvider {
    client: reqwest::Client,
    /// Token symbol to mint address mapping
    token_mints: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct JupiterPriceResponse {
    data: HashMap<String, JupiterTokenPrice>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupiterTokenPrice {
    id: String,
    mint_symbol: String,
    price: f64,
}

impl JupiterProvider {
    pub fn new() -> Self {
        let mut token_mints = HashMap::new();
        // Common Solana tokens
        token_mints.insert("SOL".to_string(), "So11111111111111111111111111111111111111112".to_string());
        token_mints.insert("USDC".to_string(), "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string());
        token_mints.insert("USDT".to_string(), "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".to_string());
        token_mints.insert("RAY".to_string(), "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R".to_string());
        token_mints.insert("SRM".to_string(), "SRMuApVNdxXokk5GT7XD5cUUgXMBCoAz2LHeuAoKWRt".to_string());
        token_mints.insert("BONK".to_string(), "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263".to_string());
        token_mints.insert("JUP".to_string(), "JUPyiwrYJFskUPiHa7hkeR8VUtAe6poCFFRLnWo6h7rL".to_string());
        token_mints.insert("ORCA".to_string(), "orcaEKTdK7LKz57vaAYr9QeNsVEPfiu6QeMU1kektZE".to_string());
        
        Self {
            client: reqwest::Client::new(),
            token_mints,
        }
    }

    /// Get the mint address for a token symbol
    fn get_mint(&self, symbol: &str) -> Option<&String> {
        self.token_mints.get(symbol)
    }

    /// Add a custom token mapping
    pub fn add_token(&mut self, symbol: String, mint: String) {
        self.token_mints.insert(symbol, mint);
    }
}

impl Default for JupiterProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DexProvider for JupiterProvider {
    fn dex_type(&self) -> DexType {
        DexType::Jupiter
    }

    async fn get_price(&self, pair: &TokenPair) -> ArbitrageResult<PriceData> {
        let base_mint = self.get_mint(&pair.base)
            .ok_or_else(|| ArbitrageError::Config(format!("Unknown token: {}", pair.base)))?;
        
        let quote_mint = self.get_mint(&pair.quote)
            .ok_or_else(|| ArbitrageError::Config(format!("Unknown token: {}", pair.quote)))?;

        let url = format!("{}?ids={}&vsToken={}", JUPITER_PRICE_API, base_mint, quote_mint);
        
        let response: JupiterPriceResponse = self.client
            .get(&url)
            .send()
            .await?
            .json()
            .await?;

        let token_price = response.data.get(base_mint)
            .ok_or_else(|| ArbitrageError::PriceFetch("No price data returned".to_string()))?;

        let price = Decimal::try_from(token_price.price)
            .map_err(|e| ArbitrageError::PriceFetch(format!("Invalid price: {}", e)))?;

        // Jupiter provides a single price, we estimate bid/ask with a small spread
        let spread = price * Decimal::new(1, 4); // 0.01% spread estimate
        let bid = price - spread;
        let ask = price + spread;

        Ok(PriceData::new(DexType::Jupiter, pair.clone(), bid, ask))
    }

    async fn subscribe(&self, pairs: Vec<TokenPair>) -> ArbitrageResult<PriceStream> {
        let (tx, rx) = mpsc::channel(100);
        let client = self.client.clone();
        let token_mints = self.token_mints.clone();

        tokio::spawn(async move {
            loop {
                for pair in &pairs {
                    let base_mint = match token_mints.get(&pair.base) {
                        Some(m) => m,
                        None => continue,
                    };
                    let quote_mint = match token_mints.get(&pair.quote) {
                        Some(m) => m,
                        None => continue,
                    };

                    let url = format!("{}?ids={}&vsToken={}", JUPITER_PRICE_API, base_mint, quote_mint);
                    
                    if let Ok(response) = client.get(&url).send().await {
                        if let Ok(data) = response.json::<JupiterPriceResponse>().await {
                            if let Some(token_price) = data.data.get(base_mint) {
                                if let Ok(price) = Decimal::try_from(token_price.price) {
                                    let spread = price * Decimal::new(1, 4);
                                    let bid = price - spread;
                                    let ask = price + spread;
                                    
                                    let price_data = PriceData::new(
                                        DexType::Jupiter,
                                        pair.clone(),
                                        bid,
                                        ask,
                                    );
                                    
                                    if tx.send(price_data).await.is_err() {
                                        return; // Channel closed
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Poll every 500ms for updates
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        });

        Ok(rx)
    }

    async fn health_check(&self) -> ArbitrageResult<bool> {
        let url = format!("{}?ids=So11111111111111111111111111111111111111112", JUPITER_PRICE_API);
        let response = self.client.get(&url).send().await?;
        Ok(response.status().is_success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires network access - run with: cargo test -- --ignored
    async fn test_jupiter_health_check() {
        let provider = JupiterProvider::new();
        let result = provider.health_check().await;
        assert!(result.is_ok());
    }
}
