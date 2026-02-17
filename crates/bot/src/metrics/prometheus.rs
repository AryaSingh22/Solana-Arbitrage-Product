use prometheus::{Gauge, Histogram, HistogramOpts, IntCounter, IntGauge, Registry};

#[allow(dead_code)]
pub struct MetricsCollector {
    registry: Registry,

    // Counters
    pub opportunities_detected: IntCounter,
    pub trades_attempted: IntCounter,
    pub trades_successful: IntCounter,
    pub trades_failed: IntCounter,

    // Gauges
    pub current_balance: Gauge,
    pub active_positions: IntGauge,
    pub circuit_breaker_state: IntGauge, // 0=closed, 1=half-open, 2=open

    // Histograms
    pub opportunity_profit: Histogram,
    pub trade_execution_time: Histogram,
    pub price_fetch_latency: Histogram,
    pub slippage_distribution: Histogram,
}

impl MetricsCollector {
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        // Initialize counters
        let opportunities_detected = IntCounter::new(
            "arb_opportunities_detected_total",
            "Total number of arbitrage opportunities detected",
        )?;
        registry.register(Box::new(opportunities_detected.clone()))?;

        let trades_attempted = IntCounter::new(
            "arb_trades_attempted_total",
            "Total number of trades attempted",
        )?;
        registry.register(Box::new(trades_attempted.clone()))?;

        let trades_successful = IntCounter::new(
            "arb_trades_successful_total",
            "Total number of successful trades",
        )?;
        registry.register(Box::new(trades_successful.clone()))?;

        let trades_failed =
            IntCounter::new("arb_trades_failed_total", "Total number of failed trades")?;
        registry.register(Box::new(trades_failed.clone()))?;

        // Initialize gauges
        let current_balance =
            Gauge::new("arb_current_balance_usd", "Current account balance in USD")?;
        registry.register(Box::new(current_balance.clone()))?;

        let active_positions = IntGauge::new(
            "arb_active_positions",
            "Number of currently active positions",
        )?;
        registry.register(Box::new(active_positions.clone()))?;

        let circuit_breaker_state = IntGauge::new(
            "arb_circuit_breaker_state",
            "Circuit breaker state (0=closed, 1=half-open, 2=open)",
        )?;
        registry.register(Box::new(circuit_breaker_state.clone()))?;

        // Initialize histograms
        let opportunity_profit = Histogram::with_opts(
            HistogramOpts::new(
                "arb_opportunity_profit_bps",
                "Distribution of opportunity profit in basis points",
            )
            .buckets(vec![10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0]),
        )?;
        registry.register(Box::new(opportunity_profit.clone()))?;

        let trade_execution_time = Histogram::with_opts(
            HistogramOpts::new(
                "arb_trade_execution_seconds",
                "Trade execution time in seconds",
            )
            .buckets(vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]),
        )?;
        registry.register(Box::new(trade_execution_time.clone()))?;

        let price_fetch_latency = Histogram::with_opts(
            HistogramOpts::new(
                "arb_price_fetch_seconds",
                "Price fetching latency in seconds",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0]),
        )?;
        registry.register(Box::new(price_fetch_latency.clone()))?;

        let slippage_distribution = Histogram::with_opts(
            HistogramOpts::new("arb_slippage_bps", "Slippage distribution in basis points")
                .buckets(vec![5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0]),
        )?;
        registry.register(Box::new(slippage_distribution.clone()))?;

        Ok(Self {
            registry,
            opportunities_detected,
            trades_attempted,
            trades_successful,
            trades_failed,
            current_balance,
            active_positions,
            circuit_breaker_state,
            opportunity_profit,
            trade_execution_time,
            price_fetch_latency,
            slippage_distribution,
        })
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }
}
