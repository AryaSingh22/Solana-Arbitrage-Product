//! Solana Arbitrage API Server
//!
//! REST and WebSocket API for the Arbitrage Dashboard

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

use solana_arb_core::{
    arbitrage::ArbitrageDetector,
    config::Config,
    dex::{jupiter::JupiterProvider, orca::OrcaProvider, raydium::RaydiumProvider, DexProvider},
    ArbitrageConfig, PriceData, TokenPair,
};

/// Application state shared across handlers
struct AppState {
    detector: RwLock<ArbitrageDetector>,
    providers: Vec<Box<dyn DexProvider>>,
    config: Config,
    dry_run: bool,
    simulated_pnl: RwLock<f64>,
    simulated_trades: RwLock<u32>,
    // Liveness tracking
    heartbeat_count: RwLock<u64>,
    last_scan_at: RwLock<DateTime<Utc>>,
    dex_health: RwLock<HashMap<String, DexHealthStatus>>,
}

/// DEX health status for monitoring
#[derive(Debug, Clone, Serialize)]
struct DexHealthStatus {
    name: String,
    last_success_at: Option<DateTime<Utc>>,
    consecutive_errors: u32,
    status: String, // "green", "yellow", "red"
}

#[derive(Debug, Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    fn error(message: impl Into<String>) -> ApiResponse<()> {
        ApiResponse {
            success: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PairQuery {
    base: Option<String>,
    quote: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpportunitiesQuery {
    min_profit: Option<f64>,
    limit: Option<usize>,
}

/// Default trading pairs
fn default_pairs() -> Vec<TokenPair> {
    vec![
        TokenPair::new("SOL", "USDC"),
        TokenPair::new("SOL", "USDT"),
        TokenPair::new("RAY", "USDC"),
        TokenPair::new("ORCA", "USDC"),
        TokenPair::new("JUP", "USDC"),
        TokenPair::new("BONK", "SOL"),
    ]
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment
    dotenvy::dotenv().ok();

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting Solana Arbitrage API Server");

    // Load configuration
    let config = Config::from_env().unwrap_or_default();

    // Initialize DEX providers
    let providers: Vec<Box<dyn DexProvider>> = vec![
        Box::new(JupiterProvider::new()),
        Box::new(RaydiumProvider::new()),
        Box::new(OrcaProvider::new()),
    ];

    // Initialize detector
    let arb_config = ArbitrageConfig {
        min_profit_threshold: rust_decimal::Decimal::try_from(config.min_profit_threshold)
            .unwrap_or_default(),
        ..Default::default()
    };
    let detector = RwLock::new(ArbitrageDetector::new(arb_config));

    // Read DRY_RUN from environment
    let dry_run = std::env::var("DRY_RUN")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);

    // Create app state
    let state = Arc::new(AppState {
        detector,
        providers,
        config: config.clone(),
        dry_run,
        simulated_pnl: RwLock::new(0.0),
        simulated_trades: RwLock::new(0),
        heartbeat_count: RwLock::new(0),
        last_scan_at: RwLock::new(Utc::now()),
        dex_health: RwLock::new(HashMap::new()),
    });

    // Spawn background price collector
    let collector_state = state.clone();
    tokio::spawn(async move {
        let pairs = default_pairs();
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
        
        loop {
            interval.tick().await;
            
            // Increment heartbeat
            {
                let mut count = collector_state.heartbeat_count.write().await;
                *count += 1;
                let mut last_scan = collector_state.last_scan_at.write().await;
                *last_scan = Utc::now();
            }
            
            for provider in &collector_state.providers {
                let dex_name = format!("{:?}", provider.dex_type());
                
                match provider.get_prices(&pairs).await {
                    Ok(prices) => {
                        let mut detector = collector_state.detector.write().await;
                        detector.update_prices(prices);
                        
                        // Update DEX health - success
                        let mut health = collector_state.dex_health.write().await;
                        health.insert(dex_name.clone(), DexHealthStatus {
                            name: dex_name,
                            last_success_at: Some(Utc::now()),
                            consecutive_errors: 0,
                            status: "green".to_string(),
                        });
                    }
                    Err(_e) => {
                        // Update DEX health - error
                        let mut health = collector_state.dex_health.write().await;
                        let entry = health.entry(dex_name.clone()).or_insert(DexHealthStatus {
                            name: dex_name.clone(),
                            last_success_at: None,
                            consecutive_errors: 0,
                            status: "red".to_string(),
                        });
                        entry.consecutive_errors += 1;
                        entry.status = if entry.consecutive_errors >= 5 { "red" } else { "yellow" }.to_string();
                    }
                }
            }
        }
    });

    // Build router
    let app = Router::new()
        // Health check
        .route("/health", get(health_check))
        // Opportunities endpoints
        .route("/api/opportunities", get(get_opportunities))
        .route("/api/opportunities/:id", get(get_opportunity))
        // Price endpoints
        .route("/api/prices", get(get_prices))
        .route("/api/prices/:pair", get(get_pair_prices))
        // Config endpoints
        .route("/api/config", get(get_config))
        // Status endpoint (DRY_RUN visibility)
        .route("/api/status", get(get_status))
        // Add CORS for frontend
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    // Start server
    let addr = format!("0.0.0.0:{}", config.api_port);
    info!("API server listening on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
    })))
}

/// Get current arbitrage opportunities
async fn get_opportunities(
    State(state): State<Arc<AppState>>,
    Query(params): Query<OpportunitiesQuery>,
) -> impl IntoResponse {
    let detector = state.detector.read().await;
    let mut opportunities = detector.find_all_opportunities();

    // Filter by minimum profit if specified
    if let Some(min_profit) = params.min_profit {
        opportunities.retain(|o| {
            o.net_profit_pct >= rust_decimal::Decimal::try_from(min_profit).unwrap_or_default()
        });
    }

    // Limit results
    let limit = params.limit.unwrap_or(50);
    opportunities.truncate(limit);

    Json(ApiResponse::success(opportunities))
}

/// Get a specific opportunity by ID
async fn get_opportunity(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let detector = state.detector.read().await;
    let opportunities = detector.find_all_opportunities();
    
    match solana_arb_core::Uuid::parse_str(&id) {
        Ok(uuid) => {
            if let Some(opp) = opportunities.iter().find(|o| o.id == uuid) {
                Json(ApiResponse::success(opp.clone())).into_response()
            } else {
                (StatusCode::NOT_FOUND, Json(ApiResponse::<()>::error("Opportunity not found"))).into_response()
            }
        }
        Err(_) => {
            (StatusCode::BAD_REQUEST, Json(ApiResponse::<()>::error("Invalid UUID"))).into_response()
        }
    }
}

/// Get current prices from all DEXs
async fn get_prices(
    State(state): State<Arc<AppState>>,
    Query(params): Query<PairQuery>,
) -> impl IntoResponse {
    let detector = state.detector.read().await;
    let prices = detector.get_prices();

    let result: Vec<_> = prices.values().cloned().collect();

    // Filter by pair if specified
    let filtered: Vec<PriceData> = if params.base.is_some() || params.quote.is_some() {
        result.into_iter()
            .filter(|p| {
                let base_match = params.base.as_ref().map_or(true, |b| &p.pair.base == b);
                let quote_match = params.quote.as_ref().map_or(true, |q| &p.pair.quote == q);
                base_match && quote_match
            })
            .collect()
    } else {
        result
    };

    Json(ApiResponse::success(filtered))
}

/// Get prices for a specific pair
async fn get_pair_prices(
    State(state): State<Arc<AppState>>,
    Path(pair_str): Path<String>,
) -> impl IntoResponse {
    // Parse pair string (e.g., "SOL-USDC" or "SOL/USDC")
    let parts: Vec<&str> = pair_str.split(|c| c == '-' || c == '/').collect();
    
    if parts.len() != 2 {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()>::error("Invalid pair format. Use BASE-QUOTE or BASE/QUOTE")),
        ).into_response();
    }

    let pair = TokenPair::new(parts[0], parts[1]);
    
    let detector = state.detector.read().await;
    let prices = detector.get_prices();

    let result: Vec<_> = prices.values()
        .filter(|p| p.pair == pair)
        .cloned()
        .collect();

    Json(ApiResponse::success(result)).into_response()
}

/// Get current configuration
async fn get_config(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(ApiResponse::success(serde_json::json!({
        "min_profit_threshold": state.config.min_profit_threshold,
        "api_port": state.config.api_port,
        "log_level": state.config.log_level,
    })))
}

/// Get bot status including DRY_RUN mode and liveness info
async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let simulated_pnl = *state.simulated_pnl.read().await;
    let simulated_trades = *state.simulated_trades.read().await;
    let heartbeat_count = *state.heartbeat_count.read().await;
    let last_scan_at = *state.last_scan_at.read().await;
    let dex_health = state.dex_health.read().await;
    
    let dex_statuses: Vec<_> = dex_health.values().cloned().collect();
    
    Json(ApiResponse::success(serde_json::json!({
        "dry_run": state.dry_run,
        "bot_running": true,
        "simulated_pnl": simulated_pnl,
        "simulated_trades": simulated_trades,
        "heartbeat_count": heartbeat_count,
        "last_scan_at": last_scan_at.to_rfc3339(),
        "dex_health": dex_statuses,
    })))
}
