// tests/integrations_test.rs — Integration tests for the integrations subsystem

use openkoi::integrations::credentials::{validate_token_format, IntegrationCredentials};
use openkoi::integrations::registry::IntegrationRegistry;
use openkoi::integrations::types::*;

use async_trait::async_trait;

// ---------- Mock adapters for testing ----------

struct MockMessagingAdapter;

#[async_trait]
impl MessagingAdapter for MockMessagingAdapter {
    async fn send(&self, target: &str, _content: &str) -> anyhow::Result<String> {
        Ok(format!("sent-to-{target}"))
    }

    async fn history(&self, channel: &str, _limit: u32) -> anyhow::Result<Vec<IncomingMessage>> {
        Ok(vec![IncomingMessage {
            id: "msg1".into(),
            channel: channel.to_string(),
            sender: "user1".into(),
            content: "Hello from mock".into(),
            timestamp: "2026-01-01T00:00:00Z".into(),
        }])
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<IncomingMessage>> {
        Ok(vec![IncomingMessage {
            id: "msg2".into(),
            channel: "general".into(),
            sender: "user2".into(),
            content: format!("Found: {query}"),
            timestamp: "2026-01-01T00:00:00Z".into(),
        }])
    }
}

struct MockDocumentAdapter;

#[async_trait]
impl DocumentAdapter for MockDocumentAdapter {
    async fn read(&self, doc_id: &str) -> anyhow::Result<Document> {
        Ok(Document {
            id: doc_id.to_string(),
            title: "Mock Doc".into(),
            content: "Mock content".into(),
            url: None,
        })
    }

    async fn write(&self, _doc_id: &str, _content: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn create(&self, title: &str, _content: &str) -> anyhow::Result<String> {
        Ok(format!("doc-{title}"))
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<DocumentRef>> {
        Ok(vec![DocumentRef {
            id: "doc1".into(),
            title: format!("Result: {query}"),
            url: None,
        }])
    }

    async fn list(&self, _folder: Option<&str>) -> anyhow::Result<Vec<DocumentRef>> {
        Ok(vec![DocumentRef {
            id: "doc1".into(),
            title: "First Doc".into(),
            url: Some("https://example.com/doc1".into()),
        }])
    }
}

/// A mock messaging integration.
struct MockMessagingIntegration;

impl Integration for MockMessagingIntegration {
    fn id(&self) -> &str {
        "mock_chat"
    }
    fn name(&self) -> &str {
        "Mock Chat"
    }
    fn messaging(&self) -> Option<&dyn MessagingAdapter> {
        Some(&MockMessagingAdapter)
    }
    fn document(&self) -> Option<&dyn DocumentAdapter> {
        None
    }
}

/// A mock document integration.
struct MockDocIntegration;

impl Integration for MockDocIntegration {
    fn id(&self) -> &str {
        "mock_docs"
    }
    fn name(&self) -> &str {
        "Mock Docs"
    }
    fn messaging(&self) -> Option<&dyn MessagingAdapter> {
        None
    }
    fn document(&self) -> Option<&dyn DocumentAdapter> {
        Some(&MockDocumentAdapter)
    }
}

/// A mock hybrid integration (both messaging and document).
struct MockHybridIntegration;

impl Integration for MockHybridIntegration {
    fn id(&self) -> &str {
        "mock_hybrid"
    }
    fn name(&self) -> &str {
        "Mock Hybrid"
    }
    fn messaging(&self) -> Option<&dyn MessagingAdapter> {
        Some(&MockMessagingAdapter)
    }
    fn document(&self) -> Option<&dyn DocumentAdapter> {
        Some(&MockDocumentAdapter)
    }
}

// ---------- Registry tests ----------

#[test]
fn test_registry_empty() {
    let registry = IntegrationRegistry::new();
    assert!(registry.list().is_empty());
    assert!(registry.all_tools().is_empty());
    assert!(registry.get("slack").is_none());
}

#[test]
fn test_registry_register_messaging() {
    let mut registry = IntegrationRegistry::new();
    registry.register(Box::new(MockMessagingIntegration));

    assert_eq!(registry.list(), vec!["mock_chat"]);
    assert!(registry.get("mock_chat").is_some());
    assert!(registry.get("nonexistent").is_none());

    // Messaging integration should have _send, _read, and _search tools
    let tools = registry.all_tools();
    assert_eq!(tools.len(), 3);
    assert!(tools.iter().any(|t| t.name == "mock_chat_send"));
    assert!(tools.iter().any(|t| t.name == "mock_chat_read"));
    assert!(tools.iter().any(|t| t.name == "mock_chat_search"));
}

#[test]
fn test_registry_register_document() {
    let mut registry = IntegrationRegistry::new();
    registry.register(Box::new(MockDocIntegration));

    let tools = registry.all_tools();
    // Document: _read_doc, _write_doc, _create_doc, _search, _list_docs
    assert_eq!(tools.len(), 5);
    assert!(tools.iter().any(|t| t.name == "mock_docs_read_doc"));
    assert!(tools.iter().any(|t| t.name == "mock_docs_write_doc"));
    assert!(tools.iter().any(|t| t.name == "mock_docs_create_doc"));
    assert!(tools.iter().any(|t| t.name == "mock_docs_search"));
    assert!(tools.iter().any(|t| t.name == "mock_docs_list_docs"));
}

#[test]
fn test_registry_register_hybrid() {
    let mut registry = IntegrationRegistry::new();
    registry.register(Box::new(MockHybridIntegration));

    let tools = registry.all_tools();
    // Hybrid: _send, _read, _search (messaging) + _read_doc, _write_doc, _create_doc, _list_docs (document, no dup _search)
    assert_eq!(tools.len(), 7);
    assert!(tools.iter().any(|t| t.name == "mock_hybrid_send"));
    assert!(tools.iter().any(|t| t.name == "mock_hybrid_read"));
    assert!(tools.iter().any(|t| t.name == "mock_hybrid_search"));
    assert!(tools.iter().any(|t| t.name == "mock_hybrid_read_doc"));
    assert!(tools.iter().any(|t| t.name == "mock_hybrid_write_doc"));
    assert!(tools.iter().any(|t| t.name == "mock_hybrid_create_doc"));
    assert!(tools.iter().any(|t| t.name == "mock_hybrid_list_docs"));
}

#[test]
fn test_registry_multiple_integrations() {
    let mut registry = IntegrationRegistry::new();
    registry.register(Box::new(MockMessagingIntegration));
    registry.register(Box::new(MockDocIntegration));

    let mut ids = registry.list();
    ids.sort();
    assert_eq!(ids, vec!["mock_chat", "mock_docs"]);

    let tools = registry.all_tools();
    // mock_chat: 3 tools (_send, _read, _search), mock_docs: 5 tools
    assert_eq!(tools.len(), 8);
}

// ---------- Credential tests ----------

#[test]
fn test_credentials_all_integrations() {
    let mut creds = IntegrationCredentials::default();

    // Set all
    creds.set_token("slack", "xoxb-test").unwrap();
    creds
        .set_token("discord", "discord-token-12345678901234567890")
        .unwrap();
    creds.set_token("telegram", "123:ABC").unwrap();
    creds.set_token("notion", "secret_test").unwrap();

    assert!(creds.has_credentials("slack"));
    assert!(creds.has_credentials("discord"));
    assert!(creds.has_credentials("telegram"));
    assert!(creds.has_credentials("notion"));

    let mut configured = creds.configured_integrations();
    configured.sort();
    assert_eq!(configured, vec!["discord", "notion", "slack", "telegram"]);
}

#[test]
fn test_credentials_replace_token() {
    let mut creds = IntegrationCredentials::default();
    creds.set_token("slack", "xoxb-old").unwrap();
    assert!(creds.has_credentials("slack"));

    // Replace
    creds.set_token("slack", "xoxb-new").unwrap();
    assert!(creds.has_credentials("slack"));
    assert_eq!(creds.slack.as_ref().unwrap().bot_token, "xoxb-new");
}

#[test]
fn test_validate_discord_token() {
    // Too short
    assert!(validate_token_format("discord", "short").is_err());
    // Long enough
    assert!(validate_token_format("discord", "MTIzNDU2Nzg5MDEyMzQ1Njc4OQ.Gg1234.abcdef").is_ok());
}

#[test]
fn test_validate_unknown_integration_passthrough() {
    // Unknown integrations pass validation (no rules to check)
    assert!(validate_token_format("unknown_app", "any-token").is_ok());
}

// ---------- Executor tool dispatch tests ----------

#[tokio::test]
async fn test_executor_dispatches_integration_tools() {
    use futures::Stream;
    use openkoi::core::executor::Executor;
    use openkoi::core::types::ExecutionContext;
    use openkoi::provider::*;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// Provider that returns a tool call then a final response.
    struct IntegrationToolProvider {
        call_count: AtomicU32,
    }

    #[async_trait]
    impl ModelProvider for IntegrationToolProvider {
        fn id(&self) -> &str {
            "mock"
        }
        fn name(&self) -> &str {
            "Mock"
        }
        fn models(&self) -> Vec<ModelInfo> {
            vec![]
        }

        async fn chat(
            &self,
            _req: ChatRequest,
        ) -> Result<ChatResponse, openkoi::infra::errors::OpenKoiError> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                // First call: request integration tool
                Ok(ChatResponse {
                    content: "Sending message...".into(),
                    tool_calls: vec![ToolCall {
                        id: "call_int_1".into(),
                        name: "mock_chat_send".into(),
                        arguments: serde_json::json!({
                            "target": "#general",
                            "message": "Hello from agent"
                        }),
                    }],
                    usage: TokenUsage {
                        input_tokens: 50,
                        output_tokens: 20,
                        ..Default::default()
                    },
                    stop_reason: StopReason::ToolUse,
                })
            } else {
                Ok(ChatResponse {
                    content: "Message sent successfully.".into(),
                    tool_calls: vec![],
                    usage: TokenUsage {
                        input_tokens: 80,
                        output_tokens: 30,
                        ..Default::default()
                    },
                    stop_reason: StopReason::EndTurn,
                })
            }
        }

        async fn chat_stream(
            &self,
            _req: ChatRequest,
        ) -> Result<
            Pin<
                Box<
                    dyn Stream<Item = Result<ChatChunk, openkoi::infra::errors::OpenKoiError>>
                        + Send,
                >,
            >,
            openkoi::infra::errors::OpenKoiError,
        > {
            Err(openkoi::infra::errors::OpenKoiError::Provider {
                provider: "mock".into(),
                message: "not supported".into(),
                retriable: false,
            })
        }

        async fn embed(
            &self,
            _: &[&str],
        ) -> Result<Vec<Vec<f32>>, openkoi::infra::errors::OpenKoiError> {
            Ok(vec![])
        }
    }

    let provider: Arc<dyn ModelProvider> = Arc::new(IntegrationToolProvider {
        call_count: AtomicU32::new(0),
    });
    let executor = Executor::new(provider, "mock".into());

    let mut registry = IntegrationRegistry::new();
    registry.register(Box::new(MockMessagingIntegration));

    let context = ExecutionContext {
        system: "Test".into(),
        messages: vec![],
        token_estimate: 100,
    };

    let tools = registry.all_tools();

    let result = executor
        .execute(&context, &tools, None, Some(&registry))
        .await
        .unwrap();

    // Should complete with the final content
    assert!(result.content.contains("Message sent successfully"));
    assert!(result.tool_calls_made >= 1);
}

#[tokio::test]
async fn test_executor_unknown_tool_returns_error() {
    use futures::Stream;
    use openkoi::core::executor::Executor;
    use openkoi::core::types::ExecutionContext;
    use openkoi::provider::*;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// Provider that calls an unknown tool then finishes.
    struct UnknownToolProvider {
        call_count: AtomicU32,
    }

    #[async_trait]
    impl ModelProvider for UnknownToolProvider {
        fn id(&self) -> &str {
            "mock"
        }
        fn name(&self) -> &str {
            "Mock"
        }
        fn models(&self) -> Vec<ModelInfo> {
            vec![]
        }

        async fn chat(
            &self,
            _req: ChatRequest,
        ) -> Result<ChatResponse, openkoi::infra::errors::OpenKoiError> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            if count == 0 {
                Ok(ChatResponse {
                    content: "Calling unknown tool...".into(),
                    tool_calls: vec![ToolCall {
                        id: "call_unk".into(),
                        name: "nonexistent_tool".into(),
                        arguments: serde_json::json!({}),
                    }],
                    usage: TokenUsage {
                        input_tokens: 30,
                        output_tokens: 10,
                        ..Default::default()
                    },
                    stop_reason: StopReason::ToolUse,
                })
            } else {
                Ok(ChatResponse {
                    content: "Done.".into(),
                    tool_calls: vec![],
                    usage: TokenUsage {
                        input_tokens: 40,
                        output_tokens: 10,
                        ..Default::default()
                    },
                    stop_reason: StopReason::EndTurn,
                })
            }
        }

        async fn chat_stream(
            &self,
            _req: ChatRequest,
        ) -> Result<
            Pin<
                Box<
                    dyn Stream<Item = Result<ChatChunk, openkoi::infra::errors::OpenKoiError>>
                        + Send,
                >,
            >,
            openkoi::infra::errors::OpenKoiError,
        > {
            Err(openkoi::infra::errors::OpenKoiError::Provider {
                provider: "mock".into(),
                message: "not supported".into(),
                retriable: false,
            })
        }

        async fn embed(
            &self,
            _: &[&str],
        ) -> Result<Vec<Vec<f32>>, openkoi::infra::errors::OpenKoiError> {
            Ok(vec![])
        }
    }

    let provider: Arc<dyn ModelProvider> = Arc::new(UnknownToolProvider {
        call_count: AtomicU32::new(0),
    });
    let executor = Executor::new(provider, "mock".into());

    let context = ExecutionContext {
        system: "Test".into(),
        messages: vec![],
        token_estimate: 100,
    };

    // No tools, no registry
    let result = executor.execute(&context, &[], None, None).await.unwrap();
    // Should still complete (error is returned to model as tool_result)
    assert!(result.content.contains("Done"));
    assert!(result.tool_calls_made >= 1);
}
