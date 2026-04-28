//! `harness-server` — driving HTTP/SSE adapter for Harness.
//!
//! Per PLAN §2.5 and §6 T1.A, this crate is the composition root: it binds
//! the loopback HTTP surface, owns the session-token middleware, and (later)
//! wires orchestrator/storage/sandbox/provider adapters into request handlers.
//!
//! T1.A scope: only `/v1/health` is implemented. Remaining endpoints land in
//! T1.E once the orchestrator and storage layers are ready.
//!
//! The crate exposes a small library surface so behavior tests can exercise
//! the router without spawning a process while the binary entry point in
//! `main.rs` stays minimal.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod auth;
pub mod handshake;
pub mod routes;
pub mod server;

pub use auth::{SessionToken, TOKEN_HEADER};
pub use handshake::Handshake;
pub use server::{bind_loopback, build_router, serve, BoundServer, ServerConfig};
