//! `LlmProvider` port — the abstraction every provider adapter
//! implements.
//!
//! The orchestrator owns conversation state and the tool-execution
//! loop. Providers are stateless beyond their config: given a request
//! they return a stream of `ChatEvent`s and that's all.

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::chat::{ChatEvent, ChatRequest};
use crate::error::ProviderError;
use crate::ids::ProviderId;

/// Capability matrix advertised by a provider. The HTTP layer surfaces
/// this so the UI can disable features the provider doesn't support
/// (e.g. tool use, vision attachments).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCapabilities {
    pub streaming: bool,
    pub tools: bool,
    pub vision: bool,
    pub system_prompt: bool,
    /// Maximum context window in tokens, when known.
    #[serde(default)]
    pub max_context_tokens: Option<u32>,
}

/// Provider configuration as stored. Open-shaped on purpose — each
/// provider declares its own JSON schema. The HTTP and storage layers
/// treat it as opaque.
///
/// Secret material (API keys) is **not** stored here in plaintext.
/// Adapters reference a secret by key name; the actual value comes
/// from the `SecretsVault` port.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider_id: ProviderId,
    /// Provider-defined fields (model defaults, base URL, secret key
    /// name, etc.).
    pub config: serde_json::Value,
}

/// Information about an available model for a configured provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
}

/// The LLM provider port.
///
/// Per PLAN §5.1 — implementations live in `harness-providers-*` and
/// are wired by the composition root (`harness-server`). The
/// orchestrator depends on this trait, never on a concrete adapter.
#[async_trait]
pub trait LlmProvider: Send + Sync + 'static {
    /// Stable string identifier ("claude", "ollama", …).
    fn id(&self) -> &'static str;

    /// Human-friendly name for the provider.
    fn display_name(&self) -> &'static str;

    /// What this provider supports.
    fn capabilities(&self) -> ProviderCapabilities;

    /// List the models this provider exposes given the configuration.
    async fn list_models(&self, cfg: &ProviderConfig) -> Result<Vec<ModelInfo>, ProviderError>;

    /// Run one chat turn. Returns a stream of `ChatEvent`s; the stream
    /// terminates with either a `MessageStop` or `Error` event before
    /// the underlying stream closes.
    async fn chat(
        &self,
        cfg: &ProviderConfig,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, ChatEvent>, ProviderError>;
}
