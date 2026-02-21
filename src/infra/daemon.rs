// src/infra/daemon.rs — Background daemon for automated task execution
//
// The daemon runs in the background, processing watch events from
// integrations and executing automated patterns.  When a Mention event
// arrives and `auto_execute` is enabled, the daemon runs the task through
// the orchestrator and delivers the result back via the same integration.
// Approved patterns with cron schedules are evaluated every 60 seconds.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use crate::api;
use crate::api::webhooks;
use crate::core::orchestrator::{Orchestrator, SessionContext};
use crate::core::safety::SafetyChecker;
use crate::core::types::{IterationEngineConfig, TaskInput, TaskResult};
use crate::infra::config::Config;
use crate::integrations::registry::IntegrationRegistry;
use crate::integrations::types::RichMessage;
use crate::integrations::watcher::{WatchConfig, WatchEvent, WatchEventType, WatcherManager};
use crate::learner::skill_selector::SkillSelector;
use crate::memory::recall::{self, HistoryRecall};
use crate::memory::store::Store;
use crate::patterns::event_logger::{EventLogger, EventType, UsageEvent};
use crate::provider::roles::ModelRoles;
use crate::provider::{ModelProvider, ModelRef, ToolDef};
use crate::skills::registry::SkillRegistry;
use crate::soul::loader;

/// Daemon configuration.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Integration watchers to run
    pub watchers: Vec<WatchConfig>,
    /// Whether to auto-execute learned patterns
    pub auto_execute: bool,
    /// Log file path
    pub log_file: Option<String>,
}

/// Everything the daemon needs to execute tasks.
pub struct DaemonContext {
    pub provider: Arc<dyn ModelProvider>,
    pub model_ref: ModelRef,
    pub config: Config,
    pub store: Option<Arc<Mutex<Store>>>,
    pub skill_registry: Arc<SkillRegistry>,
    pub mcp_tools: Vec<ToolDef>,
}

/// Run the daemon loop — polls integrations and dispatches events.
///
/// This is a long-running async task designed to be the main entry point
/// when `openkoi daemon start` is called.
pub async fn run_daemon(
    ctx: DaemonContext,
    registry: Arc<IntegrationRegistry>,
) -> anyhow::Result<()> {
    tracing::info!("OpenKoi daemon starting...");

    // Build watcher configs from integration config
    let watch_configs = build_watch_configs(&ctx.config);

    if watch_configs.is_empty() {
        tracing::warn!("No integrations configured for watching. Daemon has nothing to do.");
        println!("No integrations configured. Use `openkoi connect <app>` to set up integrations.");
        return Ok(());
    }

    let auto_execute = ctx.config.daemon.as_ref().is_none_or(|d| d.auto_execute);

    let mut watcher_manager = WatcherManager::new();
    for wc in watch_configs {
        watcher_manager.add_watch(wc);
    }

    let mut event_rx = watcher_manager.start(registry.clone());

    // ── Start the HTTP API server if enabled ────────────────────────
    let api_config = ctx.config.api.clone().unwrap_or_default();
    let webhook_config = api_config.webhooks.clone();

    if api_config.enabled {
        let api_state = api::ApiState {
            store: ctx.store.clone(),
            token: api_config.token.clone(),
            task_queue: Arc::new(Mutex::new(Vec::new())),
            cancel_requests: Arc::new(Mutex::new(Vec::new())),
        };

        let api_cfg = api_config.clone();
        tokio::spawn(async move {
            if let Err(e) = api::start_server(&api_cfg, api_state).await {
                tracing::error!("API server failed: {}", e);
            }
        });

        tracing::info!("API server started on port {}", api_config.port);
    }

    // Set up signal handler for graceful shutdown
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    // Cron evaluation interval (check approved patterns every 60 seconds)
    let mut cron_interval = tokio::time::interval(Duration::from_secs(60));
    // Consume the immediate first tick
    cron_interval.tick().await;

    println!("Daemon running. Press Ctrl+C to stop.");

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                handle_watch_event(&event, &ctx, registry.clone(), auto_execute, &webhook_config).await;
            }
            _ = cron_interval.tick() => {
                run_scheduled_patterns(&ctx, &registry, &webhook_config).await;
            }
            _ = &mut shutdown => {
                tracing::info!("Shutdown signal received");
                println!("\nShutting down daemon...");
                watcher_manager.stop();
                break;
            }
        }
    }

    tracing::info!("Daemon stopped.");
    Ok(())
}

/// Parsed command from a mention.
enum DaemonCommand {
    /// Run a task with the given description.
    Run(String),
    /// Report current daemon status.
    Status,
    /// Report token/cost information.
    Cost,
    /// Show available commands.
    Help,
}

/// Parse the extracted mention text into a daemon command.
///
/// Recognised prefixes:
///   `run <description>` — explicit task execution
///   `status`            — report recent tasks / daemon health
///   `cost`              — report token & cost totals
///   `help`              — list available commands
///
/// Anything without a recognised prefix is treated as a task description
/// (equivalent to `run <text>`).
fn parse_command(text: &str) -> DaemonCommand {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return DaemonCommand::Help;
    }

    let lower = trimmed.to_lowercase();

    if lower == "status" {
        return DaemonCommand::Status;
    }
    if lower == "cost" || lower == "costs" {
        return DaemonCommand::Cost;
    }
    if lower == "help" || lower == "?" {
        return DaemonCommand::Help;
    }

    // "run <description>"
    if lower.starts_with("run ") {
        let description = trimmed[4..].trim(); // preserve original casing
        if description.is_empty() {
            return DaemonCommand::Help;
        }
        return DaemonCommand::Run(description.to_string());
    }

    // Default: treat entire text as a task description
    DaemonCommand::Run(trimmed.to_string())
}

/// Format a status report from the store.
fn format_status(ctx: &DaemonContext) -> String {
    let store_arc = match ctx.store.as_ref() {
        Some(s) => s,
        None => return "No store configured — task history unavailable.".to_string(),
    };
    let store = match store_arc.lock() {
        Ok(g) => g,
        Err(_) => return "Failed to access store.".to_string(),
    };

    // Query recent events as a proxy for recent tasks
    let since = (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
    let events = store.query_events_since(&since).unwrap_or_default();

    if events.is_empty() {
        return "No tasks executed in the last 24 hours.".to_string();
    }

    let task_count = events.iter().filter(|e| e.event_type == "task").count();
    let avg_score: f64 = {
        let scores: Vec<f64> = events.iter().filter_map(|e| e.score).collect();
        if scores.is_empty() {
            0.0
        } else {
            scores.iter().sum::<f64>() / scores.len() as f64
        }
    };

    let mut lines = vec![format!(
        "OpenKoi status (last 24h): {} task(s), avg score {:.1}/10",
        task_count, avg_score
    )];

    // Show last 5 events
    let recent: Vec<_> = events.iter().rev().take(5).collect();
    for ev in recent {
        let desc = ev.description.as_deref().unwrap_or("(no description)");
        let score_str = ev.score.map(|s| format!(" [{:.1}]", s)).unwrap_or_default();
        lines.push(format!("  • {}{}", truncate(desc, 60), score_str));
    }

    lines.join("\n")
}

/// Format a cost report from the store.
fn format_cost(ctx: &DaemonContext) -> String {
    let store_arc = match ctx.store.as_ref() {
        Some(s) => s,
        None => return "No store configured — cost data unavailable.".to_string(),
    };
    let store = match store_arc.lock() {
        Ok(g) => g,
        Err(_) => return "Failed to access store.".to_string(),
    };

    let since = (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
    let events = store.query_events_since(&since).unwrap_or_default();

    if events.is_empty() {
        return "No usage recorded in the last 24 hours.".to_string();
    }

    let total_events = events.len();
    let task_events = events.iter().filter(|e| e.event_type == "task").count();

    format!(
        "OpenKoi cost (last 24h): {} event(s), {} task(s).\nDetailed cost tracking is available via `openkoi dashboard`.",
        total_events, task_events
    )
}

/// Help text sent back when a user sends `help` or an empty mention.
fn format_help() -> String {
    [
        "OpenKoi commands:",
        "  `@openkoi <task description>` — run a task",
        "  `@openkoi run <description>`  — run a task (explicit)",
        "  `@openkoi status`             — recent task summary",
        "  `@openkoi cost`               — usage & cost report",
        "  `@openkoi help`               — this message",
    ]
    .join("\n")
}

/// Handle an incoming watch event.
///
/// - `Mention`: If auto_execute is on, run the message content as a task
///   through the orchestrator and deliver the result back.
/// - `NewMessage`: Log only (future: match against approved patterns).
/// - `DocumentUpdated`: Log only.
async fn handle_watch_event(
    event: &WatchEvent,
    ctx: &DaemonContext,
    registry: Arc<IntegrationRegistry>,
    auto_execute: bool,
    webhook_config: &crate::infra::config::WebhookConfig,
) {
    let tid = event.thread_id.as_deref();

    match event.event_type {
        WatchEventType::NewMessage => {
            tracing::info!(
                "[{}] New message in {}: {}",
                event.integration_id,
                event.source,
                truncate(&event.payload, 100)
            );
        }
        WatchEventType::DocumentUpdated => {
            tracing::info!(
                "[{}] Document updated: {}",
                event.integration_id,
                event.source
            );
        }
        WatchEventType::Mention => {
            tracing::info!(
                "[{}] Mentioned in {}: {}",
                event.integration_id,
                event.source,
                truncate(&event.payload, 100)
            );

            let command = parse_command(&event.payload);

            match command {
                DaemonCommand::Help => {
                    deliver_result(
                        &registry,
                        &event.integration_id,
                        &event.source,
                        &format_help(),
                        tid,
                    )
                    .await;
                }
                DaemonCommand::Status => {
                    let report = format_status(ctx);
                    deliver_result(
                        &registry,
                        &event.integration_id,
                        &event.source,
                        &report,
                        tid,
                    )
                    .await;
                }
                DaemonCommand::Cost => {
                    let report = format_cost(ctx);
                    deliver_result(
                        &registry,
                        &event.integration_id,
                        &event.source,
                        &report,
                        tid,
                    )
                    .await;
                }
                DaemonCommand::Run(task_description) => {
                    if !auto_execute {
                        tracing::debug!(
                            "Auto-execute disabled; skipping task execution for mention"
                        );
                        deliver_result(
                            &registry,
                            &event.integration_id,
                            &event.source,
                            "Auto-execute is disabled. Enable it in your config to run tasks via mentions.",
                            tid,
                        )
                        .await;
                        return;
                    }

                    if task_description.trim().is_empty() {
                        tracing::debug!("Empty task description; sending help");
                        deliver_result(
                            &registry,
                            &event.integration_id,
                            &event.source,
                            &format_help(),
                            tid,
                        )
                        .await;
                        return;
                    }

                    let notify = NotifyTarget {
                        registry: registry.clone(),
                        integration_id: event.integration_id.clone(),
                        channel: event.source.clone(),
                        thread_id: tid.map(String::from),
                    };

                    let result =
                        execute_daemon_task(ctx, &registry, &task_description, Some(notify)).await;

                    match result {
                        Ok(task_result) => {
                            // Deliver a rich result message
                            let mut msg = RichMessage::new(&task_result.output.content)
                                .with_title("Task Complete")
                                .with_color("#36a64f")
                                .with_field("Score", format!("{:.2}", task_result.final_score))
                                .with_field("Cost", format!("${:.2}", task_result.cost))
                                .with_field("Iterations", format!("{}", task_result.iterations));

                            if let Some(t) = tid {
                                msg = msg.in_thread(t.to_string());
                            }

                            deliver_rich_result(
                                &registry,
                                &event.integration_id,
                                &event.source,
                                &msg,
                            )
                            .await;

                            // Fire task.complete webhook
                            webhooks::fire_webhook(
                                webhook_config,
                                webhooks::WebhookEvent::TaskComplete {
                                    task_id: uuid::Uuid::new_v4().to_string(),
                                    description: task_description.clone(),
                                    iterations: task_result.iterations,
                                    final_score: task_result.final_score,
                                    cost_usd: task_result.cost,
                                    total_tokens: task_result.total_tokens,
                                },
                            );
                        }
                        Err(e) => {
                            let err_msg = format!("Task failed: {e}");
                            tracing::error!("[{}] {}", event.integration_id, err_msg);

                            let mut msg = RichMessage::new(&err_msg)
                                .with_title("Task Failed")
                                .with_color("#cc0000");

                            if let Some(t) = tid {
                                msg = msg.in_thread(t.to_string());
                            }

                            deliver_rich_result(
                                &registry,
                                &event.integration_id,
                                &event.source,
                                &msg,
                            )
                            .await;

                            // Fire task.failed webhook
                            webhooks::fire_webhook(
                                webhook_config,
                                webhooks::WebhookEvent::TaskFailed {
                                    task_id: uuid::Uuid::new_v4().to_string(),
                                    description: task_description.clone(),
                                    error: format!("{e}"),
                                },
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Target for progress notification delivery during task execution.
struct NotifyTarget {
    registry: Arc<IntegrationRegistry>,
    integration_id: String,
    channel: String,
    thread_id: Option<String>,
}

/// Execute a task through the orchestrator and return the full TaskResult.
///
/// If `notify` is provided, a "still working..." progress notification is
/// sent once after 60 seconds of execution.
async fn execute_daemon_task(
    ctx: &DaemonContext,
    registry: &IntegrationRegistry,
    task_description: &str,
    notify: Option<NotifyTarget>,
) -> anyhow::Result<TaskResult> {
    tracing::info!("Daemon executing task: {}", truncate(task_description, 80));

    let task = TaskInput::new(task_description);

    let engine_config = IterationEngineConfig::from(&ctx.config.iteration);
    let safety = SafetyChecker::from_config(&ctx.config.iteration, &ctx.config.safety);

    // Load soul
    let soul = loader::load_soul();

    // Select relevant skills
    let selector = SkillSelector::new();
    let store_guard = ctx.store.as_ref().and_then(|s| s.lock().ok());
    let ranked_skills = selector.select(
        &task.description,
        task.category.as_deref(),
        ctx.skill_registry.all(),
        store_guard.as_deref(),
    );

    // Recall from memory
    let recall = match store_guard.as_deref() {
        Some(s) => {
            let token_budget = engine_config.token_budget / 10;
            recall::recall(s, task_description, task.category.as_deref(), token_budget)
                .unwrap_or_default()
        }
        None => HistoryRecall::default(),
    };
    drop(store_guard);

    let session_ctx = SessionContext {
        soul,
        ranked_skills,
        recall,
        tools: ctx.mcp_tools.clone(),
        skill_registry: ctx.skill_registry.clone(),
    };

    // Progress notification: send "still working..." once after 60s.
    let notified = Arc::new(AtomicBool::new(false));
    let notify_handle = if let Some(ref target) = notify {
        let notified_clone = notified.clone();
        let notify_registry = target.registry.clone();
        let notify_int_id = target.integration_id.clone();
        let notify_channel = target.channel.clone();
        let notify_thread_id = target.thread_id.clone();

        Some(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            if !notified_clone.swap(true, Ordering::SeqCst) {
                let mut msg = RichMessage::new("Still working on your task...")
                    .with_title("In Progress")
                    .with_color("#f2c744");
                if let Some(ref t) = notify_thread_id {
                    msg = msg.in_thread(t.clone());
                }
                deliver_rich_result(&notify_registry, &notify_int_id, &notify_channel, &msg).await;
            }
        }))
    } else {
        None
    };

    let mut orchestrator = Orchestrator::new(
        ctx.provider.clone(),
        ModelRoles::from_config(
            ctx.model_ref.clone(),
            ctx.config.models.executor.as_deref(),
            ctx.config.models.evaluator.as_deref(),
            ctx.config.models.planner.as_deref(),
            ctx.config.models.embedder.as_deref(),
        ),
        engine_config,
        safety,
        ctx.skill_registry.clone(),
        ctx.store.clone(),
    );

    let integrations = if registry.list().is_empty() {
        None
    } else {
        Some(registry)
    };

    let result = orchestrator
        .run(task, &session_ctx, None, integrations)
        .await;

    // Cancel the notify timer if the task finished before 60s
    notified.store(true, Ordering::SeqCst);
    if let Some(handle) = notify_handle {
        handle.abort();
    }

    let result = result?;

    // Log the usage event
    if let Some(ref s) = ctx.store {
        if let Ok(locked) = s.lock() {
            let event_logger = EventLogger::new(&locked);
            let _ = event_logger.log(&UsageEvent {
                event_type: EventType::Task,
                channel: "daemon".into(),
                description: task_description.to_string(),
                category: None,
                skills_used: result.skills_used.clone(),
                score: Some(result.final_score as f32),
            });
        }
    }

    tracing::info!(
        "Daemon task completed: {} iteration(s), {} tokens",
        result.iterations,
        result.total_tokens,
    );

    Ok(result)
}

/// Deliver a result message back to the integration channel that triggered it.
///
/// If `thread_id` is provided, replies in-thread. Uses `send_rich()` when the
/// message has structured fields; falls back to plain `send()` for simple text.
async fn deliver_result(
    registry: &IntegrationRegistry,
    integration_id: &str,
    channel: &str,
    content: &str,
    thread_id: Option<&str>,
) {
    let Some(integration) = registry.get(integration_id) else {
        tracing::warn!(
            "Cannot deliver result: integration '{}' not found",
            integration_id
        );
        return;
    };

    let Some(messaging) = integration.messaging() else {
        tracing::warn!(
            "Cannot deliver result: integration '{}' has no messaging adapter",
            integration_id
        );
        return;
    };

    let result = if let Some(tid) = thread_id {
        let msg = RichMessage::new(content).in_thread(tid.to_string());
        messaging.send_rich(channel, &msg).await
    } else {
        messaging.send(channel, content).await
    };

    match result {
        Ok(_) => {
            tracing::info!("[{}] Result delivered to {}", integration_id, channel);
        }
        Err(e) => {
            tracing::error!(
                "[{}] Failed to deliver result to {}: {}",
                integration_id,
                channel,
                e
            );
        }
    }
}

/// Deliver a rich (structured) result message back to the integration channel.
async fn deliver_rich_result(
    registry: &IntegrationRegistry,
    integration_id: &str,
    channel: &str,
    msg: &RichMessage,
) {
    let Some(integration) = registry.get(integration_id) else {
        tracing::warn!(
            "Cannot deliver result: integration '{}' not found",
            integration_id
        );
        return;
    };

    let Some(messaging) = integration.messaging() else {
        tracing::warn!(
            "Cannot deliver result: integration '{}' has no messaging adapter",
            integration_id
        );
        return;
    };

    match messaging.send_rich(channel, msg).await {
        Ok(_) => {
            tracing::info!("[{}] Rich result delivered to {}", integration_id, channel);
        }
        Err(e) => {
            tracing::error!(
                "[{}] Failed to deliver rich result to {}: {}",
                integration_id,
                channel,
                e
            );
        }
    }
}

/// Evaluate approved patterns with cron schedules and execute matching ones.
///
/// This runs every 60 seconds in the daemon loop.  It loads all patterns
/// with `status = 'approved'`, parses their `trigger_json` for a cron
/// expression, and checks whether the expression matches the current
/// minute.  If it does, the pattern's description is executed as a task.
async fn run_scheduled_patterns(
    ctx: &DaemonContext,
    registry: &IntegrationRegistry,
    webhook_config: &crate::infra::config::WebhookConfig,
) {
    let store_arc = match ctx.store.as_ref() {
        Some(s) => s,
        None => return,
    };

    // Scope the lock so the MutexGuard is dropped before any `.await` calls.
    let patterns = {
        let store_guard = match store_arc.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match store_guard.query_approved_patterns() {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!("Failed to query approved patterns: {}", e);
                return;
            }
        }
    };

    if patterns.is_empty() {
        return;
    }

    let now = chrono::Utc::now();

    for pattern in &patterns {
        // Only process patterns that have trigger_json with a cron schedule
        let Some(ref freq) = pattern.frequency else {
            continue;
        };

        // Parse the frequency as a simple cron-like spec.
        // Supported formats:
        //   "daily HH:MM"   — runs once a day at the given UTC time
        //   "hourly"        — runs at the top of every hour
        //   "every Xm"      — runs every X minutes (checked modulo)
        if let Some(task_desc) = should_run_now(freq, &now) {
            let description = if task_desc.is_empty() {
                &pattern.description
            } else {
                &task_desc
            };

            tracing::info!(
                "Cron trigger matched for pattern '{}': running task",
                pattern.id
            );

            match execute_daemon_task(ctx, registry, description, None).await {
                Ok(result) => {
                    tracing::info!(
                        "Scheduled pattern '{}' completed ({} chars output)",
                        pattern.id,
                        result.output.content.len()
                    );

                    // Fire task.complete webhook
                    webhooks::fire_webhook(
                        webhook_config,
                        webhooks::WebhookEvent::TaskComplete {
                            task_id: pattern.id.clone(),
                            description: description.to_string(),
                            iterations: result.iterations,
                            final_score: result.final_score,
                            cost_usd: result.cost,
                            total_tokens: result.total_tokens,
                        },
                    );
                }
                Err(e) => {
                    tracing::error!("Scheduled pattern '{}' failed: {}", pattern.id, e);

                    // Fire task.failed webhook
                    webhooks::fire_webhook(
                        webhook_config,
                        webhooks::WebhookEvent::TaskFailed {
                            task_id: pattern.id.clone(),
                            description: description.to_string(),
                            error: format!("{e}"),
                        },
                    );
                }
            }
        }
    }
}

/// Check if a frequency spec should trigger right now (within the current
/// UTC minute).  Returns `Some("")` to use the pattern description, or
/// `None` if no match.
fn should_run_now(freq: &str, now: &chrono::DateTime<chrono::Utc>) -> Option<String> {
    use chrono::Timelike;
    let freq = freq.trim().to_lowercase();

    if freq == "hourly" {
        // Match at minute 0 of every hour
        if now.minute() == 0 {
            return Some(String::new());
        }
    } else if let Some(time_str) = freq.strip_prefix("daily ") {
        // "daily HH:MM"
        let parts: Vec<&str> = time_str.trim().split(':').collect();
        if parts.len() == 2 {
            if let (Ok(hour), Ok(minute)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                if now.hour() == hour && now.minute() == minute {
                    return Some(String::new());
                }
            }
        }
    } else if let Some(interval_str) = freq
        .strip_prefix("every ")
        .and_then(|s| s.strip_suffix('m'))
    {
        // "every Xm"
        if let Ok(interval) = interval_str.trim().parse::<u32>() {
            if interval > 0 && (now.hour() * 60 + now.minute()).is_multiple_of(interval) {
                return Some(String::new());
            }
        }
    }

    None
}

/// Build watch configs from the integrations configuration.
fn build_watch_configs(config: &Config) -> Vec<WatchConfig> {
    let mut configs = Vec::new();

    if let Some(ref slack) = config.integrations.slack {
        if slack.enabled {
            configs.push(WatchConfig {
                integration_id: "slack".to_string(),
                targets: slack.channels.clone(),
                poll_interval_secs: 30,
                mentions_only: false,
            });
        }
    }

    if let Some(ref discord) = config.integrations.discord {
        if discord.enabled {
            configs.push(WatchConfig {
                integration_id: "discord".to_string(),
                targets: discord.channels.clone(),
                poll_interval_secs: 30,
                mentions_only: false,
            });
        }
    }

    if let Some(ref telegram) = config.integrations.telegram {
        if telegram.enabled {
            configs.push(WatchConfig {
                integration_id: "telegram".to_string(),
                targets: telegram.channels.clone(),
                poll_interval_secs: 10,
                mentions_only: false,
            });
        }
    }

    if let Some(ref notion) = config.integrations.notion {
        if notion.enabled {
            configs.push(WatchConfig {
                integration_id: "notion".to_string(),
                targets: notion.channels.clone(), // repurposed as doc IDs to watch
                poll_interval_secs: 60,
                mentions_only: false,
            });
        }
    }

    if let Some(ref msteams) = config.integrations.msteams {
        if msteams.enabled {
            configs.push(WatchConfig {
                integration_id: "msteams".to_string(),
                targets: msteams.channels.clone(),
                poll_interval_secs: 30,
                mentions_only: false,
            });
        }
    }

    #[cfg(target_os = "macos")]
    if let Some(ref imessage) = config.integrations.imessage {
        if imessage.enabled {
            configs.push(WatchConfig {
                integration_id: "imessage".to_string(),
                targets: imessage.channels.clone(),
                poll_interval_secs: 15,
                mentions_only: true, // iMessage requires "koi:" prefix
            });
        }
    }

    configs
}

/// Truncate a string for logging.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}

/// Write a PID file for the daemon.
pub fn write_pid_file() -> anyhow::Result<std::path::PathBuf> {
    let pid_path = crate::infra::paths::data_dir().join("daemon.pid");
    let pid = std::process::id();
    std::fs::write(&pid_path, pid.to_string())?;
    Ok(pid_path)
}

/// Remove the PID file.
pub fn remove_pid_file() {
    let pid_path = crate::infra::paths::data_dir().join("daemon.pid");
    let _ = std::fs::remove_file(pid_path);
}

/// Check if a daemon is already running.
pub fn is_daemon_running() -> bool {
    let pid_path = crate::infra::paths::data_dir().join("daemon.pid");
    if !pid_path.exists() {
        return false;
    }

    if let Ok(content) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = content.trim().parse::<u32>() {
            // Check if process exists by looking at /proc or using kill -0 via Command
            #[cfg(unix)]
            {
                let output = std::process::Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .output();
                return output.map(|o| o.status.success()).unwrap_or(false);
            }
            #[cfg(not(unix))]
            {
                let _ = pid;
                return true;
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_should_run_now_hourly() {
        let at_zero = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        assert!(should_run_now("hourly", &at_zero).is_some());

        let at_five = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 5, 0).unwrap();
        assert!(should_run_now("hourly", &at_five).is_none());
    }

    #[test]
    fn test_should_run_now_daily() {
        let at_match = chrono::Utc
            .with_ymd_and_hms(2025, 1, 15, 14, 30, 0)
            .unwrap();
        assert!(should_run_now("daily 14:30", &at_match).is_some());

        let at_miss = chrono::Utc
            .with_ymd_and_hms(2025, 1, 15, 14, 31, 0)
            .unwrap();
        assert!(should_run_now("daily 14:30", &at_miss).is_none());
    }

    #[test]
    fn test_should_run_now_every_xm() {
        // every 15m: should match at minute 0, 15, 30, 45
        let at_zero = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 0, 0, 0).unwrap();
        assert!(should_run_now("every 15m", &at_zero).is_some());

        let at_fifteen = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 0, 15, 0).unwrap();
        assert!(should_run_now("every 15m", &at_fifteen).is_some());

        let at_seven = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 0, 7, 0).unwrap();
        assert!(should_run_now("every 15m", &at_seven).is_none());
    }

    #[test]
    fn test_should_run_now_invalid() {
        let now = chrono::Utc::now();
        assert!(should_run_now("garbage", &now).is_none());
        assert!(should_run_now("", &now).is_none());
        assert!(should_run_now("weekly", &now).is_none());
    }

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate("hello world", 5), "hello");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }
}
