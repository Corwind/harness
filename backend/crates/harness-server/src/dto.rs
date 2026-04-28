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
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SandboxTemplateDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub profile: String,
    pub is_builtin: bool,
    /// `null` until storage exposes timestamps through the port (see module docs).
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl From<SandboxTemplate> for SandboxTemplateDto {
    fn from(t: SandboxTemplate) -> Self {
        Self {
            id: t.id.into_string(),
            name: t.name,
            description: t.description,
            profile: t.profile,
            is_builtin: t.is_builtin,
            created_at: None,
            updated_at: None,
        }
    }
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
