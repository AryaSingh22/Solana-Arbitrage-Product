use async_trait::async_trait;
use solana_arb_core::{
    types::{ArbitrageOpportunity, PriceData},
    ArbitrageResult,
};
use crate::Strategy;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Metadata for a strategy plugin
#[derive(Debug, Clone)]
pub struct StrategyDescriptor {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
}

/// Extended trait for strategies with lifecycle hooks and metadata
#[async_trait]
pub trait StrategyPlugin: Strategy {
    /// Get metadata about the strategy
    fn descriptor(&self) -> StrategyDescriptor;

    /// Called when the strategy is initialized
    async fn on_load(&self) -> ArbitrageResult<()> {
        Ok(())
    }

    /// Called when the strategy is stopped or removed
    async fn on_unload(&self) -> ArbitrageResult<()> {
        Ok(())
    }
}

/// Registry to manage and orchestrate multiple strategy plugins
pub struct StrategyRegistry {
    plugins: Arc<RwLock<Vec<Box<dyn StrategyPlugin>>>>,
}

impl Default for StrategyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl StrategyRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register a new strategy plugin
    pub async fn register(&self, plugin: Box<dyn StrategyPlugin>) -> ArbitrageResult<()> {
        plugin.on_load().await?;
        let mut plugins = self.plugins.write().await;
        plugins.push(plugin);
        Ok(())
    }

    /// Run all enabled strategies against the provided price data
    pub async fn analyze_all(&self, prices: &[PriceData]) -> Vec<ArbitrageOpportunity> {
        let plugins = self.plugins.read().await;
        let mut all_opps = Vec::new();

        for plugin in plugins.iter() {
            if plugin.descriptor().enabled {
                match plugin.analyze(prices).await {
                    Ok(opps) => all_opps.extend(opps),
                    Err(e) => {
                        tracing::warn!("Strategy {} failed during analysis: {}", plugin.name(), e);
                    }
                }
            }
        }
        all_opps
    }

    /// Update state for all enabled strategies
    pub async fn update_all(&self, price: &PriceData) {
        let plugins = self.plugins.read().await;
        for plugin in plugins.iter() {
            if plugin.descriptor().enabled {
                if let Err(e) = plugin.update_state(price).await {
                    tracing::warn!("Strategy {} state update failed: {}", plugin.name(), e);
                }
            }
        }
    }
    
    /// Get count of registered strategies
    pub async fn count(&self) -> usize {
        self.plugins.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_arb_core::types::{DexType, TokenPair};
    use rust_decimal::Decimal;

    struct MockStrategy {
        name: String,
        should_fail: bool,
    }

    #[async_trait]
    impl Strategy for MockStrategy {
        fn name(&self) -> &'static str {
            "MockStrategy" // Ideally dynamic but &'static str limit
        }

        async fn analyze(&self, _prices: &[PriceData]) -> ArbitrageResult<Vec<ArbitrageOpportunity>> {
             if self.should_fail {
                 return Err(solana_arb_core::ArbitrageError::StrategyError {
                     strategy: self.name().to_string(),
                     reason: "Simulated failure".to_string(),
                 });
             }
             
             // Return one dummy opportunity
             let opp = ArbitrageOpportunity {
                 id: uuid::Uuid::new_v4(),
                 pair: TokenPair::new("SOL", "USDC"),
                 buy_dex: DexType::Raydium,
                 sell_dex: DexType::Orca,
                 buy_price: Decimal::new(100, 0),
                 sell_price: Decimal::new(101, 0),
                 gross_profit_pct: Decimal::new(1, 0),
                 net_profit_pct: Decimal::new(1, 0),
                 estimated_profit_usd: Some(Decimal::new(10, 0)),
                 recommended_size: Some(Decimal::new(1000, 0)),
                 detected_at: chrono::Utc::now(),
                 expired_at: None,
             };
             
             Ok(vec![opp])
        }

        async fn update_state(&self, _price: &PriceData) -> ArbitrageResult<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl StrategyPlugin for MockStrategy {
        fn descriptor(&self) -> StrategyDescriptor {
            StrategyDescriptor {
                name: self.name.clone(),
                version: "1.0.0".to_string(),
                description: "Mock strategy for testing".to_string(),
                enabled: true,
            }
        }
    }

    #[tokio::test]
    async fn test_registry_lifecycle() {
        let registry = StrategyRegistry::new();
        
        let plugin = MockStrategy { 
            name: "TestStrategy".to_string(), 
            should_fail: false 
        };
        
        registry.register(Box::new(plugin)).await.unwrap();
        
        assert_eq!(registry.count().await, 1);
    }

    #[tokio::test]
    async fn test_analyze_all() {
        let registry = StrategyRegistry::new();
        registry.register(Box::new(MockStrategy { 
            name: "S1".to_string(), 
            should_fail: false 
        })).await.unwrap();

        let prices = vec![]; // Empty prices for mock
        let opps = registry.analyze_all(&prices).await;
        
        assert_eq!(opps.len(), 1);
    }
    
    #[tokio::test]
    async fn test_failed_strategy_handling() {
        let registry = StrategyRegistry::new();
        
        // Strategy that fails
        registry.register(Box::new(MockStrategy { 
            name: "BadStrategy".to_string(), 
            should_fail: true 
        })).await.unwrap();
        
        // Strategy that succeeds
        registry.register(Box::new(MockStrategy { 
            name: "GoodStrategy".to_string(), 
            should_fail: false 
        })).await.unwrap();

        let prices = vec![]; 
        let opps = registry.analyze_all(&prices).await;
        
        // Should get results from good strategy even if bad one fails
        assert_eq!(opps.len(), 1);
    }
}
