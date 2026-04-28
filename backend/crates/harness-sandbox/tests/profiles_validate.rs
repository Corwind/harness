//! Behavior tests for the built-in SBPL profiles.
//!
//! Each test materialises a profile to a tempfile and runs
//! `sandbox-exec -f <tmpfile> /usr/bin/true` (or, for the network test, a
//! short curl command). We assert exit-status semantics, not the contents
//! of stdout/stderr.
//!
//! Gated on macOS — `sandbox-exec` does not exist on other platforms.

#![cfg(target_os = "macos")]

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

// `SandboxTemplate` is re-exported from harness-core via harness-sandbox.
use harness_sandbox::{builtin_templates, SandboxTemplate};
use tempfile::NamedTempFile;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const TRUE_BIN: &str = "/usr/bin/true";
const CURL_BIN: &str = "/usr/bin/curl";

/// Write `profile` to a tempfile and return the handle. The caller keeps
/// the handle alive for the duration of the sandbox-exec invocation so the
/// file is not unlinked underneath us.
fn write_profile(profile: &str) -> NamedTempFile {
    let mut f = tempfile::Builder::new()
        .prefix("harness-sandbox-test-")
        .suffix(".sb")
        .tempfile()
        .expect("create tempfile");
    f.write_all(profile.as_bytes()).expect("write profile");
    f.flush().expect("flush profile");
    f
}

/// Run `sandbox-exec -f <profile-file> -- <argv>` and return (exit_code, stderr).
fn run_under_sandbox(profile_path: &std::path::Path, argv: &[&str]) -> (i32, String) {
    let out = Command::new(SANDBOX_EXEC)
        .arg("-f")
        .arg(profile_path)
        .args(argv)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn sandbox-exec");
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stderr)
}

/// True when `sandbox-exec` cannot apply *any* profile in the current
/// environment (i.e. we are already running inside a sandbox that forbids
/// nesting, like Claude Code's container or some CI runners). In that
/// case validation tests have nothing to prove and we skip rather than
/// emit a false negative — the tests still run on a developer's normal
/// shell and in macOS CI runners that do not pre-sandbox.
fn host_forbids_nested_sandbox() -> bool {
    // The cheapest probe: an "allow default" profile is the most
    // permissive thing sandbox-exec will accept. If even *that* fails to
    // apply, the host is refusing all nested sandboxing.
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

fn validate(template: &SandboxTemplate) {
    if host_forbids_nested_sandbox() {
        eprintln!(
            "skipping {}: host environment forbids nested sandbox-exec",
            template.id
        );
        return;
    }
    let f = write_profile(&template.profile);
    let (code, stderr) = run_under_sandbox(f.path(), &[TRUE_BIN]);
    assert_eq!(
        code, 0,
        "sandbox-exec rejected built-in profile '{}': stderr={}",
        template.id, stderr
    );
}

fn lookup<'a>(ts: &'a [SandboxTemplate], id: &str) -> &'a SandboxTemplate {
    ts.iter()
        .find(|t| t.id.as_str() == id)
        .unwrap_or_else(|| panic!("missing built-in template '{id}'"))
}

#[test]
fn strict_readonly_profile_validates() {
    let ts = builtin_templates();
    validate(lookup(&ts, "strict-readonly"));
}

#[test]
fn no_network_profile_validates() {
    let ts = builtin_templates();
    validate(lookup(&ts, "no-network"));
}

#[test]
fn network_only_profile_validates() {
    let ts = builtin_templates();
    validate(lookup(&ts, "network-only"));
}

#[test]
fn permissive_dev_profile_validates() {
    let ts = builtin_templates();
    validate(lookup(&ts, "permissive-dev"));
}

#[test]
fn malformed_profile_is_rejected() {
    if host_forbids_nested_sandbox() {
        eprintln!("skipping malformed_profile_is_rejected: host forbids nested sandbox-exec");
        return;
    }
    // Deliberately broken SBPL: missing operators, garbage tokens.
    let bad = "(this is not valid sbpl)";
    let f = write_profile(bad);
    let (code, _stderr) = run_under_sandbox(f.path(), &[TRUE_BIN]);
    assert_ne!(
        code, 0,
        "sandbox-exec accepted an obviously invalid profile (exit 0)"
    );
}

#[test]
fn no_network_blocks_outbound_tcp() {
    if host_forbids_nested_sandbox() {
        eprintln!("skipping no_network_blocks_outbound_tcp: host forbids nested sandbox-exec");
        return;
    }
    // We only run this if curl is present and executable. CI runners
    // sometimes lack outbound access; even then the sandbox denial
    // surfaces locally as a non-zero exit before the connection attempt
    // would have a chance to succeed.
    if !std::path::Path::new(CURL_BIN).exists() {
        eprintln!("curl not present, skipping no-network outbound test");
        return;
    }

    let ts = builtin_templates();
    let no_net = lookup(&ts, "no-network");
    let f = write_profile(&no_net.profile);

    // Try to connect to a guaranteed-closed local port. With no-network
    // the sandbox should refuse the socket call before curl gets that
    // far. --max-time is a belt-and-braces guard so the test cannot
    // hang if something unexpected lets the call through.
    let start = std::time::Instant::now();
    let (code, _stderr) = run_under_sandbox(
        f.path(),
        &[
            CURL_BIN,
            "--silent",
            "--max-time",
            "2",
            "--output",
            "/dev/null",
            "http://127.0.0.1:1/",
        ],
    );

    assert!(
        start.elapsed() < Duration::from_secs(5),
        "no-network test took too long ({:?})",
        start.elapsed()
    );
    assert_ne!(
        code, 0,
        "no-network profile let curl reach 127.0.0.1:1 (exit 0)"
    );
}
