// src/patterns/event_logger.rs — Usage event recording

use chrono::{Datelike, Timelike, Utc};
use uuid::Uuid;

use crate::memory::store::Store;

/// Records usage events with minimal overhead.
pub struct EventLogger<'a> {
    store: &'a Store,
}

/// Types of events the system tracks.
#[derive(Debug, Clone)]
pub enum EventType {
    Task,
    Command,
    SkillUse,
    Integration,
}

impl EventType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Task => "task",
            Self::Command => "command",
            Self::SkillUse => "skill_use",
            Self::Integration => "integration",
        }
    }
}

/// A usage event to be logged.
#[derive(Debug, Clone)]
pub struct UsageEvent {
    pub event_type: EventType,
    pub channel: String,
    pub description: String,
    pub category: Option<String>,
    pub skills_used: Vec<String>,
    pub score: Option<f32>,
}

impl<'a> EventLogger<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Log a usage event with automatic timestamp and time decomposition.
    pub fn log(&self, event: &UsageEvent) -> anyhow::Result<()> {
        let now = Utc::now();
        let id = Uuid::new_v4().to_string();
        let day = now.format("%Y-%m-%d").to_string();
        let hour = now.hour() as i32;
        let day_of_week = now.weekday().num_days_from_monday() as i32;

        let skills_json = if event.skills_used.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&event.skills_used).unwrap_or_default())
        };

        self.store.insert_usage_event(
            &id,
            event.event_type.as_str(),
            Some(&event.channel),
            Some(&event.description),
            event.category.as_deref(),
            skills_json.as_deref(),
            event.score.map(|s| s as f64),
            &day,
            Some(hour),
            Some(day_of_week),
        )?;

        Ok(())
    }
}
