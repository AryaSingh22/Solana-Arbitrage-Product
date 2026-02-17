use crate::risk::volatility::VolatilityTracker;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

/// Value at Risk (VaR) Calculator
#[allow(dead_code)]
pub struct VarCalculator {
    /// Confidence level (e.g., 0.95 or 0.99)
    confidence_level: f64,
    /// Z-score corresponding to confidence level
    z_score: f64,
}

impl VarCalculator {
    pub fn new(confidence_level: f64) -> Self {
        // Approximate Z-scores for common confidence levels
        let z_score = if confidence_level >= 0.99 {
            2.326
        } else if confidence_level >= 0.95 {
            1.645
        } else {
            1.282 // 90%
        };

        Self {
            confidence_level,
            z_score,
        }
    }

    /// Calculate VaR for a single position
    /// VaR = Position Value * Volatility * Z-Score
    pub fn calculate_var(&self, position_value: Decimal, volatility: Decimal) -> Decimal {
        let position_f64 = position_value.to_f64().unwrap_or(0.0);
        let vol_f64 = volatility.to_f64().unwrap_or(0.0);

        let var = position_f64 * vol_f64 * self.z_score;

        Decimal::try_from(var).unwrap_or(Decimal::ZERO)
    }

    /// Calculate Portfolio VaR (assuming perfect correlation for worst-case)
    /// In reality, we should use covariance matrix, but for arbitrage (SOL-based),
    /// pairs are highly correlated.
    pub fn calculate_portfolio_var(
        &self,
        positions: &std::collections::HashMap<String, Decimal>,
        vol_tracker: &VolatilityTracker,
    ) -> Decimal {
        let mut total_var = Decimal::ZERO;

        for (pair, &size) in positions {
            if let Some(vol) = vol_tracker.get_volatility(pair) {
                total_var += self.calculate_var(size, vol);
            } else {
                // Fallback minimal volatility if unknown
                total_var += self.calculate_var(size, Decimal::new(1, 2)); // 1%
            }
        }

        total_var
    }
}
