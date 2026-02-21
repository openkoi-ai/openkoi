// src/provider/github_copilot.rs — GitHub Copilot provider (OAuth device-code flow)
//
// Uses GitHub's device code flow for authentication, then hits the
// OpenAI-compatible API at api.githubcopilot.com/chat/completions.
//
// The token never expires — store as OAuth { access: token, refresh: token, expires: 0 }.

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use reqwest_eventsource::{Event, RequestBuilderExt};
use std::pin::Pin;

use super::{
    ChatChunk, ChatRequest, ChatResponse, ModelInfo, ModelProvider, Role, StopReason, TokenUsage,
    ToolCallDelta,
};
use super::model_cache;
use crate::auth::oauth;
use crate::auth::AuthInfo;
use crate::infra::errors::OpenKoiError;

/// GitHub Copilot OAuth client ID .
pub const GITHUB_CLIENT_ID: &str = "Ov23liEs4iRqyaV7Fa5k";

/// Default API endpoint.
const API_BASE: &str = "https://api.githubcopilot.com";

/// Probe timeout for /models endpoint.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct GithubCopilotProvider {
    token: String,
    client: reqwest::Client,
    api_base: String,
    /// Dynamically probed models, populated by `probe_models()`.
    /// Falls back to static defaults if probing fails or hasn't run.
    probed_models: Option<Vec<ModelInfo>>,
}

/// Static fallback models (used when probing fails or cache is cold on first install).
fn static_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gpt-4o".into(),
            name: "GPT-4o (Copilot)".into(),
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
        },
        ModelInfo {
            id: "gpt-4o-mini".into(),
            name: "GPT-4o mini (Copilot)".into(),
            context_window: 128_000,
            max_output_tokens: 4_096,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
        },
        ModelInfo {
            id: "gpt-3.5-turbo".into(),
            name: "GPT-3.5 Turbo (Copilot)".into(),
            context_window: 16_384,
            max_output_tokens: 4_096,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
        },
    ]
}

impl GithubCopilotProvider {
    pub fn new(token: String) -> Self {
        Self {
            token,
            client: reqwest::Client::new(),
            api_base: API_BASE.into(),
            probed_models: None,
        }
    }

    /// Create with a custom API base (for GitHub Enterprise Copilot).
    pub fn with_base(token: String, api_base: String) -> Self {
        Self {
            token,
            client: reqwest::Client::new(),
            api_base,
            probed_models: None,
        }
    }

    /// Probe the Copilot `/models` endpoint to discover available models.
    ///
    /// Checks disk cache first (1-hour TTL). On cache miss, queries the API
    /// and caches the result. Falls back to static defaults on any failure.
    pub async fn probe_models(&mut self) {
        // Try disk cache first
        if let Some(cached) = model_cache::load_cached("copilot") {
            tracing::info!(
                "Copilot: loaded {} models from cache",
                cached.len()
            );
            self.probed_models = Some(cached);
            return;
        }

        // Probe the API
        match self.fetch_models_from_api().await {
            Ok(models) if !models.is_empty() => {
                tracing::info!(
                    "Copilot: probed {} models from API: [{}]",
                    models.len(),
                    models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")
                );
                model_cache::save_cache("copilot", &models);
                self.probed_models = Some(models);
            }
            Ok(_) => {
                tracing::warn!(
                    "Copilot /models returned empty list, using static fallback"
                );
            }
            Err(e) => {
                tracing::warn!(
                    "Copilot /models probe failed: {e}. Using static fallback."
                );
            }
        }
    }

    /// Query `GET {api_base}/models` and parse the response.
    async fn fetch_models_from_api(&self) -> Result<Vec<ModelInfo>, OpenKoiError> {
        let url = format!("{}/models", self.api_base);
        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header(
                "User-Agent",
                format!("openkoi/{}", env!("CARGO_PKG_VERSION")),
            )
            .timeout(PROBE_TIMEOUT)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "copilot".into(),
                message: format!("Failed to probe /models: {e}"),
                retriable: false,
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "copilot".into(),
                message: format!("/models returned HTTP {status}: {body}"),
                retriable: false,
            });
        }

        let body: serde_json::Value =
            response.json().await.map_err(|e| OpenKoiError::Provider {
                provider: "copilot".into(),
                message: format!("Failed to parse /models response: {e}"),
                retriable: false,
            })?;

        // The Copilot /models endpoint returns {"data": [{"id": "...", ...}, ...]}
        // or {"models": [{"id": "...", ...}, ...]} depending on version.
        let models_array = body["data"]
            .as_array()
            .or_else(|| body["models"].as_array());

        let models = match models_array {
            Some(arr) => arr
                .iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    // Use model name from API, fall back to id
                    let name = m["name"]
                        .as_str()
                        .or_else(|| m["id"].as_str())
                        .unwrap_or("Unknown")
                        .to_string();
                    let name = format!("{name} (Copilot)");

                    // Extract capabilities if available, with sensible defaults
                    let context_window = m["context_window"]
                        .as_u64()
                        .or_else(|| m["max_input_tokens"].as_u64())
                        .unwrap_or(128_000) as u32;
                    let max_output_tokens = m["max_output_tokens"]
                        .as_u64()
                        .or_else(|| m["max_tokens"].as_u64())
                        .unwrap_or(16_384) as u32;

                    // Copilot models support tools and streaming by default
                    let supports_tools = m["capabilities"]["supports_tools"]
                        .as_bool()
                        .unwrap_or(true);
                    let supports_streaming = m["capabilities"]["supports_streaming"]
                        .as_bool()
                        .unwrap_or(true);

                    Some(ModelInfo {
                        id,
                        name,
                        context_window,
                        max_output_tokens,
                        supports_tools,
                        supports_streaming,
                        // Copilot is included with GitHub subscription
                        input_price_per_mtok: 0.0,
                        output_price_per_mtok: 0.0,
                    })
                })
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };

        Ok(models)
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.api_base)
    }

    /// Build an OpenAI-compatible request body.
    fn build_request_body(&self, request: &ChatRequest) -> serde_json::Value {
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

        body
    }
}

#[async_trait]
impl ModelProvider for GithubCopilotProvider {
    fn id(&self) -> &str {
        "copilot"
    }

    fn name(&self) -> &str {
        "GitHub Copilot"
    }

    fn models(&self) -> Vec<ModelInfo> {
        // Return probed models if available, otherwise fall back to static list
        self.probed_models.clone().unwrap_or_else(static_models)
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, OpenKoiError> {
        let body = self.build_request_body(&request);

        let response = self
            .client
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", self.token))
            .header(
                "User-Agent",
                format!("openkoi/{}", env!("CARGO_PKG_VERSION")),
            )
            .header("Openai-Intent", "conversation-edits")
            .header("x-initiator", "user")
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "copilot".into(),
                message: e.to_string(),
                retriable: e.is_timeout() || e.is_connect(),
            })?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(OpenKoiError::RateLimited {
                provider: "copilot".into(),
                retry_after_ms: 5000,
            });
        }
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "copilot".into(),
                message: format!("HTTP {}: {}", status, error_body),
                retriable: status.is_server_error(),
            });
        }

        let resp: serde_json::Value =
            response.json().await.map_err(|e| OpenKoiError::Provider {
                provider: "copilot".into(),
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
        let mut body = self.build_request_body(&request);
        body["stream"] = serde_json::json!(true);
        body["stream_options"] = serde_json::json!({"include_usage": true});

        let request_builder = self
            .client
            .post(self.chat_url())
            .header("Authorization", format!("Bearer {}", self.token))
            .header(
                "User-Agent",
                format!("openkoi/{}", env!("CARGO_PKG_VERSION")),
            )
            .header("Openai-Intent", "conversation-edits")
            .header("x-initiator", "user")
            .json(&body);

        let mut es = request_builder
            .eventsource()
            .map_err(|e| OpenKoiError::Provider {
                provider: "copilot".into(),
                message: format!("Failed to start SSE stream: {}", e),
                retriable: false,
            })?;

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
                                    provider: "copilot".into(),
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
                            provider: "copilot".into(),
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
            provider: "copilot".into(),
            message: "GitHub Copilot does not support embeddings".into(),
            retriable: false,
        })
    }
}

// ─── OAuth device code flow ─────────────────────────────────────────────────

/// Run the GitHub device code flow interactively.
/// Returns an AuthInfo::OAuth with the GitHub token.
pub async fn github_device_code_flow() -> anyhow::Result<AuthInfo> {
    let client = reqwest::Client::new();

    eprintln!("  Starting GitHub Copilot authentication...");
    eprintln!();

    // Step 1: Request device code
    let resp = client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&[("client_id", GITHUB_CLIENT_ID), ("scope", "read:user")])
        .send()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to request device code from GitHub: {e}. Check your internet connection."
            )
        })?;

    let body: serde_json::Value = resp.json().await?;

    let device_code = body["device_code"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No device_code in response"))?
        .to_string();
    let user_code = body["user_code"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No user_code in response"))?
        .to_string();
    let verification_uri = body["verification_uri"]
        .as_str()
        .unwrap_or("https://github.com/login/device")
        .to_string();
    let interval = body["interval"].as_u64().unwrap_or(5);

    // Step 2: Show user the code and open browser
    eprintln!("  Open this URL in your browser:");
    eprintln!("    {}", verification_uri);
    eprintln!();
    eprintln!("  Enter code: {}", user_code);
    eprintln!();

    oauth::open_browser(&verification_uri);

    eprintln!("  Waiting for authorization...");

    // Step 3: Poll for token
    let mut poll_interval = std::time::Duration::from_secs(interval);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(600); // 10 min
    let mut poll_count = 0u32;
    loop {
        tokio::time::sleep(poll_interval).await;

        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "Device code flow timed out after 10 minutes. \
                 Make sure you opened {} in your browser and entered code {}. \
                 Please try again with: openkoi connect copilot",
                verification_uri,
                user_code,
            );
        }

        // Show progress indicator
        poll_count += 1;
        if poll_count.is_multiple_of(6) {
            let elapsed = poll_count * poll_interval.as_secs() as u32;
            eprint!("\r  Waiting for authorization... ({elapsed}s elapsed)  ");
        }

        let resp = client
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[
                ("client_id", GITHUB_CLIENT_ID),
                ("device_code", device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;

        if let Some(token) = body["access_token"].as_str() {
            if poll_count >= 6 {
                eprintln!(); // Clear progress line
            }
            eprintln!("  Authenticated successfully!");
            // GitHub Copilot tokens do not expire and have no refresh flow.
            // We store the token as both access and refresh, with expires_at=0
            // to signal "never expires" to the is_expired() check.
            return Ok(AuthInfo::oauth(token, token, 0));
        }

        match body["error"].as_str() {
            Some("authorization_pending") => {
                // Keep polling
                continue;
            }
            Some("slow_down") => {
                // Permanently increase interval by 5 seconds per RFC 8628
                poll_interval += std::time::Duration::from_secs(5);
                continue;
            }
            Some("expired_token") => {
                anyhow::bail!("Device code expired. Please try again.");
            }
            Some("access_denied") => {
                anyhow::bail!("Authorization denied by user.");
            }
            Some(err) => {
                let desc = body["error_description"]
                    .as_str()
                    .unwrap_or("Unknown error");
                anyhow::bail!("GitHub OAuth error: {} — {}", err, desc);
            }
            None => {
                anyhow::bail!(
                    "Unexpected response from GitHub: {}",
                    serde_json::to_string_pretty(&body).unwrap_or_default()
                );
            }
        }
    }
}
