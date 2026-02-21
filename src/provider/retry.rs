// src/provider/retry.rs — Retry with exponential backoff for model providers
//
// Wraps any ModelProvider with automatic retry on transient failures.
// Retries: rate limits (429), server errors (5xx), timeouts, connection resets.
// Does NOT retry: context overflow, bad request (400), auth errors (401, 403).

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::Stream;

use super::{ChatChunk, ChatRequest, ChatResponse, ModelInfo, ModelProvider};
use crate::infra::errors::OpenKoiError;

/// Default retry configuration.
const MAX_RETRIES: u32 = 8;
const INITIAL_DELAY_MS: u64 = 2_000;
const BACKOFF_FACTOR: f64 = 2.0;
const MAX_DELAY_MS: u64 = 30_000;
const JITTER_FRACTION: f64 = 0.2;

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub backoff_factor: f64,
    pub max_delay: Duration,
    pub jitter_fraction: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: MAX_RETRIES,
            initial_delay: Duration::from_millis(INITIAL_DELAY_MS),
            backoff_factor: BACKOFF_FACTOR,
            max_delay: Duration::from_millis(MAX_DELAY_MS),
            jitter_fraction: JITTER_FRACTION,
        }
    }
}

/// A provider wrapper that adds retry with exponential backoff.
///
/// Delegates all trait methods to the inner provider, retrying `chat()` and
/// `chat_stream()` on transient errors.
pub struct RetryProvider {
    inner: Arc<dyn ModelProvider>,
    config: RetryConfig,
}

impl RetryProvider {
    pub fn new(inner: Arc<dyn ModelProvider>) -> Self {
        Self {
            inner,
            config: RetryConfig::default(),
        }
    }

    pub fn with_config(inner: Arc<dyn ModelProvider>, config: RetryConfig) -> Self {
        Self { inner, config }
    }

    /// Calculate the delay for a given retry attempt (0-indexed).
    fn delay_for_attempt(&self, attempt: u32, rate_limit_delay: Option<Duration>) -> Duration {
        // If the server told us how long to wait, use that (with a small buffer).
        if let Some(rl_delay) = rate_limit_delay {
            return rl_delay + Duration::from_millis(100);
        }

        let base_ms = self.config.initial_delay.as_millis() as f64
            * self.config.backoff_factor.powi(attempt as i32);
        let capped_ms = base_ms.min(self.config.max_delay.as_millis() as f64);

        // Add jitter: random between [1 - jitter, 1 + jitter] * capped_ms
        let jitter = deterministic_jitter(attempt, self.config.jitter_fraction);
        let final_ms = (capped_ms * jitter).max(100.0);

        Duration::from_millis(final_ms as u64)
    }
}

/// Determine if an error should be retried.
fn should_retry(error: &OpenKoiError) -> bool {
    match error {
        // Explicitly retriable errors
        OpenKoiError::RateLimited { .. } => true,
        OpenKoiError::Provider { retriable, .. } => *retriable,
        // Context overflow should NOT be retried — it needs pruning, not retry
        OpenKoiError::ContextOverflow { .. } => false,
        // Everything else: don't retry
        _ => false,
    }
}

/// Extract rate-limit retry delay from the error, if available.
fn rate_limit_delay(error: &OpenKoiError) -> Option<Duration> {
    match error {
        OpenKoiError::RateLimited { retry_after_ms, .. } if *retry_after_ms > 0 => {
            Some(Duration::from_millis(*retry_after_ms))
        }
        _ => None,
    }
}

/// Deterministic jitter for a given attempt to keep retries reproducible in tests.
/// Returns a multiplier in [1 - fraction, 1 + fraction].
fn deterministic_jitter(attempt: u32, fraction: f64) -> f64 {
    // Simple hash-based jitter — not cryptographic, just varied enough
    let hash = (attempt.wrapping_mul(2654435761)) as f64 / u32::MAX as f64; // 0.0..1.0
    1.0 + fraction * (2.0 * hash - 1.0) // [1-fraction, 1+fraction]
}

#[async_trait]
impl ModelProvider for RetryProvider {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }

    fn models(&self) -> Vec<ModelInfo> {
        self.inner.models()
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, OpenKoiError> {
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            match self.inner.chat(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    if !should_retry(&e) || attempt == self.config.max_retries {
                        return Err(e);
                    }

                    let rl_delay = rate_limit_delay(&e);
                    let delay = self.delay_for_attempt(attempt, rl_delay);

                    tracing::warn!(
                        provider = self.inner.id(),
                        attempt = attempt + 1,
                        max_retries = self.config.max_retries,
                        delay_ms = delay.as_millis() as u64,
                        "Retrying after error: {}",
                        e
                    );

                    tokio::time::sleep(delay).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(OpenKoiError::Provider {
            provider: self.inner.id().to_string(),
            message: "All retries exhausted".into(),
            retriable: false,
        }))
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatChunk, OpenKoiError>> + Send>>, OpenKoiError>
    {
        // Retry only the initial connection, not mid-stream errors
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            match self.inner.chat_stream(request.clone()).await {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    if !should_retry(&e) || attempt == self.config.max_retries {
                        return Err(e);
                    }

                    let rl_delay = rate_limit_delay(&e);
                    let delay = self.delay_for_attempt(attempt, rl_delay);

                    tracing::warn!(
                        provider = self.inner.id(),
                        attempt = attempt + 1,
                        "Retrying stream after error: {}",
                        e
                    );

                    tokio::time::sleep(delay).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(OpenKoiError::Provider {
            provider: self.inner.id().to_string(),
            message: "All retries exhausted".into(),
            retriable: false,
        }))
    }

    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, OpenKoiError> {
        // Embed is typically idempotent — retry on transient failures
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            match self.inner.embed(texts).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    if !should_retry(&e) || attempt == self.config.max_retries {
                        return Err(e);
                    }

                    let delay = self.delay_for_attempt(attempt, None);
                    tokio::time::sleep(delay).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(OpenKoiError::Provider {
            provider: self.inner.id().to_string(),
            message: "All retries exhausted".into(),
            retriable: false,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_retry_rate_limited() {
        let err = OpenKoiError::RateLimited {
            provider: "test".into(),
            retry_after_ms: 5000,
        };
        assert!(should_retry(&err));
    }

    #[test]
    fn test_should_retry_retriable_provider() {
        let err = OpenKoiError::Provider {
            provider: "test".into(),
            message: "HTTP 500".into(),
            retriable: true,
        };
        assert!(should_retry(&err));
    }

    #[test]
    fn test_should_not_retry_non_retriable_provider() {
        let err = OpenKoiError::Provider {
            provider: "test".into(),
            message: "HTTP 400 bad request".into(),
            retriable: false,
        };
        assert!(!should_retry(&err));
    }

    #[test]
    fn test_should_not_retry_context_overflow() {
        let err = OpenKoiError::ContextOverflow {
            provider: "test".into(),
            model: "gpt-4o".into(),
            message: "context too long".into(),
        };
        assert!(!should_retry(&err));
    }

    #[test]
    fn test_should_not_retry_no_provider() {
        assert!(!should_retry(&OpenKoiError::NoProvider));
    }

    #[test]
    fn test_rate_limit_delay_extraction() {
        let err = OpenKoiError::RateLimited {
            provider: "test".into(),
            retry_after_ms: 3000,
        };
        let delay = rate_limit_delay(&err);
        assert_eq!(delay, Some(Duration::from_millis(3000)));
    }

    #[test]
    fn test_rate_limit_delay_zero() {
        let err = OpenKoiError::RateLimited {
            provider: "test".into(),
            retry_after_ms: 0,
        };
        assert!(rate_limit_delay(&err).is_none());
    }

    #[test]
    fn test_rate_limit_delay_non_rate_limit_error() {
        let err = OpenKoiError::Provider {
            provider: "test".into(),
            message: "server error".into(),
            retriable: true,
        };
        assert!(rate_limit_delay(&err).is_none());
    }

    #[test]
    fn test_delay_for_attempt_exponential() {
        let provider = RetryProvider::new(Arc::new(DummyProvider));
        let d0 = provider.delay_for_attempt(0, None);
        let d1 = provider.delay_for_attempt(1, None);
        let d2 = provider.delay_for_attempt(2, None);

        // Each delay should be roughly 2x the previous (within jitter bounds)
        // d0 ≈ 2000ms, d1 ≈ 4000ms, d2 ≈ 8000ms
        assert!(d0.as_millis() >= 1500 && d0.as_millis() <= 2500);
        assert!(d1.as_millis() >= 3000 && d1.as_millis() <= 5000);
        assert!(d2.as_millis() >= 6000 && d2.as_millis() <= 10000);
    }

    #[test]
    fn test_delay_capped_at_max() {
        let provider = RetryProvider::new(Arc::new(DummyProvider));
        // Attempt 10: 2000 * 2^10 = 2,048,000ms but max is 30,000ms
        let d = provider.delay_for_attempt(10, None);
        assert!(d.as_millis() <= 36_000); // max + jitter margin
    }

    #[test]
    fn test_delay_uses_rate_limit_hint() {
        let provider = RetryProvider::new(Arc::new(DummyProvider));
        let d = provider.delay_for_attempt(0, Some(Duration::from_millis(10_000)));
        // Should be the rate limit delay + 100ms buffer, NOT the exponential delay
        assert_eq!(d.as_millis(), 10_100);
    }

    #[test]
    fn test_deterministic_jitter_range() {
        for attempt in 0..20 {
            let j = deterministic_jitter(attempt, 0.2);
            assert!(
                j >= 0.8 && j <= 1.2,
                "jitter {} out of range for attempt {}",
                j,
                attempt
            );
        }
    }

    #[test]
    fn test_deterministic_jitter_reproducible() {
        assert_eq!(deterministic_jitter(5, 0.2), deterministic_jitter(5, 0.2));
    }

    #[test]
    fn test_default_config() {
        let cfg = RetryConfig::default();
        assert_eq!(cfg.max_retries, 8);
        assert_eq!(cfg.initial_delay, Duration::from_millis(2000));
        assert_eq!(cfg.backoff_factor, 2.0);
        assert_eq!(cfg.max_delay, Duration::from_millis(30000));
        assert_eq!(cfg.jitter_fraction, 0.2);
    }

    // Dummy provider for test construction
    struct DummyProvider;

    #[async_trait]
    impl ModelProvider for DummyProvider {
        fn id(&self) -> &str {
            "dummy"
        }
        fn name(&self) -> &str {
            "Dummy"
        }
        fn models(&self) -> Vec<ModelInfo> {
            vec![]
        }
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, OpenKoiError> {
            Err(OpenKoiError::NoProvider)
        }
        async fn chat_stream(
            &self,
            _req: ChatRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatChunk, OpenKoiError>> + Send>>, OpenKoiError>
        {
            Err(OpenKoiError::NoProvider)
        }
        async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, OpenKoiError> {
            Err(OpenKoiError::NoProvider)
        }
    }
}
