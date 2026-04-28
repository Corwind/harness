//! Conversation messages and structured content blocks.
//!
//! A `Message` is the unit of conversation history. It always has a
//! role and a list of content blocks. Blocks are tagged so the same
//! representation can carry plain text, model-issued tool calls, the
//! results we feed back, and (eventually) image inputs.

use serde::{Deserialize, Serialize};

/// Author of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The end-user.
    User,
    /// The model's response.
    Assistant,
    /// A synthetic message carrying tool execution results back to the
    /// model. (Some providers fold these into the user role on the
    /// wire; we keep them separate in our domain.)
    Tool,
    /// System / instruction prompt. Persisted on the conversation
    /// itself rather than per-message in most providers, but kept in
    /// the enum for round-trip use.
    System,
}

/// A typed content block inside a `Message`.
///
/// The serialized form is internally tagged with `"type"` so JSON
/// payloads round-trip cleanly with provider APIs that use the same
/// shape.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text { text: String },

    /// A tool call requested by the model. `id` is the provider-issued
    /// identifier we will echo back in the matching `ToolResult`.
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// The result we feed back for a previous `ToolUse`. `is_error`
    /// signals whether the tool failed.
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        is_error: bool,
        content: serde_json::Value,
    },

    /// An image input (vision-capable providers only). The source is a
    /// data URL or remote URL — the orchestrator and adapter decide
    /// how to encode for the provider.
    Image { source: ImageSource },
}

/// Image source for `ContentBlock::Image`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSource {
    /// `data:` URL or base64-encoded bytes.
    Base64 { media_type: String, data: String },
    /// Remote URL the provider will fetch.
    Url { url: String },
}

/// One conversation message with structured content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// Convenience constructor for a single-text-block message.
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }
}
