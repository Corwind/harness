//! `harness-providers` — central registry for all LLM provider implementations.
//!
//! Each provider lives in its own sibling crate (`harness-providers-claude`,
//! `harness-providers-ollama`, …). This crate owns the registry that the server
//! consults at startup and re-exports the trait from `harness-core` for
//! convenience.
//!
//! Filled in by Phase 1 / track C.

pub use harness_providers_claude as claude;
