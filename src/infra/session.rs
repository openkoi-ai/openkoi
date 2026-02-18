// src/infra/session.rs — Session management

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub channel: String,
    pub model_provider: Option<String>,
    pub model_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub total_tokens: u32,
    pub total_cost_usd: f64,
    pub transcript_path: Option<String>,
}

impl Session {
    pub fn new(channel: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            channel: channel.to_string(),
            model_provider: None,
            model_id: None,
            created_at: now,
            updated_at: now,
            total_tokens: 0,
            total_cost_usd: 0.0,
            transcript_path: None,
        }
    }
}
