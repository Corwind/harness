//! Shared test scaffolding.
//!
//! Spins up an isolated SQLite (file under a tempdir), seeds the
//! built-in sandbox templates via the production bootstrap path, and
//! returns a fully-wired [`harness_server::AppState`] together with a
//! handle to the underlying DB.
//!
//! The fixture goes through `harness_server::bootstrap::acquire_app_state_with`
//! so it exercises the same wiring path as `main` minus the OS-level
//! sandbox runner / Claude provider, both of which are replaced with
//! deterministic fakes.
//!
//! Cargo compiles this file separately for every integration-test
//! binary; an item used only by one binary triggers dead-code warnings
//! in the others. Allow dead code at the module level.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use harness_core::{
    chat::{ChatEvent, ChatRequest, StopReason},
    error::ProviderError,
    ids::ProviderId,
    provider::{LlmProvider, ModelInfo, ProviderCapabilities, ProviderConfig},
    SandboxError, SandboxRunner, SandboxTemplate, ToolCommand, ToolRegistry, WrappedCommand,
};
use harness_orchestrator::Orchestrator;
use harness_server::{bootstrap::acquire_app_state_with, AppState, ProviderRegistry, SessionToken};
use harness_storage::{Db, Secret};
use harness_tools::default_registry;
use tempfile::TempDir;

pub const TOKEN: &str = "common-test-token-32bytes-cafe00";

/// 32-byte all-zero key. Fine for test fixtures; never use in prod.
pub fn test_secret() -> Secret {
    Secret::from_bytes([0u8; 32])
}

/// All the bits a test typically needs.
pub struct TestApp {
    pub state: AppState,
    pub db: Db,
    pub provider: Arc<FakeProvider>,
    /// Owned tempdir; dropped at end of test → SQLite file is removed.
    pub _tempdir: TempDir,
}

impl TestApp {
    pub async fn boot() -> Self {
        Self::boot_with_runner(test_runner()).await
    }

    /// Build with a caller-supplied sandbox runner — used by `validate`
    /// tests that need a fake whose `validate` outcome is deterministic.
    pub async fn boot_with_runner(runner: Arc<dyn SandboxRunner>) -> Self {
        Self::boot_full(runner, FakeProvider::new()).await
    }

    /// Build with a caller-supplied provider — used by run/SSE tests.
    pub async fn boot_with_provider(provider: Arc<FakeProvider>) -> Self {
        Self::boot_full(test_runner(), provider).await
    }

    pub async fn boot_full(
        sandbox_runner: Arc<dyn SandboxRunner>,
        provider: Arc<FakeProvider>,
    ) -> Self {
        let tempdir = tempfile::Builder::new()
            .prefix("harness-server-test-")
            .tempdir()
            .expect("create tempdir");
        let db_path = tempdir.path().join("test.sqlite");

        let db = Db::open(&db_path, test_secret())
            .await
            .expect("open test sqlite");

        let provider_dyn: Arc<dyn LlmProvider> = provider.clone();
        let providers = Arc::new(ProviderRegistry::new().with(provider_dyn.clone()));
        let tools: Arc<dyn ToolRegistry> = default_registry();
        let orchestrator = Arc::new(Orchestrator::new(
            provider_dyn,
            sandbox_runner.clone(),
            tools.clone(),
        ));

        let state = acquire_app_state_with(
            db.clone(),
            SessionToken::new(TOKEN),
            providers,
            sandbox_runner,
            tools,
            orchestrator,
        )
        .await
        .expect("acquire app state");

        Self {
            state,
            db,
            provider,
            _tempdir: tempdir,
        }
    }

    /// Re-open the same DB **without** running boot a second time. Used by
    /// the "second boot does not duplicate" test.
    pub async fn reopen(db_path: &std::path::Path) -> Self {
        let db = Db::open(db_path, test_secret())
            .await
            .expect("reopen test sqlite");
        let provider = FakeProvider::new();
        let provider_dyn: Arc<dyn LlmProvider> = provider.clone();
        let providers = Arc::new(ProviderRegistry::new().with(provider_dyn.clone()));
        let tools: Arc<dyn ToolRegistry> = default_registry();
        let runner = test_runner();
        let orchestrator = Arc::new(Orchestrator::new(
            provider_dyn,
            runner.clone(),
            tools.clone(),
        ));

        let state = acquire_app_state_with(
            db.clone(),
            SessionToken::new(TOKEN),
            providers,
            runner,
            tools,
            orchestrator,
        )
        .await
        .expect("acquire app state");

        // Synthesise a tempdir that won't actually be deleted.
        let tempdir = tempfile::tempdir().expect("placeholder tempdir");
        Self {
            state,
            db,
            provider,
            _tempdir: tempdir,
        }
    }
}

/// A `SandboxRunner` whose `validate` always succeeds; `wrap` is a
/// no-op pass-through (returns the original command verbatim) so the
/// orchestrator's external-tool path can spawn it directly.
pub fn test_runner() -> Arc<dyn SandboxRunner> {
    Arc::new(AlwaysValidRunner)
}

/// Helper for the failure-path validation test.
pub fn rejecting_runner(stderr: impl Into<String>) -> Arc<dyn SandboxRunner> {
    Arc::new(RejectingRunner {
        stderr: stderr.into(),
    })
}

#[derive(Debug)]
struct AlwaysValidRunner;

#[async_trait]
impl SandboxRunner for AlwaysValidRunner {
    async fn wrap(
        &self,
        _t: &SandboxTemplate,
        cmd: ToolCommand,
    ) -> Result<WrappedCommand, SandboxError> {
        // Pass-through: handle the orchestrator's external-tool path
        // without invoking real `sandbox-exec`. The tool runs as the
        // current process — fine for tests, where /bin/echo is benign.
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

#[derive(Debug)]
struct RejectingRunner {
    stderr: String,
}

#[async_trait]
impl SandboxRunner for RejectingRunner {
    async fn wrap(
        &self,
        _t: &SandboxTemplate,
        _cmd: ToolCommand,
    ) -> Result<WrappedCommand, SandboxError> {
        Err(SandboxError::Io("unused in tests".into()))
    }
    async fn validate(&self, _profile: &str) -> Result<(), SandboxError> {
        Err(SandboxError::ProfileInvalid {
            stderr: self.stderr.clone(),
        })
    }
}

/// A scriptable in-memory `LlmProvider` used by every T1.E test that
/// drives a run end-to-end. Tests push a sequence of `ChatEvent`s onto
/// the provider's queue; each call to `chat()` pops one batch and
/// returns it as a `BoxStream<ChatEvent>`.
///
/// This pattern lets a single test script (a) a tool-using turn that
/// emits a `tool_use_*` sequence + `MessageStop { ToolUse }`, then (b) a
/// follow-up turn that emits `MessageStop { EndTurn }` after the
/// orchestrator feeds the tool result back.
#[derive(Debug, Default)]
pub struct FakeProvider {
    queue: Mutex<Vec<Vec<ChatEvent>>>,
    /// Optional model list returned from `list_models`. Default is one
    /// model so tests don't have to set it up.
    models: Mutex<Vec<ModelInfo>>,
    /// How long each `ChatEvent` waits before being emitted. Tests can
    /// tune this to simulate slow streams; default 0.
    delay_ms: Mutex<u64>,
}

impl FakeProvider {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: Mutex::new(Vec::new()),
            models: Mutex::new(vec![ModelInfo {
                id: "fake-1".to_owned(),
                display_name: "Fake".to_owned(),
                context_window: Some(8192),
                max_output_tokens: Some(2048),
            }]),
            delay_ms: Mutex::new(0),
        })
    }

    pub fn enqueue_turn(&self, events: Vec<ChatEvent>) {
        self.queue.lock().unwrap().push(events);
    }

    pub fn enqueue_simple_turn(&self, text: &str) {
        self.enqueue_turn(vec![
            ChatEvent::MessageStart {
                id: "msg_fake".to_owned(),
            },
            ChatEvent::ContentDelta {
                text: text.to_owned(),
            },
            ChatEvent::MessageStop {
                stop_reason: StopReason::EndTurn,
                usage: None,
            },
        ]);
    }

    pub fn set_delay_ms(&self, ms: u64) {
        *self.delay_ms.lock().unwrap() = ms;
    }
}

#[async_trait]
impl LlmProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }
    fn display_name(&self) -> &'static str {
        "Fake provider"
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
            vision: false,
            system_prompt: true,
            max_context_tokens: Some(8192),
        }
    }
    async fn list_models(&self, _cfg: &ProviderConfig) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(self.models.lock().unwrap().clone())
    }
    async fn chat(
        &self,
        _cfg: &ProviderConfig,
        _request: ChatRequest,
    ) -> Result<BoxStream<'static, ChatEvent>, ProviderError> {
        let events = self
            .queue
            .lock()
            .unwrap()
            .drain(..1)
            .next()
            .unwrap_or_default();
        let delay = *self.delay_ms.lock().unwrap();
        if delay == 0 {
            Ok(Box::pin(stream::iter(events)))
        } else {
            // Spread events over time so tests of cancellation /
            // resume have a window to act.
            let s = async_stream::stream! {
                for ev in events {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    yield ev;
                }
            };
            Ok(Box::pin(s))
        }
    }
}

/// Convenience: provider id used for the fake's stored config.
pub fn fake_provider_id() -> ProviderId {
    ProviderId::from_string("fake")
}
