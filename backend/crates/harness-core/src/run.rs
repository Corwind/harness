//! Run-event vocabulary — the unified stream the orchestrator emits.
//!
//! `RunEvent` is the cross-crate event type produced by
//! `harness-orchestrator` and consumed by `harness-server` (which fans
//! it out as SSE per `spec/events.md`). It lives in `harness-core` so
//! both crates can name it without depending on each other.
//!
//! The variants are a superset of the provider-level [`crate::ChatEvent`]:
//! they wrap chat deltas plus run-lifecycle events (`RunStart`, `RunEnd`)
//! and tool-execution events (`ToolStart`, `ToolStdout`, `ToolStderr`,
//! `ToolFinish`, `ToolError`). Wire-format mapping is the server's job.

use serde::{Deserialize, Serialize};

use crate::chat::ChatEvent;
use crate::ids::{ConversationId, RunId};

/// Why a run terminated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Provider returned a terminal `MessageStop` with `EndTurn`,
    /// `MaxTokens`, `StopSequence`, or `Other`; no further work pending.
    Completed,
    /// Cancellation token fired before the run completed.
    Cancelled,
    /// Run aborted because of a provider, tool, or sandbox failure that
    /// could not be recovered from.
    Failed,
}

/// One event emitted on the orchestrator's run stream.
///
/// The stream begins with exactly one [`RunEvent::RunStart`] and ends
/// with exactly one [`RunEvent::RunEnd`]; consumers can rely on those
/// invariants for bookkeeping.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    /// First event. Identifies the run and its conversation.
    RunStart {
        run_id: RunId,
        conversation_id: ConversationId,
    },

    /// A provider event passed through verbatim. Includes message
    /// starts/stops, content deltas, and tool-use markers.
    Chat { event: ChatEvent },

    /// The orchestrator is about to invoke a tool the model called for.
    ToolStart {
        tool_use_id: String,
        name: String,
        input: serde_json::Value,
    },

    /// One chunk of stdout captured from a running external tool.
    ToolStdout {
        tool_use_id: String,
        chunk: String,
    },

    /// One chunk of stderr captured from a running external tool.
    ToolStderr {
        tool_use_id: String,
        chunk: String,
    },

    /// A tool invocation completed successfully. `output` is the parsed
    /// result (for in-process tools) or the captured stdout as a JSON
    /// string (for external tools).
    ToolFinish {
        tool_use_id: String,
        name: String,
        output: serde_json::Value,
    },

    /// A tool invocation failed. `code` is a stable, machine-readable
    /// short string (e.g. `"tool.unknown"`, `"tool.no_sandbox"`,
    /// `"tool.invalid_input"`, `"tool.non_zero_exit"`,
    /// `"tool.cancelled"`, `"tool.sandbox"`, `"tool.other"`).
    ToolError {
        tool_use_id: String,
        name: String,
        code: String,
        message: String,
    },

    /// Terminal event for the run.
    RunEnd { status: RunStatus },
}

impl RunEvent {
    /// Convenience constructor mirroring the older direct enum case
    /// when adapters want to forward a `ChatEvent` from a provider.
    pub fn chat(event: ChatEvent) -> Self {
        RunEvent::Chat { event }
    }
}
