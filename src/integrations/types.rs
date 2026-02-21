// src/integrations/types.rs — Integration adapter traits

use async_trait::async_trait;

/// An incoming message from a messaging integration.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct DocumentRef {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
}

/// A full document.
#[derive(Debug, Clone)]
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

/// An integration that can provide messaging, document, or both adapters.
pub trait Integration: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn messaging(&self) -> Option<&dyn MessagingAdapter>;
    fn document(&self) -> Option<&dyn DocumentAdapter>;
}
