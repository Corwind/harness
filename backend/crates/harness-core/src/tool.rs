//! Tool ports — the contract between the orchestrator and tools that
//! the model may call.
//!
//! Per PLAN §2.4.1, tools split into two kinds:
//!   * `ExternalTool` — describes a subprocess to spawn. The
//!     orchestrator wraps these in `sandbox-exec` via the sandbox port.
//!   * `InProcessTool` — pure-Rust handler. Not sandboxed; surfaced as
//!     such in the UI.

use std::collections::HashMap;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::ToolError;

/// A tool definition advertised to the model on a turn. The schema is
/// arbitrary JSON Schema; providers translate it to their wire format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema describing the tool's input.
    pub input_schema: serde_json::Value,
}

/// Whether a tool runs as an external subprocess or in-process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    External,
    InProcess,
}

/// A subprocess command that an `ExternalTool` wants to run.
///
/// Stays free of any `tokio::process` types so this crate has no
/// runtime-process imports — adapters convert to `tokio::process::Command`.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct ToolCommand {
    /// Absolute path or PATH-resolvable program name.
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
}

/// Common metadata every tool exposes regardless of kind.
pub trait ToolDescriptor: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> Option<&str>;
    fn kind(&self) -> ToolKind;
    /// JSON Schema for the tool's input.
    fn input_schema(&self) -> &serde_json::Value;
}

/// An external tool — describes a subprocess to spawn given the
/// model's input. The orchestrator is responsible for sandboxing and
/// execution.
#[async_trait]
pub trait ExternalTool: ToolDescriptor {
    /// Build the command to execute given the model's parsed input.
    /// May be async to allow lookups (e.g. resolving an argument that
    /// references a stored secret).
    async fn command(&self, input: &serde_json::Value) -> Result<ToolCommand, ToolError>;
}

/// An in-process tool — runs entirely in Rust. The orchestrator never
/// sandboxes these; the UI labels them as such.
#[async_trait]
pub trait InProcessTool: ToolDescriptor {
    /// Execute the tool and return its result as a JSON value.
    async fn run(&self, input: &serde_json::Value) -> Result<serde_json::Value, ToolError>;
}

/// Either kind of tool, paired so the registry can hand a single handle
/// back to the orchestrator without leaking whether sandboxing applies.
pub enum Tool {
    External(std::sync::Arc<dyn ExternalTool>),
    InProcess(std::sync::Arc<dyn InProcessTool>),
}

impl std::fmt::Debug for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tool::External(t) => f
                .debug_struct("Tool::External")
                .field("name", &t.name())
                .finish(),
            Tool::InProcess(t) => f
                .debug_struct("Tool::InProcess")
                .field("name", &t.name())
                .finish(),
        }
    }
}

impl Tool {
    pub fn name(&self) -> &str {
        match self {
            Tool::External(t) => t.name(),
            Tool::InProcess(t) => t.name(),
        }
    }

    pub fn kind(&self) -> ToolKind {
        match self {
            Tool::External(_) => ToolKind::External,
            Tool::InProcess(_) => ToolKind::InProcess,
        }
    }
}

/// Lookup port: name → tool. Adapters (`harness-tools`) implement this;
/// the orchestrator depends only on this trait.
pub trait ToolRegistry: Send + Sync + 'static {
    /// Return the tool registered under `name`, if any.
    fn get(&self, name: &str) -> Option<Tool>;

    /// All registered tool names.
    fn names(&self) -> Vec<String>;
}
