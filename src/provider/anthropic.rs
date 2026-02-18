// src/provider/anthropic.rs — Anthropic Messages API provider

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

pub struct AnthropicProvider {
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    fn api_url(&self) -> &str {
        "https://api.anthropic.com/v1/messages"
    }

    fn build_request_body(&self, request: &ChatRequest) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| {
                serde_json::json!({
                    "role": match m.role {
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "user",
                        Role::System => unreachable!(),
                    },
                    "content": m.content,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(4096),
        });

        if let Some(system) = &request.system {
            body["system"] = serde_json::json!([{
                "type": "text",
                "text": system,
                "cache_control": { "type": "ephemeral" }
            }]);
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
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.parameters,
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
        }

        body
    }
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn name(&self) -> &str {
        "Anthropic"
    }

    fn models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "claude-sonnet-4-20250514".into(),
                name: "Claude Sonnet 4".into(),
                context_window: 200_000,
                max_output_tokens: 16_384,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 3.0,
                output_price_per_mtok: 15.0,
            },
            ModelInfo {
                id: "claude-opus-4-20250514".into(),
                name: "Claude Opus 4".into(),
                context_window: 200_000,
                max_output_tokens: 32_768,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 15.0,
                output_price_per_mtok: 75.0,
            },
            ModelInfo {
                id: "claude-haiku-3-5-20241022".into(),
                name: "Claude 3.5 Haiku".into(),
                context_window: 200_000,
                max_output_tokens: 8_192,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.8,
                output_price_per_mtok: 4.0,
            },
        ]
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, OpenKoiError> {
        let body = self.build_request_body(&request);

        let response = self
            .client
            .post(self.api_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "anthropic".into(),
                message: e.to_string(),
                retriable: e.is_timeout() || e.is_connect(),
            })?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(5000);
            return Err(OpenKoiError::RateLimited {
                provider: "anthropic".into(),
                retry_after_ms: retry_after * 1000,
            });
        }

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "anthropic".into(),
                message: format!("HTTP {}: {}", status, error_body),
                retriable: status.is_server_error(),
            });
        }

        let resp: serde_json::Value = response.json().await.map_err(|e| OpenKoiError::Provider {
            provider: "anthropic".into(),
            message: format!("Failed to parse response: {}", e),
            retriable: false,
        })?;

        let content = resp["content"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter(|c| c["type"] == "text")
            .map(|c| c["text"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("");

        let tool_calls = resp["content"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter(|c| c["type"] == "tool_use")
            .map(|c| super::ToolCall {
                id: c["id"].as_str().unwrap_or("").to_string(),
                name: c["name"].as_str().unwrap_or("").to_string(),
                arguments: c["input"].clone(),
            })
            .collect();

        let usage = TokenUsage {
            input_tokens: resp["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: resp["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            cache_read_tokens: resp["usage"]["cache_read_input_tokens"]
                .as_u64()
                .unwrap_or(0) as u32,
            cache_write_tokens: resp["usage"]["cache_creation_input_tokens"]
                .as_u64()
                .unwrap_or(0) as u32,
        };

        let stop_reason = match resp["stop_reason"].as_str() {
            Some("end_turn") => StopReason::EndTurn,
            Some("max_tokens") => StopReason::MaxTokens,
            Some("tool_use") => StopReason::ToolUse,
            Some("stop_sequence") => StopReason::StopSequence,
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
        let mut body = self.build_request_body(&request);
        body["stream"] = serde_json::json!(true);

        let request_builder = self
            .client
            .post(self.api_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
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
                                    provider: "anthropic".into(),
                                    message: format!("Failed to parse SSE data: {}", e),
                                    retriable: false,
                                });
                                break;
                            }
                        };

                        let event_type = parsed["type"].as_str().unwrap_or("");

                        match event_type {
                            "content_block_delta" => {
                                let delta_type = parsed["delta"]["type"].as_str().unwrap_or("");
                                match delta_type {
                                    "text_delta" => {
                                        let text = parsed["delta"]["text"]
                                            .as_str()
                                            .unwrap_or("")
                                            .to_string();
                                        if !text.is_empty() {
                                            yield Ok(ChatChunk {
                                                delta: text,
                                                tool_call_delta: None,
                                                usage: None,
                                            });
                                        }
                                    }
                                    "input_json_delta" => {
                                        let partial_json = parsed["delta"]["partial_json"]
                                            .as_str()
                                            .unwrap_or("")
                                            .to_string();
                                        yield Ok(ChatChunk {
                                            delta: String::new(),
                                            tool_call_delta: Some(ToolCallDelta {
                                                id: None,
                                                name: None,
                                                arguments_delta: partial_json,
                                            }),
                                            usage: None,
                                        });
                                    }
                                    _ => {}
                                }
                            }
                            "content_block_start" => {
                                // Tool use blocks start here with id and name
                                let block_type = parsed["content_block"]["type"].as_str().unwrap_or("");
                                if block_type == "tool_use" {
                                    let id = parsed["content_block"]["id"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
                                    let name = parsed["content_block"]["name"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_string();
                                    yield Ok(ChatChunk {
                                        delta: String::new(),
                                        tool_call_delta: Some(ToolCallDelta {
                                            id: Some(id),
                                            name: Some(name),
                                            arguments_delta: String::new(),
                                        }),
                                        usage: None,
                                    });
                                }
                            }
                            "message_delta" => {
                                // Final usage info
                                let output_tokens = parsed["usage"]["output_tokens"]
                                    .as_u64()
                                    .unwrap_or(0) as u32;
                                if output_tokens > 0 {
                                    yield Ok(ChatChunk {
                                        delta: String::new(),
                                        tool_call_delta: None,
                                        usage: Some(TokenUsage {
                                            input_tokens: 0,
                                            output_tokens,
                                            cache_read_tokens: 0,
                                            cache_write_tokens: 0,
                                        }),
                                    });
                                }
                            }
                            "message_start" => {
                                // Input token usage
                                let input_tokens = parsed["message"]["usage"]["input_tokens"]
                                    .as_u64()
                                    .unwrap_or(0) as u32;
                                let cache_read = parsed["message"]["usage"]["cache_read_input_tokens"]
                                    .as_u64()
                                    .unwrap_or(0) as u32;
                                let cache_write = parsed["message"]["usage"]["cache_creation_input_tokens"]
                                    .as_u64()
                                    .unwrap_or(0) as u32;
                                if input_tokens > 0 || cache_read > 0 || cache_write > 0 {
                                    yield Ok(ChatChunk {
                                        delta: String::new(),
                                        tool_call_delta: None,
                                        usage: Some(TokenUsage {
                                            input_tokens,
                                            output_tokens: 0,
                                            cache_read_tokens: cache_read,
                                            cache_write_tokens: cache_write,
                                        }),
                                    });
                                }
                            }
                            "message_stop" => {
                                break;
                            }
                            _ => {} // ping, content_block_stop, etc.
                        }
                    }
                    Err(reqwest_eventsource::Error::StreamEnded) => break,
                    Err(e) => {
                        yield Err(OpenKoiError::Provider {
                            provider: "anthropic".into(),
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
        // Anthropic doesn't have an embedding API
        Err(OpenKoiError::Provider {
            provider: "anthropic".into(),
            message: "Anthropic does not support embeddings".into(),
            retriable: false,
        })
    }
}
