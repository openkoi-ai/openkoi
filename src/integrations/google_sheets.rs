// src/integrations/google_sheets.rs — Google Sheets adapter (REST API + OAuth2)
//
// Uses the Google Sheets API v4 (https://developers.google.com/sheets/api).
// Shares OAuth2 credentials with Google Docs integration.
// Implements DocumentAdapter — spreadsheets are treated as documents
// with tab-separated text content.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::integrations::types::{Document, DocumentAdapter, DocumentRef, Integration};

const SHEETS_API_BASE: &str = "https://sheets.googleapis.com/v4";
const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";

/// Google Sheets integration adapter.
pub struct GoogleSheetsAdapter {
    client: Client,
    access_token: String,
    refresh_token: Option<String>,
    client_id: String,
    client_secret: String,
}

impl GoogleSheetsAdapter {
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
            .post("https://oauth2.googleapis.com/token")
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

    /// Validate access by checking spreadsheets scope.
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
            "Google Sheets: {} ({})",
            user.display_name.unwrap_or_default(),
            user.email.unwrap_or_default()
        ))
    }
}

// -- Google Sheets API types --

#[derive(Deserialize)]
struct SpreadsheetResp {
    #[serde(rename = "spreadsheetId")]
    spreadsheet_id: String,
    properties: Option<SpreadsheetProperties>,
    sheets: Option<Vec<SheetMeta>>,
}

#[derive(Deserialize)]
struct SpreadsheetProperties {
    title: Option<String>,
}

#[derive(Deserialize)]
struct SheetMeta {
    properties: Option<SheetProperties>,
}

#[derive(Deserialize)]
struct SheetProperties {
    title: Option<String>,
    #[serde(rename = "sheetId")]
    _sheet_id: Option<i64>,
}

#[derive(Deserialize)]
struct ValueRangeResp {
    values: Option<Vec<Vec<String>>>,
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
impl DocumentAdapter for GoogleSheetsAdapter {
    async fn read(&self, doc_id: &str) -> anyhow::Result<Document> {
        // Get spreadsheet metadata
        let meta_url = format!("{SHEETS_API_BASE}/spreadsheets/{doc_id}");
        let meta_resp = self
            .client
            .get(&meta_url)
            .bearer_auth(&self.access_token)
            .query(&[("fields", "spreadsheetId,properties.title,sheets.properties.title")])
            .send()
            .await?;

        if !meta_resp.status().is_success() {
            let status = meta_resp.status();
            let body = meta_resp.text().await.unwrap_or_default();
            anyhow::bail!("Google Sheets API returned {status}: {body}");
        }

        let spreadsheet: SpreadsheetResp = meta_resp.json().await?;
        let title = spreadsheet
            .properties
            .and_then(|p| p.title)
            .unwrap_or_else(|| "Untitled".into());

        // Get the first sheet name
        let first_sheet = spreadsheet
            .sheets
            .as_ref()
            .and_then(|s| s.first())
            .and_then(|s| s.properties.as_ref())
            .and_then(|p| p.title.clone())
            .unwrap_or_else(|| "Sheet1".into());

        // Read values from the first sheet
        let values_url = format!(
            "{SHEETS_API_BASE}/spreadsheets/{doc_id}/values/{range}",
            range = urlencoded(&first_sheet)
        );
        let values_resp = self
            .client
            .get(&values_url)
            .bearer_auth(&self.access_token)
            .send()
            .await?;

        let content = if values_resp.status().is_success() {
            let vr: ValueRangeResp = values_resp.json().await?;
            format_values_as_tsv(&vr.values)
        } else {
            String::new()
        };

        Ok(Document {
            id: spreadsheet.spreadsheet_id,
            title,
            content,
            url: Some(format!(
                "https://docs.google.com/spreadsheets/d/{doc_id}/edit"
            )),
        })
    }

    async fn write(&self, doc_id: &str, content: &str) -> anyhow::Result<()> {
        // Parse content as TSV and write to Sheet1
        let values = parse_tsv_to_values(content);

        let url = format!(
            "{SHEETS_API_BASE}/spreadsheets/{doc_id}/values/Sheet1",
        );

        let body = serde_json::json!({
            "range": "Sheet1",
            "majorDimension": "ROWS",
            "values": values,
        });

        let resp = self
            .client
            .put(&url)
            .bearer_auth(&self.access_token)
            .query(&[("valueInputOption", "RAW")])
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Google Sheets write returned {status}: {text}");
        }

        Ok(())
    }

    async fn create(&self, title: &str, content: &str) -> anyhow::Result<String> {
        // Create a new spreadsheet
        let url = format!("{SHEETS_API_BASE}/spreadsheets");
        let body = serde_json::json!({
            "properties": {
                "title": title,
            }
        });

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
            anyhow::bail!("Google Sheets create returned {status}: {text}");
        }

        let spreadsheet: SpreadsheetResp = resp.json().await?;
        let doc_id = spreadsheet.spreadsheet_id;

        // Write initial content if provided
        if !content.is_empty() {
            self.write(&doc_id, content).await?;
        }

        Ok(doc_id)
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<DocumentRef>> {
        let q = format!(
            "name contains '{}' and mimeType = 'application/vnd.google-apps.spreadsheet'",
            query
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
        let q = "mimeType = 'application/vnd.google-apps.spreadsheet'";
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

impl Integration for GoogleSheetsAdapter {
    fn id(&self) -> &str {
        "google_sheets"
    }

    fn name(&self) -> &str {
        "Google Sheets"
    }

    fn messaging(&self) -> Option<&dyn crate::integrations::types::MessagingAdapter> {
        None
    }

    fn document(&self) -> Option<&dyn DocumentAdapter> {
        Some(self)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format spreadsheet values as tab-separated text.
fn format_values_as_tsv(values: &Option<Vec<Vec<String>>>) -> String {
    match values {
        Some(rows) => rows
            .iter()
            .map(|row| row.join("\t"))
            .collect::<Vec<_>>()
            .join("\n"),
        None => String::new(),
    }
}

/// Parse tab-separated text into a 2D vector for the Sheets API.
fn parse_tsv_to_values(content: &str) -> Vec<Vec<String>> {
    content
        .lines()
        .map(|line| line.split('\t').map(|s| s.to_string()).collect())
        .collect()
}

/// Simple URL encoding for sheet names.
fn urlencoded(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('!', "%21")
        .replace('\'', "%27")
}
