//! `harness-storage` — SQLite persistence for Harness.
//!
//! Owns the migrations under `./migrations/` and exposes repos for
//! conversations, messages, settings, providers, and sandbox templates.
//!
//! Repos themselves are filled in by Phase 1 (T1.B). Phase 0 (T0.C) only
//! provides the schema and a [`MIGRATOR`] handle to apply it.

/// Embedded SQLx migrator. Applies all migrations under `./migrations/`
/// in order. Idempotent: re-applying after the latest version is a no-op.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
