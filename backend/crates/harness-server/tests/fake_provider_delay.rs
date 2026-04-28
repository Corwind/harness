//! Behavior tests for the `HARNESS_FAKE_PROVIDER_DELAY_MS` env knob.
//!
//! Cases per the brief:
//! 1. With a non-zero delay, a streamed run takes at least
//!    `delay_ms × event_count` wall-clock — generous lower bound to
//!    survive scheduling jitter.
//! 2. With the env unset (delay = 0), a run completes well under any
//!    plausible delay budget — sanity guard that we haven't accidentally
//!    introduced a sleep on the synchronous path.
//! 3. With delay set, mid-run cancel produces `run.end {Cancelled}`
//!    inside the orchestrator's ~100ms cancellation budget.
//!
//! All tests use `FakeProvider::with_delay_ms` (the explicit-arg
//! constructor) rather than mutating the process env; env mutation in
//! Rust tests is process-wide and racey across `#[tokio::test]`s.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eventsource_stream::Eventsource;
use futures::StreamExt;
use harness_core::{LlmProvider, SandboxRunner, ToolRegistry};
use harness_orchestrator::Orchestrator;
use harness_server::{
    bind_loopback, bootstrap::acquire_app_state_with, build_router, serve, AppState, FakeProvider,
    ProviderRegistry, SessionToken,
};
use harness_storage::{Db, Secret};
use harness_tools::default_registry;
use serde_json::{json, Value};
use tempfile::TempDir;

const TOKEN: &str = "delay-token-32bytes-deadbeefcafe";

struct App {
    state: AppState,
    _tempdir: TempDir,
}

async fn boot(provider: Arc<FakeProvider>) -> App {
    let tempdir = tempfile::Builder::new()
        .prefix("harness-server-delay-")
        .tempdir()
        .expect("tempdir");
    let db = Db::open(
        &tempdir.path().join("test.sqlite"),
        Secret::from_bytes([0u8; 32]),
    )
    .await
    .expect("open db");

    let provider_dyn: Arc<dyn LlmProvider> = provider;
    let providers = Arc::new(ProviderRegistry::new().with(provider_dyn.clone()));
    let sandbox_runner: Arc<dyn SandboxRunner> = pass_through_runner();
    let tools: Arc<dyn ToolRegistry> = default_registry();
    let orchestrator = Arc::new(Orchestrator::new(
        provider_dyn,
        sandbox_runner.clone(),
        tools.clone(),
    ));
    let state = acquire_app_state_with(
        db,
        SessionToken::new(TOKEN),
        providers,
        sandbox_runner,
        tools,
        orchestrator,
    )
    .await
    .expect("acquire state");
    App {
        state,
        _tempdir: tempdir,
    }
}

fn pass_through_runner() -> Arc<dyn SandboxRunner> {
    use async_trait::async_trait;
    use harness_core::{SandboxError, SandboxTemplate, ToolCommand, WrappedCommand};

    #[derive(Debug)]
    struct R;
    #[async_trait]
    impl SandboxRunner for R {
        async fn wrap(
            &self,
            _t: &SandboxTemplate,
            cmd: ToolCommand,
        ) -> Result<WrappedCommand, SandboxError> {
            Ok(WrappedCommand {
                program: cmd.program,
                args: cmd.args,
                env: cmd.env,
                cwd: cmd.cwd,
            })
        }
        async fn validate(&self, _profile: &str) -> Result<(), SandboxError> {
            Ok(())
        }
    }
    Arc::new(R)
}

struct ServerHandle {
    base_url: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<()>>,
}

async fn spawn(state: AppState) -> ServerHandle {
    let bound = bind_loopback().await.expect("bind");
    let addr = bound.local_addr;
    let router = build_router(state);
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let join = tokio::spawn(async move {
        serve(bound, router, async move {
            let _ = rx.await;
        })
        .await
        .expect("serve");
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    ServerHandle {
        base_url: format!("http://{addr}"),
        shutdown: Some(tx),
        join: Some(join),
    }
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

async fn configure_claude(handle: &ServerHandle) {
    let res = client()
        .post(format!("{}/v1/providers/claude/config", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .json(&json!({ "api_key": "x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
}

async fn create_conversation(handle: &ServerHandle) -> String {
    let res = client()
        .post(format!("{}/v1/conversations", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .json(&json!({
            "provider_id": "claude",
            "model": "fake-claude",
            "title": "delay",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 201);
    let v: Value = res.json().await.unwrap();
    v["id"].as_str().unwrap().to_owned()
}

async fn post_user_message(handle: &ServerHandle, conv_id: &str, text: &str) -> String {
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
        .unwrap();
    assert_eq!(res.status().as_u16(), 202);
    let v: Value = res.json().await.unwrap();
    v["run_id"].as_str().unwrap().to_owned()
}

async fn collect_until_run_end(
    handle: &ServerHandle,
    run_id: &str,
    timeout: Duration,
) -> Vec<(String, Value)> {
    let res = client()
        .get(format!("{}/v1/runs/{}/events", handle.base_url, run_id))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let mut stream = res.bytes_stream().eventsource();
    let deadline = tokio::time::Instant::now() + timeout;
    let mut out = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(ev))) => {
                let data: Value = serde_json::from_str(&ev.data).unwrap_or(Value::Null);
                let is_terminal = ev.event == "run.end";
                out.push((ev.event.clone(), data));
                if is_terminal {
                    break;
                }
            }
            Ok(Some(Err(_))) | Ok(None) | Err(_) => break,
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 1. Non-zero delay slows the run.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn nonzero_delay_paces_the_chat_stream() {
    // Vanilla turn emits 3 ChatEvents (MessageStart, ContentDelta,
    // MessageStop). At 50ms/event we expect ≥ ~120ms wall-clock, with
    // a generous lower bound to survive jitter.
    let provider = Arc::new(FakeProvider::with_delay_ms(50));
    let app = boot(provider).await;
    let handle = spawn(app.state.clone()).await;
    configure_claude(&handle).await;

    let conv_id = create_conversation(&handle).await;
    let run_id = post_user_message(&handle, &conv_id, "hi").await;

    let started = Instant::now();
    let events = collect_until_run_end(&handle, &run_id, Duration::from_secs(3)).await;
    let elapsed = started.elapsed();

    // Sanity: we did see a run.end (otherwise the elapsed time means
    // nothing — it could be the test timeout).
    assert!(
        events.iter().any(|(n, _)| n == "run.end"),
        "expected run.end; got names {:?}",
        events.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );

    let lower_bound = Duration::from_millis(120);
    assert!(
        elapsed >= lower_bound,
        "expected ≥ {lower_bound:?} with 50ms × 3 events; took {elapsed:?}"
    );

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// 2. No delay — the synchronous path stays fast.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn zero_delay_keeps_the_run_synchronous() {
    let provider = Arc::new(FakeProvider::with_delay_ms(0));
    let app = boot(provider).await;
    let handle = spawn(app.state.clone()).await;
    configure_claude(&handle).await;

    let conv_id = create_conversation(&handle).await;
    let run_id = post_user_message(&handle, &conv_id, "hi").await;

    let started = Instant::now();
    let events = collect_until_run_end(&handle, &run_id, Duration::from_secs(3)).await;
    let elapsed = started.elapsed();

    assert!(events.iter().any(|(n, _)| n == "run.end"));
    // 250ms is comfortably above any reasonable synchronous time (the
    // SSE handshake + tokio scheduling is the floor, not provider work)
    // and well below the 50ms × 3 = 150ms+ floor of the delayed path.
    let upper_bound = Duration::from_millis(250);
    assert!(
        elapsed < upper_bound,
        "expected < {upper_bound:?} with no delay; took {elapsed:?}"
    );

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// 3. Cancel mid-run with delay set: terminate within budget.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn cancel_with_delay_terminates_within_orchestrator_budget() {
    // 80ms × 3 events = ~240ms total run time; cancel at ~100ms so we
    // hit the gap between events. Orchestrator's cancellation budget
    // is ~100ms; total budget here is generous (500ms) to absorb test
    // scheduling.
    let provider = Arc::new(FakeProvider::with_delay_ms(80));
    let app = boot(provider).await;
    let handle = spawn(app.state.clone()).await;
    configure_claude(&handle).await;

    let conv_id = create_conversation(&handle).await;
    let run_id = post_user_message(&handle, &conv_id, "go").await;

    // Spawn the cancel after a delay so the orchestrator is mid-stream.
    let cancel_url = format!("{}/v1/runs/{}/cancel", handle.base_url, run_id);
    let cancel_handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let res = client()
            .post(&cancel_url)
            .header("X-Harness-Token", TOKEN)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status().as_u16(), 202);
    });

    let started = Instant::now();
    let events = collect_until_run_end(&handle, &run_id, Duration::from_secs(3)).await;
    let elapsed = started.elapsed();
    cancel_handle.await.unwrap();

    let run_end = events
        .iter()
        .find(|(n, _)| n == "run.end")
        .expect("run.end before timeout");
    assert_eq!(
        run_end.1.get("status").and_then(|s| s.as_str()),
        Some("cancelled"),
        "expected cancelled status; got events {:?}",
        events.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()
    );

    // Cancellation budget: cancel fires at ~100ms; orchestrator emits
    // RunEnd within ~100ms of that; SSE has to flush. 500ms total
    // leaves room for jitter. The point is to catch a regression where
    // delay swallows the cancel notification (e.g. if we ever start
    // sleeping inside a critical section).
    assert!(
        elapsed < Duration::from_millis(500),
        "cancel took too long: {elapsed:?}"
    );

    handle.shutdown().await;
}
