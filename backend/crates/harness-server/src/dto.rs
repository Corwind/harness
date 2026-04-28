//! Wire DTOs for the HTTP surface.
//!
//! These mirror `spec/api.openapi.yaml`. We do **not** serialise the domain
//! `harness_core::SandboxTemplate` directly because:
//! * The OpenAPI shape exposes `created_at` / `updated_at`; the domain
//!   type does not carry them yet (the storage repo discards those columns
//!   in `row_to_template`). For now the DTO emits them as `null`; T1.E will
//!   plumb timestamps once the storage trait grows that capability.
//! * Wire shapes evolve independently of the domain.

use chrono::{DateTime, Utc};
use harness_core::{
    ids::{ConversationId, ProviderId, SandboxTemplateId},
    repo::{Conversation, ConversationPatch, NewConversation},
    sandbox::SandboxTemplate,
};
use serde::{Deserialize, Serialize};

/// `SandboxTemplate` shape on the wire.
///
/// `created_at` / `updated_at` are required RFC 3339 strings (matching the
/// OpenAPI `format: date-time`). The domain stores them as i64 epoch
/// seconds; we convert at the edge.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SandboxTemplateDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub profile: String,
    pub is_builtin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<SandboxTemplate> for SandboxTemplateDto {
    fn from(t: SandboxTemplate) -> Self {
        Self {
            id: t.id.into_string(),
            name: t.name,
            description: t.description,
            profile: t.profile,
            is_builtin: t.is_builtin,
            created_at: epoch_to_dt(t.created_at),
            updated_at: epoch_to_dt(t.updated_at),
        }
    }
}

/// Convert epoch seconds to `DateTime<Utc>`. Out-of-range values fall
/// back to the Unix epoch — non-issue in practice because the storage
/// layer always stamps `now()` on insert (see `harness_core::sandbox`
/// docs).
fn epoch_to_dt(secs: i64) -> DateTime<Utc> {
    use chrono::TimeZone;
    Utc.timestamp_opt(secs, 0)
        .single()
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap())
}

/// Body for `POST /v1/sandbox-templates`.
#[derive(Clone, Debug, Deserialize)]
pub struct CreateSandboxTemplateRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub profile: String,
}

/// Body for `PATCH /v1/sandbox-templates/{id}`. All fields optional;
/// missing fields leave the stored value untouched. `description` follows
/// the JSON convention: an explicit `null` clears it; key absent leaves it
/// unchanged. We model that with `serde_with::rust::double_option`-style
/// nesting: `Option<Option<String>>`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PatchSandboxTemplateRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_optional_string")]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub profile: Option<String>,
}

/// Helper: distinguish "key absent" from "key present and null".
///
/// `#[serde(default)]` on the field gives us `None` when the JSON key is
/// missing. When the key *is* present, we deserialise an
/// `Option<String>` (which is `None` for explicit `null`, `Some(s)` for a
/// string) and wrap that in an outer `Some` to signal "key present".
///
/// Result mapping:
///   absent           -> `None`             (don't change the stored value)
///   `null`           -> `Some(None)`       (clear the stored value)
///   `"foo"`          -> `Some(Some("foo"))` (set the stored value)
fn deserialize_optional_optional_string<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

/// Wrapper for list endpoints to match the OpenAPI envelope shape
/// `{"templates": [...]}`.
#[derive(Clone, Debug, Serialize)]
pub struct SandboxTemplatesEnvelope {
    pub templates: Vec<SandboxTemplateDto>,
}

/// Response body for `POST /v1/sandbox-templates/{id}/validate`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ValidateSandboxTemplateResponse {
    pub valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

/// `Conversation` shape on the wire.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConversationDto {
    pub id: String,
    pub title: String,
    pub provider_id: String,
    pub model: String,
    #[serde(default)]
    pub sandbox_template_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Conversation> for ConversationDto {
    fn from(c: Conversation) -> Self {
        Self {
            id: c.id.into_string(),
            title: c.title,
            provider_id: c.provider_id.into_string(),
            model: c.model,
            sandbox_template_id: c.sandbox_template_id.map(|s| s.into_string()),
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

/// Body for `POST /v1/conversations`.
#[derive(Clone, Debug, Deserialize)]
pub struct CreateConversationRequest {
    pub provider_id: String,
    pub model: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub sandbox_template_id: Option<String>,
}

impl CreateConversationRequest {
    pub fn into_new(self) -> NewConversation {
        NewConversation {
            // OpenAPI says title is optional; if absent we fall back to a
            // sensible default rather than fail. The UI can rename later.
            title: self.title.unwrap_or_else(|| "Untitled".to_owned()),
            provider_id: ProviderId::from_string(self.provider_id),
            model: self.model,
            sandbox_template_id: self.sandbox_template_id.map(SandboxTemplateId::from_string),
        }
    }
}

/// Body for `PATCH /v1/conversations/{id}`. `sandbox_template_id` follows
/// the explicit-null convention: `null` clears, absent leaves untouched.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PatchConversationRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_optional_string")]
    pub sandbox_template_id: Option<Option<String>>,
}

impl PatchConversationRequest {
    pub fn into_patch(self) -> ConversationPatch {
        ConversationPatch {
            title: self.title,
            model: self.model,
            sandbox_template_id: self
                .sandbox_template_id
                .map(|inner| inner.map(SandboxTemplateId::from_string)),
        }
    }
}

/// Helper newtype to serialise `ConversationId` paths in tests.
pub fn conv_id(s: impl Into<String>) -> ConversationId {
    ConversationId::from_string(s)
}

// ----------------------------------------------------------------------------
// T1.E DTOs (providers / models / messages / runs / settings).
// ----------------------------------------------------------------------------

use harness_core::{
    provider::{ModelInfo, ProviderCapabilities},
    repo::StoredMessage,
};

/// `GET /v1/providers` envelope.
#[derive(Clone, Debug, Serialize)]
pub struct ProvidersEnvelope {
    pub providers: Vec<ProviderDto>,
}

/// `Provider` shape per OpenAPI.
#[derive(Clone, Debug, Serialize)]
pub struct ProviderDto {
    pub id: String,
    pub display_name: String,
    pub configured: bool,
    pub capabilities: ProviderCapabilitiesDto,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProviderCapabilitiesDto {
    pub streaming: bool,
    pub tools: bool,
    pub vision: bool,
    pub system_prompt: bool,
    pub max_context_tokens: Option<u32>,
}

impl From<ProviderCapabilities> for ProviderCapabilitiesDto {
    fn from(c: ProviderCapabilities) -> Self {
        Self {
            streaming: c.streaming,
            tools: c.tools,
            vision: c.vision,
            system_prompt: c.system_prompt,
            max_context_tokens: c.max_context_tokens,
        }
    }
}

/// `GET /v1/providers/{id}/models` envelope.
#[derive(Clone, Debug, Serialize)]
pub struct ModelsEnvelope {
    pub models: Vec<ModelDto>,
}

/// `Model` shape per OpenAPI.
#[derive(Clone, Debug, Serialize)]
pub struct ModelDto {
    pub id: String,
    pub display_name: String,
    pub context_window: Option<u32>,
    pub supports_tools: Option<bool>,
    pub supports_vision: Option<bool>,
}

impl From<ModelInfo> for ModelDto {
    fn from(m: ModelInfo) -> Self {
        Self {
            id: m.id,
            display_name: m.display_name,
            context_window: m.context_window,
            // The domain `ModelInfo` does not carry per-model capability
            // flags yet — surface them as `null`. Provider-level
            // capabilities are still available via `/v1/providers`.
            supports_tools: None,
            supports_vision: None,
        }
    }
}

/// Body for `POST /v1/providers/{id}/config`. Open-shaped on purpose —
/// the inner JSON is forwarded to the provider adapter.
#[derive(Clone, Debug, Deserialize)]
pub struct UpsertProviderConfigRequest {
    #[serde(flatten)]
    pub config: serde_json::Value,
}

/// Response for `POST /v1/providers/{id}/config`.
#[derive(Clone, Debug, Serialize)]
pub struct ProviderConfigSummaryDto {
    pub provider_id: String,
    pub configured: bool,
    pub updated_at: DateTime<Utc>,
}

/// `GET /v1/conversations` envelope (paginated).
#[derive(Clone, Debug, Serialize)]
pub struct ConversationsEnvelope {
    pub conversations: Vec<ConversationDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// `Message` shape per OpenAPI.
#[derive(Clone, Debug, Serialize)]
pub struct MessageDto {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: Vec<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub ordinal: i64,
}

impl From<StoredMessage> for MessageDto {
    fn from(m: StoredMessage) -> Self {
        let role = match m.role {
            harness_core::Role::User => "user",
            harness_core::Role::Assistant => "assistant",
            harness_core::Role::Tool => "tool",
            harness_core::Role::System => "system",
        };
        // Round-trip through serde_json so the wire shape exactly
        // matches `MessageContentBlock` in the OpenAPI (which mirrors
        // `ContentBlock`'s own serde shape).
        let content = m
            .content
            .into_iter()
            .map(|b| serde_json::to_value(b).expect("ContentBlock serialises infallibly"))
            .collect();
        Self {
            id: m.id.into_string(),
            conversation_id: m.conversation_id.into_string(),
            role: role.to_owned(),
            content,
            created_at: m.created_at,
            ordinal: m.ordinal,
        }
    }
}

/// `GET /v1/conversations/{id}/messages` envelope.
#[derive(Clone, Debug, Serialize)]
pub struct MessagesEnvelope {
    pub messages: Vec<MessageDto>,
}

/// Body for `POST /v1/conversations/{id}/messages`.
#[derive(Clone, Debug, Deserialize)]
pub struct PostMessageRequest {
    pub content: Vec<serde_json::Value>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
}

/// `RunHandle` returned by `POST /v1/conversations/{id}/messages`.
#[derive(Clone, Debug, Serialize)]
pub struct RunHandleDto {
    pub run_id: String,
    pub conversation_id: String,
    pub message_id: String,
}

/// `Settings` shape — free-form `serde_json::Value`. Round-tripped
/// untouched.
pub type SettingsDto = serde_json::Value;
