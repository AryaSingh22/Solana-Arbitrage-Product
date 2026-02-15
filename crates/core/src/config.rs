//! Configuration module for the arbitrage system

use std::env;

/// Application configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
    /// Database connection URL
    pub database_url: String,
    /// Redis connection URL
    pub redis_url: String,
    /// Solana RPC URL
    pub solana_rpc_url: String,
    /// Minimum profit threshold percentage
    pub min_profit_threshold: f64,
    /// Maximum age of price data before it is considered stale (seconds)
    pub max_price_age_seconds: i64,
    /// API server port
    pub api_port: u16,
    /// Log level
    pub log_level: String,
    /// Priority fee in micro-lamports per compute unit
    pub priority_fee_micro_lamports: u64,
    /// Compute unit limit per transaction
    pub compute_unit_limit: u32,
    /// RPC commitment level (processed, confirmed, finalized)
    pub rpc_commitment: String,
    /// Slippage tolerance in basis points (50 = 0.5%)
    pub slippage_bps: u64,
    /// Maximum retry attempts for failed transactions
    pub max_retries: u32,
    /// Whether to use Jito bundles for MEV protection
    pub use_jito: bool,
    /// Jito block engine URL
    pub jito_block_engine_url: String,
    /// Jito tip amount in lamports
    pub jito_tip_lamports: u64,
    /// Dry run mode
    pub dry_run: bool,
    /// Maximum daily loss allowed before pausing
    pub max_daily_loss: f64,
    /// Maximum consecutive losses before pausing
    pub max_consecutive_losses: u32,
    /// Whether circuit breaker is enabled
    pub circuit_breaker_enabled: bool,
    /// Circuit breaker cooling period in seconds
    pub circuit_breaker_timeout_seconds: u64,
    /// Telegram webhook URL for alerts
    pub telegram_webhook_url: Option<String>,
    /// Discord webhook URL for alerts
    pub discord_webhook_url: Option<String>,
    /// Polling interval in milliseconds
    pub poll_interval_ms: u64,
    /// Maximum concurrent trades
    pub max_concurrent_trades: usize,
    /// Enable Prometheus metrics
    pub enable_metrics: bool,
    /// Metrics server port
    pub metrics_port: u16,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, env::VarError> {
        Ok(Self {
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://postgres:postgres@localhost:5432/solana_arb".to_string()
            }),
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            solana_rpc_url: env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
            min_profit_threshold: env::var("MIN_PROFIT_THRESHOLD")
                .unwrap_or_else(|_| "0.5".to_string())
                .parse()
                .unwrap_or(0.5),
            max_price_age_seconds: env::var("MAX_PRICE_AGE_SECONDS")
                .unwrap_or_else(|_| "5".to_string())
                .parse()
                .unwrap_or(5),
            api_port: env::var("API_PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse()
                .unwrap_or(8080),
            log_level: env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string()),
            priority_fee_micro_lamports: env::var("PRIORITY_FEE")
                .unwrap_or_else(|_| "50000".to_string())
                .parse()
                .unwrap_or(50000),
            compute_unit_limit: env::var("COMPUTE_UNIT_LIMIT")
                .unwrap_or_else(|_| "200000".to_string())
                .parse()
                .unwrap_or(200000),
            rpc_commitment: env::var("RPC_COMMITMENT").unwrap_or_else(|_| "confirmed".to_string()),
            slippage_bps: env::var("SLIPPAGE_BPS")
                .unwrap_or_else(|_| "50".to_string())
                .parse()
                .unwrap_or(50),
            max_retries: env::var("MAX_RETRIES")
                .unwrap_or_else(|_| "3".to_string())
                .parse()
                .unwrap_or(3),
            use_jito: env::var("USE_JITO")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            jito_block_engine_url: env::var("JITO_BLOCK_ENGINE_URL")
                .unwrap_or_else(|_| "https://mainnet.block-engine.jito.wtf".to_string()),
            jito_tip_lamports: env::var("JITO_TIP_LAMPORTS")
                .unwrap_or_else(|_| "10000".to_string())
                .parse()
                .unwrap_or(10000),
            dry_run: env::var("DRY_RUN")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            max_daily_loss: env::var("MAX_DAILY_LOSS")
                .unwrap_or_else(|_| "500.0".to_string())
                .parse()
                .unwrap_or(500.0),
            max_consecutive_losses: env::var("MAX_CONSECUTIVE_LOSSES")
                .unwrap_or_else(|_| "5".to_string())
                .parse()
                .unwrap_or(5),
            circuit_breaker_enabled: env::var("CIRCUIT_BREAKER_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            circuit_breaker_timeout_seconds: env::var("CIRCUIT_BREAKER_TIMEOUT_SECONDS")
                .unwrap_or_else(|_| "300".to_string())
                .parse()
                .unwrap_or(300),
            telegram_webhook_url: env::var("TELEGRAM_WEBHOOK_URL").ok(),
            discord_webhook_url: env::var("DISCORD_WEBHOOK_URL").ok(),
            poll_interval_ms: env::var("POLL_INTERVAL_MS")
                .unwrap_or_else(|_| "500".to_string())
                .parse()
                .unwrap_or(500),
            max_concurrent_trades: env::var("MAX_CONCURRENT_TRADES")
                .unwrap_or_else(|_| "1".to_string())
                .parse()
                .unwrap_or(1),
            enable_metrics: env::var("ENABLE_METRICS")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
            metrics_port: env::var("METRICS_PORT")
                .unwrap_or_else(|_| "9090".to_string())
                .parse()
                .unwrap_or(9090),
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: "postgres://postgres:postgres@localhost:5432/solana_arb".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            solana_rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            min_profit_threshold: 0.5,
            max_price_age_seconds: 5,
            api_port: 8080,
            log_level: "info".to_string(),
            priority_fee_micro_lamports: 50000,
            compute_unit_limit: 200000,
            rpc_commitment: "confirmed".to_string(),
            slippage_bps: 50,
            max_retries: 3,
            use_jito: false,
            jito_block_engine_url: "https://mainnet.block-engine.jito.wtf".to_string(),
            jito_tip_lamports: 10000,
            dry_run: true,
            max_daily_loss: 500.0,
            max_consecutive_losses: 5,
            circuit_breaker_enabled: true,
            circuit_breaker_timeout_seconds: 300,
            telegram_webhook_url: None,
            discord_webhook_url: None,
            poll_interval_ms: 500,
            max_concurrent_trades: 1,
            enable_metrics: true,
            metrics_port: 9090,
        }
    }
}
