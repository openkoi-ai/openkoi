// src/provider/github_copilot.rs — GitHub Copilot provider (OAuth device-code flow)
//
// Uses GitHub's device code flow for authentication, then exchanges the
// GitHub OAuth token (gho_*) for a short-lived Copilot session token via
// api.github.com/copilot_internal/v2/token. The session token (~30min TTL)
// is used for all api.githubcopilot.com requests (models, chat completions).
//
// The GitHub OAuth token never expires — store as OAuth { access: token, refresh: token, expires: 0 }.

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use reqwest_eventsource::{Event, RequestBuilderExt};
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::model_cache;
use super::{
    ChatChunk, ChatRequest, ChatResponse, ModelInfo, ModelProvider, Role, StopReason, TokenUsage,
    ToolCallDelta,
};
use crate::auth::oauth;
use crate::auth::AuthInfo;
use crate::infra::errors::OpenKoiError;

/// GitHub Copilot OAuth client ID .
pub const GITHUB_CLIENT_ID: &str = "Ov23liEs4iRqyaV7Fa5k";

/// Default API endpoint.
const API_BASE: &str = "https://api.githubcopilot.com";

/// GitHub API endpoint for Copilot session token exchange.
const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";

/// Probe timeout for /models endpoint.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// A short-lived Copilot session token obtained by exchanging the GitHub OAuth token.
#[derive(Debug, Clone)]
struct CopilotSessionToken {
    /// The session token to use as `Authorization: Bearer <token>`.
    token: String,
    /// Unix timestamp (seconds) when this token expires.
    expires_at: u64,
}

impl CopilotSessionToken {
    /// Whether the session token has expired (with a 60-second grace period).
    fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now >= self.expires_at.saturating_sub(60)
    }
}

pub struct GithubCopilotProvider {
    /// The long-lived GitHub OAuth token (gho_*) used to obtain session tokens.
    github_token: String,
    client: reqwest::Client,
    api_base: String,
    /// Short-lived Copilot session token, refreshed automatically.
    session_token: Arc<Mutex<Option<CopilotSessionToken>>>,
    /// Dynamically probed models, populated by `probe_models()`.
    /// Falls back to static defaults if probing fails or hasn't run.
    probed_models: Option<Vec<ModelInfo>>,
}

/// Known Copilot models that are NOT returned by the /models endpoint.
///
/// The Copilot `/models` API only lists GPT models. Claude, Gemini, and
/// reasoning models are supported but must be hardcoded. Model IDs and
/// context windows are based on the Copilot proxy limits (which are
/// smaller than direct API limits for some models).
fn extra_models() -> Vec<ModelInfo> {
    vec![
        // ─── Claude models ─────────────────────────────────────────
        ModelInfo {
            id: "claude-3.5-sonnet".into(),
            name: "Claude 3.5 Sonnet (Copilot)".into(),
            context_window: 90_000,
            max_output_tokens: 8_192,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            supports_vision: true,
            family: Some("claude-3.5".into()),
            ..Default::default()
        },
        ModelInfo {
            id: "claude-3.7-sonnet".into(),
            name: "Claude 3.7 Sonnet (Copilot)".into(),
            context_window: 200_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            supports_vision: true,
            family: Some("claude-3.7".into()),
            ..Default::default()
        },
        ModelInfo {
            id: "claude-3.7-sonnet-thought".into(),
            name: "Claude 3.7 Sonnet Thinking (Copilot)".into(),
            context_window: 200_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            supports_vision: true,
            family: Some("claude-3.7".into()),
            ..Default::default()
        },
        ModelInfo {
            id: "claude-sonnet-4".into(),
            name: "Claude Sonnet 4 (Copilot)".into(),
            context_window: 128_000,
            max_output_tokens: 16_000,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            supports_vision: true,
            family: Some("claude-4".into()),
            ..Default::default()
        },
        // ─── Reasoning models ──────────────────────────────────────
        ModelInfo {
            id: "o1".into(),
            name: "o1 (Copilot)".into(),
            context_window: 200_000,
            max_output_tokens: 100_000,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            family: Some("o1".into()),
            ..Default::default()
        },
        ModelInfo {
            id: "o3-mini".into(),
            name: "o3-mini (Copilot)".into(),
            context_window: 200_000,
            max_output_tokens: 100_000,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            family: Some("o3".into()),
            ..Default::default()
        },
        ModelInfo {
            id: "o4-mini".into(),
            name: "o4-mini (Copilot)".into(),
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            supports_vision: true,
            family: Some("o4".into()),
            ..Default::default()
        },
        // ─── Gemini models ─────────────────────────────────────────
        ModelInfo {
            id: "gemini-2.0-flash-001".into(),
            name: "Gemini 2.0 Flash (Copilot)".into(),
            context_window: 1_000_000,
            max_output_tokens: 8_192,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            supports_vision: true,
            family: Some("gemini-2.0".into()),
            ..Default::default()
        },
        ModelInfo {
            id: "gemini-2.5-pro".into(),
            name: "Gemini 2.5 Pro (Copilot)".into(),
            context_window: 128_000,
            max_output_tokens: 64_000,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            supports_vision: true,
            family: Some("gemini-2.5".into()),
            ..Default::default()
        },
        // ─── GPT (not always returned by /models) ──────────────────
        ModelInfo {
            id: "gpt-4".into(),
            name: "GPT-4 (Copilot)".into(),
            context_window: 32_768,
            max_output_tokens: 4_096,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            supports_vision: true,
            family: Some("gpt-4".into()),
            ..Default::default()
        },
        ModelInfo {
            id: "gpt-4.1".into(),
            name: "GPT-4.1 (Copilot)".into(),
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: true,
            supports_streaming: true,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            supports_vision: true,
            family: Some("gpt-4.1".into()),
            ..Default::default()
        },
    ]
}

impl GithubCopilotProvider {
    pub fn new(token: String) -> Self {
        Self {
            github_token: token,
            client: reqwest::Client::new(),
            api_base: API_BASE.into(),
            session_token: Arc::new(Mutex::new(None)),
            probed_models: None,
        }
    }

    /// Create with a custom API base (for GitHub Enterprise Copilot).
    pub fn with_base(token: String, api_base: String) -> Self {
        Self {
            github_token: token,
            client: reqwest::Client::new(),
            api_base,
            session_token: Arc::new(Mutex::new(None)),
            probed_models: None,
        }
    }

    /// Exchange the GitHub OAuth token for a short-lived Copilot session token.
    ///
    /// The session token is obtained from `api.github.com/copilot_internal/v2/token`
    /// and typically expires after ~30 minutes. This method caches the token and
    /// only re-fetches when expired.
    async fn ensure_session_token(&self) -> Result<String, OpenKoiError> {
        let mut guard = self.session_token.lock().await;

        // Return cached token if still valid
        if let Some(ref st) = *guard {
            if !st.is_expired() {
                return Ok(st.token.clone());
            }
            tracing::debug!("Copilot session token expired, refreshing...");
        }

        // Exchange GitHub OAuth token for Copilot session token
        let response = self
            .client
            .get(COPILOT_TOKEN_URL)
            .header("Authorization", format!("token {}", self.github_token))
            .header(
                "User-Agent",
                format!("openkoi/{}", env!("CARGO_PKG_VERSION")),
            )
            .header(
                "Editor-Version",
                format!("openkoi/{}", env!("CARGO_PKG_VERSION")),
            )
            .header("Editor-Plugin-Version", format!("openkoi/{}", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "copilot".into(),
                message: format!("Failed to exchange token at {COPILOT_TOKEN_URL}: {e}"),
                retriable: e.is_timeout() || e.is_connect(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "copilot".into(),
                message: format!(
                    "Copilot token exchange failed (HTTP {status}): {body}. \
                     Your GitHub token may have been revoked. \
                     Re-authenticate with: openkoi connect copilot"
                ),
                retriable: status.is_server_error(),
            });
        }

        let body: serde_json::Value =
            response.json().await.map_err(|e| OpenKoiError::Provider {
                provider: "copilot".into(),
                message: format!("Failed to parse token exchange response: {e}"),
                retriable: false,
            })?;

        let token = body["token"]
            .as_str()
            .ok_or_else(|| OpenKoiError::Provider {
                provider: "copilot".into(),
                message: "Token exchange response missing 'token' field".into(),
                retriable: false,
            })?
            .to_string();

        let expires_at = body["expires_at"].as_u64().unwrap_or_else(|| {
            // If no expires_at in response, default to 25 minutes from now
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + 25 * 60
        });

        tracing::info!(
            "Copilot session token obtained (expires_at={})",
            expires_at
        );

        let session = CopilotSessionToken {
            token: token.clone(),
            expires_at,
        };
        *guard = Some(session);

        Ok(token)
    }

    /// Common Copilot API headers required by api.githubcopilot.com.
    fn copilot_headers(&self, bearer_token: &str) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        let version = env!("CARGO_PKG_VERSION");

        headers.insert(
            "Authorization",
            format!("Bearer {bearer_token}").parse().unwrap(),
        );
        headers.insert(
            "User-Agent",
            format!("openkoi/{version}").parse().unwrap(),
        );
        headers.insert(
            "Editor-Version",
            format!("openkoi/{version}").parse().unwrap(),
        );
        headers.insert(
            "Editor-Plugin-Version",
            format!("openkoi/{version}").parse().unwrap(),
        );
        headers.insert(
            "Copilot-Integration-Id",
            "openkoi".parse().unwrap(),
        );
        headers.insert(
            "Openai-Organization",
            "github-copilot".parse().unwrap(),
        );
        headers.insert("Openai-Intent", "conversation-edits".parse().unwrap());
        headers.insert("x-initiator", "user".parse().unwrap());

        headers
    }

    /// Probe the Copilot `/models` endpoint to discover available models.
    ///
    /// Checks disk cache first (1-hour TTL). On cache miss, queries the API
    /// and caches the result. Falls back to static defaults on any failure.
    pub async fn probe_models(&mut self) {
        // Try disk cache first
        if let Some(cached) = model_cache::load_cached("copilot") {
            tracing::info!("Copilot: loaded {} models from cache", cached.len());
            self.probed_models = Some(cached);
            return;
        }

        // Obtain a session token first
        let session_token = match self.ensure_session_token().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(
                    "Copilot: failed to obtain session token: {e}. Using static fallback."
                );
                return;
            }
        };

        // Probe the API with the session token
        match self.fetch_models_from_api(&session_token).await {
            Ok(models) if !models.is_empty() => {
                tracing::info!(
                    "Copilot: probed {} models from API: [{}]",
                    models.len(),
                    models
                        .iter()
                        .map(|m| m.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                model_cache::save_cache("copilot", &models);
                self.probed_models = Some(models);
            }
            Ok(_) => {
                tracing::warn!("Copilot /models returned empty list, using static fallback");
            }
            Err(e) => {
                tracing::warn!("Copilot /models probe failed: {e}. Using static fallback.");
            }
        }
    }

    /// Query `GET {api_base}/models` and parse the response.
    async fn fetch_models_from_api(
        &self,
        session_token: &str,
    ) -> Result<Vec<ModelInfo>, OpenKoiError> {
        let url = format!("{}/models", self.api_base);
        let headers = self.copilot_headers(session_token);

        let response = self
            .client
            .get(&url)
            .headers(headers)
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
                        ..Default::default()
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
                // Include tool_calls on assistant messages so that subsequent
                // tool-result messages are properly paired (required by OpenAI API).
                if !m.tool_calls.is_empty() {
                    let tcs: Vec<serde_json::Value> = m
                        .tool_calls
                        .iter()
                        .map(|tc| {
                            serde_json::json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": serde_json::to_string(&tc.arguments).unwrap_or_default(),
                                }
                            })
                        })
                        .collect();
                    msg["tool_calls"] = serde_json::json!(tcs);
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
        // The /models endpoint only returns GPT models. We merge the probed
        // list (which may include dated GPT variants like gpt-4o-2024-11-20)
        // with our hardcoded extra_models() list (Claude, Gemini, reasoning).
        // Deduplication is by model ID — probed models take priority.
        let mut models = self.probed_models.clone().unwrap_or_default();
        let seen: std::collections::HashSet<String> =
            models.iter().map(|m| m.id.clone()).collect();
        for extra in extra_models() {
            if !seen.contains(&extra.id) {
                models.push(extra);
            }
        }
        models
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, OpenKoiError> {
        let body = self.build_request_body(&request);
        let session_token = self.ensure_session_token().await?;
        let headers = self.copilot_headers(&session_token);

        let response = self
            .client
            .post(self.chat_url())
            .headers(headers)
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

        let session_token = self.ensure_session_token().await?;
        let headers = self.copilot_headers(&session_token);

        let request_builder = self
            .client
            .post(self.chat_url())
            .headers(headers)
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
