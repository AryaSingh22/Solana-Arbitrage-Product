//! Alerts Module
//!
//! Manages external notifications via Telegram, Discord, and other channels.

use reqwest::Client;
use serde_json::json;
use tracing::{error, info};

/// Manages system alerts via multiple channels (Telegram, Discord).
///
/// Provides a unified interface for sending critical alerts and informational messages
/// to configured webhooks.
#[derive(Clone)]
#[allow(dead_code)]
pub struct AlertManager {
    telegram_webhook: Option<String>,
    discord_webhook: Option<String>,
    http_client: Client,
}

#[allow(dead_code)]
impl AlertManager {
    /// Creates a new AlertManager with specified command-line/config webhooks.
    pub fn new(telegram_webhook: Option<String>, discord_webhook: Option<String>) -> Self {
        Self {
            telegram_webhook,
            discord_webhook,
            http_client: Client::new(),
        }
    }

    /// Creates an AlertManager from environment variables.
    pub fn from_env() -> Self {
        Self {
            telegram_webhook: std::env::var("TELEGRAM_WEBHOOK_URL").ok(),
            discord_webhook: std::env::var("DISCORD_WEBHOOK_URL").ok(),
            http_client: Client::new(),
        }
    }
    
    /// Sends a critical alert (prefixed with "🚨 CRITICAL") to all configured channels.
    pub async fn send_critical(&self, message: &str) {
        let formatted = format!("🚨 CRITICAL: {}", message);
        error!("{}", formatted);
        
        // Send to Telegram
        if let Some(url) = &self.telegram_webhook {
            let _ = self.http_client
                .post(url)
                .json(&json!({
                    "text": formatted,
                    "parse_mode": "HTML"
                }))
                .send()
                .await
                .map_err(|e| error!("Failed to send Telegram alert: {}", e));
        }
        
        // Send to Discord
        if let Some(url) = &self.discord_webhook {
            let _ = self.http_client
                .post(url)
                .json(&json!({
                    "content": format!("@everyone {}", formatted),
                    "username": "ArbEngine Alert"
                }))
                .send()
                .await
                .map_err(|e| error!("Failed to send Discord alert: {}", e));
        }
    }
    
    /// Sends an informational message to all configured channels.
    pub async fn send_info(&self, message: &str) {
        let formatted = format!("ℹ️ {}", message);
        info!("{}", formatted);
        
        if let Some(url) = &self.telegram_webhook {
            let _ = self.http_client
                .post(url)
                .json(&json!({"text": formatted}))
                .send()
                .await
                .map_err(|e| error!("Failed to send Telegram info: {}", e));
        }
    }
    
    pub async fn send_profit_alert(&self, profit: f64, details: &str) {
        let formatted = format!("💰 Profit: ${:.2}\n{}", profit, details);
        self.send_info(&formatted).await;
    }
}
