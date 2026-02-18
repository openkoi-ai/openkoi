// src/infra/daemon.rs — Background daemon for automated task execution
//
// The daemon runs in the background, processing watch events from
// integrations and executing automated patterns.

use std::sync::Arc;

use crate::infra::config::Config;
use crate::integrations::registry::IntegrationRegistry;
use crate::integrations::watcher::{WatchConfig, WatchEvent, WatchEventType, WatcherManager};

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

/// Run the daemon loop — polls integrations and dispatches events.
///
/// This is a long-running async task designed to be the main entry point
/// when `openkoi daemon start` is called.
pub async fn run_daemon(config: &Config, registry: Arc<IntegrationRegistry>) -> anyhow::Result<()> {
    tracing::info!("OpenKoi daemon starting...");

    // Build watcher configs from integration config
    let watch_configs = build_watch_configs(config);

    if watch_configs.is_empty() {
        tracing::warn!("No integrations configured for watching. Daemon has nothing to do.");
        println!("No integrations configured. Use `openkoi connect <app>` to set up integrations.");
        return Ok(());
    }

    let mut watcher_manager = WatcherManager::new();
    for wc in watch_configs {
        watcher_manager.add_watch(wc);
    }

    let mut event_rx = watcher_manager.start(registry);

    // Set up signal handler for graceful shutdown
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    println!("Daemon running. Press Ctrl+C to stop.");

    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                handle_watch_event(&event, config).await;
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

/// Handle an incoming watch event.
async fn handle_watch_event(event: &WatchEvent, _config: &Config) {
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

            // TODO: Auto-execute a task when mentioned
            // This is where learned patterns from Phase 2 get triggered automatically.
        }
    }
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
