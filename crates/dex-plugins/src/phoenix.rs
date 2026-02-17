use async_trait::async_trait;
use solana_arb_core::{
    dex::DexProvider,
    error::ArbitrageError,
    types::{DexType, PriceData, TokenPair},
    ArbitrageResult,
};
use tokio::sync::mpsc;

pub struct PhoenixProvider {
    // Placeholder
}

impl Default for PhoenixProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl PhoenixProvider {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl DexProvider for PhoenixProvider {
    fn dex_type(&self) -> DexType {
        DexType::Phoenix
    }

    async fn get_price(&self, _pair: &TokenPair) -> ArbitrageResult<PriceData> {
        Err(ArbitrageError::PriceFetch(
            "Phoenix price fetching not implemented".to_string(),
        ))
    }

    async fn subscribe(
        &self,
        _pairs: Vec<TokenPair>,
    ) -> ArbitrageResult<mpsc::Receiver<PriceData>> {
        Err(ArbitrageError::PriceFetch(
            "Phoenix subscription not implemented".to_string(),
        ))
    }

    async fn health_check(&self) -> ArbitrageResult<bool> {
        Ok(true)
    }
}
