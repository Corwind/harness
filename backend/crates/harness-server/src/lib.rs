//! `harness-server` — driving HTTP/SSE adapter for Harness.
//!
//! Per PLAN §2.5 and §6 T1.A/T1.L, this crate is the composition root: it
//! binds the loopback HTTP surface, owns the session-token middleware, and
//! wires repository / sandbox / (eventually) provider adapters into request
//! handlers.
//!
//! Currently mounted:
//! * `GET /v1/health` (T1.A)
//! * `GET/POST/GET{id}/PATCH/DELETE /v1/sandbox-templates` + `/validate` (T1.L)
//! * `POST /v1/conversations`, `GET/PATCH /v1/conversations/{id}` (T1.L
//!   subset — full conversation/message/run surface lands in T1.E)
//!
//! Behavior tests under `tests/` exercise the router (and a real loopback
//! bind for the e2e smoke test) without spawning the binary.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod auth;
pub mod bootstrap;
pub mod dto;
pub mod error;
pub mod handshake;
pub mod routes;
pub mod server;
pub mod state;

pub use auth::{SessionToken, TOKEN_HEADER};
pub use bootstrap::seed_builtin_sandbox_templates;
pub use handshake::Handshake;
pub use server::{bind_loopback, build_router, serve, BoundServer, ServerConfig};
pub use state::AppState;
