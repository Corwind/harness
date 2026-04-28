//! SQLite implementation of `harness_core::repo::ProvidersConfigRepo`.
//!
//! Configs are JSON-encoded then encrypted with ChaCha20-Poly1305 using
//! the `Db`'s [`Secret`]. The on-disk blob is `nonce || ciphertext`.

use async_trait::async_trait;
use chrono::Utc;
use harness_core::error::RepoError;
use harness_core::ids::ProviderId;
use harness_core::repo::ProvidersConfigRepo;
use sqlx::Row;

use crate::cipher::{decrypt, encrypt};
use crate::error_map::{serde_to_repo, sqlx_to_repo};
use crate::Db;

#[derive(Clone, Debug)]
pub struct SqliteProvidersConfigRepo {
    db: Db,
}

impl SqliteProvidersConfigRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    fn decrypt_to_json(&self, blob: &[u8]) -> Result<serde_json::Value, RepoError> {
        let pt = decrypt(self.db.secret(), blob)
            .map_err(|e| RepoError::Storage(format!("decrypt providers_config: {e}")))?;
        serde_json::from_slice(&pt).map_err(serde_to_repo)
    }

    fn encrypt_from_json(&self, value: &serde_json::Value) -> Result<Vec<u8>, RepoError> {
        let pt = serde_json::to_vec(value).map_err(serde_to_repo)?;
        encrypt(self.db.secret(), &pt)
            .map_err(|e| RepoError::Storage(format!("encrypt providers_config: {e}")))
    }
}

#[async_trait]
impl ProvidersConfigRepo for SqliteProvidersConfigRepo {
    async fn get(&self, id: &ProviderId) -> Result<Option<serde_json::Value>, RepoError> {
        let row = sqlx::query("SELECT config_json FROM providers_config WHERE provider_id = ?1")
            .bind(id.as_str())
            .fetch_optional(self.db.pool())
            .await
            .map_err(sqlx_to_repo)?;
        match row {
            None => Ok(None),
            Some(r) => {
                let blob: Vec<u8> = r.try_get("config_json").map_err(sqlx_to_repo)?;
                Ok(Some(self.decrypt_to_json(&blob)?))
            }
        }
    }

    async fn put(&self, id: &ProviderId, config: serde_json::Value) -> Result<(), RepoError> {
        let blob = self.encrypt_from_json(&config)?;
        let now = Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO providers_config (provider_id, config_json, updated_at) \
             VALUES (?1, ?2, ?3) \
             ON CONFLICT(provider_id) DO UPDATE SET \
               config_json = excluded.config_json, updated_at = excluded.updated_at",
        )
        .bind(id.as_str())
        .bind(&blob)
        .bind(now)
        .execute(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?;
        Ok(())
    }

    async fn delete(&self, id: &ProviderId) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM providers_config WHERE provider_id = ?1")
            .bind(id.as_str())
            .execute(self.db.pool())
            .await
            .map_err(sqlx_to_repo)?;
        Ok(())
    }

    async fn list(&self) -> Result<Vec<(ProviderId, serde_json::Value)>, RepoError> {
        let rows = sqlx::query("SELECT provider_id, config_json FROM providers_config")
            .fetch_all(self.db.pool())
            .await
            .map_err(sqlx_to_repo)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let provider_id: String = r.try_get("provider_id").map_err(sqlx_to_repo)?;
            let blob: Vec<u8> = r.try_get("config_json").map_err(sqlx_to_repo)?;
            out.push((ProviderId::from_string(provider_id), self.decrypt_to_json(&blob)?));
        }
        Ok(out)
    }
}
