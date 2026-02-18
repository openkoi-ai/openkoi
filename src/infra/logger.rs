// src/infra/logger.rs — Structured logging with tracing

use tracing_subscriber::{fmt, EnvFilter};

pub fn init_logging(level: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
