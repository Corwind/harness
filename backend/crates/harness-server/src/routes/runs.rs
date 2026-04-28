//! HTTP handlers for `/v1/runs/:run_id/{events,cancel}`.
//!
//! `GET /events` returns an SSE stream that:
//! 1. Replays the buffered prefix from the run registry. If the client
//!    sends `Last-Event-ID`, only events with `seq > id` are replayed.
//! 2. Then forwards the live broadcast tail until the run terminates.
//! 3. Emits the SSE `id:` field carrying the registry's monotonic
//!    sequence number, and the `event:` field carrying a stable name
//!    derived from the `RunEvent` variant.
//!
//! `POST /cancel` fires the run's cancellation token; the orchestrator
//! observes it within ~100ms and emits `RunEnd { status: Cancelled }`.

use std::convert::Infallible;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{header::HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Router,
};
use futures::{stream::BoxStream, Stream, StreamExt};
use harness_core::{ids::RunId, RunEvent};

use crate::{
    error::ApiError,
    runs::{CancelError, SequencedEvent, SubscribeError},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/runs/:run_id/events", get(events))
        .route("/v1/runs/:run_id/cancel", post(cancel))
}

async fn events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let last_event_id: Option<u64> = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let run_id = RunId::from_string(run_id);
    let subscription = state
        .runs
        .subscribe(&run_id, last_event_id)
        .await
        .map_err(|e| match e {
            SubscribeError::NotFound => ApiError::not_found("run not found"),
            SubscribeError::Expired => ApiError::new(
                StatusCode::GONE,
                "run_expired",
                "run has finished and its event log was reaped",
            ),
        })?;

    let stream = build_event_stream(subscription);
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

fn build_event_stream(
    subscription: crate::runs::Subscription,
) -> BoxStream<'static, Result<Event, Infallible>> {
    use async_stream::stream;

    let crate::runs::Subscription {
        backlog,
        live,
        terminal,
    } = subscription;

    let s = stream! {
        for sequenced in backlog {
            yield Ok(to_sse_event(&sequenced));
        }

        if let Some(mut rx) = live {
            // Live tail. Loop until either the broadcast closes or we
            // see a terminal RunEnd. Lag is silently dropped — the
            // client can reconnect with `Last-Event-ID` to recover the
            // missed events from the buffered log.
            loop {
                match rx.recv().await {
                    Ok(sequenced) => {
                        let is_terminal = matches!(sequenced.event, RunEvent::RunEnd { .. });
                        yield Ok(to_sse_event(&sequenced));
                        if is_terminal {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Skip; client should reconnect with
                        // Last-Event-ID for missed prefix.
                        continue;
                    }
                }
            }
        }
        // If the run was already terminated when we subscribed, the
        // backlog already contained the terminal event. `terminal` is
        // unused beyond signalling that; touch it to silence the
        // warning.
        let _ = terminal;
    };
    s.boxed()
}

/// Build the SSE record for a `RunEvent`. Names + data shapes are the
/// canonical wire form documented in `spec/events.md`; we do **not**
/// just `serde_json::to_string(&event)` here because:
///
/// * The internal `#[serde(tag = "type", ...)]` on `RunEvent` and
///   `ChatEvent` adds a `type` field the spec doesn't have.
/// * `RunEvent::Chat { event: ChatEvent }` would nest the inner
///   payload under `data.event.*` instead of carrying its fields at
///   the top level.
///
/// Building the JSON explicitly per variant keeps the wire shape
/// stable independently of how `RunEvent` evolves internally.
fn to_sse_event(sequenced: &SequencedEvent) -> Event {
    let (name, data) = render(&sequenced.event);
    Event::default()
        .id(sequenced.seq.to_string())
        .event(name)
        .data(data)
}

/// Render a `RunEvent` into `(spec-name, single-line-JSON-payload)`.
fn render(event: &RunEvent) -> (&'static str, String) {
    match event {
        RunEvent::RunStart {
            run_id,
            conversation_id,
            started_at,
        } => (
            "run.start",
            serde_json::to_string(&serde_json::json!({
                "run_id": run_id,
                "conversation_id": conversation_id,
                "started_at": started_at,
            }))
            .unwrap_or_else(|_| serialise_fallback()),
        ),
        RunEvent::RunEnd {
            run_id,
            status,
            ended_at,
        } => (
            "run.end",
            serde_json::to_string(&serde_json::json!({
                "run_id": run_id,
                "status": status,
                "ended_at": ended_at,
            }))
            .unwrap_or_else(|_| serialise_fallback()),
        ),
        RunEvent::ToolStart {
            tool_use_id,
            name,
            input,
        } => (
            "tool.start",
            serde_json::to_string(&serde_json::json!({
                "tool_use_id": tool_use_id,
                "name": name,
                "input": input,
            }))
            .unwrap_or_else(|_| serialise_fallback()),
        ),
        RunEvent::ToolStdout { tool_use_id, chunk } => (
            "tool.stdout",
            serde_json::to_string(&serde_json::json!({
                "tool_use_id": tool_use_id,
                "chunk": chunk,
            }))
            .unwrap_or_else(|_| serialise_fallback()),
        ),
        RunEvent::ToolStderr { tool_use_id, chunk } => (
            "tool.stderr",
            serde_json::to_string(&serde_json::json!({
                "tool_use_id": tool_use_id,
                "chunk": chunk,
            }))
            .unwrap_or_else(|_| serialise_fallback()),
        ),
        RunEvent::ToolFinish {
            tool_use_id,
            name,
            output,
        } => (
            "tool.finish",
            serde_json::to_string(&serde_json::json!({
                "tool_use_id": tool_use_id,
                "name": name,
                "output": output,
            }))
            .unwrap_or_else(|_| serialise_fallback()),
        ),
        RunEvent::ToolError {
            tool_use_id,
            name,
            code,
            message,
        } => (
            "tool.error",
            serde_json::to_string(&serde_json::json!({
                "tool_use_id": tool_use_id,
                "name": name,
                "code": code,
                "message": message,
            }))
            .unwrap_or_else(|_| serialise_fallback()),
        ),
        RunEvent::Chat { event } => render_chat(event),
    }
}

fn render_chat(event: &harness_core::ChatEvent) -> (&'static str, String) {
    use harness_core::ChatEvent;
    match event {
        ChatEvent::MessageStart { id } => (
            "message.start",
            serde_json::to_string(&serde_json::json!({ "id": id }))
                .unwrap_or_else(|_| serialise_fallback()),
        ),
        ChatEvent::ContentDelta { text } => (
            "content.delta",
            serde_json::to_string(&serde_json::json!({ "text": text }))
                .unwrap_or_else(|_| serialise_fallback()),
        ),
        ChatEvent::ToolUseStart { id, name } => (
            "tool_use.start",
            serde_json::to_string(&serde_json::json!({ "id": id, "name": name }))
                .unwrap_or_else(|_| serialise_fallback()),
        ),
        ChatEvent::ToolUseDelta { id, partial_json } => (
            "tool_use.delta",
            serde_json::to_string(&serde_json::json!({
                "id": id,
                "partial_json": partial_json,
            }))
            .unwrap_or_else(|_| serialise_fallback()),
        ),
        ChatEvent::ToolUseStop { id, input } => (
            "tool_use.stop",
            serde_json::to_string(&serde_json::json!({ "id": id, "input": input }))
                .unwrap_or_else(|_| serialise_fallback()),
        ),
        ChatEvent::MessageStop { stop_reason, usage } => {
            let mut body = serde_json::json!({ "stop_reason": stop_reason });
            if let Some(u) = usage {
                body["usage"] = serde_json::to_value(u).unwrap_or(serde_json::Value::Null);
            }
            (
                "message.stop",
                serde_json::to_string(&body).unwrap_or_else(|_| serialise_fallback()),
            )
        }
        ChatEvent::Error { message } => (
            "error",
            serde_json::to_string(&serde_json::json!({ "message": message }))
                .unwrap_or_else(|_| serialise_fallback()),
        ),
    }
}

fn serialise_fallback() -> String {
    String::from("{\"error\":\"serialise\"}")
}

async fn cancel(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let run_id = RunId::from_string(run_id);
    state.runs.cancel(&run_id).await.map_err(|e| match e {
        CancelError::NotFound => ApiError::not_found("run not found"),
        CancelError::AlreadyTerminated => ApiError::new(
            StatusCode::CONFLICT,
            "run_terminated",
            "run already terminated",
        ),
        CancelError::Expired => ApiError::new(
            StatusCode::GONE,
            "run_expired",
            "run has finished and its event log was reaped",
        ),
    })?;
    Ok(StatusCode::ACCEPTED)
}
