//! SQLite implementation of `harness_core::repo::SettingsRepo`.

use async_trait::async_trait;
use harness_core::error::RepoError;
use harness_core::repo::SettingsRepo;
use sqlx::Row;

use crate::error_map::{serde_to_repo, sqlx_to_repo};
use crate::Db;

#[derive(Clone, Debug)]
pub struct SqliteSettingsRepo {
    db: Db,
}

impl SqliteSettingsRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SettingsRepo for SqliteSettingsRepo {
    async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, RepoError> {
        let row = sqlx::query("SELECT value_json FROM settings WHERE key = ?1")
            .bind(key)
            .fetch_optional(self.db.pool())
            .await
            .map_err(sqlx_to_repo)?;
        match row {
            None => Ok(None),
            Some(r) => {
                let blob: Vec<u8> = r.try_get("value_json").map_err(sqlx_to_repo)?;
                let v: serde_json::Value =
                    serde_json::from_slice(&blob).map_err(serde_to_repo)?;
                Ok(Some(v))
            }
        }
    }

    async fn put(&self, key: &str, value: serde_json::Value) -> Result<(), RepoError> {
        let blob = serde_json::to_vec(&value).map_err(serde_to_repo)?;
        sqlx::query(
            "INSERT INTO settings (key, value_json) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        )
        .bind(key)
        .bind(&blob)
        .execute(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), RepoError> {
        sqlx::query("DELETE FROM settings WHERE key = ?1")
            .bind(key)
            .execute(self.db.pool())
            .await
            .map_err(sqlx_to_repo)?;
        Ok(())
    }

    async fn all(&self) -> Result<Vec<(String, serde_json::Value)>, RepoError> {
        let rows = sqlx::query("SELECT key, value_json FROM settings")
            .fetch_all(self.db.pool())
            .await
            .map_err(sqlx_to_repo)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in &rows {
            let key: String = r.try_get("key").map_err(sqlx_to_repo)?;
            let blob: Vec<u8> = r.try_get("value_json").map_err(sqlx_to_repo)?;
            let v: serde_json::Value = serde_json::from_slice(&blob).map_err(serde_to_repo)?;
            out.push((key, v));
        }
        Ok(out)
    }
}
