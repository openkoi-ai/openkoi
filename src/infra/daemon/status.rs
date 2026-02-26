// src/infra/daemon/status.rs

use crate::infra::daemon::DaemonContext;
use crate::util::truncate_str;
use chrono;

/// Format a status report from the store.
pub async fn format_status(ctx: &DaemonContext) -> String {
    let store = match ctx.store.as_ref() {
        Some(s) => s,
        None => return "No store configured — task history unavailable.".to_string(),
    };

    // Query recent events
    let since = (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
    let events = match store.query_events_since(&since).await {
        Ok(e) => e,
        Err(e) => return format!("Failed to query events: {}", e),
    };

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
        lines.push(format!("  • {}{}", truncate_str(desc, 60), score_str));
    }

    lines.join("\n")
}

/// Format a cost report from the store.
pub async fn format_cost(ctx: &DaemonContext) -> String {
    let store = match ctx.store.as_ref() {
        Some(s) => s,
        None => return "No store configured — cost data unavailable.".to_string(),
    };

    let since = (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
    let events = match store.query_events_since(&since).await {
        Ok(e) => e,
        Err(e) => return format!("Failed to query events: {}", e),
    };

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
pub fn format_help() -> String {
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
