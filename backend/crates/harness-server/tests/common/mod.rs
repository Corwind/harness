//! Shared test scaffolding.
//!
//! Spins up an isolated SQLite (file under a tempdir), seeds the
//! built-in sandbox templates via the production bootstrap path, and
//! returns a fully-wired [`harness_server::AppState`] together with a
//! handle to the underlying DB so tests can inspect raw rows when
//! necessary.
//!
//! Cargo compiles this file separately for every integration-test
//! binary; an item used only by `sandbox_templates.rs` triggers
//! dead-code warnings when compiled into `auth_and_health.rs`. Allow
//! dead code at the module level so the helpers stay shared without
//! per-item annotations.

#![allow(dead_code)]

use std::sync::Arc;

use harness_core::SandboxRunner;
use harness_server::{bootstrap::seed_builtin_sandbox_templates, AppState, SessionToken};
use harness_storage::{Db, Secret, SqliteConversationRepo, SqliteSandboxTemplateRepo};
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
        let tempdir = tempfile::Builder::new()
            .prefix("harness-server-test-")
            .tempdir()
            .expect("create tempdir");
        let db_path = tempdir.path().join("test.sqlite");

        let db = Db::open(&db_path, test_secret())
            .await
            .expect("open test sqlite");

        let sandbox_templates = SqliteSandboxTemplateRepo::new(db.clone());
        seed_builtin_sandbox_templates(&sandbox_templates)
            .await
            .expect("seed builtins");

        let conversations = SqliteConversationRepo::new(db.clone());

        let state = AppState {
            token: SessionToken::new(TOKEN),
            sandbox_templates: Arc::new(sandbox_templates),
            conversations: Arc::new(conversations),
            sandbox_runner: runner,
        };

        Self {
            state,
            db,
            _tempdir: tempdir,
        }
    }

    /// Re-open the same DB **without** running boot a second time. Used by
    /// the "second boot does not duplicate" test.
    pub async fn reopen(db_path: &std::path::Path) -> Self {
        let db = Db::open(db_path, test_secret())
            .await
            .expect("reopen test sqlite");
        let sandbox_templates = SqliteSandboxTemplateRepo::new(db.clone());
        seed_builtin_sandbox_templates(&sandbox_templates)
            .await
            .expect("re-seed builtins (must be idempotent)");
        let conversations = SqliteConversationRepo::new(db.clone());

        // Synthesise a tempdir that won't actually be deleted (the caller
        // owns the original); this keeps the type uniform.
        let tempdir = tempfile::tempdir().expect("placeholder tempdir");
        let state = AppState {
            token: SessionToken::new(TOKEN),
            sandbox_templates: Arc::new(sandbox_templates),
            conversations: Arc::new(conversations),
            sandbox_runner: test_runner(),
        };

        Self {
            state,
            db,
            _tempdir: tempdir,
        }
    }
}

/// A `SandboxRunner` whose `validate` always succeeds; `wrap` is a
/// stub. The handlers under test never call `wrap` (it lives in the
/// orchestrator path) so its body returns `Io("unused in tests")`.
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

#[async_trait::async_trait]
impl SandboxRunner for AlwaysValidRunner {
    async fn wrap(
        &self,
        _t: &harness_core::SandboxTemplate,
        _cmd: harness_core::ToolCommand,
    ) -> Result<harness_core::WrappedCommand, harness_core::SandboxError> {
        Err(harness_core::SandboxError::Io("unused in tests".into()))
    }
    async fn validate(&self, _profile: &str) -> Result<(), harness_core::SandboxError> {
        Ok(())
    }
}

#[derive(Debug)]
struct RejectingRunner {
    stderr: String,
}

#[async_trait::async_trait]
impl SandboxRunner for RejectingRunner {
    async fn wrap(
        &self,
        _t: &harness_core::SandboxTemplate,
        _cmd: harness_core::ToolCommand,
    ) -> Result<harness_core::WrappedCommand, harness_core::SandboxError> {
        Err(harness_core::SandboxError::Io("unused in tests".into()))
    }
    async fn validate(&self, _profile: &str) -> Result<(), harness_core::SandboxError> {
        Err(harness_core::SandboxError::ProfileInvalid {
            stderr: self.stderr.clone(),
        })
    }
}
