use crate::types::{DexType, PriceData, TokenPair};
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde_json::json;
use std::str::FromStr;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub struct WebSocketManager {
    price_tx: mpsc::Sender<PriceData>,
    reconnect_delay_ms: u64,
    max_reconnect_attempts: u32,
}

impl WebSocketManager {
    pub fn new(price_tx: mpsc::Sender<PriceData>) -> Self {
        Self {
            price_tx,
            reconnect_delay_ms: 1000,
            max_reconnect_attempts: 10,
        }
    }

    pub fn with_reconnect(mut self, delay_ms: u64, max_attempts: u32) -> Self {
        self.reconnect_delay_ms = delay_ms;
        self.max_reconnect_attempts = max_attempts;
        self
    }

    /// Start a WebSocket subscription with automatic reconnection on disconnect.
    pub async fn start_with_reconnection(&self, dex: DexType, pair: TokenPair) {
        let mut attempt = 0u32;
        let mut delay = self.reconnect_delay_ms;

        loop {
            tracing::info!(
                "🔌 WS connection attempt {}/{} for {} on {:?}",
                attempt + 1,
                self.max_reconnect_attempts,
                pair,
                dex
            );

            self.subscribe_to_pair(dex, pair.clone()).await;

            attempt += 1;
            if attempt >= self.max_reconnect_attempts {
                tracing::error!(
                    "❌ Exceeded max reconnect attempts ({}) for {} on {:?}",
                    self.max_reconnect_attempts,
                    pair,
                    dex
                );
                break;
            }

            tracing::warn!(
                "🔄 Reconnecting in {}ms (attempt {})",
                delay,
                attempt
            );
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;

            // Exponential backoff: 1s → 2s → 4s → 8s → capped at 30s
            delay = (delay * 2).min(30_000);
        }
    }

    pub async fn subscribe_to_pair(&self, dex: DexType, pair: TokenPair) {
        let url = match dex {
            DexType::Jupiter => "wss://quote-api.jup.ag/v6/quote-ws".to_string(),
            DexType::Raydium => {
                format!("wss://api.raydium.io/v2/main/price/{}", pair.symbol())
            }
            _ => return,
        };

        let result = connect_async(url.as_str()).await;

        match result {
            Ok((ws_stream, _response)) => {
                tracing::info!("🔌 Connected to WS for {} on {:?}", pair, dex);
                let (mut write, mut read): (
                    futures_util::stream::SplitSink<_, Message>,
                    futures_util::stream::SplitStream<_>,
                ) = ws_stream.split();

                // Send subscribe message
                let subscribe_msg = json!({
                    "method": "subscribe",
                    "params": [pair.symbol()]
                });
                if let Err(e) = write.send(Message::Text(subscribe_msg.to_string())).await {
                    tracing::error!("Failed to send subscribe message: {}", e);
                    return;
                }

                let price_tx = self.price_tx.clone();
                let pair_clone = pair.clone();

                // Process messages until disconnect
                while let Some(msg_result) = read.next().await {
                    match msg_result {
                        Ok(Message::Text(text)) => {
                            match Self::parse_price_message(&text, dex, &pair_clone) {
                                Ok(Some(price_data)) => {
                                    if let Err(e) = price_tx.send(price_data).await {
                                        tracing::error!(
                                            "Failed to send price update through channel: {}",
                                            e
                                        );
                                        break;
                                    }
                                }
                                Ok(None) => {
                                    // Non-price message (heartbeat, ack, etc.) – ignore
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to parse WS message for {} on {:?}: {}",
                                        pair_clone,
                                        dex,
                                        e
                                    );
                                }
                            }
                        }
                        Ok(Message::Ping(payload)) => {
                            tracing::trace!("Received ping, sending pong");
                            if let Err(e) = write.send(Message::Pong(payload)).await {
                                tracing::warn!("Failed to send pong: {}", e);
                                break;
                            }
                        }
                        Ok(Message::Close(frame)) => {
                            tracing::info!(
                                "WS closed by server for {} on {:?}: {:?}",
                                pair_clone,
                                dex,
                                frame
                            );
                            break;
                        }
                        Ok(_) => {
                            // Binary, Pong, Frame – ignore
                        }
                        Err(e) => {
                            tracing::error!(
                                "WS read error for {} on {:?}: {}",
                                pair_clone,
                                dex,
                                e
                            );
                            break;
                        }
                    }
                }

                tracing::warn!("WS disconnected for {} on {:?}", pair_clone, dex);
            }
            Err(e) => {
                tracing::warn!("Failed to connect to WS for {} on {:?}: {}", pair, dex, e);
            }
        }
    }

    /// Parse a WebSocket text message into a `PriceData`, returning `Ok(None)` for
    /// non-price messages (heartbeats, subscription acks, etc.).
    fn parse_price_message(
        text: &str,
        dex: DexType,
        pair: &TokenPair,
    ) -> Result<Option<PriceData>, String> {
        let json: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;

        // Check for heartbeat / subscription ack messages
        if json.get("type").and_then(|t| t.as_str()) == Some("heartbeat")
            || json.get("type").and_then(|t| t.as_str()) == Some("subscribed")
        {
            return Ok(None);
        }

        // Check for error messages
        if json.get("type").and_then(|t| t.as_str()) == Some("error") {
            let msg = json
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(format!("WS error from server: {}", msg));
        }

        // Try different field-name patterns used by various DEXs:
        //   Jupiter: { "bid": "...", "ask": "..." } or { "inAmount": ..., "outAmount": ... }
        //   Raydium: { "price": "..." } (single mid-price)
        //   Generic: { "data": { "bid": ..., "ask": ... } }

        // Pattern 1: explicit bid/ask at top level or inside "data"
        let data_obj = json.get("data").unwrap_or(&json);

        if let (Some(bid_val), Some(ask_val)) = (data_obj.get("bid"), data_obj.get("ask")) {
            let bid = parse_decimal_value(bid_val)
                .ok_or_else(|| "Cannot parse 'bid' field".to_string())?;
            let ask = parse_decimal_value(ask_val)
                .ok_or_else(|| "Cannot parse 'ask' field".to_string())?;
            return Ok(Some(PriceData::new(dex, pair.clone(), bid, ask)));
        }

        // Pattern 2: single "price" field → use as both bid and ask (spread = 0)
        if let Some(price_val) = data_obj.get("price") {
            let price = parse_decimal_value(price_val)
                .ok_or_else(|| "Cannot parse 'price' field".to_string())?;
            return Ok(Some(PriceData::new(dex, pair.clone(), price, price)));
        }

        // Pattern 3: Jupiter quote-style with inAmount/outAmount
        if let (Some(in_val), Some(out_val)) =
            (data_obj.get("inAmount"), data_obj.get("outAmount"))
        {
            let in_amount = parse_decimal_value(in_val)
                .ok_or_else(|| "Cannot parse 'inAmount'".to_string())?;
            let out_amount = parse_decimal_value(out_val)
                .ok_or_else(|| "Cannot parse 'outAmount'".to_string())?;

            if in_amount.is_zero() {
                return Err("inAmount is zero".to_string());
            }
            let price = out_amount / in_amount;
            return Ok(Some(PriceData::new(dex, pair.clone(), price, price)));
        }

        // Unrecognized format – not necessarily an error, could be metadata
        Ok(None)
    }
}

/// Parse a JSON value that might be a number or a string containing a number.
fn parse_decimal_value(val: &serde_json::Value) -> Option<Decimal> {
    match val {
        serde_json::Value::Number(n) => {
            // Try i64 first, then f64
            n.as_i64()
                .map(Decimal::from)
                .or_else(|| n.as_f64().and_then(Decimal::from_f64_retain))
        }
        serde_json::Value::String(s) => Decimal::from_str(s).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bid_ask_message() {
        let msg = r#"{"bid": "100.5", "ask": "101.0"}"#;
        let pair = TokenPair::new("SOL", "USDC");
        let result = WebSocketManager::parse_price_message(msg, DexType::Jupiter, &pair);
        assert!(result.is_ok());
        let price = result.unwrap().expect("Should produce PriceData");
        assert_eq!(price.bid, Decimal::from_str("100.5").unwrap());
        assert_eq!(price.ask, Decimal::from_str("101.0").unwrap());
        assert_eq!(price.dex, DexType::Jupiter);
    }

    #[test]
    fn test_parse_single_price_message() {
        let msg = r#"{"price": 42.5}"#;
        let pair = TokenPair::new("RAY", "USDC");
        let result = WebSocketManager::parse_price_message(msg, DexType::Raydium, &pair);
        assert!(result.is_ok());
        let price = result.unwrap().expect("Should produce PriceData");
        assert_eq!(price.bid, price.ask); // single price → same bid and ask
    }

    #[test]
    fn test_parse_nested_data_message() {
        let msg = r#"{"type": "update", "data": {"bid": 99, "ask": 101}}"#;
        let pair = TokenPair::new("SOL", "USDC");
        let result = WebSocketManager::parse_price_message(msg, DexType::Jupiter, &pair);
        assert!(result.is_ok());
        let price = result.unwrap().expect("Should produce PriceData");
        assert_eq!(price.bid, Decimal::from(99));
        assert_eq!(price.ask, Decimal::from(101));
    }

    #[test]
    fn test_parse_heartbeat_ignored() {
        let msg = r#"{"type": "heartbeat"}"#;
        let pair = TokenPair::new("SOL", "USDC");
        let result = WebSocketManager::parse_price_message(msg, DexType::Jupiter, &pair);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none(), "Heartbeat should produce None");
    }

    #[test]
    fn test_parse_subscription_ack_ignored() {
        let msg = r#"{"type": "subscribed", "channel": "prices"}"#;
        let pair = TokenPair::new("SOL", "USDC");
        let result = WebSocketManager::parse_price_message(msg, DexType::Jupiter, &pair);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_parse_error_message() {
        let msg = r#"{"type": "error", "message": "rate limited"}"#;
        let pair = TokenPair::new("SOL", "USDC");
        let result = WebSocketManager::parse_price_message(msg, DexType::Jupiter, &pair);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("rate limited"));
    }

    #[test]
    fn test_parse_invalid_json() {
        let msg = "not json at all";
        let pair = TokenPair::new("SOL", "USDC");
        let result = WebSocketManager::parse_price_message(msg, DexType::Jupiter, &pair);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unrecognized_format_returns_none() {
        let msg = r#"{"status": "ok", "info": "connected"}"#;
        let pair = TokenPair::new("SOL", "USDC");
        let result = WebSocketManager::parse_price_message(msg, DexType::Jupiter, &pair);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_parse_jupiter_quote_style() {
        let msg = r#"{"inAmount": "1000000", "outAmount": "42500000"}"#;
        let pair = TokenPair::new("USDC", "SOL");
        let result = WebSocketManager::parse_price_message(msg, DexType::Jupiter, &pair);
        assert!(result.is_ok());
        let price = result.unwrap().expect("Should produce PriceData");
        // 42500000 / 1000000 = 42.5
        assert_eq!(price.mid_price, Decimal::from_str("42.5").unwrap());
    }

    #[test]
    fn test_parse_decimal_value_number() {
        let val = serde_json::json!(42.5);
        assert!(parse_decimal_value(&val).is_some());
    }

    #[test]
    fn test_parse_decimal_value_string() {
        let val = serde_json::json!("100.25");
        assert_eq!(
            parse_decimal_value(&val),
            Some(Decimal::from_str("100.25").unwrap())
        );
    }

    #[test]
    fn test_parse_decimal_value_bool_returns_none() {
        let val = serde_json::json!(true);
        assert!(parse_decimal_value(&val).is_none());
    }
}
