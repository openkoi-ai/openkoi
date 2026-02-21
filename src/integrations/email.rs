// src/integrations/email.rs — Email adapter (IMAP/SMTP)
//
// Implements MessagingAdapter for email using IMAP (read) and SMTP (send).
// Uses the `imap` crate for IMAP, `lettre` for SMTP, and `mailparse` for parsing.

use async_trait::async_trait;

use crate::integrations::types::{IncomingMessage, Integration, MessagingAdapter};

/// Email integration adapter.
pub struct EmailAdapter {
    /// IMAP server hostname (e.g. "imap.gmail.com")
    imap_host: String,
    /// IMAP port (typically 993 for TLS)
    imap_port: u16,
    /// SMTP server hostname (e.g. "smtp.gmail.com")
    smtp_host: String,
    /// SMTP port (typically 587 for STARTTLS)
    smtp_port: u16,
    /// Email address (used as both IMAP username and SMTP sender)
    email: String,
    /// Password or app-specific password
    password: String,
}

impl EmailAdapter {
    pub fn new(
        imap_host: String,
        imap_port: u16,
        smtp_host: String,
        smtp_port: u16,
        email: String,
        password: String,
    ) -> Self {
        Self {
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            email,
            password,
        }
    }

    /// Create with Gmail defaults.
    pub fn gmail(email: String, password: String) -> Self {
        Self::new(
            "imap.gmail.com".into(),
            993,
            "smtp.gmail.com".into(),
            587,
            email,
            password,
        )
    }

    /// Create with Outlook/Office 365 defaults.
    pub fn outlook(email: String, password: String) -> Self {
        Self::new(
            "outlook.office365.com".into(),
            993,
            "smtp.office365.com".into(),
            587,
            email,
            password,
        )
    }

    /// Validate the connection by attempting IMAP login.
    pub fn validate(&self) -> anyhow::Result<String> {
        let client = imap::ClientBuilder::new(&self.imap_host, self.imap_port)
            .connect()
            .map_err(|e| anyhow::anyhow!("IMAP connect failed: {}", e))?;

        let mut session = client
            .login(&self.email, &self.password)
            .map_err(|e| anyhow::anyhow!("IMAP login failed: {}", e.0))?;

        let mailbox = session.select("INBOX")?;
        let count = mailbox.exists;
        session.logout()?;

        Ok(format!(
            "Email connected: {} ({} messages in INBOX)",
            self.email, count
        ))
    }

    /// Fetch recent messages from IMAP.
    fn fetch_messages(&self, folder: &str, limit: u32) -> anyhow::Result<Vec<IncomingMessage>> {
        let client = imap::ClientBuilder::new(&self.imap_host, self.imap_port)
            .connect()
            .map_err(|e| anyhow::anyhow!("IMAP connect failed: {}", e))?;

        let mut session = client
            .login(&self.email, &self.password)
            .map_err(|e| anyhow::anyhow!("IMAP login failed: {}", e.0))?;

        let mailbox = session.select(folder)?;
        let total = mailbox.exists;

        if total == 0 {
            session.logout()?;
            return Ok(Vec::new());
        }

        // Fetch the most recent `limit` messages
        let start = if total > limit { total - limit + 1 } else { 1 };
        let range = format!("{}:{}", start, total);

        let messages = session.fetch(&range, "(UID ENVELOPE BODY[TEXT])")?;

        let mut result = Vec::new();

        for msg in messages.iter() {
            let uid = msg.uid.unwrap_or(0);

            // Parse envelope for sender and subject
            let (sender, subject, date) = if let Some(env) = msg.envelope() {
                let from = env
                    .from
                    .as_ref()
                    .and_then(|addrs: &Vec<_>| addrs.first())
                    .map(|a| {
                        let mailbox = a
                            .mailbox
                            .as_ref()
                            .map(|m| std::str::from_utf8(m).unwrap_or("unknown"))
                            .unwrap_or("unknown");
                        let host = a
                            .host
                            .as_ref()
                            .map(|h| std::str::from_utf8(h).unwrap_or("unknown"))
                            .unwrap_or("unknown");
                        format!("{}@{}", mailbox, host)
                    })
                    .unwrap_or_else(|| "unknown".into());

                let subj = env
                    .subject
                    .as_ref()
                    .map(|s| std::str::from_utf8(s).unwrap_or("(no subject)").to_string())
                    .unwrap_or_else(|| "(no subject)".into());

                let dt = env
                    .date
                    .as_ref()
                    .map(|d| std::str::from_utf8(d).unwrap_or("").to_string())
                    .unwrap_or_default();

                (from, subj, dt)
            } else {
                ("unknown".into(), "(no subject)".into(), String::new())
            };

            // Get body text
            let body_text = msg
                .text()
                .map(|b| String::from_utf8_lossy(b).to_string())
                .unwrap_or_default();

            // Try to parse with mailparse for better text extraction
            let content = if let Ok(parsed) = mailparse::parse_mail(body_text.as_bytes()) {
                extract_text_from_mail(&parsed)
            } else {
                body_text.chars().take(2000).collect()
            };

            result.push(IncomingMessage {
                id: uid.to_string(),
                channel: folder.to_string(),
                sender,
                content: format!("Subject: {}\n\n{}", subject, content),
                timestamp: date,
                thread_id: None,
            });
        }

        session.logout()?;

        // Return in reverse chronological order
        result.reverse();
        Ok(result)
    }

    /// Search for messages matching a query.
    fn search_messages(&self, query: &str) -> anyhow::Result<Vec<IncomingMessage>> {
        let client = imap::ClientBuilder::new(&self.imap_host, self.imap_port)
            .connect()
            .map_err(|e| anyhow::anyhow!("IMAP connect failed: {}", e))?;

        let mut session = client
            .login(&self.email, &self.password)
            .map_err(|e| anyhow::anyhow!("IMAP login failed: {}", e.0))?;

        session.select("INBOX")?;

        // IMAP SEARCH command
        let search_criteria = format!("OR SUBJECT \"{}\" BODY \"{}\"", query, query);
        let uids = session.search(&search_criteria)?;

        let mut result = Vec::new();

        if !uids.is_empty() {
            // Take at most 20 results — sort descending for recency
            let mut uid_vec: Vec<u32> = uids.into_iter().collect();
            uid_vec.sort_unstable_by(|a, b| b.cmp(a));
            let uid_list: Vec<String> = uid_vec.iter().take(20).map(|u| u.to_string()).collect();
            let uid_range = uid_list.join(",");

            let messages = session.fetch(&uid_range, "(UID ENVELOPE BODY[TEXT])")?;

            for msg in messages.iter() {
                let uid = msg.uid.unwrap_or(0);

                let (sender, subject, date) = if let Some(env) = msg.envelope() {
                    let from = env
                        .from
                        .as_ref()
                        .and_then(|addrs: &Vec<_>| addrs.first())
                        .map(|a| {
                            let mailbox = a
                                .mailbox
                                .as_ref()
                                .map(|m| std::str::from_utf8(m).unwrap_or("unknown"))
                                .unwrap_or("unknown");
                            let host = a
                                .host
                                .as_ref()
                                .map(|h| std::str::from_utf8(h).unwrap_or("unknown"))
                                .unwrap_or("unknown");
                            format!("{}@{}", mailbox, host)
                        })
                        .unwrap_or_else(|| "unknown".into());

                    let subj = env
                        .subject
                        .as_ref()
                        .map(|s| std::str::from_utf8(s).unwrap_or("(no subject)").to_string())
                        .unwrap_or_else(|| "(no subject)".into());

                    let dt = env
                        .date
                        .as_ref()
                        .map(|d| std::str::from_utf8(d).unwrap_or("").to_string())
                        .unwrap_or_default();

                    (from, subj, dt)
                } else {
                    ("unknown".into(), "(no subject)".into(), String::new())
                };

                let body_text = msg
                    .text()
                    .map(|b| String::from_utf8_lossy(b).to_string())
                    .unwrap_or_default();

                let content = if let Ok(parsed) = mailparse::parse_mail(body_text.as_bytes()) {
                    extract_text_from_mail(&parsed)
                } else {
                    body_text.chars().take(2000).collect()
                };

                result.push(IncomingMessage {
                    id: uid.to_string(),
                    channel: "INBOX".to_string(),
                    sender,
                    content: format!("Subject: {}\n\n{}", subject, content),
                    timestamp: date,
                    thread_id: None,
                });
            }
        }

        session.logout()?;
        Ok(result)
    }

    /// Send an email via SMTP.
    fn send_email(&self, to: &str, content: &str) -> anyhow::Result<String> {
        use lettre::message::header::ContentType;
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::{Message, SmtpTransport, Transport};

        // Parse subject and body from content
        // Format: "Subject: ...\n\nBody..."
        let (subject, body) = if let Some(rest) = content.strip_prefix("Subject: ") {
            if let Some(idx) = rest.find("\n\n") {
                (rest[..idx].to_string(), rest[idx + 2..].to_string())
            } else {
                (rest.to_string(), String::new())
            }
        } else {
            ("Message from OpenKoi".to_string(), content.to_string())
        };

        let email = Message::builder()
            .from(self.email.parse()?)
            .to(to.parse()?)
            .subject(&subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)?;

        let creds = Credentials::new(self.email.clone(), self.password.clone());

        let mailer = SmtpTransport::starttls_relay(&self.smtp_host)?
            .port(self.smtp_port)
            .credentials(creds)
            .build();

        let response = mailer.send(&email)?;
        Ok(format!("Email sent ({})", response.code()))
    }
}

// -- MessagingAdapter implementation --

#[async_trait]
impl MessagingAdapter for EmailAdapter {
    async fn send(&self, target: &str, content: &str) -> anyhow::Result<String> {
        // IMAP/SMTP are blocking, so we run on a blocking thread
        let adapter = EmailSendParams {
            imap_host: self.imap_host.clone(),
            imap_port: self.imap_port,
            smtp_host: self.smtp_host.clone(),
            smtp_port: self.smtp_port,
            email: self.email.clone(),
            password: self.password.clone(),
        };
        let target = target.to_string();
        let content = content.to_string();

        tokio::task::spawn_blocking(move || {
            let adapter = EmailAdapter::new(
                adapter.imap_host,
                adapter.imap_port,
                adapter.smtp_host,
                adapter.smtp_port,
                adapter.email,
                adapter.password,
            );
            adapter.send_email(&target, &content)
        })
        .await?
    }

    async fn history(&self, channel: &str, limit: u32) -> anyhow::Result<Vec<IncomingMessage>> {
        let adapter = EmailSendParams {
            imap_host: self.imap_host.clone(),
            imap_port: self.imap_port,
            smtp_host: self.smtp_host.clone(),
            smtp_port: self.smtp_port,
            email: self.email.clone(),
            password: self.password.clone(),
        };
        let channel = channel.to_string();

        tokio::task::spawn_blocking(move || {
            let adapter = EmailAdapter::new(
                adapter.imap_host,
                adapter.imap_port,
                adapter.smtp_host,
                adapter.smtp_port,
                adapter.email,
                adapter.password,
            );
            adapter.fetch_messages(&channel, limit)
        })
        .await?
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<IncomingMessage>> {
        let adapter = EmailSendParams {
            imap_host: self.imap_host.clone(),
            imap_port: self.imap_port,
            smtp_host: self.smtp_host.clone(),
            smtp_port: self.smtp_port,
            email: self.email.clone(),
            password: self.password.clone(),
        };
        let query = query.to_string();

        tokio::task::spawn_blocking(move || {
            let adapter = EmailAdapter::new(
                adapter.imap_host,
                adapter.imap_port,
                adapter.smtp_host,
                adapter.smtp_port,
                adapter.email,
                adapter.password,
            );
            adapter.search_messages(&query)
        })
        .await?
    }
}

/// Helper struct to pass connection params to blocking tasks.
struct EmailSendParams {
    imap_host: String,
    imap_port: u16,
    smtp_host: String,
    smtp_port: u16,
    email: String,
    password: String,
}

// -- Integration trait --

impl Integration for EmailAdapter {
    fn id(&self) -> &str {
        "email"
    }

    fn name(&self) -> &str {
        "Email"
    }

    fn messaging(&self) -> Option<&dyn MessagingAdapter> {
        Some(self)
    }

    fn document(&self) -> Option<&dyn crate::integrations::types::DocumentAdapter> {
        None
    }
}

// ---------------------------------------------------------------------------
// Mail parsing helpers
// ---------------------------------------------------------------------------

/// Extract plain text from a parsed email.
fn extract_text_from_mail(mail: &mailparse::ParsedMail) -> String {
    // Try to find text/plain part
    if mail.subparts.is_empty() {
        // Single-part message
        mail.get_body().unwrap_or_default()
    } else {
        // Multipart — find text/plain
        for part in &mail.subparts {
            let ctype = &part.ctype.mimetype;
            if ctype == "text/plain" {
                if let Ok(body) = part.get_body() {
                    return body;
                }
            }
        }
        // Fallback: try text/html stripped of tags
        for part in &mail.subparts {
            let ctype = &part.ctype.mimetype;
            if ctype == "text/html" {
                if let Ok(body) = part.get_body() {
                    return strip_html_tags(&body);
                }
            }
        }
        // Last resort: first part
        mail.subparts
            .first()
            .and_then(|p| p.get_body().ok())
            .unwrap_or_default()
    }
}

/// Strip HTML tags for plain text extraction.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;

    for ch in html.chars() {
        if ch == '<' {
            in_tag = true;
        } else if ch == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(ch);
        }
    }

    // Collapse whitespace
    let mut collapsed = String::new();
    let mut last_was_space = false;
    for ch in result.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                collapsed.push(' ');
                last_was_space = true;
            }
        } else {
            collapsed.push(ch);
            last_was_space = false;
        }
    }

    collapsed.trim().to_string()
}
