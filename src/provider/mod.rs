// src/provider/mod.rs — Model provider layer

pub mod anthropic;
pub mod bedrock;
pub mod fallback;
pub mod github_copilot;
pub mod google;
pub mod model_cache;
pub mod ollama;
pub mod openai;
pub mod openai_compat;
pub mod openai_oauth;
pub mod resolver;
pub mod retry;
pub mod roles;

use async_trait::async_trait;
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

use crate::infra::errors::OpenKoiError;

/// Core trait that all model providers implement.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn models(&self) -> Vec<ModelInfo>;

    async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, OpenKoiError>;

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatChunk, OpenKoiError>> + Send>>, OpenKoiError>;

    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, OpenKoiError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub supports_tools: bool,
    pub supports_streaming: bool,
    pub input_price_per_mtok: f64,
    pub output_price_per_mtok: f64,
    // ─── Extended metadata (Phase 4) ─────────────────────────────
    /// Whether the model supports extended reasoning / chain-of-thought.
    #[serde(default)]
    pub can_reason: bool,
    /// Whether the model supports image/vision inputs.
    #[serde(default)]
    pub supports_vision: bool,
    /// Whether the model supports file attachments.
    #[serde(default)]
    pub supports_attachments: bool,
    /// Lifecycle status of the model.
    #[serde(default)]
    pub status: ModelStatus,
    /// Model family grouping (e.g., "gpt-4o", "claude-sonnet").
    #[serde(default)]
    pub family: Option<String>,
    /// Release date in ISO 8601 format (e.g., "2025-01-01").
    #[serde(default)]
    pub release_date: Option<String>,
    /// Pricing for prompt cache reads (per million tokens).
    #[serde(default)]
    pub cache_read_price_per_mtok: f64,
    /// Pricing for prompt cache writes (per million tokens).
    #[serde(default)]
    pub cache_write_price_per_mtok: f64,
}

/// Lifecycle status of a model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelStatus {
    #[default]
    Active,
    Beta,
    Deprecated,
}

impl Default for ModelInfo {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            context_window: 128_000,
            max_output_tokens: 16_384,
            supports_tools: false,
            supports_streaming: false,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
            can_reason: false,
            supports_vision: false,
            supports_attachments: false,
            status: ModelStatus::Active,
            family: None,
            release_date: None,
            cache_read_price_per_mtok: 0.0,
            cache_write_price_per_mtok: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub system: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
    pub stop_reason: StopReason,
}

#[derive(Debug, Clone)]
pub struct ChatChunk {
    pub delta: String,
    pub tool_call_delta: Option<ToolCallDelta>,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub tool_call_id: Option<String>,
    /// Tool calls made by the assistant in this message.
    /// Required by OpenAI-compatible APIs: when a `Role::Tool` message follows an
    /// `Role::Assistant` message, the assistant message MUST include the `tool_calls`
    /// that the tool results are responding to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// Create an assistant message that includes tool calls.
    /// This is required by OpenAI-compatible APIs: tool result messages
    /// must be preceded by an assistant message carrying the `tool_calls`.
    pub fn assistant_with_tool_calls(
        content: impl Into<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_call_id: None,
            tool_calls,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
}

impl TokenUsage {
    pub fn total(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ToolCallDelta {
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments_delta: String,
}

/// Reference to a specific model on a specific provider.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

impl ModelRef {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
        }
    }

    /// Parse "provider/model" format
    pub fn parse(s: &str) -> Option<Self> {
        let (provider, model) = s.split_once('/')?;
        Some(Self {
            provider: provider.to_string(),
            model: model.to_string(),
        })
    }
}

impl std::fmt::Display for ModelRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── ModelRef tests ─────────────────────────────────────────

    #[test]
    fn test_model_ref_new() {
        let r = ModelRef::new("anthropic", "claude-sonnet-4");
        assert_eq!(r.provider, "anthropic");
        assert_eq!(r.model, "claude-sonnet-4");
    }

    #[test]
    fn test_model_ref_parse() {
        let r = ModelRef::parse("anthropic/claude-sonnet-4").unwrap();
        assert_eq!(r.provider, "anthropic");
        assert_eq!(r.model, "claude-sonnet-4");
    }

    #[test]
    fn test_model_ref_parse_no_slash() {
        assert!(ModelRef::parse("no-slash").is_none());
    }

    #[test]
    fn test_model_ref_parse_empty() {
        assert!(ModelRef::parse("").is_none());
    }

    #[test]
    fn test_model_ref_display() {
        let r = ModelRef::new("openai", "gpt-4.1");
        assert_eq!(format!("{}", r), "openai/gpt-4.1");
    }

    #[test]
    fn test_model_ref_equality() {
        let a = ModelRef::new("x", "y");
        let b = ModelRef::new("x", "y");
        let c = ModelRef::new("x", "z");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // ─── TokenUsage tests ───────────────────────────────────────

    #[test]
    fn test_token_usage_total() {
        let u = TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        };
        assert_eq!(u.total(), 150);
    }

    #[test]
    fn test_token_usage_default() {
        let u = TokenUsage::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.total(), 0);
    }

    // ─── Message tests ──────────────────────────────────────────

    #[test]
    fn test_message_system() {
        let m = Message::system("You are helpful");
        assert_eq!(m.role, Role::System);
        assert_eq!(m.content, "You are helpful");
        assert!(m.tool_call_id.is_none());
        assert!(m.tool_calls.is_empty());
    }

    #[test]
    fn test_message_user() {
        let m = Message::user("Hello");
        assert_eq!(m.role, Role::User);
        assert_eq!(m.content, "Hello");
        assert!(m.tool_calls.is_empty());
    }

    #[test]
    fn test_message_assistant() {
        let m = Message::assistant("Sure!");
        assert_eq!(m.role, Role::Assistant);
        assert!(m.tool_calls.is_empty());
    }

    #[test]
    fn test_message_assistant_with_tool_calls() {
        let tool_calls = vec![
            ToolCall {
                id: "call_1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "index.html"}),
            },
            ToolCall {
                id: "call_2".into(),
                name: "write_file".into(),
                arguments: serde_json::json!({"path": "index.html", "content": "<html>"}),
            },
        ];
        let m = Message::assistant_with_tool_calls("I'll help with that.", tool_calls.clone());
        assert_eq!(m.role, Role::Assistant);
        assert_eq!(m.content, "I'll help with that.");
        assert_eq!(m.tool_calls.len(), 2);
        assert_eq!(m.tool_calls[0].id, "call_1");
        assert_eq!(m.tool_calls[1].name, "write_file");
        assert!(m.tool_call_id.is_none());
    }

    #[test]
    fn test_message_tool_result() {
        let m = Message::tool_result("call_123", "result data");
        assert_eq!(m.role, Role::Tool);
        assert_eq!(m.tool_call_id, Some("call_123".into()));
        assert_eq!(m.content, "result data");
        assert!(m.tool_calls.is_empty());
    }

    // ─── StopReason tests ───────────────────────────────────────

    #[test]
    fn test_stop_reason_default() {
        let s = StopReason::default();
        assert!(matches!(s, StopReason::Unknown));
    }
}
