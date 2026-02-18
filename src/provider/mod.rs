// src/provider/mod.rs — Model provider layer

pub mod anthropic;
pub mod bedrock;
pub mod fallback;
pub mod google;
pub mod ollama;
pub mod openai;
pub mod openai_compat;
pub mod resolver;
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
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_call_id: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
    Unknown,
}

impl Default for StopReason {
    fn default() -> Self {
        Self::Unknown
    }
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
    }

    #[test]
    fn test_message_user() {
        let m = Message::user("Hello");
        assert_eq!(m.role, Role::User);
        assert_eq!(m.content, "Hello");
    }

    #[test]
    fn test_message_assistant() {
        let m = Message::assistant("Sure!");
        assert_eq!(m.role, Role::Assistant);
    }

    #[test]
    fn test_message_tool_result() {
        let m = Message::tool_result("call_123", "result data");
        assert_eq!(m.role, Role::Tool);
        assert_eq!(m.tool_call_id, Some("call_123".into()));
        assert_eq!(m.content, "result data");
    }

    // ─── StopReason tests ───────────────────────────────────────

    #[test]
    fn test_stop_reason_default() {
        let s = StopReason::default();
        assert!(matches!(s, StopReason::Unknown));
    }
}
