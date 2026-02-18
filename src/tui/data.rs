// src/tui/data.rs — Data structures and fetchers for the TUI dashboard.
//
// This module queries Store (SQLite) and Config to produce display-ready
// data structs consumed by the widget layer.

use crate::infra::config::Config;
use crate::memory::store::{LearningRow, SkillEffectivenessRow, Store, UsagePatternRow};

// ── Snapshot structs ─────────────────────────────────────────────

/// Top-level dashboard data, refreshed periodically.
#[derive(Debug, Default)]
pub struct DashboardData {
    pub overview: OverviewData,
    pub tasks: TasksData,
    pub learnings: LearningsData,
    pub costs: CostsData,
    pub plugins: PluginsData,
    pub config_tree: ConfigTree,
}

#[derive(Debug, Default)]
pub struct OverviewData {
    pub version: String,
    pub daemon_running: bool,
    pub total_sessions: i64,
    pub total_tasks: i64,
    pub total_learnings: i64,
    pub total_patterns: i64,
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub avg_score: f64,
    pub plugin_summary: String,
    pub integration_count: usize,
    pub integrations: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct TaskRow {
    pub id: String,
    pub description: String,
    pub category: String,
    pub final_score: Option<f64>,
    pub iterations: Option<i32>,
    pub decision: String,
    pub total_tokens: Option<i64>,
    pub total_cost: Option<f64>,
    pub created_at: String,
}

#[derive(Debug, Default)]
pub struct TasksData {
    pub tasks: Vec<TaskRow>,
    pub recent_findings: Vec<FindingRow>,
}

#[derive(Debug, Default, Clone)]
pub struct FindingRow {
    pub severity: String,
    pub dimension: String,
    pub title: String,
    pub task_desc: String,
}

#[derive(Debug, Default)]
pub struct LearningsData {
    pub learnings: Vec<LearningRow>,
    pub patterns: Vec<UsagePatternRow>,
    pub skills: Vec<SkillEffectivenessRow>,
}

#[derive(Debug, Default)]
pub struct CostsData {
    pub total_tokens: i64,
    pub total_cost_usd: f64,
    pub by_model: Vec<ModelCostRow>,
    pub daily: Vec<DailyCostRow>,
}

#[derive(Debug, Default, Clone)]
pub struct ModelCostRow {
    pub provider: String,
    pub model: String,
    pub tokens: i64,
    pub cost: f64,
    pub sessions: i64,
}

#[derive(Debug, Default, Clone)]
pub struct DailyCostRow {
    pub day: String,
    pub tokens: i64,
    pub cost: f64,
    pub tasks: i64,
}

#[derive(Debug, Default)]
pub struct PluginsData {
    pub wasm_plugins: Vec<String>,
    pub rhai_scripts: Vec<String>,
    pub mcp_servers: Vec<McpServerInfo>,
    pub hook_summary: String,
}

#[derive(Debug, Default, Clone)]
pub struct McpServerInfo {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Default)]
pub struct ConfigTree {
    /// Flattened key=value pairs grouped by section.
    pub sections: Vec<ConfigSection>,
}

#[derive(Debug, Default, Clone)]
pub struct ConfigSection {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

// ── Fetching ─────────────────────────────────────────────────────

/// Load all dashboard data from Store + Config.
pub fn fetch_all(store: Option<&Store>, config: &Config) -> DashboardData {
    DashboardData {
        overview: fetch_overview(store, config),
        tasks: fetch_tasks(store),
        learnings: fetch_learnings(store),
        costs: fetch_costs(store),
        plugins: fetch_plugins(config),
        config_tree: build_config_tree(config),
    }
}

fn fetch_overview(store: Option<&Store>, config: &Config) -> OverviewData {
    let mut data = OverviewData {
        version: env!("CARGO_PKG_VERSION").to_string(),
        daemon_running: crate::infra::daemon::is_daemon_running(),
        plugin_summary: String::new(),
        ..Default::default()
    };

    // Count WASM + Rhai
    let wasm_count = config.plugins.wasm.len();
    let rhai_count = config.plugins.scripts.len();
    let mcp_count = config.plugins.mcp.len();
    if wasm_count + rhai_count + mcp_count > 0 {
        data.plugin_summary = format!("{wasm_count} WASM, {rhai_count} Rhai, {mcp_count} MCP");
    } else {
        data.plugin_summary = "none".to_string();
    }

    // Count integrations from config
    let mut integrations = Vec::new();
    let ic = &config.integrations;
    if ic.slack.as_ref().is_some_and(|e| e.enabled) {
        integrations.push("Slack".to_string());
    }
    if ic.notion.as_ref().is_some_and(|e| e.enabled) {
        integrations.push("Notion".to_string());
    }
    if ic.discord.as_ref().is_some_and(|e| e.enabled) {
        integrations.push("Discord".to_string());
    }
    if ic.telegram.as_ref().is_some_and(|e| e.enabled) {
        integrations.push("Telegram".to_string());
    }
    if ic.google_sheets.as_ref().is_some_and(|e| e.enabled) {
        integrations.push("Google Sheets".to_string());
    }
    if ic.email.as_ref().is_some_and(|e| e.enabled) {
        integrations.push("Email".to_string());
    }
    if ic.msoffice.as_ref().is_some_and(|c| c.enabled) {
        integrations.push("MS Office".to_string());
    }
    if ic.imessage.as_ref().is_some_and(|e| e.enabled) {
        integrations.push("iMessage".to_string());
    }
    data.integration_count = integrations.len();
    data.integrations = integrations;

    if let Some(store) = store {
        data.total_learnings = store.count_learnings().unwrap_or(0);

        // Sessions count
        data.total_sessions = query_count(store, "SELECT COUNT(*) FROM sessions");
        data.total_tasks = query_count(store, "SELECT COUNT(*) FROM tasks");
        data.total_patterns = store
            .query_detected_patterns()
            .map(|v| v.len() as i64)
            .unwrap_or(0);

        // Aggregate tokens and cost
        let (tokens, cost) = query_sum_pair(
            store,
            "SELECT COALESCE(SUM(total_tokens),0), COALESCE(SUM(total_cost_usd),0.0) FROM tasks",
        );
        data.total_tokens = tokens;
        data.total_cost_usd = cost;

        // Average score
        data.avg_score = query_f64(
            store,
            "SELECT COALESCE(AVG(final_score),0.0) FROM tasks WHERE final_score IS NOT NULL",
        );
    }

    data
}

fn fetch_tasks(store: Option<&Store>) -> TasksData {
    let mut data = TasksData::default();
    let Some(store) = store else { return data };

    // Recent tasks (last 100)
    if let Ok(mut stmt) = store.conn().prepare(
        "SELECT id, description, COALESCE(category,''), final_score, iterations, \
         COALESCE(decision,''), total_tokens, total_cost_usd, created_at \
         FROM tasks ORDER BY created_at DESC LIMIT 100",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(TaskRow {
                id: row.get(0)?,
                description: row.get(1)?,
                category: row.get(2)?,
                final_score: row.get(3)?,
                iterations: row.get(4)?,
                decision: row.get(5)?,
                total_tokens: row.get(6)?,
                total_cost: row.get(7)?,
                created_at: row.get(8)?,
            })
        }) {
            data.tasks = rows.filter_map(|r| r.ok()).collect();
        }
    }

    // Recent findings (last 50)
    if let Ok(mut stmt) = store.conn().prepare(
        "SELECT f.severity, f.dimension, f.title, COALESCE(t.description,'') \
         FROM findings f \
         JOIN iteration_cycles c ON f.cycle_id = c.id \
         JOIN tasks t ON c.task_id = t.id \
         ORDER BY f.rowid DESC LIMIT 50",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(FindingRow {
                severity: row.get(0)?,
                dimension: row.get(1)?,
                title: row.get(2)?,
                task_desc: row.get(3)?,
            })
        }) {
            data.recent_findings = rows.filter_map(|r| r.ok()).collect();
        }
    }

    data
}

fn fetch_learnings(store: Option<&Store>) -> LearningsData {
    let mut data = LearningsData::default();
    let Some(store) = store else { return data };

    data.learnings = store.query_all_learnings().unwrap_or_default();
    data.patterns = store.query_detected_patterns().unwrap_or_default();

    // Top skills across all categories
    if let Ok(mut stmt) = store.conn().prepare(
        "SELECT skill_name, task_category, avg_score, sample_count \
         FROM skill_effectiveness ORDER BY avg_score DESC LIMIT 50",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(SkillEffectivenessRow {
                skill_name: row.get(0)?,
                task_category: row.get(1)?,
                avg_score: row.get(2)?,
                sample_count: row.get(3)?,
            })
        }) {
            data.skills = rows.filter_map(|r| r.ok()).collect();
        }
    }

    data
}

fn fetch_costs(store: Option<&Store>) -> CostsData {
    let mut data = CostsData::default();
    let Some(store) = store else { return data };

    // Totals
    let (tokens, cost) = query_sum_pair(
        store,
        "SELECT COALESCE(SUM(total_tokens),0), COALESCE(SUM(total_cost_usd),0.0) FROM sessions",
    );
    data.total_tokens = tokens;
    data.total_cost_usd = cost;

    // Cost by model
    if let Ok(mut stmt) = store.conn().prepare(
        "SELECT model_provider, model_id, COALESCE(SUM(total_tokens),0), \
         COALESCE(SUM(total_cost_usd),0.0), COUNT(*) \
         FROM sessions \
         WHERE model_provider IS NOT NULL \
         GROUP BY model_provider, model_id \
         ORDER BY SUM(total_cost_usd) DESC",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(ModelCostRow {
                provider: row.get(0)?,
                model: row.get(1)?,
                tokens: row.get(2)?,
                cost: row.get(3)?,
                sessions: row.get(4)?,
            })
        }) {
            data.by_model = rows.filter_map(|r| r.ok()).collect();
        }
    }

    // Daily cost (last 30 days)
    if let Ok(mut stmt) = store.conn().prepare(
        "SELECT DATE(created_at) as d, COALESCE(SUM(total_tokens),0), \
         COALESCE(SUM(total_cost_usd),0.0), COUNT(*) \
         FROM tasks WHERE created_at >= DATE('now', '-30 days') \
         GROUP BY d ORDER BY d DESC",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok(DailyCostRow {
                day: row.get(0)?,
                tokens: row.get(1)?,
                cost: row.get(2)?,
                tasks: row.get(3)?,
            })
        }) {
            data.daily = rows.filter_map(|r| r.ok()).collect();
        }
    }

    data
}

fn fetch_plugins(config: &Config) -> PluginsData {
    let wasm_plugins: Vec<String> = config
        .plugins
        .wasm
        .iter()
        .map(|p| {
            std::path::Path::new(p)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| p.clone())
        })
        .collect();

    let rhai_scripts: Vec<String> = config
        .plugins
        .scripts
        .iter()
        .map(|p| {
            std::path::Path::new(p)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| p.clone())
        })
        .collect();

    let mcp_servers: Vec<McpServerInfo> = config
        .plugins
        .mcp
        .iter()
        .map(|c| McpServerInfo {
            name: c.name.clone(),
            command: c.command.clone(),
        })
        .collect();

    let parts: Vec<String> = [
        (!wasm_plugins.is_empty()).then(|| format!("{} WASM", wasm_plugins.len())),
        (!rhai_scripts.is_empty()).then(|| format!("{} Rhai", rhai_scripts.len())),
        (!mcp_servers.is_empty()).then(|| format!("{} MCP", mcp_servers.len())),
    ]
    .into_iter()
    .flatten()
    .collect();

    let hook_summary = if parts.is_empty() {
        "No plugins loaded".to_string()
    } else {
        parts.join(", ")
    };

    PluginsData {
        wasm_plugins,
        rhai_scripts,
        mcp_servers,
        hook_summary,
    }
}

fn build_config_tree(config: &Config) -> ConfigTree {
    let mut sections = Vec::new();

    // Models
    sections.push(ConfigSection {
        name: "models".to_string(),
        entries: vec![
            (
                "executor".into(),
                config
                    .models
                    .executor
                    .as_deref()
                    .unwrap_or("auto")
                    .to_string(),
            ),
            (
                "evaluator".into(),
                config
                    .models
                    .evaluator
                    .as_deref()
                    .unwrap_or("auto")
                    .to_string(),
            ),
            (
                "planner".into(),
                config
                    .models
                    .planner
                    .as_deref()
                    .unwrap_or("auto")
                    .to_string(),
            ),
            (
                "fallback".into(),
                if config.models.fallback.executor.is_empty() {
                    "none".into()
                } else {
                    config.models.fallback.executor.join(", ")
                },
            ),
        ],
    });

    // Iteration
    sections.push(ConfigSection {
        name: "iteration".to_string(),
        entries: vec![
            (
                "max_iterations".into(),
                config.iteration.max_iterations.to_string(),
            ),
            (
                "quality_threshold".into(),
                format!("{:.2}", config.iteration.quality_threshold),
            ),
            (
                "improvement_threshold".into(),
                format!("{:.2}", config.iteration.improvement_threshold),
            ),
            (
                "timeout_seconds".into(),
                config.iteration.timeout_seconds.to_string(),
            ),
            (
                "token_budget".into(),
                config.iteration.token_budget.to_string(),
            ),
            (
                "skip_eval_confidence".into(),
                format!("{:.2}", config.iteration.skip_eval_confidence),
            ),
        ],
    });

    // Safety
    sections.push(ConfigSection {
        name: "safety".to_string(),
        entries: vec![
            (
                "max_cost_usd".into(),
                format!("${:.2}", config.safety.max_cost_usd),
            ),
            (
                "abort_on_regression".into(),
                config.safety.abort_on_regression.to_string(),
            ),
            (
                "regression_threshold".into(),
                format!("{:.2}", config.safety.regression_threshold),
            ),
            (
                "tool_loop.warning".into(),
                config.safety.tool_loop.warning.to_string(),
            ),
            (
                "tool_loop.critical".into(),
                config.safety.tool_loop.critical.to_string(),
            ),
            (
                "tool_loop.circuit_breaker".into(),
                config.safety.tool_loop.circuit_breaker.to_string(),
            ),
        ],
    });

    // Patterns
    sections.push(ConfigSection {
        name: "patterns".to_string(),
        entries: vec![
            ("enabled".into(), config.patterns.enabled.to_string()),
            (
                "mine_interval_hours".into(),
                config.patterns.mine_interval_hours.to_string(),
            ),
            (
                "min_confidence".into(),
                format!("{:.2}", config.patterns.min_confidence),
            ),
            (
                "min_samples".into(),
                config.patterns.min_samples.to_string(),
            ),
            (
                "auto_propose".into(),
                config.patterns.auto_propose.to_string(),
            ),
        ],
    });

    // Memory
    sections.push(ConfigSection {
        name: "memory".to_string(),
        entries: vec![
            ("compaction".into(), config.memory.compaction.to_string()),
            (
                "learning_decay_rate".into(),
                format!("{:.3}", config.memory.learning_decay_rate),
            ),
            (
                "max_storage_mb".into(),
                format!("{} MB", config.memory.max_storage_mb),
            ),
        ],
    });

    // Plugins
    sections.push(ConfigSection {
        name: "plugins".to_string(),
        entries: vec![
            (
                "wasm".into(),
                if config.plugins.wasm.is_empty() {
                    "none".into()
                } else {
                    format!("{} plugin(s)", config.plugins.wasm.len())
                },
            ),
            (
                "scripts".into(),
                if config.plugins.scripts.is_empty() {
                    "none".into()
                } else {
                    format!("{} script(s)", config.plugins.scripts.len())
                },
            ),
            (
                "mcp".into(),
                if config.plugins.mcp.is_empty() {
                    "none".into()
                } else {
                    format!("{} server(s)", config.plugins.mcp.len())
                },
            ),
        ],
    });

    // Integrations
    let mut int_entries = Vec::new();
    let ic = &config.integrations;
    int_entries.push((
        "slack".into(),
        ic.slack
            .as_ref()
            .map(|e| if e.enabled { "enabled" } else { "disabled" })
            .unwrap_or("not configured")
            .to_string(),
    ));
    int_entries.push((
        "notion".into(),
        ic.notion
            .as_ref()
            .map(|e| if e.enabled { "enabled" } else { "disabled" })
            .unwrap_or("not configured")
            .to_string(),
    ));
    int_entries.push((
        "discord".into(),
        ic.discord
            .as_ref()
            .map(|e| if e.enabled { "enabled" } else { "disabled" })
            .unwrap_or("not configured")
            .to_string(),
    ));
    int_entries.push((
        "telegram".into(),
        ic.telegram
            .as_ref()
            .map(|e| if e.enabled { "enabled" } else { "disabled" })
            .unwrap_or("not configured")
            .to_string(),
    ));
    int_entries.push((
        "email".into(),
        ic.email
            .as_ref()
            .map(|e| if e.enabled { "enabled" } else { "disabled" })
            .unwrap_or("not configured")
            .to_string(),
    ));
    int_entries.push((
        "msoffice".into(),
        ic.msoffice
            .as_ref()
            .map(|c| if c.enabled { "enabled" } else { "disabled" })
            .unwrap_or("not configured")
            .to_string(),
    ));
    sections.push(ConfigSection {
        name: "integrations".to_string(),
        entries: int_entries,
    });

    ConfigTree { sections }
}

// ── SQL helpers ──────────────────────────────────────────────────

fn query_count(store: &Store, sql: &str) -> i64 {
    store
        .conn()
        .query_row(sql, [], |row| row.get(0))
        .unwrap_or(0)
}

fn query_f64(store: &Store, sql: &str) -> f64 {
    store
        .conn()
        .query_row(sql, [], |row| row.get(0))
        .unwrap_or(0.0)
}

fn query_sum_pair(store: &Store, sql: &str) -> (i64, f64) {
    store
        .conn()
        .query_row(sql, [], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap_or((0, 0.0))
}
