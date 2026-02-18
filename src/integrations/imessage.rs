// src/integrations/imessage.rs — iMessage adapter (macOS AppleScript)
//
// macOS-only integration using AppleScript to interact with Messages.app.
// This provides a basic MessagingAdapter for iMessage.

use async_trait::async_trait;

use crate::integrations::types::{IncomingMessage, Integration, MessagingAdapter};

/// iMessage integration adapter (macOS only).
pub struct IMessageAdapter;

impl IMessageAdapter {
    pub fn new() -> anyhow::Result<Self> {
        // Verify we're on macOS
        if !cfg!(target_os = "macos") {
            anyhow::bail!("iMessage integration is only available on macOS");
        }
        Ok(Self)
    }

    /// Validate that Messages.app is accessible.
    pub async fn validate(&self) -> anyhow::Result<String> {
        if !cfg!(target_os = "macos") {
            anyhow::bail!("iMessage is macOS only");
        }

        // Check if Messages.app is available
        let output = tokio::process::Command::new("osascript")
            .args(["-e", "tell application \"Messages\" to get name"])
            .output()
            .await?;

        if output.status.success() {
            Ok("Messages.app is accessible".to_string())
        } else {
            anyhow::bail!("Cannot access Messages.app. Grant Terminal access in System Settings > Privacy > Automation.");
        }
    }
}

#[async_trait]
impl MessagingAdapter for IMessageAdapter {
    async fn send(&self, target: &str, content: &str) -> anyhow::Result<String> {
        if !cfg!(target_os = "macos") {
            anyhow::bail!("iMessage is macOS only");
        }

        // Escape content for AppleScript
        let escaped = content.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            r#"tell application "Messages"
                set targetBuddy to buddy "{target}" of service "iMessage"
                send "{escaped}" to targetBuddy
            end tell"#
        );

        let output = tokio::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("iMessage send failed: {stderr}");
        }

        Ok(format!("sent to {target}"))
    }

    async fn history(&self, _channel: &str, limit: u32) -> anyhow::Result<Vec<IncomingMessage>> {
        if !cfg!(target_os = "macos") {
            anyhow::bail!("iMessage is macOS only");
        }

        // Read recent messages from Messages.app using AppleScript
        // Note: This is limited — AppleScript access to Messages history is restricted.
        // A more robust approach would use the chat.db SQLite file directly.
        let script = format!(
            r#"tell application "Messages"
                set msgs to {{}}
                set allChats to chats
                repeat with c in allChats
                    if (count of messages of c) > 0 then
                        set lastMsg to message 1 of c
                        set end of msgs to (sender of lastMsg) & "|||" & (text of lastMsg)
                    end if
                    if (count of msgs) >= {limit} then exit repeat
                end repeat
                return msgs
            end tell"#
        );

        let output = tokio::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .await?;

        if !output.status.success() {
            // Fallback: try reading from chat.db
            return self.read_from_chat_db(limit).await;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let messages = stdout
            .split(", ")
            .enumerate()
            .filter_map(|(i, entry)| {
                let parts: Vec<&str> = entry.splitn(2, "|||").collect();
                if parts.len() == 2 {
                    Some(IncomingMessage {
                        id: i.to_string(),
                        channel: "iMessage".to_string(),
                        sender: parts[0].trim().to_string(),
                        content: parts[1].trim().to_string(),
                        timestamp: String::new(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(messages)
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<IncomingMessage>> {
        if !cfg!(target_os = "macos") {
            anyhow::bail!("iMessage is macOS only");
        }

        // Search the chat.db SQLite database directly
        self.search_chat_db(query).await
    }
}

impl IMessageAdapter {
    /// Read messages from ~/Library/Messages/chat.db (more reliable than AppleScript).
    async fn read_from_chat_db(&self, limit: u32) -> anyhow::Result<Vec<IncomingMessage>> {
        let home = crate::infra::paths::dirs_home();
        let db_path = home.join("Library/Messages/chat.db");

        if !db_path.exists() {
            anyhow::bail!("Messages database not found at {}", db_path.display());
        }

        // Use sqlite3 CLI to avoid linking against the system sqlite
        let query = format!(
            "SELECT m.ROWID, m.text, h.id, datetime(m.date/1000000000 + 978307200, 'unixepoch') as ts \
             FROM message m \
             LEFT JOIN handle h ON m.handle_id = h.ROWID \
             WHERE m.text IS NOT NULL \
             ORDER BY m.date DESC LIMIT {limit}"
        );

        let output = tokio::process::Command::new("sqlite3")
            .args(["-separator", "|||", db_path.to_str().unwrap_or(""), &query])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("chat.db query failed: {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let messages = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(4, "|||").collect();
                if parts.len() >= 2 {
                    Some(IncomingMessage {
                        id: parts[0].to_string(),
                        channel: "iMessage".to_string(),
                        sender: parts.get(2).unwrap_or(&"unknown").to_string(),
                        content: parts[1].to_string(),
                        timestamp: parts.get(3).unwrap_or(&"").to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(messages)
    }

    /// Search chat.db for messages matching a query.
    async fn search_chat_db(&self, query: &str) -> anyhow::Result<Vec<IncomingMessage>> {
        let home = crate::infra::paths::dirs_home();
        let db_path = home.join("Library/Messages/chat.db");

        if !db_path.exists() {
            anyhow::bail!("Messages database not found at {}", db_path.display());
        }

        let escaped_query = query.replace('\'', "''");
        let sql = format!(
            "SELECT m.ROWID, m.text, h.id, datetime(m.date/1000000000 + 978307200, 'unixepoch') as ts \
             FROM message m \
             LEFT JOIN handle h ON m.handle_id = h.ROWID \
             WHERE m.text LIKE '%{escaped_query}%' \
             ORDER BY m.date DESC LIMIT 20"
        );

        let output = tokio::process::Command::new("sqlite3")
            .args(["-separator", "|||", db_path.to_str().unwrap_or(""), &sql])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("chat.db search failed: {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let messages = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(4, "|||").collect();
                if parts.len() >= 2 {
                    Some(IncomingMessage {
                        id: parts[0].to_string(),
                        channel: "iMessage".to_string(),
                        sender: parts.get(2).unwrap_or(&"unknown").to_string(),
                        content: parts[1].to_string(),
                        timestamp: parts.get(3).unwrap_or(&"").to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(messages)
    }
}

// -- Integration trait --

impl Integration for IMessageAdapter {
    fn id(&self) -> &str {
        "imessage"
    }

    fn name(&self) -> &str {
        "iMessage"
    }

    fn messaging(&self) -> Option<&dyn MessagingAdapter> {
        Some(self)
    }

    fn document(&self) -> Option<&dyn crate::integrations::types::DocumentAdapter> {
        None
    }
}
