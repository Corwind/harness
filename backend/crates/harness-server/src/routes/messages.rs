//! HTTP handlers for `/v1/conversations/:id/messages`.
//!
//! Two endpoints:
//! * `GET`  — list persisted messages, optional `after_ordinal` filter
//! * `POST` — append a user turn, mint a `run_id`, kick off the
//!   orchestrator, register the run with the `RunRegistry`, return a
//!   `RunHandle`. The actual streaming happens through
//!   `GET /v1/runs/:run_id/events` (see `routes/runs.rs`).
//!
//! The persistence loop runs as a detached task per run: it consumes the
//! orchestrator's `BoxStream<RunEvent>`, mirrors every event into the
//! registry, and on terminal events persists the assistant + tool messages
//! via `MessageRepo::append`.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use futures::StreamExt;
use harness_core::{
    chat::{ChatRequest, StopReason},
    ids::{ConversationId, RunId},
    message::{ContentBlock, Message, Role},
    provider::ProviderConfig,
    repo::NewMessage,
    tool::{Tool, ToolDefinition, ToolRegistry},
    ChatEvent, RunEvent,
};
use harness_orchestrator::RunOptions;
use serde::Deserialize;

use crate::{
    dto::{MessageDto, MessagesEnvelope, PostMessageRequest, RunHandleDto},
    error::ApiError,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/v1/conversations/:id/messages",
        get(list).post(post_message),
    )
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    after_ordinal: Option<i64>,
}

const DEFAULT_MESSAGE_LIMIT: usize = 200;
const MAX_MESSAGE_LIMIT: usize = 500;

async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<MessagesEnvelope>, ApiError> {
    let conv_id = ConversationId::from_string(id);
    // Surface 404 if the conversation does not exist.
    let _ = state.conversations.get(&conv_id).await?;

    let all = state.messages.list(&conv_id).await?;
    let after = q.after_ordinal.unwrap_or(-1);
    let limit = q
        .limit
        .map(|l| l.clamp(1, MAX_MESSAGE_LIMIT))
        .unwrap_or(DEFAULT_MESSAGE_LIMIT);
    let filtered: Vec<MessageDto> = all
        .into_iter()
        .filter(|m| m.ordinal > after)
        .take(limit)
        .map(MessageDto::from)
        .collect();

    Ok(Json(MessagesEnvelope { messages: filtered }))
}

async fn post_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PostMessageRequest>,
) -> Result<(StatusCode, Json<RunHandleDto>), ApiError> {
    if body.content.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "validation",
            "content must not be empty",
        ));
    }

    let conv_id = ConversationId::from_string(id);
    let conversation = state.conversations.get(&conv_id).await?;

    // Decode the wire-format content blocks into the domain enum so
    // we can both persist them and feed them to the chat request.
    let user_blocks: Vec<ContentBlock> = body
        .content
        .into_iter()
        .map(|v| {
            serde_json::from_value::<ContentBlock>(v).map_err(|e| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "validation",
                    format!("invalid content block: {e}"),
                )
            })
        })
        .collect::<Result<_, _>>()?;

    // Persist the user message first. If we fail later on, the user's
    // turn is at least durable.
    let user_msg = state
        .messages
        .append(NewMessage {
            conversation_id: conv_id.clone(),
            role: Role::User,
            content: user_blocks.clone(),
        })
        .await?;

    // Build the orchestrator's input from the full conversation history.
    let history = state.messages.list(&conv_id).await?;
    let domain_messages: Vec<Message> = history
        .into_iter()
        .map(|m| Message {
            role: m.role,
            content: m.content,
        })
        .collect();

    // Resolve the provider + its config.
    let provider = state
        .providers
        .get(&conversation.provider_id)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "provider '{}' is not registered",
                conversation.provider_id
            ))
        })?;
    let config_value = state
        .providers_config
        .get(&conversation.provider_id)
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::CONFLICT,
                "provider.unconfigured",
                format!(
                    "provider '{}' has no stored configuration",
                    conversation.provider_id
                ),
            )
        })?;
    let provider_config = ProviderConfig {
        provider_id: conversation.provider_id.clone(),
        config: config_value,
    };

    // Resolve the optional sandbox template.
    let sandbox_template = match conversation.sandbox_template_id.as_ref() {
        Some(tpl_id) => Some(state.sandbox_templates.get(tpl_id).await?),
        None => None,
    };

    // Build tool definitions from the registry. Empty when no tools
    // are registered (T1.E ships only `echo`).
    let tools: Vec<ToolDefinition> = collect_tool_definitions(&*state.tools);

    let request = ChatRequest {
        model: conversation.model.clone(),
        messages: domain_messages,
        system: None,
        tools,
        max_tokens: body.max_tokens,
        temperature: body.temperature,
    };

    let run_id = RunId::generate();
    let cancel = state.runs.register(&run_id).await;

    // Kick off the orchestrator and the per-run mirror task.
    let opts = RunOptions {
        run_id: run_id.clone(),
        conversation_id: conv_id.clone(),
        sandbox_template,
        provider_config,
        request,
    };
    let stream = Arc::clone(&state.orchestrator).run(opts, cancel);

    let registry = state.runs.clone();
    let messages_repo = state.messages.clone();
    let conv_id_for_task = conv_id.clone();
    let _ = provider; // silence "unused" — we hold a clone in the orchestrator.
    tokio::spawn(async move {
        run_mirror_task(stream, registry, messages_repo, conv_id_for_task).await;
    });

    Ok((
        StatusCode::ACCEPTED,
        Json(RunHandleDto {
            run_id: run_id.into_string(),
            conversation_id: conv_id.into_string(),
            message_id: user_msg.id.into_string(),
        }),
    ))
}

/// Collect tool definitions from the registry. The registry exposes
/// names; we look each one up to read its input schema and description.
fn collect_tool_definitions(registry: &dyn ToolRegistry) -> Vec<ToolDefinition> {
    registry
        .names()
        .into_iter()
        .filter_map(|name| {
            let tool = registry.get(&name)?;
            let (description, schema) = match &tool {
                Tool::External(t) => (t.description().map(str::to_owned), t.input_schema().clone()),
                Tool::InProcess(t) => {
                    (t.description().map(str::to_owned), t.input_schema().clone())
                }
            };
            Some(ToolDefinition {
                name: tool.name().to_owned(),
                description,
                input_schema: schema,
            })
        })
        .collect()
}

/// Per-run mirror: forwards every event into the run registry and
/// persists assistant + tool messages on terminal markers.
///
/// Persistence rules:
/// * Each `Chat::MessageStop` reaches the end of one assistant turn —
///   persist whatever text + tool-use blocks we accumulated.
/// * Each tool finish/error produces a `Tool` message containing a
///   single `ToolResult` block. The orchestrator already feeds those
///   results back into its own message vector when looping; we mirror
///   them into storage so reload-after-restart sees the same history.
async fn run_mirror_task(
    mut stream: futures::stream::BoxStream<'static, RunEvent>,
    registry: Arc<crate::runs::RunRegistry>,
    messages_repo: Arc<dyn harness_core::MessageRepo>,
    conversation_id: ConversationId,
) {
    let mut current_text = String::new();
    let mut current_tool_uses: Vec<ContentBlock> = Vec::new();
    let mut last_run_id: Option<RunId> = None;

    while let Some(event) = stream.next().await {
        // Always mirror to the registry first.
        if let RunEvent::RunStart { run_id, .. } = &event {
            last_run_id = Some(run_id.clone());
        }
        let mirror_id = match &last_run_id {
            Some(id) => id.clone(),
            None => continue,
        };
        registry.append(&mirror_id, event.clone()).await;

        match event {
            RunEvent::Chat { event: chat } => match chat {
                ChatEvent::ContentDelta { text } => {
                    current_text.push_str(&text);
                }
                ChatEvent::ToolUseStop { id, input } => {
                    // We do not have the name on `ToolUseStop`, but the
                    // orchestrator paired it with a prior `ToolUseStart`.
                    // The MessageStop handler below assembles the
                    // assistant message; tool-use blocks need the name.
                    // For a minimal v1 we fall back to "tool" — later
                    // revisions of the orchestrator should expose the
                    // matched name on ToolUseStop.
                    current_tool_uses.push(ContentBlock::ToolUse {
                        id,
                        name: "tool".to_owned(),
                        input,
                    });
                }
                ChatEvent::MessageStop { stop_reason, .. } => {
                    let mut blocks: Vec<ContentBlock> = Vec::new();
                    if !current_text.is_empty() {
                        blocks.push(ContentBlock::Text {
                            text: std::mem::take(&mut current_text),
                        });
                    }
                    blocks.append(&mut current_tool_uses);
                    if !blocks.is_empty() {
                        let _ = messages_repo
                            .append(NewMessage {
                                conversation_id: conversation_id.clone(),
                                role: Role::Assistant,
                                content: blocks,
                            })
                            .await;
                    }
                    if matches!(stop_reason, StopReason::ToolUse) {
                        // The orchestrator will keep streaming
                        // (executes tools, re-invokes chat). The next
                        // MessageStop will yield the next assistant
                        // turn.
                    }
                }
                _ => {}
            },
            RunEvent::ToolFinish {
                tool_use_id,
                output,
                ..
            } => {
                let _ = messages_repo
                    .append(NewMessage {
                        conversation_id: conversation_id.clone(),
                        role: Role::Tool,
                        content: vec![ContentBlock::ToolResult {
                            tool_use_id,
                            is_error: false,
                            content: output,
                        }],
                    })
                    .await;
            }
            RunEvent::ToolError {
                tool_use_id,
                code,
                message,
                ..
            } => {
                let _ = messages_repo
                    .append(NewMessage {
                        conversation_id: conversation_id.clone(),
                        role: Role::Tool,
                        content: vec![ContentBlock::ToolResult {
                            tool_use_id,
                            is_error: true,
                            content: serde_json::json!({
                                "error": { "code": code, "message": message }
                            }),
                        }],
                    })
                    .await;
            }
            RunEvent::RunEnd { .. } => break,
            _ => {}
        }
    }
}
