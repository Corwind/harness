//! Concrete adapter for the [`SandboxRunner`] port: wraps subprocess
//! invocations in `/usr/bin/sandbox-exec -f <profile> -- <argv>` and
//! validates SBPL profiles via the same binary's dry-run mode.
//!
//! ## Tempdir ownership
//!
//! Each [`SbxRunner`] owns a [`tempfile::TempDir`] created on
//! construction. All materialised profile files live inside it. The
//! tempdir is removed (recursively) when the runner is dropped. We do
//! **not** unlink individual profile files after each invocation
//! because:
//!
//! 1. Sandbox-exec only needs the file open long enough to compile the
//!    profile, but a child that re-execs (e.g. a tool that fork-execs a
//!    helper) might be re-checked against the policy. Keeping the file
//!    around avoids races.
//! 2. The tempdir cleanup on `Drop` is sufficient: profiles are tiny,
//!    and the runner's lifetime is tied to a single backend process.
//!
//! ## Profile materialisation
//!
//! Every call to [`SbxRunner::wrap`] writes a **fresh** `<uuid>.sb`
//! file under the tempdir; we never reuse files across invocations.
//! Reasons:
//!
//! - Concurrent `wrap()` calls would otherwise have to coordinate on a
//!   shared filename. Per-invocation UUIDs make the path unique and
//!   sidestep the synchronisation entirely.
//! - A future change to a template's text would have to invalidate any
//!   cached file; per-invocation files mean every call sees the latest
//!   profile by construction.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use harness_core::{SandboxError, SandboxRunner, SandboxTemplate, ToolCommand, WrappedCommand};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::wrap::{build_wrapped, SANDBOX_EXEC};

/// Path to `/usr/bin/true`. The validation dry-run executes this — on
/// macOS it always exists, but the constant keeps the assumption in
/// one place.
const TRUE_BIN: &str = "/usr/bin/true";

/// `sandbox-exec` adapter implementing [`SandboxRunner`].
///
/// Construct one per-process at startup; clone via [`Arc`] across
/// orchestrator tasks. [`SbxRunner`] is `Send + Sync + 'static`, which
/// the trait demands.
#[derive(Debug)]
pub struct SbxRunner {
    /// Held by [`Arc`] so we can hand out cheap clones into background
    /// tasks while keeping a single tempdir for the whole runner.
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// Owning handle. Dropping this removes the tempdir and every
    /// materialised profile inside it.
    tempdir: tempfile::TempDir,
}

impl SbxRunner {
    /// Create a new runner. Allocates a tempdir under the OS temp
    /// root with a `harness-sandbox-` prefix.
    ///
    /// Errors only if the tempdir cannot be created (e.g. disk full,
    /// `/tmp` not writable).
    pub fn new() -> Result<Self, SandboxError> {
        let tempdir = tempfile::Builder::new()
            .prefix("harness-sandbox-")
            .tempdir()
            .map_err(|e| SandboxError::Io(format!("create tempdir: {e}")))?;
        Ok(Self {
            inner: Arc::new(Inner { tempdir }),
        })
    }

    /// Path to the runner's profile tempdir. Test-only.
    #[cfg(test)]
    pub(crate) fn profile_dir(&self) -> &std::path::Path {
        self.inner.tempdir.path()
    }

    /// Materialise `profile` to a fresh `<uuid>.sb` under the tempdir
    /// and return its absolute path. Used by both `wrap()` and
    /// `validate()`.
    async fn write_profile(&self, profile: &str) -> Result<PathBuf, SandboxError> {
        let filename = format!("{}.sb", Uuid::new_v4());
        let path = self.inner.tempdir.path().join(filename);
        // `tokio::fs::write` would do, but we want an explicit flush so
        // that `sandbox-exec` can never see a half-written file.
        let mut f = tokio::fs::File::create(&path)
            .await
            .map_err(|e| SandboxError::Io(format!("create profile {}: {e}", path.display())))?;
        f.write_all(profile.as_bytes())
            .await
            .map_err(|e| SandboxError::Io(format!("write profile {}: {e}", path.display())))?;
        f.flush()
            .await
            .map_err(|e| SandboxError::Io(format!("flush profile {}: {e}", path.display())))?;
        Ok(path)
    }
}

#[async_trait]
impl SandboxRunner for SbxRunner {
    async fn wrap(
        &self,
        template: &SandboxTemplate,
        cmd: ToolCommand,
    ) -> Result<WrappedCommand, SandboxError> {
        let profile_path = self.write_profile(&template.profile).await?;
        Ok(build_wrapped(&profile_path, cmd))
    }

    async fn validate(&self, profile: &str) -> Result<(), SandboxError> {
        let profile_path = self.write_profile(profile).await?;
        let output = tokio::process::Command::new(SANDBOX_EXEC)
            .arg("-f")
            .arg(&profile_path)
            .arg(TRUE_BIN)
            .output()
            .await
            .map_err(|e| {
                // `sandbox-exec` not present (non-macOS / stripped image)
                // shows up as NotFound; everything else is plain I/O.
                if e.kind() == std::io::ErrorKind::NotFound {
                    SandboxError::RuntimeUnavailable(format!("{SANDBOX_EXEC}: {e}"))
                } else {
                    SandboxError::Io(format!("spawn sandbox-exec: {e}"))
                }
            })?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            Err(SandboxError::ProfileInvalid { stderr })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn write_profile_creates_unique_files() {
        let r = SbxRunner::new().unwrap();
        let a = r.write_profile("(version 1)\n(allow default)\n").await.unwrap();
        let b = r.write_profile("(version 1)\n(allow default)\n").await.unwrap();
        assert_ne!(a, b, "successive write_profile calls must use unique paths");
        assert!(a.starts_with(r.profile_dir()));
        assert!(b.starts_with(r.profile_dir()));
    }
}
