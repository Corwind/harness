//! `harness-tools` — `Tool` trait and built-in tools.
//!
//! Tools come in two kinds:
//! - `ExternalTool`: spawns a subprocess. Wrapped by `harness-sandbox` when invoked
//!   through the orchestrator.
//! - `InProcessTool`: pure Rust function, not sandboxed (visible to the user as such).
//!
//! Filled in by Phase 1 / track D.
