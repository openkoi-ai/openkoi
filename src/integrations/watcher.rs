// src/integrations/watcher.rs — Background watchers for integration events
//
// Watchers poll integrations for new messages/events and trigger
// automated actions based on learned patterns.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::integrations::registry::IntegrationRegistry;

/// Event emitted by a watcher when something interesting happens.
#[derive(Debug, Clone)]
pub struct WatchEvent {
    /// Which integration generated the event
    pub integration_id: String,
    /// Type of event
    pub event_type: WatchEventType,
    /// Event payload (e.g., message content, document change)
    pub payload: String,
    /// Channel/document/source ID
    pub source: String,
    /// Thread identifier for reply-in-thread (Slack thread_ts, Telegram message_id, etc.)
    pub thread_id: Option<String>,
}

#[derive(Debug, Clone)]
pub enum WatchEventType {
    /// New message received
    NewMessage,
    /// Document was updated
    DocumentUpdated,
    /// Mention or direct message to the bot
    Mention,
}

/// Configuration for a specific watcher.
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// Integration to watch
    pub integration_id: String,
    /// Channels/documents to monitor
    pub targets: Vec<String>,
    /// Poll interval in seconds
    pub poll_interval_secs: u64,
    /// Whether to only trigger on mentions
    pub mentions_only: bool,
}

/// Manages background polling watchers for all configured integrations.
pub struct WatcherManager {
    configs: Vec<WatchConfig>,
    shutdown_tx: Option<tokio::sync::broadcast::Sender<()>>,
}

impl Default for WatcherManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WatcherManager {
    pub fn new() -> Self {
        Self {
            configs: Vec::new(),
            shutdown_tx: None,
        }
    }

    /// Add a watcher configuration.
    pub fn add_watch(&mut self, config: WatchConfig) {
        self.configs.push(config);
    }

    /// Start all configured watchers, returning a channel that receives events.
    pub fn start(&mut self, registry: Arc<IntegrationRegistry>) -> mpsc::Receiver<WatchEvent> {
        let (event_tx, event_rx) = mpsc::channel(100);
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        for config in &self.configs {
            let config = config.clone();
            let registry = registry.clone();
            let tx = event_tx.clone();
            let mut shutdown_rx = shutdown_tx.subscribe();

            tokio::spawn(async move {
                let interval = Duration::from_secs(config.poll_interval_secs);
                // Track last-seen message IDs per target to avoid re-emitting
                let mut last_seen: HashMap<String, String> = HashMap::new();
                tracing::info!(
                    "Watcher started for {} ({}s interval)",
                    config.integration_id,
                    config.poll_interval_secs
                );

                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(interval) => {
                            if let Err(e) = poll_integration(&registry, &config, &tx, &mut last_seen).await {
                                tracing::warn!(
                                    "Watcher poll failed for {}: {}",
                                    config.integration_id,
                                    e
                                );
                            }
                        }
                        _ = shutdown_rx.recv() => {
                            tracing::info!(
                                "Watcher stopping for {}",
                                config.integration_id
                            );
                            break;
                        }
                    }
                }
            });
        }

        event_rx
    }

    /// Stop all running watchers.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Check if any watchers are configured.
    pub fn has_watchers(&self) -> bool {
        !self.configs.is_empty()
    }
}

/// Poll a single integration for new events.
/// `last_seen` tracks the last-seen message ID per target to avoid
/// re-emitting duplicate events across poll cycles.
async fn poll_integration(
    registry: &IntegrationRegistry,
    config: &WatchConfig,
    tx: &mpsc::Sender<WatchEvent>,
    last_seen: &mut HashMap<String, String>,
) -> anyhow::Result<()> {
    let integration = registry
        .get(&config.integration_id)
        .ok_or_else(|| anyhow::anyhow!("Integration '{}' not found", config.integration_id))?;

    // Poll messaging integrations
    if let Some(messaging) = integration.messaging() {
        for target in &config.targets {
            match messaging.history(target, 5).await {
                Ok(messages) => {
                    let prev_last = last_seen.get(target).cloned();

                    // Find new messages: everything after the last-seen ID.
                    // Messages are assumed to be in chronological order.
                    let new_msgs: Vec<_> = if let Some(ref last_id) = prev_last {
                        // Skip until we pass the last-seen message
                        let skip_pos = messages.iter().position(|m| m.id == *last_id);
                        match skip_pos {
                            Some(pos) => messages.into_iter().skip(pos + 1).collect(),
                            // last_id not found in batch — emit all (may have scrolled past)
                            None => messages,
                        }
                    } else {
                        // First poll — don't emit historical messages, just record latest
                        if let Some(last) = messages.last() {
                            last_seen.insert(target.clone(), last.id.clone());
                        }
                        Vec::new()
                    };

                    // Update last-seen to the newest message
                    if let Some(newest) = new_msgs.last() {
                        last_seen.insert(target.clone(), newest.id.clone());
                    }

                    for msg in new_msgs {
                        let (event_type, payload) =
                            match detect_mention(&msg.content, &config.integration_id) {
                                Some(command_text) => {
                                    (WatchEventType::Mention, command_text)
                                }
                                None => {
                                    if config.mentions_only {
                                        continue;
                                    }
                                    (WatchEventType::NewMessage, msg.content)
                                }
                            };

                        let event = WatchEvent {
                            integration_id: config.integration_id.clone(),
                            event_type,
                            payload,
                            source: msg.channel,
                            thread_id: msg.thread_id,
                        };

                        if tx.send(event).await.is_err() {
                            // Receiver dropped
                            return Ok(());
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        "Watcher: failed to poll {}/{}: {}",
                        config.integration_id,
                        target,
                        e
                    );
                }
            }
        }
    }

    // Poll document integrations — use content hash to detect changes
    if let Some(docs) = integration.document() {
        for target in &config.targets {
            match docs.read(target).await {
                Ok(doc) => {
                    // Hash the content to detect changes
                    let content_hash = format!("{:x}", simple_hash(&doc.content));
                    let doc_key = format!("doc:{target}");
                    let prev_hash = last_seen.get(&doc_key).cloned();

                    if prev_hash.as_deref() == Some(&content_hash) {
                        // No change, skip
                        continue;
                    }

                    last_seen.insert(doc_key, content_hash);

                    // Skip event on first poll (just record baseline)
                    if prev_hash.is_none() {
                        continue;
                    }

                    let event = WatchEvent {
                        integration_id: config.integration_id.clone(),
                        event_type: WatchEventType::DocumentUpdated,
                        payload: doc.content,
                        source: doc.id,
                        thread_id: None,
                    };

                    if tx.send(event).await.is_err() {
                        return Ok(());
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        "Watcher: failed to poll doc {}/{}: {}",
                        config.integration_id,
                        target,
                        e
                    );
                }
            }
        }
    }

    Ok(())
}

/// Detect whether a message is a mention / command directed at OpenKoi.
///
/// Returns `Some(command_text)` with the trigger prefix stripped when a
/// mention is detected, or `None` when the message is not addressed to us.
///
/// Trigger patterns by integration:
/// - slack / discord / msteams: `@openkoi` anywhere in the message
/// - telegram: `/koi` command or `@openkoi_bot` mention
/// - imessage: message starts with `koi:` prefix
/// - fallback: `@openkoi` anywhere
fn detect_mention(content: &str, integration_id: &str) -> Option<String> {
    let lower = content.to_lowercase();

    match integration_id {
        "slack" | "discord" | "msteams" => {
            // Look for @openkoi (with optional surrounding angle brackets for Slack)
            let patterns = ["@openkoi", "<@openkoi>"];
            for pat in patterns {
                if let Some(pos) = lower.find(pat) {
                    let after = content[pos + pat.len()..].trim().to_string();
                    let before = content[..pos].trim();
                    let command = if after.is_empty() {
                        before.to_string()
                    } else {
                        after
                    };
                    return Some(command);
                }
            }
            None
        }
        "telegram" => {
            // /koi command or @openkoi_bot mention
            if lower.starts_with("/koi") {
                let rest = content["/koi".len()..].trim();
                // Handle /koi@openkoi_bot (Telegram group format)
                let rest = rest
                    .strip_prefix("@openkoi_bot")
                    .map(|s| s.trim())
                    .unwrap_or(rest);
                return Some(rest.to_string());
            }
            if let Some(pos) = lower.find("@openkoi_bot") {
                let after = content[pos + "@openkoi_bot".len()..].trim();
                let before = content[..pos].trim();
                let command = if after.is_empty() {
                    before.to_string()
                } else {
                    after.to_string()
                };
                return Some(command);
            }
            None
        }
        "imessage" => {
            // Messages starting with "koi:" prefix
            let trimmed = content.trim();
            let lower_trimmed = trimmed.to_lowercase();
            if lower_trimmed.starts_with("koi:") {
                let rest = trimmed["koi:".len()..].trim();
                return Some(rest.to_string());
            }
            None
        }
        _ => {
            // Fallback: look for @openkoi
            if let Some(pos) = lower.find("@openkoi") {
                let after = content[pos + "@openkoi".len()..].trim().to_string();
                let before = content[..pos].trim();
                let command = if after.is_empty() {
                    before.to_string()
                } else {
                    after
                };
                return Some(command);
            }
            None
        }
    }
}

/// Simple non-cryptographic hash for change detection.
fn simple_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}
