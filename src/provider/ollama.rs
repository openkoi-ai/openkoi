// src/provider/ollama.rs — Ollama local model provider

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use std::pin::Pin;

use super::{
    ChatChunk, ChatRequest, ChatResponse, ModelInfo, ModelProvider, Role, StopReason, TokenUsage,
};
use crate::infra::errors::OpenKoiError;

pub struct OllamaProvider {
    base_url: String,
    client: reqwest::Client,
    available_models: Vec<String>,
}

impl OllamaProvider {
    pub fn new(base_url: Option<String>) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(|| "http://localhost:11434".into()),
            client: reqwest::Client::new(),
            available_models: Vec::new(),
        }
    }

    pub async fn probe(&mut self) -> Result<Vec<String>, OpenKoiError> {
        let resp = self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "ollama".into(),
                message: format!("Cannot reach Ollama: {}", e),
                retriable: false,
            })?;

        let body: serde_json::Value = resp.json().await.map_err(|e| OpenKoiError::Provider {
            provider: "ollama".into(),
            message: format!("Invalid Ollama response: {}", e),
            retriable: false,
        })?;

        let models: Vec<String> = body["models"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
            .collect();

        self.available_models = models.clone();
        Ok(models)
    }

    pub fn pick_best_model(models: &[String]) -> String {
        let priority = [
            "qwen2.5-coder",
            "codestral",
            "deepseek-coder-v2",
            "llama3.3",
            "llama3.1",
            "mistral",
            "gemma2",
        ];
        for preferred in &priority {
            if let Some(m) = models.iter().find(|m| m.contains(preferred)) {
                return m.clone();
            }
        }
        models.first().cloned().unwrap_or_else(|| "llama3.3".into())
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new(None)
    }
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
    }

    fn name(&self) -> &str {
        "Ollama"
    }

    fn models(&self) -> Vec<ModelInfo> {
        self.available_models
            .iter()
            .map(|m| ModelInfo {
                id: m.clone(),
                name: m.clone(),
                context_window: 128_000,
                max_output_tokens: 32_768,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
            })
            .collect()
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
                msgs.push(serde_json::json!({
                    "role": match m.role {
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::System => "system",
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
            "stream": false,
        });

        if let Some(temp) = request.temperature {
            body["options"] = serde_json::json!({ "temperature": temp });
        }

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "ollama".into(),
                message: e.to_string(),
                retriable: e.is_timeout() || e.is_connect(),
            })?;

        if !response.status().is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "ollama".into(),
                message: format!("HTTP error: {}", error_body),
                retriable: false,
            });
        }

        let resp: serde_json::Value =
            response.json().await.map_err(|e| OpenKoiError::Provider {
                provider: "ollama".into(),
                message: format!("Failed to parse response: {}", e),
                retriable: false,
            })?;

        let content = resp["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let usage = TokenUsage {
            input_tokens: resp["prompt_eval_count"].as_u64().unwrap_or(0) as u32,
            output_tokens: resp["eval_count"].as_u64().unwrap_or(0) as u32,
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
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::System => "system",
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

        if let Some(temp) = request.temperature {
            body["options"] = serde_json::json!({ "temperature": temp });
        }

        let response = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "ollama".into(),
                message: e.to_string(),
                retriable: e.is_timeout() || e.is_connect(),
            })?;

        if !response.status().is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "ollama".into(),
                message: format!("HTTP error: {}", error_body),
                retriable: false,
            });
        }

        // Ollama uses NDJSON streaming, not SSE.
        // Each line is a JSON object: {"message":{"content":"..."},"done":false}
        // The final line has "done":true and includes eval_count/prompt_eval_count.
        let byte_stream = response.bytes_stream();

        let stream = async_stream::stream! {
            let mut buffer = String::new();
            let mut byte_stream = std::pin::pin!(byte_stream);

            while let Some(chunk_result) = byte_stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        yield Err(OpenKoiError::Provider {
                            provider: "ollama".into(),
                            message: format!("Stream read error: {}", e),
                            retriable: false,
                        });
                        break;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&bytes));

                // Process complete lines
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() {
                        continue;
                    }

                    let parsed: serde_json::Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(e) => {
                            yield Err(OpenKoiError::Provider {
                                provider: "ollama".into(),
                                message: format!("Failed to parse NDJSON: {}", e),
                                retriable: false,
                            });
                            break;
                        }
                    };

                    let done = parsed["done"].as_bool().unwrap_or(false);

                    if done {
                        // Final message includes usage stats
                        let input_tokens = parsed["prompt_eval_count"]
                            .as_u64()
                            .unwrap_or(0) as u32;
                        let output_tokens = parsed["eval_count"]
                            .as_u64()
                            .unwrap_or(0) as u32;
                        if input_tokens > 0 || output_tokens > 0 {
                            yield Ok(ChatChunk {
                                delta: String::new(),
                                tool_call_delta: None,
                                usage: Some(TokenUsage {
                                    input_tokens,
                                    output_tokens,
                                    cache_read_tokens: 0,
                                    cache_write_tokens: 0,
                                }),
                            });
                        }
                        break;
                    }

                    let content = parsed["message"]["content"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();

                    if !content.is_empty() {
                        yield Ok(ChatChunk {
                            delta: content,
                            tool_call_delta: None,
                            usage: None,
                        });
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, OpenKoiError> {
        let mut results = Vec::new();
        for text in texts {
            let body = serde_json::json!({
                "model": "nomic-embed-text",
                "prompt": text,
            });
            let response = self
                .client
                .post(format!("{}/api/embeddings", self.base_url))
                .json(&body)
                .send()
                .await
                .map_err(|e| OpenKoiError::Provider {
                    provider: "ollama".into(),
                    message: e.to_string(),
                    retriable: false,
                })?;
            let resp: serde_json::Value =
                response.json().await.map_err(|e| OpenKoiError::Provider {
                    provider: "ollama".into(),
                    message: e.to_string(),
                    retriable: false,
                })?;
            let embedding: Vec<f32> = resp["embedding"]
                .as_array()
                .unwrap_or(&vec![])
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect();
            results.push(embedding);
        }
        Ok(results)
    }
}
