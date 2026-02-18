// src/memory/decay.rs — Learning confidence decay

use crate::memory::store::{LearningRow, Store};
use chrono::{DateTime, Utc};

/// Apply exponential decay to learning confidence based on time since last use.
///
/// Learnings that are frequently reinforced maintain high confidence.
/// Unreinforced learnings decay and are eventually pruned.
pub fn apply_decay(learnings: &mut Vec<LearningRow>, rate_per_week: f32) {
    let now = Utc::now();

    for learning in learnings.iter_mut() {
        if let Some(last_used_str) = &learning.last_used {
            if let Ok(last_used) = DateTime::parse_from_rfc3339(last_used_str) {
                let weeks_since = (now - last_used.with_timezone(&Utc)).num_days() as f32 / 7.0;
                let decay = (-rate_per_week * weeks_since).exp();
                learning.confidence *= decay as f64;
            }
        }
    }

    // Prune learnings below 0.1 confidence
    learnings.retain(|l| l.confidence >= 0.1);
}

/// Calculate what a learning's confidence would be after decay (without mutating).
pub fn decayed_confidence(confidence: f64, last_used: &str, rate_per_week: f32) -> f64 {
    let now = Utc::now();
    if let Ok(last_used) = DateTime::parse_from_rfc3339(last_used) {
        let weeks_since = (now - last_used.with_timezone(&Utc)).num_days() as f32 / 7.0;
        let decay = (-rate_per_week * weeks_since).exp();
        confidence * decay as f64
    } else {
        confidence
    }
}

/// Run the full decay cycle: query all learnings, apply decay, persist changes.
///
/// Returns the number of learnings pruned (confidence dropped below threshold).
pub fn run_decay(store: &Store, rate_per_week: f32) -> anyhow::Result<usize> {
    let mut learnings = store.query_all_learnings()?;

    // Remember all IDs before decay
    let all_ids: Vec<String> = learnings.iter().map(|l| l.id.clone()).collect();

    apply_decay(&mut learnings, rate_per_week);

    // Build set of surviving IDs
    let surviving_ids: std::collections::HashSet<&str> =
        learnings.iter().map(|l| l.id.as_str()).collect();

    // Update confidence for surviving learnings
    for learning in &learnings {
        store.update_learning_confidence(&learning.id, learning.confidence)?;
    }

    // Delete pruned learnings (those in all_ids but not in surviving_ids)
    let mut pruned = 0;
    for id in &all_ids {
        if !surviving_ids.contains(id.as_str()) {
            store.delete_learning(id)?;
            pruned += 1;
        }
    }

    if pruned > 0 {
        tracing::info!("Decay: pruned {} low-confidence learnings", pruned);
    }

    Ok(pruned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn learning_row(confidence: f64, last_used: Option<String>) -> LearningRow {
        LearningRow {
            id: "test-1".into(),
            learning_type: "heuristic".into(),
            content: "test learning".into(),
            category: None,
            confidence,
            source_task: None,
            reinforced: 0,
            last_used,
        }
    }

    #[test]
    fn test_no_decay_recent() {
        let now = Utc::now().to_rfc3339();
        let mut learnings = vec![learning_row(0.9, Some(now))];
        apply_decay(&mut learnings, 0.1);
        // Should be very close to 0.9 (almost no time elapsed)
        assert!(learnings[0].confidence > 0.85);
        assert_eq!(learnings.len(), 1);
    }

    #[test]
    fn test_decay_old_learning() {
        let four_weeks_ago = (Utc::now() - Duration::weeks(4)).to_rfc3339();
        let mut learnings = vec![learning_row(0.9, Some(four_weeks_ago))];
        apply_decay(&mut learnings, 0.3); // aggressive decay
                                          // After 4 weeks at 0.3/week rate: 0.9 * e^(-0.3*4) = 0.9 * 0.301 ≈ 0.271
        assert!(learnings[0].confidence < 0.4);
    }

    #[test]
    fn test_prune_very_old() {
        let long_ago = (Utc::now() - Duration::weeks(52)).to_rfc3339();
        let mut learnings = vec![learning_row(0.5, Some(long_ago))];
        apply_decay(&mut learnings, 0.1);
        // After a year with moderate decay: confidence should drop below 0.1 → pruned
        assert!(learnings.is_empty());
    }

    #[test]
    fn test_no_decay_without_last_used() {
        let mut learnings = vec![learning_row(0.8, None)];
        apply_decay(&mut learnings, 0.1);
        // No last_used → no decay applied
        assert_eq!(learnings.len(), 1);
        assert!((learnings[0].confidence - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_decayed_confidence_calculation() {
        let now = Utc::now().to_rfc3339();
        let result = decayed_confidence(1.0, &now, 0.1);
        assert!(result > 0.99); // almost no time elapsed

        let result_bad_date = decayed_confidence(1.0, "not-a-date", 0.1);
        assert!((result_bad_date - 1.0).abs() < f64::EPSILON); // returns original on parse error
    }
}
