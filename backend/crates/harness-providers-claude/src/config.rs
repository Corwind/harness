//! Provider-specific configuration shape.
//!
//! Stored opaquely as a `serde_json::Value` inside `ProviderConfig`;
//! parsed here on every call so we never persist a typed copy.

use harness_core::error::ProviderError;
use harness_core::provider::ProviderConfig;
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub(crate) const DEFAULT_API_VERSION: &str = "2023-06-01";

/// Parsed provider config used by the Claude adapter.
#[derive(Debug, Clone)]
pub(crate) struct ClaudeConfig {
    pub api_key: String,
    pub base_url: String,
    pub api_version: String,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    /// Either an inline API key (test-only / dev) or the key the
    /// composition root used to fetch it from `SecretsVault`. Adapters
    /// don't talk to the vault — the server resolves the secret and
    /// inlines it before invoking the provider, so we simply take
    /// what's here.
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_version: Option<String>,
}

pub(crate) fn parse(cfg: &ProviderConfig) -> Result<ClaudeConfig, ProviderError> {
    let raw: RawConfig = serde_json::from_value(cfg.config.clone())
        .map_err(|e| ProviderError::NotConfigured(format!("invalid claude config: {e}")))?;

    let api_key = raw
        .api_key
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProviderError::NotConfigured("missing api_key".to_string()))?;

    Ok(ClaudeConfig {
        api_key,
        base_url: raw
            .base_url
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        api_version: raw
            .api_version
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_API_VERSION.to_string()),
    })
}
