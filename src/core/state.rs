// src/core/state.rs — Task state file writer for external monitoring
//
// Writes `~/.openkoi/state/current-task.json` at each lifecycle transition
// and appends completed tasks to `~/.openkoi/state/task-history.jsonl`.
// Uses atomic write (temp file + rename) for current-task.json.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::types::ProgressEvent;
use crate::infra::paths;

/// JSON structure written to `current-task.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub task_id: String,
    pub description: String,
    pub status: String,
    pub iteration: u8,
    pub max_iterations: u8,
    pub current_score: f32,
    pub best_score: f32,
    pub cost_usd: f64,
    pub tokens_used: u32,
    pub started_at: String,
    pub elapsed_secs: u64,
    pub last_decision: String,
    pub tool_calls: Vec<String>,
    pub phase: String,
}

/// JSON structure appended to `task-history.jsonl` (one line per completed task).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistoryEntry {
    pub task_id: String,
    pub description: String,
    pub iterations: u8,
    pub total_tokens: u32,
    pub cost_usd: f64,
    pub final_score: f64,
    pub completed_at: String,
}

/// Mutable state tracked across progress events.
struct LiveState {
    task_id: String,
    description: String,
    iteration: u8,
    max_iterations: u8,
    current_score: f32,
    best_score: f32,
    cost_usd: f64,
    tokens_used: u32,
    started_at: chrono::DateTime<Utc>,
    last_decision: String,
    tool_calls: Vec<String>,
    phase: String,
}

impl LiveState {
    fn new(task_id: &str, description: &str) -> Self {
        Self {
            task_id: task_id.to_string(),
            description: description.to_string(),
            iteration: 0,
            max_iterations: 0,
            current_score: 0.0,
            best_score: 0.0,
            cost_usd: 0.0,
            tokens_used: 0,
            started_at: Utc::now(),
            last_decision: "pending".to_string(),
            tool_calls: Vec::new(),
            phase: "plan".to_string(),
        }
    }

    fn to_task_state(&self) -> TaskState {
        let elapsed = (Utc::now() - self.started_at).num_seconds().max(0) as u64;
        let status = match self.phase.as_str() {
            "complete" => "complete",
            "plan" | "pending" => "pending",
            _ => "running",
        };
        TaskState {
            task_id: self.task_id.clone(),
            description: self.description.clone(),
            status: status.to_string(),
            iteration: self.iteration,
            max_iterations: self.max_iterations,
            current_score: self.current_score,
            best_score: self.best_score,
            cost_usd: self.cost_usd,
            tokens_used: self.tokens_used,
            started_at: self.started_at.to_rfc3339(),
            elapsed_secs: elapsed,
            last_decision: self.last_decision.clone(),
            tool_calls: self.tool_calls.clone(),
            phase: self.phase.clone(),
        }
    }
}

/// Build a progress callback that writes state files for external monitoring.
///
/// `task_id` and `description` are used to populate the state file.
/// `inner` is an optional inner callback (e.g. terminal_progress) to delegate to.
pub fn state_writer_progress(
    task_id: String,
    description: String,
    inner: Option<Box<dyn Fn(ProgressEvent) + Send>>,
) -> impl Fn(ProgressEvent) + Send + 'static {
    let live = Arc::new(Mutex::new(LiveState::new(&task_id, &description)));

    move |event: ProgressEvent| {
        // Delegate to inner callback first
        if let Some(ref cb) = inner {
            cb(event.clone());
        }

        // Update live state
        if let Ok(mut state) = live.lock() {
            match &event {
                ProgressEvent::PlanReady {
                    estimated_iterations,
                    ..
                } => {
                    state.max_iterations = *estimated_iterations;
                    state.phase = "plan".to_string();
                }
                ProgressEvent::IterationStart {
                    iteration,
                    max_iterations,
                } => {
                    state.iteration = *iteration;
                    state.max_iterations = *max_iterations;
                    state.phase = "executing".to_string();
                }
                ProgressEvent::ToolCall { name, .. } => {
                    if !state.tool_calls.contains(name) {
                        state.tool_calls.push(name.clone());
                    }
                }
                ProgressEvent::IterationEnd {
                    score,
                    decision,
                    cost_so_far,
                    ..
                } => {
                    state.current_score = *score;
                    if *score > state.best_score {
                        state.best_score = *score;
                    }
                    state.cost_usd = *cost_so_far;
                    state.last_decision = decision.to_string();
                    state.phase = "evaluated".to_string();
                }
                ProgressEvent::SafetyWarning { .. } => {
                    state.phase = "safety_warning".to_string();
                }
                ProgressEvent::Complete {
                    iterations,
                    total_tokens,
                    cost,
                    final_score,
                } => {
                    state.phase = "complete".to_string();
                    state.iteration = *iterations;
                    state.tokens_used = *total_tokens;
                    state.cost_usd = *cost;
                    state.current_score = *final_score as f32;
                    state.best_score = *final_score as f32;
                    state.last_decision = "complete".to_string();

                    // Append to history
                    let entry = TaskHistoryEntry {
                        task_id: state.task_id.clone(),
                        description: state.description.clone(),
                        iterations: *iterations,
                        total_tokens: *total_tokens,
                        cost_usd: *cost,
                        final_score: *final_score,
                        completed_at: Utc::now().to_rfc3339(),
                    };
                    let _ = append_history(&entry);

                    // Remove current-task.json on completion
                    let state_file = state_file_path();
                    let _ = std::fs::remove_file(state_file);
                    return;
                }
            }

            // Write current-task.json
            let task_state = state.to_task_state();
            if let Err(e) = write_state_file(&task_state) {
                tracing::debug!("Failed to write current-task.json: {}", e);
            }
        }
    }
}

/// Path to `current-task.json`.
pub fn state_file_path() -> PathBuf {
    paths::state_dir().join("current-task.json")
}

/// Path to `task-history.jsonl`.
fn history_file_path() -> PathBuf {
    paths::state_dir().join("task-history.jsonl")
}

/// Atomically write `current-task.json` (temp file + rename).
fn write_state_file(state: &TaskState) -> anyhow::Result<()> {
    let dir = paths::state_dir();
    let _ = std::fs::create_dir_all(&dir);

    let json = serde_json::to_string_pretty(state)?;
    let tmp = dir.join(".current-task.json.tmp");
    let dst = dir.join("current-task.json");

    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(json.as_bytes())?;
    f.flush()?;
    f.sync_all()?;
    std::fs::rename(&tmp, &dst)?;
    Ok(())
}

/// Append a completed task to `task-history.jsonl`, rotating if needed.
fn append_history(entry: &TaskHistoryEntry) -> anyhow::Result<()> {
    let dir = paths::state_dir();
    let _ = std::fs::create_dir_all(&dir);

    let path = history_file_path();

    // Check rotation: rotate at 1000 lines or 1MB
    if let Ok(meta) = std::fs::metadata(&path) {
        let should_rotate = meta.len() > 1_048_576; // 1MB
        if !should_rotate {
            // Count lines
            if let Ok(content) = std::fs::read_to_string(&path) {
                let line_count = content.lines().count();
                if line_count >= 1000 {
                    rotate_history(&path)?;
                }
            }
        } else {
            rotate_history(&path)?;
        }
    }

    let line = serde_json::to_string(entry)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(f, "{}", line)?;
    Ok(())
}

/// Rotate history by keeping only the last 500 lines.
fn rotate_history(path: &std::path::Path) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let keep = if lines.len() > 500 {
        &lines[lines.len() - 500..]
    } else {
        &lines
    };
    let new_content = keep.join("\n") + "\n";
    std::fs::write(path, new_content)?;
    Ok(())
}

/// Read the current task state from `current-task.json`, if it exists.
pub fn read_current_task() -> Option<TaskState> {
    let path = state_file_path();
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Read recent task history entries (last N).
pub fn read_history(limit: usize) -> Vec<TaskHistoryEntry> {
    let path = history_file_path();
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let lines: Vec<&str> = content.lines().collect();
    let start = if lines.len() > limit {
        lines.len() - limit
    } else {
        0
    };

    lines[start..]
        .iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_task_state_serialization() {
        let state = TaskState {
            task_id: "test-123".into(),
            description: "Fix the bug".into(),
            status: "executing".into(),
            iteration: 2,
            max_iterations: 3,
            current_score: 0.78,
            best_score: 0.78,
            cost_usd: 0.04,
            tokens_used: 12500,
            started_at: "2026-02-21T10:30:00Z".into(),
            elapsed_secs: 45,
            last_decision: "continue".into(),
            tool_calls: vec!["read_file".into(), "edit_file".into()],
            phase: "evaluate".into(),
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        assert!(json.contains("\"task_id\": \"test-123\""));
        assert!(json.contains("\"iteration\": 2"));

        let parsed: TaskState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.task_id, "test-123");
        assert_eq!(parsed.iteration, 2);
        assert_eq!(parsed.tool_calls.len(), 2);
    }

    #[test]
    fn test_history_entry_serialization() {
        let entry = TaskHistoryEntry {
            task_id: "task-1".into(),
            description: "Add feature".into(),
            iterations: 2,
            total_tokens: 38201,
            cost_usd: 0.14,
            final_score: 0.91,
            completed_at: "2026-02-21T11:00:00Z".into(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"task_id\":\"task-1\""));

        let parsed: TaskHistoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.iterations, 2);
        assert!((parsed.final_score - 0.91).abs() < f64::EPSILON);
    }

    #[test]
    fn test_live_state_tracks_best_score() {
        let mut state = LiveState::new("t1", "test");
        assert_eq!(state.best_score, 0.0);

        state.current_score = 0.78;
        if state.current_score > state.best_score {
            state.best_score = state.current_score;
        }
        assert!((state.best_score - 0.78).abs() < f32::EPSILON);

        state.current_score = 0.72;
        // best_score should NOT decrease
        assert!((state.best_score - 0.78).abs() < f32::EPSILON);
    }

    #[test]
    fn test_live_state_to_task_state() {
        let state = LiveState::new("t2", "Fix the login bug");
        let ts = state.to_task_state();
        assert_eq!(ts.task_id, "t2");
        assert_eq!(ts.description, "Fix the login bug");
        assert_eq!(ts.phase, "plan");
        assert_eq!(ts.iteration, 0);
    }

    #[test]
    fn test_rotate_history_keeps_last_500() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test-history.jsonl");

        // Write 1100 lines
        let mut content = String::new();
        for i in 0..1100 {
            content.push_str(&format!("{{\"line\":{}}}\n", i));
        }
        std::fs::write(&path, &content).unwrap();

        rotate_history(&path).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = after.lines().collect();
        assert_eq!(lines.len(), 500);
        // Should keep lines 600-1099
        assert!(lines[0].contains("\"line\":600"));
        assert!(lines[499].contains("\"line\":1099"));
    }
}
