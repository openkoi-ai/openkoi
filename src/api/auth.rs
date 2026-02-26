// src/api/auth.rs

use crate::api::{types::ErrorResponse, ApiState};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;

/// Verify the bearer token if one is configured.
pub fn check_auth(
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

    if constant_time_eq(token.as_bytes(), expected.as_bytes()) {
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

/// Constant-time byte comparison to prevent timing attacks on token auth.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
