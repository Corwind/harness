//! Repository ports.
//!
//! These traits are storage-agnostic. `harness-storage` (SQLite)
//! implements them; the orchestrator and HTTP layer depend on the
//! traits, never on `sqlx`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::RepoError;
use crate::ids::{ConversationId, MessageId, ProviderId, SandboxTemplateId};
use crate::message::{ContentBlock, Role};
use crate::sandbox::SandboxTemplate;

/// A persisted conversation row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub title: String,
    pub provider_id: ProviderId,
    pub model: String,
    /// Optional attached sandbox template. None = no sandbox; external
    /// tool invocations are refused per fail-closed default.
    #[serde(default)]
    pub sandbox_template_id: Option<SandboxTemplateId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A persisted message row. Ordinal is monotonic per-conversation and
/// is the source of truth for ordering.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: MessageId,
    pub conversation_id: ConversationId,
    pub role: Role,
    pub content: Vec<ContentBlock>,
    pub created_at: DateTime<Utc>,
    pub ordinal: i64,
}

/// Patch payload for `ConversationRepo::update`. Only the fields that
/// are `Some` are applied.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ConversationPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// `Some(None)` clears the attached template; `Some(Some(id))`
    /// sets it; `None` leaves it untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_template_id: Option<Option<SandboxTemplateId>>,
}

/// New-conversation payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewConversation {
    pub title: String,
    pub provider_id: ProviderId,
    pub model: String,
    #[serde(default)]
    pub sandbox_template_id: Option<SandboxTemplateId>,
}

/// New-message payload (without an ordinal — the repo allocates).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NewMessage {
    pub conversation_id: ConversationId,
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

/// CRUD for conversations.
#[async_trait]
pub trait ConversationRepo: Send + Sync + 'static {
    async fn create(&self, new: NewConversation) -> Result<Conversation, RepoError>;
    async fn get(&self, id: &ConversationId) -> Result<Conversation, RepoError>;
    async fn list(&self) -> Result<Vec<Conversation>, RepoError>;
    async fn update(
        &self,
        id: &ConversationId,
        patch: ConversationPatch,
    ) -> Result<Conversation, RepoError>;
    async fn delete(&self, id: &ConversationId) -> Result<(), RepoError>;
}

/// CRUD for messages within a conversation.
#[async_trait]
pub trait MessageRepo: Send + Sync + 'static {
    async fn append(&self, new: NewMessage) -> Result<StoredMessage, RepoError>;
    async fn list(&self, conversation_id: &ConversationId)
        -> Result<Vec<StoredMessage>, RepoError>;
    async fn get(&self, id: &MessageId) -> Result<StoredMessage, RepoError>;
}

/// Key/value settings store. Values are arbitrary JSON.
#[async_trait]
pub trait SettingsRepo: Send + Sync + 'static {
    async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, RepoError>;
    async fn put(&self, key: &str, value: serde_json::Value) -> Result<(), RepoError>;
    async fn delete(&self, key: &str) -> Result<(), RepoError>;
    async fn all(&self) -> Result<Vec<(String, serde_json::Value)>, RepoError>;
}

/// CRUD for stored provider configurations. The repo is responsible
/// for at-rest encryption of the JSON blob using the key handed to it
/// at construction time.
#[async_trait]
pub trait ProvidersConfigRepo: Send + Sync + 'static {
    async fn get(&self, id: &ProviderId) -> Result<Option<serde_json::Value>, RepoError>;
    async fn put(&self, id: &ProviderId, config: serde_json::Value) -> Result<(), RepoError>;
    async fn delete(&self, id: &ProviderId) -> Result<(), RepoError>;
    async fn list(&self) -> Result<Vec<(ProviderId, serde_json::Value)>, RepoError>;
}

/// CRUD for sandbox templates.
#[async_trait]
pub trait SandboxTemplateRepo: Send + Sync + 'static {
    async fn create(&self, template: SandboxTemplate) -> Result<SandboxTemplate, RepoError>;
    async fn get(&self, id: &SandboxTemplateId) -> Result<SandboxTemplate, RepoError>;
    async fn list(&self) -> Result<Vec<SandboxTemplate>, RepoError>;
    async fn update(&self, template: SandboxTemplate) -> Result<SandboxTemplate, RepoError>;
    async fn delete(&self, id: &SandboxTemplateId) -> Result<(), RepoError>;
}
