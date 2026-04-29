//! Composition root: build the router, bind loopback, run with graceful
//! shutdown on SIGTERM (and SIGINT for ergonomic dev runs).
//!
//! Layout choices:
//! * `build_router` is pure (no I/O) so behavior tests can exercise it via
//!   `tower::ServiceExt::oneshot`.
//! * `bind_loopback` returns the OS-chosen port alongside the listener so the
//!   handshake is emitted exactly once before any logs.
//! * `serve` consumes the listener and a shutdown future, allowing tests to
//!   trigger shutdown deterministically.

use std::{future::Future, net::SocketAddr};

use axum::{middleware, Router};
use tokio::net::TcpListener;

use crate::{auth::require_token, routes, state::AppState};

/// Bridge type kept for source compatibility with T1.A callers.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub state: AppState,
}

impl ServerConfig {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

/// Build the axum `Router` for the public HTTP surface, with the
/// `X-Harness-Token` middleware applied to every route.
///
/// PLAN §6 T1.A acceptance: every route is gated by the same token —
/// the loopback bind is defense in depth, not the primary access control.
pub fn build_router(state: AppState) -> Router {
    let token = state.token.clone();

    Router::new()
        .merge(routes::health::router::<AppState>())
        .merge(routes::sandbox_templates::router())
        .merge(routes::conversations::router())
        .merge(routes::providers::router())
        .merge(routes::messages::router())
        .merge(routes::runs::router())
        .merge(routes::settings::router())
        .merge(routes::diagnostics::router())
        .with_state(state)
        .layer(middleware::from_fn_with_state(token, require_token))
}

/// A bound TCP listener plus the loopback address it actually picked. Ports
/// are chosen by the OS (we bind `127.0.0.1:0`) so callers must read the
/// returned `SocketAddr` to learn the port.
#[derive(Debug)]
pub struct BoundServer {
    pub listener: TcpListener,
    pub local_addr: SocketAddr,
}

/// Bind `127.0.0.1` on an OS-chosen port. Errors propagate as-is.
pub async fn bind_loopback() -> std::io::Result<BoundServer> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;
    Ok(BoundServer {
        listener,
        local_addr,
    })
}

/// Run the server until `shutdown` resolves. In-flight requests are allowed
/// to finish; PLAN §6 T1.A allots up to 5s — axum's `with_graceful_shutdown`
/// honours that as long as handlers cooperate.
pub async fn serve<F>(bound: BoundServer, router: Router, shutdown: F) -> std::io::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    axum::serve(bound.listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}
