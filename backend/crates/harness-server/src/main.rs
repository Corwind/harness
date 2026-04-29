//! `harness-server` binary entry point.
//!
//! On startup:
//! 1. Bind `127.0.0.1:0` (OS picks the port).
//! 2. Generate a session token (`uuid::Uuid::new_v4().simple()`).
//! 3. Print `{"port":N,"token":"..."}` to **stdout** as one line, then flush.
//!    All subsequent diagnostics go to **stderr** so the parent (Swift app)
//!    can read the handshake unambiguously from the first line.
//! 4. Initialise `tracing` against stderr.
//! 5. Open the SQLite DB (path from `HARNESS_DB_PATH`, key from
//!    `HARNESS_DB_KEY_HEX`), seed built-in sandbox templates (idempotent),
//!    build app state.
//! 6. Run the axum app with graceful shutdown on SIGTERM/SIGINT.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use harness_server::{
    bind_loopback, build_router, serve, AppState, Handshake, LogRing, LogRingLayer, SessionToken,
};
use harness_storage::{Db, Secret};
use tokio::signal::unix::{signal, SignalKind};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const ENV_DB_PATH: &str = "HARNESS_DB_PATH";
const ENV_DB_KEY_HEX: &str = "HARNESS_DB_KEY_HEX";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Bind first; if we cannot bind, fail before printing anything.
    let bound = bind_loopback().await.context("bind 127.0.0.1:0")?;
    let port = bound.local_addr.port();

    // 2. Token: 32 hex chars from a v4 UUID is enough entropy and trivial
    //    to transport in a header value.
    let token_str = uuid::Uuid::new_v4().simple().to_string();
    let token = SessionToken::new(token_str.clone());

    // 3. Handshake on stdout, single line, before any other output.
    let handshake = Handshake::new(port, token_str);
    {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{}", handshake.to_json_line()).context("write handshake")?;
        stdout.flush().context("flush handshake")?;
    }

    // 4. Logs to stderr (unchanged) AND to an in-memory ring that the
    //    Settings UI tails via `/v1/diagnostics/logs`. The two layers
    //    are composed inside a single `Registry` so every event lands
    //    in both sinks.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let logs = Arc::new(LogRing::new());
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false);
    let ring_layer = LogRingLayer::new(logs.clone());
    tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(ring_layer)
        .init();

    tracing::info!(port = port, "harness-server listening on loopback");

    // 5. Open DB and build app state. The Swift parent supplies both the
    //    DB path and the at-rest encryption key; refusing to start without
    //    them keeps the threat model honest.
    let state = acquire_state(token, logs).await?;

    // 5b. Spawn the run-registry reaper. Detached for the lifetime of
    //    the process; aborted implicitly on shutdown via tokio runtime.
    let _reaper = state.runs.clone().spawn_reaper();

    // 6. Build the router and run with graceful shutdown.
    let router = build_router(state);
    serve(bound, router, shutdown_signal()).await?;
    tracing::info!("harness-server shutdown complete");
    Ok(())
}

async fn acquire_state(token: SessionToken, logs: Arc<LogRing>) -> anyhow::Result<AppState> {
    let db_path = std::env::var(ENV_DB_PATH)
        .map(PathBuf::from)
        .map_err(|_| anyhow!("{ENV_DB_PATH} must be set to the SQLite file path"))?;

    let key_hex = std::env::var(ENV_DB_KEY_HEX)
        .map_err(|_| anyhow!("{ENV_DB_KEY_HEX} must be exactly 64 hex characters"))?;
    let secret = Secret::from_hex(&key_hex)
        .ok_or_else(|| anyhow!("{ENV_DB_KEY_HEX} must be exactly 64 hex characters"))?;

    let db = Db::open(&db_path, secret)
        .await
        .with_context(|| format!("open SQLite at {}", db_path.display()))?;

    harness_server::bootstrap::acquire_app_state(db, token, logs)
        .await
        .context("acquire app state")
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
