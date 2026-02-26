// src/infra/daemon/handler.rs

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::api::webhooks;
use crate::core::orchestrator::{Orchestrator, SessionContext};
use crate::core::safety::SafetyChecker;
use crate::core::types::{IterationEngineConfig, TaskInput, TaskResult};
use crate::infra::daemon::{status, DaemonContext};
use crate::integrations::registry::IntegrationRegistry;
use crate::integrations::types::RichMessage;
use crate::integrations::watcher::{WatchEvent, WatchEventType};
use crate::learner::skill_selector::SkillSelector;
use crate::memory::recall::{self, HistoryRecall};
use crate::provider::roles::ModelRoles;
use crate::soul::loader;
use crate::util::truncate_str;

/// Parsed command from a mention.
pub enum DaemonCommand {
    /// Run a task with the given description.
    Run(String),
    /// Report current daemon status.
    Status,
    /// Report token/cost information.
    Cost,
    /// Show available commands.
    Help,
}

/// Target for progress notification delivery during task execution.
pub struct NotifyTarget {
    pub registry: Arc<IntegrationRegistry>,
    pub integration_id: String,
    pub channel: String,
    pub thread_id: Option<String>,
}

/// Parse the extracted mention text into a daemon command.
pub fn parse_command(text: &str) -> DaemonCommand {
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

/// Handle an incoming watch event.
pub async fn handle_watch_event(
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
                truncate_str(&event.payload, 100)
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
                truncate_str(&event.payload, 100)
            );

            let command = parse_command(&event.payload);

            match command {
                DaemonCommand::Help => {
                    deliver_result(
                        &registry,
                        &event.integration_id,
                        &event.source,
                        &status::format_help(),
                        tid,
                    )
                    .await;
                }
                DaemonCommand::Status => {
                    let report = status::format_status(ctx).await;
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
                    let report = status::format_cost(ctx).await;
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
                            &status::format_help(),
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

/// Execute a task through the orchestrator and return the full TaskResult.
pub async fn execute_daemon_task(
    ctx: &DaemonContext,
    registry: &IntegrationRegistry,
    task_description: &str,
    notify: Option<NotifyTarget>,
) -> anyhow::Result<TaskResult> {
    tracing::info!(
        "Daemon executing task: {}",
        truncate_str(task_description, 80)
    );

    let task = TaskInput::new(task_description);

    let engine_config = IterationEngineConfig::from(&ctx.config.iteration);
    let safety = SafetyChecker::from_config(&ctx.config.iteration, &ctx.config.safety);

    // Load soul
    let soul = loader::load_soul();

    // Select relevant skills
    let selector = SkillSelector::new();
    let ranked_skills = selector
        .select(
            &task.description,
            task.category.as_deref(),
            ctx.skill_registry.all(),
            ctx.store.as_ref(),
        )
        .await;

    // Recall from memory
    let recall = match ctx.store {
        Some(ref s) => {
            let token_budget = engine_config.token_budget / 10;
            recall::recall(s, task_description, task.category.as_deref(), token_budget)
                .await
                .unwrap_or_default()
        }
        None => HistoryRecall::default(),
    };

    let session_ctx = SessionContext {
        soul,
        ranked_skills,
        recall,
        tools: ctx.mcp_tools.clone(),
        skill_registry: ctx.skill_registry.clone(),
        conversation_history: None,
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
        use chrono::{Datelike, Timelike};
        let _ = s
            .insert_usage_event(
                uuid::Uuid::new_v4().to_string(),
                "task".to_string(),
                Some("daemon".to_string()),
                Some(task_description.to_string()),
                None,
                Some(result.skills_used.join(", ")),
                Some(result.final_score as f32 as f64),
                chrono::Utc::now().format("%Y-%m-%d").to_string(),
                Some(chrono::Utc::now().hour() as i32),
                Some(chrono::Utc::now().weekday().number_from_monday() as i32),
            )
            .await;
    }

    tracing::info!(
        "Daemon task completed: {} iteration(s), {} tokens",
        result.iterations,
        result.total_tokens,
    );

    Ok(result)
}

/// Deliver a result message back to the integration channel that triggered it.
pub async fn deliver_result(
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
pub async fn deliver_rich_result(
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
