// src/integrations/telegram.rs — Telegram adapter (Bot API)
//
// Uses the Telegram Bot API (https://core.telegram.org/bots/api).

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::integrations::types::{IncomingMessage, Integration, MessagingAdapter};

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";

/// Telegram integration adapter.
pub struct TelegramAdapter {
    client: Client,
    bot_token: String,
}

impl TelegramAdapter {
    pub fn new(bot_token: String) -> Self {
        Self {
            client: Client::new(),
            bot_token,
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("{TELEGRAM_API_BASE}/bot{}/{method}", self.bot_token)
    }

    /// Validate the bot token by calling getMe.
    pub async fn validate(&self) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct GetMeResp {
            ok: bool,
            result: Option<BotUser>,
        }

        #[derive(Deserialize)]
        struct BotUser {
            username: Option<String>,
            first_name: Option<String>,
        }

        let resp: GetMeResp = self
            .client
            .get(self.api_url("getMe"))
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!("Telegram auth failed");
        }

        let bot = resp.result.unwrap_or(BotUser {
            username: None,
            first_name: None,
        });
        Ok(format!(
            "Authenticated as @{}",
            bot.username
                .unwrap_or_else(|| bot.first_name.unwrap_or_default())
        ))
    }
}

// -- Telegram API response types --

#[derive(Deserialize)]
struct TelegramResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct TgMessage {
    message_id: i64,
    chat: TgChat,
    from: Option<TgUser>,
    text: Option<String>,
    date: i64,
}

#[derive(Deserialize)]
struct TgChat {
    id: i64,
    title: Option<String>,
}

#[derive(Deserialize)]
struct TgUser {
    username: Option<String>,
    first_name: Option<String>,
}

#[derive(Deserialize)]
struct TgUpdate {
    message: Option<TgMessage>,
}

#[derive(Deserialize)]
struct SendMessageResp {
    message_id: i64,
}

// -- MessagingAdapter implementation --

#[async_trait]
impl MessagingAdapter for TelegramAdapter {
    async fn send(&self, target: &str, content: &str) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "chat_id": target,
            "text": content,
            "parse_mode": "Markdown",
        });

        let resp: TelegramResponse<SendMessageResp> = self
            .client
            .post(self.api_url("sendMessage"))
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!(
                "Telegram send failed: {}",
                resp.description.unwrap_or_else(|| "unknown".into())
            );
        }

        Ok(resp
            .result
            .map(|r| r.message_id.to_string())
            .unwrap_or_default())
    }

    async fn history(&self, _channel: &str, limit: u32) -> anyhow::Result<Vec<IncomingMessage>> {
        // Telegram Bot API doesn't provide direct history access.
        // We use getUpdates to get recent messages (polling-based).
        let body = serde_json::json!({
            "limit": limit,
            "timeout": 0,
        });

        let resp: TelegramResponse<Vec<TgUpdate>> = self
            .client
            .post(self.api_url("getUpdates"))
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if !resp.ok {
            anyhow::bail!(
                "Telegram getUpdates failed: {}",
                resp.description.unwrap_or_else(|| "unknown".into())
            );
        }

        let messages = resp
            .result
            .unwrap_or_default()
            .into_iter()
            .filter_map(|u| u.message)
            .map(|m| {
                let sender = m
                    .from
                    .as_ref()
                    .and_then(|u| u.username.clone().or(u.first_name.clone()))
                    .unwrap_or_else(|| "unknown".into());
                IncomingMessage {
                    id: m.message_id.to_string(),
                    channel: m.chat.title.unwrap_or_else(|| m.chat.id.to_string()),
                    sender,
                    content: m.text.unwrap_or_default(),
                    timestamp: m.date.to_string(),
                }
            })
            .collect();

        Ok(messages)
    }

    async fn search(&self, _query: &str) -> anyhow::Result<Vec<IncomingMessage>> {
        // Telegram Bot API doesn't support message search.
        anyhow::bail!("Telegram Bot API does not support message search")
    }
}

// -- Integration trait --

impl Integration for TelegramAdapter {
    fn id(&self) -> &str {
        "telegram"
    }

    fn name(&self) -> &str {
        "Telegram"
    }

    fn messaging(&self) -> Option<&dyn MessagingAdapter> {
        Some(self)
    }

    fn document(&self) -> Option<&dyn crate::integrations::types::DocumentAdapter> {
        None
    }
}
