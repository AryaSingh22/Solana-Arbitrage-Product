use crate::types::{ArbitrageOpportunity};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use chrono::Utc;

#[derive(Debug, Serialize, Deserialize)]
pub struct TradeRecord {
    pub timestamp: String,
    pub session_id: String,
    pub trade_type: String, // "SIMULATION" or "REAL"
    pub pair: String,
    pub buy_dex: String,
    pub sell_dex: String,
    pub size_usd: String,
    pub profit_usd: String,
    pub profit_pct: String,
    pub tx_signature: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

pub struct HistoryRecorder {
    file_path: String,
    session_id: String,
}

impl HistoryRecorder {
    pub fn new(file_path: &str, session_id: &str) -> Self {
        // Ensure directory exists
        if let Some(parent) = Path::new(file_path).parent() {
            let _ = fs::create_dir_all(parent);
        }

        Self {
            file_path: file_path.to_string(),
            session_id: session_id.to_string(),
        }
    }

    pub fn record_trade(
        &self,
        opp: &ArbitrageOpportunity,
        size_usd: Decimal,
        profit_usd: Decimal,
        success: bool,
        tx_sig: Option<String>,
        error: Option<String>,
        is_dry_run: bool,
    ) {
        let record = TradeRecord {
            timestamp: Utc::now().to_rfc3339(),
            session_id: self.session_id.clone(),
            trade_type: if is_dry_run { "SIMULATION".to_string() } else { "REAL".to_string() },
            pair: opp.pair.symbol(),
            buy_dex: opp.buy_dex.display_name().to_string(),
            sell_dex: opp.sell_dex.display_name().to_string(),
            size_usd: size_usd.round_dp(2).to_string(),
            profit_usd: profit_usd.round_dp(4).to_string(),
            profit_pct: opp.net_profit_pct.round_dp(2).to_string(),
            tx_signature: tx_sig,
            success,
            error,
        };

        match serde_json::to_string(&record) {
            Ok(json) => {
                 let open_result = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.file_path);
                
                match open_result {
                    Ok(mut file) => {
                         if let Err(e) = writeln!(file, "{}", json) {
                            eprintln!("Failed to write to history file: {}", e);
                        }
                    },
                    Err(e) => eprintln!("Failed to open history file {}: {}", self.file_path, e),
                }
            },
            Err(e) => eprintln!("Failed to serialize trade record: {}", e),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub total_trades: usize,
    pub successful_trades: usize,
    pub success_rate: f64,
    pub total_profit_usd: String,
    pub avg_profit_usd: String,
    pub best_pair: Option<String>,
    pub best_route: Option<String>,
    pub worst_route: Option<String>,
    pub total_volume_usd: String,
}

pub struct HistoryAnalyzer;

impl HistoryAnalyzer {
    pub fn analyze(file_path: &str) -> Result<AnalysisReport, std::io::Error> {
        let path = Path::new(file_path);
        if !path.exists() {
             return Ok(AnalysisReport {
                total_trades: 0,
                successful_trades: 0,
                success_rate: 0.0,
                total_profit_usd: "0.00".to_string(),
                avg_profit_usd: "0.00".to_string(),
                best_pair: None, 
                best_route: None,
                worst_route: None,
                total_volume_usd: "0.00".to_string(),
             });
        }

        let file = fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let mut trades: Vec<TradeRecord> = Vec::new();

        use std::io::BufRead;
        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(record) = serde_json::from_str::<TradeRecord>(&line) {
                    trades.push(record);
                }
            }
        }

        let total_trades = trades.len();
        if total_trades == 0 {
             return Ok(AnalysisReport {
                total_trades: 0,
                successful_trades: 0,
                success_rate: 0.0,
                total_profit_usd: "0.00".to_string(),
                avg_profit_usd: "0.00".to_string(),
                best_pair: None, 
                best_route: None,
                worst_route: None,
                total_volume_usd: "0.00".to_string(),
             });
        }

        let successful_trades = trades.iter().filter(|t| t.success).count();
        let success_rate = if total_trades > 0 {
            (successful_trades as f64 / total_trades as f64) * 100.0
        } else {
            0.0
        };

        let mut total_profit = Decimal::ZERO;
        let mut total_volume = Decimal::ZERO;
        let mut pair_profit: std::collections::HashMap<String, Decimal> = std::collections::HashMap::new();
        let mut route_profit: std::collections::HashMap<String, Decimal> = std::collections::HashMap::new();

        use std::str::FromStr;
        for trade in &trades {
            if let Ok(profit) = Decimal::from_str(&trade.profit_usd) {
                total_profit += profit;
                *pair_profit.entry(trade.pair.clone()).or_default() += profit;
                
                let route = format!("{}->{}", trade.buy_dex, trade.sell_dex);
                *route_profit.entry(route).or_default() += profit;
            }
            if let Ok(size) = Decimal::from_str(&trade.size_usd) {
                total_volume += size;
            }
        }

        let avg_profit = if total_trades > 0 {
            total_profit / Decimal::from(total_trades)
        } else {
            Decimal::ZERO
        };

        let best_pair = pair_profit.iter()
            .max_by(|a, b| a.1.cmp(b.1))
            .map(|(k, _)| k.clone());

        let best_route = route_profit.iter()
            .max_by(|a, b| a.1.cmp(b.1))
            .map(|(k, _)| k.clone());

        let worst_route = route_profit.iter()
            .min_by(|a, b| a.1.cmp(b.1))
            .map(|(k, _)| k.clone());

        Ok(AnalysisReport {
            total_trades,
            successful_trades,
            success_rate,
            total_profit_usd: total_profit.round_dp(2).to_string(),
            avg_profit_usd: avg_profit.round_dp(4).to_string(),
            best_pair,
            best_route,
            worst_route,
            total_volume_usd: total_volume.round_dp(2).to_string(),
        })
    }
}

