// src/api/mod.rs — Lightweight HTTP API server for external integrations

pub mod auth;
pub mod handlers;
pub mod types;
pub mod webhooks;

use axum::routing::{get, post};
use axum::Router;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;

use crate::infra::config::ApiConfig;
use crate::memory::StoreHandle;
pub use types::TaskRequest;

/// Shared state for API handlers.
#[derive(Clone)]
pub struct ApiState {
    pub store: Option<StoreHandle>,
    pub token: Option<String>,
    /// Queue for tasks submitted via the API — consumed by the daemon loop.
    pub task_queue: Arc<Mutex<Vec<TaskRequest>>>,
    /// Set of task IDs that have been requested to cancel.
    pub cancel_requests: Arc<Mutex<Vec<String>>>,
}

/// Build the axum router with all API routes.
pub fn build_router(state: ApiState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin([
            "http://localhost:3000".parse().unwrap(),
            "http://localhost:5173".parse().unwrap(),
            "http://127.0.0.1:3000".parse().unwrap(),
            "http://127.0.0.1:5173".parse().unwrap(),
        ])
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    Router::new()
        .route("/api/v1/tasks", post(handlers::create_task))
        .route("/api/v1/tasks", get(handlers::list_tasks))
        .route("/api/v1/tasks/{id}", get(handlers::get_task))
        .route("/api/v1/tasks/{id}/cancel", post(handlers::cancel_task))
        .route("/api/v1/status", get(handlers::get_status))
        .route("/api/v1/cost", get(handlers::get_cost))
        .route("/api/v1/health", get(handlers::health))
        .layer(cors)
        .with_state(state)
}

/// Start the API server on the given port (blocking).
pub async fn start_server(config: &ApiConfig, state: ApiState) -> anyhow::Result<()> {
    let port = config.port;
    let addr = format!("127.0.0.1:{port}");

    let router = build_router(state);

    tracing::info!("API server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            std::future::pending::<()>().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_state() -> ApiState {
        ApiState {
            store: None,
            token: None,
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
    }
}
