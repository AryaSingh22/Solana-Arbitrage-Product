//! Risk Management Module
//!
//! Implements position sizing, exposure limits, and circuit breakers
//! for safe automated trading.

use rust_decimal::Decimal;
use std::collections::HashMap;
use chrono::{DateTime, Utc, Duration};

/// Risk configuration parameters
#[derive(Debug, Clone)]
pub struct RiskConfig {
    /// Maximum position size in USD per trade
    pub max_position_size: Decimal,
    /// Maximum total exposure across all positions
    pub max_total_exposure: Decimal,
    /// Maximum loss per day before circuit breaker triggers
    pub max_daily_loss: Decimal,
    /// Minimum profit threshold to execute a trade
    pub min_profit_threshold: Decimal,
    /// Maximum slippage tolerance percentage
    pub max_slippage: Decimal,
    /// Cool-down period after a loss (seconds)
    pub loss_cooldown_seconds: i64,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_position_size: Decimal::from(1000),      // $1,000 max per trade
            max_total_exposure: Decimal::from(5000),     // $5,000 total exposure
            max_daily_loss: Decimal::from(100),          // $100 daily loss limit
            min_profit_threshold: Decimal::new(5, 3),    // 0.5% min profit
            max_slippage: Decimal::new(1, 2),            // 1% max slippage
            loss_cooldown_seconds: 300,                   // 5 minute cooldown
        }
    }
}

/// Trade outcome for tracking
#[derive(Debug, Clone)]
pub struct TradeOutcome {
    pub timestamp: DateTime<Utc>,
    pub pair: String,
    pub profit_loss: Decimal, // Positive = profit, negative = loss
    pub was_successful: bool,
}

/// Risk manager for controlling trade execution
pub struct RiskManager {
    config: RiskConfig,
    /// Current open positions by pair
    positions: HashMap<String, Decimal>,
    /// Trade history for the current day
    daily_trades: Vec<TradeOutcome>,
    /// Timestamp of last loss
    last_loss_time: Option<DateTime<Utc>>,
    /// Circuit breaker state
    circuit_breaker_triggered: bool,
}

impl RiskManager {
    pub fn new(config: RiskConfig) -> Self {
        Self {
            config,
            positions: HashMap::new(),
            daily_trades: Vec::new(),
            last_loss_time: None,
            circuit_breaker_triggered: false,
        }
    }

    /// Check if a trade is allowed under current risk parameters
    pub fn can_trade(&self, _pair: &str, size: Decimal) -> TradeDecision {
        // Check circuit breaker
        if self.circuit_breaker_triggered {
            return TradeDecision::Rejected {
                reason: "Circuit breaker triggered - daily loss limit exceeded".to_string(),
            };
        }

        // Check cooldown after loss
        if let Some(last_loss) = self.last_loss_time {
            let cooldown = Duration::seconds(self.config.loss_cooldown_seconds);
            if Utc::now() - last_loss < cooldown {
                let remaining = (last_loss + cooldown - Utc::now()).num_seconds();
                return TradeDecision::Rejected {
                    reason: format!("Cooldown active - {} seconds remaining", remaining),
                };
            }
        }

        // Check position size limit
        if size > self.config.max_position_size {
            return TradeDecision::Reduced {
                new_size: self.config.max_position_size,
                reason: "Size reduced to max position limit".to_string(),
            };
        }

        // Check total exposure
        let current_exposure: Decimal = self.positions.values().sum();
        if current_exposure + size > self.config.max_total_exposure {
            let available = self.config.max_total_exposure - current_exposure;
            if available <= Decimal::ZERO {
                return TradeDecision::Rejected {
                    reason: "Maximum exposure limit reached".to_string(),
                };
            }
            return TradeDecision::Reduced {
                new_size: available,
                reason: "Size reduced due to exposure limit".to_string(),
            };
        }

        TradeDecision::Approved { size }
    }

    /// Calculate optimal position size based on risk parameters
    pub fn calculate_position_size(
        &self,
        expected_profit_pct: Decimal,
        available_liquidity: Decimal,
    ) -> Decimal {
        // Kelly criterion simplified: size = edge / odds
        // For arbitrage: size proportional to expected profit
        
        let base_size = self.config.max_position_size;
        
        // Scale down if profit is marginal
        let profit_factor = if expected_profit_pct > Decimal::from(2) {
            Decimal::ONE
        } else {
            expected_profit_pct / Decimal::from(2)
        };

        let calculated = base_size * profit_factor;
        
        // Don't exceed liquidity
        calculated.min(available_liquidity).min(self.config.max_position_size)
    }

    /// Record a trade outcome
    pub fn record_trade(&mut self, outcome: TradeOutcome) {
        if outcome.profit_loss < Decimal::ZERO {
            self.last_loss_time = Some(outcome.timestamp);
        }

        self.daily_trades.push(outcome);

        // Check if daily loss limit exceeded
        let daily_pnl: Decimal = self.daily_trades.iter().map(|t| t.profit_loss).sum();
        if daily_pnl < -self.config.max_daily_loss {
            self.circuit_breaker_triggered = true;
        }
    }

    /// Update position tracking
    pub fn update_position(&mut self, pair: &str, size: Decimal) {
        if size.is_zero() {
            self.positions.remove(pair);
        } else {
            self.positions.insert(pair.to_string(), size);
        }
    }

    /// Get current total exposure
    pub fn total_exposure(&self) -> Decimal {
        self.positions.values().sum()
    }

    /// Get daily P&L
    pub fn daily_pnl(&self) -> Decimal {
        self.daily_trades.iter().map(|t| t.profit_loss).sum()
    }

    /// Reset daily statistics (call at start of new trading day)
    pub fn reset_daily(&mut self) {
        self.daily_trades.clear();
        self.circuit_breaker_triggered = false;
    }

    /// Check if trading is currently paused
    pub fn is_paused(&self) -> bool {
        self.circuit_breaker_triggered
    }

    /// Get current risk status
    pub fn status(&self) -> RiskStatus {
        RiskStatus {
            total_exposure: self.total_exposure(),
            daily_pnl: self.daily_pnl(),
            trades_today: self.daily_trades.len(),
            is_paused: self.is_paused(),
            positions: self.positions.clone(),
        }
    }
}

/// Decision from risk manager
#[derive(Debug, Clone)]
pub enum TradeDecision {
    Approved { size: Decimal },
    Reduced { new_size: Decimal, reason: String },
    Rejected { reason: String },
}

/// Current risk status
#[derive(Debug, Clone)]
pub struct RiskStatus {
    pub total_exposure: Decimal,
    pub daily_pnl: Decimal,
    pub trades_today: usize,
    pub is_paused: bool,
    pub positions: HashMap<String, Decimal>,
}

impl Default for RiskManager {
    fn default() -> Self {
        Self::new(RiskConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trade_approval() {
        let manager = RiskManager::default();
        
        let decision = manager.can_trade("SOL/USDC", Decimal::from(500));
        assert!(matches!(decision, TradeDecision::Approved { .. }));
    }

    #[test]
    fn test_position_size_reduction() {
        let manager = RiskManager::default();
        
        // Request more than max
        let decision = manager.can_trade("SOL/USDC", Decimal::from(5000));
        assert!(matches!(decision, TradeDecision::Reduced { .. }));
    }

    #[test]
    fn test_circuit_breaker() {
        let config = RiskConfig {
            max_daily_loss: Decimal::from(50),
            ..Default::default()
        };
        let mut manager = RiskManager::new(config);

        // Record a big loss
        manager.record_trade(TradeOutcome {
            timestamp: Utc::now(),
            pair: "SOL/USDC".to_string(),
            profit_loss: Decimal::from(-100),
            was_successful: false,
        });

        assert!(manager.is_paused());
        
        let decision = manager.can_trade("SOL/USDC", Decimal::from(100));
        assert!(matches!(decision, TradeDecision::Rejected { .. }));
    }

    #[test]
    fn test_position_tracking() {
        let mut manager = RiskManager::default();
        
        manager.update_position("SOL/USDC", Decimal::from(1000));
        manager.update_position("RAY/USDC", Decimal::from(500));
        
        assert_eq!(manager.total_exposure(), Decimal::from(1500));
    }
}
