// src/provider/fallback.rs — Fallback chain for provider resilience

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{ModelProvider, ModelRef};
use crate::infra::errors::OpenKoiError;

pub struct FallbackChain {
    candidates: Vec<ModelRef>,
    providers: Vec<Arc<dyn ModelProvider>>,
    cooldowns: HashMap<String, Instant>,
    cooldown_duration: Duration,
}

impl FallbackChain {
    pub fn new(candidates: Vec<ModelRef>, providers: Vec<Arc<dyn ModelProvider>>) -> Self {
        Self {
            candidates,
            providers,
            cooldowns: HashMap::new(),
            cooldown_duration: Duration::from_secs(60),
        }
    }

    fn is_cooled_down(&self, candidate: &ModelRef) -> bool {
        if let Some(cooldown_start) = self.cooldowns.get(&candidate.to_string()) {
            cooldown_start.elapsed() < self.cooldown_duration
        } else {
            false
        }
    }

    /// Get the next available provider, skipping cooled-down ones.
    pub fn next_available(&self) -> Option<(&ModelRef, &Arc<dyn ModelProvider>)> {
        for candidate in &self.candidates {
            if self.is_cooled_down(candidate) {
                continue;
            }
            if let Some(provider) = self.providers.iter().find(|p| p.id() == candidate.provider) {
                return Some((candidate, provider));
            }
        }
        None
    }

    /// Mark a candidate as temporarily unavailable.
    pub fn mark_failed(&mut self, candidate: &ModelRef) {
        self.cooldowns.insert(candidate.to_string(), Instant::now());
    }

    /// Run a chat request through the fallback chain.
    pub async fn chat(
        &mut self,
        mut request: super::ChatRequest,
    ) -> Result<super::ChatResponse, OpenKoiError> {
        let candidates = self.candidates.clone();
        for candidate in &candidates {
            if self.is_cooled_down(candidate) {
                continue;
            }

            if let Some(provider) = self.providers.iter().find(|p| p.id() == candidate.provider) {
                request.model = candidate.model.clone();
                match provider.chat(request.clone()).await {
                    Ok(response) => return Ok(response),
                    Err(e) if e.is_retriable() => {
                        tracing::warn!(
                            provider = %candidate.provider,
                            model = %candidate.model,
                            "Provider failed, trying fallback: {}",
                            e
                        );
                        self.mark_failed(candidate);
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        Err(OpenKoiError::AllProvidersExhausted)
    }
}
