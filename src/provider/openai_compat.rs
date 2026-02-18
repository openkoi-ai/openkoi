// src/provider/openai_compat.rs — Generic OpenAI-compatible provider

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use reqwest_eventsource::{Event, RequestBuilderExt};
use std::pin::Pin;

use super::{
    ChatChunk, ChatRequest, ChatResponse, ModelInfo, ModelProvider, Role, StopReason, TokenUsage,
};
use crate::infra::errors::OpenKoiError;

/// Provider for any OpenAI-compatible API endpoint (Together, Groq, DeepSeek, etc.)
pub struct OpenAICompatProvider {
    id_str: String,
    name_str: String,
    api_key: String,
    base_url: String,
    default_model: String,
    client: reqwest::Client,
}

impl OpenAICompatProvider {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        api_key: String,
        base_url: String,
        default_model: String,
    ) -> Self {
        Self {
            id_str: id.into(),
            name_str: name.into(),
            api_key,
            base_url,
            default_model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl ModelProvider for OpenAICompatProvider {
    fn id(&self) -> &str {
        &self.id_str
    }

    fn name(&self) -> &str {
        &self.name_str
    }

    fn models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: self.default_model.clone(),
            name: self.default_model.clone(),
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
        }]
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, OpenKoiError> {
        let messages: Vec<serde_json::Value> = {
            let mut msgs = Vec::new();
            if let Some(system) = &request.system {
                msgs.push(serde_json::json!({"role": "system", "content": system}));
            }
            for m in &request.messages {
                msgs.push(serde_json::json!({
                    "role": match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    },
                    "content": m.content,
                }));
            }
            msgs
        };

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
        });
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: self.id_str.clone(),
                message: e.to_string(),
                retriable: e.is_timeout(),
            })?;

        if !response.status().is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: self.id_str.clone(),
                message: error_body,
                retriable: false,
            });
        }

        let resp: serde_json::Value =
            response.json().await.map_err(|e| OpenKoiError::Provider {
                provider: self.id_str.clone(),
                message: e.to_string(),
                retriable: false,
            })?;

        let content = resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let usage = TokenUsage {
            input_tokens: resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: resp["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };

        Ok(ChatResponse {
            content,
            tool_calls: Vec::new(),
            usage,
            stop_reason: StopReason::EndTurn,
        })
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatChunk, OpenKoiError>> + Send>>, OpenKoiError>
    {
        let messages: Vec<serde_json::Value> = {
            let mut msgs = Vec::new();
            if let Some(system) = &request.system {
                msgs.push(serde_json::json!({"role": "system", "content": system}));
            }
            for m in &request.messages {
                msgs.push(serde_json::json!({
                    "role": match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    },
                    "content": m.content,
                }));
            }
            msgs
        };

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
        });
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        let provider_id = self.id_str.clone();

        let request_builder = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body);

        let mut es = request_builder.eventsource().unwrap();

        let stream = async_stream::stream! {
            while let Some(event) = es.next().await {
                match event {
                    Ok(Event::Open) => {},
                    Ok(Event::Message(msg)) => {
                        if msg.data == "[DONE]" {
                            break;
                        }
                        let parsed: serde_json::Value = match serde_json::from_str(&msg.data) {
                            Ok(v) => v,
                            Err(e) => {
                                yield Err(OpenKoiError::Provider {
                                    provider: provider_id.clone(),
                                    message: format!("Failed to parse SSE data: {}", e),
                                    retriable: false,
                                });
                                break;
                            }
                        };

                        let delta_content = parsed["choices"][0]["delta"]["content"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();

                        // Extract usage if present (some compat providers include it)
                        let usage = if parsed["usage"].is_object() && !parsed["usage"].is_null() {
                            Some(TokenUsage {
                                input_tokens: parsed["usage"]["prompt_tokens"]
                                    .as_u64()
                                    .unwrap_or(0) as u32,
                                output_tokens: parsed["usage"]["completion_tokens"]
                                    .as_u64()
                                    .unwrap_or(0) as u32,
                                cache_read_tokens: 0,
                                cache_write_tokens: 0,
                            })
                        } else {
                            None
                        };

                        if !delta_content.is_empty() || usage.is_some() {
                            yield Ok(ChatChunk {
                                delta: delta_content,
                                tool_call_delta: None,
                                usage,
                            });
                        }
                    }
                    Err(reqwest_eventsource::Error::StreamEnded) => break,
                    Err(e) => {
                        yield Err(OpenKoiError::Provider {
                            provider: provider_id.clone(),
                            message: format!("SSE stream error: {}", e),
                            retriable: false,
                        });
                        break;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn embed(&self, _texts: &[&str]) -> Result<Vec<Vec<f32>>, OpenKoiError> {
        Err(OpenKoiError::Provider {
            provider: self.id_str.clone(),
            message: "Embeddings not supported".into(),
            retriable: false,
        })
    }
}
