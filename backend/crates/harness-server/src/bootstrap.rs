//! Boot-time wiring: open the database, seed the four built-in sandbox
//! templates on first run, and return a fully constructed [`AppState`].
//!
//! Idempotency: `SqliteSandboxTemplateRepo::seed_builtins` upserts each
//! built-in by `id`. The four ids are stable consts in `harness-sandbox`
//! (`BUILTIN_STRICT_READONLY`, …), so re-running on every startup is safe
//! and keeps drifted bundled profiles in sync.
//!
//! The seeding helper is exposed publicly (alongside the more general
//! `acquire_app_state`) so behavior tests can drive it against a fresh
//! in-memory SQLite without going through `main`.

use std::sync::Arc;

use harness_core::SandboxRunner;
use harness_sandbox::{builtin_templates, SbxRunner};
use harness_storage::{Db, SqliteConversationRepo, SqliteSandboxTemplateRepo};

use crate::{auth::SessionToken, state::AppState};

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

/// Build a complete [`AppState`] from a freshly opened [`Db`] and a
/// generated session token.
///
/// Seeds the built-in templates as a side effect (idempotent).
pub async fn acquire_app_state(db: Db, token: SessionToken) -> Result<AppState, anyhow::Error> {
    let sandbox_templates = SqliteSandboxTemplateRepo::new(db.clone());
    let _ = seed_builtin_sandbox_templates(&sandbox_templates).await?;

    let conversations = SqliteConversationRepo::new(db);
    let sandbox_runner: Arc<dyn SandboxRunner> = Arc::new(SbxRunner::new()?);

    Ok(AppState {
        token,
        sandbox_templates: Arc::new(sandbox_templates),
        conversations: Arc::new(conversations),
        sandbox_runner,
    })
}
