//! Behavior tests for `harness-tools`.

use std::sync::Arc;

use harness_core::{ExternalTool, Tool, ToolKind, ToolRegistry};
use harness_tools::{default_registry, EchoTool, InMemoryToolRegistry};
use pretty_assertions::assert_eq;

#[test]
fn default_registry_contains_echo() {
    let reg = default_registry();
    let names = reg.names();
    assert!(names.contains(&"echo".to_string()));
    let tool = reg.get("echo").expect("echo tool present");
    assert_eq!(tool.name(), "echo");
    assert_eq!(tool.kind(), ToolKind::External);
}

#[test]
fn unknown_tool_is_none() {
    let reg = default_registry();
    assert!(reg.get("does-not-exist").is_none());
}

#[tokio::test]
async fn echo_tool_builds_correct_command() {
    let echo = EchoTool::default();
    let cmd = echo
        .command(&serde_json::json!({"text": "hello world"}))
        .await
        .expect("valid input");
    assert_eq!(cmd.program, "/bin/echo");
    assert_eq!(cmd.args, vec!["hello world".to_string()]);
}

#[tokio::test]
async fn echo_tool_rejects_invalid_input() {
    let echo = EchoTool::default();
    let err = echo.command(&serde_json::json!({})).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("echo"), "error mentions tool name: {}", msg);
}

#[test]
fn register_external_replaces_existing_returns_true() {
    let mut reg = InMemoryToolRegistry::new();
    let first = Arc::new(EchoTool::default()) as Arc<dyn ExternalTool>;
    let second = Arc::new(EchoTool::default()) as Arc<dyn ExternalTool>;
    assert!(!reg.register_external(first));
    assert!(reg.register_external(second));
}

#[test]
fn registry_get_returns_external_kind() {
    let mut reg = InMemoryToolRegistry::new();
    reg.register_external(Arc::new(EchoTool::default()));
    match reg.get("echo") {
        Some(Tool::External(_)) => {}
        other => panic!("expected External, got {:?}", other),
    }
}
