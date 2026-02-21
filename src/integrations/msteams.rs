// src/integrations/msteams.rs — Microsoft Teams adapter (Graph API)
//
// Uses the Microsoft Graph API v1.0 for sending/receiving messages in Teams
// channels. Requires a registered Azure AD app with appropriate permissions
// (ChannelMessage.Send, ChannelMessage.Read.All).

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::integrations::types::{IncomingMessage, Integration, MessagingAdapter};

const GRAPH_API_BASE: &str = "https://graph.microsoft.com/v1.0";

/// Microsoft Teams integration adapter using Microsoft Graph API.
pub struct MsTeamsAdapter {
    client: Client,
    /// OAuth2 access token for Microsoft Graph API
    access_token: String,
    /// Tenant ID (reserved for future token refresh)
    _tenant_id: String,
    /// Default team ID for operations
    team_id: Option<String>,
}

impl MsTeamsAdapter {
    pub fn new(access_token: String, tenant_id: String, team_id: Option<String>) -> Self {
        Self {
            client: Client::new(),
            access_token,
            _tenant_id: tenant_id,
            team_id,
        }
    }

    /// Make an authenticated GET request to the Microsoft Graph API.
    async fn api_get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> anyhow::Result<T> {
        let url = format!("{GRAPH_API_BASE}{path}");
        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .query(params)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("MS Graph API {path} returned {status}: {body}");
        }

        let body: T = resp.json().await?;
        Ok(body)
    }

    /// Make an authenticated POST request to the Microsoft Graph API.
    async fn api_post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<T> {
        let url = format!("{GRAPH_API_BASE}{path}");
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("MS Graph API {path} returned {status}: {text}");
        }

        let body: T = resp.json().await?;
        Ok(body)
    }

    /// Validate the access token by fetching the current user profile.
    pub async fn validate(&self) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct UserProfile {
            display_name: Option<String>,
            mail: Option<String>,
        }

        let profile: UserProfile = self.api_get("/me", &[]).await?;
        Ok(format!(
            "Authenticated as {} ({})",
            profile.display_name.unwrap_or_else(|| "Unknown".into()),
            profile.mail.unwrap_or_else(|| "no email".into())
        ))
    }

    /// Parse a target string in "team_id/channel_id" format.
    /// If only a channel_id is given, uses the configured default team.
    fn parse_target(&self, target: &str) -> anyhow::Result<(String, String)> {
        if let Some((team, channel)) = target.split_once('/') {
            Ok((team.to_string(), channel.to_string()))
        } else if let Some(ref team) = self.team_id {
            Ok((team.clone(), target.to_string()))
        } else {
            anyhow::bail!(
                "Target must be 'team_id/channel_id' format, or set a default team_id. Got: {target}"
            )
        }
    }
}

// ─── Microsoft Graph API response types ─────────────────────────────────────

#[derive(Deserialize)]
struct GraphMessageList {
    value: Vec<GraphMessage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphMessage {
    id: String,
    #[serde(default)]
    body: GraphMessageBody,
    from: Option<GraphMessageFrom>,
    created_date_time: Option<String>,
    channel_identity: Option<GraphChannelIdentity>,
}

#[derive(Deserialize, Default)]
struct GraphMessageBody {
    #[serde(default)]
    content: String,
    #[serde(rename = "contentType", default)]
    content_type: String,
}

#[derive(Deserialize)]
struct GraphMessageFrom {
    user: Option<GraphUser>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphUser {
    display_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphChannelIdentity {
    channel_id: Option<String>,
}

#[derive(Deserialize)]
struct CreateMessageResp {
    id: String,
}

// ─── MessagingAdapter implementation ────────────────────────────────────────

#[async_trait]
impl MessagingAdapter for MsTeamsAdapter {
    async fn send(&self, target: &str, content: &str) -> anyhow::Result<String> {
        let (team_id, channel_id) = self.parse_target(target)?;

        let body = serde_json::json!({
            "body": {
                "contentType": "text",
                "content": content,
            }
        });

        let resp: CreateMessageResp = self
            .api_post(
                &format!("/teams/{team_id}/channels/{channel_id}/messages"),
                &body,
            )
            .await?;

        Ok(resp.id)
    }

    async fn history(&self, channel: &str, limit: u32) -> anyhow::Result<Vec<IncomingMessage>> {
        let (team_id, channel_id) = self.parse_target(channel)?;
        let top = limit.to_string();

        let messages: GraphMessageList = self
            .api_get(
                &format!("/teams/{team_id}/channels/{channel_id}/messages"),
                &[("$top", &top)],
            )
            .await?;

        let result = messages
            .value
            .into_iter()
            .map(|m| {
                let sender = m
                    .from
                    .and_then(|f| f.user.and_then(|u| u.display_name))
                    .unwrap_or_else(|| "unknown".into());

                let channel = m
                    .channel_identity
                    .and_then(|c| c.channel_id)
                    .unwrap_or_else(|| channel_id.clone());

                // Strip HTML if content type is html
                let content = if m.body.content_type == "html" {
                    strip_html_tags(&m.body.content)
                } else {
                    m.body.content
                };

                IncomingMessage {
                    id: m.id,
                    channel,
                    sender,
                    content,
                    timestamp: m.created_date_time.unwrap_or_default(),
                    thread_id: None,
                }
            })
            .collect();

        Ok(result)
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<IncomingMessage>> {
        // Microsoft Graph supports /search/query for messages, but it requires
        // specific permissions (Mail.Read, etc.) and uses a different API surface.
        // For Teams messages, search is limited. Return an actionable error.
        anyhow::bail!(
            "MS Teams message search requires Microsoft Search API permissions. \
             Use history() on specific channels instead. Query: {query}"
        )
    }
}

// ─── Integration trait ──────────────────────────────────────────────────────

impl Integration for MsTeamsAdapter {
    fn id(&self) -> &str {
        "msteams"
    }

    fn name(&self) -> &str {
        "Microsoft Teams"
    }

    fn messaging(&self) -> Option<&dyn MessagingAdapter> {
        Some(self)
    }

    fn document(&self) -> Option<&dyn crate::integrations::types::DocumentAdapter> {
        None
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Simple HTML tag stripper for Teams message content.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<p>Hello <b>World</b></p>"), "Hello World");
        assert_eq!(strip_html_tags("plain text"), "plain text");
        assert_eq!(strip_html_tags(""), "");
    }

    #[test]
    fn test_parse_target_with_team() {
        let adapter =
            MsTeamsAdapter::new("token".into(), "tenant".into(), Some("default-team".into()));
        let (team, channel) = adapter.parse_target("channel-123").unwrap();
        assert_eq!(team, "default-team");
        assert_eq!(channel, "channel-123");
    }

    #[test]
    fn test_parse_target_explicit() {
        let adapter = MsTeamsAdapter::new("token".into(), "tenant".into(), None);
        let (team, channel) = adapter.parse_target("team-abc/channel-123").unwrap();
        assert_eq!(team, "team-abc");
        assert_eq!(channel, "channel-123");
    }

    #[test]
    fn test_parse_target_no_default() {
        let adapter = MsTeamsAdapter::new("token".into(), "tenant".into(), None);
        assert!(adapter.parse_target("channel-only").is_err());
    }

    #[test]
    fn test_integration_trait() {
        let adapter = MsTeamsAdapter::new("token".into(), "tenant".into(), None);
        assert_eq!(adapter.id(), "msteams");
        assert_eq!(adapter.name(), "Microsoft Teams");
        assert!(adapter.messaging().is_some());
        assert!(adapter.document().is_none());
    }
}
