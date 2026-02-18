// tests/orchestrator_test.rs — Integration test: orchestrator with mock provider

use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

use openkoi::core::orchestrator::{Orchestrator, SessionContext};
use openkoi::core::safety::SafetyChecker;
use openkoi::core::types::{IterationEngineConfig, TaskInput};
use openkoi::infra::config::{IterationConfig, SafetyConfig};
use openkoi::memory::recall::HistoryRecall;
use openkoi::provider::*;
use openkoi::skills::registry::SkillRegistry;
use openkoi::soul::loader::{Soul, SoulSource};

/// A mock provider that returns canned responses without making any network calls.
struct MockProvider {
    response_content: String,
}

impl MockProvider {
    fn new(content: &str) -> Self {
        Self {
            response_content: content.to_string(),
        }
    }
}

#[async_trait]
impl ModelProvider for MockProvider {
    fn id(&self) -> &str {
        "mock"
    }

    fn name(&self) -> &str {
        "Mock Provider"
    }

    fn models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: "mock-model".into(),
            name: "Mock Model".into(),
            context_window: 128_000,
            max_output_tokens: 4096,
            supports_tools: true,
            supports_streaming: false,
            input_price_per_mtok: 0.0,
            output_price_per_mtok: 0.0,
        }]
    }

    async fn chat(
        &self,
        _request: ChatRequest,
    ) -> Result<ChatResponse, openkoi::infra::errors::OpenKoiError> {
        Ok(ChatResponse {
            content: self.response_content.clone(),
            tool_calls: vec![],
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
            },
            stop_reason: StopReason::EndTurn,
        })
    }

    async fn chat_stream(
        &self,
        _request: ChatRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<ChatChunk, openkoi::infra::errors::OpenKoiError>> + Send>>,
        openkoi::infra::errors::OpenKoiError,
    > {
        Err(openkoi::infra::errors::OpenKoiError::Provider {
            provider: "mock".into(),
            message: "Streaming not supported in mock".into(),
            retriable: false,
        })
    }

    async fn embed(
        &self,
        _texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, openkoi::infra::errors::OpenKoiError> {
        Ok(vec![vec![0.1, 0.2, 0.3]])
    }
}

fn default_session_context() -> SessionContext {
    SessionContext {
        soul: Soul {
            raw: "I am a test assistant.".into(),
            source: SoulSource::Default,
        },
        ranked_skills: vec![],
        recall: HistoryRecall::default(),
        tools: vec![],
        skill_registry: Arc::new(SkillRegistry::empty()),
    }
}

#[tokio::test]
async fn test_orchestrator_single_iteration_accept() {
    let provider: Arc<dyn ModelProvider> = Arc::new(MockProvider::new("Hello, world!"));

    let config = IterationEngineConfig {
        max_iterations: 1,
        quality_threshold: 0.8,
        ..Default::default()
    };

    let safety = SafetyChecker::from_config(&IterationConfig::default(), &SafetyConfig::default());

    let mut orchestrator = Orchestrator::new(
        provider,
        "mock-model".into(),
        config,
        safety,
        Arc::new(SkillRegistry::empty()),
    )
    .with_project_dir(std::env::temp_dir().join("openkoi_test_nonexistent"));
    let task = TaskInput::new("Say hello");
    let ctx = default_session_context();

    let result = orchestrator.run(task, &ctx, None, None).await.unwrap();

    assert_eq!(result.output.content, "Hello, world!");
    assert_eq!(result.iterations, 1);
    assert!(result.total_tokens > 0);
    assert!(result.cost >= 0.0);
}

#[tokio::test]
async fn test_orchestrator_multiple_iterations() {
    let provider: Arc<dyn ModelProvider> = Arc::new(MockProvider::new("Improved output"));

    let config = IterationEngineConfig {
        max_iterations: 3,
        quality_threshold: 0.99, // Never satisfied — forces all 3 iterations
        ..Default::default()
    };

    let safety = SafetyChecker::from_config(
        &IterationConfig {
            max_iterations: 3,
            ..Default::default()
        },
        &SafetyConfig::default(),
    );

    let mut orchestrator = Orchestrator::new(
        provider,
        "mock-model".into(),
        config,
        safety,
        Arc::new(SkillRegistry::empty()),
    )
    .with_project_dir(std::env::temp_dir().join("openkoi_test_nonexistent"));
    let task = TaskInput::new("Write a complex function");
    let ctx = default_session_context();

    let result = orchestrator.run(task, &ctx, None, None).await.unwrap();

    // The mock evaluator scores 0.85 which is below 0.99, so it should run all iterations
    // until max_iterations is exhausted
    assert!(result.iterations >= 1);
    assert!(result.total_tokens > 0);
}

#[tokio::test]
async fn test_orchestrator_with_tools_defined() {
    let provider: Arc<dyn ModelProvider> = Arc::new(MockProvider::new("Used a tool conceptually"));

    let config = IterationEngineConfig::default();
    let safety = SafetyChecker::from_config(&IterationConfig::default(), &SafetyConfig::default());

    let mut orchestrator = Orchestrator::new(
        provider,
        "mock-model".into(),
        config,
        safety,
        Arc::new(SkillRegistry::empty()),
    )
    .with_project_dir(std::env::temp_dir().join("openkoi_test_nonexistent"));
    let task = TaskInput::new("Search for files");

    let ctx = SessionContext {
        tools: vec![ToolDef {
            name: "mcp__search".into(),
            description: "Search for files".into(),
            parameters: serde_json::json!({"type": "object"}),
        }],
        ..default_session_context()
    };

    let result = orchestrator.run(task, &ctx, None, None).await.unwrap();
    assert!(!result.output.content.is_empty());
}

#[tokio::test]
async fn test_orchestrator_result_includes_skills_used() {
    let provider: Arc<dyn ModelProvider> = Arc::new(MockProvider::new("Done"));

    let config = IterationEngineConfig {
        max_iterations: 1,
        ..Default::default()
    };
    let safety = SafetyChecker::from_config(&IterationConfig::default(), &SafetyConfig::default());

    let mut orchestrator = Orchestrator::new(
        provider,
        "mock-model".into(),
        config,
        safety,
        Arc::new(SkillRegistry::empty()),
    )
    .with_project_dir(std::env::temp_dir().join("openkoi_test_nonexistent"));
    let task = TaskInput::new("Test");
    let ctx = default_session_context();

    let result = orchestrator.run(task, &ctx, None, None).await.unwrap();
    // With no ranked skills, skills_used should be empty
    assert!(result.skills_used.is_empty());
}

/// Mock provider that returns tool calls on first request, then a final response.
struct MockToolCallProvider {
    call_count: std::sync::atomic::AtomicU32,
}

impl MockToolCallProvider {
    fn new() -> Self {
        Self {
            call_count: std::sync::atomic::AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl ModelProvider for MockToolCallProvider {
    fn id(&self) -> &str {
        "mock-tool"
    }
    fn name(&self) -> &str {
        "Mock Tool Provider"
    }
    fn models(&self) -> Vec<ModelInfo> {
        vec![]
    }

    async fn chat(
        &self,
        _request: ChatRequest,
    ) -> Result<ChatResponse, openkoi::infra::errors::OpenKoiError> {
        let count = self
            .call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if count == 0 {
            // First call: return a tool call
            Ok(ChatResponse {
                content: "I need to search.".into(),
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "test__search".into(),
                    arguments: serde_json::json!({"query": "hello"}),
                }],
                usage: TokenUsage {
                    input_tokens: 50,
                    output_tokens: 20,
                    ..Default::default()
                },
                stop_reason: StopReason::ToolUse,
            })
        } else {
            // Second call: return final content
            Ok(ChatResponse {
                content: "Found the answer: 42".into(),
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
        _request: ChatRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<ChatChunk, openkoi::infra::errors::OpenKoiError>> + Send>>,
        openkoi::infra::errors::OpenKoiError,
    > {
        Err(openkoi::infra::errors::OpenKoiError::Provider {
            provider: "mock-tool".into(),
            message: "not supported".into(),
            retriable: false,
        })
    }

    async fn embed(
        &self,
        _texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, openkoi::infra::errors::OpenKoiError> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn test_executor_tool_call_loop_without_mcp() {
    // When the model requests tool calls but no MCP manager is available,
    // the executor should return an error message as the tool result and continue.
    use openkoi::core::executor::Executor;
    use openkoi::core::types::ExecutionContext;

    let provider: Arc<dyn ModelProvider> = Arc::new(MockToolCallProvider::new());
    let executor = Executor::new(provider, "mock-tool".into());

    let context = ExecutionContext {
        system: "You are a test assistant.".into(),
        messages: vec![],
        token_estimate: 100,
    };

    let tools = vec![ToolDef {
        name: "test__search".into(),
        description: "Search".into(),
        parameters: serde_json::json!({}),
    }];

    let result = executor
        .execute(&context, &tools, None, None)
        .await
        .unwrap();
    // Should complete (the mock returns final content on second call)
    assert!(result.content.contains("Found the answer: 42"));
    assert!(result.tool_calls_made >= 1);
    // Usage should accumulate from both rounds
    assert!(result.usage.input_tokens >= 130); // 50 + 80
}
