//! Pure translation layer: Anthropic SSE event → `harness_core::ChatEvent`.
//!
//! Implemented as a small state machine fed one event at a time. Each
//! call returns a (possibly empty) batch of `ChatEvent`s, plus a flag
//! indicating whether the upstream stream has terminated (so the caller
//! can stop polling).
//!
//! Keeping this stateless w.r.t. I/O makes the streaming behaviour
//! exhaustively unit-testable from raw fixtures.

use std::collections::HashMap;

use harness_core::chat::{ChatEvent, StopReason, Usage};

use crate::wire::{
    ContentBlockDeltaInner, ContentBlockDeltaPayload, ContentBlockStartInner,
    ContentBlockStartPayload, ContentBlockStopPayload, MessageDeltaPayload, MessageStartPayload,
    UsagePayload,
};

/// Per-block bookkeeping. We only need to remember enough about a block
/// to (a) ignore deltas of the wrong shape and (b) parse accumulated
/// JSON for tool inputs at `content_block_stop` time.
#[derive(Debug)]
enum BlockState {
    Text,
    ToolUse {
        id: String,
        partial_json: String,
    },
    /// A block kind we don't recognise — drop deltas silently.
    Unknown,
}

/// Translator state. One instance per `chat()` invocation.
#[derive(Debug, Default)]
pub(crate) struct Translator {
    blocks: HashMap<u32, BlockState>,
    /// Accumulated usage across `message_start` and `message_delta`.
    usage: Option<Usage>,
    /// Latest stop reason from `message_delta`.
    stop_reason: Option<StopReason>,
    /// Set once the underlying stream is done emitting (after
    /// `message_stop` or `error`).
    pub(crate) terminated: bool,
}

impl Translator {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Feed one named SSE event. Returns the chat events produced and
    /// flips `self.terminated` on terminal events.
    pub(crate) fn handle(&mut self, event_name: &str, data: &str) -> Vec<ChatEvent> {
        match event_name {
            "message_start" => self.on_message_start(data),
            "content_block_start" => self.on_content_block_start(data),
            "content_block_delta" => self.on_content_block_delta(data),
            "content_block_stop" => self.on_content_block_stop(data),
            "message_delta" => {
                self.on_message_delta(data);
                Vec::new()
            }
            "message_stop" => {
                self.terminated = true;
                vec![ChatEvent::MessageStop {
                    stop_reason: self.stop_reason.unwrap_or(StopReason::Other),
                    usage: self.usage,
                }]
            }
            "error" => {
                self.terminated = true;
                vec![ChatEvent::Error {
                    message: parse_error(data),
                }]
            }
            // Anthropic emits `ping` events as keep-alives; ignore.
            _ => Vec::new(),
        }
    }

    /// Surface a transport-level decode error as a terminal `ChatEvent`.
    pub(crate) fn fatal(&mut self, message: String) -> ChatEvent {
        self.terminated = true;
        ChatEvent::Error { message }
    }

    fn on_message_start(&mut self, data: &str) -> Vec<ChatEvent> {
        match serde_json::from_str::<MessageStartPayload>(data) {
            Ok(p) => {
                if let Some(u) = p.message.usage {
                    self.usage = Some(merge_usage(self.usage, u));
                }
                vec![ChatEvent::MessageStart { id: p.message.id }]
            }
            Err(e) => vec![self.fatal(format!("decode message_start: {e}"))],
        }
    }

    fn on_content_block_start(&mut self, data: &str) -> Vec<ChatEvent> {
        let payload: ContentBlockStartPayload = match serde_json::from_str(data) {
            Ok(p) => p,
            Err(e) => return vec![self.fatal(format!("decode content_block_start: {e}"))],
        };
        match payload.content_block {
            ContentBlockStartInner::Text { .. } => {
                self.blocks.insert(payload.index, BlockState::Text);
                Vec::new()
            }
            ContentBlockStartInner::ToolUse { id, name } => {
                self.blocks.insert(
                    payload.index,
                    BlockState::ToolUse {
                        id: id.clone(),
                        partial_json: String::new(),
                    },
                );
                vec![ChatEvent::ToolUseStart { id, name }]
            }
            ContentBlockStartInner::Unknown => {
                self.blocks.insert(payload.index, BlockState::Unknown);
                Vec::new()
            }
        }
    }

    fn on_content_block_delta(&mut self, data: &str) -> Vec<ChatEvent> {
        let payload: ContentBlockDeltaPayload = match serde_json::from_str(data) {
            Ok(p) => p,
            Err(e) => return vec![self.fatal(format!("decode content_block_delta: {e}"))],
        };
        let state = self.blocks.get_mut(&payload.index);
        match (state, payload.delta) {
            (Some(BlockState::Text), ContentBlockDeltaInner::TextDelta { text }) => {
                vec![ChatEvent::ContentDelta { text }]
            }
            (
                Some(BlockState::ToolUse { id, partial_json }),
                ContentBlockDeltaInner::InputJsonDelta {
                    partial_json: chunk,
                },
            ) => {
                partial_json.push_str(&chunk);
                vec![ChatEvent::ToolUseDelta {
                    id: id.clone(),
                    partial_json: chunk,
                }]
            }
            // Mismatched delta kinds, unknown blocks, or out-of-order
            // deltas are silently ignored — Anthropic occasionally
            // emits new shapes (e.g. thinking deltas) we should not
            // crash on.
            _ => Vec::new(),
        }
    }

    fn on_content_block_stop(&mut self, data: &str) -> Vec<ChatEvent> {
        let payload: ContentBlockStopPayload = match serde_json::from_str(data) {
            Ok(p) => p,
            Err(e) => return vec![self.fatal(format!("decode content_block_stop: {e}"))],
        };
        match self.blocks.remove(&payload.index) {
            Some(BlockState::ToolUse { id, partial_json }) => {
                let input = if partial_json.is_empty() {
                    serde_json::Value::Object(serde_json::Map::new())
                } else {
                    match serde_json::from_str::<serde_json::Value>(&partial_json) {
                        Ok(v) => v,
                        Err(_) => serde_json::Value::String(partial_json),
                    }
                };
                vec![ChatEvent::ToolUseStop { id, input }]
            }
            // Closing a text or unknown block produces no event.
            _ => Vec::new(),
        }
    }

    fn on_message_delta(&mut self, data: &str) {
        match serde_json::from_str::<MessageDeltaPayload>(data) {
            Ok(p) => {
                if let Some(reason) = p.delta.stop_reason {
                    self.stop_reason = Some(map_stop_reason(&reason));
                }
                if let Some(u) = p.usage {
                    self.usage = Some(merge_usage(self.usage, u));
                }
            }
            Err(_) => {
                // A malformed message_delta is non-fatal — we still
                // expect a message_stop afterwards.
            }
        }
    }
}

fn merge_usage(existing: Option<Usage>, incoming: UsagePayload) -> Usage {
    let base = existing.unwrap_or_default();
    Usage {
        // input_tokens / cache fields are reported on message_start
        // and never updated; output_tokens accumulates via
        // message_delta. Take max() so an out-of-order missing field
        // does not regress the count.
        input_tokens: base.input_tokens.max(incoming.input_tokens),
        output_tokens: base.output_tokens.max(incoming.output_tokens),
        cache_read_input_tokens: base
            .cache_read_input_tokens
            .max(incoming.cache_read_input_tokens),
        cache_creation_input_tokens: base
            .cache_creation_input_tokens
            .max(incoming.cache_creation_input_tokens),
    }
}

fn map_stop_reason(s: &str) -> StopReason {
    match s {
        "end_turn" => StopReason::EndTurn,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        "tool_use" => StopReason::ToolUse,
        _ => StopReason::Other,
    }
}

fn parse_error(data: &str) -> String {
    match serde_json::from_str::<crate::wire::ErrorEnvelope>(data) {
        Ok(env) => env.error.message.unwrap_or_else(|| {
            env.error
                .kind
                .unwrap_or_else(|| "unknown error".to_string())
        }),
        Err(_) => {
            if data.is_empty() {
                "unknown error".to_string()
            } else {
                data.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Small helper to drive the translator with a sequence of (event,
    /// data) tuples and collect all emitted ChatEvents.
    fn drive(events: &[(&str, &str)]) -> Vec<ChatEvent> {
        let mut t = Translator::new();
        let mut out = Vec::new();
        for (name, data) in events {
            out.extend(t.handle(name, data));
            if t.terminated {
                break;
            }
        }
        out
    }

    #[test]
    fn streamed_text_concatenates_via_multiple_deltas() {
        let evs = drive(&[
            (
                "message_start",
                r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-x","usage":{"input_tokens":10,"output_tokens":0}}}"#,
            ),
            (
                "content_block_start",
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            ),
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello "}}"#,
            ),
            (
                "content_block_delta",
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"world"}}"#,
            ),
            (
                "content_block_stop",
                r#"{"type":"content_block_stop","index":0}"#,
            ),
            (
                "message_delta",
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":5}}"#,
            ),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ]);
        assert_eq!(
            evs,
            vec![
                ChatEvent::MessageStart { id: "msg_1".into() },
                ChatEvent::ContentDelta { text: "hello ".into() },
                ChatEvent::ContentDelta { text: "world".into() },
                ChatEvent::MessageStop {
                    stop_reason: StopReason::EndTurn,
                    usage: Some(Usage {
                        input_tokens: 10,
                        output_tokens: 5,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    }),
                },
            ]
        );
    }

    #[test]
    fn unknown_event_names_are_ignored() {
        let evs = drive(&[
            ("ping", r#"{}"#),
            (
                "message_start",
                r#"{"type":"message_start","message":{"id":"x","usage":{"input_tokens":1}}}"#,
            ),
        ]);
        assert_eq!(evs, vec![ChatEvent::MessageStart { id: "x".into() }]);
    }
}
