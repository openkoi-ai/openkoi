// src/plugins/wasm.rs — WASM plugin runtime (wasmtime)
//
// WASM plugins run in wasmtime with explicit capability grants. A plugin must
// declare what it needs in its TOML manifest, and the user approves on install.
// Trust level: Low — sandboxed, explicit capabilities required.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use wasmtime::*;

use crate::plugins::hooks::Hook;

// ---------------------------------------------------------------------------
// Capability model
// ---------------------------------------------------------------------------

/// What a WASM plugin is allowed to do.
#[derive(Debug, Clone, Default)]
pub struct WasmCapabilities {
    pub filesystem: Vec<FsGrant>,
    pub network: Vec<String>, // URL patterns (e.g. "https://api.example.com/*")
    pub environment: Vec<String>, // Allowed env var names
}

/// A single filesystem grant: a glob pattern with an access mode.
#[derive(Debug, Clone)]
pub struct FsGrant {
    pub pattern: String,
    pub access: FsAccess,
}

/// Read / Write / ReadWrite filesystem access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsAccess {
    Read,
    Write,
    ReadWrite,
}

impl FsGrant {
    /// Parse a capability string like "read:~/.config/my-app/*".
    pub fn parse(s: &str) -> Option<Self> {
        let (access, pattern) = if let Some(rest) = s.strip_prefix("read:") {
            (FsAccess::Read, rest)
        } else if let Some(rest) = s.strip_prefix("write:") {
            (FsAccess::Write, rest)
        } else if let Some(rest) = s.strip_prefix("readwrite:") {
            (FsAccess::ReadWrite, rest)
        } else if let Some(rest) = s.strip_prefix("rw:") {
            (FsAccess::ReadWrite, rest)
        } else {
            // Default to read-only
            (FsAccess::Read, s)
        };

        Some(Self {
            pattern: pattern.to_string(),
            access,
        })
    }

    /// Check if a given path matches this grant.
    pub fn allows(&self, path: &str, write: bool) -> bool {
        // Check access mode
        if write && self.access == FsAccess::Read {
            return false;
        }

        // Expand ~ to home directory
        let expanded = if self.pattern.starts_with('~') {
            let home = crate::infra::paths::dirs_home();
            home.join(self.pattern.trim_start_matches("~/"))
                .to_string_lossy()
                .to_string()
        } else {
            self.pattern.clone()
        };

        // Use glob matching
        if let Ok(pattern) = glob::Pattern::new(&expanded) {
            pattern.matches(path)
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin manifest (plugin.toml)
// ---------------------------------------------------------------------------

/// TOML manifest for a WASM plugin.
#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub capabilities: ManifestCapabilities,
    #[serde(default)]
    pub hooks: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct ManifestCapabilities {
    #[serde(default)]
    pub filesystem: Vec<String>,
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub environment: Vec<String>,
}

impl PluginManifest {
    /// Load a manifest from a TOML file.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let manifest: PluginManifest = toml::from_str(&content)?;
        Ok(manifest)
    }

    /// Convert the declared capabilities into our runtime model.
    pub fn capabilities(&self) -> WasmCapabilities {
        let filesystem = self
            .capabilities
            .filesystem
            .iter()
            .filter_map(|s| FsGrant::parse(s))
            .collect();

        WasmCapabilities {
            filesystem,
            network: self.capabilities.network.clone(),
            environment: self.capabilities.environment.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Sandbox state — passed into wasmtime Store
// ---------------------------------------------------------------------------

/// Per-plugin state held in the wasmtime Store.
pub struct PluginState {
    /// Filtered environment variables (only what the plugin declared).
    pub env: HashMap<String, String>,
    /// Filesystem capabilities for runtime checks.
    pub fs_caps: Vec<FsGrant>,
    /// Network URL patterns the plugin can access.
    pub net_patterns: Vec<String>,
    /// Output buffer for plugin log messages.
    pub log_buffer: Vec<String>,
}

// ---------------------------------------------------------------------------
// WasmPlugin — a loaded and instantiated plugin
// ---------------------------------------------------------------------------

/// A loaded WASM plugin instance.
pub struct WasmPlugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub hooks: Vec<String>,
    store: Store<PluginState>,
    instance: Instance,
}

impl WasmPlugin {
    /// Load a WASM plugin from a `.wasm` file.
    ///
    /// If a `plugin.toml` manifest exists alongside the `.wasm` file, capabilities
    /// are read from it. Otherwise, the plugin runs with zero capabilities.
    pub fn load(wasm_path: &Path) -> anyhow::Result<Self> {
        let wasm_path = wasm_path
            .canonicalize()
            .unwrap_or_else(|_| wasm_path.to_path_buf());

        // Look for a manifest alongside the .wasm file
        let manifest_path = wasm_path.with_extension("toml");
        let alt_manifest = wasm_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("plugin.toml");

        let manifest = if manifest_path.exists() {
            Some(PluginManifest::load(&manifest_path)?)
        } else if alt_manifest.exists() {
            Some(PluginManifest::load(&alt_manifest)?)
        } else {
            None
        };

        let (name, version, description, hooks, caps) = if let Some(ref m) = manifest {
            (
                m.plugin.name.clone(),
                m.plugin.version.clone(),
                m.plugin.description.clone(),
                m.hooks.clone(),
                m.capabilities(),
            )
        } else {
            let stem = wasm_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();
            (
                stem,
                "0.0.0".into(),
                String::new(),
                vec![],
                WasmCapabilities::default(),
            )
        };

        // Read WASM bytes
        let wasm_bytes = std::fs::read(&wasm_path).map_err(|e| {
            anyhow::anyhow!("Failed to read WASM file {}: {}", wasm_path.display(), e)
        })?;

        // Create wasmtime engine and sandbox
        let (store, instance) = WasmSandbox::instantiate(&wasm_bytes, &caps)?;

        Ok(Self {
            name,
            version,
            description,
            hooks,
            store,
            instance,
        })
    }

    /// Check if this plugin subscribes to the given hook.
    pub fn handles_hook(&self, hook: &Hook) -> bool {
        let hook_str = hook.as_str();
        self.hooks.iter().any(|h| h == hook_str)
    }

    /// Run a hook by calling the named exported function.
    ///
    /// The plugin is expected to export a function with the hook name that takes
    /// a single i32 parameter (pointer to context JSON in plugin memory) and
    /// returns an i32 (0 = success, non-zero = error code).
    ///
    /// For simplicity, if the hook function is not exported, we silently skip.
    pub fn run_hook(&mut self, hook: &Hook, _context_json: &str) -> anyhow::Result<()> {
        let hook_name = hook.as_str();

        // Look for the exported function
        let func = match self.instance.get_func(&mut self.store, hook_name) {
            Some(f) => f,
            None => {
                // Plugin doesn't export this hook — not an error
                tracing::debug!(
                    "WASM plugin '{}' does not export hook '{}'",
                    self.name,
                    hook_name
                );
                return Ok(());
            }
        };

        // Call with no arguments, expect no return or an i32
        let mut results = [Val::I32(0)];
        let ty = func.ty(&self.store);

        if ty.params().len() == 0 && ty.results().len() == 0 {
            // No-arg, no-return hook
            func.call(&mut self.store, &[], &mut [])?;
        } else if ty.params().len() == 0 && ty.results().len() == 1 {
            // No-arg, returns i32
            func.call(&mut self.store, &[], &mut results)?;
            if let Val::I32(code) = results[0] {
                if code != 0 {
                    anyhow::bail!(
                        "WASM plugin '{}' hook '{}' returned error code {}",
                        self.name,
                        hook_name,
                        code
                    );
                }
            }
        } else {
            tracing::warn!(
                "WASM plugin '{}' hook '{}' has unsupported signature ({}p/{}r), skipping",
                self.name,
                hook_name,
                ty.params().len(),
                ty.results().len(),
            );
        }

        Ok(())
    }

    /// Drain the log buffer (messages written by the plugin via the `log` host function).
    pub fn drain_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.store.data_mut().log_buffer)
    }
}

impl std::fmt::Debug for WasmPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmPlugin")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("hooks", &self.hooks)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// WasmSandbox — creates the sandboxed wasmtime environment
// ---------------------------------------------------------------------------

pub struct WasmSandbox;

impl WasmSandbox {
    /// Instantiate a WASM module with the given capabilities.
    pub fn instantiate(
        wasm_bytes: &[u8],
        caps: &WasmCapabilities,
    ) -> anyhow::Result<(Store<PluginState>, Instance)> {
        let mut config = wasmtime::Config::new();
        // Enable fuel-based metering for safety (prevent infinite loops)
        config.consume_fuel(true);

        let engine = Engine::new(&config)?;

        // Build the filtered environment
        let filtered_env: HashMap<String, String> = caps
            .environment
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.clone(), v)))
            .collect();

        let state = PluginState {
            env: filtered_env,
            fs_caps: caps.filesystem.clone(),
            net_patterns: caps.network.clone(),
            log_buffer: Vec::new(),
        };

        let mut store = Store::new(&engine, state);

        // Give plugins a fuel budget (prevents infinite loops)
        store.set_fuel(1_000_000)?;

        // Compile the module
        let module = Module::new(&engine, wasm_bytes)?;

        // Create linker with host functions
        let mut linker = Linker::new(&engine);
        Self::link_host_functions(&mut linker, caps)?;

        // Instantiate
        let instance = linker.instantiate(&mut store, &module)?;

        Ok((store, instance))
    }

    /// Link host functions into the linker based on capabilities.
    fn link_host_functions(
        linker: &mut Linker<PluginState>,
        caps: &WasmCapabilities,
    ) -> anyhow::Result<()> {
        // Always provide logging
        linker.func_wrap(
            "env",
            "log",
            |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| {
                let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                if let Some(mem) = memory {
                    let start = ptr as usize;
                    let end = start + len as usize;
                    let data = mem.data(&caller);
                    if end <= data.len() {
                        if let Ok(msg) = std::str::from_utf8(&data[start..end]) {
                            let msg_owned = msg.to_string();
                            caller.data_mut().log_buffer.push(msg_owned.clone());
                            tracing::info!(target: "wasm_plugin", "{}", msg_owned);
                        }
                    }
                }
            },
        )?;

        // Environment variable access
        linker.func_wrap(
            "env",
            "get_env",
            |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| -> i32 {
                let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                if let Some(mem) = memory {
                    let data = mem.data(&caller);
                    let start = ptr as usize;
                    let end = start + len as usize;
                    if end <= data.len() {
                        if let Ok(key) = std::str::from_utf8(&data[start..end]) {
                            if caller.data().env.contains_key(key) {
                                return 1; // found
                            }
                        }
                    }
                }
                0 // not found
            },
        )?;

        // Filesystem access (only if capabilities include it)
        if !caps.filesystem.is_empty() {
            linker.func_wrap(
                "env",
                "fs_read",
                |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| -> i32 {
                    let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                    if let Some(mem) = memory {
                        let data = mem.data(&caller);
                        let start = ptr as usize;
                        let end = start + len as usize;
                        if end <= data.len() {
                            if let Ok(path) = std::str::from_utf8(&data[start..end]) {
                                // Check capabilities
                                let allowed =
                                    caller.data().fs_caps.iter().any(|g| g.allows(path, false));
                                if allowed {
                                    return 1; // allowed
                                }
                                tracing::warn!("WASM plugin denied fs_read to: {}", path);
                            }
                        }
                    }
                    0 // denied
                },
            )?;
        }

        // Network access check (only if capabilities include it)
        if !caps.network.is_empty() {
            linker.func_wrap(
                "env",
                "net_allowed",
                |mut caller: Caller<'_, PluginState>, ptr: i32, len: i32| -> i32 {
                    let memory = caller.get_export("memory").and_then(|e| e.into_memory());
                    if let Some(mem) = memory {
                        let data = mem.data(&caller);
                        let start = ptr as usize;
                        let end = start + len as usize;
                        if end <= data.len() {
                            if let Ok(url) = std::str::from_utf8(&data[start..end]) {
                                let allowed = caller
                                    .data()
                                    .net_patterns
                                    .iter()
                                    .any(|pattern| url_matches_pattern(url, pattern));
                                if allowed {
                                    return 1;
                                }
                                tracing::warn!("WASM plugin denied network access to: {}", url);
                            }
                        }
                    }
                    0
                },
            )?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Plugin manager — manages multiple loaded WASM plugins
// ---------------------------------------------------------------------------

/// Manages all loaded WASM plugins.
pub struct WasmPluginManager {
    plugins: Vec<WasmPlugin>,
}

impl WasmPluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Load WASM plugins from config paths.
    ///
    /// Each path can be absolute or relative. Relative paths are resolved from
    /// the wasm plugins directory (~/.local/share/openkoi/plugins/wasm/).
    pub fn load_from_config(paths: &[String]) -> Self {
        let mut manager = Self::new();
        let wasm_dir = crate::infra::paths::wasm_plugins_dir();

        for path_str in paths {
            let path = PathBuf::from(path_str);
            let full_path = if path.is_absolute() {
                path
            } else {
                wasm_dir.join(path)
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

            match WasmPlugin::load(&full_path) {
                Ok(plugin) => {
                    tracing::info!(
                        "Loaded WASM plugin: {} v{} ({} hooks)",
                        plugin.name,
                        plugin.version,
                        plugin.hooks.len()
                    );
                    manager.plugins.push(plugin);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to load WASM plugin from {}: {}",
                        full_path.display(),
                        e
                    );
                }
            }
        }

        manager
    }

    /// Run a hook on all plugins that subscribe to it.
    pub fn run_hook(&mut self, hook: &Hook, context_json: &str) -> anyhow::Result<()> {
        for plugin in &mut self.plugins {
            if plugin.handles_hook(hook) {
                if let Err(e) = plugin.run_hook(hook, context_json) {
                    tracing::warn!(
                        "WASM plugin '{}' hook '{}' failed: {}",
                        plugin.name,
                        hook.as_str(),
                        e
                    );
                    // Continue with other plugins — one failure shouldn't block all
                }

                // Drain any log messages
                for msg in plugin.drain_logs() {
                    tracing::info!(target: "wasm_plugin", "[{}] {}", plugin.name, msg);
                }
            }
        }
        Ok(())
    }

    /// Get the number of loaded plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }

    /// List loaded plugin names.
    pub fn plugin_names(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.name.as_str()).collect()
    }

    /// Check if any plugins are loaded.
    pub fn has_plugins(&self) -> bool {
        !self.plugins.is_empty()
    }
}

impl Default for WasmPluginManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a URL matches a simple pattern like "https://api.example.com/*".
fn url_matches_pattern(url: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        url.starts_with(prefix)
    } else {
        url == pattern
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fs_grant_parse_read() {
        let grant = FsGrant::parse("read:~/.config/my-app/*").unwrap();
        assert_eq!(grant.access, FsAccess::Read);
        assert_eq!(grant.pattern, "~/.config/my-app/*");
    }

    #[test]
    fn test_fs_grant_parse_write() {
        let grant = FsGrant::parse("write:/tmp/output/*").unwrap();
        assert_eq!(grant.access, FsAccess::Write);
        assert_eq!(grant.pattern, "/tmp/output/*");
    }

    #[test]
    fn test_fs_grant_parse_readwrite() {
        let grant = FsGrant::parse("readwrite:/data/*").unwrap();
        assert_eq!(grant.access, FsAccess::ReadWrite);
        assert_eq!(grant.pattern, "/data/*");
    }

    #[test]
    fn test_fs_grant_parse_rw_shorthand() {
        let grant = FsGrant::parse("rw:/data/*").unwrap();
        assert_eq!(grant.access, FsAccess::ReadWrite);
    }

    #[test]
    fn test_fs_grant_parse_default_read() {
        let grant = FsGrant::parse("/etc/config/*").unwrap();
        assert_eq!(grant.access, FsAccess::Read);
        assert_eq!(grant.pattern, "/etc/config/*");
    }

    #[test]
    fn test_fs_grant_allows_read() {
        let grant = FsGrant {
            pattern: "/tmp/test/*".to_string(),
            access: FsAccess::Read,
        };
        assert!(grant.allows("/tmp/test/foo.txt", false));
        assert!(!grant.allows("/tmp/test/foo.txt", true)); // write denied
        assert!(!grant.allows("/etc/passwd", false)); // wrong path
    }

    #[test]
    fn test_fs_grant_allows_readwrite() {
        let grant = FsGrant {
            pattern: "/tmp/test/*".to_string(),
            access: FsAccess::ReadWrite,
        };
        assert!(grant.allows("/tmp/test/foo.txt", false));
        assert!(grant.allows("/tmp/test/foo.txt", true));
    }

    #[test]
    fn test_url_matches_pattern() {
        assert!(url_matches_pattern(
            "https://api.example.com/v1/data",
            "https://api.example.com/*"
        ));
        assert!(!url_matches_pattern(
            "https://evil.com/v1/data",
            "https://api.example.com/*"
        ));
        assert!(url_matches_pattern(
            "https://exact.com/path",
            "https://exact.com/path"
        ));
    }

    #[test]
    fn test_plugin_manifest_parse() {
        let toml_str = r#"
hooks = ["before_execute", "after_execute"]

[plugin]
name = "test-plugin"
version = "0.1.0"
description = "A test plugin"

[capabilities]
filesystem = ["read:~/.config/test/*"]
network = ["https://api.test.com/*"]
environment = ["TEST_TOKEN"]
"#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.plugin.name, "test-plugin");
        assert_eq!(manifest.plugin.version, "0.1.0");
        assert_eq!(manifest.hooks.len(), 2);
        assert_eq!(manifest.capabilities.filesystem.len(), 1);
        assert_eq!(manifest.capabilities.network.len(), 1);
        assert_eq!(manifest.capabilities.environment.len(), 1);

        let caps = manifest.capabilities();
        assert_eq!(caps.filesystem.len(), 1);
        assert_eq!(caps.filesystem[0].access, FsAccess::Read);
    }

    #[test]
    fn test_wasm_capabilities_default() {
        let caps = WasmCapabilities::default();
        assert!(caps.filesystem.is_empty());
        assert!(caps.network.is_empty());
        assert!(caps.environment.is_empty());
    }

    #[test]
    fn test_wasm_plugin_manager_empty() {
        let manager = WasmPluginManager::new();
        assert_eq!(manager.plugin_count(), 0);
        assert!(!manager.has_plugins());
        assert!(manager.plugin_names().is_empty());
    }

    #[test]
    fn test_wasm_plugin_manager_load_nonexistent() {
        let manager =
            WasmPluginManager::load_from_config(&["/nonexistent/plugin.wasm".to_string()]);
        // Should log a warning but not panic
        assert_eq!(manager.plugin_count(), 0);
    }
}
