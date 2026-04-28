//! SQLite implementation of `harness_core::repo::MessageRepo`.
//!
//! Ordinals are allocated inside a single transaction as
//! `COALESCE(MAX(ordinal) + 1, 0)`. This guarantees monotonicity per
//! conversation under concurrent appends because SQLite serialises
//! writes across the database.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use harness_core::error::RepoError;
use harness_core::ids::{ConversationId, MessageId};
use harness_core::message::{ContentBlock, Role};
use harness_core::repo::{MessageRepo, NewMessage, StoredMessage};
use sqlx::Row;
use uuid::Uuid;

use crate::error_map::{serde_to_repo, sqlx_to_repo};
use crate::Db;

#[derive(Clone, Debug)]
pub struct SqliteMessageRepo {
    db: Db,
}

impl SqliteMessageRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

fn role_to_str(r: Role) -> &'static str {
    match r {
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
        Role::System => "system",
    }
}

fn str_to_role(s: &str) -> Result<Role, RepoError> {
    match s {
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool" => Ok(Role::Tool),
        "system" => Ok(Role::System),
        other => Err(RepoError::Serde(format!("unknown role `{other}`"))),
    }
}

fn ts(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0).single().unwrap_or_else(Utc::now)
}

fn row_to_stored(row: &sqlx::sqlite::SqliteRow) -> Result<StoredMessage, RepoError> {
    let id: String = row.try_get("id").map_err(sqlx_to_repo)?;
    let conversation_id: String = row.try_get("conversation_id").map_err(sqlx_to_repo)?;
    let role: String = row.try_get("role").map_err(sqlx_to_repo)?;
    let content_json: Vec<u8> = row.try_get("content_json").map_err(sqlx_to_repo)?;
    let created_at: i64 = row.try_get("created_at").map_err(sqlx_to_repo)?;
    let ordinal: i64 = row.try_get("ordinal").map_err(sqlx_to_repo)?;

    let content: Vec<ContentBlock> = serde_json::from_slice(&content_json).map_err(serde_to_repo)?;

    Ok(StoredMessage {
        id: MessageId::from_string(id),
        conversation_id: ConversationId::from_string(conversation_id),
        role: str_to_role(&role)?,
        content,
        created_at: ts(created_at),
        ordinal,
    })
}

#[async_trait]
impl MessageRepo for SqliteMessageRepo {
    async fn append(&self, new: NewMessage) -> Result<StoredMessage, RepoError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();
        let content_blob = serde_json::to_vec(&new.content).map_err(serde_to_repo)?;
        let role_str = role_to_str(new.role);

        let mut tx = self.db.pool().begin().await.map_err(sqlx_to_repo)?;

        // Allocate the next ordinal under the transaction. SQLite serialises
        // writes, so this is safe under concurrent appends.
        let next_ordinal: i64 = sqlx::query(
            "SELECT COALESCE(MAX(ordinal) + 1, 0) AS next FROM messages WHERE conversation_id = ?1",
        )
        .bind(new.conversation_id.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_to_repo)?
        .try_get("next")
        .map_err(sqlx_to_repo)?;

        sqlx::query(
            "INSERT INTO messages (id, conversation_id, role, content_json, created_at, ordinal) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(&id)
        .bind(new.conversation_id.as_str())
        .bind(role_str)
        .bind(&content_blob)
        .bind(now)
        .bind(next_ordinal)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_to_repo)?;

        tx.commit().await.map_err(sqlx_to_repo)?;

        Ok(StoredMessage {
            id: MessageId::from_string(id),
            conversation_id: new.conversation_id,
            role: new.role,
            content: new.content,
            created_at: ts(now),
            ordinal: next_ordinal,
        })
    }

    async fn list(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<Vec<StoredMessage>, RepoError> {
        let rows = sqlx::query(
            "SELECT id, conversation_id, role, content_json, created_at, ordinal \
             FROM messages WHERE conversation_id = ?1 ORDER BY ordinal ASC",
        )
        .bind(conversation_id.as_str())
        .fetch_all(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?;
        rows.iter().map(row_to_stored).collect()
    }

    async fn get(&self, id: &MessageId) -> Result<StoredMessage, RepoError> {
        let row = sqlx::query(
            "SELECT id, conversation_id, role, content_json, created_at, ordinal \
             FROM messages WHERE id = ?1",
        )
        .bind(id.as_str())
        .fetch_optional(self.db.pool())
        .await
        .map_err(sqlx_to_repo)?
        .ok_or(RepoError::NotFound)?;
        row_to_stored(&row)
    }
}
