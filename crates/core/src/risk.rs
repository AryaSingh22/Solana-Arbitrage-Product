//! Risk Management Module
//!
//! Implements position sizing, exposure limits, and circuit breakers
//! for safe automated trading.

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use crate::events::{EventBus, TradingEvent};

pub mod circuit_breaker;
pub mod var;
pub mod volatility;

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
            max_position_size: Decimal::from(1000), // $1,000 max per trade
            max_total_exposure: Decimal::from(5000), // $5,000 total exposure
            max_daily_loss: Decimal::from(100),     // $100 daily loss limit
            min_profit_threshold: Decimal::new(5, 3), // 0.5% min profit
            max_slippage: Decimal::new(1, 2),       // 1% max slippage
            loss_cooldown_seconds: 300,             // 5 minute cooldown
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
    /// Circuit breaker
    pub circuit_breaker: circuit_breaker::CircuitBreaker,
    /// Volatility tracker
    pub volatility_tracker: volatility::VolatilityTracker,
    /// VaR calculator
    pub var_calculator: var::VarCalculator,
    /// Event bus for publishing risk events
    event_bus: Option<Arc<EventBus>>,
}

impl RiskManager {
    pub fn new(config: RiskConfig) -> Self {
        Self {
            config,
            positions: HashMap::new(),
            daily_trades: Vec::new(),
            last_loss_time: None,
            circuit_breaker: circuit_breaker::CircuitBreaker::new(3, 5, 300), // 3 failures, 5 successes, 5 min timeout
            volatility_tracker: volatility::VolatilityTracker::new(20), // 20-period moving average
            var_calculator: var::VarCalculator::new(0.95),              // 95% confidence
            event_bus: None,
        }
    }

    pub async fn set_event_bus(&mut self, event_bus: Arc<EventBus>) {
        self.event_bus = Some(event_bus.clone());
        self.circuit_breaker.set_event_bus(event_bus).await;
    }

    /// Check if a trade is allowed under current risk parameters
    pub async fn can_trade(&self, _pair: &str, size: Decimal) -> TradeDecision {
        // Check circuit breaker
        if !self.circuit_breaker.can_execute().await {
            let reason = "Circuit breaker OPEN - trading halted".to_string();
            if let Some(bus) = &self.event_bus {
                 bus.publish(TradingEvent::TradeRejected {
                     id: "pre-check".to_string(), // No opp ID here yet
                     reason: reason.clone(),
                 });
            }
            return TradeDecision::Rejected { reason };
        }

        // Check cooldown after loss
        if let Some(last_loss) = self.last_loss_time {
            let cooldown = Duration::seconds(self.config.loss_cooldown_seconds);
            if Utc::now() - last_loss < cooldown {
                let remaining = (last_loss + cooldown - Utc::now()).num_seconds();
                let reason = format!("Cooldown active - {} seconds remaining", remaining);
                if let Some(bus) = &self.event_bus {
                     bus.publish(TradingEvent::TradeRejected {
                         id: "pre-check".to_string(),
                         reason: reason.clone(),
                     });
                }
                return TradeDecision::Rejected { reason };
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
                let reason = "Maximum exposure limit reached".to_string();
                if let Some(bus) = &self.event_bus {
                     bus.publish(TradingEvent::TradeRejected {
                         id: "pre-check".to_string(),
                         reason: reason.clone(),
                     });
                }
                return TradeDecision::Rejected { reason };
            }
            return TradeDecision::Reduced {
                new_size: available,
                reason: "Size reduced due to exposure limit".to_string(),
            };
        }

        TradeDecision::Approved { size }
    }

    /// Calculate optimal position size based on risk parameters and volatility
    pub fn calculate_position_size(
        &self,
        pair: &str,
        expected_profit_pct: Decimal,
        available_liquidity: Decimal,
    ) -> Decimal {
        // Kelly criterion simplified: size = edge / odds
        // For arbitrage: size proportional to expected profit

        let base_size = self.config.max_position_size;

        // Scale down if profit is marginal
        let mut profit_factor = if expected_profit_pct > Decimal::from(2) {
            Decimal::ONE
        } else {
            expected_profit_pct / Decimal::from(2)
        };

        // Adjust for volatility if available
        if let Some(vol) = self.volatility_tracker.get_volatility(pair) {
            // If volatility is high (> 1%), reduce size
            // Simple model: scale = 1 / (1 + volatility_pct)
            // e.g. vol = 1% -> scale = 1/2 = 0.5
            // vol = 0.1% -> scale = 1/1.1 = ~0.9
            let vol_pct = vol * Decimal::from(100);
            if vol_pct > Decimal::ONE {
                let vol_scale = Decimal::ONE / vol_pct;
                profit_factor *= vol_scale;
            }
        }

        let calculated = base_size * profit_factor;

        // Don't exceed liquidity
        calculated
            .min(available_liquidity)
            .min(self.config.max_position_size)
    }

    /// Record a trade outcome
    pub async fn record_trade(&mut self, outcome: TradeOutcome) {
        if outcome.profit_loss < Decimal::ZERO {
            self.last_loss_time = Some(outcome.timestamp);
            self.circuit_breaker.record_failure().await;
        } else {
            self.circuit_breaker.record_success().await;
        }

        self.daily_trades.push(outcome);

        // Check if daily loss limit exceeded
        let daily_pnl: Decimal = self.daily_trades.iter().map(|t| t.profit_loss).sum();
        if daily_pnl < -self.config.max_daily_loss {
            // Force open circuit breaker
            // In a real impl, we'd have a specific method for this
            // For now, we simulate by recording enough failures
            for _ in 0..3 {
                self.circuit_breaker.record_failure().await;
            }
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

    /// Update price data for volatility tracking
    pub fn update_prices(&mut self, prices: &[crate::PriceData]) {
        for price in prices {
            // Use mid price for volatility
            let mid_price = (price.bid + price.ask) / Decimal::from(2);
            self.volatility_tracker
                .update_price(&price.pair.symbol(), mid_price);
        }
    }

    /// Reset daily statistics (call at start of new trading day)
    pub async fn reset_daily(&mut self) {
        self.daily_trades.clear();
        // Note: Circuit breaker state is persistent across days unless manually reset
        // Here we might want to reset it if it was triggered by daily loss
        // For now, allow it to remain as is
    }

    /// Check if trading is currently paused
    pub async fn is_paused(&self) -> bool {
        !self.circuit_breaker.can_execute().await
    }

    /// Get current risk status
    pub async fn status(&self) -> RiskStatus {
        let var = self
            .var_calculator
            .calculate_portfolio_var(&self.positions, &self.volatility_tracker);

        RiskStatus {
            total_exposure: self.total_exposure(),
            daily_pnl: self.daily_pnl(),
            portfolio_var: var,
            trades_today: self.daily_trades.len(),
            is_paused: self.is_paused().await,
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
    pub portfolio_var: Decimal,
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

    #[tokio::test]
    async fn test_trade_approval() {
        let manager = RiskManager::default();

        let decision = manager.can_trade("SOL/USDC", Decimal::from(500)).await;
        assert!(matches!(decision, TradeDecision::Approved { .. }));
    }

    #[tokio::test]
    async fn test_position_size_reduction() {
        let manager = RiskManager::default();

        // Request more than max
        let decision = manager.can_trade("SOL/USDC", Decimal::from(5000)).await;
        assert!(matches!(decision, TradeDecision::Reduced { .. }));
    }

    #[tokio::test]
    async fn test_circuit_breaker() {
        let config = RiskConfig {
            max_daily_loss: Decimal::from(50),
            ..Default::default()
        };
        let mut manager = RiskManager::new(config.clone());

        // Record a big loss
        manager.record_trade(TradeOutcome {
            timestamp: Utc::now(),
            pair: "SOL/USDC".to_string(),
            profit_loss: Decimal::from(-100),
            was_successful: false,
        }).await;

        assert!(manager.is_paused().await);

        let decision = manager.can_trade("SOL/USDC", Decimal::from(100)).await;
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
