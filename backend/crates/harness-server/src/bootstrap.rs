//! Boot-time wiring: open the database, seed the four built-in sandbox
//! templates on first run, build the orchestrator + provider registry,
//! and return a fully constructed [`AppState`].
//!
//! Idempotency: `SqliteSandboxTemplateRepo::seed_builtins` upserts each
//! built-in by `id`. The four ids are stable consts in `harness-sandbox`
//! (`BUILTIN_STRICT_READONLY`, …), so re-running on every startup is safe
//! and keeps drifted bundled profiles in sync.
//!
//! ## Runtime knobs
//!
//! * `HARNESS_FAKE_PROVIDER=1` — register the deterministic
//!   [`crate::testing::FakeProvider`] under provider id `"claude"`
//!   instead of the real `ClaudeProvider`. Used by Swift-side e2e tests
//!   (T2.1, T2.2). The fake still requires a stored config row (any
//!   non-empty `api_key` will do) so the configured/unconfigured
//!   semantics match the production path. A warning is logged at
//!   startup.
//! * `HARNESS_FAKE_PROVIDER_DELAY_MS=<n>` — when the fake is active,
//!   inject `n` milliseconds of `tokio::time::sleep` between every
//!   emitted `ChatEvent`. Lets live e2e tests drive cancellation
//!   mid-stream without racing the synchronous default. See
//!   [`crate::testing::ENV_FAKE_PROVIDER_DELAY_MS`].
//!
//! Behavior tests can either:
//! * drive `seed_builtin_sandbox_templates` directly (still public), or
//! * use the lower-level [`acquire_app_state_with`] constructor that
//!   accepts a pre-built provider registry / orchestrator so tests can
//!   inject fakes.

use std::sync::Arc;

use harness_core::{LlmProvider, SandboxRunner, ToolRegistry};
use harness_orchestrator::Orchestrator;
use harness_providers::claude::ClaudeProvider;
use harness_sandbox::{builtin_templates, SbxRunner};
use harness_storage::{
    Db, SqliteConversationRepo, SqliteMessageRepo, SqliteProvidersConfigRepo,
    SqliteSandboxTemplateRepo, SqliteSettingsRepo,
};
use harness_tools::default_registry;

use crate::{
    auth::SessionToken, diagnostics::LogRing, providers::ProviderRegistry, runs::RunRegistry,
    state::AppState, testing::FakeProvider,
};

/// Env var that swaps the production Claude provider for the
/// deterministic [`FakeProvider`]. Set to `"1"` to enable.
pub const ENV_FAKE_PROVIDER: &str = "HARNESS_FAKE_PROVIDER";

/// Upsert the four built-in templates from `harness-sandbox`. Returns the
/// number of templates seeded (always 4 today; surfaced for tests).
pub async fn seed_builtin_sandbox_templates(
    repo: &SqliteSandboxTemplateRepo,
) -> Result<usize, harness_core::error::RepoError> {
    let templates = builtin_templates();
    let n = templates.len();
    repo.seed_builtins(&templates).await?;
    Ok(n)
}

/// Build a complete [`AppState`] for production.
///
/// Honours the [`ENV_FAKE_PROVIDER`] runtime knob: when set to `"1"`,
/// the registered "claude" provider is the deterministic
/// [`FakeProvider`] from this crate. The configured/unconfigured
/// semantics still apply — operators (or Swift e2e tests) must POST a
/// config to `/v1/providers/claude/config` before models / runs work,
/// which keeps this seam consistent with the real-provider path.
///
/// `logs` is the diagnostics ring shared with the tracing subscriber
/// installed in `main.rs`; both halves must reference the same `Arc`
/// so what's logged is what `/v1/diagnostics/logs` returns.
pub async fn acquire_app_state(
    db: Db,
    token: SessionToken,
    logs: Arc<LogRing>,
) -> Result<AppState, anyhow::Error> {
    let provider: Arc<dyn LlmProvider> = if fake_provider_enabled() {
        tracing::warn!(
            env = ENV_FAKE_PROVIDER,
            "{} is set; registering FakeProvider under id 'claude' (NOT for production)",
            ENV_FAKE_PROVIDER
        );
        Arc::new(FakeProvider::new())
    } else {
        Arc::new(ClaudeProvider::new())
    };
    let providers = Arc::new(ProviderRegistry::new().with(provider.clone()));
    let sandbox_runner: Arc<dyn SandboxRunner> = Arc::new(SbxRunner::new()?);
    let tools: Arc<dyn ToolRegistry> = default_registry();
    let orchestrator = Arc::new(Orchestrator::new(
        provider,
        sandbox_runner.clone(),
        tools.clone(),
    ));

    acquire_app_state_with(
        db,
        token,
        providers,
        sandbox_runner,
        tools,
        orchestrator,
        logs,
    )
    .await
}

/// Returns `true` when the env var [`ENV_FAKE_PROVIDER`] is set to
/// exactly `"1"`. Any other value (including unset) returns `false`.
pub fn fake_provider_enabled() -> bool {
    matches!(std::env::var(ENV_FAKE_PROVIDER).as_deref(), Ok("1"))
}

/// Build an [`AppState`] from a pre-built provider registry / runner /
/// tools / orchestrator. Used by tests that want fakes.
///
/// `logs` is the diagnostics ring; tests usually pass
/// `Arc::new(LogRing::new())` if they don't care about the contents.
pub async fn acquire_app_state_with(
    db: Db,
    token: SessionToken,
    providers: Arc<ProviderRegistry>,
    sandbox_runner: Arc<dyn SandboxRunner>,
    tools: Arc<dyn ToolRegistry>,
    orchestrator: Arc<Orchestrator>,
    logs: Arc<LogRing>,
) -> Result<AppState, anyhow::Error> {
    let sandbox_templates = SqliteSandboxTemplateRepo::new(db.clone());
    let _ = seed_builtin_sandbox_templates(&sandbox_templates).await?;

    let conversations = SqliteConversationRepo::new(db.clone());
    let messages = SqliteMessageRepo::new(db.clone());
    let settings = SqliteSettingsRepo::new(db.clone());
    let providers_config = SqliteProvidersConfigRepo::new(db);

    let runs = Arc::new(RunRegistry::new());

    Ok(AppState {
        token,
        sandbox_templates: Arc::new(sandbox_templates),
        conversations: Arc::new(conversations),
        messages: Arc::new(messages),
        settings: Arc::new(settings),
        providers_config: Arc::new(providers_config),
        sandbox_runner,
        providers,
        tools,
        orchestrator,
        runs,
        logs,
    })
}
