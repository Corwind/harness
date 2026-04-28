//! Round-trip serde tests for the wire-shape types.
//!
//! These tests pin the JSON representation we expose on the public
//! HTTP / SSE surface. Changing the JSON shape of any of these types
//! is a breaking change for the Swift client and any provider adapter
//! and should require updating these fixtures intentionally.

use harness_core::chat::{ChatEvent, ChatRequest, StopReason, Usage};
use harness_core::ids::{ConversationId, ProviderId, SandboxTemplateId};
use harness_core::message::{ContentBlock, ImageSource, Message, Role};
use harness_core::provider::{ModelInfo, ProviderCapabilities, ProviderConfig};
use harness_core::repo::{Conversation, ConversationPatch};
use harness_core::sandbox::SandboxTemplate;
use harness_core::tool::{ToolCommand, ToolDefinition, ToolKind};
use pretty_assertions::assert_eq;
use serde_json::json;

/// Helper: assert a value serialises to the expected JSON value and
/// the JSON value deserialises back to an equal value.
fn round_trip<T>(value: T, expected: serde_json::Value)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let serialized = serde_json::to_value(&value).expect("serialize");
    assert_eq!(serialized, expected, "serialised JSON shape mismatch");

    let deserialized: T = serde_json::from_value(expected).expect("deserialize");
    assert_eq!(deserialized, value, "round-trip value mismatch");
}

#[test]
fn role_serializes_lowercase() {
    round_trip(Role::User, json!("user"));
    round_trip(Role::Assistant, json!("assistant"));
    round_trip(Role::Tool, json!("tool"));
    round_trip(Role::System, json!("system"));
}

#[test]
fn content_block_text_round_trip() {
    round_trip(
        ContentBlock::Text {
            text: "hello".into(),
        },
        json!({"type": "text", "text": "hello"}),
    );
}

#[test]
fn content_block_tool_use_round_trip() {
    round_trip(
        ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "echo".into(),
            input: json!({"text": "hi"}),
        },
        json!({
            "type": "tool_use",
            "id": "tu_1",
            "name": "echo",
            "input": {"text": "hi"},
        }),
    );
}

#[test]
fn content_block_tool_result_round_trip() {
    round_trip(
        ContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            is_error: false,
            content: json!({"stdout": "hi\n"}),
        },
        json!({
            "type": "tool_result",
            "tool_use_id": "tu_1",
            "is_error": false,
            "content": {"stdout": "hi\n"},
        }),
    );
}

#[test]
fn content_block_image_round_trip() {
    round_trip(
        ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "AAAA".into(),
            },
        },
        json!({
            "type": "image",
            "source": {"kind": "base64", "media_type": "image/png", "data": "AAAA"},
        }),
    );

    round_trip(
        ContentBlock::Image {
            source: ImageSource::Url {
                url: "https://example.com/cat.png".into(),
            },
        },
        json!({
            "type": "image",
            "source": {"kind": "url", "url": "https://example.com/cat.png"},
        }),
    );
}

#[test]
fn message_round_trip() {
    round_trip(
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
        },
        json!({
            "role": "user",
            "content": [{"type": "text", "text": "hi"}],
        }),
    );
}

#[test]
fn chat_request_omits_optional_fields() {
    let req = ChatRequest {
        model: "claude-sonnet-4-6".into(),
        messages: vec![Message::text(Role::User, "hi")],
        system: None,
        tools: vec![],
        max_tokens: None,
        temperature: None,
    };
    let v = serde_json::to_value(&req).unwrap();
    assert_eq!(
        v,
        json!({
            "model": "claude-sonnet-4-6",
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hi"}]}
            ],
        })
    );
    let back: ChatRequest = serde_json::from_value(v).unwrap();
    assert_eq!(back, req);
}

#[test]
fn chat_request_includes_optional_fields_when_set() {
    let req = ChatRequest {
        model: "m".into(),
        messages: vec![],
        system: Some("you are helpful".into()),
        tools: vec![ToolDefinition {
            name: "echo".into(),
            description: Some("echoes input".into()),
            input_schema: json!({"type": "object"}),
        }],
        max_tokens: Some(1024),
        temperature: Some(0.5),
    };
    round_trip(
        req,
        json!({
            "model": "m",
            "messages": [],
            "system": "you are helpful",
            "tools": [{
                "name": "echo",
                "description": "echoes input",
                "input_schema": {"type": "object"},
            }],
            "max_tokens": 1024,
            "temperature": 0.5,
        }),
    );
}

#[test]
fn chat_event_message_start_round_trip() {
    round_trip(
        ChatEvent::MessageStart { id: "msg_1".into() },
        json!({"type": "message_start", "id": "msg_1"}),
    );
}

#[test]
fn chat_event_content_delta_round_trip() {
    round_trip(
        ChatEvent::ContentDelta {
            text: "hello".into(),
        },
        json!({"type": "content_delta", "text": "hello"}),
    );
}

#[test]
fn chat_event_tool_use_lifecycle_round_trip() {
    round_trip(
        ChatEvent::ToolUseStart {
            id: "tu_1".into(),
            name: "echo".into(),
        },
        json!({"type": "tool_use_start", "id": "tu_1", "name": "echo"}),
    );
    round_trip(
        ChatEvent::ToolUseDelta {
            id: "tu_1".into(),
            partial_json: "{\"x\":".into(),
        },
        json!({"type": "tool_use_delta", "id": "tu_1", "partial_json": "{\"x\":"}),
    );
    round_trip(
        ChatEvent::ToolUseStop {
            id: "tu_1".into(),
            input: json!({"x": 1}),
        },
        json!({"type": "tool_use_stop", "id": "tu_1", "input": {"x": 1}}),
    );
}

#[test]
fn chat_event_message_stop_round_trip() {
    round_trip(
        ChatEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
            usage: Some(Usage {
                input_tokens: 10,
                output_tokens: 20,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }),
        },
        json!({
            "type": "message_stop",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 20,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0,
            },
        }),
    );
}

#[test]
fn chat_event_message_stop_omits_usage_when_absent() {
    let v = serde_json::to_value(&ChatEvent::MessageStop {
        stop_reason: StopReason::ToolUse,
        usage: None,
    })
    .unwrap();
    assert_eq!(
        v,
        json!({"type": "message_stop", "stop_reason": "tool_use"})
    );
}

#[test]
fn chat_event_error_round_trip() {
    round_trip(
        ChatEvent::Error {
            message: "boom".into(),
        },
        json!({"type": "error", "message": "boom"}),
    );
}

#[test]
fn stop_reason_serializes_snake_case() {
    round_trip(StopReason::EndTurn, json!("end_turn"));
    round_trip(StopReason::MaxTokens, json!("max_tokens"));
    round_trip(StopReason::StopSequence, json!("stop_sequence"));
    round_trip(StopReason::ToolUse, json!("tool_use"));
    round_trip(StopReason::Cancelled, json!("cancelled"));
    round_trip(StopReason::Other, json!("other"));
}

#[test]
fn provider_capabilities_round_trip() {
    round_trip(
        ProviderCapabilities {
            streaming: true,
            tools: true,
            vision: false,
            system_prompt: true,
            max_context_tokens: Some(200_000),
        },
        json!({
            "streaming": true,
            "tools": true,
            "vision": false,
            "system_prompt": true,
            "max_context_tokens": 200_000,
        }),
    );
}

#[test]
fn provider_config_round_trip() {
    round_trip(
        ProviderConfig {
            provider_id: ProviderId::from_string("claude"),
            config: json!({"secret_ref": "anthropic_api_key"}),
        },
        json!({
            "provider_id": "claude",
            "config": {"secret_ref": "anthropic_api_key"},
        }),
    );
}

#[test]
fn model_info_round_trip() {
    round_trip(
        ModelInfo {
            id: "claude-sonnet-4-6".into(),
            display_name: "Claude Sonnet 4.6".into(),
            context_window: Some(200_000),
            max_output_tokens: Some(8192),
        },
        json!({
            "id": "claude-sonnet-4-6",
            "display_name": "Claude Sonnet 4.6",
            "context_window": 200_000,
            "max_output_tokens": 8192,
        }),
    );
}

#[test]
fn ids_serialize_transparent() {
    let cid = ConversationId::from_string("conv_123");
    let v = serde_json::to_value(&cid).unwrap();
    assert_eq!(v, json!("conv_123"));
    let back: ConversationId = serde_json::from_value(json!("conv_123")).unwrap();
    assert_eq!(back, cid);
}

#[test]
fn conversation_patch_distinguishes_unset_from_clear() {
    let unset = ConversationPatch::default();
    assert_eq!(serde_json::to_value(&unset).unwrap(), json!({}));

    let clear = ConversationPatch {
        sandbox_template_id: Some(None),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&clear).unwrap(),
        json!({"sandbox_template_id": null}),
    );

    let set = ConversationPatch {
        sandbox_template_id: Some(Some(SandboxTemplateId::from_string("tpl_1"))),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&set).unwrap(),
        json!({"sandbox_template_id": "tpl_1"}),
    );
}

#[test]
fn conversation_round_trip_uses_iso_timestamps() {
    let created = chrono::DateTime::parse_from_rfc3339("2026-04-28T10:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let updated = chrono::DateTime::parse_from_rfc3339("2026-04-28T10:05:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let conv = Conversation {
        id: ConversationId::from_string("conv_1"),
        title: "tea time".into(),
        provider_id: ProviderId::from_string("claude"),
        model: "claude-sonnet-4-6".into(),
        sandbox_template_id: Some(SandboxTemplateId::from_string("tpl_1")),
        created_at: created,
        updated_at: updated,
    };
    let v = serde_json::to_value(&conv).unwrap();
    assert_eq!(
        v,
        json!({
            "id": "conv_1",
            "title": "tea time",
            "provider_id": "claude",
            "model": "claude-sonnet-4-6",
            "sandbox_template_id": "tpl_1",
            "created_at": "2026-04-28T10:00:00Z",
            "updated_at": "2026-04-28T10:05:00Z",
        })
    );
    let back: Conversation = serde_json::from_value(v).unwrap();
    assert_eq!(back, conv);
}

#[test]
fn sandbox_template_round_trip() {
    round_trip(
        SandboxTemplate {
            id: SandboxTemplateId::from_string("tpl_strict"),
            name: "strict-readonly".into(),
            description: Some("deny all writes".into()),
            profile: "(version 1)\n(deny default)\n".into(),
            is_builtin: true,
            created_at: 1_700_000_000,
            updated_at: 1_700_000_500,
        },
        json!({
            "id": "tpl_strict",
            "name": "strict-readonly",
            "description": "deny all writes",
            "profile": "(version 1)\n(deny default)\n",
            "is_builtin": true,
            "created_at": 1_700_000_000,
            "updated_at": 1_700_000_500,
        }),
    );
}

#[test]
fn tool_command_round_trip() {
    let mut env = std::collections::HashMap::new();
    env.insert("PATH".into(), "/usr/bin".into());
    let cmd = ToolCommand {
        program: "/bin/echo".into(),
        args: vec!["hi".into()],
        env,
        cwd: Some(std::path::PathBuf::from("/tmp")),
    };
    let v = serde_json::to_value(&cmd).unwrap();
    let back: ToolCommand = serde_json::from_value(v).unwrap();
    assert_eq!(back, cmd);
}

#[test]
fn tool_kind_round_trip() {
    round_trip(ToolKind::External, json!("external"));
    round_trip(ToolKind::InProcess, json!("in_process"));
}

#[test]
fn tool_definition_round_trip() {
    round_trip(
        ToolDefinition {
            name: "echo".into(),
            description: Some("echo input".into()),
            input_schema: json!({
                "type": "object",
                "properties": {"text": {"type": "string"}},
                "required": ["text"],
            }),
        },
        json!({
            "name": "echo",
            "description": "echo input",
            "input_schema": {
                "type": "object",
                "properties": {"text": {"type": "string"}},
                "required": ["text"],
            },
        }),
    );
}
