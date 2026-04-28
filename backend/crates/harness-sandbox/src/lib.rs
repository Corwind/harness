//! `harness-sandbox` — wrap subprocess invocations in macOS `sandbox-exec`.
//!
//! Materialises an SBPL profile to a temp file and builds a
//! `tokio::process::Command` of the form
//!
//!     /usr/bin/sandbox-exec -f <profile-file> -- <argv>
//!
//! Also validates profiles by dry-running `sandbox-exec -f <profile> /usr/bin/true`.
//!
//! Built-in profiles live under `./profiles/`. Filled in by Phase 1 / track K.
