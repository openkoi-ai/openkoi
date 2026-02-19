// src/infra/config.rs — Configuration loading (TOML)

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::infra::paths;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub models: ModelsConfig,

    #[serde(default)]
    pub iteration: IterationConfig,

    #[serde(default)]
    pub safety: SafetyConfig,

    #[serde(default)]
    pub patterns: PatternsConfig,

    #[serde(default)]
    pub memory: MemoryConfig,

    #[serde(default)]
    pub plugins: PluginsConfig,

    #[serde(default)]
    pub integrations: IntegrationsConfig,

    /// Daemon-specific settings (optional section in config.toml).
    #[serde(default)]
    pub daemon: Option<DaemonTomlConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelsConfig {
    pub executor: Option<String>,
    pub evaluator: Option<String>,
    pub planner: Option<String>,
    pub embedder: Option<String>,
    #[serde(default)]
    pub fallback: FallbackConfig,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self {
            executor: None,
            evaluator: None,
            planner: None,
            embedder: Some("openai/text-embedding-3-small".into()),
            fallback: FallbackConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FallbackConfig {
    #[serde(default)]
    pub executor: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationConfig {
    pub max_iterations: u8,
    pub quality_threshold: f32,
    pub improvement_threshold: f32,
    pub timeout_seconds: u64,
    pub token_budget: u32,
    pub skip_eval_confidence: f32,
}

impl Default for IterationConfig {
    fn default() -> Self {
        Self {
            max_iterations: 3,
            quality_threshold: 0.8,
            improvement_threshold: 0.05,
            timeout_seconds: 300,
            token_budget: 200_000,
            skip_eval_confidence: 0.95,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    pub max_cost_usd: f64,
    pub abort_on_regression: bool,
    pub regression_threshold: f32,
    #[serde(default)]
    pub tool_loop: ToolLoopConfig,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            max_cost_usd: 2.0,
            abort_on_regression: true,
            regression_threshold: 0.2,
            tool_loop: ToolLoopConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolLoopConfig {
    pub warning: u32,
    pub critical: u32,
    pub circuit_breaker: u32,
}

impl Default for ToolLoopConfig {
    fn default() -> Self {
        Self {
            warning: 10,
            critical: 20,
            circuit_breaker: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternsConfig {
    pub enabled: bool,
    pub mine_interval_hours: u32,
    pub min_confidence: f32,
    pub min_samples: u32,
    pub auto_propose: bool,
}

impl Default for PatternsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mine_interval_hours: 24,
            min_confidence: 0.7,
            min_samples: 3,
            auto_propose: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub compaction: bool,
    pub learning_decay_rate: f32,
    pub max_storage_mb: u32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            compaction: true,
            learning_decay_rate: 0.05,
            max_storage_mb: 500,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(default)]
    pub wasm: Vec<String>,
    #[serde(default)]
    pub scripts: Vec<String>,
    #[serde(default)]
    pub mcp: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default = "default_transport")]
    pub transport: String,
}

fn default_transport() -> String {
    "stdio".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntegrationsConfig {
    pub slack: Option<IntegrationEntry>,
    pub notion: Option<IntegrationEntry>,
    pub imessage: Option<IntegrationEntry>,
    pub telegram: Option<IntegrationEntry>,
    pub discord: Option<IntegrationEntry>,
    pub msteams: Option<IntegrationEntry>,
    pub google_sheets: Option<IntegrationEntry>,
    pub email: Option<IntegrationEntry>,
    pub msoffice: Option<MsOfficeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationEntry {
    pub enabled: bool,
    #[serde(default)]
    pub channels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsOfficeConfig {
    pub enabled: bool,
    /// Base directory for Office files (defaults to ~/Documents)
    #[serde(default)]
    pub base_dir: Option<String>,
}

/// Optional `[daemon]` section in config.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonTomlConfig {
    /// Whether to auto-execute tasks when the bot is mentioned.
    /// Defaults to `true` when the section is present.
    #[serde(default = "default_true")]
    pub auto_execute: bool,
}

fn default_true() -> bool {
    true
}

impl Config {
    /// Load config from file, falling back to defaults.
    pub fn load() -> anyhow::Result<Self> {
        let path = paths::config_file_path();
        if path.exists() {
            Self::load_from(&path)
        } else {
            Ok(Self::default())
        }
    }

    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_reasonable() {
        let c = Config::default();
        assert_eq!(c.iteration.max_iterations, 3);
        assert!((c.iteration.quality_threshold - 0.8).abs() < 0.001);
        assert_eq!(c.iteration.token_budget, 200_000);
        assert!((c.safety.max_cost_usd - 2.0).abs() < 0.001);
        assert!(c.safety.abort_on_regression);
        assert!(c.patterns.enabled);
        assert!(c.memory.compaction);
    }

    #[test]
    fn test_tool_loop_defaults() {
        let tl = ToolLoopConfig::default();
        assert_eq!(tl.warning, 10);
        assert_eq!(tl.critical, 20);
        assert_eq!(tl.circuit_breaker, 30);
    }

    #[test]
    fn test_models_default_embedder() {
        let m = ModelsConfig::default();
        assert!(m.executor.is_none());
        assert!(m.evaluator.is_none());
        assert_eq!(m.embedder, Some("openai/text-embedding-3-small".into()));
    }

    #[test]
    fn test_memory_defaults() {
        let m = MemoryConfig::default();
        assert!((m.learning_decay_rate - 0.05).abs() < 0.001);
        assert_eq!(m.max_storage_mb, 500);
    }

    #[test]
    fn test_patterns_defaults() {
        let p = PatternsConfig::default();
        assert_eq!(p.mine_interval_hours, 24);
        assert!((p.min_confidence - 0.7).abs() < 0.001);
        assert_eq!(p.min_samples, 3);
        assert!(p.auto_propose);
    }

    #[test]
    fn test_parse_minimal_toml() {
        let toml_str = "";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.iteration.max_iterations, 3);
    }

    #[test]
    fn test_parse_full_toml() {
        let toml_str = r#"
[iteration]
max_iterations = 5
quality_threshold = 0.9
improvement_threshold = 0.1
timeout_seconds = 600
token_budget = 500000
skip_eval_confidence = 0.99

[safety]
max_cost_usd = 5.0
abort_on_regression = false
regression_threshold = 0.3

[safety.tool_loop]
warning = 15
critical = 25
circuit_breaker = 40

[memory]
compaction = false
learning_decay_rate = 0.1
max_storage_mb = 1000

[patterns]
enabled = false
mine_interval_hours = 48
min_confidence = 0.9
min_samples = 5
auto_propose = false
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.iteration.max_iterations, 5);
        assert!((config.iteration.quality_threshold - 0.9).abs() < 0.001);
        assert_eq!(config.iteration.token_budget, 500_000);
        assert!((config.safety.max_cost_usd - 5.0).abs() < 0.001);
        assert!(!config.safety.abort_on_regression);
        assert_eq!(config.safety.tool_loop.warning, 15);
        assert!(!config.memory.compaction);
        assert!((config.memory.learning_decay_rate - 0.1).abs() < 0.001);
        assert!(!config.patterns.enabled);
        assert!(!config.patterns.auto_propose);
    }

    #[test]
    fn test_parse_models_toml() {
        let toml_str = r#"
[models]
executor = "anthropic/claude-sonnet-4"
evaluator = "anthropic/claude-opus-4"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.models.executor,
            Some("anthropic/claude-sonnet-4".into())
        );
        assert_eq!(
            config.models.evaluator,
            Some("anthropic/claude-opus-4".into())
        );
        assert!(config.models.planner.is_none());
    }

    #[test]
    fn test_parse_plugins_toml() {
        let toml_str = r#"
[plugins]
wasm = ["plugin1.wasm", "plugin2.wasm"]
scripts = ["script.rhai"]

[[plugins.mcp]]
name = "test-server"
command = "npx"
args = ["-y", "test-server"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.plugins.wasm.len(), 2);
        assert_eq!(config.plugins.scripts.len(), 1);
        assert_eq!(config.plugins.mcp.len(), 1);
        assert_eq!(config.plugins.mcp[0].name, "test-server");
        assert_eq!(config.plugins.mcp[0].transport, "stdio");
    }

    #[test]
    fn test_parse_integrations_toml() {
        let toml_str = r##"
[integrations.slack]
enabled = true
channels = ["#engineering"]

[integrations.email]
enabled = false
"##;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.integrations.slack.as_ref().unwrap().enabled);
        assert_eq!(
            config.integrations.slack.as_ref().unwrap().channels.len(),
            1
        );
        assert!(!config.integrations.email.as_ref().unwrap().enabled);
        assert!(config.integrations.notion.is_none());
    }

    #[test]
    fn test_serialize_roundtrip() {
        let config = Config::default();
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(
            deserialized.iteration.max_iterations,
            config.iteration.max_iterations
        );
        assert!((deserialized.safety.max_cost_usd - config.safety.max_cost_usd).abs() < 0.001);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = Config::load_from(Path::new("/nonexistent/config.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_fallback_config_empty() {
        let fb = FallbackConfig::default();
        assert!(fb.executor.is_empty());
    }

    #[test]
    fn test_safety_defaults() {
        let s = SafetyConfig::default();
        assert!((s.regression_threshold - 0.2).abs() < 0.001);
    }
}
