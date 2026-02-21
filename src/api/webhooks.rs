// src/api/webhooks.rs — Outbound webhook callbacks on lifecycle events
//
// Fires HTTP POST requests to configured URLs when tasks complete,
// fail, or trigger budget warnings. Non-blocking (spawns a tokio task).

use serde::Serialize;

use crate::infra::config::WebhookConfig;

/// Lifecycle event that can trigger a webhook.
#[derive(Debug, Clone)]
pub enum WebhookEvent {
    /// A task completed successfully.
    TaskComplete {
        task_id: String,
        description: String,
        iterations: u8,
        final_score: f64,
        cost_usd: f64,
        total_tokens: u32,
    },
    /// A task failed with an error.
    TaskFailed {
        task_id: String,
        description: String,
        error: String,
    },
    /// The cost budget is running low.
    BudgetWarning {
        current_cost: f64,
        budget_limit: f64,
        percentage_used: f64,
    },
}

/// JSON payload sent to the webhook URL.
#[derive(Debug, Serialize)]
struct WebhookPayload {
    event: String,
    timestamp: String,
    data: serde_json::Value,
}

/// Fire a webhook if a URL is configured for the given event type.
///
/// This spawns a background tokio task so it never blocks the caller.
pub fn fire_webhook(config: &WebhookConfig, event: WebhookEvent) {
    let url = match &event {
        WebhookEvent::TaskComplete { .. } => config.on_task_complete.clone(),
        WebhookEvent::TaskFailed { .. } => config.on_task_failed.clone(),
        WebhookEvent::BudgetWarning { .. } => config.on_budget_warning.clone(),
    };

    let Some(url) = url else {
        return; // No URL configured for this event type
    };

    let payload = build_payload(&event);

    tokio::spawn(async move {
        if let Err(e) = send_webhook(&url, &payload).await {
            tracing::warn!("Webhook delivery to {} failed: {}", url, e);
        }
    });
}

/// Build the JSON payload for a webhook event.
fn build_payload(event: &WebhookEvent) -> WebhookPayload {
    let (event_name, data) = match event {
        WebhookEvent::TaskComplete {
            task_id,
            description,
            iterations,
            final_score,
            cost_usd,
            total_tokens,
        } => (
            "task.complete",
            serde_json::json!({
                "task_id": task_id,
                "description": description,
                "iterations": iterations,
                "final_score": final_score,
                "cost_usd": cost_usd,
                "total_tokens": total_tokens,
            }),
        ),
        WebhookEvent::TaskFailed {
            task_id,
            description,
            error,
        } => (
            "task.failed",
            serde_json::json!({
                "task_id": task_id,
                "description": description,
                "error": error,
            }),
        ),
        WebhookEvent::BudgetWarning {
            current_cost,
            budget_limit,
            percentage_used,
        } => (
            "budget.warning",
            serde_json::json!({
                "current_cost": current_cost,
                "budget_limit": budget_limit,
                "percentage_used": percentage_used,
            }),
        ),
    };

    WebhookPayload {
        event: event_name.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        data,
    }
}

/// Send the webhook POST request.
async fn send_webhook(url: &str, payload: &WebhookPayload) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .header("user-agent", format!("openkoi/{}", env!("CARGO_PKG_VERSION")))
        .json(payload)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!(
            "Webhook returned HTTP {}: {}",
            status.as_u16(),
            truncate(&body, 200)
        );
    } else {
        tracing::debug!("Webhook delivered to {} (HTTP {})", url, status.as_u16());
    }

    Ok(())
}

/// Truncate a string for logging.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_payload_task_complete() {
        let event = WebhookEvent::TaskComplete {
            task_id: "t-123".into(),
            description: "Fix the bug".into(),
            iterations: 3,
            final_score: 0.92,
            cost_usd: 0.14,
            total_tokens: 38201,
        };

        let payload = build_payload(&event);
        assert_eq!(payload.event, "task.complete");
        assert_eq!(payload.data["task_id"], "t-123");
        assert_eq!(payload.data["iterations"], 3);
        assert!(!payload.timestamp.is_empty());
    }

    #[test]
    fn test_build_payload_task_failed() {
        let event = WebhookEvent::TaskFailed {
            task_id: "t-456".into(),
            description: "Deploy app".into(),
            error: "Connection timeout".into(),
        };

        let payload = build_payload(&event);
        assert_eq!(payload.event, "task.failed");
        assert_eq!(payload.data["error"], "Connection timeout");
    }

    #[test]
    fn test_build_payload_budget_warning() {
        let event = WebhookEvent::BudgetWarning {
            current_cost: 1.85,
            budget_limit: 2.00,
            percentage_used: 92.5,
        };

        let payload = build_payload(&event);
        assert_eq!(payload.event, "budget.warning");
        assert_eq!(payload.data["percentage_used"], 92.5);
    }

    #[test]
    fn test_fire_webhook_no_url_configured() {
        // With no URLs configured, fire_webhook should just return without panicking
        let config = WebhookConfig::default();
        let event = WebhookEvent::TaskComplete {
            task_id: "t-1".into(),
            description: "test".into(),
            iterations: 1,
            final_score: 0.9,
            cost_usd: 0.01,
            total_tokens: 1000,
        };

        // This should not panic — it just returns immediately
        fire_webhook(&config, event);
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
    fn test_webhook_payload_serialization() {
        let payload = WebhookPayload {
            event: "task.complete".into(),
            timestamp: "2026-02-21T10:00:00Z".into(),
            data: serde_json::json!({"task_id": "t-1"}),
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"event\":\"task.complete\""));
        assert!(json.contains("\"timestamp\":\"2026-02-21T10:00:00Z\""));
    }
}
