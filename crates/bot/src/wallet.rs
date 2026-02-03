//! Wallet Management (Simulation)
//!
//! Handles wallet configuration for simulated trading environment.
//! Uses raw strings instead of SDK Keypairs to avoid build conflicts on Windows.

use anyhow::Result;
use std::env;
use tracing::{info, warn};

/// Wallet wrapper for simulation
pub struct Wallet {
    pub pubkey: String,
}

impl Wallet {
    /// Load wallet public key from environment or generate dummy
    pub fn new() -> Result<Self> {
        let pk_str = env::var("PRIVATE_KEY").ok();
        
        let pubkey = if let Some(pk) = pk_str {
            if pk.is_empty() {
                "SimulatedWallet1111111111111111111111111111111".to_string()
            } else {
                // In a real scenario we'd derive pubkey from private key
                // For now, just use a dummy or the string itself if it looks like a pubkey
                if pk.len() > 40 {
                   // Assume it's a private key, derive a fake pubkey
                   "DerivedPubkeyFromEnv1111111111111111111111".to_string()
                } else {
                   pk
                }
            }
        } else {
            warn!("PRIVATE_KEY not set. Using simulated wallet.");
            "SimulatedWallet1111111111111111111111111111111".to_string()
        };

        info!("Wallet loaded (simulated): {}", pubkey);
        Ok(Self { pubkey })
    }

    pub fn pubkey(&self) -> String {
        self.pubkey.clone()
    }
}
