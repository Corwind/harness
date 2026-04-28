//! Shared inline fakes for orchestrator behavior tests.
//!
//! These fakes implement the harness-core ports without pulling in any
//! adapter crate. The orchestrator's hexagonal contract is "depends on
//! ports only"; the tests exercise that by constructing the runner with
//! these inline fakes.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use harness_core::{
    ChatEvent, ChatRequest, ExternalTool, InProcessTool, LlmProvider, ModelInfo, ProviderCapabilities,
    ProviderConfig, ProviderError, SandboxError, SandboxRunner, SandboxTemplate, Tool, ToolCommand,
    ToolDescriptor, ToolError, ToolKind, ToolRegistry, WrappedCommand,
};

/// Provider that returns a different scripted event sequence on each
/// successive `chat()` call. Useful for the round-trip tests where the
/// orchestrator calls back after a tool result.
pub struct FakeProvider {
    scripts: Mutex<Vec<Vec<ChatEvent>>>,
    calls: AtomicUsize,
    /// Captured `ChatRequest` payloads, one per `chat()` call.
    pub requests: Arc<Mutex<Vec<ChatRequest>>>,
}

impl FakeProvider {
    pub fn new(scripts: Vec<Vec<ChatEvent>>) -> Self {
        Self {
            scripts: Mutex::new(scripts),
            calls: AtomicUsize::new(0),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl LlmProvider for FakeProvider {
    fn id(&self) -> &'static str {
        "fake"
    }
    fn display_name(&self) -> &'static str {
        "Fake"
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            tools: true,
            vision: false,
            system_prompt: true,
            max_context_tokens: None,
        }
    }
    async fn list_models(&self, _cfg: &ProviderConfig) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![])
    }
    async fn chat(
        &self,
        _cfg: &ProviderConfig,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, ChatEvent>, ProviderError> {
        self.requests.lock().unwrap().push(request);
        let idx = self.calls.fetch_add(1, Ordering::SeqCst);
        let mut scripts = self.scripts.lock().unwrap();
        if idx >= scripts.len() {
            return Err(ProviderError::Other(format!(
                "fake provider has no script for call #{}",
                idx
            )));
        }
        let events = std::mem::take(&mut scripts[idx]);
        Ok(Box::pin(stream::iter(events)))
    }
}

/// Sandbox runner that records `wrap()` calls and returns a passthrough
/// `WrappedCommand` (program preserved, args preserved). Useful both as
/// the "happy" runner and for asserting fail-closed behavior never
/// reaches it.
#[derive(Default)]
pub struct RecordingSandbox {
    pub wrap_calls: Mutex<Vec<(String, ToolCommand)>>,
}

#[async_trait]
impl SandboxRunner for RecordingSandbox {
    async fn wrap(
        &self,
        template: &SandboxTemplate,
        cmd: ToolCommand,
    ) -> Result<WrappedCommand, SandboxError> {
        self.wrap_calls
            .lock()
            .unwrap()
            .push((template.id.as_str().to_string(), cmd.clone()));
        // Pass-through (we want the inner program to actually run for
        // round-trip tests). A real adapter would wrap with sandbox-exec.
        Ok(WrappedCommand {
            program: cmd.program,
            args: cmd.args,
            env: cmd.env,
            cwd: cmd.cwd,
        })
    }

    async fn validate(&self, _profile: &str) -> Result<(), SandboxError> {
        Ok(())
    }
}

/// Sandbox runner that panics if invoked. Used to prove the fail-closed
/// `NoSandbox` path never spawns or wraps anything.
#[derive(Default)]
pub struct ForbiddenSandbox;

#[async_trait]
impl SandboxRunner for ForbiddenSandbox {
    async fn wrap(
        &self,
        _template: &SandboxTemplate,
        _cmd: ToolCommand,
    ) -> Result<WrappedCommand, SandboxError> {
        panic!("ForbiddenSandbox::wrap must not be called");
    }
    async fn validate(&self, _profile: &str) -> Result<(), SandboxError> {
        panic!("ForbiddenSandbox::validate must not be called");
    }
}

/// In-memory registry built from a list of tools.
pub struct StaticRegistry {
    external: Vec<Arc<dyn ExternalTool>>,
    in_process: Vec<Arc<dyn InProcessTool>>,
}

impl StaticRegistry {
    pub fn new() -> Self {
        Self {
            external: Vec::new(),
            in_process: Vec::new(),
        }
    }
    pub fn with_external(mut self, t: Arc<dyn ExternalTool>) -> Self {
        self.external.push(t);
        self
    }
    #[allow(dead_code)]
    pub fn with_in_process(mut self, t: Arc<dyn InProcessTool>) -> Self {
        self.in_process.push(t);
        self
    }
}

impl ToolRegistry for StaticRegistry {
    fn get(&self, name: &str) -> Option<Tool> {
        if let Some(t) = self.external.iter().find(|t| t.name() == name) {
            return Some(Tool::External(t.clone()));
        }
        if let Some(t) = self.in_process.iter().find(|t| t.name() == name) {
            return Some(Tool::InProcess(t.clone()));
        }
        None
    }
    fn names(&self) -> Vec<String> {
        self.external
            .iter()
            .map(|t| t.name().to_string())
            .chain(self.in_process.iter().map(|t| t.name().to_string()))
            .collect()
    }
}

/// External tool that runs `/bin/echo $text`, matching the real
/// `harness-tools::EchoTool`. Inlined here so tests do not import the
/// adapter crate.
pub struct EchoCmd {
    schema: serde_json::Value,
}

impl Default for EchoCmd {
    fn default() -> Self {
        Self {
            schema: serde_json::json!({
                "type": "object",
                "required": ["text"],
                "properties": {"text": {"type": "string"}}
            }),
        }
    }
}

impl ToolDescriptor for EchoCmd {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> Option<&str> {
        None
    }
    fn kind(&self) -> ToolKind {
        ToolKind::External
    }
    fn input_schema(&self) -> &serde_json::Value {
        &self.schema
    }
}

#[async_trait]
impl ExternalTool for EchoCmd {
    async fn command(&self, input: &serde_json::Value) -> Result<ToolCommand, ToolError> {
        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput {
                tool: "echo".to_string(),
                message: "missing text".to_string(),
            })?;
        Ok(ToolCommand {
            program: "/bin/echo".to_string(),
            args: vec![text.to_string()],
            env: Default::default(),
            cwd: None,
        })
    }
}

/// External tool that *would* spawn /bin/sleep — used to verify that
/// when sandbox is missing we never reach `command()` even.
#[derive(Default)]
pub struct NeverCalledTool {
    pub schema: serde_json::Value,
    pub command_called: Arc<std::sync::atomic::AtomicBool>,
}

impl NeverCalledTool {
    pub fn new() -> Self {
        Self {
            schema: serde_json::json!({"type":"object"}),
            command_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

impl ToolDescriptor for NeverCalledTool {
    fn name(&self) -> &str {
        "never"
    }
    fn description(&self) -> Option<&str> {
        None
    }
    fn kind(&self) -> ToolKind {
        ToolKind::External
    }
    fn input_schema(&self) -> &serde_json::Value {
        &self.schema
    }
}

#[async_trait]
impl ExternalTool for NeverCalledTool {
    async fn command(&self, _input: &serde_json::Value) -> Result<ToolCommand, ToolError> {
        self.command_called
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(ToolCommand {
            program: "/bin/sleep".to_string(),
            args: vec!["10".to_string()],
            env: Default::default(),
            cwd: None,
        })
    }
}

/// Convenience builder for a `ChatRequest` with no tools or messages.
pub fn empty_request(model: &str) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages: Vec::new(),
        system: None,
        tools: Vec::new(),
        max_tokens: None,
        temperature: None,
    }
}

/// Convenience for assembling a sandbox template.
pub fn fake_template(id: &str) -> SandboxTemplate {
    SandboxTemplate {
        id: harness_core::SandboxTemplateId::from_string(id),
        name: "fake".to_string(),
        description: None,
        profile: "(version 1)(allow default)".to_string(),
        is_builtin: false,
    }
}

/// Convenience for a `ProviderConfig`.
pub fn fake_provider_config() -> ProviderConfig {
    ProviderConfig {
        provider_id: harness_core::ProviderId::from_string("fake"),
        config: serde_json::json!({}),
    }
}
