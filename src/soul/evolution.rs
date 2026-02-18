// src/soul/evolution.rs — Soul evolution proposals from accumulated learnings

use std::sync::Arc;

use anyhow::Result;

use crate::memory::store::{LearningRow, Store};
use crate::provider::{ChatRequest, Message, ModelProvider};
use crate::soul::loader::Soul;

/// Manages soul evolution by analyzing accumulated learnings.
pub struct SoulEvolution {
    model: Arc<dyn ModelProvider>,
}

/// A proposed update to the soul document.
pub struct SoulUpdate {
    pub proposed: String,
    pub diff_summary: String,
    pub learning_count: usize,
}

impl SoulEvolution {
    pub fn new(model: Arc<dyn ModelProvider>) -> Self {
        Self { model }
    }

    /// Check if the soul should evolve based on accumulated learnings.
    /// Called periodically (e.g., every 50 tasks).
    pub async fn check_evolution(&self, soul: &Soul, store: &Store) -> Result<Option<SoulUpdate>> {
        let learnings = store.query_high_confidence_learnings(0.8, 20)?;
        let anti_patterns = store.query_learnings_by_type("anti_pattern", 10)?;

        // Not enough signal to evolve
        if learnings.len() < 10 {
            return Ok(None);
        }

        let n_tasks = learnings.len();
        let formatted_learnings = format_learnings(&learnings);
        let formatted_anti = format_learnings(&anti_patterns);

        let response = self
            .model
            .chat(ChatRequest {
                messages: vec![Message::user(format!(
                    "You are reviewing your own soul/identity document. Based on \
                     what you've learned from {n_tasks} tasks, suggest minimal \
                     updates to your soul.\n\n\
                     ## Current Soul\n{soul_raw}\n\n\
                     ## Key Learnings\n{formatted_learnings}\n\n\
                     ## Anti-Patterns Discovered\n{formatted_anti}\n\n\
                     Rules:\n\
                     - Only add/change what you've genuinely learned\n\
                     - Keep the same voice and structure\n\
                     - Max 2-3 small changes\n\
                     - Output the full updated soul document",
                    soul_raw = soul.raw,
                ))],
                max_tokens: Some(3000),
                temperature: Some(0.4),
                ..Default::default()
            })
            .await?;

        let proposed = response.content;

        // Simple diff: count changed lines
        let old_lines: Vec<&str> = soul.raw.lines().collect();
        let new_lines: Vec<&str> = proposed.lines().collect();
        let changed = count_changed_lines(&old_lines, &new_lines);

        // Only propose if changes are meaningful but not drastic
        if changed > 0 && changed < 20 {
            let diff_summary = generate_diff_summary(&soul.raw, &proposed);
            Ok(Some(SoulUpdate {
                proposed,
                diff_summary,
                learning_count: n_tasks,
            }))
        } else {
            Ok(None)
        }
    }
}

pub(crate) fn format_learnings(learnings: &[LearningRow]) -> String {
    learnings
        .iter()
        .enumerate()
        .map(|(i, l)| {
            format!(
                "{}. [{}] {} (confidence: {:.2})",
                i + 1,
                l.learning_type,
                l.content,
                l.confidence,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn count_changed_lines(old: &[&str], new: &[&str]) -> usize {
    let max_len = old.len().max(new.len());
    let mut changed = 0;
    for i in 0..max_len {
        let old_line = old.get(i).copied().unwrap_or("");
        let new_line = new.get(i).copied().unwrap_or("");
        if old_line != new_line {
            changed += 1;
        }
    }
    changed
}

pub(crate) fn generate_diff_summary(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut summary = String::new();
    let max_len = old_lines.len().max(new_lines.len());

    for i in 0..max_len {
        let old_line = old_lines.get(i).copied().unwrap_or("");
        let new_line = new_lines.get(i).copied().unwrap_or("");
        if old_line != new_line {
            if !old_line.is_empty() {
                summary.push_str(&format!("- {old_line}\n"));
            }
            if !new_line.is_empty() {
                summary.push_str(&format!("+ {new_line}\n"));
            }
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn learning(id: &str, content: &str, confidence: f64, ltype: &str) -> LearningRow {
        LearningRow {
            id: id.into(),
            learning_type: ltype.into(),
            content: content.into(),
            category: None,
            confidence,
            source_task: None,
            reinforced: 0,
            last_used: None,
        }
    }

    // ─── count_changed_lines ────────────────────────────────────

    #[test]
    fn test_count_no_changes() {
        let a = vec!["line1", "line2", "line3"];
        assert_eq!(count_changed_lines(&a, &a), 0);
    }

    #[test]
    fn test_count_all_changed() {
        let a = vec!["a", "b", "c"];
        let b = vec!["x", "y", "z"];
        assert_eq!(count_changed_lines(&a, &b), 3);
    }

    #[test]
    fn test_count_changed_different_lengths() {
        let a = vec!["a", "b"];
        let b = vec!["a", "b", "c", "d"];
        assert_eq!(count_changed_lines(&a, &b), 2); // "c" and "d" are new
    }

    #[test]
    fn test_count_changed_shorter_new() {
        let a = vec!["a", "b", "c"];
        let b = vec!["a"];
        assert_eq!(count_changed_lines(&a, &b), 2); // "b" and "c" removed
    }

    #[test]
    fn test_count_changed_empty() {
        let a: Vec<&str> = vec![];
        let b: Vec<&str> = vec![];
        assert_eq!(count_changed_lines(&a, &b), 0);
    }

    // ─── generate_diff_summary ──────────────────────────────────

    #[test]
    fn test_diff_identical() {
        let s = generate_diff_summary("hello\nworld", "hello\nworld");
        assert!(s.is_empty());
    }

    #[test]
    fn test_diff_one_line_changed() {
        let s = generate_diff_summary("hello\nworld", "hello\nearth");
        assert!(s.contains("- world"));
        assert!(s.contains("+ earth"));
    }

    #[test]
    fn test_diff_added_line() {
        let s = generate_diff_summary("a\nb", "a\nb\nc");
        assert!(s.contains("+ c"));
        assert!(!s.contains("- a"));
    }

    #[test]
    fn test_diff_removed_line() {
        let s = generate_diff_summary("a\nb\nc", "a\nb");
        assert!(s.contains("- c"));
    }

    // ─── format_learnings ───────────────────────────────────────

    #[test]
    fn test_format_empty() {
        let result = format_learnings(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_format_single() {
        let learnings = vec![learning("1", "Use Result types", 0.9, "heuristic")];
        let result = format_learnings(&learnings);
        assert!(result.contains("1. [heuristic] Use Result types"));
        assert!(result.contains("0.90"));
    }

    #[test]
    fn test_format_multiple() {
        let learnings = vec![
            learning("1", "First", 0.9, "heuristic"),
            learning("2", "Second", 0.7, "anti_pattern"),
        ];
        let result = format_learnings(&learnings);
        assert!(result.contains("1. [heuristic] First"));
        assert!(result.contains("2. [anti_pattern] Second"));
    }
}
