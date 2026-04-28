//! `harness-core` — domain types and ports for the Harness backend.
//!
//! Per PLAN §2.5, this crate is the **hexagonal core**: pure types,
//! errors, and `async-trait` ports. It depends on nothing else in the
//! workspace and performs no I/O. Concrete adapters
//! (`harness-storage`, `harness-providers-*`, `harness-sandbox`,
//! `harness-tools`) implement the ports defined here; the orchestrator
//! and HTTP layer depend only on these traits, never on adapter
//! crates.
//!
//! Module layout mirrors the ports:
//! * [`provider`] — `LlmProvider` and friends
//! * [`tool`] — `ExternalTool`, `InProcessTool`, `ToolCommand`,
//!   `ToolRegistry`, `Tool`
//! * [`run`] — `RunEvent`, `RunStatus`
//! * [`sandbox`] — `SandboxRunner`, `SandboxTemplate`, `WrappedCommand`
//! * [`repo`] — repository ports + persisted entity types
//! * [`secrets`] — `SecretsVault`
//! * [`message`] — `Message`, `Role`, `ContentBlock`
//! * [`chat`] — `ChatRequest`, `ChatEvent`, `StopReason`, `Usage`
//! * [`error`] — `ProviderError`, `ToolError`, `SandboxError`,
//!   `RepoError`, `SecretsError`
//! * [`ids`] — typed ID newtypes

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, rust_2018_idioms)]

pub mod chat;
pub mod error;
pub mod ids;
pub mod message;
pub mod provider;
pub mod repo;
pub mod run;
pub mod sandbox;
pub mod secrets;
pub mod tool;

pub use chat::{ChatEvent, ChatRequest, StopReason, Usage};
pub use error::{ProviderError, RepoError, SandboxError, SecretsError, ToolError};
pub use ids::{ConversationId, MessageId, ProviderId, RunId, SandboxTemplateId};
pub use message::{ContentBlock, ImageSource, Message, Role};
pub use provider::{LlmProvider, ModelInfo, ProviderCapabilities, ProviderConfig};
pub use repo::{
    Conversation, ConversationPatch, ConversationRepo, MessageRepo, NewConversation, NewMessage,
    ProvidersConfigRepo, SandboxTemplateRepo, SettingsRepo, StoredMessage,
};
pub use run::{RunEvent, RunStatus};
pub use sandbox::{SandboxRunner, SandboxTemplate, WrappedCommand};
pub use secrets::SecretsVault;
pub use tool::{
    ExternalTool, InProcessTool, Tool, ToolCommand, ToolDefinition, ToolDescriptor, ToolKind,
    ToolRegistry,
};
