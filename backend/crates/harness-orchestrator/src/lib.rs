//! `harness-orchestrator` — application layer: drives a conversation turn.
//!
//! Loops `LlmProvider::chat()` ↔ tool execution and emits a unified `RunEvent`
//! stream. Owns cancellation semantics and asks the injected `SandboxRunner`
//! port to wrap every `ExternalTool` invocation.
//!
//! Hexagonal: this crate depends on `harness-core` ports ONLY. The
//! composition root (`harness-server`) injects concrete adapters.
//!
//! Filled in by Phase 1 / track D.
