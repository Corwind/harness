//! Behavior tests for the Claude provider.
//!
//! These pin every observable contract between
//! `ClaudeProvider` and the rest of the system: HTTP request shape,
//! Anthropic SSE → `ChatEvent` translation, error mapping, and
//! `list_models`. They use `wiremock` to stand in for `api.anthropic.com`,
//! so no network access is required.

use futures::StreamExt;
use harness_core::chat::{ChatEvent, ChatRequest, StopReason, Usage};
use harness_core::error::ProviderError;
use harness_core::message::{Message, Role};
use harness_core::provider::{LlmProvider, ProviderConfig};
use harness_providers_claude::ClaudeProvider;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SSE_MIME: &str = "text/event-stream";

fn provider_config(base_url: &str) -> ProviderConfig {
    ProviderConfig {
        provider_id: "claude".into(),
        config: json!({
            "api_key": "sk-test-key",
            "base_url": base_url,
            "api_version": "2023-06-01",
        }),
    }
}

fn simple_request() -> ChatRequest {
    ChatRequest {
        model: "claude-sonnet-4-6".into(),
        messages: vec![Message::text(Role::User, "hi")],
        system: None,
        tools: vec![],
        max_tokens: Some(64),
        temperature: None,
    }
}

/// Build an SSE response body from a list of (event, data) pairs.
fn sse_body(events: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (name, data) in events {
        out.push_str("event: ");
        out.push_str(name);
        out.push('\n');
        out.push_str("data: ");
        out.push_str(data);
        out.push_str("\n\n");
    }
    out
}

async fn collect(provider: &ClaudeProvider, cfg: &ProviderConfig) -> Vec<ChatEvent> {
    let stream = provider
        .chat(cfg, simple_request())
        .await
        .expect("chat() should return a stream when the upstream is 200");
    stream.collect().await
}

// ---------------------------------------------------------------------------
// 1. simple non-streamed text — single delta + stop
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_text_response() {
    let server = MockServer::start().await;
    let body = sse_body(&[
        (
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_simple","type":"message","role":"assistant","content":[],"model":"claude-sonnet-4-6","usage":{"input_tokens":7,"output_tokens":0}}}"#,
        ),
        (
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello!"}}"#,
        ),
        (
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        ),
        (
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}"#,
        ),
        ("message_stop", r#"{"type":"message_stop"}"#),
    ]);

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk-test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .and(header("accept", SSE_MIME))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(body, SSE_MIME)
                .insert_header("content-type", SSE_MIME),
        )
        .mount(&server)
        .await;

    let cfg = provider_config(&server.uri());
    let provider = ClaudeProvider::new();
    let events = collect(&provider, &cfg).await;

    assert_eq!(
        events,
        vec![
            ChatEvent::MessageStart {
                id: "msg_simple".into()
            },
            ChatEvent::ContentDelta {
                text: "hello!".into()
            },
            ChatEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
                usage: Some(Usage {
                    input_tokens: 7,
                    output_tokens: 3,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                }),
            },
        ]
    );
}

// ---------------------------------------------------------------------------
// 2. streamed text — multiple deltas concatenate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn streamed_text_multiple_deltas() {
    let server = MockServer::start().await;
    let body = sse_body(&[
        (
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_stream","usage":{"input_tokens":12,"output_tokens":0}}}"#,
        ),
        (
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"the "}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"quick "}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"brown fox"}}"#,
        ),
        (
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        ),
        (
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":11}}"#,
        ),
        ("message_stop", r#"{"type":"message_stop"}"#),
    ]);

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(body, SSE_MIME)
                .insert_header("content-type", SSE_MIME),
        )
        .mount(&server)
        .await;

    let cfg = provider_config(&server.uri());
    let provider = ClaudeProvider::new();
    let events = collect(&provider, &cfg).await;

    let texts: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            ChatEvent::ContentDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts.concat(), "the quick brown fox");
    assert!(matches!(
        events.last(),
        Some(ChatEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
            ..
        })
    ));
}

// ---------------------------------------------------------------------------
// 3. tool use — Start / Delta / Stop with parsed JSON
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_use_lifecycle() {
    let server = MockServer::start().await;
    let body = sse_body(&[
        (
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_tool","usage":{"input_tokens":40,"output_tokens":0}}}"#,
        ),
        (
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"echo","input":{}}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"text\":"}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"hi\"}"}}"#,
        ),
        (
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        ),
        (
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":4}}"#,
        ),
        ("message_stop", r#"{"type":"message_stop"}"#),
    ]);

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(body, SSE_MIME)
                .insert_header("content-type", SSE_MIME),
        )
        .mount(&server)
        .await;

    let cfg = provider_config(&server.uri());
    let provider = ClaudeProvider::new();
    let events = collect(&provider, &cfg).await;

    assert_eq!(
        events,
        vec![
            ChatEvent::MessageStart {
                id: "msg_tool".into()
            },
            ChatEvent::ToolUseStart {
                id: "toolu_1".into(),
                name: "echo".into(),
            },
            ChatEvent::ToolUseDelta {
                id: "toolu_1".into(),
                partial_json: "{\"text\":".into(),
            },
            ChatEvent::ToolUseDelta {
                id: "toolu_1".into(),
                partial_json: "\"hi\"}".into(),
            },
            ChatEvent::ToolUseStop {
                id: "toolu_1".into(),
                input: json!({"text": "hi"}),
            },
            ChatEvent::MessageStop {
                stop_reason: StopReason::ToolUse,
                usage: Some(Usage {
                    input_tokens: 40,
                    output_tokens: 4,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                }),
            },
        ]
    );
}

// ---------------------------------------------------------------------------
// 4. parallel tool use — interleaved deltas across two block indices
// ---------------------------------------------------------------------------

#[tokio::test]
async fn parallel_tool_use() {
    let server = MockServer::start().await;
    let body = sse_body(&[
        (
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_par","usage":{"input_tokens":80,"output_tokens":0}}}"#,
        ),
        (
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_a","name":"search","input":{}}}"#,
        ),
        (
            "content_block_start",
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_b","name":"calc","input":{}}}"#,
        ),
        // Interleave deltas across the two blocks.
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"q\":"}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"x\":"}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"cats\"}"}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"42}"}}"#,
        ),
        (
            "content_block_stop",
            r#"{"type":"content_block_stop","index":1}"#,
        ),
        (
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        ),
        (
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":15}}"#,
        ),
        ("message_stop", r#"{"type":"message_stop"}"#),
    ]);

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(body, SSE_MIME)
                .insert_header("content-type", SSE_MIME),
        )
        .mount(&server)
        .await;

    let cfg = provider_config(&server.uri());
    let provider = ClaudeProvider::new();
    let events = collect(&provider, &cfg).await;

    // Both tool_use lifecycles must be present, properly attributed by id.
    let starts: Vec<&ChatEvent> = events
        .iter()
        .filter(|e| matches!(e, ChatEvent::ToolUseStart { .. }))
        .collect();
    assert_eq!(starts.len(), 2);

    // Verify both ToolUseStop events with parsed inputs.
    let stops: Vec<(&str, &serde_json::Value)> = events
        .iter()
        .filter_map(|e| match e {
            ChatEvent::ToolUseStop { id, input } => Some((id.as_str(), input)),
            _ => None,
        })
        .collect();
    assert_eq!(stops.len(), 2);
    let stops_map: std::collections::HashMap<&str, &serde_json::Value> =
        stops.into_iter().collect();
    assert_eq!(stops_map.get("toolu_a"), Some(&&json!({"q": "cats"})));
    assert_eq!(stops_map.get("toolu_b"), Some(&&json!({"x": 42})));

    // ToolUseDelta events should preserve their per-id attribution and
    // partial_json content.
    let deltas: Vec<(&str, &str)> = events
        .iter()
        .filter_map(|e| match e {
            ChatEvent::ToolUseDelta { id, partial_json } => {
                Some((id.as_str(), partial_json.as_str()))
            }
            _ => None,
        })
        .collect();
    let a_chunks: String = deltas
        .iter()
        .filter_map(|(id, c)| (*id == "toolu_a").then_some(*c))
        .collect();
    let b_chunks: String = deltas
        .iter()
        .filter_map(|(id, c)| (*id == "toolu_b").then_some(*c))
        .collect();
    assert_eq!(a_chunks, "{\"q\":\"cats\"}");
    assert_eq!(b_chunks, "{\"x\":42}");

    assert!(matches!(
        events.last(),
        Some(ChatEvent::MessageStop {
            stop_reason: StopReason::ToolUse,
            ..
        })
    ));
}

// ---------------------------------------------------------------------------
// 5. 401 unauthorized → Err(ProviderError::Unauthorized)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unauthorized_returns_typed_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "type": "error",
            "error": {"type": "authentication_error", "message": "invalid api key"}
        })))
        .mount(&server)
        .await;

    let cfg = provider_config(&server.uri());
    let provider = ClaudeProvider::new();
    let result = provider.chat(&cfg, simple_request()).await;
    match result {
        Err(ProviderError::Unauthorized(msg)) => {
            assert!(msg.contains("invalid api key"), "got: {msg}")
        }
        Err(other) => panic!("expected Unauthorized, got {other:?}"),
        Ok(_) => panic!("expected Unauthorized, got Ok stream"),
    }
}

// ---------------------------------------------------------------------------
// 6. 429 rate limited → Err(ProviderError::RateLimited)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rate_limited_carries_retry_after() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("retry-after", "12")
                .set_body_json(json!({
                    "type": "error",
                    "error": {"type": "rate_limit_error", "message": "slow down"}
                })),
        )
        .mount(&server)
        .await;

    let cfg = provider_config(&server.uri());
    let provider = ClaudeProvider::new();
    let result = provider.chat(&cfg, simple_request()).await;
    match result {
        Err(ProviderError::RateLimited {
            retry_after_secs,
            message,
        }) => {
            assert_eq!(retry_after_secs, Some(12));
            assert!(message.contains("slow down"), "got: {message}");
        }
        Err(other) => panic!("expected RateLimited, got {other:?}"),
        Ok(_) => panic!("expected RateLimited, got Ok stream"),
    }
}

// ---------------------------------------------------------------------------
// 7. mid-stream error — terminal ChatEvent::Error, not Err(...)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mid_stream_error_is_terminal_chat_event() {
    let server = MockServer::start().await;
    let body = sse_body(&[
        (
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_err","usage":{"input_tokens":3}}}"#,
        ),
        (
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
        ),
        (
            "error",
            r#"{"type":"error","error":{"type":"overloaded_error","message":"server overloaded"}}"#,
        ),
        // Anything after the error should be ignored.
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"NEVER"}}"#,
        ),
        ("message_stop", r#"{"type":"message_stop"}"#),
    ]);

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(body, SSE_MIME)
                .insert_header("content-type", SSE_MIME),
        )
        .mount(&server)
        .await;

    let cfg = provider_config(&server.uri());
    let provider = ClaudeProvider::new();
    let events = collect(&provider, &cfg).await;

    // The chat() call itself succeeded — error came inside the stream.
    assert!(matches!(events.first(), Some(ChatEvent::MessageStart { .. })));
    // The stream must terminate at the Error event.
    let last = events.last().expect("at least one event");
    match last {
        ChatEvent::Error { message } => {
            assert!(message.contains("server overloaded"), "got: {message}");
        }
        other => panic!("expected terminal Error event, got {other:?}"),
    }
    // No stray content after the terminal error.
    assert!(!events
        .iter()
        .any(|e| matches!(e, ChatEvent::ContentDelta { text } if text == "NEVER")));
}

// ---------------------------------------------------------------------------
// 8. list_models
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_models_returns_expected_models() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", "sk-test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {
                    "type": "model",
                    "id": "claude-sonnet-4-6",
                    "display_name": "Claude Sonnet 4.6",
                    "created_at": "2026-01-01T00:00:00Z"
                },
                {
                    "type": "model",
                    "id": "claude-opus-4-7",
                    "display_name": "Claude Opus 4.7",
                    "created_at": "2026-02-01T00:00:00Z"
                }
            ],
            "has_more": false
        })))
        .mount(&server)
        .await;

    let cfg = provider_config(&server.uri());
    let provider = ClaudeProvider::new();
    let models = provider
        .list_models(&cfg)
        .await
        .expect("list_models should succeed");

    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "claude-sonnet-4-6");
    assert_eq!(models[0].display_name, "Claude Sonnet 4.6");
    assert_eq!(models[1].id, "claude-opus-4-7");
    assert_eq!(models[1].display_name, "Claude Opus 4.7");
    // We populate context_window / max_output_tokens with sensible
    // defaults since Anthropic's list endpoint doesn't return them.
    assert_eq!(models[0].context_window, Some(200_000));
    assert_eq!(models[0].max_output_tokens, Some(8192));
}

// ---------------------------------------------------------------------------
// Static check that the trait is implemented (compile-time).
// ---------------------------------------------------------------------------

#[test]
fn claude_provider_is_a_trait_object() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ClaudeProvider>();
    let _: Box<dyn LlmProvider> = Box::new(ClaudeProvider::new());
}
