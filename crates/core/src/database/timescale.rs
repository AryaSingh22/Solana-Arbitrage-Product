use crate::risk::TradeOutcome;
use crate::types::ArbitrageOpportunity;
use anyhow::Result;
use chrono::Utc;
use deadpool_postgres::{ManagerConfig, Pool, RecyclingMethod, Runtime};
use rust_decimal::prelude::ToPrimitive;
use tokio_postgres::NoTls;
use uuid::Uuid;

pub struct TimescaleClient {
    pool: Pool,
}

impl TimescaleClient {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pg_config: tokio_postgres::Config = database_url.parse()?;

        let mgr_config = ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        };

        let mgr = deadpool_postgres::Manager::from_config(pg_config, NoTls, mgr_config);

        let pool = Pool::builder(mgr)
            .max_size(20)
            .runtime(Runtime::Tokio1)
            .build()?;

        Ok(Self { pool })
    }

    pub async fn insert_opportunity(&self, opp: &ArbitrageOpportunity) -> Result<Uuid> {
        let client = self.pool.get().await?;
        let opp_id = Uuid::new_v4();
        let now = Utc::now();

        let stmt = client
            .prepare(
                "INSERT INTO opportunities 
            (time, opportunity_id, path, expected_profit_bps, input_amount, dex_route, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .await?;

        client
            .execute(
                &stmt,
                &[
                    &now,
                    &opp_id,
                    &opp.pair.to_string(), // path
                    &opp.net_profit_pct.to_f64().unwrap_or(0.0),
                    &100.0f64, // input_amount placeholder
                    &format!("{} -> {}", opp.buy_dex, opp.sell_dex), // dex_route
                    &"detected", // status
                ],
            )
            .await?;

        Ok(opp_id)
    }

    pub async fn insert_trade(
        &self,
        trade: &TradeOutcome,
        opp_id: Option<Uuid>,
        signature: &str,
    ) -> Result<()> {
        let client = self.pool.get().await?;
        let opp_id = opp_id.unwrap_or_else(Uuid::new_v4);
        let trade_id = Uuid::new_v4();

        let stmt = client
            .prepare(
                "INSERT INTO trades 
            (time, trade_id, opportunity_id, signature, actual_profit, 
             execution_time_ms, slippage_bps, gas_used, priority_fee, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .await?;

        client
            .execute(
                &stmt,
                &[
                    &trade.timestamp,
                    &trade_id,
                    &opp_id,
                    &signature,
                    &trade.profit_loss.to_f64().unwrap_or(0.0),
                    &0i32,   // execution_time_ms
                    &0.0f64, // slippage_bps
                    &0i64,   // gas_used
                    &0i64,   // priority_fee
                    &if trade.was_successful {
                        "success"
                    } else {
                        "failed"
                    },
                ],
            )
            .await?;

        Ok(())
    }
}
