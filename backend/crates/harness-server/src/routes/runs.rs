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

fn to_sse_event(sequenced: &SequencedEvent) -> Event {
    let name = event_name(&sequenced.event);
    let data = serde_json::to_string(&sequenced.event)
        .unwrap_or_else(|_| String::from("{\"error\":\"serialise\"}"));
    Event::default()
        .id(sequenced.seq.to_string())
        .event(name)
        .data(data)
}

/// Map `RunEvent` to the stable SSE event name surfaced to clients. The
/// names are also documented in `spec/events.md` (TODO: write that file
/// in a follow-up; the names below are the source of truth for now).
fn event_name(e: &RunEvent) -> &'static str {
    match e {
        RunEvent::RunStart { .. } => "run.start",
        RunEvent::RunEnd { .. } => "run.end",
        RunEvent::ToolStart { .. } => "tool.start",
        RunEvent::ToolStdout { .. } => "tool.stdout",
        RunEvent::ToolStderr { .. } => "tool.stderr",
        RunEvent::ToolFinish { .. } => "tool.finish",
        RunEvent::ToolError { .. } => "tool.error",
        RunEvent::Chat { event } => match event {
            harness_core::ChatEvent::MessageStart { .. } => "chat.message_start",
            harness_core::ChatEvent::ContentDelta { .. } => "chat.content_delta",
            harness_core::ChatEvent::ToolUseStart { .. } => "chat.tool_use_start",
            harness_core::ChatEvent::ToolUseDelta { .. } => "chat.tool_use_delta",
            harness_core::ChatEvent::ToolUseStop { .. } => "chat.tool_use_stop",
            harness_core::ChatEvent::MessageStop { .. } => "chat.message_stop",
            harness_core::ChatEvent::Error { .. } => "chat.error",
        },
    }
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
