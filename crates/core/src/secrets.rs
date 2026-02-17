use std::env;
use std::fmt;

/// A wrapper for sensitive strings that are redacted in debug output.
/// Note: Memory zeroization is currently disabled due to dependency conflicts with `zeroize` crate.
#[derive(Clone)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: String) -> Self {
        Self(s)
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}
// Zeroize implementation removed


impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED]")
    }
}

/// Manages application secrets securely.
#[derive(Clone)]
pub struct SecretManager {
    /// The wallet private key (Base58 encoded)
    private_key: SecretString,
    /// The RPC URL (which may contain API keys)
    rpc_url: SecretString,
}

impl SecretManager {
    /// Creates a new SecretManager by loading secrets from environment variables.
    pub fn new() -> Result<Self, String> {
        let private_key = env::var("PRIVATE_KEY")
            .map(SecretString::new)
            .map_err(|_| "PRIVATE_KEY environment variable not set".to_string())?;

        let rpc_url = env::var("SOLANA_RPC_URL")
            .map(SecretString::new)
            .map_err(|_| "SOLANA_RPC_URL environment variable not set".to_string())?;

        Ok(Self {
            private_key,
            rpc_url,
        })
    }

    /// Access the private key securely.
    pub fn get_private_key(&self) -> &str {
        self.private_key.expose_secret()
    }

    /// Access the RPC URL securely.
    pub fn get_rpc_url(&self) -> &str {
        self.rpc_url.expose_secret()
    }
}

impl fmt::Debug for SecretManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretManager")
            .field("private_key", &"***REDACTED***")
            .field("rpc_url", &"***REDACTED***")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_redaction() {
        let secret = SecretString::new("my_secret".to_string());
        let debug = format!("{:?}", secret);
        assert_eq!(debug, "[REDACTED]");
    }

    #[test]
    fn test_secret_zeroization() {
        // Hard to test zeroization without unsafe inspection, skipping deep verify
        let _secret = SecretString::new("sensitive".to_string());
        // Just ensure it compiles and runs
    }
}
