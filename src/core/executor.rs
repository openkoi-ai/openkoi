// src/core/executor.rs — Task execution with MCP + integration tool dispatch

use std::sync::Arc;

use super::types::*;
use crate::integrations::registry::IntegrationRegistry;
use crate::plugins::mcp::McpManager;
use crate::provider::{ChatRequest, Message, ModelProvider, StopReason, ToolDef};

/// Maximum number of tool-call round-trips per execution to prevent infinite loops.
const MAX_TOOL_ROUNDS: usize = 20;

/// Known integration tool suffixes for dispatch routing.
const INTEGRATION_TOOL_SUFFIXES: &[&str] = &[
    "_send",
    "_read",
    "_read_doc",
    "_write_doc",
    "_search",
    "_list_docs",
    "_create_doc",
];

/// Executes tasks by sending them to the model provider.
/// When the model returns tool calls, they are dispatched to MCP servers
/// or integration adapters, and results are fed back in a loop.
pub struct Executor {
    provider: Arc<dyn ModelProvider>,
    model_id: String,
}

impl Executor {
    pub fn new(provider: Arc<dyn ModelProvider>, model_id: String) -> Self {
        Self { provider, model_id }
    }

    /// Execute a task given the prepared context.
    ///
    /// Tool calls are dispatched to:
    /// 1. Integration adapters (for tools like `slack_send`, `notion_read_doc`)
    /// 2. MCP servers (for tools namespaced as `server__tool`)
    pub async fn execute(
        &self,
        context: &ExecutionContext,
        tools: &[ToolDef],
        mcp: Option<&mut McpManager>,
        integrations: Option<&IntegrationRegistry>,
    ) -> anyhow::Result<ExecutionOutput> {
        // On iteration 0 there are no conversation messages, so we send a
        // single user message prompting the model to begin.
        let mut messages = if context.messages.is_empty() {
            vec![Message::user("Begin.")]
        } else {
            context.messages.clone()
        };

        let mut total_tool_calls: u32 = 0;
        let mut accumulated_content = String::new();
        let mut total_usage = crate::provider::TokenUsage::default();
        let files_modified = Vec::new();

        // We need to reborrow mcp across loop iterations
        let mut mcp = mcp;

        for _round in 0..MAX_TOOL_ROUNDS {
            let request = ChatRequest {
                model: self.model_id.clone(),
                messages: messages.clone(),
                tools: tools.to_vec(),
                max_tokens: Some(4096),
                temperature: Some(0.7),
                system: Some(context.system.clone()),
            };

            let response = self
                .provider
                .chat(request)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            // Accumulate usage
            total_usage.input_tokens += response.usage.input_tokens;
            total_usage.output_tokens += response.usage.output_tokens;
            total_usage.cache_read_tokens += response.usage.cache_read_tokens;
            total_usage.cache_write_tokens += response.usage.cache_write_tokens;

            // If no tool calls, we're done — collect the final content
            if response.tool_calls.is_empty() {
                accumulated_content.push_str(&response.content);
                break;
            }

            // Model made tool calls — dispatch them
            total_tool_calls += response.tool_calls.len() as u32;

            // Add the assistant's response (with tool calls) to conversation
            if !response.content.is_empty() {
                accumulated_content.push_str(&response.content);
                accumulated_content.push('\n');
            }
            messages.push(Message::assistant(&response.content));

            // Dispatch each tool call
            for tc in &response.tool_calls {
                let result = dispatch_tool_call(tc, &mut mcp, integrations).await;
                messages.push(Message::tool_result(&tc.id, &result));
            }

            // If the model said it's done (EndTurn) even with tool calls, break
            if matches!(response.stop_reason, StopReason::EndTurn) {
                break;
            }
        }

        Ok(ExecutionOutput {
            content: accumulated_content,
            usage: total_usage,
            tool_calls_made: total_tool_calls,
            files_modified,
        })
    }
}

/// Dispatch a single tool call to the appropriate handler.
///
/// Routing logic:
/// 1. If the tool name matches an integration pattern (e.g., `slack_send`),
///    dispatch to the integration registry.
/// 2. If the tool name contains `__` (e.g., `server__tool`), dispatch to MCP.
/// 3. Otherwise, return an error.
async fn dispatch_tool_call(
    tc: &crate::provider::ToolCall,
    mcp: &mut Option<&mut McpManager>,
    integrations: Option<&IntegrationRegistry>,
) -> String {
    // Check if this is an integration tool
    if let Some(result) = try_dispatch_integration(tc, integrations).await {
        return result;
    }

    // Check if this is an MCP tool (namespaced with __)
    if tc.name.contains("__") {
        return dispatch_mcp_tool(tc, mcp).await;
    }

    // Unknown tool
    format!(
        "Error: Tool '{}' is not recognized. Expected an integration tool (e.g., slack_send) or MCP tool (e.g., server__tool).",
        tc.name
    )
}

/// Try to dispatch a tool call to an integration adapter.
/// Returns Some(result) if the tool matches an integration, None otherwise.
async fn try_dispatch_integration(
    tc: &crate::provider::ToolCall,
    integrations: Option<&IntegrationRegistry>,
) -> Option<String> {
    let registry = integrations?;

    // Parse the tool name: "{integration_id}_{action}"
    // We try to find the longest matching integration ID prefix.
    let tool_name = &tc.name;

    // Check known integration tool suffixes
    let suffix = INTEGRATION_TOOL_SUFFIXES
        .iter()
        .find(|s| tool_name.ends_with(*s))?;

    let integration_id = &tool_name[..tool_name.len() - suffix.len()];
    let integration = registry.get(integration_id)?;

    let result = match *suffix {
        "_send" => {
            let target = tc
                .arguments
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let message = tc
                .arguments
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if let Some(msg_adapter) = integration.messaging() {
                match msg_adapter.send(target, message).await {
                    Ok(id) => format!("Message sent successfully (id: {id})"),
                    Err(e) => format!("Error sending message: {e}"),
                }
            } else {
                format!("Error: {integration_id} does not support messaging")
            }
        }
        "_read" => {
            let channel = tc
                .arguments
                .get("channel")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = tc
                .arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20) as u32;
            if let Some(msg_adapter) = integration.messaging() {
                match msg_adapter.history(channel, limit).await {
                    Ok(messages) => {
                        let formatted: Vec<String> = messages
                            .iter()
                            .map(|m| format!("[{}] {}: {}", m.timestamp, m.sender, m.content))
                            .collect();
                        if formatted.is_empty() {
                            "No messages found.".to_string()
                        } else {
                            formatted.join("\n")
                        }
                    }
                    Err(e) => format!("Error reading messages: {e}"),
                }
            } else {
                format!("Error: {integration_id} does not support messaging")
            }
        }
        "_read_doc" => {
            let doc_id = tc
                .arguments
                .get("doc_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if let Some(doc_adapter) = integration.document() {
                match doc_adapter.read(doc_id).await {
                    Ok(doc) => {
                        format!("# {}\n\n{}", doc.title, doc.content)
                    }
                    Err(e) => format!("Error reading document: {e}"),
                }
            } else {
                format!("Error: {integration_id} does not support documents")
            }
        }
        "_write_doc" => {
            let doc_id = tc
                .arguments
                .get("doc_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = tc
                .arguments
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if let Some(doc_adapter) = integration.document() {
                match doc_adapter.write(doc_id, content).await {
                    Ok(()) => "Document updated successfully.".to_string(),
                    Err(e) => format!("Error writing document: {e}"),
                }
            } else {
                format!("Error: {integration_id} does not support documents")
            }
        }
        "_search" => {
            let query = tc
                .arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // Try messaging search first, then document search
            if let Some(msg_adapter) = integration.messaging() {
                match msg_adapter.search(query).await {
                    Ok(messages) => {
                        let formatted: Vec<String> = messages
                            .iter()
                            .map(|m| format!("[{}] {}: {}", m.channel, m.sender, m.content))
                            .collect();
                        if formatted.is_empty() {
                            "No messages found.".to_string()
                        } else {
                            formatted.join("\n")
                        }
                    }
                    Err(e) => format!("Search error: {e}"),
                }
            } else if let Some(doc_adapter) = integration.document() {
                match doc_adapter.search(query).await {
                    Ok(refs) => {
                        let formatted: Vec<String> = refs
                            .iter()
                            .map(|r| {
                                format!(
                                    "- {} (id: {}{})",
                                    r.title,
                                    r.id,
                                    r.url
                                        .as_ref()
                                        .map(|u| format!(", url: {u}"))
                                        .unwrap_or_default()
                                )
                            })
                            .collect();
                        if formatted.is_empty() {
                            "No documents found.".to_string()
                        } else {
                            formatted.join("\n")
                        }
                    }
                    Err(e) => format!("Search error: {e}"),
                }
            } else {
                format!("Error: {integration_id} does not support search")
            }
        }
        "_list_docs" => {
            let folder = tc.arguments.get("folder").and_then(|v| v.as_str());
            if let Some(doc_adapter) = integration.document() {
                match doc_adapter.list(folder).await {
                    Ok(refs) => {
                        let formatted: Vec<String> = refs
                            .iter()
                            .map(|r| format!("- {} (id: {})", r.title, r.id))
                            .collect();
                        if formatted.is_empty() {
                            "No documents found.".to_string()
                        } else {
                            formatted.join("\n")
                        }
                    }
                    Err(e) => format!("Error listing documents: {e}"),
                }
            } else {
                format!("Error: {integration_id} does not support documents")
            }
        }
        "_create_doc" => {
            let title = tc
                .arguments
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");
            let content = tc
                .arguments
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if let Some(doc_adapter) = integration.document() {
                match doc_adapter.create(title, content).await {
                    Ok(id) => format!("Document created successfully (id: {id})"),
                    Err(e) => format!("Error creating document: {e}"),
                }
            } else {
                format!("Error: {integration_id} does not support documents")
            }
        }
        _ => {
            format!("Error: Unknown integration action '{suffix}' for {integration_id}")
        }
    };

    Some(result)
}

/// Dispatch a tool call to an MCP server.
async fn dispatch_mcp_tool(
    tc: &crate::provider::ToolCall,
    mcp: &mut Option<&mut McpManager>,
) -> String {
    let mcp = match mcp {
        Some(ref mut m) => m,
        None => {
            return format!(
                "Error: Tool '{}' was called but no MCP manager is available.",
                tc.name
            );
        }
    };

    // Parse namespaced tool name: "server__tool"
    let (server, tool) = match tc.name.split_once("__") {
        Some((s, t)) => (s, t),
        None => {
            return format!(
                "Error: Tool '{}' is not namespaced (expected 'server__tool').",
                tc.name
            );
        }
    };

    match mcp.call(server, tool, tc.arguments.clone()).await {
        Ok(result) => {
            // MCP returns a JSON Value; convert to string for the model
            match result.as_str() {
                Some(s) => s.to_string(),
                None => serde_json::to_string_pretty(&result).unwrap_or_default(),
            }
        }
        Err(e) => {
            tracing::warn!("MCP tool call {}/{} failed: {}", server, tool, e);
            format!("Error calling tool '{}': {}", tc.name, e)
        }
    }
}
