// src/integrations/notion.rs — Notion adapter (REST API)
//
// Uses the Notion API v2022-06-28 (https://developers.notion.com/reference).

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::integrations::types::{Document, DocumentAdapter, DocumentRef, Integration};

const NOTION_API_BASE: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";

/// Notion integration adapter.
pub struct NotionAdapter {
    client: Client,
    api_key: String,
}

impl NotionAdapter {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    /// Make an authenticated GET request to the Notion API.
    async fn api_get<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let url = format!("{NOTION_API_BASE}{path}");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .header("Notion-Version", NOTION_VERSION)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Notion API {path} returned {status}: {body}");
        }

        Ok(resp.json().await?)
    }

    /// Make an authenticated POST request to the Notion API.
    async fn api_post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<T> {
        let url = format!("{NOTION_API_BASE}{path}");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Notion-Version", NOTION_VERSION)
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Notion API {path} returned {status}: {text}");
        }

        Ok(resp.json().await?)
    }

    /// Make an authenticated PATCH request to the Notion API.
    async fn api_patch<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<T> {
        let url = format!("{NOTION_API_BASE}{path}");
        let resp = self
            .client
            .patch(&url)
            .bearer_auth(&self.api_key)
            .header("Notion-Version", NOTION_VERSION)
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Notion API {path} returned {status}: {text}");
        }

        Ok(resp.json().await?)
    }

    /// Validate the API key by listing the current user.
    pub async fn validate(&self) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct MeResp {
            #[serde(rename = "type")]
            user_type: Option<String>,
            name: Option<String>,
        }

        let resp: MeResp = self.api_get("/users/me").await?;
        Ok(format!(
            "Authenticated as {} ({})",
            resp.name.unwrap_or_default(),
            resp.user_type.unwrap_or_default()
        ))
    }

    /// Extract plain text from Notion blocks.
    fn blocks_to_text(blocks: &[NotionBlock]) -> String {
        blocks
            .iter()
            .filter_map(|b| {
                let rich_text = match &b.block_type {
                    BlockType::Paragraph(p) => Some(&p.rich_text),
                    BlockType::Heading1(h) => Some(&h.rich_text),
                    BlockType::Heading2(h) => Some(&h.rich_text),
                    BlockType::Heading3(h) => Some(&h.rich_text),
                    BlockType::BulletedListItem(l) => Some(&l.rich_text),
                    BlockType::NumberedListItem(l) => Some(&l.rich_text),
                    BlockType::Code(c) => Some(&c.rich_text),
                    BlockType::Unknown => None,
                };

                rich_text.map(|rt| {
                    rt.iter()
                        .map(|t| t.plain_text.as_str())
                        .collect::<Vec<_>>()
                        .join("")
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Convert plain text to Notion paragraph blocks.
    fn text_to_blocks(content: &str) -> Vec<serde_json::Value> {
        content
            .lines()
            .map(|line| {
                serde_json::json!({
                    "object": "block",
                    "type": "paragraph",
                    "paragraph": {
                        "rich_text": [{
                            "type": "text",
                            "text": { "content": line }
                        }]
                    }
                })
            })
            .collect()
    }
}

// -- Notion API types --

#[derive(Deserialize)]
struct BlocksResp {
    results: Vec<NotionBlock>,
}

#[derive(Deserialize)]
struct NotionBlock {
    #[serde(flatten)]
    block_type: BlockType,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum BlockType {
    #[serde(rename = "paragraph")]
    Paragraph(TextBlock),
    #[serde(rename = "heading_1")]
    Heading1(TextBlock),
    #[serde(rename = "heading_2")]
    Heading2(TextBlock),
    #[serde(rename = "heading_3")]
    Heading3(TextBlock),
    #[serde(rename = "bulleted_list_item")]
    BulletedListItem(TextBlock),
    #[serde(rename = "numbered_list_item")]
    NumberedListItem(TextBlock),
    #[serde(rename = "code")]
    Code(TextBlock),
    #[serde(other)]
    Unknown,
}

#[derive(Deserialize)]
struct TextBlock {
    rich_text: Vec<RichText>,
}

#[derive(Deserialize)]
struct RichText {
    plain_text: String,
}

#[derive(Deserialize)]
struct SearchResp {
    results: Vec<SearchResult>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SearchResult {
    id: String,
    #[serde(rename = "type")]
    object_type: Option<String>,
    properties: Option<serde_json::Value>,
    url: Option<String>,
}

#[derive(Deserialize)]
struct PageResp {
    id: String,
    properties: Option<serde_json::Value>,
    url: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct DatabaseQueryResp {
    results: Vec<SearchResult>,
}

// -- DocumentAdapter implementation --

#[async_trait]
impl DocumentAdapter for NotionAdapter {
    async fn read(&self, doc_id: &str) -> anyhow::Result<Document> {
        // Get page metadata
        let page: PageResp = self.api_get(&format!("/pages/{doc_id}")).await?;

        // Get page content (blocks)
        let blocks: BlocksResp = self
            .api_get(&format!("/blocks/{doc_id}/children?page_size=100"))
            .await?;

        let title = extract_page_title(&page.properties);
        let content = Self::blocks_to_text(&blocks.results);

        Ok(Document {
            id: page.id,
            title,
            content,
            url: page.url,
        })
    }

    async fn write(&self, doc_id: &str, content: &str) -> anyhow::Result<()> {
        // Notion doesn't support replacing all blocks atomically.
        // Strategy: append new blocks (existing content is preserved).
        let blocks = Self::text_to_blocks(content);
        let body = serde_json::json!({
            "children": blocks,
        });

        let _: serde_json::Value = self
            .api_patch(&format!("/blocks/{doc_id}/children"), &body)
            .await?;

        Ok(())
    }

    async fn create(&self, title: &str, content: &str) -> anyhow::Result<String> {
        let blocks = Self::text_to_blocks(content);

        // Create a page in the user's workspace (requires a parent).
        // Without a specific parent, we'll search for a workspace-level page.
        let body = serde_json::json!({
            "parent": { "type": "workspace", "workspace": true },
            "properties": {
                "title": [{
                    "type": "text",
                    "text": { "content": title }
                }]
            },
            "children": blocks,
        });

        let resp: PageResp = self.api_post("/pages", &body).await?;
        Ok(resp.id)
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<DocumentRef>> {
        let body = serde_json::json!({
            "query": query,
            "page_size": 20,
        });

        let resp: SearchResp = self.api_post("/search", &body).await?;

        let refs = resp
            .results
            .into_iter()
            .map(|r| DocumentRef {
                id: r.id,
                title: extract_page_title(&r.properties),
                url: r.url,
            })
            .collect();

        Ok(refs)
    }

    async fn list(&self, _folder: Option<&str>) -> anyhow::Result<Vec<DocumentRef>> {
        // List recent pages via search with empty query
        let body = serde_json::json!({
            "page_size": 20,
            "filter": { "property": "object", "value": "page" },
        });

        let resp: SearchResp = self.api_post("/search", &body).await?;

        let refs = resp
            .results
            .into_iter()
            .map(|r| DocumentRef {
                id: r.id,
                title: extract_page_title(&r.properties),
                url: r.url,
            })
            .collect();

        Ok(refs)
    }
}

/// Extract page title from Notion properties.
fn extract_page_title(properties: &Option<serde_json::Value>) -> String {
    properties
        .as_ref()
        .and_then(|p| p.get("title").or_else(|| p.get("Name")))
        .and_then(|t| {
            // Try Notion title format: {"title": [{"plain_text": "..."}]}
            t.get("title")
                .or(Some(t))
                .and_then(|arr| arr.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| item.get("plain_text"))
                .and_then(|pt| pt.as_str())
        })
        .unwrap_or("Untitled")
        .to_string()
}

// -- Integration trait --

impl Integration for NotionAdapter {
    fn id(&self) -> &str {
        "notion"
    }

    fn name(&self) -> &str {
        "Notion"
    }

    fn messaging(&self) -> Option<&dyn crate::integrations::types::MessagingAdapter> {
        None
    }

    fn document(&self) -> Option<&dyn DocumentAdapter> {
        Some(self)
    }
}
