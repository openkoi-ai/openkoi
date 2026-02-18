// src/integrations/registry.rs — Integration registry

use std::collections::HashMap;

use crate::integrations::types::Integration;
use crate::provider::ToolDef;

/// Registry of connected integrations.
pub struct IntegrationRegistry {
    integrations: HashMap<String, Box<dyn Integration>>,
}

impl Default for IntegrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl IntegrationRegistry {
    pub fn new() -> Self {
        Self {
            integrations: HashMap::new(),
        }
    }

    /// Register an integration.
    pub fn register(&mut self, integration: Box<dyn Integration>) {
        let id = integration.id().to_string();
        self.integrations.insert(id, integration);
    }

    /// Get an integration by ID.
    pub fn get(&self, id: &str) -> Option<&dyn Integration> {
        self.integrations.get(id).map(|b| b.as_ref())
    }

    /// List all registered integration IDs.
    pub fn list(&self) -> Vec<&str> {
        self.integrations.keys().map(|s| s.as_str()).collect()
    }

    /// Auto-register tools from all connected integrations.
    pub fn all_tools(&self) -> Vec<ToolDef> {
        self.integrations
            .values()
            .flat_map(|i| super::tools::tools_for_integration(i.as_ref()))
            .collect()
    }
}
