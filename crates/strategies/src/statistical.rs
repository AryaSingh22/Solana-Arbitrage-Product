use crate::Strategy;
use async_trait::async_trait;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use solana_arb_core::{
    types::{ArbitrageOpportunity, DexType, PriceData},
    ArbitrageResult,
};
use std::collections::VecDeque;
use tokio::sync::RwLock;

pub struct StatisticalArbitrage {
    // Sliding window of price ratios for pairs
    // Key: Pair symbol, Value: Queue of (price_ratio, timestamp)
    history: RwLock<std::collections::HashMap<String, VecDeque<(Decimal, i64)>>>,
    window_size: usize,
    z_score_threshold: Decimal,
}

impl StatisticalArbitrage {
    pub fn new(window_size: usize, z_score_threshold: Decimal) -> Self {
        Self {
            history: RwLock::new(std::collections::HashMap::new()),
            window_size,
            z_score_threshold,
        }
    }

    fn calculate_z_score(
        &self,
        value: Decimal,
        history: &VecDeque<(Decimal, i64)>,
    ) -> Option<Decimal> {
        if history.len() < self.window_size {
            return None;
        }

        let sum: Decimal = history.iter().map(|(v, _)| *v).sum();
        let count = Decimal::from(history.len());
        let mean = sum / count;

        let variance_sum: Decimal = history.iter().map(|(v, _)| (*v - mean) * (*v - mean)).sum();

        if variance_sum.is_zero() {
            return Some(Decimal::ZERO);
        }

        let variance = variance_sum / count;
        // Decimal sqrt is not standard, convert to f64
        let std_dev = variance
            .to_f64()
            .map(|f| f.sqrt())
            .and_then(Decimal::from_f64_retain)?;

        if std_dev.is_zero() {
            return Some(Decimal::ZERO);
        }

        Some((value - mean) / std_dev)
    }
}

#[async_trait]
impl Strategy for StatisticalArbitrage {
    fn name(&self) -> &'static str {
        "Statistical Arbitrage (Mean Reversion)"
    }

    async fn update_state(&self, price: &PriceData) -> ArbitrageResult<()> {
        let mut history = self.history.write().await;
        let pair_symbol = price.pair.symbol();

        let entry = history.entry(pair_symbol).or_insert_with(VecDeque::new);
        entry.push_back((price.mid_price, price.timestamp.timestamp()));

        if entry.len() > self.window_size {
            entry.pop_front();
        }

        Ok(())
    }

    async fn analyze(&self, prices: &[PriceData]) -> ArbitrageResult<Vec<ArbitrageOpportunity>> {
        let history = self.history.read().await;
        let mut opportunities = Vec::new();

        for price in prices {
            if let Some(queue) = history.get(&price.pair.symbol()) {
                if let Some(z_score) = self.calculate_z_score(price.mid_price, queue) {
                    // Mean reversion logic:
                    // If Z-score > threshold, price is historically high -> SELL or SHORT
                    // If Z-score < -threshold, price is historically low -> BUY or LONG

                    if z_score.abs() > self.z_score_threshold {
                        tracing::info!(
                            "📈 StatArb signal: {} Z-score {} (Threshold {})",
                            price.pair.symbol(),
                            z_score,
                            self.z_score_threshold
                        );

                        // Calculate the historical mean for profit estimation
                        let sum: Decimal = queue.iter().map(|(v, _)| *v).sum();
                        let mean = sum / Decimal::from(queue.len());

                        // Determine trade direction:
                        //  z > 0 → price above mean → expect reversion down → sell on this DEX, buy on another
                        //  z < 0 → price below mean → expect reversion up → buy on this DEX, sell on another
                        let (buy_dex, sell_dex, buy_price, sell_price) = if z_score > Decimal::ZERO {
                            // Price is high: sell on current DEX at ask price, expect to buy at mean
                            (DexType::Jupiter, price.dex, mean, price.ask)
                        } else {
                            // Price is low: buy on current DEX at ask price, expect to sell at mean
                            (price.dex, DexType::Jupiter, price.ask, mean)
                        };

                        // Gross profit as percentage of buy price
                        let gross_profit_pct = if buy_price.is_zero() {
                            Decimal::ZERO
                        } else {
                            ((sell_price - buy_price) / buy_price) * Decimal::from(100)
                        };

                        // Net profit after estimated fees
                        let total_fees = buy_dex.fee_percentage() + sell_dex.fee_percentage();
                        let net_profit_pct = gross_profit_pct - total_fees;

                        // Only create opportunity if net profit is positive
                        if net_profit_pct > Decimal::ZERO {
                            // Confidence-based position sizing: higher |z-score| → more confidence
                            let confidence = z_score.abs().to_f64().unwrap_or(0.0);
                            let base_size = Decimal::from(100); // $100 base
                            let recommended_size = base_size * Decimal::from_f64_retain(confidence.min(5.0))
                                .unwrap_or(Decimal::ONE);

                            let estimated_profit = recommended_size * net_profit_pct / Decimal::from(100);

                            let opp = ArbitrageOpportunity {
                                id: uuid::Uuid::new_v4(),
                                pair: price.pair.clone(),
                                buy_dex,
                                sell_dex,
                                buy_price,
                                sell_price,
                                gross_profit_pct,
                                net_profit_pct,
                                estimated_profit_usd: Some(estimated_profit),
                                recommended_size: Some(recommended_size),
                                detected_at: chrono::Utc::now(),
                                expired_at: None,
                            };

                            tracing::info!(
                                "💡 StatArb opportunity: {} buy@{} on {:?}, sell@{} on {:?} (net {:.4}%)",
                                price.pair.symbol(),
                                buy_price,
                                buy_dex,
                                sell_price,
                                sell_dex,
                                net_profit_pct
                            );

                            opportunities.push(opp);
                        }
                    }
                }
            }
        }

        Ok(opportunities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_arb_core::TokenPair;
    use std::collections::VecDeque;

    fn build_history(values: &[f64]) -> VecDeque<(Decimal, i64)> {
        values
            .iter()
            .enumerate()
            .map(|(i, &v)| (Decimal::from_f64_retain(v).unwrap(), i as i64))
            .collect()
    }

    #[test]
    fn test_z_score_calculation_basic() {
        let strat = StatisticalArbitrage::new(5, Decimal::from(2));
        // mean=100, std_dev≈0 for identical values → z=0
        let history = build_history(&[100.0, 100.0, 100.0, 100.0, 100.0]);
        let z = strat.calculate_z_score(Decimal::from(100), &history);
        assert_eq!(z, Some(Decimal::ZERO));
    }

    #[test]
    fn test_z_score_insufficient_data() {
        let strat = StatisticalArbitrage::new(10, Decimal::from(2));
        let history = build_history(&[100.0, 101.0, 102.0]); // only 3, need 10
        let z = strat.calculate_z_score(Decimal::from(100), &history);
        assert!(z.is_none());
    }

    #[test]
    fn test_z_score_high_value() {
        let strat = StatisticalArbitrage::new(5, Decimal::from(2));
        // Values: 100, 101, 102, 103, 104 → mean=102, value=110 → positive z
        let history = build_history(&[100.0, 101.0, 102.0, 103.0, 104.0]);
        let z = strat.calculate_z_score(Decimal::from(110), &history);
        assert!(z.is_some());
        assert!(z.unwrap() > Decimal::from(2), "Z-score should be > 2 for outlier");
    }

    #[tokio::test]
    async fn test_analyze_creates_opportunity_above_threshold() {
        let strat = StatisticalArbitrage::new(5, Decimal::from(2));

        // Seed history with slightly varying prices so variance is nonzero
        for &v in &[99.0, 100.0, 101.0, 100.5, 99.5] {
            let d = Decimal::from_f64_retain(v).unwrap();
            let price = PriceData::new(
                DexType::Raydium,
                TokenPair::new("SOL", "USDC"),
                d,
                d,
            );
            strat.update_state(&price).await.unwrap();
        }

        // Now present a far-outlier price
        let outlier = PriceData::new(
            DexType::Raydium,
            TokenPair::new("SOL", "USDC"),
            Decimal::from(120), // bid well above mean
            Decimal::from(121), // ask well above mean
        );

        let opps = strat.analyze(&[outlier]).await.unwrap();
        assert!(
            !opps.is_empty(),
            "Should create opportunity when z-score exceeds threshold"
        );

        let opp = &opps[0];
        assert_eq!(opp.pair.base, "SOL");
        assert!(opp.net_profit_pct > Decimal::ZERO);
        assert!(opp.estimated_profit_usd.is_some());
        assert!(opp.recommended_size.is_some());
    }

    #[tokio::test]
    async fn test_analyze_no_opportunity_below_threshold() {
        let strat = StatisticalArbitrage::new(5, Decimal::from(2));

        // Seed with slightly varying prices
        for v in &[100.0, 100.1, 99.9, 100.05, 99.95] {
            let price = PriceData::new(
                DexType::Raydium,
                TokenPair::new("SOL", "USDC"),
                Decimal::from_f64_retain(*v).unwrap(),
                Decimal::from_f64_retain(*v).unwrap(),
            );
            strat.update_state(&price).await.unwrap();
        }

        // Present a price very close to the mean
        let normal = PriceData::new(
            DexType::Raydium,
            TokenPair::new("SOL", "USDC"),
            Decimal::from(100),
            Decimal::from(100),
        );

        let opps = strat.analyze(&[normal]).await.unwrap();
        assert!(
            opps.is_empty(),
            "Should NOT create opportunity when z-score is below threshold"
        );
    }

    #[tokio::test]
    async fn test_analyze_negative_z_score_direction() {
        let strat = StatisticalArbitrage::new(5, Decimal::from(2));

        // Seed history with slightly varying prices so variance is nonzero
        for &v in &[99.0, 100.0, 101.0, 100.5, 99.5] {
            let d = Decimal::from_f64_retain(v).unwrap();
            let price = PriceData::new(
                DexType::Orca,
                TokenPair::new("SOL", "USDC"),
                d,
                d,
            );
            strat.update_state(&price).await.unwrap();
        }

        // Price crashes below mean → negative z-score → buy opportunity
        let low = PriceData::new(
            DexType::Orca,
            TokenPair::new("SOL", "USDC"),
            Decimal::from(80),
            Decimal::from(81),
        );

        let opps = strat.analyze(&[low]).await.unwrap();
        assert!(!opps.is_empty());

        let opp = &opps[0];
        // When price is low: buy on current DEX, sell on Jupiter (target mean)
        assert_eq!(opp.buy_dex, DexType::Orca);
    }
}
