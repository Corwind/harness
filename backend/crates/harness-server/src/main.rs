//! `harness-server` — entry point.
//!
//! On startup the server binds 127.0.0.1:0, generates a session token, and
//! prints `{"port": <port>, "token": "<token>"}` to stdout (one line). The
//! parent (Swift app) reads that line and uses it to authenticate every
//! subsequent request via the `X-Harness-Token` header.
//!
//! Filled in by Phase 1 / track A.

fn main() {
    eprintln!("harness-server: skeleton only — see PLAN.md T1.A");
}
