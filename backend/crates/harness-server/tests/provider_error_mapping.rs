//! Behavior tests for the `ProviderError` → HTTP mapping (T1.followup).
//!
//! Coverage:
//! 1. Unauthorized → 401, body code `provider.unauthorized`.
//! 2. RateLimited → 429, body code `provider.rate_limited`, plus a
//!    `Retry-After` header carrying the supplied seconds.
//! 3. NotConfigured → 409, body code `provider.unconfigured` (the
//!    route's own precondition; verified to use the new code, not the
//!    legacy `provider_error`).
//! 4. Default success path still works against the same fake.
//! 5. Transport → 502, body code `provider.unavailable`.
//! 6. Upstream 5xx → 502, body code `provider.upstream`.
//! 7. Upstream 4xx (non-401) → echoed status, body code `provider.request`.

use std::sync::Arc;
use std::time::Duration;

use harness_core::{LlmProvider, SandboxRunner, ToolRegistry};
use harness_orchestrator::Orchestrator;
use harness_server::{
    bind_loopback, bootstrap::acquire_app_state_with, build_router, serve, AppState, FakeFailure,
    FakeProvider, ProviderRegistry, SessionToken,
};
use harness_storage::{Db, Secret};
use harness_tools::default_registry;
use pretty_assertions::assert_eq;
use serde_json::{json, Value};
use tempfile::TempDir;

const TOKEN: &str = "perr-token-32bytes-deadbeefcafe0";

struct App {
    state: AppState,
    _tempdir: TempDir,
}

async fn boot(provider: Arc<FakeProvider>) -> App {
    let tempdir = tempfile::Builder::new()
        .prefix("harness-server-perr-")
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
        .json(&json!({ "api_key": "anything-non-empty" }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
}

async fn get_models(handle: &ServerHandle) -> reqwest::Response {
    client()
        .get(format!("{}/v1/providers/claude/models", handle.base_url))
        .header("X-Harness-Token", TOKEN)
        .send()
        .await
        .unwrap()
}

// 1. Unauthorized → 401 + provider.unauthorized.
#[tokio::test]
async fn unauthorized_provider_error_maps_to_401() {
    let app = boot(Arc::new(FakeProvider::with_failure(
        FakeFailure::Unauthorized,
    )))
    .await;
    let handle = spawn(app.state.clone()).await;
    configure_claude(&handle).await;

    let res = get_models(&handle).await;
    assert_eq!(res.status().as_u16(), 401);
    let body: Value = res.json().await.unwrap();
    assert_eq!(
        body["code"].as_str(),
        Some("provider.unauthorized"),
        "body was {body}"
    );
    assert!(
        body["message"].as_str().is_some(),
        "expected a human-readable message: {body}"
    );

    handle.shutdown().await;
}

// 2. RateLimited with retry_after → 429 + Retry-After header.
#[tokio::test]
async fn rate_limited_emits_429_and_retry_after_header() {
    let app = boot(Arc::new(FakeProvider::with_failure(
        FakeFailure::RateLimited {
            retry_after_secs: Some(7),
        },
    )))
    .await;
    let handle = spawn(app.state.clone()).await;
    configure_claude(&handle).await;

    let res = get_models(&handle).await;
    assert_eq!(res.status().as_u16(), 429);
    let retry = res
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    assert_eq!(retry.as_deref(), Some("7"));
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["code"].as_str(), Some("provider.rate_limited"));

    handle.shutdown().await;
}

// 2b. RateLimited with no retry_after → 429 but no header.
#[tokio::test]
async fn rate_limited_without_retry_after_omits_header() {
    let app = boot(Arc::new(FakeProvider::with_failure(
        FakeFailure::RateLimited {
            retry_after_secs: None,
        },
    )))
    .await;
    let handle = spawn(app.state.clone()).await;
    configure_claude(&handle).await;

    let res = get_models(&handle).await;
    assert_eq!(res.status().as_u16(), 429);
    assert!(
        res.headers().get("retry-after").is_none(),
        "should omit Retry-After when upstream gave us none"
    );
    handle.shutdown().await;
}

// 3. NotConfigured (no row in providers_config) → 409 + provider.unconfigured.
//    This is the route's own precondition; the test pins the new code
//    so the UI's rule-of-thumb (every provider error code starts with
//    'provider.') stays consistent.
#[tokio::test]
async fn unconfigured_provider_returns_409_with_provider_unconfigured_code() {
    let app = boot(Arc::new(FakeProvider::new())).await;
    let handle = spawn(app.state.clone()).await;
    // Deliberately do NOT call configure_claude here.

    let res = get_models(&handle).await;
    assert_eq!(res.status().as_u16(), 409);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["code"].as_str(), Some("provider.unconfigured"));

    handle.shutdown().await;
}

// 4. The default success path still works (regression guard for the
//    rename + the new From impl).
#[tokio::test]
async fn default_success_path_still_returns_models() {
    let app = boot(Arc::new(FakeProvider::new())).await;
    let handle = spawn(app.state.clone()).await;
    configure_claude(&handle).await;

    let res = get_models(&handle).await;
    assert_eq!(res.status().as_u16(), 200);
    let body: Value = res.json().await.unwrap();
    let models = body["models"].as_array().unwrap();
    assert!(!models.is_empty());
    assert_eq!(models[0]["id"].as_str(), Some("fake-claude"));

    handle.shutdown().await;
}

// 5. Transport → 502 + provider.unavailable.
#[tokio::test]
async fn transport_error_maps_to_502_unavailable() {
    let app = boot(Arc::new(FakeProvider::with_failure(FakeFailure::Transport))).await;
    let handle = spawn(app.state.clone()).await;
    configure_claude(&handle).await;

    let res = get_models(&handle).await;
    assert_eq!(res.status().as_u16(), 502);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["code"].as_str(), Some("provider.unavailable"));

    handle.shutdown().await;
}

// 6. Upstream 5xx Request → 502 + provider.upstream.
#[tokio::test]
async fn upstream_5xx_collapses_to_502_with_upstream_code() {
    let app = boot(Arc::new(FakeProvider::with_failure(
        FakeFailure::UpstreamStatus(503),
    )))
    .await;
    let handle = spawn(app.state.clone()).await;
    configure_claude(&handle).await;

    let res = get_models(&handle).await;
    assert_eq!(res.status().as_u16(), 502);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["code"].as_str(), Some("provider.upstream"));

    handle.shutdown().await;
}

// 7. Upstream 4xx Request (non-401/429) → echoed 4xx + provider.request.
#[tokio::test]
async fn upstream_4xx_request_preserves_status_code() {
    let app = boot(Arc::new(FakeProvider::with_failure(
        FakeFailure::UpstreamStatus(422),
    )))
    .await;
    let handle = spawn(app.state.clone()).await;
    configure_claude(&handle).await;

    let res = get_models(&handle).await;
    assert_eq!(res.status().as_u16(), 422);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["code"].as_str(), Some("provider.request"));

    handle.shutdown().await;
}
