//! Sandbox port — abstracts wrapping a tool subprocess with a deny-by-
//! default macOS sandbox profile.
//!
//! `harness-core` itself does not import any process-runtime types.
//! `SandboxRunner::wrap` returns a structural `WrappedCommand`; the
//! `harness-sandbox` adapter (or any future replacement) converts that
//! to a `tokio::process::Command` at the edge.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::SandboxError;
use crate::ids::SandboxTemplateId;
use crate::tool::ToolCommand;

/// A user- or built-in-defined sandbox profile, persisted in the DB.
///
/// Timestamps are epoch seconds and authoritatively owned by the
/// storage layer:
/// * `harness-storage::SqliteSandboxTemplateRepo::create` stamps
///   `created_at = updated_at = now()` on insert, ignoring whatever
///   the caller put in the struct.
/// * `seed_builtins` preserves `created_at` on upsert and bumps
///   `updated_at = now()` so the bundled SBPL drift gets a fresh mtime
///   without losing the original install date.
/// * `update` rewrites `updated_at = now()`; `created_at` is never
///   touched once a row exists.
///
/// Construction sites that pre-date this field (built-ins,
/// fixtures, migration test data) use `0` as a sentinel; the storage
/// layer overwrites it on first seed. `0` is a valid epoch value but
/// no production row will ever observe it because storage always
/// stamps `now()` at insert time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxTemplate {
    pub id: SandboxTemplateId,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// SBPL profile text.
    pub profile: String,
    /// True for templates that ship with the app and are seeded on
    /// first run.
    pub is_builtin: bool,
    /// Epoch seconds when the row was first inserted. Owned by the
    /// storage layer (see struct-level docs).
    #[serde(default)]
    pub created_at: i64,
    /// Epoch seconds when the row was last updated.
    #[serde(default)]
    pub updated_at: i64,
}

/// The wrapped command an adapter should execute. Structurally
/// identical to `ToolCommand` but distinguished as a *post-sandbox*
/// shape — typically `program = "/usr/bin/sandbox-exec"` with the
/// original argv prefixed by `-f <profile-path> --`.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct WrappedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
}

/// The sandbox port.
///
/// Adapters: `harness-sandbox` ships the `sandbox-exec` impl. A future
/// container/seatbelt-based adapter would implement this trait without
/// any other code change.
#[async_trait]
pub trait SandboxRunner: Send + Sync + 'static {
    /// Wrap a `ToolCommand` so that it runs under the supplied
    /// template. The returned `WrappedCommand` is what the caller
    /// should spawn.
    async fn wrap(
        &self,
        template: &SandboxTemplate,
        cmd: ToolCommand,
    ) -> Result<WrappedCommand, SandboxError>;

    /// Validate a profile by dry-run (e.g. `sandbox-exec -f <profile>
    /// /usr/bin/true`). Returns `Ok(())` if the profile loads.
    async fn validate(&self, profile: &str) -> Result<(), SandboxError>;
}
