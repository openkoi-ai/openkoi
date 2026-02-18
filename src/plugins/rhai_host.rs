// src/plugins/rhai_host.rs — Rhai scripting host
//
// Rhai scripts run in a sandboxed interpreter with no I/O by default.
// The host (OpenKoi) exposes specific functions that scripts can call.
// Trust level: Medium — no I/O unless explicitly exposed by host.

use std::path::{Path, PathBuf};

use rhai::{Dynamic, Engine, Scope, AST};

use crate::plugins::hooks::Hook;

// ---------------------------------------------------------------------------
// Configuration for which host APIs to expose
// ---------------------------------------------------------------------------

/// Controls which host functions are exposed to Rhai scripts.
#[derive(Debug, Clone)]
pub struct RhaiExposedFunctions {
    pub allow_log: bool,
    pub allow_http: bool,
    pub allow_memory_search: bool,
    pub allow_send_message: bool,
}

impl Default for RhaiExposedFunctions {
    fn default() -> Self {
        Self {
            allow_log: true,
            allow_http: false,
            allow_memory_search: false,
            allow_send_message: false,
        }
    }
}

// ---------------------------------------------------------------------------
// RhaiHost — the scripting engine
// ---------------------------------------------------------------------------

/// Manages Rhai scripts and provides the execution environment.
pub struct RhaiHost {
    engine: Engine,
    scripts: Vec<LoadedScript>,
}

/// A loaded Rhai script with its metadata.
struct LoadedScript {
    name: String,
    ast: AST,
    hooks: Vec<String>,
}

impl RhaiHost {
    /// Create a new RhaiHost with the given exposed functions.
    pub fn new(exposed: &RhaiExposedFunctions) -> Self {
        let engine = create_rhai_engine(exposed);
        Self {
            engine,
            scripts: Vec::new(),
        }
    }

    /// Create a host with default exposed functions (log only).
    pub fn with_defaults() -> Self {
        Self::new(&RhaiExposedFunctions::default())
    }

    /// Load a Rhai script from a file.
    pub fn load_script(&mut self, path: &Path) -> anyhow::Result<()> {
        let content = std::fs::read_to_string(path)?;
        self.load_script_str(path, &content)
    }

    /// Load a Rhai script from a string (for testing or inline scripts).
    pub fn load_script_str(&mut self, path: &Path, content: &str) -> anyhow::Result<()> {
        let ast = self.engine.compile(content).map_err(|e| {
            anyhow::anyhow!("Failed to compile Rhai script {}: {}", path.display(), e)
        })?;

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Discover which hooks this script defines by checking for functions
        // named after hook points.
        let hooks = discover_hook_functions(&ast);

        tracing::info!(
            "Loaded Rhai script: {} ({} hooks: [{}])",
            name,
            hooks.len(),
            hooks.join(", ")
        );

        self.scripts.push(LoadedScript { name, ast, hooks });
        Ok(())
    }

    /// Run a hook on all loaded scripts that define a function for it.
    pub fn run_hook(&self, hook: &Hook, context: &serde_json::Value) -> anyhow::Result<()> {
        let hook_str = hook.as_str();

        for script in &self.scripts {
            if !script.hooks.iter().any(|h| h == hook_str) {
                continue;
            }

            let mut scope = Scope::new();

            // Pass context as a Rhai Dynamic map
            let context_dynamic = json_to_dynamic(context);

            let result = self.engine.call_fn::<Dynamic>(
                &mut scope,
                &script.ast,
                hook_str,
                (context_dynamic,),
            );

            match result {
                Ok(_) => {
                    tracing::debug!(
                        "Rhai script '{}' hook '{}' executed successfully",
                        script.name,
                        hook_str
                    );
                }
                Err(e) => {
                    // Check if it's a "function not found" error — not a real error
                    let err_str = e.to_string();
                    if err_str.contains("Function not found") {
                        tracing::debug!(
                            "Rhai script '{}' does not define hook '{}'",
                            script.name,
                            hook_str
                        );
                    } else {
                        tracing::warn!(
                            "Rhai script '{}' hook '{}' failed: {}",
                            script.name,
                            hook_str,
                            e
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the number of loaded scripts.
    pub fn script_count(&self) -> usize {
        self.scripts.len()
    }

    /// List loaded script names.
    pub fn script_names(&self) -> Vec<&str> {
        self.scripts.iter().map(|s| s.name.as_str()).collect()
    }

    /// Check if any scripts are loaded.
    pub fn has_scripts(&self) -> bool {
        !self.scripts.is_empty()
    }

    /// Load scripts from config paths.
    pub fn load_from_config(paths: &[String], exposed: &RhaiExposedFunctions) -> Self {
        let mut host = Self::new(exposed);
        let scripts_dir = crate::infra::paths::rhai_scripts_dir();

        for path_str in paths {
            let path = PathBuf::from(path_str);
            let full_path = if path.is_absolute() {
                path
            } else {
                scripts_dir.join(path)
            };

            // Expand ~ in path
            let full_path = if let Some(s) = full_path.to_str() {
                if s.starts_with('~') {
                    let home = crate::infra::paths::dirs_home();
                    PathBuf::from(s.replacen('~', &home.to_string_lossy(), 1))
                } else {
                    full_path
                }
            } else {
                full_path
            };

            match host.load_script(&full_path) {
                Ok(()) => {
                    tracing::info!("Loaded Rhai script: {}", full_path.display());
                }
                Err(e) => {
                    tracing::warn!("Failed to load Rhai script {}: {}", full_path.display(), e);
                }
            }
        }

        host
    }
}

impl Default for RhaiHost {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// ---------------------------------------------------------------------------
// Engine factory
// ---------------------------------------------------------------------------

/// Create a Rhai engine with the specified exposed functions.
///
/// Rhai has no built-in I/O. We expose only what the user configured.
pub fn create_rhai_engine(exposed: &RhaiExposedFunctions) -> Engine {
    let mut engine = Engine::new();

    // Set safety limits
    engine.set_max_expr_depths(64, 32);
    engine.set_max_operations(100_000);
    engine.set_max_string_size(1_048_576); // 1MB
    engine.set_max_array_size(10_000);
    engine.set_max_map_size(10_000);

    // Always expose basic string utilities
    engine.register_fn("to_upper", |s: &str| s.to_uppercase());
    engine.register_fn("to_lower", |s: &str| s.to_lowercase());
    engine.register_fn("trim", |s: &str| s.trim().to_string());
    engine.register_fn("contains", |s: &str, sub: &str| s.contains(sub));

    // Logging (almost always enabled)
    if exposed.allow_log {
        engine.register_fn("log", |msg: &str| {
            tracing::info!(target: "rhai_script", "{}", msg);
        });
        engine.register_fn("log_debug", |msg: &str| {
            tracing::debug!(target: "rhai_script", "{}", msg);
        });
        engine.register_fn("log_warn", |msg: &str| {
            tracing::warn!(target: "rhai_script", "{}", msg);
        });
    }

    // Memory search (if enabled)
    if exposed.allow_memory_search {
        engine.register_fn("search_memory", |_query: &str| -> String {
            // This is a placeholder — the actual implementation would need
            // access to the Store, which requires more complex host binding.
            // For now, return empty results.
            tracing::debug!(target: "rhai_script", "search_memory called (stub)");
            "[]".to_string()
        });
    }

    // Send message (if enabled)
    if exposed.allow_send_message {
        engine.register_fn("send_message", |_app: &str, _msg: &str| -> bool {
            // Placeholder — would need IntegrationRegistry access
            tracing::debug!(target: "rhai_script", "send_message called (stub)");
            false
        });
    }

    // HTTP GET (if enabled — runs synchronously via blocking runtime)
    if exposed.allow_http {
        engine.register_fn("http_get", |_url: &str| -> String {
            // Placeholder — actual impl would need URL pattern filtering
            // like WASM plugins, and a blocking HTTP call
            tracing::debug!(target: "rhai_script", "http_get called (stub)");
            String::new()
        });
    }

    // No filesystem access, no shell exec, no env vars unless explicitly exposed
    engine
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Discover which hook functions a Rhai script defines.
///
/// We check the AST for function definitions whose names match known hook points.
fn discover_hook_functions(ast: &AST) -> Vec<String> {
    let known_hooks = [
        "before_plan",
        "after_plan",
        "before_execute",
        "after_execute",
        "before_evaluate",
        "after_evaluate",
        "on_learning",
        "on_pattern",
        "message_received",
        "message_sending",
    ];

    let mut found = Vec::new();

    // Rhai AST provides access to function definitions
    for func in ast.iter_functions() {
        let name = func.name;
        if known_hooks.contains(&name) {
            found.push(name.to_string());
        }
    }

    found
}

/// Convert a serde_json::Value to a Rhai Dynamic.
fn json_to_dynamic(value: &serde_json::Value) -> Dynamic {
    match value {
        serde_json::Value::Null => Dynamic::UNIT,
        serde_json::Value::Bool(b) => Dynamic::from(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Dynamic::from(i)
            } else if let Some(f) = n.as_f64() {
                Dynamic::from(f)
            } else {
                Dynamic::UNIT
            }
        }
        serde_json::Value::String(s) => Dynamic::from(s.clone()),
        serde_json::Value::Array(arr) => {
            let rhai_arr: Vec<Dynamic> = arr.iter().map(json_to_dynamic).collect();
            Dynamic::from(rhai_arr)
        }
        serde_json::Value::Object(obj) => {
            let mut map = rhai::Map::new();
            for (k, v) in obj {
                map.insert(k.clone().into(), json_to_dynamic(v));
            }
            Dynamic::from(map)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rhai_host_defaults() {
        let host = RhaiHost::with_defaults();
        assert_eq!(host.script_count(), 0);
        assert!(!host.has_scripts());
    }

    #[test]
    fn test_rhai_host_load_script() {
        let mut host = RhaiHost::with_defaults();

        let script = r#"
fn before_execute(ctx) {
    log("before_execute hook called");
}

fn after_execute(ctx) {
    log("after_execute hook called");
}
"#;

        let path = Path::new("test-hooks.rhai");
        host.load_script_str(path, script).unwrap();

        assert_eq!(host.script_count(), 1);
        assert!(host.has_scripts());
        assert_eq!(host.script_names(), vec!["test-hooks"]);
    }

    #[test]
    fn test_rhai_host_run_hook() {
        let mut host = RhaiHost::with_defaults();

        let script = r#"
fn before_execute(ctx) {
    log("hook fired!");
    return true;
}
"#;

        let path = Path::new("test.rhai");
        host.load_script_str(path, script).unwrap();

        let context = serde_json::json!({"task": "test task"});
        let result = host.run_hook(&Hook::BeforeExecute, &context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rhai_host_missing_hook() {
        let mut host = RhaiHost::with_defaults();

        let script = r#"
fn before_execute(ctx) {
    return true;
}
"#;

        let path = Path::new("test.rhai");
        host.load_script_str(path, script).unwrap();

        // Try running a hook the script doesn't define — should succeed silently
        let context = serde_json::json!({});
        let result = host.run_hook(&Hook::AfterExecute, &context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rhai_host_compile_error() {
        let mut host = RhaiHost::with_defaults();

        let script = r#"
fn broken( {
    this is not valid rhai
}
"#;

        let path = Path::new("broken.rhai");
        let result = host.load_script_str(path, script);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_rhai_engine_defaults() {
        let exposed = RhaiExposedFunctions::default();
        let engine = create_rhai_engine(&exposed);

        // Engine should be able to evaluate simple expressions
        let result: i64 = engine.eval("40 + 2").unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_rhai_string_utils() {
        let exposed = RhaiExposedFunctions::default();
        let engine = create_rhai_engine(&exposed);

        let result: String = engine.eval(r#"to_upper("hello")"#).unwrap();
        assert_eq!(result, "HELLO");

        let result: String = engine.eval(r#"to_lower("WORLD")"#).unwrap();
        assert_eq!(result, "world");

        let result: String = engine.eval(r#"trim("  hello  ")"#).unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_json_to_dynamic_primitives() {
        let d = json_to_dynamic(&serde_json::json!(null));
        assert!(d.is_unit());

        let d = json_to_dynamic(&serde_json::json!(true));
        assert!(d.as_bool().unwrap());

        let d = json_to_dynamic(&serde_json::json!(42));
        assert_eq!(d.as_int().unwrap(), 42);

        let d = json_to_dynamic(&serde_json::json!("hello"));
        assert_eq!(d.into_string().unwrap(), "hello");
    }

    #[test]
    fn test_json_to_dynamic_complex() {
        let d = json_to_dynamic(&serde_json::json!({"key": "value", "num": 42}));
        let map = d.cast::<rhai::Map>();
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_exposed_functions_default() {
        let exposed = RhaiExposedFunctions::default();
        assert!(exposed.allow_log);
        assert!(!exposed.allow_http);
        assert!(!exposed.allow_memory_search);
        assert!(!exposed.allow_send_message);
    }

    #[test]
    fn test_discover_hook_functions() {
        let engine = Engine::new();
        let ast = engine
            .compile(
                r#"
fn before_plan(ctx) { }
fn after_execute(ctx) { }
fn custom_function() { }
"#,
            )
            .unwrap();

        let hooks = discover_hook_functions(&ast);
        assert_eq!(hooks.len(), 2);
        assert!(hooks.contains(&"before_plan".to_string()));
        assert!(hooks.contains(&"after_execute".to_string()));
    }

    #[test]
    fn test_load_from_config_nonexistent() {
        let exposed = RhaiExposedFunctions::default();
        let host = RhaiHost::load_from_config(&["/nonexistent/script.rhai".to_string()], &exposed);
        // Should log a warning but not panic
        assert_eq!(host.script_count(), 0);
    }
}
