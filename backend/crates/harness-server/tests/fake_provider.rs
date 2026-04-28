//! Behavior tests for the production `harness_server::testing::FakeProvider`
//! and the `HARNESS_FAKE_PROVIDER` env knob.
//!
//! Three required cases per the T2.1.x brief:
//! 1. `GET /v1/providers` shows `claude` configured only after a
//!    config row is POSTed (consistency with the real-provider path).
//! 2. POSTing a vanilla user message yields content.delta carrying
//!    "Hello from fake provider.".
//! 3. POSTing a message containing "echo: hi" yields the full sequence
//!    `tool_use.start` → `tool.start` → `tool.finish` → final
//!    assistant text quoting "hi".
//!
//! A fourth covers the env knob's parser (`fake_provider_enabled`).

use std::sync::Arc;
use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use harness_core::{LlmProvider, SandboxRunner, ToolRegistry};
use harness_orchestrator::Orchestrator;
use harness_server::{
    bind_loopback,
    bootstrap::{acquire_app_state_with, fake_provider_enabled, ENV_FAKE_PROVIDER},
    build_router, serve, AppState, FakeProvider, ProviderRegistry, SessionToken,
};
use harness_storage::{Db, Secret};
use harness_tools::default_registry;
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use tempfile::TempDir;

const TOKEN: &str = "fake-prov-token-32bytes-cafe000";

/// Mirrors the production `acquire_app_state` env-switch but without
/// reading from process state — the orchestrator captures
/// `harness_server::testing::FakeProvider` directly.
struct FakeProviderApp {
    state: AppState,
    _tempdir: TempDir,
}

impl FakeProviderApp {
    async fn boot() -> Self {
        let tempdir = tempfile::Builder::new()
            .prefix("harness-server-fakeprov-")
            .tempdir()
            .expect("tempdir");
        let db = Db::open(
            &tempdir.path().join("test.sqlite"),
            Secret::from_bytes([0u8; 32]),
        )
        .await
        .expect("open db");

        let provider: Arc<dyn LlmProvider> = Arc::new(FakeProvider::new());
        let providers = Arc::new(ProviderRegistry::new().with(provider.clone()));
        let sandbox_runner: Arc<dyn SandboxRunner> = pass_through_runner();
        let tools: Arc<dyn ToolRegistry> = default_registry();
        let orchestrator = Arc::new(Orchestrator::new(
            provider,
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

        Self {
            state,
            _tempdir: tempdir,
        }
    }
}

/// Pass-through `SandboxRunner` that returns the input command verbatim
/// from `wrap` so external-tool invocations of `/bin/echo` can spawn
/// directly. Same shape as the one in tests/common/mod.rs but
/// duplicated here to keep this test file standalone.
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

async fn spawn_server(state: AppState) -> ServerHandle {
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
        .json(&json!({ "api_key": "anything-non-empty" }))
        .send()
        .await
        .expect("upsert config");
    assert_eq!(res.status().as_u16(), 200);
}

async fn create_conversation(handle: &ServerHandle) -> String {
    let res = client()
        .post(format!("{}/v1/conversations", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .json(&json!({
            "provider_id": "claude",
            "model": "fake-claude",
            "title": "fake-test",
            "sandbox_template_id": "strict-readonly",
        }))
        .send()
        .await
        .expect("create conv");
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
        .expect("post msg");
    assert_eq!(res.status().as_u16(), 202);
    let v: Value = res.json().await.unwrap();
    v["run_id"].as_str().unwrap().to_owned()
}

async fn collect_run_events(
    handle: &ServerHandle,
    run_id: &str,
    timeout: Duration,
) -> Vec<(String, Value, String)> {
    let res = client()
        .get(format!("{}/v1/runs/{}/events", handle.base_url, run_id))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .expect("sse");
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
                out.push((ev.event.clone(), data, ev.id.clone()));
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
// Test 1 — providers list reflects configured/unconfigured state.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn fake_provider_requires_config_for_configured_true() {
    let app = FakeProviderApp::boot().await;
    let handle = spawn_server(app.state.clone()).await;

    // Pre-config: claude is registered (via the fake) but configured=false.
    let res = client()
        .get(format!("{}/v1/providers", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = res.json().await.unwrap();
    let claude = body["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "claude")
        .expect("claude present");
    assert_eq!(claude["display_name"].as_str(), Some("Fake Claude"));
    assert_eq!(claude["configured"].as_bool(), Some(false));

    // After uploading any non-empty config, configured flips to true.
    configure_claude(&handle).await;
    let res = client()
        .get(format!("{}/v1/providers", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap();
    let body: Value = res.json().await.unwrap();
    let claude = body["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "claude")
        .unwrap();
    assert_eq!(claude["configured"].as_bool(), Some(true));

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Test 2 — vanilla user message yields the canned greeting.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn vanilla_message_emits_canned_greeting_via_sse() {
    let app = FakeProviderApp::boot().await;
    let handle = spawn_server(app.state.clone()).await;
    configure_claude(&handle).await;

    let conv_id = create_conversation(&handle).await;
    let run_id = post_user_message(&handle, &conv_id, "Hi.").await;

    let events = collect_run_events(&handle, &run_id, Duration::from_secs(3)).await;

    // Per spec/events.md the SSE event name is `content.delta` and the
    // payload is flat — `data.text` rather than `data.event.text`.
    let greeting = events.iter().find_map(|(name, data, _)| {
        if name == "content.delta" {
            data["text"].as_str().map(str::to_owned)
        } else {
            None
        }
    });
    assert_eq!(
        greeting.as_deref(),
        Some("Hello from fake provider."),
        "expected canned greeting; events were {events:?}"
    );

    let run_end = events.iter().find(|(n, _, _)| n == "run.end").unwrap();
    assert_eq!(run_end.1["status"].as_str(), Some("completed"));

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Test 3 — "echo: hi" routes through the echo tool and quotes back.
// ---------------------------------------------------------------------------
#[tokio::test]
async fn echo_substring_invokes_tool_and_quotes_output() {
    let app = FakeProviderApp::boot().await;
    let handle = spawn_server(app.state.clone()).await;
    configure_claude(&handle).await;

    let conv_id = create_conversation(&handle).await;
    let run_id = post_user_message(&handle, &conv_id, "Please echo: hi").await;

    let events = collect_run_events(&handle, &run_id, Duration::from_secs(5)).await;
    let names: Vec<&str> = events.iter().map(|(n, _, _)| n.as_str()).collect();

    assert!(
        names.contains(&"tool_use.start"),
        "missing tool_use.start: {names:?}"
    );
    assert!(
        names.contains(&"tool.start"),
        "missing tool.start: {names:?}"
    );
    assert!(
        names.contains(&"tool.finish"),
        "missing tool.finish: {names:?}"
    );

    // Final assistant text must quote "hi". Per spec/events.md the
    // payload is flat: `data.text`.
    let assistant_quote = events.iter().rev().find_map(|(n, d, _)| {
        if n == "content.delta" {
            d["text"].as_str().map(str::to_owned)
        } else {
            None
        }
    });
    let quote = assistant_quote.expect("expected at least one content_delta");
    assert!(
        quote.contains("hi"),
        "expected the fake to quote 'hi'; got {quote:?}"
    );
    assert!(
        quote.starts_with("Tool said:"),
        "expected the fake's quote-back format; got {quote:?}"
    );

    let run_end = events.iter().find(|(n, _, _)| n == "run.end").unwrap();
    assert_eq!(run_end.1["status"].as_str(), Some("completed"));

    handle.shutdown().await;
}

// ---------------------------------------------------------------------------
// Test 4 — env-knob parser. Pure, no HTTP.
// ---------------------------------------------------------------------------
#[test]
fn fake_provider_enabled_only_for_literal_one() {
    // Snapshot whatever value the test runner happens to have set so
    // we can restore it; setting env in tests is process-wide.
    let prior = std::env::var(ENV_FAKE_PROVIDER).ok();

    std::env::set_var(ENV_FAKE_PROVIDER, "1");
    assert!(fake_provider_enabled());

    std::env::set_var(ENV_FAKE_PROVIDER, "true");
    assert!(!fake_provider_enabled(), "only the literal '1' enables it");

    std::env::set_var(ENV_FAKE_PROVIDER, "");
    assert!(!fake_provider_enabled());

    std::env::remove_var(ENV_FAKE_PROVIDER);
    assert!(!fake_provider_enabled());

    if let Some(v) = prior {
        std::env::set_var(ENV_FAKE_PROVIDER, v);
    }
}
