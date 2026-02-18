// src/patterns/miner.rs — Pattern detection from usage events

use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::memory::store::{Store, UsageEventRow};

/// Analyzes usage events to detect recurring patterns.
pub struct PatternMiner<'a> {
    store: &'a Store,
}

/// A detected usage pattern.
#[derive(Debug, Clone)]
pub struct DetectedPattern {
    pub id: String,
    pub pattern_type: PatternType,
    pub description: String,
    pub frequency: Option<String>,
    pub confidence: f32,
    pub sample_count: u32,
}

/// Types of patterns the miner can detect.
#[derive(Debug, Clone)]
pub enum PatternType {
    /// Same task executed repeatedly
    RecurringTask,
    /// Tasks at consistent times of day
    TimeBased,
    /// Chains of tasks executed in order
    Workflow,
}

impl PatternType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::RecurringTask => "recurring",
            Self::TimeBased => "time_based",
            Self::Workflow => "workflow",
        }
    }
}

impl<'a> PatternMiner<'a> {
    pub fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Mine patterns from events in the lookback window.
    pub fn mine(&self, lookback_days: u32) -> anyhow::Result<Vec<DetectedPattern>> {
        let since = (Utc::now() - Duration::days(lookback_days as i64)).to_rfc3339();
        let events = self.store.query_events_since(&since)?;
        let mut patterns = Vec::new();

        // 1. Recurring tasks: group by description similarity
        patterns.extend(self.detect_recurring_tasks(&events));

        // 2. Time-based patterns: tasks at consistent times
        patterns.extend(self.detect_time_patterns(&events));

        // 3. Workflow sequences: chains of tasks in order
        patterns.extend(self.detect_workflows(&events));

        // Filter by confidence and sample count
        patterns.retain(|p| p.confidence >= 0.6 && p.sample_count >= 3);
        Ok(patterns)
    }

    /// Detect tasks that appear repeatedly with similar descriptions.
    fn detect_recurring_tasks(&self, events: &[UsageEventRow]) -> Vec<DetectedPattern> {
        use std::collections::HashMap;

        // Simple grouping by category (more sophisticated: use embeddings)
        let mut category_counts: HashMap<String, Vec<&UsageEventRow>> = HashMap::new();
        for event in events {
            if let Some(ref cat) = event.category {
                category_counts.entry(cat.clone()).or_default().push(event);
            }
        }

        let mut patterns = Vec::new();
        for (category, group_events) in &category_counts {
            if group_events.len() >= 3 {
                let confidence = (group_events.len() as f32 / events.len().max(1) as f32).min(0.95);
                patterns.push(DetectedPattern {
                    id: Uuid::new_v4().to_string(),
                    pattern_type: PatternType::RecurringTask,
                    description: format!("{category} tasks"),
                    frequency: Some(detect_frequency(group_events)),
                    confidence,
                    sample_count: group_events.len() as u32,
                });
            }
        }
        patterns
    }

    /// Detect time-of-day patterns.
    fn detect_time_patterns(&self, events: &[UsageEventRow]) -> Vec<DetectedPattern> {
        use std::collections::HashMap;

        // Group by hour and day of week
        let mut hour_counts: HashMap<i32, u32> = HashMap::new();
        for event in events {
            if let Some(hour) = event.hour {
                *hour_counts.entry(hour).or_default() += 1;
            }
        }

        let mut patterns = Vec::new();
        let total = events.len() as f32;
        for (hour, count) in &hour_counts {
            let ratio = *count as f32 / total;
            // If >30% of events happen at this hour, it's a time pattern
            if ratio > 0.3 && *count >= 3 {
                patterns.push(DetectedPattern {
                    id: Uuid::new_v4().to_string(),
                    pattern_type: PatternType::TimeBased,
                    description: format!("Tasks concentrated at {hour:02}:00"),
                    frequency: Some(format!("daily around {hour:02}:00")),
                    confidence: ratio.min(0.95),
                    sample_count: *count,
                });
            }
        }
        patterns
    }

    /// Detect sequential workflow patterns.
    fn detect_workflows(&self, events: &[UsageEventRow]) -> Vec<DetectedPattern> {
        // Simplified: detect sequences of category transitions
        // A full implementation would use sequence mining (e.g., GSP algorithm)
        if events.len() < 6 {
            return Vec::new();
        }

        let mut patterns = Vec::new();
        let categories: Vec<Option<&str>> = events.iter().map(|e| e.category.as_deref()).collect();

        // Look for repeated pairs
        use std::collections::HashMap;
        let mut pair_counts: HashMap<(String, String), u32> = HashMap::new();
        for window in categories.windows(2) {
            if let [Some(a), Some(b)] = window {
                *pair_counts
                    .entry((a.to_string(), b.to_string()))
                    .or_default() += 1;
            }
        }

        for ((a, b), count) in &pair_counts {
            if *count >= 3 {
                patterns.push(DetectedPattern {
                    id: Uuid::new_v4().to_string(),
                    pattern_type: PatternType::Workflow,
                    description: format!("{a} -> {b}"),
                    frequency: Some(format!("{count}x observed")),
                    confidence: (*count as f32 / events.len() as f32 * 2.0).min(0.95),
                    sample_count: *count,
                });
            }
        }
        patterns
    }

    /// Save detected patterns to the database.
    pub fn persist_patterns(&self, patterns: &[DetectedPattern]) -> anyhow::Result<()> {
        for p in patterns {
            self.store.insert_usage_pattern(
                &p.id,
                p.pattern_type.as_str(),
                &p.description,
                p.frequency.as_deref(),
                None, // trigger_json
                p.confidence as f64,
                p.sample_count as i32,
            )?;
        }
        Ok(())
    }
}

/// Detect frequency label from a group of events.
fn detect_frequency(events: &[&UsageEventRow]) -> String {
    if events.len() < 2 {
        return "once".into();
    }

    // Check day_of_week distribution
    let mut days: std::collections::HashSet<i32> = std::collections::HashSet::new();
    for e in events {
        if let Some(dow) = e.day_of_week {
            days.insert(dow);
        }
    }

    if days.len() >= 5 {
        "daily".into()
    } else if days.len() >= 2 {
        format!("{}x/week", days.len())
    } else {
        "weekly".into()
    }
}
