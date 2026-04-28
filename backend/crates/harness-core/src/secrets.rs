//! Secrets port — opaque key/value store for API keys and similar
//! credentials.
//!
//! Adapters: in production, the SQLite-backed encrypted store in
//! `harness-storage`; in tests, an in-memory map. The Swift side
//! manages the encryption key via Keychain and passes it to the
//! backend at startup (see PLAN §2.3).

use async_trait::async_trait;

use crate::error::SecretsError;

/// The secrets port.
#[async_trait]
pub trait SecretsVault: Send + Sync + 'static {
    /// Fetch a secret by key. `Ok(None)` when the key is absent.
    async fn get(&self, key: &str) -> Result<Option<String>, SecretsError>;

    /// Store a secret, replacing any existing value at this key.
    async fn put(&self, key: &str, value: &str) -> Result<(), SecretsError>;

    /// Remove a secret. Idempotent — deleting an absent key returns
    /// `Ok`.
    async fn delete(&self, key: &str) -> Result<(), SecretsError>;
}
