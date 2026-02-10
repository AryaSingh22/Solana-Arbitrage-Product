use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::IntoResponse,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, warn, error};

use solana_arb_core::{ArbitrageOpportunity, PriceData};
use crate::AppState;

/// WebSocket message sent to clients
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "payload")]
pub enum WebSocketMessage {
    /// Initial connection status
    Status(String),
    /// Real-time price update
    PriceUpdate(Vec<PriceData>),
    /// New arbitrage opportunity detected
    NewOpportunity(ArbitrageOpportunity),
    /// Heartbeat / Ping
    Heartbeat(u64),
}

/// WebSocket handler function
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle a single WebSocket connection
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to broadcast channel
    let mut rx = state.tx.subscribe();

    // Spawn task to forward broadcast messages to WebSocket client
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // Serialize message to JSON string
            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming messages (mostly for PING/PONG or commands if needed)
    // For now, we just keep the connection alive
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Close(_) => break,
                Message::Ping(_) => {}, // Automatically handled by axum/tungstenite mostly
                _ => {},
            }
        }
    });

    // Wait for either task to finish (e.g. connection closed)
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };
    
    info!("WebSocket client disconnected");
}
