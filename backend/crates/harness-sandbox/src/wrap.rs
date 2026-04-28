//! Pure helpers for translating a `(SandboxTemplate, ToolCommand)` pair
//! into the post-sandbox `WrappedCommand` shape, plus the edge-of-crate
//! conversion to a `tokio::process::Command` that the orchestrator can
//! spawn.
//!
//! Keeping the wrap step separate from the [`crate::runner::SbxRunner`]
//! means we can unit-test argv assembly without touching the filesystem
//! and without needing `sandbox-exec` to be available.

use std::path::Path;

use harness_core::{ToolCommand, WrappedCommand};

/// Absolute path to the macOS `sandbox-exec` binary. Hard-coded because
/// we require the system one (PATH could be hijacked).
pub(crate) const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Build the post-sandbox argv for `cmd` given that its profile has
/// already been materialised at `profile_path`.
///
/// The resulting `WrappedCommand` runs:
///
/// ```text
/// /usr/bin/sandbox-exec -f <profile_path> -- <cmd.program> <cmd.args...>
/// ```
///
/// Env and cwd are forwarded verbatim from the inner `ToolCommand`. Per
/// `spec/sandbox.md` §3 the wrapping is transparent — the orchestrator
/// is responsible for scrubbing sensitive env vars at a higher layer.
pub(crate) fn build_wrapped(profile_path: &Path, cmd: ToolCommand) -> WrappedCommand {
    let mut args: Vec<String> = Vec::with_capacity(cmd.args.len() + 4);
    args.push("-f".to_owned());
    args.push(profile_path.to_string_lossy().into_owned());
    // The `--` is a convention many *nix tools accept to mark the end
    // of options. `sandbox-exec` itself does not require it, but it
    // future-proofs us against a tool whose name happens to start
    // with `-`.
    args.push("--".to_owned());
    args.push(cmd.program);
    args.extend(cmd.args);

    WrappedCommand {
        program: SANDBOX_EXEC.to_owned(),
        args,
        env: cmd.env,
        cwd: cmd.cwd,
    }
}

/// Convert a [`WrappedCommand`] into a `tokio::process::Command` ready
/// to spawn. Re-exported at crate root so the orchestrator never
/// touches the structural shape directly.
///
/// The returned `Command` does **not** clear the parent environment;
/// `WrappedCommand::env` is layered on top of inherited vars. Stdio is
/// left at its `tokio` default (inherited) — the caller wires
/// stdin/stdout/stderr as it needs.
pub fn wrapped_command_to_tokio(wrapped: WrappedCommand) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(&wrapped.program);
    cmd.args(&wrapped.args);
    for (k, v) in &wrapped.env {
        cmd.env(k, v);
    }
    if let Some(cwd) = wrapped.cwd.as_ref() {
        cmd.current_dir(cwd);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn build_wrapped_prefixes_sandbox_exec_with_profile_flag() {
        let profile = PathBuf::from("/tmp/harness-sandbox/abc.sb");
        let cmd = ToolCommand {
            program: "/bin/echo".into(),
            args: vec!["hello".into(), "world".into()],
            env: HashMap::new(),
            cwd: None,
        };
        let w = build_wrapped(&profile, cmd);
        assert_eq!(w.program, SANDBOX_EXEC);
        // Expect: -f /tmp/.../abc.sb -- /bin/echo hello world
        assert_eq!(w.args[0], "-f");
        assert_eq!(w.args[1], "/tmp/harness-sandbox/abc.sb");
        assert_eq!(w.args[2], "--");
        assert_eq!(w.args[3], "/bin/echo");
        assert_eq!(w.args[4], "hello");
        assert_eq!(w.args[5], "world");
    }

    #[test]
    fn build_wrapped_forwards_env_and_cwd() {
        let mut env = HashMap::new();
        env.insert("FOO".to_owned(), "1".to_owned());
        let cmd = ToolCommand {
            program: "/bin/true".into(),
            args: vec![],
            env,
            cwd: Some(PathBuf::from("/tmp")),
        };
        let w = build_wrapped(Path::new("/tmp/p.sb"), cmd);
        assert_eq!(w.env.get("FOO").map(String::as_str), Some("1"));
        assert_eq!(w.cwd.as_deref(), Some(Path::new("/tmp")));
    }
}
