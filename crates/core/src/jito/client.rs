use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, warn};

/// Jito block engine client for bundle submission
#[derive(Debug, Clone)]
pub struct JitoClient {
    client: Client,
    block_engine_url: String,
    tip_lamports: u64,
}

#[derive(Debug, Serialize)]
struct BundleRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    params: Vec<Vec<String>>, // Array of base64-encoded transactions
}

#[derive(Debug, Deserialize)]
struct BundleResponse {
    result: Option<String>, // Bundle ID
    error: Option<BundleError>,
}

#[derive(Debug, Deserialize)]
struct BundleError {
    message: String,
}

impl JitoClient {
    pub fn new(block_engine_url: &str, tip_lamports: u64) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
            block_engine_url: block_engine_url.to_string(),
            tip_lamports,
        }
    }

    /// Submit a transaction as a Jito bundle
    pub async fn send_bundle(&self, signed_tx_base64: &str) -> Result<String> {
        info!(
            "📦 Submitting Jito bundle (tip: {} lamports) to {}",
            self.tip_lamports, self.block_engine_url
        );

        let bundle_req = BundleRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "sendBundle".to_string(),
            params: vec![vec![signed_tx_base64.to_string()]],
        };

        let url = format!("{}/api/v1/bundles", self.block_engine_url);
        debug!("Jito bundle endpoint: {}", url);

        let response = self.client.post(&url).json(&bundle_req).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!(
                "Jito bundle submission failed ({}): {}",
                status,
                error_text
            ));
        }

        let bundle_resp: BundleResponse = response.json().await?;

        if let Some(error) = bundle_resp.error {
            warn!("❌ Jito bundle error: {}", error.message);
            return Err(anyhow!("Jito bundle error: {}", error.message));
        }

        match bundle_resp.result {
            Some(bundle_id) => {
                info!("✅ Jito bundle accepted: {}", bundle_id);
                Ok(bundle_id)
            }
            None => Err(anyhow!("Jito bundle returned no result and no error")),
        }
    }

    /// Check if the Jito block engine is reachable
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/api/v1/bundles", self.block_engine_url);
        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success() || resp.status().as_u16() == 405),
            Err(e) => {
                warn!("Jito health check failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Get random tip account (Placeholder - normally fetched from Jito API)
    pub async fn get_tip_account(&self) -> Result<String> {
        // List of common Jito tip accounts
        let tip_accounts = ["96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
            "HFqU5x63VTqvQss8hp11i4wVV8bD44Puy60pxTKAW4PH",
            "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
            "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
            "DfXygSm4jCyNCyb3qzK6966vGgy5tQSZHarris11tc66",
            "ADuUkR4ykG49cvq5RTu3TRLpVIUwDiIHjYyC1E1AtDyV",
            "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
            "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnIzKZ6jJ"];

        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        // Safety: tip_accounts is a non-empty compile-time constant
        Ok(tip_accounts
            .choose(&mut rng)
            .expect("tip_accounts is non-empty")
            .to_string())
    }

    /// Get the tip amount in lamports
    pub fn tip_lamports(&self) -> u64 {
        self.tip_lamports
    }
}
