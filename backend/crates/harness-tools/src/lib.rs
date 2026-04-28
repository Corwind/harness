//! `harness-tools` — built-in tools and the default `ToolRegistry`.
//!
//! Tools come in two kinds (per `harness-core::tool`):
//! - [`harness_core::ExternalTool`]: spawns a subprocess. The
//!   orchestrator wraps these in `sandbox-exec` via the sandbox port.
//! - [`harness_core::InProcessTool`]: pure Rust, not sandboxed.
//!
//! This crate is an adapter over `harness-core`: the orchestrator
//! depends on `ToolRegistry` (the port), never on this crate directly.
//! `harness-server` is the composition root that wires
//! `default_registry()` into the orchestrator at startup.

#![forbid(unsafe_code)]
#![warn(missing_debug_implementations, rust_2018_idioms)]

mod echo;
mod registry;

pub use echo::EchoTool;
pub use registry::InMemoryToolRegistry;

use std::sync::Arc;

use harness_core::ToolRegistry;

/// Build the default registry shipped with the app.
///
/// Currently registers:
/// - `echo` — an [`ExternalTool`](harness_core::ExternalTool) that
///   spawns `/bin/echo` with the model-supplied `text`. Useful as a
///   smoke-test target for sandbox + orchestrator wiring.
pub fn default_registry() -> Arc<dyn ToolRegistry> {
    let mut reg = InMemoryToolRegistry::new();
    reg.register_external(Arc::new(EchoTool::default()));
    Arc::new(reg)
}
