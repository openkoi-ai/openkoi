// src/integrations/types.rs — Integration adapter traits

use async_trait::async_trait;
use serde::Serialize;

/// An incoming message from a messaging integration.
#[derive(Debug, Clone, Serialize)]
pub struct IncomingMessage {
    pub id: String,
    pub channel: String,
    pub sender: String,
    pub content: String,
    pub timestamp: String,
    /// Thread identifier (Slack thread_ts, Telegram reply_to_message_id, etc.)
    pub thread_id: Option<String>,
}

/// A rich message for structured delivery (task results, progress updates).
#[derive(Debug, Clone, Serialize)]
pub struct RichMessage {
    /// Plain-text fallback (always required)
    pub text: String,
    /// Title / header line
    pub title: Option<String>,
    /// Key-value fields to display
    pub fields: Vec<(String, String)>,
    /// Optional color (hex string like "#36a64f")
    pub color: Option<String>,
    /// If set, reply in the given thread instead of creating a new message
    pub thread_id: Option<String>,
}

impl RichMessage {
    /// Create a minimal rich message with just text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            title: None,
            fields: Vec::new(),
            color: None,
            thread_id: None,
        }
    }

    /// Builder: set the title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Builder: add a field.
    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push((key.into(), value.into()));
        self
    }

    /// Builder: set the color.
    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Builder: set the thread ID for threaded replies.
    pub fn in_thread(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = Some(thread_id.into());
        self
    }
}

/// A document reference from a document integration.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentRef {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
}

/// A full document.
#[derive(Debug, Clone, Serialize)]
pub struct Document {
    pub id: String,
    pub title: String,
    pub content: String,
    pub url: Option<String>,
}

/// Adapter for messaging apps (Slack, Telegram, iMessage, Discord).
#[async_trait]
pub trait MessagingAdapter: Send + Sync {
    async fn send(&self, target: &str, content: &str) -> anyhow::Result<String>;

    /// Send a rich/structured message. Falls back to plain `send()` by default.
    async fn send_rich(&self, target: &str, msg: &RichMessage) -> anyhow::Result<String> {
        self.send(target, &msg.text).await
    }

    async fn history(&self, channel: &str, limit: u32) -> anyhow::Result<Vec<IncomingMessage>>;
    async fn search(&self, query: &str) -> anyhow::Result<Vec<IncomingMessage>>;
}

/// Adapter for document apps (Notion, Google Docs, MS Office).
#[async_trait]
pub trait DocumentAdapter: Send + Sync {
    async fn read(&self, doc_id: &str) -> anyhow::Result<Document>;
    async fn write(&self, doc_id: &str, content: &str) -> anyhow::Result<()>;
    async fn create(&self, title: &str, content: &str) -> anyhow::Result<String>;
    async fn search(&self, query: &str) -> anyhow::Result<Vec<DocumentRef>>;
    async fn list(&self, folder: Option<&str>) -> anyhow::Result<Vec<DocumentRef>>;
}

#[async_trait]
pub trait Integration: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn messaging(&self) -> Option<&dyn MessagingAdapter>;
    fn document(&self) -> Option<&dyn DocumentAdapter>;

    /// Dispatch a generic tool call to this integration.
    async fn call(&self, action: &str, args: serde_json::Value) -> anyhow::Result<String> {
        match action {
            "send" => {
                let messaging = self
                    .messaging()
                    .ok_or_else(|| anyhow::anyhow!("Messaging not supported"))?;
                let target = args["target"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing target"))?;
                let message = args["message"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing message"))?;
                messaging.send(target, message).await
            }
            "read" => {
                let messaging = self
                    .messaging()
                    .ok_or_else(|| anyhow::anyhow!("Messaging not supported"))?;
                let channel = args["channel"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing channel"))?;
                let limit = args["limit"].as_u64().unwrap_or(20) as u32;
                let msgs = messaging.history(channel, limit).await?;
                Ok(serde_json::to_string_pretty(&msgs)?)
            }
            "search" => {
                let query = args["query"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing query"))?;
                if let Some(m) = self.messaging() {
                    let msgs = m.search(query).await?;
                    if !msgs.is_empty() {
                        return Ok(serde_json::to_string_pretty(&msgs)?);
                    }
                }
                if let Some(d) = self.document() {
                    let docs = d.search(query).await?;
                    return Ok(serde_json::to_string_pretty(&docs)?);
                }
                anyhow::bail!("Search not supported or no results found")
            }
            "read_doc" => {
                let document = self
                    .document()
                    .ok_or_else(|| anyhow::anyhow!("Document storage not supported"))?;
                let doc_id = args["doc_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing doc_id"))?;
                let doc = document.read(doc_id).await?;
                Ok(doc.content)
            }
            "write_doc" => {
                let document = self
                    .document()
                    .ok_or_else(|| anyhow::anyhow!("Document storage not supported"))?;
                let doc_id = args["doc_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing doc_id"))?;
                let content = args["content"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing content"))?;
                document.write(doc_id, content).await?;
                Ok("Document updated".into())
            }
            "create_doc" => {
                let document = self
                    .document()
                    .ok_or_else(|| anyhow::anyhow!("Document storage not supported"))?;
                let title = args["title"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing title"))?;
                let content = args["content"].as_str().unwrap_or_default();
                let doc_id = document.create(title, content).await?;
                Ok(format!("Document created with ID: {doc_id}"))
            }
            "list_docs" => {
                let document = self
                    .document()
                    .ok_or_else(|| anyhow::anyhow!("Document storage not supported"))?;
                let folder = args["folder"].as_str();
                let docs = document.list(folder).await?;
                Ok(serde_json::to_string_pretty(&docs)?)
            }
            _ => anyhow::bail!("Unknown action: {action}"),
        }
    }
}
