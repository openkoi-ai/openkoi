// src/plugins/hooks.rs — Hook execution engine
//
// Dispatches lifecycle hooks to both WASM plugins and Rhai scripts.
// Each hook point represents a stage in the agent lifecycle where
// plugins can observe or modify behavior.

use crate::plugins::rhai_host::RhaiHost;
use crate::plugins::wasm::WasmPluginManager;

/// Hook points in the agent lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Hook {
    BeforePlan,
    AfterPlan,
    BeforeExecute,
    AfterExecute,
    BeforeEvaluate,
    AfterEvaluate,
    OnLearning,
    OnPattern,
    MessageReceived,
    MessageSending,
}

impl Hook {
    pub fn as_str(&self) -> &str {
        match self {
            Self::BeforePlan => "before_plan",
            Self::AfterPlan => "after_plan",
            Self::BeforeExecute => "before_execute",
            Self::AfterExecute => "after_execute",
            Self::BeforeEvaluate => "before_evaluate",
            Self::AfterEvaluate => "after_evaluate",
            Self::OnLearning => "on_learning",
            Self::OnPattern => "on_pattern",
            Self::MessageReceived => "message_received",
            Self::MessageSending => "message_sending",
        }
    }

    /// All known hook variants.
    pub fn all() -> &'static [Hook] {
        &[
            Hook::BeforePlan,
            Hook::AfterPlan,
            Hook::BeforeExecute,
            Hook::AfterExecute,
            Hook::BeforeEvaluate,
            Hook::AfterEvaluate,
            Hook::OnLearning,
            Hook::OnPattern,
            Hook::MessageReceived,
            Hook::MessageSending,
        ]
    }
}

impl std::fmt::Display for Hook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ---------------------------------------------------------------------------
// HookExecutor — dispatches hooks to WASM and Rhai
// ---------------------------------------------------------------------------

/// The central hook execution engine.
///
/// Holds references to both the WASM plugin manager and the Rhai scripting host,
/// and dispatches hook calls to both in order: Rhai first (lightweight), then WASM.
pub struct HookExecutor {
    wasm: Option<WasmPluginManager>,
    rhai: Option<RhaiHost>,
}

impl HookExecutor {
    /// Create a new HookExecutor with the given plugin managers.
    pub fn new(wasm: Option<WasmPluginManager>, rhai: Option<RhaiHost>) -> Self {
        Self { wasm, rhai }
    }

    /// Create an empty executor with no plugins.
    pub fn empty() -> Self {
        Self {
            wasm: None,
            rhai: None,
        }
    }

    /// Fire a hook with the given context.
    ///
    /// Dispatches to Rhai scripts first (lightweight, synchronous), then WASM plugins.
    /// Errors from individual plugins are logged but do not stop other plugins from running.
    pub fn fire(&mut self, hook: &Hook, context: &serde_json::Value) {
        tracing::debug!("Firing hook: {}", hook.as_str());

        // 1. Rhai scripts (medium trust, fast)
        if let Some(ref rhai_host) = self.rhai {
            if let Err(e) = rhai_host.run_hook(hook, context) {
                tracing::warn!("Rhai hook '{}' error: {}", hook.as_str(), e);
            }
        }

        // 2. WASM plugins (low trust, sandboxed)
        if let Some(ref mut wasm_mgr) = self.wasm {
            let context_json = serde_json::to_string(context).unwrap_or_default();
            if let Err(e) = wasm_mgr.run_hook(hook, &context_json) {
                tracing::warn!("WASM hook '{}' error: {}", hook.as_str(), e);
            }
        }
    }

    /// Fire a hook with a simple string context (convenience method).
    pub fn fire_simple(&mut self, hook: &Hook, message: &str) {
        let context = serde_json::json!({
            "message": message,
        });
        self.fire(hook, &context);
    }

    /// Check if any plugins are loaded (either WASM or Rhai).
    pub fn has_plugins(&self) -> bool {
        let has_wasm = self.wasm.as_ref().map_or(false, |w| w.has_plugins());
        let has_rhai = self.rhai.as_ref().map_or(false, |r| r.has_scripts());
        has_wasm || has_rhai
    }

    /// Get a summary of loaded plugins for status display.
    pub fn status_summary(&self) -> String {
        let wasm_count = self.wasm.as_ref().map_or(0, |w| w.plugin_count());
        let rhai_count = self.rhai.as_ref().map_or(0, |r| r.script_count());

        if wasm_count == 0 && rhai_count == 0 {
            "No plugins loaded".into()
        } else {
            let mut parts = Vec::new();
            if wasm_count > 0 {
                parts.push(format!("{} WASM", wasm_count));
            }
            if rhai_count > 0 {
                parts.push(format!("{} Rhai", rhai_count));
            }
            parts.join(", ")
        }
    }

    /// Get mutable access to the WASM manager (for direct operations).
    pub fn wasm_mut(&mut self) -> Option<&mut WasmPluginManager> {
        self.wasm.as_mut()
    }

    /// Get reference to the Rhai host (for direct operations).
    pub fn rhai(&self) -> Option<&RhaiHost> {
        self.rhai.as_ref()
    }
}

impl Default for HookExecutor {
    fn default() -> Self {
        Self::empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_as_str() {
        assert_eq!(Hook::BeforePlan.as_str(), "before_plan");
        assert_eq!(Hook::AfterExecute.as_str(), "after_execute");
        assert_eq!(Hook::MessageSending.as_str(), "message_sending");
    }

    #[test]
    fn test_hook_all_variants() {
        let all = Hook::all();
        assert_eq!(all.len(), 10);
    }

    #[test]
    fn test_hook_display() {
        assert_eq!(format!("{}", Hook::OnLearning), "on_learning");
    }

    #[test]
    fn test_hook_executor_empty() {
        let executor = HookExecutor::empty();
        assert!(!executor.has_plugins());
        assert_eq!(executor.status_summary(), "No plugins loaded");
    }

    #[test]
    fn test_hook_executor_fire_empty() {
        let mut executor = HookExecutor::empty();
        // Should not panic even with no plugins
        executor.fire(&Hook::BeforePlan, &serde_json::json!({}));
        executor.fire_simple(&Hook::AfterExecute, "test");
    }

    #[test]
    fn test_hook_executor_with_rhai() {
        let mut rhai = RhaiHost::with_defaults();
        let script = r#"
fn before_execute(ctx) {
    log("hook fired from rhai!");
}
"#;
        rhai.load_script_str(std::path::Path::new("test.rhai"), script)
            .unwrap();

        let mut executor = HookExecutor::new(None, Some(rhai));
        assert!(executor.has_plugins());
        assert_eq!(executor.status_summary(), "1 Rhai");

        // Fire the hook — should not error
        executor.fire(&Hook::BeforeExecute, &serde_json::json!({"task": "test"}));
    }

    #[test]
    fn test_hook_executor_with_wasm_empty() {
        let wasm = WasmPluginManager::new();
        let executor = HookExecutor::new(Some(wasm), None);
        assert!(!executor.has_plugins()); // No actual plugins loaded
        assert_eq!(executor.status_summary(), "No plugins loaded");
    }

    #[test]
    fn test_hook_executor_status_summary() {
        let mut rhai = RhaiHost::with_defaults();
        let script = r#"fn before_plan(ctx) { }"#;
        rhai.load_script_str(std::path::Path::new("a.rhai"), script)
            .unwrap();
        rhai.load_script_str(std::path::Path::new("b.rhai"), script)
            .unwrap();

        let executor = HookExecutor::new(None, Some(rhai));
        assert_eq!(executor.status_summary(), "2 Rhai");
    }
}
