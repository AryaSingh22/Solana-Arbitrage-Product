//! Event-driven architecture for decoupled component communication
//!
//! Provides a publish-subscribe event bus for trading events, allowing
//! components to communicate without direct dependencies.

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Trading system events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradingEvent {
    // ── Price Events ────────────────────────────────────────────────
    /// Price update received from a DEX
    PriceUpdate {
        pair: String,
        price: f64,
        source: String,
        timestamp: i64,
    },

    // ── Opportunity Events ──────────────────────────────────────────
    /// Arbitrage opportunity detected by a strategy
    OpportunityDetected {
        id: String,
        strategy: String,
        expected_profit_bps: f64,
    },

    /// Opportunity expired or became invalid
    OpportunityExpired { id: String, reason: String },

    // ── Trade Events ────────────────────────────────────────────────
    /// Trade execution completed
    TradeExecuted {
        id: String,
        pair: String,
        success: bool,
        profit: f64,
        execution_time_ms: u64,
    },

    /// Trade was rejected by risk management
    TradeRejected { id: String, reason: String },

    // ── Risk Events ─────────────────────────────────────────────────
    /// Circuit breaker state changed
    CircuitBreakerStateChanged {
        old_state: String,
        new_state: String,
    },

    /// A risk limit was breached
    RiskLimitBreached {
        limit_type: String,
        current: f64,
        max: f64,
    },

    // ── System Events ───────────────────────────────────────────────
    /// System started successfully
    SystemStarted { mode: String },

    /// System is stopping
    SystemStopping { reason: String },

    /// Emergency stop triggered
    EmergencyStop { reason: String },

    /// Health check status
    HealthCheck {
        uptime_secs: u64,
        total_trades: u64,
        success_rate: f64,
    },
}

/// Broadcast-based event bus for zero-copy event distribution
pub struct EventBus {
    tx: broadcast::Sender<TradingEvent>,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publish an event to all subscribers
    ///
    /// Returns the number of active subscribers that received the event.
    /// If no subscribers are listening, the event is silently dropped.
    pub fn publish(&self, event: TradingEvent) -> usize {
        self.tx.send(event).unwrap_or(0)
    }

    /// Create a new subscription to receive events
    pub fn subscribe(&self) -> broadcast::Receiver<TradingEvent> {
        self.tx.subscribe()
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_bus_publish_subscribe() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        let event = TradingEvent::SystemStarted {
            mode: "dry-run".to_string(),
        };
        let count = bus.publish(event);
        assert_eq!(count, 1);

        let received = rx.recv().await.unwrap();
        match received {
            TradingEvent::SystemStarted { mode } => assert_eq!(mode, "dry-run"),
            _ => panic!("Wrong event type"),
        }
    }

    #[tokio::test]
    async fn test_event_bus_no_subscribers() {
        let bus = EventBus::new(16);
        // No subscribers — publish should return 0 and not panic
        let count = bus.publish(TradingEvent::EmergencyStop {
            reason: "test".into(),
        });
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_event_bus_multiple_subscribers() {
        let bus = EventBus::new(16);
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        bus.publish(TradingEvent::HealthCheck {
            uptime_secs: 3600,
            total_trades: 42,
            success_rate: 0.85,
        });

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();

        // Both subscribers should receive the same event
        match (&e1, &e2) {
            (
                TradingEvent::HealthCheck {
                    total_trades: t1, ..
                },
                TradingEvent::HealthCheck {
                    total_trades: t2, ..
                },
            ) => {
                assert_eq!(*t1, 42);
                assert_eq!(*t2, 42);
            }
            _ => panic!("Wrong event types"),
        }
    }

    #[test]
    fn test_subscriber_count() {
        let bus = EventBus::new(16);
        assert_eq!(bus.subscriber_count(), 0);

        let _rx1 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);

        let _rx2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);

        drop(_rx1);
        assert_eq!(bus.subscriber_count(), 1);
    }
}
