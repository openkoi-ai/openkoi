// src/api/handlers.rs

use crate::api::{auth, types::*, ApiState};
use crate::core::state::{self, TaskHistoryEntry};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;

/// POST /api/v1/tasks — Create a new task (queued for daemon execution).
pub async fn create_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(body): Json<TaskRequest>,
) -> Result<(StatusCode, Json<TaskCreatedResponse>), (StatusCode, Json<ErrorResponse>)> {
    auth::check_auth(&state, &headers)?;

    if body.description.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Task description cannot be empty".into(),
            }),
        ));
    }

    let task_id = uuid::Uuid::new_v4().to_string();

    // Enqueue the task for the daemon to pick up
    if let Ok(mut queue) = state.task_queue.lock() {
        let mut req = body.clone();
        req.task_id = Some(task_id.clone());
        queue.push(req);
    } else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Internal error: failed to enqueue task".into(),
            }),
        ));
    }

    Ok((
        StatusCode::CREATED,
        Json(TaskCreatedResponse {
            task_id,
            status: "queued".into(),
            message: format!("Task queued: {}", body.description),
        }),
    ))
}

/// GET /api/v1/tasks — List recent tasks from history.
pub async fn list_tasks(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TaskHistoryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    auth::check_auth(&state, &headers)?;

    let history = state::read_history(50);
    Ok(Json(history))
}

/// GET /api/v1/tasks/:id — Get a specific task by ID.
pub async fn get_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    auth::check_auth(&state, &headers)?;

    // Check active task first
    if let Some(current) = state::read_current_task() {
        if current.task_id == id {
            let json = serde_json::to_value(&current).unwrap_or_default();
            return Ok(Json(json));
        }
    }

    // Check history
    let history = state::read_history(1000);
    for entry in &history {
        if entry.task_id == id {
            let json = serde_json::to_value(entry).unwrap_or_default();
            return Ok(Json(json));
        }
    }

    Err((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: format!("Task '{id}' not found"),
        }),
    ))
}

/// POST /api/v1/tasks/:id/cancel — Request cancellation of a running task.
pub async fn cancel_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    auth::check_auth(&state, &headers)?;

    // Verify task exists and is active
    let active = state::read_current_task();
    let is_active = active.as_ref().is_some_and(|t| t.task_id == id);

    if !is_active {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("No active task with ID '{id}'"),
            }),
        ));
    }

    // Record cancel request
    if let Ok(mut cancels) = state.cancel_requests.lock() {
        if !cancels.contains(&id) {
            cancels.push(id.clone());
        }
    }

    Ok(Json(serde_json::json!({
        "task_id": id,
        "status": "cancel_requested",
        "message": "Cancel request recorded. Task will stop at next iteration boundary."
    })))
}

/// GET /api/v1/status — System status including active task and daily summary.
pub async fn get_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<SystemStatus>, (StatusCode, Json<ErrorResponse>)> {
    auth::check_auth(&state, &headers)?;

    let active_task = state::read_current_task();
    let history = state::read_history(100);

    // Count tasks completed today
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let tasks_today: Vec<&TaskHistoryEntry> = history
        .iter()
        .filter(|e| e.completed_at.starts_with(&today))
        .collect();
    let total_cost_today: f64 = tasks_today.iter().map(|e| e.cost_usd).sum();

    Ok(Json(SystemStatus {
        version: env!("CARGO_PKG_VERSION").to_string(),
        daemon_running: true,
        active_task,
        tasks_completed_today: tasks_today.len(),
        total_cost_today,
    }))
}

/// GET /api/v1/cost — Cost summary from the store.
pub async fn get_cost(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<CostSummary>, (StatusCode, Json<ErrorResponse>)> {
    auth::check_auth(&state, &headers)?;

    // Query from store if available
    let (total_events, task_events) = if let Some(ref store) = state.store {
        if let Ok(events) = store
            .query_events_since(&(chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339())
            .await
        {
            let total = events.len();
            let tasks = events.iter().filter(|e| e.event_type == "task").count();
            (total, tasks)
        } else {
            (0, 0)
        }
    } else {
        (0, 0)
    };

    let recent_tasks = state::read_history(20);

    Ok(Json(CostSummary {
        total_events_24h: total_events,
        task_events_24h: task_events,
        recent_tasks,
    }))
}

/// GET /api/v1/health — Simple health check.
pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
