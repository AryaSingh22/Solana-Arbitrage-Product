use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolendReserve {
    pub symbol: String,
    pub address: String, // Base58 encoded Pubkey
    pub liquidity_supply_pubkey: String,
    pub liquidity_fee_receiver: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolendConfig {
    pub reserves: Vec<SolendReserve>,
    pub lending_market: String,
}

pub struct SolendConfigManager {
    config: Arc<RwLock<SolendConfig>>,
    reserve_map: Arc<RwLock<HashMap<String, Pubkey>>>,
}

impl SolendConfigManager {
    pub fn new(config: SolendConfig) -> Self {
        let mut map = HashMap::new();
        for reserve in &config.reserves {
            if let Ok(pubkey) = Pubkey::from_str(&reserve.address) {
                map.insert(reserve.symbol.clone(), pubkey);
            }
        }

        Self {
            config: Arc::new(RwLock::new(config)),
            reserve_map: Arc::new(RwLock::new(map)),
        }
    }

    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: SolendConfig = serde_json::from_str(&content)?;
        Ok(Self::new(config))
    }

    pub async fn get_reserve_pubkey(&self, symbol: &str) -> Option<Pubkey> {
        let map = self.reserve_map.read().await;
        map.get(symbol).cloned()
    }

    pub async fn update_config(&self, new_config: SolendConfig) {
        let mut config = self.config.write().await;
        let mut map = self.reserve_map.write().await;
        
        *config = new_config.clone();
        map.clear();
        for reserve in &new_config.reserves {
            if let Ok(pubkey) = Pubkey::from_str(&reserve.address) {
                map.insert(reserve.symbol.clone(), pubkey);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let json = r#"{
            "lending_market": "4UpD2fh7xH3VP9QQaXtsS1YY3bxzWhtfpks7FatyKvdY",
            "reserves": [
                {
                    "symbol": "SOL",
                    "address": "8Pbodeaos3mpNo5SktQLD7PDi1TuHbS439LQPnpsJaRw",
                    "liquidity_supply_pubkey": "8UviNr47S8eL6JRPkC5dZqRfrscucqCkmKygroGAC6z8"
                }
            ]
        }"#;
        
        let config: SolendConfig = serde_json::from_str(json).unwrap();
        let manager = SolendConfigManager::new(config);
        
        let rt = tokio::runtime::Runtime::new().unwrap();
        let pubkey = rt.block_on(manager.get_reserve_pubkey("SOL"));
        
        assert!(pubkey.is_some());
        assert_eq!(pubkey.unwrap().to_string(), "8Pbodeaos3mpNo5SktQLD7PDi1TuHbS439LQPnpsJaRw");
    }

    #[test]
    fn test_update_config() {
        let config = SolendConfig {
            lending_market: "market1".to_string(),
            reserves: vec![],
        };
        let manager = SolendConfigManager::new(config);
        
        let new_config = SolendConfig {
            lending_market: "market1".to_string(),
            reserves: vec![
                SolendReserve {
                    symbol: "USDC".to_string(),
                    address: "BgxfHJDzm44T7XG68MYKx7YisTjZu73tVovyZSjJMpmw".to_string(),
                    liquidity_supply_pubkey: "supply1".to_string(),
                    liquidity_fee_receiver: None,
                }
            ],
        };
        
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(manager.update_config(new_config));
        
        let pubkey = rt.block_on(manager.get_reserve_pubkey("USDC"));
        assert!(pubkey.is_some());
        assert_eq!(pubkey.unwrap().to_string(), "BgxfHJDzm44T7XG68MYKx7YisTjZu73tVovyZSjJMpmw");
    }
}
