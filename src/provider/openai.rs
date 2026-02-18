// src/provider/openai.rs — OpenAI Chat API provider

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use reqwest_eventsource::{Event, RequestBuilderExt};
use std::pin::Pin;

use super::{
    ChatChunk, ChatRequest, ChatResponse, ModelInfo, ModelProvider, Role, StopReason, TokenUsage,
    ToolCallDelta,
};
use crate::infra::errors::OpenKoiError;

pub struct OpenAIProvider {
    api_key: String,
    client: reqwest::Client,
    base_url: String,
}

impl OpenAIProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            base_url: "https://api.openai.com/v1".into(),
        }
    }

    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            base_url,
        }
    }
}

#[async_trait]
impl ModelProvider for OpenAIProvider {
    fn id(&self) -> &str {
        "openai"
    }

    fn name(&self) -> &str {
        "OpenAI"
    }

    fn models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "gpt-4.1".into(),
                name: "GPT-4.1".into(),
                context_window: 1_047_576,
                max_output_tokens: 32_768,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 2.0,
                output_price_per_mtok: 8.0,
            },
            ModelInfo {
                id: "gpt-4.1-mini".into(),
                name: "GPT-4.1 Mini".into(),
                context_window: 1_047_576,
                max_output_tokens: 32_768,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.4,
                output_price_per_mtok: 1.6,
            },
            ModelInfo {
                id: "o3-mini".into(),
                name: "o3-mini".into(),
                context_window: 200_000,
                max_output_tokens: 100_000,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 1.1,
                output_price_per_mtok: 4.4,
            },
        ]
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, OpenKoiError> {
        let messages: Vec<serde_json::Value> = {
            let mut msgs = Vec::new();

            if let Some(system) = &request.system {
                msgs.push(serde_json::json!({
                    "role": "system",
                    "content": system,
                }));
            }

            for m in &request.messages {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                let mut msg = serde_json::json!({
                    "role": role,
                    "content": m.content,
                });
                if let Some(tc_id) = &m.tool_call_id {
                    msg["tool_call_id"] = serde_json::json!(tc_id);
                }
                msgs.push(msg);
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
        if !request.tools.is_empty() {
            let tools: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
        }

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "openai".into(),
                message: e.to_string(),
                retriable: e.is_timeout() || e.is_connect(),
            })?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(OpenKoiError::RateLimited {
                provider: "openai".into(),
                retry_after_ms: 5000,
            });
        }

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "openai".into(),
                message: format!("HTTP {}: {}", status, error_body),
                retriable: status.is_server_error(),
            });
        }

        let resp: serde_json::Value = response.json().await.map_err(|e| OpenKoiError::Provider {
            provider: "openai".into(),
            message: format!("Failed to parse response: {}", e),
            retriable: false,
        })?;

        let choice = &resp["choices"][0];
        let content = choice["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let tool_calls = choice["message"]["tool_calls"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|tc| super::ToolCall {
                id: tc["id"].as_str().unwrap_or("").to_string(),
                name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                arguments: serde_json::from_str(
                    tc["function"]["arguments"].as_str().unwrap_or("{}"),
                )
                .unwrap_or_default(),
            })
            .collect();

        let usage = TokenUsage {
            input_tokens: resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: resp["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };

        let stop_reason = match choice["finish_reason"].as_str() {
            Some("stop") => StopReason::EndTurn,
            Some("length") => StopReason::MaxTokens,
            Some("tool_calls") => StopReason::ToolUse,
            _ => StopReason::Unknown,
        };

        Ok(ChatResponse {
            content,
            tool_calls,
            usage,
            stop_reason,
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
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                let mut msg = serde_json::json!({"role": role, "content": m.content});
                if let Some(tc_id) = &m.tool_call_id {
                    msg["tool_call_id"] = serde_json::json!(tc_id);
                }
                msgs.push(msg);
            }
            msgs
        };

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if !request.tools.is_empty() {
            let tools: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
        }

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
                                    provider: "openai".into(),
                                    message: format!("Failed to parse SSE data: {}", e),
                                    retriable: false,
                                });
                                break;
                            }
                        };

                        // Extract text delta
                        let delta_content = parsed["choices"][0]["delta"]["content"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();

                        // Extract tool call deltas
                        let tool_call_delta = parsed["choices"][0]["delta"]["tool_calls"]
                            .as_array()
                            .and_then(|tcs| tcs.first())
                            .map(|tc| ToolCallDelta {
                                id: tc["id"].as_str().map(|s| s.to_string()),
                                name: tc["function"]["name"].as_str().map(|s| s.to_string()),
                                arguments_delta: tc["function"]["arguments"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string(),
                            });

                        // Extract usage (sent in the final chunk when stream_options.include_usage is true)
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

                        if !delta_content.is_empty() || tool_call_delta.is_some() || usage.is_some() {
                            yield Ok(ChatChunk {
                                delta: delta_content,
                                tool_call_delta,
                                usage,
                            });
                        }
                    }
                    Err(reqwest_eventsource::Error::StreamEnded) => break,
                    Err(e) => {
                        yield Err(OpenKoiError::Provider {
                            provider: "openai".into(),
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

    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, OpenKoiError> {
        let body = serde_json::json!({
            "model": "text-embedding-3-small",
            "input": texts,
        });

        let response = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "openai".into(),
                message: e.to_string(),
                retriable: e.is_timeout(),
            })?;

        let resp: serde_json::Value = response.json().await.map_err(|e| OpenKoiError::Provider {
            provider: "openai".into(),
            message: format!("Failed to parse embedding response: {}", e),
            retriable: false,
        })?;

        let embeddings = resp["data"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|d| {
                d["embedding"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                    .collect()
            })
            .collect();

        Ok(embeddings)
    }
}
