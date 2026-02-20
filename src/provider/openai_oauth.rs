// src/provider/openai_oauth.rs — OpenAI ChatGPT Plus/Pro Codex provider (OAuth)
//
// Uses a device-code variant flow for authentication, then hits
// chatgpt.com/backend-api/codex/responses (NOT the standard OpenAI API).
//
// The Codex API uses OpenAI's Responses API format, not Chat Completions.

use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use reqwest_eventsource::{Event, RequestBuilderExt};
use std::collections::HashMap;
use std::pin::Pin;

use super::{
    ChatChunk, ChatRequest, ChatResponse, ModelInfo, ModelProvider, Role, StopReason, TokenUsage,
    ToolCallDelta,
};
use crate::auth::oauth;
use crate::auth::AuthInfo;
use crate::infra::errors::OpenKoiError;

/// OpenAI Codex OAuth Client ID.
pub const OPENAI_CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

const API_BASE: &str = "https://chatgpt.com/backend-api/codex/responses";
const AUTH_BASE: &str = "https://auth.openai.com";

pub struct OpenAICodexProvider {
    access_token: String,
    account_id: String,
    client: reqwest::Client,
}

impl OpenAICodexProvider {
    pub fn new(access_token: String, account_id: String) -> Self {
        Self {
            access_token,
            account_id,
            client: reqwest::Client::new(),
        }
    }

    /// Build a Responses API body from a ChatRequest.
    /// The Codex endpoint uses OpenAI's Responses API format.
    fn build_request_body(&self, request: &ChatRequest) -> serde_json::Value {
        // Convert messages to the Responses API "input" format
        let mut input = Vec::new();

        if let Some(system) = &request.system {
            input.push(serde_json::json!({
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
            input.push(msg);
        }

        let mut body = serde_json::json!({
            "model": request.model,
            "input": input,
        });

        if let Some(max_tokens) = request.max_tokens {
            body["max_output_tokens"] = serde_json::json!(max_tokens);
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
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
        }

        body
    }
}

#[async_trait]
impl ModelProvider for OpenAICodexProvider {
    fn id(&self) -> &str {
        "chatgpt"
    }

    fn name(&self) -> &str {
        "ChatGPT (Plus/Pro)"
    }

    fn models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "gpt-5.1-codex".into(),
                name: "GPT-5.1 Codex (ChatGPT)".into(),
                context_window: 200_000,
                max_output_tokens: 32_768,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
            },
            ModelInfo {
                id: "gpt-5.1-codex-max".into(),
                name: "GPT-5.1 Codex Max (ChatGPT)".into(),
                context_window: 200_000,
                max_output_tokens: 65_536,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
            },
            ModelInfo {
                id: "gpt-5.1-codex-mini".into(),
                name: "GPT-5.1 Codex Mini (ChatGPT)".into(),
                context_window: 200_000,
                max_output_tokens: 16_384,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
            },
            ModelInfo {
                id: "gpt-5.2-codex".into(),
                name: "GPT-5.2 Codex (ChatGPT)".into(),
                context_window: 200_000,
                max_output_tokens: 32_768,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
            },
            ModelInfo {
                id: "gpt-5.3-codex".into(),
                name: "GPT-5.3 Codex (ChatGPT)".into(),
                context_window: 200_000,
                max_output_tokens: 32_768,
                supports_tools: true,
                supports_streaming: true,
                input_price_per_mtok: 0.0,
                output_price_per_mtok: 0.0,
            },
        ]
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, OpenKoiError> {
        let body = self.build_request_body(&request);

        let response = self
            .client
            .post(API_BASE)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("ChatGPT-Account-Id", &self.account_id)
            .header("originator", "openkoi")
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenKoiError::Provider {
                provider: "chatgpt".into(),
                message: e.to_string(),
                retriable: e.is_timeout() || e.is_connect(),
            })?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(OpenKoiError::RateLimited {
                provider: "chatgpt".into(),
                retry_after_ms: 5000,
            });
        }
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            return Err(OpenKoiError::Provider {
                provider: "chatgpt".into(),
                message: format!("HTTP {}: {}", status, error_body),
                retriable: status.is_server_error(),
            });
        }

        let resp: serde_json::Value =
            response.json().await.map_err(|e| OpenKoiError::Provider {
                provider: "chatgpt".into(),
                message: format!("Failed to parse response: {}", e),
                retriable: false,
            })?;

        // Responses API format: output is an array of items
        let mut content = String::new();
        let mut tool_calls = Vec::new();

        if let Some(output) = resp["output"].as_array() {
            for item in output {
                match item["type"].as_str() {
                    Some("message") => {
                        if let Some(msg_content) = item["content"].as_array() {
                            for c in msg_content {
                                if c["type"] == "output_text" {
                                    if let Some(text) = c["text"].as_str() {
                                        content.push_str(text);
                                    }
                                }
                            }
                        }
                    }
                    Some("function_call") => {
                        tool_calls.push(super::ToolCall {
                            id: item["call_id"].as_str().unwrap_or("").to_string(),
                            name: item["name"].as_str().unwrap_or("").to_string(),
                            arguments: serde_json::from_str(
                                item["arguments"].as_str().unwrap_or("{}"),
                            )
                            .unwrap_or_default(),
                        });
                    }
                    _ => {}
                }
            }
        }

        let usage = TokenUsage {
            input_tokens: resp["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: resp["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };

        let stop_reason = match resp["status"].as_str() {
            Some("completed") => StopReason::EndTurn,
            Some("incomplete") => StopReason::MaxTokens,
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
            .post(API_BASE)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("ChatGPT-Account-Id", &self.account_id)
            .header("originator", "openkoi")
            .json(&body);

        let mut es = request_builder.eventsource().map_err(|e| OpenKoiError::Provider {
            provider: "chatgpt".into(),
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
                                    provider: "chatgpt".into(),
                                    message: format!("Failed to parse SSE data: {}", e),
                                    retriable: false,
                                });
                                break;
                            }
                        };

                        let event_type = parsed["type"].as_str().unwrap_or("");

                        match event_type {
                            // Responses API streaming: response.output_text.delta
                            "response.output_text.delta" => {
                                let text = parsed["delta"]
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
                            // Function call streaming
                            "response.function_call_arguments.delta" => {
                                let args_delta = parsed["delta"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string();
                                yield Ok(ChatChunk {
                                    delta: String::new(),
                                    tool_call_delta: Some(ToolCallDelta {
                                        id: parsed["item_id"].as_str().map(|s| s.to_string()),
                                        name: None,
                                        arguments_delta: args_delta,
                                    }),
                                    usage: None,
                                });
                            }
                            "response.function_call_arguments.done" => {
                                // Function call complete — name is in the done event
                                yield Ok(ChatChunk {
                                    delta: String::new(),
                                    tool_call_delta: Some(ToolCallDelta {
                                        id: parsed["item_id"].as_str().map(|s| s.to_string()),
                                        name: parsed["name"].as_str().map(|s| s.to_string()),
                                        arguments_delta: String::new(),
                                    }),
                                    usage: None,
                                });
                            }
                            // Usage in completion event
                            "response.completed" => {
                                let usage = if parsed["response"]["usage"].is_object() {
                                    Some(TokenUsage {
                                        input_tokens: parsed["response"]["usage"]["input_tokens"]
                                            .as_u64()
                                            .unwrap_or(0) as u32,
                                        output_tokens: parsed["response"]["usage"]["output_tokens"]
                                            .as_u64()
                                            .unwrap_or(0) as u32,
                                        cache_read_tokens: 0,
                                        cache_write_tokens: 0,
                                    })
                                } else {
                                    None
                                };
                                if usage.is_some() {
                                    yield Ok(ChatChunk {
                                        delta: String::new(),
                                        tool_call_delta: None,
                                        usage,
                                    });
                                }
                            }
                            _ => {} // other events: response.created, response.in_progress, etc.
                        }
                    }
                    Err(reqwest_eventsource::Error::StreamEnded) => break,
                    Err(e) => {
                        yield Err(OpenKoiError::Provider {
                            provider: "chatgpt".into(),
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
            provider: "chatgpt".into(),
            message: "ChatGPT Codex does not support embeddings".into(),
            retriable: false,
        })
    }
}

// ─── OAuth device code flow ─────────────────────────────────────────────────

/// Run the OpenAI Codex device code flow interactively.
/// Returns an AuthInfo::OAuth with access + refresh tokens and account_id in extra.
pub async fn openai_codex_device_flow() -> anyhow::Result<AuthInfo> {
    let client = reqwest::Client::new();

    eprintln!("  Starting OpenAI ChatGPT Plus/Pro authentication...");
    eprintln!();

    // Step 1: Request device code
    let resp = client
        .post(format!("{}/api/accounts/deviceauth/usercode", AUTH_BASE))
        .json(&serde_json::json!({
            "client_id": OPENAI_CODEX_CLIENT_ID,
        }))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to request device code from OpenAI: {e}. Check your internet connection."))?;

    if !resp.status().is_success() {
        let error_body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to request device code: {}", error_body);
    }

    let body: serde_json::Value = resp.json().await?;

    let device_auth_id = body["device_auth_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No device_auth_id in response"))?
        .to_string();
    let user_code = body["user_code"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No user_code in response"))?
        .to_string();

    let verification_url = "https://auth.openai.com/codex/device";

    // Step 2: Show user the code and open browser
    eprintln!("  Open this URL in your browser:");
    eprintln!("    {}", verification_url);
    eprintln!();
    eprintln!("  Enter code: {}", user_code);
    eprintln!();

    oauth::open_browser(verification_url);

    eprintln!("  Waiting for authorization...");

    // Step 3: Poll for authorization
    let mut poll_interval = std::time::Duration::from_secs(5);
    let authorization_code;
    let code_verifier;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(600); // 10 min
    let mut poll_count = 0u32;

    loop {
        tokio::time::sleep(poll_interval).await;

        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!(
                "Device code flow timed out after 10 minutes. \
                 Make sure you opened the URL above in your browser and entered the code. \
                 Please try again with: openkoi connect chatgpt"
            );
        }

        // Show progress indicator
        poll_count += 1;
        if poll_count.is_multiple_of(6) {
            let elapsed = poll_count * poll_interval.as_secs() as u32;
            eprint!("\r  Waiting for authorization... ({elapsed}s elapsed)  ");
        }

        let resp = client
            .post(format!("{}/api/accounts/deviceauth/token", AUTH_BASE))
            .json(&serde_json::json!({
                "device_auth_id": device_auth_id,
                "user_code": user_code,
            }))
            .send()
            .await?;

        let status = resp.status();

        if status.as_u16() == 403 || status.as_u16() == 404 {
            // Still pending
            continue;
        }

        if status.as_u16() == 429 {
            // Rate limited during polling — back off
            poll_interval += std::time::Duration::from_secs(5);
            continue;
        }

        if status.is_success() {
            eprintln!(); // Clear progress line
            let body: serde_json::Value = resp.json().await?;
            authorization_code = body["authorization_code"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("No authorization_code in response"))?
                .to_string();
            code_verifier = body["code_verifier"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("No code_verifier in response"))?
                .to_string();
            break;
        }

        let error_body = resp.text().await.unwrap_or_default();
        eprintln!(); // Clear progress line
        anyhow::bail!("Unexpected response during polling: HTTP {} — {}", status, error_body);
    }

    // Step 4: Exchange authorization code for tokens
    let resp = client
        .post(format!("{}/oauth/token", AUTH_BASE))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
            oauth::urlencoding(&authorization_code),
            oauth::urlencoding("https://auth.openai.com/deviceauth/callback"),
            oauth::urlencoding(OPENAI_CODEX_CLIENT_ID),
            oauth::urlencoding(&code_verifier),
        ))
        .send()
        .await?;

    if !resp.status().is_success() {
        let error_body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Token exchange failed: {}", error_body);
    }

    let body: serde_json::Value = resp.json().await?;

    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No access_token in response"))?
        .to_string();
    let refresh_token = body["refresh_token"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if refresh_token.is_empty() {
        eprintln!("  Warning: No refresh token received. You may need to re-authenticate when the token expires.");
    }
    let expires_in = body["expires_in"].as_u64().unwrap_or(3600);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let expires_at = now + expires_in;

    // Extract account_id from JWT claims
    let account_id = match oauth::decode_jwt_payload(&access_token) {
        Ok(claims) => claims["https://api.openai.com/auth"]["user_id"]
            .as_str()
            .or_else(|| claims["sub"].as_str())
            .unwrap_or("")
            .to_string(),
        Err(_) => String::new(),
    };

    if account_id.is_empty() {
        eprintln!("  Warning: Could not extract account_id from JWT. Some API calls may fail.");
        eprintln!("  If you experience issues, try re-authenticating with: openkoi connect chatgpt");
    }

    let mut extra = HashMap::new();
    if !account_id.is_empty() {
        extra.insert("account_id".into(), account_id);
    }

    eprintln!("  Authenticated successfully!");

    Ok(AuthInfo::oauth_with_extra(
        access_token,
        refresh_token,
        expires_at,
        extra,
    ))
}

/// Refresh an OpenAI Codex token.
pub async fn openai_codex_refresh_token(refresh_token: &str) -> anyhow::Result<AuthInfo> {
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/oauth/token", AUTH_BASE))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}",
            oauth::urlencoding(refresh_token),
            oauth::urlencoding(OPENAI_CODEX_CLIENT_ID),
        ))
        .send()
        .await?;

    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        let error_body = resp.text().await.unwrap_or_default();
        anyhow::bail!(
            "OpenAI refresh token was rejected (HTTP {status}): {error_body}\n\
             Your token may have been revoked or your subscription may have changed.\n\
             Please re-authenticate with: openkoi connect chatgpt"
        );
    }
    if !status.is_success() {
        let error_body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Token refresh failed (HTTP {status}): {error_body}");
    }

    let body: serde_json::Value = resp.json().await?;

    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No access_token in refresh response"))?
        .to_string();
    let new_refresh = body["refresh_token"]
        .as_str()
        .unwrap_or(refresh_token)
        .to_string();
    let expires_in = body["expires_in"].as_u64().unwrap_or(3600);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let expires_at = now + expires_in;

    // Extract account_id from JWT claims
    let account_id = match oauth::decode_jwt_payload(&access_token) {
        Ok(claims) => claims["https://api.openai.com/auth"]["user_id"]
            .as_str()
            .or_else(|| claims["sub"].as_str())
            .unwrap_or("")
            .to_string(),
        Err(_) => String::new(),
    };

    let mut extra = HashMap::new();
    if !account_id.is_empty() {
        extra.insert("account_id".into(), account_id);
    }

    Ok(AuthInfo::oauth_with_extra(
        access_token,
        new_refresh,
        expires_at,
        extra,
    ))
}

