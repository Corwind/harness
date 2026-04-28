//! Built-in `FakeProvider` for deterministic end-to-end testing.
//!
//! Activated at runtime when the binary boots with
//! `HARNESS_FAKE_PROVIDER=1`. The fake registers under provider id
//! `"claude"` so the Swift client and existing conversations don't need
//! to care that the backend isn't talking to Anthropic.
//!
//! This module is **not** `#[cfg(test)]` — it ships in the binary so
//! Swift-side e2e tests (T2.1, T2.2) can drive the full stack without
//! a real network.
//!
//! ## Behavior
//!
//! Stateless: the fake decides what to emit purely from the conversation
//! history passed in `ChatRequest::messages`.
//!
//! * If the last message is a `Role::Tool` `ToolResult` block, this is
//!   the orchestrator's second turn after a tool call — emit a text
//!   reply quoting the tool output (`"Tool said: <output>."`).
//! * Else, look at the last user-text content. If the last `Role::User`
//!   message contains the substring `"echo:"`, emit a `tool_use` for
//!   the `echo` tool with input `{ "text": "<everything after the
//!   first 'echo:' substring, trimmed>" }` and a
//!   `MessageStop { stop_reason: ToolUse }`.
//! * Otherwise emit a single `ContentDelta { text: "Hello from fake
//!   provider." }` followed by `MessageStop { EndTurn }`.
//!
//! ## Pacing
//!
//! `HARNESS_FAKE_PROVIDER_DELAY_MS=<n>` (parsed at construction time)
//! injects a `tokio::time::sleep` of `n` milliseconds between every
//! emitted `ChatEvent`. Unset / non-numeric / negative → `0` (the fake
//! is fully synchronous, matching the original T2.1.x behaviour).
//!
//! The Swift live-cancellation e2e test
//! (`ChatViewModelLiveTests.testCancellationLiveAgainstFakeProvider`)
//! sets this so the orchestrator has a window to observe `cancel()`
//! between the canned events.

use std::time::Duration;

use async_trait::async_trait;
use futures::stream::BoxStream;
use harness_core::{
    chat::{ChatEvent, ChatRequest, StopReason},
    error::ProviderError,
    message::{ContentBlock, Role},
    provider::{LlmProvider, ModelInfo, ProviderCapabilities, ProviderConfig},
};

/// Stable text the no-tool path emits.
pub const FAKE_GREETING: &str = "Hello from fake provider.";

/// Stable id used for the synthetic tool-use call. Must match between
/// the `tool_use_*` deltas the orchestrator stitches together.
pub const FAKE_TOOL_USE_ID: &str = "fake_tu_1";

/// Env var that paces the fake provider's chat stream. See module docs.
pub const ENV_FAKE_PROVIDER_DELAY_MS: &str = "HARNESS_FAKE_PROVIDER_DELAY_MS";

/// Failure-mode injection for tests of the `ProviderError` → HTTP
/// mapping (T1.followup).
///
/// Each variant flips a switch on [`FakeProvider`]: every subsequent
/// `list_models` / `chat` call returns the corresponding `ProviderError`
/// instead of the canned success path. Production code uses
/// [`FakeProvider::new`] which leaves `failure: None`; tests use
/// [`FakeProvider::with_failure`].
#[derive(Debug, Clone)]
pub enum FakeFailure {
    Unauthorized,
    RateLimited { retry_after_secs: Option<u64> },
    Transport,
    UpstreamStatus(u16),
}

/// Built-in deterministic provider used by the `HARNESS_FAKE_PROVIDER`
/// runtime knob and (transitively) by the Swift e2e suite.
#[derive(Debug, Default)]
pub struct FakeProvider {
    failure: Option<FakeFailure>,
    /// Inter-event sleep applied to the chat stream. `0` = no delay,
    /// matching the original synchronous behaviour. Sourced from
    /// [`ENV_FAKE_PROVIDER_DELAY_MS`] when constructed via
    /// [`FakeProvider::new`].
    delay_ms: u64,
}

impl FakeProvider {
    /// Production constructor: reads pacing from the env. Used by
    /// `bootstrap::acquire_app_state` when `HARNESS_FAKE_PROVIDER=1`.
    pub fn new() -> Self {
        Self {
            failure: None,
            delay_ms: read_delay_env(),
        }
    }

    /// Construct a fake whose every `list_models` / `chat` call returns
    /// the supplied `ProviderError`. Test-only seam — the production
    /// env-knob path uses [`FakeProvider::new`].
    pub fn with_failure(failure: FakeFailure) -> Self {
        Self {
            failure: Some(failure),
            delay_ms: 0,
        }
    }

    /// Test-only: explicit pacing (bypasses the env). Used by the
    /// behaviour tests of [`ENV_FAKE_PROVIDER_DELAY_MS`] itself so they
    /// stay deterministic regardless of the running process's env.
    pub fn with_delay_ms(delay_ms: u64) -> Self {
        Self {
            failure: None,
            delay_ms,
        }
    }

    fn synth_error(&self) -> Option<ProviderError> {
        self.failure.as_ref().map(|f| match f {
            FakeFailure::Unauthorized => ProviderError::Unauthorized("bad api key".to_owned()),
            FakeFailure::RateLimited { retry_after_secs } => ProviderError::RateLimited {
                retry_after_secs: *retry_after_secs,
                message: "slow down".to_owned(),
            },
            FakeFailure::Transport => ProviderError::Transport("connect refused".to_owned()),
            FakeFailure::UpstreamStatus(s) => ProviderError::Request {
                status: Some(*s),
                message: format!("upstream returned {s}"),
            },
        })
    }
}

/// Parse [`ENV_FAKE_PROVIDER_DELAY_MS`] from the process env. Unset,
/// non-numeric, or out-of-range values fall back to `0` (synchronous).
/// Made `pub` so behavior tests can pin the parser separately from the
/// stream pacing.
pub fn read_delay_env() -> u64 {
    match std::env::var(ENV_FAKE_PROVIDER_DELAY_MS) {
        Ok(s) => s.parse::<u64>().unwrap_or(0),
        Err(_) => 0,
    }
}

#[async_trait]
impl LlmProvider for FakeProvider {
    fn id(&self) -> &'static str {
        // Registered as "claude" so existing conversations keep working
        // when the operator flips the env. The display name carries the
        // truth for the UI.
        "claude"
    }

    fn display_name(&self) -> &'static str {
        "Fake Claude"
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
            vision: false,
            system_prompt: true,
            max_context_tokens: Some(8192),
        }
    }

    async fn list_models(&self, _cfg: &ProviderConfig) -> Result<Vec<ModelInfo>, ProviderError> {
        if let Some(err) = self.synth_error() {
            return Err(err);
        }
        Ok(vec![ModelInfo {
            id: "fake-claude".to_owned(),
            display_name: "Fake Claude".to_owned(),
            context_window: Some(8192),
            max_output_tokens: Some(2048),
        }])
    }

    async fn chat(
        &self,
        _cfg: &ProviderConfig,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, ChatEvent>, ProviderError> {
        if let Some(err) = self.synth_error() {
            return Err(err);
        }
        let events = decide_turn(&request);
        let delay = Duration::from_millis(self.delay_ms);
        // async-stream lets us yield events with awaits between them.
        // The orchestrator's chat consumer races `cancel.cancelled()`
        // against `stream.next()`, so a non-zero delay opens a window
        // for cancellation between events.
        let s = async_stream::stream! {
            for ev in events {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                yield ev;
            }
        };
        Ok(Box::pin(s))
    }
}

/// Pure function: pick the events to emit for one chat turn from the
/// supplied history. Split out for unit-testability.
pub fn decide_turn(request: &ChatRequest) -> Vec<ChatEvent> {
    // Second-turn detection: if the last message is a Tool message
    // with a ToolResult block, the orchestrator just fed us the echo
    // result. Quote it.
    if let Some(last) = request.messages.last() {
        if matches!(last.role, Role::Tool) {
            if let Some(ContentBlock::ToolResult { content, .. }) = last.content.last() {
                let quoted = stringify_tool_output(content);
                return vec![
                    ChatEvent::MessageStart {
                        id: "msg_fake_2".to_owned(),
                    },
                    ChatEvent::ContentDelta {
                        text: format!("Tool said: {quoted}."),
                    },
                    ChatEvent::MessageStop {
                        stop_reason: StopReason::EndTurn,
                        usage: None,
                    },
                ];
            }
        }
    }

    // First-turn detection: scan the latest user message for an
    // "echo: <payload>" substring.
    if let Some(payload) = latest_echo_payload(request) {
        return tool_use_turn(&payload);
    }

    // Default greeting.
    vec![
        ChatEvent::MessageStart {
            id: "msg_fake_1".to_owned(),
        },
        ChatEvent::ContentDelta {
            text: FAKE_GREETING.to_owned(),
        },
        ChatEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
            usage: None,
        },
    ]
}

fn tool_use_turn(payload: &str) -> Vec<ChatEvent> {
    let input = serde_json::json!({ "text": payload });
    vec![
        ChatEvent::MessageStart {
            id: "msg_fake_1".to_owned(),
        },
        ChatEvent::ToolUseStart {
            id: FAKE_TOOL_USE_ID.to_owned(),
            name: "echo".to_owned(),
        },
        ChatEvent::ToolUseStop {
            id: FAKE_TOOL_USE_ID.to_owned(),
            input,
        },
        ChatEvent::MessageStop {
            stop_reason: StopReason::ToolUse,
            usage: None,
        },
    ]
}

/// Look for the last user message; if any of its text blocks contain
/// `"echo:"`, return the substring after it (trimmed).
fn latest_echo_payload(request: &ChatRequest) -> Option<String> {
    let last_user = request
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, Role::User))?;
    for block in &last_user.content {
        if let ContentBlock::Text { text } = block {
            if let Some(idx) = text.find("echo:") {
                let after = &text[idx + "echo:".len()..];
                let trimmed = after.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_owned());
                }
            }
        }
    }
    None
}

/// Render a tool-result content payload as a short string for the fake
/// to quote back. The `echo` tool returns its stdout as a JSON string
/// (per `harness-orchestrator::tool_exec`); we strip the surrounding
/// quotes when present and trim the trailing newline `/bin/echo` adds.
fn stringify_tool_output(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.trim_end().to_owned(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_core::message::Message;

    fn user_text(text: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_owned(),
            }],
        }
    }

    fn req(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: "fake-claude".into(),
            messages,
            system: None,
            tools: vec![],
            max_tokens: None,
            temperature: None,
        }
    }

    #[test]
    fn vanilla_user_turn_emits_greeting() {
        let r = req(vec![user_text("Hi there.")]);
        let events = decide_turn(&r);
        assert!(matches!(events[1], ChatEvent::ContentDelta { ref text } if text == FAKE_GREETING));
        assert!(matches!(
            events.last().unwrap(),
            ChatEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
                ..
            }
        ));
    }

    #[test]
    fn echo_substring_triggers_tool_use() {
        let r = req(vec![user_text("please echo: hi there")]);
        let events = decide_turn(&r);
        assert!(matches!(events[1], ChatEvent::ToolUseStart { ref name, .. } if name == "echo"));
        match &events[2] {
            ChatEvent::ToolUseStop { input, .. } => {
                assert_eq!(input.get("text").and_then(|v| v.as_str()), Some("hi there"));
            }
            other => panic!("expected ToolUseStop, got {other:?}"),
        }
        assert!(matches!(
            events.last().unwrap(),
            ChatEvent::MessageStop {
                stop_reason: StopReason::ToolUse,
                ..
            }
        ));
    }

    #[test]
    fn tool_result_in_history_triggers_quote_back() {
        let messages = vec![
            user_text("please echo: hi"),
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: FAKE_TOOL_USE_ID.into(),
                    name: "echo".into(),
                    input: serde_json::json!({ "text": "hi" }),
                }],
            },
            Message {
                role: Role::Tool,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: FAKE_TOOL_USE_ID.into(),
                    is_error: false,
                    content: serde_json::Value::String("hi\n".into()),
                }],
            },
        ];
        let events = decide_turn(&req(messages));
        match &events[1] {
            ChatEvent::ContentDelta { text } => {
                assert!(
                    text.contains("hi"),
                    "expected quote-back to mention 'hi': {text}"
                );
                assert!(text.starts_with("Tool said:"));
            }
            other => panic!("expected ContentDelta, got {other:?}"),
        }
    }

    #[test]
    fn no_echo_substring_falls_back_to_greeting() {
        // Even with the keyword "echo" in the prompt without a colon,
        // we must NOT trigger the tool path.
        let r = req(vec![user_text("can you echo back? thanks.")]);
        let events = decide_turn(&r);
        assert!(events
            .iter()
            .any(|e| matches!(e, ChatEvent::ContentDelta { text } if text == FAKE_GREETING)));
    }
}
