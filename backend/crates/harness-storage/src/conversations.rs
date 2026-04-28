//! SQLite implementation of `harness_core::repo::ConversationRepo`.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use harness_core::error::RepoError;
use harness_core::ids::{ConversationId, ProviderId, SandboxTemplateId};
use harness_core::repo::{Conversation, ConversationPatch, ConversationRepo, NewConversation};
use sqlx::Row;
use uuid::Uuid;

use crate::error_map::sqlx_to_repo;
use crate::Db;

/// SQLite-backed `ConversationRepo`.
#[derive(Clone, Debug)]
pub struct SqliteConversationRepo {
    db: Db,
}

impl SqliteConversationRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

fn now_secs() -> i64 {
    Utc::now().timestamp()
}

fn ts(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).single().unwrap_or_else(Utc::now)
}

fn row_to_conversation(row: &sqlx::sqlite::SqliteRow) -> Result<Conversation, RepoError> {
    let id: String = row.try_get("id").map_err(sqlx_to_repo)?;
    let title: String = row.try_get("title").map_err(sqlx_to_repo)?;
    let provider_id: String = row.try_get("provider_id").map_err(sqlx_to_repo)?;
    let model: String = row.try_get("model").map_err(sqlx_to_repo)?;
    let sandbox_template_id: Option<String> =
        row.try_get("sandbox_template_id").map_err(sqlx_to_repo)?;
    let created_at: i64 = row.try_get("created_at").map_err(sqlx_to_repo)?;
    let updated_at: i64 = row.try_get("updated_at").map_err(sqlx_to_repo)?;
    Ok(Conversation {
        id: ConversationId::from_string(id),
        title,
        provider_id: ProviderId::from_string(provider_id),
        model,
        sandbox_template_id: sandbox_template_id.map(SandboxTemplateId::from_string),
        created_at: ts(created_at),
        updated_at: ts(updated_at),
    })
}

#[async_trait]
impl ConversationRepo for SqliteConversationRepo {
    async fn create(&self, new: NewConversation) -> Result<Conversation, RepoError> {
        let id = Uuid::new_v4().to_string();
        let now = now_secs();
        let sandbox_template_id = new.sandbox_template_id.as_ref().map(|s| s.as_str());

        sqlx::query(
            "INSERT INTO conversations \
             (id, title, provider_id, model, sandbox_template_id, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        )
        .bind(&id)
        .bind(&new.title)
        .bind(new.provider_id.as_str())
        .bind(&new.model)
        .bind(sandbox_template_id)
        .bind(now)
        .execute(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?;

        self.get(&ConversationId::from_string(id)).await
    }

    async fn get(&self, id: &ConversationId) -> Result<Conversation, RepoError> {
        let row = sqlx::query(
            "SELECT id, title, provider_id, model, sandbox_template_id, created_at, updated_at \
             FROM conversations WHERE id = ?1",
        )
        .bind(id.as_str())
        .fetch_optional(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?
        .ok_or(RepoError::NotFound)?;
        row_to_conversation(&row)
    }

    async fn list(&self) -> Result<Vec<Conversation>, RepoError> {
        // Tiebreak on rowid (monotonic insertion order) so ordering is
        // deterministic when two rows share the same `updated_at`
        // second.
        let rows = sqlx::query(
            "SELECT id, title, provider_id, model, sandbox_template_id, created_at, updated_at \
             FROM conversations ORDER BY updated_at DESC, rowid DESC",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?;
        rows.iter().map(row_to_conversation).collect()
    }

    async fn update(
        &self,
        id: &ConversationId,
        patch: ConversationPatch,
    ) -> Result<Conversation, RepoError> {
        // Confirm the row exists up front to surface NotFound cleanly.
        let _existing = self.get(id).await?;

        let mut tx = self.db.pool().begin().await.map_err(sqlx_to_repo)?;

        if let Some(title) = patch.title.as_ref() {
            sqlx::query("UPDATE conversations SET title = ?1 WHERE id = ?2")
                .bind(title)
                .bind(id.as_str())
                .execute(&mut *tx)
                .await
                .map_err(sqlx_to_repo)?;
        }
        if let Some(model) = patch.model.as_ref() {
            sqlx::query("UPDATE conversations SET model = ?1 WHERE id = ?2")
                .bind(model)
                .bind(id.as_str())
                .execute(&mut *tx)
                .await
                .map_err(sqlx_to_repo)?;
        }
        if let Some(sandbox_opt) = patch.sandbox_template_id.as_ref() {
            sqlx::query("UPDATE conversations SET sandbox_template_id = ?1 WHERE id = ?2")
                .bind(sandbox_opt.as_ref().map(|s| s.as_str()))
                .bind(id.as_str())
                .execute(&mut *tx)
                .await
                .map_err(sqlx_to_repo)?;
        }

        sqlx::query("UPDATE conversations SET updated_at = ?1 WHERE id = ?2")
            .bind(now_secs())
            .bind(id.as_str())
            .execute(&mut *tx)
            .await
            .map_err(sqlx_to_repo)?;

        tx.commit().await.map_err(sqlx_to_repo)?;
        self.get(id).await
    }

    async fn delete(&self, id: &ConversationId) -> Result<(), RepoError> {
        let res = sqlx::query("DELETE FROM conversations WHERE id = ?1")
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
