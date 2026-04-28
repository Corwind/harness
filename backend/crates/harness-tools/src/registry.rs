//! In-memory implementation of [`harness_core::ToolRegistry`].

use std::collections::BTreeMap;
use std::sync::Arc;

use harness_core::{ExternalTool, InProcessTool, Tool, ToolRegistry};

#[derive(Default)]
pub struct InMemoryToolRegistry {
    tools: BTreeMap<String, ToolEntry>,
}

impl std::fmt::Debug for InMemoryToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryToolRegistry")
            .field("names", &self.tools.keys().collect::<Vec<_>>())
            .finish()
    }
}

enum ToolEntry {
    External(Arc<dyn ExternalTool>),
    InProcess(Arc<dyn InProcessTool>),
}

impl InMemoryToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an external (subprocess) tool. Returns `true` if it
    /// replaced an existing entry under the same name.
    pub fn register_external(&mut self, tool: Arc<dyn ExternalTool>) -> bool {
        let name = tool.name().to_string();
        self.tools.insert(name, ToolEntry::External(tool)).is_some()
    }

    /// Register an in-process tool. Returns `true` if it replaced an
    /// existing entry under the same name.
    pub fn register_in_process(&mut self, tool: Arc<dyn InProcessTool>) -> bool {
        let name = tool.name().to_string();
        self.tools.insert(name, ToolEntry::InProcess(tool)).is_some()
    }
}

impl ToolRegistry for InMemoryToolRegistry {
    fn get(&self, name: &str) -> Option<Tool> {
        self.tools.get(name).map(|entry| match entry {
            ToolEntry::External(t) => Tool::External(t.clone()),
            ToolEntry::InProcess(t) => Tool::InProcess(t.clone()),
        })
    }

    fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}
