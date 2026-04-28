//! Provider registry — composition-root binding from `ProviderId` to a
//! concrete `LlmProvider` adapter.
//!
//! The orchestrator depends on a single `Arc<dyn LlmProvider>`. To
//! support several providers in v1+ we keep them in this lookup map and
//! resolve per-request based on the conversation's `provider_id`. T1.E
//! ships only Claude in production; tests inject a fake.

use std::collections::HashMap;
use std::sync::Arc;

use harness_core::{ids::ProviderId, LlmProvider};

/// Read-only after construction. Cloning an `Arc<ProviderRegistry>` is
/// the canonical way to share it.
pub struct ProviderRegistry {
    by_id: HashMap<String, Arc<dyn LlmProvider>>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("ids", &self.by_id.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            by_id: HashMap::new(),
        }
    }

    pub fn with(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.by_id.insert(provider.id().to_owned(), provider);
        self
    }

    pub fn get(&self, id: &ProviderId) -> Option<Arc<dyn LlmProvider>> {
        self.by_id.get(id.as_str()).cloned()
    }

    /// All registered providers in deterministic id order.
    pub fn all(&self) -> Vec<Arc<dyn LlmProvider>> {
        let mut keys: Vec<&String> = self.by_id.keys().collect();
        keys.sort();
        keys.into_iter()
            .map(|k| self.by_id.get(k).unwrap().clone())
            .collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
