// src/integrations/slack.rs — Slack adapter (Web API + Socket Mode)
//
// Slack is a hybrid integration: messaging + document (canvas/files).
// This adapter uses the Slack Web API (https://api.slack.com/methods).

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::integrations::types::{
    Document, DocumentAdapter, DocumentRef, IncomingMessage, Integration, MessagingAdapter,
    RichMessage,
};

const SLACK_API_BASE: &str = "https://slack.com/api";

/// Slack integration adapter.
pub struct SlackAdapter {
    client: Client,
    bot_token: String,
}

impl SlackAdapter {
    pub fn new(bot_token: String) -> Self {
        Self {
            client: Client::new(),
            bot_token,
        }
    }

    /// Make an authenticated GET request to the Slack API.
    async fn api_get<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        let url = format!("{SLACK_API_BASE}/{method}");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.bot_token)
            .query(params)
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Slack API {method} returned {}", resp.status());
        }

        let body: T = resp.json().await?;
        Ok(body)
    }

    /// Make an authenticated POST request to the Slack API.
    async fn api_post<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<T> {
        let url = format!("{SLACK_API_BASE}/{method}");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.bot_token)
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Slack API {method} returned {}", resp.status());
        }

        let body: T = resp.json().await?;
        Ok(body)
    }

    /// Test authentication by calling auth.test.
    pub async fn validate(&self) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct AuthResp {
            ok: bool,
            user: Option<String>,
            team: Option<String>,
            error: Option<String>,
        }

        let resp: AuthResp = self.api_get("auth.test", &[]).await?;
        if !resp.ok {
            anyhow::bail!(
                "Slack auth failed: {}",
                resp.error.unwrap_or_else(|| "unknown".into())
            );
        }

        Ok(format!(
            "Authenticated as {} in {}",
            resp.user.unwrap_or_default(),
            resp.team.unwrap_or_default()
        ))
    }
}

// -- Slack API response types --

#[derive(Deserialize)]
struct SlackResponse {
    ok: bool,
    error: Option<String>,
}

#[derive(Deserialize)]
struct ConversationsHistoryResp {
    ok: bool,
    messages: Option<Vec<SlackMessage>>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SlackMessage {
    #[serde(rename = "type")]
    msg_type: Option<String>,
    user: Option<String>,
    text: Option<String>,
    ts: Option<String>,
}

#[derive(Deserialize)]
struct ChatPostMessageResp {
    ok: bool,
    ts: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct SearchResp {
    ok: bool,
    messages: Option<SearchMessages>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct SearchMessages {
    matches: Option<Vec<SearchMatch>>,
}

#[derive(Deserialize)]
struct SearchMatch {
    text: Option<String>,
    channel: Option<SearchChannel>,
    user: Option<String>,
    ts: Option<String>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SearchChannel {
    id: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct FilesListResp {
    ok: bool,
    files: Option<Vec<SlackFile>>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct SlackFile {
    id: String,
    name: Option<String>,
    title: Option<String>,
    url_private: Option<String>,
}

// -- MessagingAdapter implementation --

#[async_trait]
impl MessagingAdapter for SlackAdapter {
    async fn send(&self, target: &str, content: &str) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "channel": target,
            "text": content,
        });

        let resp: ChatPostMessageResp = self.api_post("chat.postMessage", &body).await?;
        if !resp.ok {
            anyhow::bail!(
                "Slack send failed: {}",
                resp.error.unwrap_or_else(|| "unknown".into())
            );
        }

        Ok(resp.ts.unwrap_or_default())
    }

    async fn send_rich(&self, target: &str, msg: &RichMessage) -> anyhow::Result<String> {
        let mut blocks = Vec::<serde_json::Value>::new();

        // Header block (title)
        if let Some(ref title) = msg.title {
            blocks.push(serde_json::json!({
                "type": "header",
                "text": { "type": "plain_text", "text": title }
            }));
        }

        // Fields as a section block
        if !msg.fields.is_empty() {
            let fields: Vec<serde_json::Value> = msg
                .fields
                .iter()
                .map(|(k, v)| {
                    serde_json::json!({
                        "type": "mrkdwn",
                        "text": format!("*{}:* {}", k, v)
                    })
                })
                .collect();
            blocks.push(serde_json::json!({
                "type": "section",
                "fields": fields
            }));
        }

        // Body text as a section block
        if !msg.text.is_empty() {
            blocks.push(serde_json::json!({
                "type": "section",
                "text": { "type": "mrkdwn", "text": msg.text }
            }));
        }

        let mut body = serde_json::json!({
            "channel": target,
            "text": msg.text,
            "blocks": blocks,
        });

        // Attachment with color sidebar (wraps blocks in an attachment for color)
        if let Some(ref color) = msg.color {
            body = serde_json::json!({
                "channel": target,
                "text": msg.text,
                "attachments": [{
                    "color": color,
                    "blocks": blocks,
                }],
            });
        }

        // Thread support
        if let Some(ref thread_ts) = msg.thread_id {
            body["thread_ts"] = serde_json::json!(thread_ts);
        }

        let resp: ChatPostMessageResp = self.api_post("chat.postMessage", &body).await?;
        if !resp.ok {
            anyhow::bail!(
                "Slack send_rich failed: {}",
                resp.error.unwrap_or_else(|| "unknown".into())
            );
        }

        Ok(resp.ts.unwrap_or_default())
    }

    async fn history(&self, channel: &str, limit: u32) -> anyhow::Result<Vec<IncomingMessage>> {
        let limit_str = limit.to_string();
        let resp: ConversationsHistoryResp = self
            .api_get(
                "conversations.history",
                &[("channel", channel), ("limit", &limit_str)],
            )
            .await?;

        if !resp.ok {
            anyhow::bail!(
                "Slack history failed: {}",
                resp.error.unwrap_or_else(|| "unknown".into())
            );
        }

        let messages = resp
            .messages
            .unwrap_or_default()
            .into_iter()
            .map(|m| IncomingMessage {
                id: m.ts.clone().unwrap_or_default(),
                channel: channel.to_string(),
                sender: m.user.unwrap_or_else(|| "unknown".into()),
                content: m.text.unwrap_or_default(),
                timestamp: m.ts.unwrap_or_default(),
                thread_id: None,
            })
            .collect();

        Ok(messages)
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<IncomingMessage>> {
        let resp: SearchResp = self
            .api_get("search.messages", &[("query", query), ("count", "20")])
            .await?;

        if !resp.ok {
            anyhow::bail!(
                "Slack search failed: {}",
                resp.error.unwrap_or_else(|| "unknown".into())
            );
        }

        let messages = resp
            .messages
            .and_then(|m| m.matches)
            .unwrap_or_default()
            .into_iter()
            .map(|m| {
                let ch = m.channel.as_ref();
                IncomingMessage {
                    id: m.ts.clone().unwrap_or_default(),
                    channel: ch
                        .and_then(|c| c.name.clone())
                        .unwrap_or_else(|| "unknown".into()),
                    sender: m.user.unwrap_or_else(|| "unknown".into()),
                    content: m.text.unwrap_or_default(),
                    timestamp: m.ts.unwrap_or_default(),
                    thread_id: None,
                }
            })
            .collect();

        Ok(messages)
    }
}

// -- DocumentAdapter for Slack (files/canvas) --

#[async_trait]
impl DocumentAdapter for SlackAdapter {
    async fn read(&self, doc_id: &str) -> anyhow::Result<Document> {
        // Slack files require files.info + authenticated download
        let resp: serde_json::Value = self.api_get("files.info", &[("file", doc_id)]).await?;

        let file = resp
            .get("file")
            .ok_or_else(|| anyhow::anyhow!("No file in response"))?;
        let title = file
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Untitled")
            .to_string();
        let url = file
            .get("url_private")
            .and_then(|u| u.as_str())
            .map(String::from);

        // Download file content
        let content = if let Some(ref file_url) = url {
            let dl_resp = self
                .client
                .get(file_url)
                .bearer_auth(&self.bot_token)
                .send()
                .await?;
            dl_resp.text().await.unwrap_or_default()
        } else {
            String::new()
        };

        Ok(Document {
            id: doc_id.to_string(),
            title,
            content,
            url,
        })
    }

    async fn write(&self, doc_id: &str, content: &str) -> anyhow::Result<()> {
        // Slack doesn't support updating file content directly via Web API.
        // Upload as a new file snippet in the same channel.
        let body = serde_json::json!({
            "channels": doc_id,
            "content": content,
            "filetype": "text",
            "title": "Updated content",
        });

        let resp: SlackResponse = self.api_post("files.upload", &body).await?;
        if !resp.ok {
            anyhow::bail!(
                "Slack file upload failed: {}",
                resp.error.unwrap_or_else(|| "unknown".into())
            );
        }
        Ok(())
    }

    async fn create(&self, title: &str, content: &str) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "content": content,
            "filetype": "text",
            "title": title,
        });

        let resp: serde_json::Value = self.api_post("files.upload", &body).await?;
        let file_id = resp
            .get("file")
            .and_then(|f| f.get("id"))
            .and_then(|id| id.as_str())
            .unwrap_or("")
            .to_string();

        Ok(file_id)
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<DocumentRef>> {
        let resp: FilesListResp = self.api_get("files.list", &[("query", query)]).await?;

        if !resp.ok {
            anyhow::bail!(
                "Slack files.list failed: {}",
                resp.error.unwrap_or_else(|| "unknown".into())
            );
        }

        let refs = resp
            .files
            .unwrap_or_default()
            .into_iter()
            .map(|f| DocumentRef {
                id: f.id,
                title: f.title.or(f.name).unwrap_or_default(),
                url: f.url_private,
            })
            .collect();

        Ok(refs)
    }

    async fn list(&self, _folder: Option<&str>) -> anyhow::Result<Vec<DocumentRef>> {
        // Slack doesn't have folders; list recent files
        let resp: FilesListResp = self.api_get("files.list", &[("count", "20")]).await?;

        if !resp.ok {
            anyhow::bail!(
                "Slack files.list failed: {}",
                resp.error.unwrap_or_else(|| "unknown".into())
            );
        }

        let refs = resp
            .files
            .unwrap_or_default()
            .into_iter()
            .map(|f| DocumentRef {
                id: f.id,
                title: f.title.or(f.name).unwrap_or_default(),
                url: f.url_private,
            })
            .collect();

        Ok(refs)
    }
}

// -- Integration trait --

impl Integration for SlackAdapter {
    fn id(&self) -> &str {
        "slack"
    }

    fn name(&self) -> &str {
        "Slack"
    }

    fn messaging(&self) -> Option<&dyn MessagingAdapter> {
        Some(self)
    }

    fn document(&self) -> Option<&dyn DocumentAdapter> {
        Some(self)
    }
}
