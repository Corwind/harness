//! `harness-server` binary entry point.
//!
//! On startup:
//! 1. Bind `127.0.0.1:0` (OS picks the port).
//! 2. Generate a session token (`uuid::Uuid::new_v4().simple()`).
//! 3. Print `{"port":N,"token":"..."}` to **stdout** as one line, then flush.
//!    All subsequent diagnostics go to **stderr** so the parent (Swift app)
//!    can read the handshake unambiguously from the first line.
//! 4. Initialise `tracing` against stderr.
//! 5. Run the axum app with graceful shutdown on SIGTERM/SIGINT.

use std::io::Write;

use anyhow::Context;
use harness_server::{bind_loopback, build_router, serve, Handshake, ServerConfig, SessionToken};
use tokio::signal::unix::{signal, SignalKind};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Bind first; if we cannot bind, fail before printing anything.
    let bound = bind_loopback().await.context("bind 127.0.0.1:0")?;
    let port = bound.local_addr.port();

    // 2. Token: 32 hex chars from a v4 UUID is enough entropy and trivial to
    //    transport in a header value.
    let token_str = uuid::Uuid::new_v4().simple().to_string();
    let token = SessionToken::new(token_str.clone());

    // 3. Handshake on stdout, single line, before any other output.
    let handshake = Handshake::new(port, token_str);
    {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{}", handshake.to_json_line()).context("write handshake")?;
        stdout.flush().context("flush handshake")?;
    }

    // 4. Logs to stderr only — never stdout.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    tracing::info!(port = port, "harness-server listening on loopback");

    // 5. Build the router and run with graceful shutdown.
    let router = build_router(ServerConfig::new(token));
    serve(bound, router, shutdown_signal()).await?;
    tracing::info!("harness-server shutdown complete");
    Ok(())
}

/// Future that resolves on the first received SIGTERM or SIGINT.
async fn shutdown_signal() {
    let mut sigterm =
        signal(SignalKind::terminate()).expect("install SIGTERM handler in shutdown_signal");
    let mut sigint =
        signal(SignalKind::interrupt()).expect("install SIGINT handler in shutdown_signal");

    tokio::select! {
        _ = sigterm.recv() => tracing::info!("SIGTERM received, shutting down"),
        _ = sigint.recv()  => tracing::info!("SIGINT received, shutting down"),
    }
}
