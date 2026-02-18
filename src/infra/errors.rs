// src/infra/errors.rs — Error types for OpenKoi

use thiserror::Error;

#[derive(Error, Debug)]
pub enum OpenKoiError {
    // Provider errors (retriable)
    #[error("Provider '{provider}' error: {message}")]
    Provider {
        provider: String,
        message: String,
        retriable: bool,
    },

    #[error("Rate limited by '{provider}', retry after {retry_after_ms}ms")]
    RateLimited {
        provider: String,
        retry_after_ms: u64,
    },

    #[error("All providers exhausted")]
    AllProvidersExhausted,

    // Safety errors (not retriable)
    #[error("Token budget exceeded: {spent}/{budget}")]
    BudgetExceeded { spent: u32, budget: u32 },

    #[error("Cost limit exceeded: ${spent:.2}/${limit:.2}")]
    CostLimitExceeded { spent: f64, limit: f64 },

    #[error("Tool loop detected: {tool} called {count} times")]
    ToolLoop { tool: String, count: u32 },

    #[error("Score regression: {current:.2} < {previous:.2} (threshold: {threshold:.2})")]
    ScoreRegression {
        current: f32,
        previous: f32,
        threshold: f32,
    },

    // User errors
    #[error("No provider configured. Run `openkoi init` or set ANTHROPIC_API_KEY.")]
    NoProvider,

    #[error("Skill '{name}' not found")]
    SkillNotFound { name: String },

    // Infra
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("MCP server '{server}' failed: {message}")]
    McpServer { server: String, message: String },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl OpenKoiError {
    pub fn is_retriable(&self) -> bool {
        matches!(
            self,
            OpenKoiError::Provider {
                retriable: true,
                ..
            } | OpenKoiError::RateLimited { .. }
        )
    }
}
