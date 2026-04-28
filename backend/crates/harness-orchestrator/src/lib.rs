//! `harness-orchestrator` — drives a conversation turn.
//!
//! Loops `LlmProvider::chat()` ↔ tool execution; emits a unified `RunEvent`
//! stream consumed by the HTTP/SSE layer. Owns cancellation semantics and
//! ensures every `ExternalTool` invocation is wrapped via `harness-sandbox`.
//!
//! Filled in by Phase 1 / track D.
