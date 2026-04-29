//! Shared application state injected into axum handlers.
//!
//! All ports live behind `Arc<dyn Trait>` so:
//! * Tests can supply fakes (in-memory repo, fake sandbox runner) without
//!   touching SQLite or `sandbox-exec`.
//! * The composition root (`main`) constructs a single instance per process
//!   and clones it into the router (`Arc` clone is cheap).
//!
//! T1.E expands this to carry the orchestrator, the provider registry, and
//! the in-memory run registry. Adding new ports is additive: handlers
//! destructure the fields they need without changing their signatures.

use std::sync::Arc;

use harness_core::{
    ConversationRepo, MessageRepo, ProvidersConfigRepo, SandboxRunner, SandboxTemplateRepo,
    SettingsRepo, ToolRegistry,
};
use harness_orchestrator::Orchestrator;

use crate::{
    auth::SessionToken, diagnostics::LogRing, providers::ProviderRegistry, runs::RunRegistry,
};

/// Container for every port a request handler may need.
#[derive(Clone)]
pub struct AppState {
    pub token: SessionToken,

    // Storage ports
    pub sandbox_templates: Arc<dyn SandboxTemplateRepo>,
    pub conversations: Arc<dyn ConversationRepo>,
    pub messages: Arc<dyn MessageRepo>,
    pub settings: Arc<dyn SettingsRepo>,
    pub providers_config: Arc<dyn ProvidersConfigRepo>,

    // Sandboxing
    pub sandbox_runner: Arc<dyn SandboxRunner>,

    // Orchestration
    pub providers: Arc<ProviderRegistry>,
    pub tools: Arc<dyn ToolRegistry>,
    pub orchestrator: Arc<Orchestrator>,
    pub runs: Arc<RunRegistry>,

    // Diagnostics
    pub logs: Arc<LogRing>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("token", &"<redacted>")
            .field("providers", &self.providers)
            .finish_non_exhaustive()
    }
}
