// src/integrations/discord.rs — Discord adapter (Bot API)
//
// Uses the Discord REST API (https://discord.com/developers/docs/reference).

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::integrations::types::{IncomingMessage, Integration, MessagingAdapter, RichMessage};

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";

/// Discord integration adapter.
pub struct DiscordAdapter {
    client: Client,
    bot_token: String,
}

impl DiscordAdapter {
    pub fn new(bot_token: String) -> Self {
        Self {
            client: Client::new(),
            bot_token,
        }
    }

    /// Make an authenticated GET request to the Discord API.
    async fn api_get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        let url = format!("{DISCORD_API_BASE}{path}");
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .query(params)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Discord API {path} returned {status}: {body}");
        }

        let body: T = resp.json().await?;
        Ok(body)
    }

    /// Make an authenticated POST request to the Discord API.
    async fn api_post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<T> {
        let url = format!("{DISCORD_API_BASE}{path}");
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bot {}", self.bot_token))
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Discord API {path} returned {status}: {text}");
        }

        let body: T = resp.json().await?;
        Ok(body)
    }

    /// Validate the bot token by fetching the current user.
    pub async fn validate(&self) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct User {
            username: Option<String>,
            discriminator: Option<String>,
        }

        let user: User = self.api_get("/users/@me", &[]).await?;
        Ok(format!(
            "Authenticated as {}#{}",
            user.username.unwrap_or_default(),
            user.discriminator.unwrap_or_default()
        ))
    }
}

// -- Discord API response types --

#[derive(Deserialize)]
struct DiscordMessage {
    id: String,
    channel_id: String,
    author: DiscordAuthor,
    content: String,
    timestamp: String,
}

#[derive(Deserialize)]
struct DiscordAuthor {
    username: String,
}

#[derive(Deserialize)]
struct CreateMessageResp {
    id: String,
}

// -- MessagingAdapter implementation --

#[async_trait]
impl MessagingAdapter for DiscordAdapter {
    async fn send(&self, target: &str, content: &str) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "content": content,
        });

        let resp: CreateMessageResp = self
            .api_post(&format!("/channels/{target}/messages"), &body)
            .await?;

        Ok(resp.id)
    }

    async fn send_rich(&self, target: &str, msg: &RichMessage) -> anyhow::Result<String> {
        let mut embed = serde_json::json!({
            "description": msg.text,
        });

        if let Some(ref title) = msg.title {
            embed["title"] = serde_json::json!(title);
        }

        if let Some(ref color) = msg.color {
            // Convert hex color "#RRGGBB" to integer
            let hex = color.trim_start_matches('#');
            if let Ok(c) = u32::from_str_radix(hex, 16) {
                embed["color"] = serde_json::json!(c);
            }
        }

        if !msg.fields.is_empty() {
            let fields: Vec<serde_json::Value> = msg
                .fields
                .iter()
                .map(|(k, v)| {
                    serde_json::json!({
                        "name": k,
                        "value": v,
                        "inline": true
                    })
                })
                .collect();
            embed["fields"] = serde_json::json!(fields);
        }

        let mut body = serde_json::json!({
            "content": "",
            "embeds": [embed],
        });

        // Thread support: if thread_id is set, use message_reference to reply
        // in the thread. Discord threads are just channels, so we post to the
        // thread channel. For forum/thread replies, set message_reference.
        if let Some(ref thread_id) = msg.thread_id {
            body["message_reference"] = serde_json::json!({
                "message_id": thread_id,
            });
        }

        let resp: CreateMessageResp = self
            .api_post(&format!("/channels/{target}/messages"), &body)
            .await?;

        Ok(resp.id)
    }

    async fn history(&self, channel: &str, limit: u32) -> anyhow::Result<Vec<IncomingMessage>> {
        let limit_str = limit.to_string();
        let messages: Vec<DiscordMessage> = self
            .api_get(
                &format!("/channels/{channel}/messages"),
                &[("limit", &limit_str)],
            )
            .await?;

        let result = messages
            .into_iter()
            .map(|m| IncomingMessage {
                id: m.id,
                channel: m.channel_id,
                sender: m.author.username,
                content: m.content,
                timestamp: m.timestamp,
                thread_id: None,
            })
            .collect();

        Ok(result)
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<IncomingMessage>> {
        // Discord's message search is guild-scoped and requires guild ID.
        // For simplicity, we return an error suggesting channel-based history instead.
        anyhow::bail!(
            "Discord search requires a guild ID. Use history() on specific channels instead. Query: {query}"
        )
    }
}

// -- Integration trait --

impl Integration for DiscordAdapter {
    fn id(&self) -> &str {
        "discord"
    }

    fn name(&self) -> &str {
        "Discord"
    }

    fn messaging(&self) -> Option<&dyn MessagingAdapter> {
        Some(self)
    }

    fn document(&self) -> Option<&dyn crate::integrations::types::DocumentAdapter> {
        None
    }
}
