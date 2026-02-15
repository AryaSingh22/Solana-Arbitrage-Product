use reqwest::Client;
use serde_json::json;
use tracing::{error, info};

#[derive(Clone)]
pub struct AlertManager {
    telegram_webhook: Option<String>,
    discord_webhook: Option<String>,
    http_client: Client,
}

impl AlertManager {
    pub fn new(telegram_webhook: Option<String>, discord_webhook: Option<String>) -> Self {
        Self {
            telegram_webhook,
            discord_webhook,
            http_client: Client::new(),
        }
    }

    pub fn from_env() -> Self {
        Self {
            telegram_webhook: std::env::var("TELEGRAM_WEBHOOK_URL").ok(),
            discord_webhook: std::env::var("DISCORD_WEBHOOK_URL").ok(),
            http_client: Client::new(),
        }
    }
    
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
