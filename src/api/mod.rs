// src/api/mod.rs — Lightweight HTTP API server for external integrations
//
// Runs alongside the daemon on localhost:9742 (configurable).
// Provides task CRUD, status, cost, and cancel endpoints.
// Bearer token auth when configured. CORS headers for local web UIs.

pub mod webhooks;

use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};

use crate::core::state::{self, TaskHistoryEntry, TaskState};
use crate::infra::config::ApiConfig;
use crate::memory::store::Store;

/// Shared state for API handlers.
#[derive(Clone)]
pub struct ApiState {
    pub store: Option<Arc<Mutex<Store>>>,
    pub token: Option<String>,
    /// Queue for tasks submitted via the API — consumed by the daemon loop.
    pub task_queue: Arc<Mutex<Vec<TaskRequest>>>,
    /// Set of task IDs that have been requested to cancel.
    pub cancel_requests: Arc<Mutex<Vec<String>>>,
}

/// Request body for creating a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRequest {
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

/// Build the axum router with all API routes.
pub fn build_router(state: ApiState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/v1/tasks", post(create_task))
        .route("/api/v1/tasks", get(list_tasks))
        .route("/api/v1/tasks/{id}", get(get_task))
        .route("/api/v1/tasks/{id}/cancel", post(cancel_task))
        .route("/api/v1/status", get(get_status))
        .route("/api/v1/cost", get(get_cost))
        .route("/api/v1/health", get(health))
        .layer(cors)
        .with_state(state)
}

/// Start the API server on the given port (blocking — run in a spawned task).
pub async fn start_server(config: &ApiConfig, state: ApiState) -> anyhow::Result<()> {
    let port = config.port;
    let addr = format!("127.0.0.1:{port}");

    let router = build_router(state);

    tracing::info!("API server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

// ── Auth middleware helper ──────────────────────────────────────────

/// Verify the bearer token if one is configured.
fn check_auth(
    state: &ApiState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    let Some(ref expected) = state.token else {
        return Ok(());
    };

    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = auth_header.strip_prefix("Bearer ").unwrap_or("");

    if token == expected {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid or missing bearer token".into(),
            }),
        ))
    }
}

// ── Handlers ────────────────────────────────────────────────────────

/// POST /api/v1/tasks — Create a new task (queued for daemon execution).
async fn create_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(body): Json<TaskRequest>,
) -> Result<(StatusCode, Json<TaskCreatedResponse>), (StatusCode, Json<ErrorResponse>)> {
    check_auth(&state, &headers)?;

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
        queue.push(body.clone());
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
async fn list_tasks(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<TaskHistoryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    check_auth(&state, &headers)?;

    let history = state::read_history(50);
    Ok(Json(history))
}

/// GET /api/v1/tasks/:id — Get a specific task by ID.
///
/// Checks both the active task and history. Returns 404 if not found.
async fn get_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    check_auth(&state, &headers)?;

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
///
/// This is best-effort: the cancel request is recorded and checked by
/// the orchestrator at the next iteration boundary.
async fn cancel_task(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    check_auth(&state, &headers)?;

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
async fn get_status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<SystemStatus>, (StatusCode, Json<ErrorResponse>)> {
    check_auth(&state, &headers)?;

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
        daemon_running: true, // if this endpoint responds, the daemon is running
        active_task,
        tasks_completed_today: tasks_today.len(),
        total_cost_today,
    }))
}

/// GET /api/v1/cost — Cost summary from the store.
async fn get_cost(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<CostSummary>, (StatusCode, Json<ErrorResponse>)> {
    check_auth(&state, &headers)?;

    // Query from store if available
    let (total_events, task_events) = if let Some(ref store_arc) = state.store {
        if let Ok(store) = store_arc.lock() {
            let since = (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
            let events = store.query_events_since(&since).unwrap_or_default();
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
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state() -> ApiState {
        ApiState {
            store: None,
            token: None,
            task_queue: Arc::new(Mutex::new(Vec::new())),
            cancel_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn test_state_with_auth(token: &str) -> ApiState {
        ApiState {
            store: None,
            token: Some(token.to_string()),
            task_queue: Arc::new(Mutex::new(Vec::new())),
            cancel_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = build_router(test_state());

        let req = Request::builder()
            .uri("/api/v1/health")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn test_status_endpoint() {
        let app = build_router(test_state());

        let req = Request::builder()
            .uri("/api/v1/status")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["daemon_running"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn test_create_task_empty_description() {
        let app = build_router(test_state());

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tasks")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"description": ""}"#))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_task_success() {
        let state = test_state();
        let app = build_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tasks")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"description": "Fix the login bug"}"#))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "queued");

        // Verify task was queued
        let queue = state.task_queue.lock().unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].description, "Fix the login bug");
    }

    #[tokio::test]
    async fn test_list_tasks_empty() {
        let app = build_router(test_state());

        let req = Request::builder()
            .uri("/api/v1/tasks")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_task_not_found() {
        let app = build_router(test_state());

        let req = Request::builder()
            .uri("/api/v1/tasks/nonexistent-id")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_cancel_task_not_active() {
        let app = build_router(test_state());

        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/tasks/nonexistent-id/cancel")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_auth_required_when_configured() {
        let app = build_router(test_state_with_auth("secret-token"));

        // Request without auth header
        let req = Request::builder()
            .uri("/api/v1/status")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_valid_token() {
        let app = build_router(test_state_with_auth("secret-token"));

        let req = Request::builder()
            .uri("/api/v1/status")
            .header("authorization", "Bearer secret-token")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_invalid_token() {
        let app = build_router(test_state_with_auth("secret-token"));

        let req = Request::builder()
            .uri("/api/v1/status")
            .header("authorization", "Bearer wrong-token")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_not_required_when_unconfigured() {
        let app = build_router(test_state());

        let req = Request::builder()
            .uri("/api/v1/status")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_cost_endpoint() {
        let app = build_router(test_state());

        let req = Request::builder()
            .uri("/api/v1/cost")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["total_events_24h"], 0);
    }

    #[test]
    fn test_task_request_deserialization() {
        let json = r#"{"description": "Fix bug", "category": "bugfix", "max_iterations": 5}"#;
        let req: TaskRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.description, "Fix bug");
        assert_eq!(req.category.as_deref(), Some("bugfix"));
        assert_eq!(req.max_iterations, Some(5));
        assert!(req.quality_threshold.is_none());
    }

    #[test]
    fn test_task_request_minimal() {
        let json = r#"{"description": "Do something"}"#;
        let req: TaskRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.description, "Do something");
        assert!(req.category.is_none());
        assert!(req.max_iterations.is_none());
    }
}
