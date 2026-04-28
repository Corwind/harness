//! `harness-storage` — SQLite persistence for Harness.
//!
//! This crate is a **driven adapter** in the hexagonal layout (PLAN §2.5).
//! It implements the repository ports defined in `harness-core::repo`
//! against a single SQLite file. Owns the migrations under `./migrations/`.
//!
//! Construct a [`Db`] with [`Db::open`], then build any of the SQLite
//! repos against it:
//!
//! - [`SqliteConversationRepo`]
//! - [`SqliteMessageRepo`]
//! - [`SqliteSettingsRepo`]
//! - [`SqliteProvidersConfigRepo`] (encrypted at rest)
//! - [`SqliteSandboxTemplateRepo`]

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, rust_2018_idioms)]

mod cipher;
mod conversations;
mod error_map;
mod messages;
mod providers_config;
mod sandbox_templates;
mod secret;
mod settings;

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

pub use cipher::CipherError;
pub use conversations::SqliteConversationRepo;
pub use messages::SqliteMessageRepo;
pub use providers_config::SqliteProvidersConfigRepo;
pub use sandbox_templates::SqliteSandboxTemplateRepo;
pub use secret::Secret;
pub use settings::SqliteSettingsRepo;

/// Embedded SQLx migrator. Applies all migrations under `./migrations/`
/// in order. Idempotent: re-applying after the latest version is a no-op.
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Connection pool + at-rest encryption key.
///
/// Cloning is cheap — internally, both the pool and the secret are
/// reference-counted.
#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
    secret: Secret,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db").finish_non_exhaustive()
    }
}

/// Failures observable while constructing a [`Db`].
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite open error: {0}")]
    Open(String),
    #[error("migration error: {0}")]
    Migrate(String),
    #[error("pragma error: {0}")]
    Pragma(String),
}

impl Db {
    /// Open or create the SQLite database at `path`, run migrations, and
    /// enable `foreign_keys`. The supplied [`Secret`] becomes the at-rest
    /// encryption key for [`SqliteProvidersConfigRepo`].
    pub async fn open(path: &Path, secret: Secret) -> Result<Self, DbError> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await
            .map_err(|e| DbError::Open(e.to_string()))?;
        MIGRATOR
            .run(&pool)
            .await
            .map_err(|e| DbError::Migrate(e.to_string()))?;
        Ok(Self { pool, secret })
    }

    /// Borrow the underlying pool. Public so tests in this crate can
    /// inspect raw rows; downstream callers should prefer the typed
    /// repos.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Borrow the at-rest encryption key.
    pub(crate) fn secret(&self) -> &Secret {
        &self.secret
    }
}
