//! `harness-providers-claude` — Anthropic Messages API implementation of `LlmProvider`.
//!
//! Streams against the Messages API and translates Anthropic's SSE event names
//! to the canonical `harness_core::ChatEvent` enum so callers see one shape.
//!
//! Filled in by Phase 1 / track C.
