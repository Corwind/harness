//! `harness-providers-claude` — Anthropic (Claude) Messages API
//! implementation of `harness_core::LlmProvider`.
//!
//! Talks raw HTTPS via `reqwest` (rustls); SSE is decoded with
//! `eventsource-stream`. Translates Anthropic's `message_start` /
//! `content_block_*` / `message_delta` / `message_stop` / `error` SSE
//! events into the canonical `ChatEvent` enum so the orchestrator sees
//! one shape across every provider.
//!
//! Per PLAN R2 there is no third-party Anthropic SDK dependency.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, rust_2018_idioms)]

mod config;
mod provider;
mod translate;
mod wire;

pub use provider::ClaudeProvider;
