//! `harness-orchestrator` — application layer: drives a conversation turn.
//!
//! Loops `LlmProvider::chat()` ↔ tool execution and emits a unified
//! [`RunEvent`] stream. Owns cancellation semantics and routes every
//! `ExternalTool` invocation through the injected [`SandboxRunner`]
//! port.
//!
//! Hexagonal: this crate depends on `harness-core` ports ONLY. The
//! composition root (`harness-server`) injects concrete adapters.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, rust_2018_idioms)]

use std::sync::Arc;

use futures::{stream::BoxStream, StreamExt};
use harness_core::{
    ChatEvent, ChatRequest, ContentBlock, ConversationId, LlmProvider, Message, ProviderConfig,
    Role, RunEvent, RunId, RunStatus, SandboxRunner, SandboxTemplate, StopReason, Tool, ToolError,
    ToolRegistry,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

mod tool_exec;

/// One-shot input to [`Orchestrator::run`].
#[derive(Debug)]
pub struct RunOptions {
    /// Identifier the server already minted for this run.
    pub run_id: RunId,
    /// The conversation this run belongs to (echoed in `RunStart`).
    pub conversation_id: ConversationId,
    /// The sandbox template attached to the conversation, if any.
    /// `None` enables the fail-closed path: any `ExternalTool` call will
    /// produce `ToolError::NoSandbox` and never spawn a process.
    pub sandbox_template: Option<SandboxTemplate>,
    /// Provider configuration to pass to `LlmProvider::chat`.
    pub provider_config: ProviderConfig,
    /// The initial chat request. The orchestrator owns the message
    /// vector from here on: when the model emits tool calls, the loop
    /// appends the assistant message + tool results and re-invokes
    /// `chat`.
    pub request: ChatRequest,
}

/// The conversation runner. Stateless; one instance can drive many
/// runs concurrently.
pub struct Orchestrator {
    provider: Arc<dyn LlmProvider>,
    sandbox: Arc<dyn SandboxRunner>,
    tools: Arc<dyn ToolRegistry>,
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator")
            .field("provider", &self.provider.id())
            .field("tools", &self.tools.names())
            .finish()
    }
}

impl Orchestrator {
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        sandbox: Arc<dyn SandboxRunner>,
        tools: Arc<dyn ToolRegistry>,
    ) -> Self {
        Self {
            provider,
            sandbox,
            tools,
        }
    }

    /// Drive a run to completion. Returns a stream of [`RunEvent`]s.
    ///
    /// The stream is guaranteed to start with `RunStart` and end with
    /// exactly one `RunEnd`. Once `RunEnd` has been observed the stream
    /// terminates. When `cancel` fires the run terminates within ~100ms
    /// with `RunEnd { status: Cancelled }`.
    pub fn run(
        self: Arc<Self>,
        opts: RunOptions,
        cancel: CancellationToken,
    ) -> BoxStream<'static, RunEvent> {
        let (tx, rx) = mpsc::channel::<RunEvent>(64);
        tokio::spawn(async move {
            self.drive(opts, cancel, tx).await;
        });
        Box::pin(tokio_stream_recv(rx))
    }

    async fn drive(
        self: Arc<Self>,
        opts: RunOptions,
        cancel: CancellationToken,
        tx: mpsc::Sender<RunEvent>,
    ) {
        let RunOptions {
            run_id,
            conversation_id,
            sandbox_template,
            provider_config,
            mut request,
        } = opts;

        if tx
            .send(RunEvent::RunStart {
                run_id: run_id.clone(),
                conversation_id: conversation_id.clone(),
            })
            .await
            .is_err()
        {
            return;
        }

        let status = self
            .conversation_loop(
                &mut request,
                &provider_config,
                sandbox_template.as_ref(),
                &cancel,
                &tx,
            )
            .await;

        let _ = tx.send(RunEvent::RunEnd { status }).await;
    }

    /// Inner loop. Returns the terminal status.
    async fn conversation_loop(
        &self,
        request: &mut ChatRequest,
        provider_config: &ProviderConfig,
        sandbox_template: Option<&SandboxTemplate>,
        cancel: &CancellationToken,
        tx: &mpsc::Sender<RunEvent>,
    ) -> RunStatus {
        loop {
            if cancel.is_cancelled() {
                return RunStatus::Cancelled;
            }

            let stream = match self
                .provider
                .chat(provider_config, request.clone())
                .await
            {
                Ok(s) => s,
                Err(err) => {
                    let _ = tx
                        .send(RunEvent::Chat {
                            event: ChatEvent::Error {
                                message: err.to_string(),
                            },
                        })
                        .await;
                    return RunStatus::Failed;
                }
            };

            let outcome = match self.consume_chat(stream, cancel, tx).await {
                Ok(o) => o,
                Err(ChatLoopAbort::Cancelled) => return RunStatus::Cancelled,
                Err(ChatLoopAbort::ChannelClosed) => return RunStatus::Failed,
            };

            // Re-build the assistant message from what we just streamed
            // so it can be appended to history before tool execution.
            let assistant_msg = Message {
                role: Role::Assistant,
                content: outcome.assistant_blocks.clone(),
            };

            match outcome.stop {
                StopOutcome::EndTurnLike(reason) => {
                    let _ = reason; // reason already streamed inside ChatEvent
                    return RunStatus::Completed;
                }
                StopOutcome::ToolUse { calls } => {
                    request.messages.push(assistant_msg);

                    let mut results: Vec<ContentBlock> = Vec::with_capacity(calls.len());
                    for call in &calls {
                        if cancel.is_cancelled() {
                            return RunStatus::Cancelled;
                        }
                        let result_block = self
                            .execute_one_tool(call, sandbox_template, cancel, tx)
                            .await;
                        results.push(result_block);
                    }
                    request.messages.push(Message {
                        role: Role::Tool,
                        content: results,
                    });
                    // loop and re-invoke chat
                }
                StopOutcome::Error => return RunStatus::Failed,
                StopOutcome::Truncated => {
                    // provider stream ended without MessageStop nor Error
                    return RunStatus::Failed;
                }
            }
        }
    }

    async fn consume_chat(
        &self,
        mut stream: BoxStream<'static, ChatEvent>,
        cancel: &CancellationToken,
        tx: &mpsc::Sender<RunEvent>,
    ) -> Result<ChatTurnOutcome, ChatLoopAbort> {
        let mut assistant_blocks: Vec<ContentBlock> = Vec::new();
        let mut current_text = String::new();
        let mut pending_tools: std::collections::BTreeMap<String, PendingToolUse> =
            Default::default();

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    return Err(ChatLoopAbort::Cancelled);
                }
                next = stream.next() => {
                    let Some(event) = next else {
                        // Stream ended before any terminal event.
                        if !current_text.is_empty() {
                            assistant_blocks.push(ContentBlock::Text { text: std::mem::take(&mut current_text) });
                        }
                        return Ok(ChatTurnOutcome {
                            assistant_blocks,
                            stop: StopOutcome::Truncated,
                        });
                    };

                    // Forward the event verbatim to the run stream.
                    if tx.send(RunEvent::Chat { event: event.clone() }).await.is_err() {
                        return Err(ChatLoopAbort::ChannelClosed);
                    }

                    match event {
                        ChatEvent::MessageStart { .. } => {}
                        ChatEvent::ContentDelta { text } => {
                            current_text.push_str(&text);
                        }
                        ChatEvent::ToolUseStart { id, name } => {
                            // Flush any text accumulated before the tool call.
                            if !current_text.is_empty() {
                                assistant_blocks.push(ContentBlock::Text {
                                    text: std::mem::take(&mut current_text),
                                });
                            }
                            pending_tools.insert(
                                id.clone(),
                                PendingToolUse { name, partial_json: String::new(), input: None },
                            );
                        }
                        ChatEvent::ToolUseDelta { id, partial_json } => {
                            if let Some(p) = pending_tools.get_mut(&id) {
                                p.partial_json.push_str(&partial_json);
                            }
                        }
                        ChatEvent::ToolUseStop { id, input } => {
                            if let Some(p) = pending_tools.get_mut(&id) {
                                p.input = Some(input);
                            }
                        }
                        ChatEvent::MessageStop { stop_reason, .. } => {
                            if !current_text.is_empty() {
                                assistant_blocks.push(ContentBlock::Text {
                                    text: std::mem::take(&mut current_text),
                                });
                            }
                            // Promote pending tool uses to content blocks
                            // in deterministic order (BTreeMap iteration).
                            let mut tool_calls: Vec<PendingToolCall> = Vec::new();
                            for (id, p) in pending_tools.into_iter() {
                                let input = match p.input {
                                    Some(v) => v,
                                    None => {
                                        // Try to parse the accumulated partial JSON
                                        match serde_json::from_str(&p.partial_json) {
                                            Ok(v) => v,
                                            Err(_) => serde_json::Value::Null,
                                        }
                                    }
                                };
                                assistant_blocks.push(ContentBlock::ToolUse {
                                    id: id.clone(),
                                    name: p.name.clone(),
                                    input: input.clone(),
                                });
                                tool_calls.push(PendingToolCall { id, name: p.name, input });
                            }

                            return Ok(ChatTurnOutcome {
                                assistant_blocks,
                                stop: match stop_reason {
                                    StopReason::ToolUse if !tool_calls.is_empty() => {
                                        StopOutcome::ToolUse { calls: tool_calls }
                                    }
                                    StopReason::ToolUse => StopOutcome::EndTurnLike(stop_reason),
                                    other => StopOutcome::EndTurnLike(other),
                                },
                            });
                        }
                        ChatEvent::Error { .. } => {
                            return Ok(ChatTurnOutcome {
                                assistant_blocks,
                                stop: StopOutcome::Error,
                            });
                        }
                    }
                }
            }
        }
    }

    async fn execute_one_tool(
        &self,
        call: &PendingToolCall,
        sandbox_template: Option<&SandboxTemplate>,
        cancel: &CancellationToken,
        tx: &mpsc::Sender<RunEvent>,
    ) -> ContentBlock {
        let _ = tx
            .send(RunEvent::ToolStart {
                tool_use_id: call.id.clone(),
                name: call.name.clone(),
                input: call.input.clone(),
            })
            .await;

        let Some(tool) = self.tools.get(&call.name) else {
            return emit_tool_failure(
                tx,
                &call.id,
                &call.name,
                "tool.unknown",
                &format!("tool '{}' is not registered", call.name),
            )
            .await;
        };

        match tool {
            Tool::InProcess(t) => match t.run(&call.input).await {
                Ok(value) => {
                    let _ = tx
                        .send(RunEvent::ToolFinish {
                            tool_use_id: call.id.clone(),
                            name: call.name.clone(),
                            output: value.clone(),
                        })
                        .await;
                    ContentBlock::ToolResult {
                        tool_use_id: call.id.clone(),
                        is_error: false,
                        content: value,
                    }
                }
                Err(err) => emit_tool_error(tx, &call.id, &call.name, &err).await,
            },
            Tool::External(t) => {
                let template = match sandbox_template {
                    Some(t) => t,
                    None => {
                        let err = ToolError::NoSandbox {
                            tool: call.name.clone(),
                        };
                        return emit_tool_error(tx, &call.id, &call.name, &err).await;
                    }
                };

                let cmd = match t.command(&call.input).await {
                    Ok(c) => c,
                    Err(err) => return emit_tool_error(tx, &call.id, &call.name, &err).await,
                };

                let wrapped = match self.sandbox.wrap(template, cmd).await {
                    Ok(w) => w,
                    Err(err) => {
                        let mapped = ToolError::Sandbox {
                            tool: call.name.clone(),
                            source: err,
                        };
                        return emit_tool_error(tx, &call.id, &call.name, &mapped).await;
                    }
                };

                tool_exec::run_external(&call.id, &call.name, wrapped, cancel, tx).await
            }
        }
    }
}

#[derive(Debug)]
struct ChatTurnOutcome {
    assistant_blocks: Vec<ContentBlock>,
    stop: StopOutcome,
}

#[derive(Debug)]
enum StopOutcome {
    EndTurnLike(StopReason),
    ToolUse { calls: Vec<PendingToolCall> },
    Error,
    Truncated,
}

#[derive(Debug)]
struct PendingToolUse {
    name: String,
    partial_json: String,
    input: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct PendingToolCall {
    id: String,
    name: String,
    input: serde_json::Value,
}

#[derive(Debug)]
enum ChatLoopAbort {
    Cancelled,
    ChannelClosed,
}

async fn emit_tool_error(
    tx: &mpsc::Sender<RunEvent>,
    id: &str,
    name: &str,
    err: &ToolError,
) -> ContentBlock {
    let code = tool_error_code(err);
    emit_tool_failure(tx, id, name, code, &err.to_string()).await
}

pub(crate) async fn emit_tool_failure(
    tx: &mpsc::Sender<RunEvent>,
    id: &str,
    name: &str,
    code: &str,
    message: &str,
) -> ContentBlock {
    let _ = tx
        .send(RunEvent::ToolError {
            tool_use_id: id.to_string(),
            name: name.to_string(),
            code: code.to_string(),
            message: message.to_string(),
        })
        .await;
    ContentBlock::ToolResult {
        tool_use_id: id.to_string(),
        is_error: true,
        content: serde_json::json!({ "error": { "code": code, "message": message } }),
    }
}

fn tool_error_code(err: &ToolError) -> &'static str {
    match err {
        ToolError::NoSandbox { .. } => "tool.no_sandbox",
        ToolError::InvalidInput { .. } => "tool.invalid_input",
        ToolError::NonZeroExit { .. } => "tool.non_zero_exit",
        ToolError::Cancelled { .. } => "tool.cancelled",
        ToolError::Sandbox { .. } => "tool.sandbox",
        ToolError::Other { .. } => "tool.other",
    }
}

fn tokio_stream_recv(rx: mpsc::Receiver<RunEvent>) -> impl futures::Stream<Item = RunEvent> {
    futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| (event, rx))
    })
}
