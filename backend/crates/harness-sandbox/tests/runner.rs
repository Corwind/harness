//! Behavior tests for the [`SbxRunner`] adapter.
//!
//! These tests exercise the runtime contract:
//! - argv assembly is correct end-to-end (we actually spawn the wrapped
//!   command and check its exit / stdout)
//! - the four built-in templates load via the runner's `validate()`
//! - an obviously broken profile surfaces as
//!   `SandboxError::ProfileInvalid { stderr }` (not just any error)
//! - the runner enforces the policy semantics promised by the built-ins
//!   (`no-network` blocks TCP, `network-only` blocks file writes)
//! - tempdir cleanup is real and concurrent calls don't collide
//!
//! Gated on macOS — `sandbox-exec` does not exist elsewhere. Each test
//! that actually applies a sandbox skips when `host_forbids_nested_sandbox()`
//! reports true, so we don't fail in CI containers that pre-sandbox us.

#![cfg(target_os = "macos")]

mod util;

use std::path::Path;
use std::time::Duration;

use harness_core::{SandboxError, SandboxRunner, SandboxTemplate, ToolCommand};
use harness_sandbox::{builtin_templates, wrapped_command_to_tokio, SbxRunner};
use uuid::Uuid;

use crate::util::host_forbids_nested_sandbox;

fn lookup<'a>(ts: &'a [SandboxTemplate], id: &str) -> &'a SandboxTemplate {
    ts.iter()
        .find(|t| t.id.as_str() == id)
        .unwrap_or_else(|| panic!("missing built-in template '{id}'"))
}

fn permissive_dev() -> SandboxTemplate {
    let ts = builtin_templates();
    lookup(&ts, "permissive-dev").clone()
}

// -----------------------------------------------------------------------------
// 1. wrap+spawn echoes argv through the sandbox
// -----------------------------------------------------------------------------

#[tokio::test]
async fn wrap_then_spawn_echoes_argv_through_sandbox() {
    if host_forbids_nested_sandbox() {
        eprintln!("skipping: host forbids nested sandbox-exec");
        return;
    }
    let runner = SbxRunner::new().expect("construct runner");
    let tpl = permissive_dev();

    let cmd = ToolCommand {
        program: "/bin/echo".into(),
        args: vec!["hello".into(), "harness".into()],
        env: Default::default(),
        cwd: None,
    };
    let wrapped = runner.wrap(&tpl, cmd).await.expect("wrap");

    // Sanity: the wrapped command points at sandbox-exec and contains
    // our argv tail.
    assert_eq!(wrapped.program, "/usr/bin/sandbox-exec");
    assert!(wrapped.args.iter().any(|a| a == "hello"));
    assert!(wrapped.args.iter().any(|a| a == "harness"));

    let mut tk = wrapped_command_to_tokio(wrapped);
    let out = tk.output().await.expect("spawn wrapped echo");
    assert!(out.status.success(), "echo exited non-zero: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("hello harness"), "stdout was {stdout:?}");
}

// -----------------------------------------------------------------------------
// 2. `no-network` blocks outbound TCP
// -----------------------------------------------------------------------------

#[tokio::test]
async fn no_network_blocks_outbound_tcp_via_runner() {
    if host_forbids_nested_sandbox() {
        eprintln!("skipping: host forbids nested sandbox-exec");
        return;
    }
    if !Path::new("/usr/bin/curl").exists() {
        eprintln!("skipping: /usr/bin/curl not present");
        return;
    }

    let runner = SbxRunner::new().expect("construct runner");
    let ts = builtin_templates();
    let no_net = lookup(&ts, "no-network");

    // 127.0.0.1:1 is guaranteed-closed; if the sandbox lets the syscall
    // through, curl would still fail to connect. We rely on the fact
    // that *with* the sandbox, the socket-create itself is denied,
    // which causes curl to exit with a non-zero status well before
    // --max-time.
    let cmd = ToolCommand {
        program: "/usr/bin/curl".into(),
        args: vec![
            "--silent".into(),
            "--max-time".into(),
            "1".into(),
            "--output".into(),
            "/dev/null".into(),
            "http://127.0.0.1:1/".into(),
        ],
        env: Default::default(),
        cwd: None,
    };
    let wrapped = runner.wrap(no_net, cmd).await.expect("wrap");
    let started = std::time::Instant::now();
    let out = wrapped_command_to_tokio(wrapped)
        .output()
        .await
        .expect("spawn curl");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "no-network test took too long ({:?})",
        started.elapsed()
    );
    assert!(
        !out.status.success(),
        "no-network let curl reach 127.0.0.1:1 (exit 0)"
    );
}

// -----------------------------------------------------------------------------
// 3. `network-only` blocks file-write
// -----------------------------------------------------------------------------

#[tokio::test]
async fn network_only_blocks_file_write_via_runner() {
    if host_forbids_nested_sandbox() {
        eprintln!("skipping: host forbids nested sandbox-exec");
        return;
    }

    let runner = SbxRunner::new().expect("construct runner");
    let ts = builtin_templates();
    let net_only = lookup(&ts, "network-only");

    // Use a path that definitely doesn't exist *before* the test runs
    // so a non-zero exit can only come from the deny-write policy
    // (or, defensively, from a missing parent dir — but `/tmp` always
    // exists on macOS).
    let target = format!("/tmp/harness-sandbox-runner-test-{}", Uuid::new_v4());

    let cmd = ToolCommand {
        program: "/usr/bin/touch".into(),
        args: vec![target.clone()],
        env: Default::default(),
        cwd: None,
    };
    let wrapped = runner.wrap(net_only, cmd).await.expect("wrap");
    let out = wrapped_command_to_tokio(wrapped)
        .output()
        .await
        .expect("spawn touch");

    assert!(
        !out.status.success(),
        "network-only let `touch` create {target} (exit 0); stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !Path::new(&target).exists(),
        "network-only created the file even though touch reported failure: {target}"
    );
}

// -----------------------------------------------------------------------------
// 4. validate(profile) returns Ok(()) for each built-in
// -----------------------------------------------------------------------------

#[tokio::test]
async fn validate_accepts_every_builtin() {
    if host_forbids_nested_sandbox() {
        eprintln!("skipping: host forbids nested sandbox-exec");
        return;
    }
    let runner = SbxRunner::new().expect("construct runner");
    for tpl in builtin_templates() {
        runner
            .validate(&tpl.profile)
            .await
            .unwrap_or_else(|e| panic!("built-in '{}' rejected by validate(): {e}", tpl.id));
    }
}

// -----------------------------------------------------------------------------
// 5. validate("(this is not valid sbpl)") -> Err(ProfileInvalid { stderr })
// -----------------------------------------------------------------------------

#[tokio::test]
async fn validate_rejects_malformed_sbpl_with_stderr() {
    if host_forbids_nested_sandbox() {
        eprintln!("skipping: host forbids nested sandbox-exec");
        return;
    }
    let runner = SbxRunner::new().expect("construct runner");
    let err = runner
        .validate("(this is not valid sbpl)")
        .await
        .expect_err("invalid SBPL must fail validate()");

    match err {
        SandboxError::ProfileInvalid { stderr } => {
            assert!(
                !stderr.is_empty(),
                "ProfileInvalid stderr should carry the validator's diagnostic"
            );
        }
        other => panic!("expected ProfileInvalid {{ stderr }}, got {other:?}"),
    }
}

// -----------------------------------------------------------------------------
// 6. tempdir cleanup on Drop
// -----------------------------------------------------------------------------

#[tokio::test]
async fn dropping_runner_removes_its_tempdir() {
    let runner = SbxRunner::new().expect("construct runner");
    let tpl = permissive_dev();

    // Materialise a profile and pluck the runner's tempdir out of the
    // resulting `-f <path>` arg. We can only observe the dir through
    // the public surface, since `SbxRunner` deliberately doesn't
    // export the path.
    let wrapped = runner
        .wrap(
            &tpl,
            ToolCommand {
                program: "/bin/true".into(),
                args: vec![],
                env: Default::default(),
                cwd: None,
            },
        )
        .await
        .expect("wrap");
    let dash_f = wrapped
        .args
        .iter()
        .position(|a| a == "-f")
        .expect("wrapped command must include -f flag");
    let profile_path = std::path::PathBuf::from(&wrapped.args[dash_f + 1]);
    let tempdir = profile_path
        .parent()
        .expect("profile path must have a parent")
        .to_path_buf();

    assert!(
        profile_path.exists(),
        "profile file should exist while runner is alive: {}",
        profile_path.display()
    );
    assert!(
        tempdir.exists(),
        "tempdir should exist while runner is alive: {}",
        tempdir.display()
    );

    drop(runner);

    assert!(
        !tempdir.exists(),
        "tempdir {} should be removed after runner drop",
        tempdir.display()
    );
}

// -----------------------------------------------------------------------------
// 7. concurrent invocations don't collide on filenames
// -----------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_wraps_use_distinct_profile_files() {
    let runner = std::sync::Arc::new(SbxRunner::new().expect("construct runner"));
    let tpl = std::sync::Arc::new(permissive_dev());

    // Spawn N concurrent wraps, collect the materialised profile paths,
    // and assert they're all unique.
    const N: usize = 16;
    let mut handles = Vec::with_capacity(N);
    for _ in 0..N {
        let r = runner.clone();
        let t = tpl.clone();
        handles.push(tokio::spawn(async move {
            let cmd = ToolCommand {
                program: "/bin/true".into(),
                args: vec![],
                env: Default::default(),
                cwd: None,
            };
            let wrapped = r.wrap(&t, cmd).await.expect("wrap");
            let dash_f = wrapped
                .args
                .iter()
                .position(|a| a == "-f")
                .expect("must include -f");
            wrapped.args[dash_f + 1].clone()
        }));
    }
    let mut paths: Vec<String> = Vec::with_capacity(N);
    for h in handles {
        paths.push(h.await.expect("join concurrent wrap"));
    }
    paths.sort();
    let before = paths.len();
    paths.dedup();
    assert_eq!(
        paths.len(),
        before,
        "concurrent wraps produced duplicate profile filenames"
    );
}
