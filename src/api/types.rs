// src/api/types.rs

use crate::core::state::{TaskHistoryEntry, TaskState};
use serde::{Deserialize, Serialize};

/// Request body for creating a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
    /// Server-assigned task ID (populated on enqueue, not from client JSON).
    #[serde(default)]
    pub task_id: Option<String>,
    pub description: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub max_iterations: Option<u8>,
    #[serde(default)]
    pub quality_threshold: Option<f32>,
}

/// Response for task creation.
#[derive(Debug, Serialize)]
pub struct TaskCreatedResponse {
    pub task_id: String,
    pub status: String,
    pub message: String,
}

/// System status response.
#[derive(Debug, Serialize)]
pub struct SystemStatus {
    pub version: String,
    pub daemon_running: bool,
    pub active_task: Option<TaskState>,
    pub tasks_completed_today: usize,
    pub total_cost_today: f64,
}

/// Cost summary response.
#[derive(Debug, Serialize)]
pub struct CostSummary {
    pub total_events_24h: usize,
    pub task_events_24h: usize,
    pub recent_tasks: Vec<TaskHistoryEntry>,
}

/// Error response body.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}
