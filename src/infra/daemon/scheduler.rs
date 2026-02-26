// src/infra/daemon/scheduler.rs

use crate::api::webhooks;
use crate::infra::daemon::{execute_daemon_task, DaemonContext};
use crate::integrations::registry::IntegrationRegistry;

/// Evaluate approved patterns with cron schedules and execute matching ones.
pub async fn run_scheduled_patterns(
    ctx: &DaemonContext,
    registry: &IntegrationRegistry,
    webhook_config: &crate::infra::config::WebhookConfig,
) {
    let store = match ctx.store.as_ref() {
        Some(s) => s,
        None => return,
    };

    let patterns = match store.query_approved_patterns().await {
        Ok(p) => p,
        Err(e) => {
            tracing::debug!("Failed to query approved patterns: {}", e);
            return;
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
pub fn should_run_now(freq: &str, now: &chrono::DateTime<chrono::Utc>) -> Option<String> {
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
}
