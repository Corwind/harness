//! Shared helpers for integration tests in this crate.
//!
//! Extracted so `profiles_validate.rs` and `runner.rs` agree on
//! exactly what "this host can run nested sandboxes" means.

#![cfg(target_os = "macos")]
#![allow(dead_code)]

use std::io::Write as _;
use std::process::{Command, Stdio};

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
pub const TRUE_BIN: &str = "/usr/bin/true";

/// Write `profile` to a tempfile and return the handle. Caller must
/// keep the handle alive while `sandbox-exec` reads the file.
pub fn write_profile(profile: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new()
        .prefix("harness-sandbox-test-")
        .suffix(".sb")
        .tempfile()
        .expect("create tempfile");
    f.write_all(profile.as_bytes()).expect("write profile");
    f.flush().expect("flush profile");
    f
}

/// True when `sandbox-exec` cannot apply *any* profile in the current
/// environment (i.e. we are already running inside a sandbox that
/// forbids nesting, like Claude Code's container or some CI runners).
/// In that case the runtime tests skip rather than emit a false
/// negative.
pub fn host_forbids_nested_sandbox() -> bool {
    let probe = "(version 1)\n(allow default)\n";
    let f = write_profile(probe);
    let out = Command::new(SANDBOX_EXEC)
        .arg("-f")
        .arg(f.path())
        .arg(TRUE_BIN)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    let Ok(out) = out else { return true };
    let stderr = String::from_utf8_lossy(&out.stderr);
    !out.status.success() && stderr.contains("sandbox_apply")
}
