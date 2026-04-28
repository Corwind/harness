//! Behavior tests for `harness-orchestrator`.
//!
//! These tests use only inline fakes (in `common/`) — they never reach
//! into `harness-storage`, `harness-sandbox`, or `harness-providers-*`.
//! The contract being verified is the one declared in `harness-core`'s
//! ports.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use harness_core::{
    ChatEvent, ConversationId, RunEvent, RunId, RunStatus, StopReason, ToolRegistry,
};
use harness_orchestrator::{Orchestrator, RunOptions};
use pretty_assertions::assert_eq;
use tokio_util::sync::CancellationToken;

mod common;
use common::*;

/// Drain the run stream into a Vec for ergonomic assertions.
async fn collect_events(
    mut stream: futures::stream::BoxStream<'static, RunEvent>,
) -> Vec<RunEvent> {
    let mut out = Vec::new();
    while let Some(ev) = stream.next().await {
        out.push(ev);
    }
    out
}

fn first_run_status(events: &[RunEvent]) -> Option<RunStatus> {
    events.iter().rev().find_map(|e| match e {
        RunEvent::RunEnd { status } => Some(*status),
        _ => None,
    })
}

#[tokio::test]
async fn happy_path_no_tools_emits_run_start_chat_run_end() {
    let provider = Arc::new(FakeProvider::new(vec![vec![
        ChatEvent::MessageStart {
            id: "m1".to_string(),
        },
        ChatEvent::ContentDelta {
            text: "Hello".to_string(),
        },
        ChatEvent::ContentDelta {
            text: ", world".to_string(),
        },
        ChatEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
            usage: None,
        },
    ]]));
    let sandbox = Arc::new(RecordingSandbox::default());
    let registry: Arc<dyn ToolRegistry> = Arc::new(StaticRegistry::new());
    let orch = Arc::new(Orchestrator::new(
        provider.clone(),
        sandbox.clone(),
        registry,
    ));

    let opts = RunOptions {
        run_id: RunId::from_string("run-1"),
        conversation_id: ConversationId::from_string("conv-1"),
        sandbox_template: None,
        provider_config: fake_provider_config(),
        request: empty_request("test-model"),
    };

    let events = collect_events(orch.run(opts, CancellationToken::new())).await;

    // RunStart first, RunEnd last with status Completed.
    assert!(matches!(events.first(), Some(RunEvent::RunStart { .. })));
    assert_eq!(first_run_status(&events), Some(RunStatus::Completed));

    // Provider events were forwarded inside `Chat` events.
    let chat_events: Vec<&ChatEvent> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Chat { event } => Some(event),
            _ => None,
        })
        .collect();
    assert_eq!(chat_events.len(), 4);
    assert!(matches!(chat_events[0], ChatEvent::MessageStart { .. }));
    assert!(matches!(chat_events[3], ChatEvent::MessageStop { .. }));

    // Sandbox runner was never invoked (no tools).
    assert!(sandbox.wrap_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn tool_round_trip_provider_then_tool_then_provider_then_done() {
    // First chat call: model emits a single tool_use(echo, {"text":"hi"}).
    // Second chat call (after we feed tool results back): model wraps up.
    let provider = Arc::new(FakeProvider::new(vec![
        vec![
            ChatEvent::MessageStart {
                id: "m1".to_string(),
            },
            ChatEvent::ToolUseStart {
                id: "tu_1".to_string(),
                name: "echo".to_string(),
            },
            ChatEvent::ToolUseStop {
                id: "tu_1".to_string(),
                input: serde_json::json!({"text": "hi"}),
            },
            ChatEvent::MessageStop {
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
        ],
        vec![
            ChatEvent::MessageStart {
                id: "m2".to_string(),
            },
            ChatEvent::ContentDelta {
                text: "Done.".to_string(),
            },
            ChatEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ],
    ]));

    let sandbox = Arc::new(RecordingSandbox::default());
    let registry: Arc<dyn ToolRegistry> = Arc::new(
        StaticRegistry::new().with_external(Arc::new(EchoCmd::default())),
    );

    let orch = Arc::new(Orchestrator::new(
        provider.clone(),
        sandbox.clone(),
        registry,
    ));

    let opts = RunOptions {
        run_id: RunId::from_string("run-2"),
        conversation_id: ConversationId::from_string("conv-2"),
        sandbox_template: Some(fake_template("tpl-1")),
        provider_config: fake_provider_config(),
        request: empty_request("test-model"),
    };

    let events = collect_events(orch.run(opts, CancellationToken::new())).await;

    assert_eq!(first_run_status(&events), Some(RunStatus::Completed));

    // Tool lifecycle events present in order.
    let tool_starts: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, RunEvent::ToolStart { .. }))
        .collect();
    let tool_finishes: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, RunEvent::ToolFinish { .. }))
        .collect();
    assert_eq!(tool_starts.len(), 1, "one ToolStart expected");
    assert_eq!(tool_finishes.len(), 1, "one ToolFinish expected");

    if let RunEvent::ToolStart {
        tool_use_id, name, ..
    } = tool_starts[0]
    {
        assert_eq!(tool_use_id, "tu_1");
        assert_eq!(name, "echo");
    }

    // Sandbox was actually invoked because a template was attached.
    let wraps = sandbox.wrap_calls.lock().unwrap();
    assert_eq!(wraps.len(), 1);
    assert_eq!(wraps[0].0, "tpl-1");
    assert_eq!(wraps[0].1.program, "/bin/echo");

    // Provider was called twice (initial + post-tool-result).
    assert_eq!(provider.requests.lock().unwrap().len(), 2);
    let second_request = provider.requests.lock().unwrap()[1].clone();
    // The orchestrator must have appended the assistant + tool messages.
    assert_eq!(second_request.messages.len(), 2);
    assert!(matches!(
        second_request.messages[0].role,
        harness_core::Role::Assistant
    ));
    assert!(matches!(
        second_request.messages[1].role,
        harness_core::Role::Tool
    ));
}

#[tokio::test]
async fn cancellation_terminates_within_100ms() {
    // Provider returns a stream that never terminates. The orchestrator
    // should still emit RunEnd { Cancelled } promptly when the token
    // fires.
    use futures::stream;

    struct StallingProvider;
    #[async_trait::async_trait]
    impl harness_core::LlmProvider for StallingProvider {
        fn id(&self) -> &'static str {
            "stalling"
        }
        fn display_name(&self) -> &'static str {
            "Stalling"
        }
        fn capabilities(&self) -> harness_core::ProviderCapabilities {
            Default::default()
        }
        async fn list_models(
            &self,
            _cfg: &harness_core::ProviderConfig,
        ) -> Result<Vec<harness_core::ModelInfo>, harness_core::ProviderError> {
            Ok(vec![])
        }
        async fn chat(
            &self,
            _cfg: &harness_core::ProviderConfig,
            _request: harness_core::ChatRequest,
        ) -> Result<
            futures::stream::BoxStream<'static, ChatEvent>,
            harness_core::ProviderError,
        > {
            // A stream that yields one MessageStart then waits forever.
            let s = stream::once(async {
                ChatEvent::MessageStart {
                    id: "m1".to_string(),
                }
            })
            .chain(stream::pending());
            Ok(Box::pin(s))
        }
    }

    let provider: Arc<dyn harness_core::LlmProvider> = Arc::new(StallingProvider);
    let sandbox = Arc::new(RecordingSandbox::default());
    let registry: Arc<dyn ToolRegistry> = Arc::new(StaticRegistry::new());
    let orch = Arc::new(Orchestrator::new(provider, sandbox, registry));

    let cancel = CancellationToken::new();
    let opts = RunOptions {
        run_id: RunId::from_string("run-3"),
        conversation_id: ConversationId::from_string("conv-3"),
        sandbox_template: None,
        provider_config: fake_provider_config(),
        request: empty_request("test-model"),
    };

    let cancel_clone = cancel.clone();
    let mut stream = orch.run(opts, cancel);

    // Fire cancellation after a brief delay. Measure end-to-end how
    // long it takes to observe RunEnd.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel_clone.cancel();
    });

    let start = Instant::now();
    let mut last: Option<RunEvent> = None;
    while let Some(ev) = stream.next().await {
        last = Some(ev);
    }
    let elapsed = start.elapsed();

    assert!(matches!(
        last,
        Some(RunEvent::RunEnd {
            status: RunStatus::Cancelled
        })
    ));
    // Cancellation budget: token fires at ~20ms, RunEnd within ~100ms
    // of that. Use 200ms total wall-clock to absorb test scheduling.
    assert!(
        elapsed < Duration::from_millis(200),
        "took {:?} to terminate after cancel",
        elapsed
    );
}

#[tokio::test]
async fn external_tool_without_sandbox_fails_closed_and_never_spawns() {
    let provider = Arc::new(FakeProvider::new(vec![vec![
        ChatEvent::MessageStart {
            id: "m1".to_string(),
        },
        ChatEvent::ToolUseStart {
            id: "tu_1".to_string(),
            name: "never".to_string(),
        },
        ChatEvent::ToolUseStop {
            id: "tu_1".to_string(),
            input: serde_json::json!({}),
        },
        ChatEvent::MessageStop {
            stop_reason: StopReason::ToolUse,
            usage: None,
        },
    ]]));

    // ForbiddenSandbox panics on any wrap/validate call. If the
    // fail-closed path is honored, the orchestrator must NOT call it.
    let sandbox = Arc::new(ForbiddenSandbox);
    let never_tool = Arc::new(NeverCalledTool::new());
    let command_flag = never_tool.command_called.clone();

    let registry: Arc<dyn ToolRegistry> = Arc::new(
        StaticRegistry::new().with_external(never_tool.clone() as Arc<dyn harness_core::ExternalTool>),
    );

    let orch = Arc::new(Orchestrator::new(provider.clone(), sandbox, registry));

    let opts = RunOptions {
        run_id: RunId::from_string("run-4"),
        conversation_id: ConversationId::from_string("conv-4"),
        sandbox_template: None, // <-- no sandbox attached → fail-closed
        provider_config: fake_provider_config(),
        request: empty_request("test-model"),
    };

    let events = collect_events(orch.run(opts, CancellationToken::new())).await;

    // The orchestrator emitted a ToolError with the no-sandbox code.
    let saw_no_sandbox = events.iter().any(|e| matches!(e,
        RunEvent::ToolError { code, name, .. } if code == "tool.no_sandbox" && name == "never"));
    assert!(saw_no_sandbox, "expected tool.no_sandbox error event");

    // ExternalTool::command() was never called (we never even ask the
    // tool for its command line if there's no sandbox).
    assert!(
        !command_flag.load(std::sync::atomic::Ordering::SeqCst),
        "ExternalTool::command must not be invoked when no sandbox is attached"
    );

    // Run still terminates cleanly: orchestrator should send the tool
    // error back to the provider as a ToolResult and let the model
    // either continue or stop. Status: Completed (model stops naturally
    // in the second turn — but here the second turn would error since
    // the provider has only one script. We still want a terminal RunEnd
    // — Failed is acceptable since provider has no second script.)
    assert!(events
        .iter()
        .any(|e| matches!(e, RunEvent::RunEnd { .. })));
}

#[tokio::test]
async fn sandbox_runner_invoked_when_template_attached() {
    // Already covered partially in the round-trip test, but assert
    // explicitly that wrap() received the right template id.
    let provider = Arc::new(FakeProvider::new(vec![
        vec![
            ChatEvent::ToolUseStart {
                id: "tu_1".to_string(),
                name: "echo".to_string(),
            },
            ChatEvent::ToolUseStop {
                id: "tu_1".to_string(),
                input: serde_json::json!({"text": "hello"}),
            },
            ChatEvent::MessageStop {
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
        ],
        vec![ChatEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
            usage: None,
        }],
    ]));
    let sandbox = Arc::new(RecordingSandbox::default());
    let registry: Arc<dyn ToolRegistry> = Arc::new(
        StaticRegistry::new().with_external(Arc::new(EchoCmd::default())),
    );

    let orch = Arc::new(Orchestrator::new(
        provider.clone(),
        sandbox.clone(),
        registry,
    ));

    let opts = RunOptions {
        run_id: RunId::from_string("run-5"),
        conversation_id: ConversationId::from_string("conv-5"),
        sandbox_template: Some(fake_template("strict-readonly")),
        provider_config: fake_provider_config(),
        request: empty_request("test-model"),
    };

    let _ = collect_events(orch.run(opts, CancellationToken::new())).await;

    let wraps = sandbox.wrap_calls.lock().unwrap();
    assert_eq!(wraps.len(), 1, "wrap must be called exactly once");
    assert_eq!(wraps[0].0, "strict-readonly");
}

#[tokio::test]
async fn unknown_tool_emits_tool_unknown_and_run_continues() {
    let provider = Arc::new(FakeProvider::new(vec![
        vec![
            ChatEvent::ToolUseStart {
                id: "tu_x".to_string(),
                name: "does_not_exist".to_string(),
            },
            ChatEvent::ToolUseStop {
                id: "tu_x".to_string(),
                input: serde_json::json!({}),
            },
            ChatEvent::MessageStop {
                stop_reason: StopReason::ToolUse,
                usage: None,
            },
        ],
        vec![
            ChatEvent::ContentDelta {
                text: "ack".to_string(),
            },
            ChatEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ],
    ]));
    let sandbox = Arc::new(RecordingSandbox::default());
    let registry: Arc<dyn ToolRegistry> = Arc::new(StaticRegistry::new());
    let orch = Arc::new(Orchestrator::new(provider.clone(), sandbox, registry));

    let opts = RunOptions {
        run_id: RunId::from_string("run-6"),
        conversation_id: ConversationId::from_string("conv-6"),
        sandbox_template: Some(fake_template("tpl-1")),
        provider_config: fake_provider_config(),
        request: empty_request("test-model"),
    };

    let events = collect_events(orch.run(opts, CancellationToken::new())).await;

    let saw_unknown = events.iter().any(|e| matches!(e,
        RunEvent::ToolError { code, .. } if code == "tool.unknown"));
    assert!(saw_unknown, "expected tool.unknown error event");

    // Run continues to completion via the second provider script.
    assert_eq!(first_run_status(&events), Some(RunStatus::Completed));

    // Provider was re-invoked (so the error round-tripped as a tool result).
    assert_eq!(provider.requests.lock().unwrap().len(), 2);
}
