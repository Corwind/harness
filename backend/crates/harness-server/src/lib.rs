//! `harness-server` — driving HTTP/SSE adapter for Harness.
//!
//! Per PLAN §2.5 / §6 T1.A / T1.L / T1.E this crate is the composition
//! root: it binds the loopback HTTP surface, owns the session-token
//! middleware, and wires every adapter (storage, sandbox, providers,
//! orchestrator, tools) into the request handlers.
//!
//! Mounted endpoints (full v1 surface):
//! * `GET    /v1/health`
//! * `GET    /v1/providers`
//! * `GET    /v1/providers/:id/models`            (409 if not configured)
//! * `POST   /v1/providers/:id/config`
//! * `GET    /v1/conversations`                   (limit + cursor)
//! * `POST   /v1/conversations`
//! * `GET    /v1/conversations/:id`
//! * `PATCH  /v1/conversations/:id`
//! * `DELETE /v1/conversations/:id`               (cascades messages)
//! * `GET    /v1/conversations/:id/messages`      (after_ordinal)
//! * `POST   /v1/conversations/:id/messages`      (mints run_id, returns RunHandle)
//! * `GET    /v1/runs/:run_id/events`             (SSE; Last-Event-ID resume)
//! * `POST   /v1/runs/:run_id/cancel`
//! * `GET    /v1/sandbox-templates`               (+ POST/PATCH/DELETE/validate)
//! * `GET/PATCH /v1/settings`

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod auth;
pub mod bootstrap;
pub mod dto;
pub mod error;
pub mod handshake;
pub mod providers;
pub mod routes;
pub mod runs;
pub mod server;
pub mod state;
pub mod testing;

pub use auth::{SessionToken, TOKEN_HEADER};
pub use bootstrap::seed_builtin_sandbox_templates;
pub use handshake::Handshake;
pub use providers::ProviderRegistry;
pub use runs::{RunRegistry, RUN_RETENTION};
pub use server::{bind_loopback, build_router, serve, BoundServer, ServerConfig};
pub use state::AppState;
pub use testing::FakeProvider;
