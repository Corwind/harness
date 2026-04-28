//! Anthropic Messages API wire types — request body, list_models
//! response, and the SSE event payload shapes we deserialise on the way
//! in.
//!
//! These types are private to the adapter. Nothing here is re-exported
//! from `lib.rs` — the only public surface is the `ChatEvent` stream
//! produced by the translation layer.

use std::collections::BTreeMap;

use harness_core::chat::ChatRequest;
use harness_core::message::{ContentBlock, ImageSource, Message, Role};
use harness_core::tool::ToolDefinition;
use serde::{Deserialize, Serialize};

/// Request body for `POST /v1/messages`.
#[derive(Debug, Serialize)]
pub(crate) struct AnthropicChatRequest<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub messages: Vec<AnthropicMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AnthropicToolDef<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub stream: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicMessage<'a> {
    pub role: &'static str,
    pub content: Vec<AnthropicContentBlock<'a>>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnthropicContentBlock<'a> {
    Text {
        text: &'a str,
    },
    ToolUse {
        id: &'a str,
        name: &'a str,
        input: &'a serde_json::Value,
    },
    ToolResult {
        tool_use_id: &'a str,
        content: serde_json::Value,
        #[serde(skip_serializing_if = "is_false")]
        is_error: bool,
    },
    Image {
        source: AnthropicImageSource<'a>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AnthropicImageSource<'a> {
    Base64 {
        media_type: &'a str,
        data: &'a str,
    },
    Url {
        url: &'a str,
    },
}

#[derive(Debug, Serialize)]
pub(crate) struct AnthropicToolDef<'a> {
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a str>,
    pub input_schema: &'a serde_json::Value,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Build the wire request from a domain `ChatRequest`. Anthropic
/// requires `max_tokens`; we default to a generous cap when the caller
/// did not specify one.
pub(crate) fn build_request<'a>(
    req: &'a ChatRequest,
    default_max_tokens: u32,
) -> AnthropicChatRequest<'a> {
    AnthropicChatRequest {
        model: &req.model,
        max_tokens: req.max_tokens.unwrap_or(default_max_tokens),
        messages: req.messages.iter().map(message_to_wire).collect(),
        system: req.system.as_deref(),
        tools: req.tools.iter().map(tool_def_to_wire).collect(),
        temperature: req.temperature,
        stream: true,
    }
}

fn message_to_wire(m: &Message) -> AnthropicMessage<'_> {
    AnthropicMessage {
        // Anthropic only accepts "user" and "assistant" on the
        // messages array. `Tool` results are sent back as a `user`
        // role message containing `tool_result` blocks; `System` is
        // handled via the top-level `system` field.
        role: match m.role {
            Role::Assistant => "assistant",
            Role::User | Role::Tool | Role::System => "user",
        },
        content: m.content.iter().map(content_to_wire).collect(),
    }
}

fn content_to_wire(c: &ContentBlock) -> AnthropicContentBlock<'_> {
    match c {
        ContentBlock::Text { text } => AnthropicContentBlock::Text { text },
        ContentBlock::ToolUse { id, name, input } => AnthropicContentBlock::ToolUse {
            id,
            name,
            input,
        },
        ContentBlock::ToolResult {
            tool_use_id,
            is_error,
            content,
        } => AnthropicContentBlock::ToolResult {
            tool_use_id,
            content: content.clone(),
            is_error: *is_error,
        },
        ContentBlock::Image { source } => AnthropicContentBlock::Image {
            source: match source {
                ImageSource::Base64 { media_type, data } => AnthropicImageSource::Base64 {
                    media_type,
                    data,
                },
                ImageSource::Url { url } => AnthropicImageSource::Url { url },
            },
        },
    }
}

fn tool_def_to_wire(t: &ToolDefinition) -> AnthropicToolDef<'_> {
    AnthropicToolDef {
        name: &t.name,
        description: t.description.as_deref(),
        input_schema: &t.input_schema,
    }
}

// ---------------------------------------------------------------------------
// SSE event payloads
// ---------------------------------------------------------------------------

/// Inner shape of the `message_start` event's `data:` line.
#[derive(Debug, Deserialize)]
pub(crate) struct MessageStartPayload {
    pub message: MessageStartMessage,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MessageStartMessage {
    pub id: String,
    #[serde(default)]
    pub usage: Option<UsagePayload>,
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub(crate) struct UsagePayload {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    #[serde(default)]
    pub cache_read_input_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
}

/// `content_block_start` payload.
#[derive(Debug, Deserialize)]
pub(crate) struct ContentBlockStartPayload {
    pub index: u32,
    pub content_block: ContentBlockStartInner,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ContentBlockStartInner {
    Text {
        #[serde(default)]
        #[allow(dead_code)]
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
    },
    /// Anything else (future-compatible).
    #[serde(other)]
    Unknown,
}

/// `content_block_delta` payload.
#[derive(Debug, Deserialize)]
pub(crate) struct ContentBlockDeltaPayload {
    pub index: u32,
    pub delta: ContentBlockDeltaInner,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ContentBlockDeltaInner {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
    #[serde(other)]
    Unknown,
}

/// `content_block_stop` payload.
#[derive(Debug, Deserialize)]
pub(crate) struct ContentBlockStopPayload {
    pub index: u32,
}

/// `message_delta` payload — carries final stop_reason and updated
/// `usage.output_tokens`.
#[derive(Debug, Deserialize)]
pub(crate) struct MessageDeltaPayload {
    #[serde(default)]
    pub delta: MessageDeltaInner,
    #[serde(default)]
    pub usage: Option<UsagePayload>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct MessageDeltaInner {
    #[serde(default)]
    pub stop_reason: Option<String>,
}

/// `error` event payload — both for top-level streamed errors and for
/// non-streamed error responses on the JSON body.
#[derive(Debug, Deserialize)]
pub(crate) struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ErrorBody {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

// ---------------------------------------------------------------------------
// list_models
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct ListModelsResponse {
    pub data: Vec<ListModelsItem>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListModelsItem {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Anthropic's list endpoint does not include context window /
    /// max output tokens directly; we keep the field open in case that
    /// changes, and otherwise fall back to a static table.
    #[serde(flatten, default)]
    pub _extra: BTreeMap<String, serde_json::Value>,
}
