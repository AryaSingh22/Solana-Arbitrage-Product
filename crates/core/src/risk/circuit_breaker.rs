use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use crate::events::{EventBus, TradingEvent};

#[derive(Debug, Clone)]
pub enum CircuitState {
    Closed,   // Normal operation
    HalfOpen, // Testing if system recovered
    Open,     // Trading disabled
}

pub struct CircuitBreaker {
    state: Arc<RwLock<CircuitState>>,
    failure_threshold: usize,
    success_threshold: usize,
    timeout: Duration,

    // Counters
    consecutive_failures: Arc<RwLock<usize>>,
    consecutive_successes: Arc<RwLock<usize>>,
    last_failure_time: Arc<RwLock<Option<Instant>>>,
    event_bus: Arc<RwLock<Option<Arc<EventBus>>>>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: usize, success_threshold: usize, timeout_secs: u64) -> Self {
        Self {
            state: Arc::new(RwLock::new(CircuitState::Closed)),
            failure_threshold,
            success_threshold,
            timeout: Duration::from_secs(timeout_secs),
            consecutive_failures: Arc::new(RwLock::new(0)),
            consecutive_successes: Arc::new(RwLock::new(0)),
            last_failure_time: Arc::new(RwLock::new(None)),
            event_bus: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn set_event_bus(&self, bus: Arc<EventBus>) {
        *self.event_bus.write().await = Some(bus);
    }

    pub async fn record_success(&self) {
        let mut successes = self.consecutive_successes.write().await;
        *successes += 1;

        let mut failures = self.consecutive_failures.write().await;
        *failures = 0;

        // Transition from HalfOpen to Closed if enough successes
        if *successes >= self.success_threshold {
            let mut state = self.state.write().await;
            if matches!(*state, CircuitState::HalfOpen) {
                *state = CircuitState::Closed;
                tracing::info!("Circuit breaker CLOSED - system recovered");
                
                if let Some(bus) = self.event_bus.read().await.as_ref() {
                    bus.publish(TradingEvent::CircuitBreakerStateChanged {
                        old_state: "HalfOpen".to_string(),
                        new_state: "Closed".to_string(),
                    });
                }
            }
        }
    }

    pub async fn record_failure(&self) {
        let mut failures = self.consecutive_failures.write().await;
        *failures += 1;

        let mut successes = self.consecutive_successes.write().await;
        *successes = 0;

        *self.last_failure_time.write().await = Some(Instant::now());

        // Open circuit if threshold exceeded
        if *failures >= self.failure_threshold {
            let mut state = self.state.write().await;
            *state = CircuitState::Open;
            tracing::error!(
                "Circuit breaker OPEN - trading halted"
            );

            if let Some(bus) = self.event_bus.read().await.as_ref() {
                bus.publish(TradingEvent::CircuitBreakerStateChanged {
                    old_state: "Closed".to_string(), // Could refer to previous state, but record_failure usually from Closed/HalfOpen
                    new_state: "Open".to_string(),
                });
            }
        }
    }

    pub async fn can_execute(&self) -> bool {
        let mut state = self.state.write().await;

        match *state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout elapsed
                if let Some(last_failure) = *self.last_failure_time.read().await {
                    if last_failure.elapsed() >= self.timeout {
                        *state = CircuitState::HalfOpen;
                        tracing::warn!("Circuit breaker HALF-OPEN - testing recovery");
                        
                        if let Some(bus) = self.event_bus.read().await.as_ref() {
                            bus.publish(TradingEvent::CircuitBreakerStateChanged {
                                old_state: "Open".to_string(),
                                new_state: "HalfOpen".to_string(),
                            });
                        }
                        
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true, // Allow test trades
        }
    }
}
