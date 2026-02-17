//! Dynamic configuration management with validation and hot-reload
//!
//! Provides a `ConfigManager` that loads trading configuration from a JSON file,
//! validates all values on load, and supports hot-reloading via file change detection.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Complete dynamic trading configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicConfig {
    /// Config version for tracking changes
    pub version: String,
    /// Trading parameters
    pub trading: TradingConfig,
    /// Risk management parameters
    pub risk: RiskConfig,
    /// Performance tuning
    pub performance: PerformanceConfig,
    /// Alert configuration
    pub alerts: AlertConfig,
}

/// Trading-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    /// Master switch for trading
    pub enabled: bool,
    /// Maximum position size in USD
    pub max_position_size: u64,
    /// Minimum profit threshold in basis points
    pub min_profit_bps: f64,
    /// Maximum allowed slippage in basis points
    pub max_slippage_bps: u64,
}

/// Risk management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Enable/disable circuit breaker
    pub circuit_breaker_enabled: bool,
    /// Max consecutive losses before circuit breaker opens
    pub max_consecutive_losses: u32,
    /// Maximum daily loss in USD
    pub max_daily_loss: f64,
    /// Value at Risk limit percentage
    pub var_limit_percent: f64,
}

/// Performance tuning configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Price polling interval in milliseconds
    pub poll_interval_ms: u64,
    /// Enable WebSocket streaming (vs HTTP polling only)
    pub enable_websocket: bool,
    /// Enable parallel price fetching across DEXs
    pub enable_parallel_fetching: bool,
}

/// Alert/notification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    /// Send alerts via Telegram
    pub telegram_enabled: bool,
    /// Send alerts via Discord
    pub discord_enabled: bool,
    /// Minimum profit (USD) before alerting
    pub alert_on_profit: f64,
    /// Minimum loss (USD) before alerting
    pub alert_on_loss: f64,
}

impl DynamicConfig {
    /// Validate all configuration values
    pub fn validate(&self) -> Result<(), String> {
        if self.trading.max_position_size == 0 {
            return Err("trading.max_position_size must be > 0".into());
        }
        if self.trading.min_profit_bps < 0.0 {
            return Err("trading.min_profit_bps must be >= 0".into());
        }
        if self.trading.max_slippage_bps == 0 {
            return Err("trading.max_slippage_bps must be > 0".into());
        }
        if self.risk.max_daily_loss <= 0.0 {
            return Err("risk.max_daily_loss must be > 0".into());
        }
        if self.risk.var_limit_percent <= 0.0 || self.risk.var_limit_percent > 100.0 {
            return Err("risk.var_limit_percent must be between 0 and 100".into());
        }
        if self.performance.poll_interval_ms < 50 {
            return Err("performance.poll_interval_ms must be >= 50ms".into());
        }
        if self.alerts.alert_on_loss < 0.0 {
            return Err("alerts.alert_on_loss must be >= 0".into());
        }

        Ok(())
    }
}

/// Manages dynamic configuration with hot-reload support
pub struct ConfigManager {
    config: Arc<RwLock<DynamicConfig>>,
    config_path: PathBuf,
}

impl ConfigManager {
    /// Load configuration from a JSON file
    pub fn new(config_path: impl AsRef<Path>) -> Result<Self, String> {
        let path = config_path.as_ref().to_path_buf();
        let config = Self::load_config(&path)?;
        config.validate()?;

        info!("Configuration loaded from {:?} (version: {})", path, config.version);

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            config_path: path,
        })
    }

    /// Get a snapshot of the current configuration
    pub async fn get(&self) -> DynamicConfig {
        self.config.read().await.clone()
    }

    /// Reload configuration from disk
    pub async fn reload(&self) -> Result<(), String> {
        let new_config = Self::load_config(&self.config_path)?;
        new_config.validate()?;

        let old_version = {
            let current = self.config.read().await;
            current.version.clone()
        };

        let mut config = self.config.write().await;
        *config = new_config;

        info!(
            "Configuration reloaded: {} → {}",
            old_version, config.version
        );

        Ok(())
    }

    /// Get a cloneable reference to the config Arc for sharing
    pub fn shared(&self) -> Arc<RwLock<DynamicConfig>> {
        self.config.clone()
    }

    fn load_config(path: &Path) -> Result<DynamicConfig, String> {
        let data = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config from {:?}: {}", path, e))?;

        let config: DynamicConfig = serde_json::from_str(&data)
            .map_err(|e| format!("Failed to parse config JSON: {}", e))?;

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> DynamicConfig {
        DynamicConfig {
            version: "1.0.0".to_string(),
            trading: TradingConfig {
                enabled: true,
                max_position_size: 1000,
                min_profit_bps: 50.0,
                max_slippage_bps: 100,
            },
            risk: RiskConfig {
                circuit_breaker_enabled: true,
                max_consecutive_losses: 5,
                max_daily_loss: 500.0,
                var_limit_percent: 2.0,
            },
            performance: PerformanceConfig {
                poll_interval_ms: 500,
                enable_websocket: true,
                enable_parallel_fetching: true,
            },
            alerts: AlertConfig {
                telegram_enabled: true,
                discord_enabled: false,
                alert_on_profit: 50.0,
                alert_on_loss: 10.0,
            },
        }
    }

    #[test]
    fn test_valid_config_passes() {
        assert!(valid_config().validate().is_ok());
    }

    #[test]
    fn test_zero_position_size_fails() {
        let mut c = valid_config();
        c.trading.max_position_size = 0;
        assert!(c.validate().is_err());
    }

    #[test]
    fn test_negative_daily_loss_fails() {
        let mut c = valid_config();
        c.risk.max_daily_loss = -100.0;
        assert!(c.validate().is_err());
    }

    #[test]
    fn test_poll_interval_too_low_fails() {
        let mut c = valid_config();
        c.performance.poll_interval_ms = 10;
        assert!(c.validate().is_err());
    }

    #[test]
    fn test_var_limit_out_of_range_fails() {
        let mut c = valid_config();
        c.risk.var_limit_percent = 150.0;
        assert!(c.validate().is_err());
    }

    #[tokio::test]
    async fn test_config_manager_load_and_get() {
        // Write a temp config file
        let dir = std::env::temp_dir().join("arb_config_test");
        std::fs::create_dir_all(&dir).ok();
        let config_path = dir.join("test_config.json");

        let config = valid_config();
        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&config_path, &json).unwrap();

        let manager = ConfigManager::new(&config_path).unwrap();
        let loaded = manager.get().await;

        assert_eq!(loaded.version, "1.0.0");
        assert_eq!(loaded.trading.max_position_size, 1000);

        // Cleanup
        let _ = std::fs::remove_file(&config_path);
        let _ = std::fs::remove_dir(&dir);
    }
}
