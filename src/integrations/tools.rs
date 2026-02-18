// src/integrations/tools.rs — Auto-register integration tools for the agent

use serde_json::json;

use crate::integrations::types::Integration;
use crate::provider::ToolDef;

/// Generate tool definitions from a connected integration.
pub fn tools_for_integration(integration: &dyn Integration) -> Vec<ToolDef> {
    let mut tools = Vec::new();
    let id = integration.id();
    let name = integration.name();
    let has_messaging = integration.messaging().is_some();
    let has_document = integration.document().is_some();

    if has_messaging {
        tools.push(ToolDef {
            name: format!("{id}_send"),
            description: format!("Send a message via {name}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "target": { "type": "string", "description": "Channel or conversation ID" },
                    "message": { "type": "string", "description": "Message content to send" }
                },
                "required": ["target", "message"]
            }),
        });
        tools.push(ToolDef {
            name: format!("{id}_read"),
            description: format!("Read recent messages from {name}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "channel": { "type": "string", "description": "Channel or conversation ID" },
                    "limit": { "type": "integer", "description": "Number of messages to fetch (default 20)" }
                },
                "required": ["channel"]
            }),
        });
        // _search for messaging (also covers document search fallback in executor)
        tools.push(ToolDef {
            name: format!("{id}_search"),
            description: format!("Search messages in {name}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" }
                },
                "required": ["query"]
            }),
        });
    }

    if has_document {
        tools.push(ToolDef {
            name: format!("{id}_read_doc"),
            description: format!("Read a document from {name}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "doc_id": { "type": "string", "description": "Document ID" }
                },
                "required": ["doc_id"]
            }),
        });
        tools.push(ToolDef {
            name: format!("{id}_write_doc"),
            description: format!("Write/update a document in {name}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "doc_id": { "type": "string", "description": "Document ID to update" },
                    "content": { "type": "string", "description": "New content for the document" }
                },
                "required": ["doc_id", "content"]
            }),
        });
        tools.push(ToolDef {
            name: format!("{id}_create_doc"),
            description: format!("Create a new document in {name}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Document title" },
                    "content": { "type": "string", "description": "Initial content" }
                },
                "required": ["title"]
            }),
        });
        // Only add _search for document if messaging didn't already add it
        if !has_messaging {
            tools.push(ToolDef {
                name: format!("{id}_search"),
                description: format!("Search documents in {name}"),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Search query" }
                    },
                    "required": ["query"]
                }),
            });
        }
        tools.push(ToolDef {
            name: format!("{id}_list_docs"),
            description: format!("List documents in {name}"),
            parameters: json!({
                "type": "object",
                "properties": {
                    "folder": { "type": "string", "description": "Optional folder/database to list from" }
                }
            }),
        });
    }

    tools
}
