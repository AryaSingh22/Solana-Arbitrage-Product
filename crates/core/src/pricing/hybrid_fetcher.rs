use crate::pricing::parallel_fetcher::ParallelPriceFetcher;
#[cfg(feature = "ws")]
use crate::streaming::ws_manager::WebSocketManager;
use crate::types::{PriceData, TokenPair};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[allow(dead_code)]
pub struct HybridPriceFetcher {
    #[cfg(feature = "ws")]
    ws_manager: WebSocketManager,
    http_fetcher: ParallelPriceFetcher,
    // precise cache of latest prices
    price_cache: Arc<RwLock<HashMap<String, PriceData>>>,
}

impl HybridPriceFetcher {
    #[cfg(feature = "ws")]
    pub fn new(http_fetcher: ParallelPriceFetcher, ws_manager: WebSocketManager) -> Self {
        Self {
            ws_manager,
            http_fetcher,
            price_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[cfg(not(feature = "ws"))]
    pub fn new_http_only(http_fetcher: ParallelPriceFetcher) -> Self {
        Self {
            http_fetcher,
            price_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start(&self, _pairs: &[TokenPair]) {
        tracing::info!("Starting Hybrid Price Fetcher...");

        #[cfg(feature = "ws")]
        {
            // Example: Subscribe to WS if available
            // for pair in _pairs {
            //     self.ws_manager.subscribe_to_pair(DexType::Jupiter, pair.clone()).await;
            // }
        }
    }

    pub async fn fetch_all_prices(&self, pairs: &[TokenPair]) -> Vec<PriceData> {
        let prices = self.http_fetcher.fetch_all_prices(pairs).await;

        // Update cache with HTTP prices
        {
            let mut cache = self.price_cache.write().await;
            for price in &prices {
                cache.insert(price.pair.symbol(), price.clone());
            }
        }

        prices
    }

    pub async fn get_price(&self, pair_symbol: &str) -> Option<PriceData> {
        let cache = self.price_cache.read().await;
        cache.get(pair_symbol).cloned()
    }
}
