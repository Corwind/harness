//! Built-in `echo` tool: an [`ExternalTool`] that runs `/bin/echo` with
//! the model-supplied `text`. Useful as a sandbox/orchestrator
//! smoke-test target.

use async_trait::async_trait;
use harness_core::{ExternalTool, ToolCommand, ToolDescriptor, ToolError, ToolKind};
use serde::{Deserialize, Serialize};

const ECHO_PATH: &str = "/bin/echo";
const SCHEMA: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["text"],
  "properties": {
    "text": { "type": "string" }
  }
}"#;

#[derive(Debug)]
pub struct EchoTool {
    schema: serde_json::Value,
}

impl Default for EchoTool {
    fn default() -> Self {
        Self {
            schema: serde_json::from_str(SCHEMA).expect("static echo schema must parse"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct EchoInput {
    text: String,
}

impl ToolDescriptor for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> Option<&str> {
        Some("Echoes the supplied text back via /bin/echo.")
    }

    fn kind(&self) -> ToolKind {
        ToolKind::External
    }

    fn input_schema(&self) -> &serde_json::Value {
        &self.schema
    }
}

#[async_trait]
impl ExternalTool for EchoTool {
    async fn command(&self, input: &serde_json::Value) -> Result<ToolCommand, ToolError> {
        let parsed: EchoInput = serde_json::from_value(input.clone()).map_err(|e| {
            ToolError::InvalidInput {
                tool: "echo".to_string(),
                message: e.to_string(),
            }
        })?;
        Ok(ToolCommand {
            program: ECHO_PATH.to_string(),
            args: vec![parsed.text],
            env: Default::default(),
            cwd: None,
        })
    }
}
