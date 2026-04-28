//! Boot-time wiring: open the database, seed the four built-in sandbox
//! templates on first run, build the orchestrator + provider registry,
//! and return a fully constructed [`AppState`].
//!
//! Idempotency: `SqliteSandboxTemplateRepo::seed_builtins` upserts each
//! built-in by `id`. The four ids are stable consts in `harness-sandbox`
//! (`BUILTIN_STRICT_READONLY`, …), so re-running on every startup is safe
//! and keeps drifted bundled profiles in sync.
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

use crate::{auth::SessionToken, providers::ProviderRegistry, runs::RunRegistry, state::AppState};

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

/// Build a complete [`AppState`] for production: real Claude provider,
/// real `SbxRunner`, real default tool registry.
pub async fn acquire_app_state(db: Db, token: SessionToken) -> Result<AppState, anyhow::Error> {
    let claude: Arc<dyn LlmProvider> = Arc::new(ClaudeProvider::new());
    let providers = Arc::new(ProviderRegistry::new().with(claude.clone()));
    let sandbox_runner: Arc<dyn SandboxRunner> = Arc::new(SbxRunner::new()?);
    let tools: Arc<dyn ToolRegistry> = default_registry();
    // Orchestrator uses the *first* provider for now. Per-conversation
    // routing (different providers in the same backend) lands in T1.E's
    // follow-up: the message-post handler resolves the right provider
    // by id from the registry, but the orchestrator currently captures
    // a single `Arc<dyn LlmProvider>`. We pass Claude here as the
    // production default; T1.E tests use a fake injected via
    // `acquire_app_state_with`.
    let orchestrator = Arc::new(Orchestrator::new(
        claude,
        sandbox_runner.clone(),
        tools.clone(),
    ));

    acquire_app_state_with(db, token, providers, sandbox_runner, tools, orchestrator).await
}

/// Build an [`AppState`] from a pre-built provider registry / runner /
/// tools / orchestrator. Used by tests that want fakes.
pub async fn acquire_app_state_with(
    db: Db,
    token: SessionToken,
    providers: Arc<ProviderRegistry>,
    sandbox_runner: Arc<dyn SandboxRunner>,
    tools: Arc<dyn ToolRegistry>,
    orchestrator: Arc<Orchestrator>,
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
    })
}
