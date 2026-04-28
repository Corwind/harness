//! SQLite implementation of `harness_core::repo::SandboxTemplateRepo`.
//!
//! Adds a non-port helper [`SqliteSandboxTemplateRepo::seed_builtins`]
//! used by the composition root (typically `harness-server`) to upsert
//! the bundled built-in templates from `harness-sandbox::builtin_templates()`
//! on every startup. It is idempotent: existing rows have their
//! `name`, `description`, `profile`, and `is_builtin` refreshed; new
//! rows are inserted.

use async_trait::async_trait;
use chrono::Utc;
use harness_core::error::RepoError;
use harness_core::ids::SandboxTemplateId;
use harness_core::repo::SandboxTemplateRepo;
use harness_core::sandbox::SandboxTemplate;
use sqlx::Row;

use crate::error_map::sqlx_to_repo;
use crate::Db;

#[derive(Clone, Debug)]
pub struct SqliteSandboxTemplateRepo {
    db: Db,
}

impl SqliteSandboxTemplateRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Upsert a slice of built-in templates. Idempotent: re-running
    /// against the same input is a no-op (besides bumping `updated_at`).
    pub async fn seed_builtins(&self, templates: &[SandboxTemplate]) -> Result<(), RepoError> {
        let now = Utc::now().timestamp();
        let mut tx = self.db.pool().begin().await.map_err(sqlx_to_repo)?;
        for t in templates {
            sqlx::query(
                "INSERT INTO sandbox_templates \
                 (id, name, description, profile, is_builtin, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) \
                 ON CONFLICT(id) DO UPDATE SET \
                   name = excluded.name, \
                   description = excluded.description, \
                   profile = excluded.profile, \
                   is_builtin = excluded.is_builtin, \
                   updated_at = excluded.updated_at",
            )
            .bind(t.id.as_str())
            .bind(&t.name)
            .bind(t.description.as_deref())
            .bind(&t.profile)
            .bind(if t.is_builtin { 1_i64 } else { 0_i64 })
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(sqlx_to_repo)?;
        }
        tx.commit().await.map_err(sqlx_to_repo)?;
        Ok(())
    }
}

fn row_to_template(row: &sqlx::sqlite::SqliteRow) -> Result<SandboxTemplate, RepoError> {
    let id: String = row.try_get("id").map_err(sqlx_to_repo)?;
    let name: String = row.try_get("name").map_err(sqlx_to_repo)?;
    let description: Option<String> = row.try_get("description").map_err(sqlx_to_repo)?;
    let profile: String = row.try_get("profile").map_err(sqlx_to_repo)?;
    let is_builtin: i64 = row.try_get("is_builtin").map_err(sqlx_to_repo)?;
    Ok(SandboxTemplate {
        id: SandboxTemplateId::from_string(id),
        name,
        description,
        profile,
        is_builtin: is_builtin != 0,
    })
}

#[async_trait]
impl SandboxTemplateRepo for SqliteSandboxTemplateRepo {
    async fn create(&self, template: SandboxTemplate) -> Result<SandboxTemplate, RepoError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO sandbox_templates \
             (id, name, description, profile, is_builtin, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        )
        .bind(template.id.as_str())
        .bind(&template.name)
        .bind(template.description.as_deref())
        .bind(&template.profile)
        .bind(if template.is_builtin { 1_i64 } else { 0_i64 })
        .bind(now)
        .execute(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?;
        self.get(&template.id).await
    }

    async fn get(&self, id: &SandboxTemplateId) -> Result<SandboxTemplate, RepoError> {
        let row = sqlx::query(
            "SELECT id, name, description, profile, is_builtin, created_at, updated_at \
             FROM sandbox_templates WHERE id = ?1",
        )
        .bind(id.as_str())
        .fetch_optional(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?
        .ok_or(RepoError::NotFound)?;
        row_to_template(&row)
    }

    async fn list(&self) -> Result<Vec<SandboxTemplate>, RepoError> {
        let rows = sqlx::query(
            "SELECT id, name, description, profile, is_builtin, created_at, updated_at \
             FROM sandbox_templates ORDER BY name ASC",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?;
        rows.iter().map(row_to_template).collect()
    }

    async fn update(&self, template: SandboxTemplate) -> Result<SandboxTemplate, RepoError> {
        // Confirm existence to surface `NotFound` instead of silently
        // succeeding on zero rows updated.
        let _existing = self.get(&template.id).await?;
        let now = Utc::now().timestamp();
        sqlx::query(
            "UPDATE sandbox_templates SET \
                name = ?1, description = ?2, profile = ?3, is_builtin = ?4, updated_at = ?5 \
             WHERE id = ?6",
        )
        .bind(&template.name)
        .bind(template.description.as_deref())
        .bind(&template.profile)
        .bind(if template.is_builtin { 1_i64 } else { 0_i64 })
        .bind(now)
        .bind(template.id.as_str())
        .execute(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?;
        self.get(&template.id).await
    }

    async fn delete(&self, id: &SandboxTemplateId) -> Result<(), RepoError> {
        let res = sqlx::query("DELETE FROM sandbox_templates WHERE id = ?1")
            .bind(id.as_str())
            .execute(self.db.pool())
            .await
            .map_err(sqlx_to_repo)?;
        if res.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }
}
