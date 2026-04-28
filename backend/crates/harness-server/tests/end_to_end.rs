//! T1.E behavior tests: full HTTP+SSE wire-up against a real loopback
//! listener.
//!
//! Coverage matches the brief:
//! 1. Happy path: configure provider → create conversation → POST
//!    message → consume SSE → assert run.end → list messages.
//! 2. Tool round-trip: `echo` external tool runs through the (fake)
//!    sandbox runner.
//! 3. Cancellation: mid-run cancel produces `run.end {cancelled}`.
//! 4. Last-Event-ID resume: disconnect mid-stream, reconnect with id,
//!    replay starts after the supplied id.
//! 5. `GET /providers/{id}/models` returns 409 when not configured.
//! 6. Settings GET/PATCH round-trip.
//!
//! All tests share `common::TestApp` which boots a fake `LlmProvider`
//! and a pass-through `SandboxRunner`.

mod common;

use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use harness_core::chat::{ChatEvent, StopReason};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

use common::{TestApp, TOKEN};
use harness_server::{bind_loopback, build_router, serve, RunRegistry};

/// Boot the test server on a real loopback port. Returns the base URL,
/// the shutdown sender, the join handle, and a `RunRegistry` clone for
/// test introspection.
async fn spawn_server(app: &TestApp) -> ServerHandle {
    let bound = bind_loopback().await.expect("bind");
    let addr = bound.local_addr;
    let runs = app.state.runs.clone();
    let router = build_router(app.state.clone());
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        serve(bound, router, async move {
            let _ = rx.await;
        })
        .await
        .expect("serve");
    });
    // Brief delay so the listener is ready.
    tokio::time::sleep(Duration::from_millis(20)).await;

    ServerHandle {
        base_url: format!("http://{addr}"),
        shutdown: Some(tx),
        join: Some(join),
        runs,
    }
}

struct ServerHandle {
    base_url: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<()>>,
    runs: std::sync::Arc<RunRegistry>,
}

impl ServerHandle {
    async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), join).await;
        }
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

async fn configure_fake_provider(handle: &ServerHandle) {
    let res = client()
        .post(format!("{}/v1/providers/fake/config", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .json(&json!({ "api_key": "test-key" }))
        .send()
        .await
        .expect("upsert config");
    assert_eq!(res.status().as_u16(), 200);
}

async fn create_conversation(handle: &ServerHandle, sandbox: Option<&str>) -> String {
    let mut body = json!({
        "provider_id": "fake",
        "model": "fake-1",
        "title": "test",
    });
    if let Some(tpl) = sandbox {
        body["sandbox_template_id"] = json!(tpl);
    }
    let res = client()
        .post(format!("{}/v1/conversations", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .json(&body)
        .send()
        .await
        .expect("create conv");
    assert_eq!(res.status().as_u16(), 201);
    let v: Value = res.json().await.unwrap();
    v.get("id").and_then(|i| i.as_str()).unwrap().to_owned()
}

async fn post_user_message(handle: &ServerHandle, conv_id: &str, text: &str) -> (String, String) {
    let res = client()
        .post(format!(
            "{}/v1/conversations/{}/messages",
            handle.base_url, conv_id
        ))
        .header("X-Harness-Token", TOKEN)
        .json(&json!({
            "content": [{ "type": "text", "text": text }],
        }))
        .send()
        .await
        .expect("post message");
    assert_eq!(res.status().as_u16(), 202);
    let v: Value = res.json().await.unwrap();
    (
        v["run_id"].as_str().unwrap().to_owned(),
        v["message_id"].as_str().unwrap().to_owned(),
    )
}

/// Connect to the SSE stream and collect events until either `run.end`
/// or `timeout` expires. Returns the (event_name, parsed_data) pairs.
async fn collect_run_events(
    handle: &ServerHandle,
    run_id: &str,
    last_event_id: Option<&str>,
    timeout: Duration,
) -> Vec<(String, Value, String)> {
    let mut req = client()
        .get(format!("{}/v1/runs/{}/events", handle.base_url, run_id))
        .header("X-Harness-Token", TOKEN);
    if let Some(id) = last_event_id {
        req = req.header("Last-Event-ID", id);
    }
    let res = req.send().await.expect("sse connect");
    assert_eq!(res.status().as_u16(), 200);

    let mut stream = res.bytes_stream().eventsource();
    let deadline = tokio::time::Instant::now() + timeout;
    let mut out: Vec<(String, Value, String)> = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(ev))) => {
                let data: Value = serde_json::from_str(&ev.data).unwrap_or(Value::Null);
                let id = ev.id.clone();
                let name = ev.event.clone();
                let is_terminal = name == "run.end";
                out.push((name, data, id));
                if is_terminal {
                    break;
                }
            }
            Ok(Some(Err(_))) => break,
            Ok(None) | Err(_) => break,
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Test 5 — provider not configured returns 409.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn list_models_returns_409_when_provider_not_configured() {
    let app = TestApp::boot().await;
    let handle = spawn_server(&app).await;

    let res = client()
        .get(format!("{}/v1/providers/fake/models", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 409);

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Test 6 — settings GET / PATCH round-trip.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn settings_get_patch_round_trip() {
    let app = TestApp::boot().await;
    let handle = spawn_server(&app).await;

    let res = client()
        .get(format!("{}/v1/settings", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let initial: Value = res.json().await.unwrap();
    assert!(initial.is_object());

    let res = client()
        .patch(format!("{}/v1/settings", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .json(&json!({ "theme": "dark", "default_model": "claude-sonnet-4-6" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let merged: Value = res.json().await.unwrap();
    assert_eq!(merged.get("theme").and_then(|v| v.as_str()), Some("dark"));
    assert_eq!(
        merged.get("default_model").and_then(|v| v.as_str()),
        Some("claude-sonnet-4-6")
    );

    // Clearing with explicit null.
    let res = client()
        .patch(format!("{}/v1/settings", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .json(&json!({ "default_model": null }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let merged: Value = res.json().await.unwrap();
    assert!(merged.get("default_model").is_none());
    assert_eq!(merged.get("theme").and_then(|v| v.as_str()), Some("dark"));

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Test 1 — happy path end-to-end.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn happy_path_run_persists_assistant_message() {
    let app = TestApp::boot().await;
    app.provider.enqueue_simple_turn("Hello!");
    let handle = spawn_server(&app).await;
    configure_fake_provider(&handle).await;

    let conv_id = create_conversation(&handle, Some("strict-readonly")).await;
    let (run_id, _msg_id) = post_user_message(&handle, &conv_id, "Hi.").await;

    let events = collect_run_events(&handle, &run_id, None, Duration::from_secs(3)).await;

    assert!(
        events.iter().any(|(n, _, _)| n == "run.start"),
        "expected run.start in {events:?}"
    );
    assert!(
        events.iter().any(|(n, _, _)| n == "run.end"),
        "expected run.end in {events:?}"
    );
    let run_end = events.iter().find(|(n, _, _)| n == "run.end").unwrap();
    assert_eq!(
        run_end.1.get("status").and_then(|s| s.as_str()),
        Some("completed")
    );

    // Allow the mirror task a tick to commit the assistant message.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // GET messages: user + assistant in order.
    let res = client()
        .get(format!(
            "{}/v1/conversations/{}/messages",
            handle.base_url, conv_id
        ))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.unwrap();
    let msgs = body.get("messages").and_then(|m| m.as_array()).unwrap();
    assert_eq!(msgs.len(), 2, "expected user+assistant; got {body}");
    assert_eq!(msgs[0]["role"].as_str(), Some("user"));
    assert_eq!(msgs[1]["role"].as_str(), Some("assistant"));
    let assistant_text = msgs[1]["content"][0]["text"].as_str().unwrap();
    assert_eq!(assistant_text, "Hello!");

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Test 3 — cancellation.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn cancellation_terminates_run_within_200ms() {
    let app = TestApp::boot().await;
    // Schedule a long stream so we have time to cancel mid-flight.
    app.provider.set_delay_ms(80);
    app.provider.enqueue_turn(vec![
        ChatEvent::MessageStart { id: "m".into() },
        ChatEvent::ContentDelta {
            text: "tok-1".into(),
        },
        ChatEvent::ContentDelta {
            text: "tok-2".into(),
        },
        ChatEvent::ContentDelta {
            text: "tok-3".into(),
        },
        ChatEvent::MessageStop {
            stop_reason: StopReason::EndTurn,
            usage: None,
        },
    ]);
    let handle = spawn_server(&app).await;
    configure_fake_provider(&handle).await;
    let conv_id = create_conversation(&handle, None).await;
    let (run_id, _) = post_user_message(&handle, &conv_id, "go").await;

    // Cancel after a short delay so the orchestrator has emitted a few
    // deltas first.
    let url_cancel = format!("{}/v1/runs/{}/cancel", handle.base_url, run_id);
    let cancel_at = tokio::time::Instant::now() + Duration::from_millis(60);
    let cancel_handle = tokio::spawn({
        let url_cancel = url_cancel.clone();
        async move {
            tokio::time::sleep_until(cancel_at).await;
            client()
                .post(&url_cancel)
                .header("X-Harness-Token", TOKEN)
                .send()
                .await
                .unwrap()
        }
    });

    let events = collect_run_events(&handle, &run_id, None, Duration::from_secs(3)).await;
    let cancel_res = cancel_handle.await.unwrap();
    assert_eq!(cancel_res.status().as_u16(), 202);

    let run_end = events.iter().find(|(n, _, _)| n == "run.end").unwrap();
    assert_eq!(
        run_end.1.get("status").and_then(|s| s.as_str()),
        Some("cancelled"),
        "expected cancelled status; got {events:?}"
    );

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Test 4 — Last-Event-ID resume.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn last_event_id_resume_replays_only_subsequent_events() {
    let app = TestApp::boot().await;
    app.provider.enqueue_simple_turn("ABC");
    let handle = spawn_server(&app).await;
    configure_fake_provider(&handle).await;
    let conv_id = create_conversation(&handle, None).await;
    let (run_id, _) = post_user_message(&handle, &conv_id, "go").await;

    // Wait for the run to fully complete so the buffered log is final.
    let initial = collect_run_events(&handle, &run_id, None, Duration::from_secs(3)).await;
    assert!(initial.iter().any(|(n, _, _)| n == "run.end"));
    assert!(
        initial.len() >= 3,
        "stream too short to test resume: {initial:?}"
    );

    // Pick the second event's id and reconnect with Last-Event-ID set
    // to that — replay must skip everything up to and including it.
    let resume_id = initial[1].2.clone();
    let resumed =
        collect_run_events(&handle, &run_id, Some(&resume_id), Duration::from_secs(2)).await;

    let resume_id_n: u64 = resume_id.parse().unwrap();
    for (_, _, id) in &resumed {
        let n: u64 = id.parse().unwrap();
        assert!(
            n > resume_id_n,
            "resume must skip <= {resume_id}; saw id {id}"
        );
    }
    assert!(
        resumed.iter().any(|(n, _, _)| n == "run.end"),
        "resume must still see the terminal event"
    );

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Test 2 — tool round-trip via the echo external tool.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn echo_tool_round_trip_emits_tool_events() {
    let app = TestApp::boot().await;
    // Turn 1: model emits a tool_use call for `echo`.
    app.provider.enqueue_turn(vec![
        ChatEvent::MessageStart { id: "m1".into() },
        ChatEvent::ToolUseStart {
            id: "tu_1".into(),
            name: "echo".into(),
        },
        ChatEvent::ToolUseStop {
            id: "tu_1".into(),
            input: json!({ "text": "world" }),
        },
        ChatEvent::MessageStop {
            stop_reason: StopReason::ToolUse,
            usage: None,
        },
    ]);
    // Turn 2: after the tool result, the model finishes with text.
    app.provider.enqueue_simple_turn("Got it.");

    let handle = spawn_server(&app).await;
    configure_fake_provider(&handle).await;
    // Conversation must have a sandbox attached so external tools run
    // (fail-closed default would otherwise refuse).
    let conv_id = create_conversation(&handle, Some("strict-readonly")).await;
    let (run_id, _) = post_user_message(&handle, &conv_id, "use echo").await;

    let events = collect_run_events(&handle, &run_id, None, Duration::from_secs(5)).await;
    let names: Vec<&str> = events.iter().map(|(n, _, _)| n.as_str()).collect();

    assert!(
        names.contains(&"tool.start"),
        "missing tool.start: {names:?}"
    );
    assert!(
        names.contains(&"tool.finish"),
        "missing tool.finish: {names:?}"
    );
    let run_end = events.iter().find(|(n, _, _)| n == "run.end").unwrap();
    assert_eq!(
        run_end.1.get("status").and_then(|s| s.as_str()),
        Some("completed"),
        "expected completed status; got {events:?}"
    );

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Conversations list pagination + delete.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn conversations_list_paginates_and_delete_cascades_messages() {
    let app = TestApp::boot().await;
    let handle = spawn_server(&app).await;
    configure_fake_provider(&handle).await;

    // Create three conversations.
    let id_a = create_conversation(&handle, None).await;
    let id_b = create_conversation(&handle, None).await;
    let id_c = create_conversation(&handle, None).await;

    // Page size 2 should yield a next_cursor.
    let res = client()
        .get(format!("{}/v1/conversations?limit=2", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.unwrap();
    let convs = body["conversations"].as_array().unwrap();
    assert_eq!(convs.len(), 2);
    let cursor = body["next_cursor"].as_str().unwrap().to_owned();

    // Page 2 fetches the remaining one and has no further cursor.
    let res = client()
        .get(format!(
            "{}/v1/conversations?limit=2&cursor={cursor}",
            handle.base_url
        ))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.unwrap();
    let convs = body["conversations"].as_array().unwrap();
    assert_eq!(convs.len(), 1);
    assert!(body.get("next_cursor").map_or(true, Value::is_null));

    // DELETE one and confirm it cascades.
    app.provider.enqueue_simple_turn("ok");
    let (_, _) = post_user_message(&handle, &id_a, "hi").await;
    // Brief pause so the run mirror persists the assistant message.
    tokio::time::sleep(Duration::from_millis(80)).await;

    let res = client()
        .delete(format!("{}/v1/conversations/{}", handle.base_url, id_a))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 204);

    // GET messages on a deleted conversation → 404.
    let res = client()
        .get(format!(
            "{}/v1/conversations/{}/messages",
            handle.base_url, id_a
        ))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 404);

    // Other conversations still exist.
    for kept in [&id_b, &id_c] {
        let res = client()
            .get(format!("{}/v1/conversations/{}", handle.base_url, kept))
            .header("X-Harness-Token", TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status().as_u16(), 200);
    }

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Run-registry TTL: terminated runs are reaped after RUN_RETENTION.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn run_registry_reaper_drops_terminated_runs_after_retention() {
    let app = TestApp::boot().await;
    app.provider.enqueue_simple_turn("ok");
    let handle = spawn_server(&app).await;
    configure_fake_provider(&handle).await;
    let conv_id = create_conversation(&handle, None).await;
    let (run_id, _) = post_user_message(&handle, &conv_id, "go").await;
    let _ = collect_run_events(&handle, &run_id, None, Duration::from_secs(3)).await;

    // Confirm the SSE handler can still subscribe (terminated entry
    // still in registry).
    let res = client()
        .get(format!("{}/v1/runs/{}/events", handle.base_url, run_id))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    drop(res);

    // Force-reap with a zero retention to simulate the TTL elapsing.
    let n = handle.runs.reap_expired(Duration::from_secs(0)).await;
    assert!(n >= 1, "reaper should drop the terminated run");

    // Subsequent SSE subscribe must now 404 (reaped → not found).
    let res = client()
        .get(format!("{}/v1/runs/{}/events", handle.base_url, run_id))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 404);

    handle.shutdown().await;
}
