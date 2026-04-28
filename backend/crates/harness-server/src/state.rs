//! Shared application state injected into axum handlers.
//!
//! All ports live behind `Arc<dyn Trait>` so:
//! * Tests can supply fakes (in-memory repo, fake sandbox runner) without
//!   touching SQLite or `sandbox-exec`.
//! * The composition root (`main`) constructs a single instance per process
//!   and clones it into the router (`Arc` clone is cheap).
//!
//! Future tasks (T1.E) will extend this with `LlmProvider`, `MessageRepo`,
//! `SettingsRepo`, etc. Keeping `AppState` a struct of trait objects (rather
//! than separate `State<...>` extractors per handler) means new ports can be
//! added without touching every signature.

use std::sync::Arc;

use harness_core::{ConversationRepo, SandboxRunner, SandboxTemplateRepo};

use crate::auth::SessionToken;

/// Container for every port a request handler may need.
#[derive(Clone)]
pub struct AppState {
    pub token: SessionToken,
    pub sandbox_templates: Arc<dyn SandboxTemplateRepo>,
    pub conversations: Arc<dyn ConversationRepo>,
    pub sandbox_runner: Arc<dyn SandboxRunner>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}
