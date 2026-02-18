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
