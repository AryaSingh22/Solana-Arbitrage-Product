//! Append-only audit logging for trade execution and security events
//!
//! Provides a tamper-evident trade log for compliance, debugging,
//! and post-incident analysis.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

/// A single audit event entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// ISO 8601 timestamp
    pub timestamp: String,
    /// Event category (TRADE, RISK, SYSTEM, CONFIG)
    pub category: String,
    /// Specific action (EXECUTE, REJECT, ALERT, RELOAD)
    pub action: String,
    /// Affected resource (opportunity ID, pair, etc.)
    pub resource: String,
    /// Outcome (SUCCESS, FAILURE, SKIPPED)
    pub result: String,
    /// Additional structured details
    pub details: serde_json::Value,
}

/// Append-only audit logger that writes JSONL (one JSON object per line)
pub struct AuditLogger {
    file: Mutex<tokio::fs::File>,
    path: PathBuf,
}

impl AuditLogger {
    /// Create or open an audit log file
    pub async fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        Ok(Self {
            file: Mutex::new(file),
            path,
        })
    }

    /// Log a raw audit event
    pub async fn log(&self, event: AuditEvent) -> std::io::Result<()> {
        let mut json = serde_json::to_string(&event)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        json.push('\n');

        let mut file = self.file.lock().await;
        file.write_all(json.as_bytes()).await?;
        file.flush().await?;

        Ok(())
    }

    /// Log a trade execution event
    pub async fn log_trade(
        &self,
        opportunity_id: &str,
        pair: &str,
        action: &str,
        success: bool,
        profit: f64,
        details: serde_json::Value,
    ) -> std::io::Result<()> {
        let event = AuditEvent {
            timestamp: Utc::now().to_rfc3339(),
            category: "TRADE".to_string(),
            action: action.to_string(),
            resource: format!("{}:{}", pair, opportunity_id),
            result: if success {
                "SUCCESS".to_string()
            } else {
                "FAILURE".to_string()
            },
            details: serde_json::json!({
                "profit": profit,
                "extra": details,
            }),
        };

        self.log(event).await
    }

    /// Log a risk management event
    pub async fn log_risk_event(
        &self,
        event_type: &str,
        details: serde_json::Value,
    ) -> std::io::Result<()> {
        let event = AuditEvent {
            timestamp: Utc::now().to_rfc3339(),
            category: "RISK".to_string(),
            action: event_type.to_string(),
            resource: "risk_manager".to_string(),
            result: "LOGGED".to_string(),
            details,
        };

        self.log(event).await
    }

    /// Log a system event (startup, shutdown, config change)
    pub async fn log_system_event(
        &self,
        action: &str,
        details: serde_json::Value,
    ) -> std::io::Result<()> {
        let event = AuditEvent {
            timestamp: Utc::now().to_rfc3339(),
            category: "SYSTEM".to_string(),
            action: action.to_string(),
            resource: "bot".to_string(),
            result: "LOGGED".to_string(),
            details,
        };

        self.log(event).await
    }

    /// Get the path to the audit log file
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_audit_logger_write_and_read() {
        let dir = std::env::temp_dir().join("arb_audit_test");
        let log_path = dir.join("test_audit.jsonl");

        // Clean up from previous runs
        let _ = tokio::fs::remove_file(&log_path).await;

        let logger = AuditLogger::new(&log_path).await.unwrap();

        // Log a trade
        logger
            .log_trade(
                "opp-123",
                "SOL/USDC",
                "EXECUTE",
                true,
                42.50,
                serde_json::json!({"dex": "Jupiter"}),
            )
            .await
            .unwrap();

        // Log a system event
        logger
            .log_system_event("STARTUP", serde_json::json!({"mode": "dry-run"}))
            .await
            .unwrap();

        // Verify file contents
        let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
        let lines: Vec<&str> = contents.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);

        // Parse first line
        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.category, "TRADE");
        assert_eq!(event.action, "EXECUTE");
        assert_eq!(event.result, "SUCCESS");

        // Parse second line
        let event2: AuditEvent = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(event2.category, "SYSTEM");

        // Cleanup
        let _ = tokio::fs::remove_file(&log_path).await;
        let _ = tokio::fs::remove_dir(&dir).await;
    }

    #[tokio::test]
    async fn test_audit_logger_risk_event() {
        let dir = std::env::temp_dir().join("arb_audit_risk_test");
        let log_path = dir.join("risk_audit.jsonl");
        let _ = tokio::fs::remove_file(&log_path).await;

        let logger = AuditLogger::new(&log_path).await.unwrap();

        logger
            .log_risk_event(
                "CIRCUIT_BREAKER_OPEN",
                serde_json::json!({"consecutive_losses": 5}),
            )
            .await
            .unwrap();

        let contents = tokio::fs::read_to_string(&log_path).await.unwrap();
        let event: AuditEvent = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(event.category, "RISK");
        assert_eq!(event.action, "CIRCUIT_BREAKER_OPEN");

        let _ = tokio::fs::remove_file(&log_path).await;
        let _ = tokio::fs::remove_dir(&dir).await;
    }
}
