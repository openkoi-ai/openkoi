// src/provider/google.rs — Google Generative AI (Gemini) provider

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use reqwest_eventsource::{Event, RequestBuilderExt};
use std::pin::Pin;

use super::{
    ChatChunk, ChatRequest, ChatResponse, ModelInfo, ModelProvider, Role, StopReason, TokenUsage,
    ToolCall, ToolCallDelta,
};
use crate::infra::errors::OpenKoiError;

pub struct GoogleProvider {
    api_key: String,
    client: reqwest::Client,
}

impl GoogleProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    fn base_url(&self) -> &str {
        "https://generativelanguage.googleapis.com/v1beta"
    }

    /// Build the Gemini request body from a ChatRequest.
    fn build_request_body(&self, request: &ChatRequest) -> serde_json::Value {
        let mut contents: Vec<serde_json::Value> = Vec::new();

        for m in &request.messages {
            let role = match m.role {
                Role::User | Role::Tool => "user",
                Role::Assistant => "model",
                Role::System => continue, // system handled via system_instruction
            };

            contents.push(serde_json::json!({
                "role": role,
                "parts": [{ "text": m.content }],
            }));
        }

        let mut body = serde_json::json!({
            "contents": contents,
        });

        // System instruction
        if let Some(ref system) = request.system {
            body["system_instruction"] = serde_json::json!({
                "parts": [{ "text": system }],
            });
        }

        // Generation config
        let mut gen_config = serde_json::json!({});
        if let Some(max_tokens) = request.max_tokens {
            gen_config["maxOutputTokens"] = serde_json::json!(max_tokens);
        }
        if let Some(temp) = request.temperature {
            gen_config["temperature"] = serde_json::json!(temp);
        }
        if gen_config != serde_json::json!({}) {
            body["generationConfig"] = gen_config;
        }

        // Tools (function calling)
        if !request.tools.is_empty() {
            let function_declarations: Vec<serde_json::Value> = request
                .tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    })
                })
                .collect();
            body["tools"] = serde_json::json!([{
                "function_declarations": function_declarations,
            }]);
        }

        body
    }
}

#[async_trait]
impl ModelProvider for GoogleProvider {
    fn id(&self) -> &str {
        "google"
    }

    fn name(&self) -> &str {
        "Google"
    }

    fn models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "gemini-2.5-pro".into(),
                name: "Gemini 2.5 Pro".into(),
                context_window: 1_048_576,
                max_output_tokens: 65_536,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 1.25,
                output_price_per_mtok: 10.0,
                can_reason: true,
                supports_vision: true,
                supports_attachments: true,
                family: Some("gemini-2.5".into()),
                release_date: Some("2025-03-25".into()),
                ..Default::default()
            },
            ModelInfo {
                id: "gemini-2.5-flash".into(),
                name: "Gemini 2.5 Flash".into(),
                context_window: 1_048_576,
                max_output_tokens: 65_536,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.15,
                output_price_per_mtok: 0.60,
                can_reason: true,
                supports_vision: true,
                supports_attachments: true,
                family: Some("gemini-2.5".into()),
                release_date: Some("2025-04-17".into()),
                ..Default::default()
            },
            ModelInfo {
                id: "gemini-2.0-flash".into(),
                name: "Gemini 2.0 Flash".into(),
                context_window: 1_048_576,
                max_output_tokens: 8_192,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.10,
                output_price_per_mtok: 0.40,
                supports_vision: true,
                supports_attachments: true,
                family: Some("gemini-2.0".into()),
                release_date: Some("2025-02-05".into()),
                ..Default::default()
            },
        ]
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, OpenKoiError> {
        let body = self.build_request_body(&request);

        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url(),
            request.model,
            self.api_key,
        );

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "google".into(),
                message: e.to_string(),
                retriable: e.is_timeout() || e.is_connect(),
            })?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(OpenKoiError::RateLimited {
                provider: "google".into(),
                retry_after_ms: 5000,
            });
        }

        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "google".into(),
                message: format!("HTTP {}: {}", status, error_body),
                retriable: status.is_server_error(),
            });
        }

        let resp: serde_json::Value =
            response.json().await.map_err(|e| OpenKoiError::Provider {
                provider: "google".into(),
                message: format!("Failed to parse response: {}", e),
                retriable: false,
            })?;

        // Extract text content from candidates[0].content.parts
        let parts = resp["candidates"][0]["content"]["parts"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let mut content = String::new();
        let mut tool_calls = Vec::new();

        for part in &parts {
            if let Some(text) = part["text"].as_str() {
                content.push_str(text);
            }
            if let Some(fc) = part.get("functionCall") {
                tool_calls.push(ToolCall {
                    id: fc["name"].as_str().unwrap_or("").to_string(),
                    name: fc["name"].as_str().unwrap_or("").to_string(),
                    arguments: fc["args"].clone(),
                });
            }
        }

        let usage = TokenUsage {
            input_tokens: resp["usageMetadata"]["promptTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
            output_tokens: resp["usageMetadata"]["candidatesTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
            cache_read_tokens: resp["usageMetadata"]["cachedContentTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
            cache_write_tokens: 0,
        };

        let stop_reason = match resp["candidates"][0]["finishReason"].as_str() {
            Some("STOP") => StopReason::EndTurn,
            Some("MAX_TOKENS") => StopReason::MaxTokens,
            Some("SAFETY") => StopReason::StopSequence,
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
        let body = self.build_request_body(&request);

        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            self.base_url(),
            request.model,
            self.api_key,
        );

        let request_builder = self
            .client
            .post(&url)
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
                                    provider: "google".into(),
                                    message: format!("Failed to parse SSE data: {}", e),
                                    retriable: false,
                                });
                                break;
                            }
                        };

                        // Extract text parts from the streamed candidate
                        let parts = parsed["candidates"][0]["content"]["parts"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default();

                        let mut delta_text = String::new();
                        let mut tool_delta: Option<ToolCallDelta> = None;

                        for part in &parts {
                            if let Some(text) = part["text"].as_str() {
                                delta_text.push_str(text);
                            }
                            if let Some(fc) = part.get("functionCall") {
                                tool_delta = Some(ToolCallDelta {
                                    id: fc["name"].as_str().map(|s| s.to_string()),
                                    name: fc["name"].as_str().map(|s| s.to_string()),
                                    arguments_delta: fc["args"]
                                        .as_object()
                                        .map(|o| serde_json::to_string(o).unwrap_or_default())
                                        .unwrap_or_default(),
                                });
                            }
                        }

                        // Extract usage from usageMetadata if present
                        let usage = if parsed["usageMetadata"].is_object()
                            && !parsed["usageMetadata"].is_null()
                        {
                            let input = parsed["usageMetadata"]["promptTokenCount"]
                                .as_u64()
                                .unwrap_or(0) as u32;
                            let output = parsed["usageMetadata"]["candidatesTokenCount"]
                                .as_u64()
                                .unwrap_or(0) as u32;
                            if input > 0 || output > 0 {
                                Some(TokenUsage {
                                    input_tokens: input,
                                    output_tokens: output,
                                    cache_read_tokens: 0,
                                    cache_write_tokens: 0,
                                })
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        if !delta_text.is_empty() || tool_delta.is_some() || usage.is_some() {
                            yield Ok(ChatChunk {
                                delta: delta_text,
                                tool_call_delta: tool_delta,
                                usage,
                            });
                        }
                    }
                    Err(reqwest_eventsource::Error::StreamEnded) => break,
                    Err(e) => {
                        yield Err(OpenKoiError::Provider {
                            provider: "google".into(),
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
        // Gemini embedding endpoint: models/text-embedding-004:batchEmbedContents
        let requests: Vec<serde_json::Value> = texts
            .iter()
            .map(|text| {
                serde_json::json!({
                    "model": "models/text-embedding-004",
                    "content": { "parts": [{ "text": text }] },
                })
            })
            .collect();

        let body = serde_json::json!({
            "requests": requests,
        });

        let url = format!(
            "{}/models/text-embedding-004:batchEmbedContents?key={}",
            self.base_url(),
            self.api_key,
        );

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "google".into(),
                message: format!("Embedding request failed: {}", e),
                retriable: e.is_timeout(),
            })?;

        if !response.status().is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "google".into(),
                message: format!("Embedding error: {}", error_body),
                retriable: false,
            });
        }

        let resp: serde_json::Value =
            response.json().await.map_err(|e| OpenKoiError::Provider {
                provider: "google".into(),
                message: format!("Failed to parse embedding response: {}", e),
                retriable: false,
            })?;

        let embeddings = resp["embeddings"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|e| {
                e["values"]
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
