use crate::dex::DexProvider;
use crate::types::{PriceData, TokenPair};
use std::sync::Arc;
use std::time::Instant;
use tokio::task::JoinSet;

pub struct ParallelPriceFetcher {
    dex_providers: Vec<Arc<dyn DexProvider>>,
}

impl ParallelPriceFetcher {
    pub fn new(providers: Vec<Arc<dyn DexProvider>>) -> Self {
        Self {
            dex_providers: providers,
        }
    }

    pub async fn fetch_all_prices(&self, pairs: &[TokenPair]) -> Vec<PriceData> {
        let start = Instant::now();
        let mut join_set = JoinSet::new();

        // Iterate over providers
        for provider in &self.dex_providers {
            let provider = provider.clone();
            let pairs = pairs.to_vec();

            // Spawn concurrent task for each provider
            // We use spawn since we want them to run in parallel
            join_set.spawn(async move { provider.get_prices(&pairs).await });
        }

        let mut all_prices = Vec::new();

        // Collect results
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(prices)) => {
                    all_prices.extend(prices);
                }
                Ok(Err(e)) => {
                    tracing::warn!("Task error in price fetch: {}", e);
                }
                Err(e) => {
                    tracing::error!("Join error in price fetch: {}", e);
                }
            }
        }

        tracing::debug!(
            elapsed_ms = start.elapsed().as_millis(),
            price_count = all_prices.len(),
            "Parallel price fetch completed"
        );

        all_prices
    }
}
