// src/integrations/google_docs.rs — Google Docs adapter (REST API + OAuth2)
//
// Uses the Google Docs API v1 (https://developers.google.com/docs/api/reference/rest).
// Requires OAuth2 credentials (client_id, client_secret, refresh_token).

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::integrations::types::{Document, DocumentAdapter, DocumentRef, Integration};

const DOCS_API_BASE: &str = "https://docs.googleapis.com/v1";
const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// Google Docs integration adapter.
pub struct GoogleDocsAdapter {
    client: Client,
    access_token: String,
    refresh_token: Option<String>,
    client_id: String,
    client_secret: String,
}

impl GoogleDocsAdapter {
    pub fn new(
        access_token: String,
        refresh_token: Option<String>,
        client_id: String,
        client_secret: String,
    ) -> Self {
        Self {
            client: Client::new(),
            access_token,
            refresh_token,
            client_id,
            client_secret,
        }
    }

    /// Refresh the access token using the refresh token.
    pub async fn refresh_access_token(&mut self) -> anyhow::Result<()> {
        let refresh = self
            .refresh_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No refresh token available"))?;

        #[derive(Deserialize)]
        struct TokenResp {
            access_token: String,
        }

        let resp: TokenResp = self
            .client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
            ])
            .send()
            .await?
            .json()
            .await?;

        self.access_token = resp.access_token;
        Ok(())
    }

    /// Validate access by fetching user info.
    pub async fn validate(&self) -> anyhow::Result<String> {
        #[derive(Deserialize)]
        struct AboutResp {
            user: Option<AboutUser>,
        }

        #[derive(Deserialize)]
        struct AboutUser {
            #[serde(rename = "displayName")]
            display_name: Option<String>,
            #[serde(rename = "emailAddress")]
            email: Option<String>,
        }

        let resp: AboutResp = self
            .client
            .get(format!("{DRIVE_API_BASE}/about"))
            .bearer_auth(&self.access_token)
            .query(&[("fields", "user(displayName,emailAddress)")])
            .send()
            .await?
            .json()
            .await?;

        let user = resp.user.unwrap_or(AboutUser {
            display_name: None,
            email: None,
        });
        Ok(format!(
            "Authenticated as {} ({})",
            user.display_name.unwrap_or_default(),
            user.email.unwrap_or_default()
        ))
    }

    /// Extract plain text from a Google Doc.
    fn extract_text(doc: &GoogleDoc) -> String {
        let mut text = String::new();
        if let Some(ref body) = doc.body {
            for element in &body.content {
                if let Some(ref paragraph) = element.paragraph {
                    for pe in &paragraph.elements {
                        if let Some(ref tr) = pe.text_run {
                            text.push_str(&tr.content);
                        }
                    }
                }
            }
        }
        text
    }
}

// -- Google Docs API types --

#[derive(Deserialize)]
struct GoogleDoc {
    #[serde(rename = "documentId")]
    document_id: String,
    title: Option<String>,
    body: Option<DocBody>,
}

#[derive(Deserialize)]
struct DocBody {
    content: Vec<StructuralElement>,
}

#[derive(Deserialize)]
struct StructuralElement {
    paragraph: Option<Paragraph>,
}

#[derive(Deserialize)]
struct Paragraph {
    elements: Vec<ParagraphElement>,
}

#[derive(Deserialize)]
struct ParagraphElement {
    #[serde(rename = "textRun")]
    text_run: Option<TextRun>,
}

#[derive(Deserialize)]
struct TextRun {
    content: String,
}

#[derive(Deserialize)]
struct DriveFileList {
    files: Option<Vec<DriveFile>>,
}

#[derive(Deserialize)]
struct DriveFile {
    id: String,
    name: Option<String>,
    #[serde(rename = "webViewLink")]
    web_view_link: Option<String>,
}

// -- DocumentAdapter implementation --

#[async_trait]
impl DocumentAdapter for GoogleDocsAdapter {
    async fn read(&self, doc_id: &str) -> anyhow::Result<Document> {
        let url = format!("{DOCS_API_BASE}/documents/{doc_id}");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Google Docs API returned {status}: {body}");
        }

        let doc: GoogleDoc = resp.json().await?;
        let content = Self::extract_text(&doc);

        Ok(Document {
            id: doc.document_id,
            title: doc.title.unwrap_or_else(|| "Untitled".into()),
            content,
            url: Some(format!("https://docs.google.com/document/d/{doc_id}/edit")),
        })
    }

    async fn write(&self, doc_id: &str, content: &str) -> anyhow::Result<()> {
        // Google Docs API uses batchUpdate with requests.
        // Strategy: insert text at the end of the document.
        let body = serde_json::json!({
            "requests": [{
                "insertText": {
                    "location": { "index": 1 },
                    "text": content,
                }
            }]
        });

        let url = format!("{DOCS_API_BASE}/documents/{doc_id}:batchUpdate");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Google Docs batchUpdate returned {status}: {text}");
        }

        Ok(())
    }

    async fn create(&self, title: &str, content: &str) -> anyhow::Result<String> {
        // Step 1: Create the document
        let create_body = serde_json::json!({
            "title": title,
        });

        let url = format!("{DOCS_API_BASE}/documents");
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&create_body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Google Docs create returned {status}: {text}");
        }

        let doc: GoogleDoc = resp.json().await?;
        let doc_id = doc.document_id;

        // Step 2: Insert content
        if !content.is_empty() {
            self.write(&doc_id, content).await?;
        }

        Ok(doc_id)
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<DocumentRef>> {
        // Use Google Drive API to search for documents
        let q = format!(
            "name contains '{query}' and mimeType = 'application/vnd.google-apps.document'"
        );
        let url = format!("{DRIVE_API_BASE}/files");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[
                ("q", q.as_str()),
                ("fields", "files(id,name,webViewLink)"),
                ("pageSize", "20"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Google Drive search returned {status}: {body}");
        }

        let file_list: DriveFileList = resp.json().await?;
        let refs = file_list
            .files
            .unwrap_or_default()
            .into_iter()
            .map(|f| DocumentRef {
                id: f.id,
                title: f.name.unwrap_or_else(|| "Untitled".into()),
                url: f.web_view_link,
            })
            .collect();

        Ok(refs)
    }

    async fn list(&self, _folder: Option<&str>) -> anyhow::Result<Vec<DocumentRef>> {
        // List recent Google Docs
        let q = "mimeType = 'application/vnd.google-apps.document'";
        let url = format!("{DRIVE_API_BASE}/files");
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[
                ("q", q),
                ("fields", "files(id,name,webViewLink)"),
                ("pageSize", "20"),
                ("orderBy", "modifiedTime desc"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Google Drive list returned {status}: {body}");
        }

        let file_list: DriveFileList = resp.json().await?;
        let refs = file_list
            .files
            .unwrap_or_default()
            .into_iter()
            .map(|f| DocumentRef {
                id: f.id,
                title: f.name.unwrap_or_else(|| "Untitled".into()),
                url: f.web_view_link,
            })
            .collect();

        Ok(refs)
    }
}

// -- Integration trait --

impl Integration for GoogleDocsAdapter {
    fn id(&self) -> &str {
        "google_docs"
    }

    fn name(&self) -> &str {
        "Google Docs"
    }

    fn messaging(&self) -> Option<&dyn crate::integrations::types::MessagingAdapter> {
        None
    }

    fn document(&self) -> Option<&dyn DocumentAdapter> {
        Some(self)
    }
}
