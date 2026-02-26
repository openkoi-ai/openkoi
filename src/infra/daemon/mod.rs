// src/infra/daemon/mod.rs

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use crate::api;
use crate::infra::config::Config;
use crate::integrations::registry::IntegrationRegistry;
use crate::integrations::watcher::{WatchConfig, WatcherManager};
use crate::memory::StoreHandle;
use crate::provider::{ModelProvider, ModelRef, ToolDef};
use crate::skills::registry::SkillRegistry;

pub mod handler;
pub mod process;
pub mod scheduler;
pub mod status;

// Re-export key functions for external use
pub use handler::execute_daemon_task;
pub use process::{is_daemon_running, remove_pid_file, write_pid_file};

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
    pub store: Option<StoreHandle>,
    pub skill_registry: Arc<SkillRegistry>,
    pub mcp_tools: Vec<ToolDef>,
}

/// Run the daemon loop — polls integrations and dispatches events.
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

    // Shared task queue and cancel set — shared between API server and daemon loop.
    let shared_task_queue: Arc<Mutex<Vec<api::TaskRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let shared_cancel_requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    if api_config.enabled {
        let api_state = api::ApiState {
            store: ctx.store.clone(),
            token: api_config.token.clone(),
            task_queue: shared_task_queue.clone(),
            cancel_requests: shared_cancel_requests.clone(),
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

    // API task queue polling interval (check for new tasks every 2 seconds)
    let mut queue_interval = tokio::time::interval(Duration::from_secs(2));
    queue_interval.tick().await;

    println!("Daemon running. Press Ctrl+C to stop.");

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                handler::handle_watch_event(&event, &ctx, registry.clone(), auto_execute, &webhook_config).await;
            }
            _ = cron_interval.tick() => {
                scheduler::run_scheduled_patterns(&ctx, &registry, &webhook_config).await;
            }
            _ = queue_interval.tick() => {
                // Drain tasks submitted via the HTTP API
                let tasks: Vec<api::TaskRequest> = {
                    if let Ok(mut queue) = shared_task_queue.lock() {
                        queue.drain(..).collect()
                    } else {
                        Vec::new()
                    }
                };
                for task_req in tasks {
                    let task_id = task_req.task_id.clone().unwrap_or_default();
                    tracing::info!("API task dequeued [{}]: {}", task_id, crate::util::truncate_str(&task_req.description, 80));
                    match handler::execute_daemon_task(&ctx, &registry, &task_req.description, None).await {
                        Ok(result) => {
                            tracing::info!(
                                "API task [{}] completed: {} iter, score {:.2}",
                                task_id, result.iterations, result.final_score
                            );
                            api::webhooks::fire_webhook(
                                &webhook_config,
                                api::webhooks::WebhookEvent::TaskComplete {
                                    task_id: task_id.clone(),
                                    description: task_req.description.clone(),
                                    iterations: result.iterations,
                                    final_score: result.final_score,
                                    cost_usd: result.cost,
                                    total_tokens: result.total_tokens,
                                },
                            );
                        }
                        Err(e) => {
                            tracing::error!("API task [{}] failed: {}", task_id, e);
                            api::webhooks::fire_webhook(
                                &webhook_config,
                                api::webhooks::WebhookEvent::TaskFailed {
                                    task_id: task_id.clone(),
                                    description: task_req.description.clone(),
                                    error: format!("{e}"),
                                },
                            );
                        }
                    }
                }

                // Check for cancel requests
                let cancel_ids: Vec<String> = {
                    if let Ok(mut cancels) = shared_cancel_requests.lock() {
                        cancels.drain(..).collect()
                    } else {
                        Vec::new()
                    }
                };
                for cid in cancel_ids {
                    tracing::info!("Cancel requested for task [{}] (best-effort)", cid);
                }
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
                targets: notion.channels.clone(),
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
