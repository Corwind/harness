//! Chat request / event types — the streaming contract between the
//! orchestrator and any `LlmProvider`.
//!
//! `ChatEvent` is the canonical event vocabulary. Provider adapters
//! translate vendor-specific SSE payloads into this enum; the
//! orchestrator translates this enum (plus its own tool-execution
//! events) into the public SSE stream surfaced to the UI.

use serde::{Deserialize, Serialize};

use crate::message::Message;
use crate::tool::ToolDefinition;

/// One turn's request to a provider.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,

    /// Optional system prompt. Providers without system support either
    /// fold this into the first user message or surface a
    /// `ProviderCapabilities` flag indicating it is unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,

    /// Tools the model may call this turn. Empty means tool use is
    /// disabled for this turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

/// Streaming events emitted by an `LlmProvider::chat` call.
///
/// The serialized form is internally tagged with `"type"` and uses
/// `snake_case` so the wire format is stable across providers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    /// First event of a response. `id` is the assistant message id
    /// from the provider.
    MessageStart { id: String },

    /// Incremental text delta belonging to the current assistant
    /// message.
    ContentDelta { text: String },

    /// The model has begun emitting a tool call.
    ToolUseStart { id: String, name: String },

    /// Incremental JSON fragment for a tool call's input. Concatenate
    /// across deltas to reconstruct the full input string.
    ToolUseDelta { id: String, partial_json: String },

    /// The model finished emitting a tool call. `input` is the parsed
    /// JSON value.
    ToolUseStop {
        id: String,
        input: serde_json::Value,
    },

    /// Terminal event for an assistant turn. Includes a stop reason
    /// and (when known) usage stats.
    MessageStop {
        stop_reason: StopReason,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
    },

    /// Non-fatal-but-visible error from the provider. Terminates the
    /// stream.
    Error { message: String },
}

/// Why an assistant message ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Model emitted a natural end-of-turn marker.
    EndTurn,
    /// Provider hit `max_tokens`.
    MaxTokens,
    /// Provider hit a stop sequence.
    StopSequence,
    /// Model wants tools to be executed; orchestrator should run them
    /// and re-invoke chat with the augmented history.
    ToolUse,
    /// Cancelled by the orchestrator (user pressed cancel, etc.).
    Cancelled,
    /// Catch-all for provider-specific reasons.
    Other,
}

/// Token usage stats reported by the provider when known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    /// Cache-read tokens (Anthropic-style); zero when unsupported.
    #[serde(default)]
    pub cache_read_input_tokens: u32,
    /// Cache-creation tokens (Anthropic-style); zero when unsupported.
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
}
