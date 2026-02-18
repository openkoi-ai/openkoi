// src/integrations/msoffice.rs — MS Office adapter (local docx/xlsx files)
//
// Reads and writes local .docx and .xlsx files using pure-Rust parsing.
// .docx files are ZIP archives containing XML; we parse word/document.xml.
// .xlsx files are ZIP archives containing XML; we parse xl/sharedStrings.xml + xl/worksheets/sheet1.xml.
//
// This adapter operates on local files — no API credentials needed.
// Trust level: Full (local filesystem).

use async_trait::async_trait;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use crate::integrations::types::{Document, DocumentAdapter, DocumentRef, Integration};

/// MS Office integration adapter for local .docx and .xlsx files.
pub struct MsOfficeAdapter {
    /// Base directory to scan for Office files.
    base_dir: PathBuf,
}

impl MsOfficeAdapter {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Create with the user's Documents directory as the base.
    pub fn with_documents_dir() -> anyhow::Result<Self> {
        let home = crate::infra::paths::dirs_home();
        let docs_dir = home.join("Documents");
        if !docs_dir.exists() {
            anyhow::bail!("Documents directory not found: {}", docs_dir.display());
        }
        Ok(Self {
            base_dir: docs_dir,
        })
    }

    /// Resolve a doc_id to a full path.
    /// doc_id can be a relative path (from base_dir) or absolute path.
    fn resolve_path(&self, doc_id: &str) -> PathBuf {
        let path = PathBuf::from(doc_id);
        if path.is_absolute() {
            path
        } else {
            self.base_dir.join(path)
        }
    }

    /// Extract text from a .docx file.
    fn read_docx(path: &Path) -> anyhow::Result<String> {
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        // Read word/document.xml
        let mut doc_xml = String::new();
        {
            let mut entry = archive.by_name("word/document.xml")?;
            entry.read_to_string(&mut doc_xml)?;
        }

        // Extract text content from XML (simple approach: strip tags)
        Ok(extract_text_from_xml(&doc_xml))
    }

    /// Extract text from a .xlsx file.
    fn read_xlsx(path: &Path) -> anyhow::Result<String> {
        let file = std::fs::File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        // Read shared strings
        let shared_strings = if let Ok(mut entry) = archive.by_name("xl/sharedStrings.xml") {
            let mut xml = String::new();
            entry.read_to_string(&mut xml)?;
            parse_shared_strings(&xml)
        } else {
            Vec::new()
        };

        // Read first worksheet
        let mut sheet_xml = String::new();
        {
            let mut entry = archive.by_name("xl/worksheets/sheet1.xml")?;
            entry.read_to_string(&mut sheet_xml)?;
        }

        Ok(extract_xlsx_text(&sheet_xml, &shared_strings))
    }

    /// Write a .docx file with simple text content.
    fn write_docx(path: &Path, content: &str) -> anyhow::Result<()> {
        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let options = zip::write::SimpleFileOptions::default();

            // [Content_Types].xml
            zip.start_file("[Content_Types].xml", options)?;
            zip.write_all(CONTENT_TYPES_XML.as_bytes())?;

            // _rels/.rels
            zip.start_file("_rels/.rels", options)?;
            zip.write_all(RELS_XML.as_bytes())?;

            // word/_rels/document.xml.rels
            zip.start_file("word/_rels/document.xml.rels", options)?;
            zip.write_all(DOCUMENT_RELS_XML.as_bytes())?;

            // word/document.xml
            zip.start_file("word/document.xml", options)?;
            let doc_xml = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:r>
        <w:t>{}</w:t>
      </w:r>
    </w:p>
  </w:body>
</w:document>"#,
                escape_xml(content)
            );
            zip.write_all(doc_xml.as_bytes())?;

            zip.finish()?;
        }

        std::fs::write(path, &buf)?;
        Ok(())
    }
}

// Minimal .docx scaffolding
const CONTENT_TYPES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOCUMENT_RELS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
</Relationships>"#;

// -- DocumentAdapter implementation --

#[async_trait]
impl DocumentAdapter for MsOfficeAdapter {
    async fn read(&self, doc_id: &str) -> anyhow::Result<Document> {
        let path = self.resolve_path(doc_id);

        if !path.exists() {
            anyhow::bail!("File not found: {}", path.display());
        }

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let content = match extension.as_str() {
            "docx" => Self::read_docx(&path)?,
            "xlsx" => Self::read_xlsx(&path)?,
            "txt" | "md" | "csv" => std::fs::read_to_string(&path)?,
            _ => anyhow::bail!("Unsupported file type: .{}", extension),
        };

        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled")
            .to_string();

        Ok(Document {
            id: doc_id.to_string(),
            title,
            content,
            url: Some(format!("file://{}", path.display())),
        })
    }

    async fn write(&self, doc_id: &str, content: &str) -> anyhow::Result<()> {
        let path = self.resolve_path(doc_id);

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            "docx" => Self::write_docx(&path, content)?,
            "txt" | "md" | "csv" => std::fs::write(&path, content)?,
            _ => anyhow::bail!("Cannot write to .{} files", extension),
        }

        Ok(())
    }

    async fn create(&self, title: &str, content: &str) -> anyhow::Result<String> {
        // Determine extension from title or default to .docx
        let has_ext = Path::new(title).extension().is_some();
        let filename = if has_ext {
            title.to_string()
        } else {
            format!("{}.docx", title)
        };

        let path = self.base_dir.join(&filename);

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("docx")
            .to_lowercase();

        match extension.as_str() {
            "docx" => Self::write_docx(&path, content)?,
            "txt" | "md" | "csv" => std::fs::write(&path, content)?,
            _ => anyhow::bail!("Cannot create .{} files", extension),
        }

        Ok(filename)
    }

    async fn search(&self, query: &str) -> anyhow::Result<Vec<DocumentRef>> {
        let mut results = Vec::new();
        let query_lower = query.to_lowercase();

        // Search for files matching the query in the base directory
        if self.base_dir.exists() {
            for entry in walkdir(&self.base_dir, 3) {
                let name = entry
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");

                if name.to_lowercase().contains(&query_lower) {
                    let ext = entry
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();

                    if matches!(ext.as_str(), "docx" | "xlsx" | "txt" | "md" | "csv") {
                        let rel_path = entry
                            .strip_prefix(&self.base_dir)
                            .unwrap_or(&entry)
                            .to_string_lossy()
                            .to_string();

                        results.push(DocumentRef {
                            id: rel_path,
                            title: name.to_string(),
                            url: Some(format!("file://{}", entry.display())),
                        });
                    }
                }

                if results.len() >= 20 {
                    break;
                }
            }
        }

        Ok(results)
    }

    async fn list(&self, folder: Option<&str>) -> anyhow::Result<Vec<DocumentRef>> {
        let scan_dir = if let Some(f) = folder {
            self.base_dir.join(f)
        } else {
            self.base_dir.clone()
        };

        let mut results = Vec::new();

        if scan_dir.exists() {
            for entry in walkdir(&scan_dir, 2) {
                let ext = entry
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                if matches!(ext.as_str(), "docx" | "xlsx" | "txt" | "md" | "csv") {
                    let name = entry
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Untitled")
                        .to_string();

                    let rel_path = entry
                        .strip_prefix(&self.base_dir)
                        .unwrap_or(&entry)
                        .to_string_lossy()
                        .to_string();

                    results.push(DocumentRef {
                        id: rel_path,
                        title: name,
                        url: Some(format!("file://{}", entry.display())),
                    });
                }

                if results.len() >= 50 {
                    break;
                }
            }
        }

        Ok(results)
    }
}

// -- Integration trait --

impl Integration for MsOfficeAdapter {
    fn id(&self) -> &str {
        "msoffice"
    }

    fn name(&self) -> &str {
        "MS Office (Local)"
    }

    fn messaging(&self) -> Option<&dyn crate::integrations::types::MessagingAdapter> {
        None
    }

    fn document(&self) -> Option<&dyn DocumentAdapter> {
        Some(self)
    }
}

// ---------------------------------------------------------------------------
// XML parsing helpers (minimal, no extra dependencies)
// ---------------------------------------------------------------------------

/// Extract text content from Office XML by stripping tags.
/// Specifically targets <w:t> elements for .docx.
fn extract_text_from_xml(xml: &str) -> String {
    let mut text = String::new();
    let mut in_text_tag = false;
    let mut chars = xml.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Read the tag name
            let mut tag = String::new();
            for c in chars.by_ref() {
                if c == '>' {
                    break;
                }
                tag.push(c);
            }

            // Check if this is a text element start/end
            let tag_trimmed = tag.trim();
            if tag_trimmed == "w:t" || tag_trimmed.starts_with("w:t ") {
                in_text_tag = true;
            } else if tag_trimmed == "/w:t" {
                in_text_tag = false;
            } else if tag_trimmed == "/w:p" || tag_trimmed.starts_with("w:br") {
                text.push('\n');
            }
        } else if in_text_tag {
            text.push(ch);
        }
    }

    text.trim().to_string()
}

/// Parse shared strings from xl/sharedStrings.xml.
fn parse_shared_strings(xml: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut in_t_tag = false;
    let mut current = String::new();
    let mut chars = xml.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut tag = String::new();
            for c in chars.by_ref() {
                if c == '>' {
                    break;
                }
                tag.push(c);
            }

            let tag_trimmed = tag.trim();
            if tag_trimmed == "t" || tag_trimmed.starts_with("t ") {
                in_t_tag = true;
                current.clear();
            } else if tag_trimmed == "/t" {
                in_t_tag = false;
                strings.push(current.clone());
            }
        } else if in_t_tag {
            current.push(ch);
        }
    }

    strings
}

/// Extract text from xlsx worksheet XML using shared strings.
fn extract_xlsx_text(sheet_xml: &str, shared_strings: &[String]) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut in_value = false;
    let mut current_value = String::new();
    let mut cell_type = String::new();
    let mut chars = sheet_xml.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut tag = String::new();
            for c in chars.by_ref() {
                if c == '>' {
                    break;
                }
                tag.push(c);
            }

            let tag_trimmed = tag.trim();
            if tag_trimmed.starts_with("row") && !tag_trimmed.starts_with("row/") {
                current_row = Vec::new();
            } else if tag_trimmed == "/row" {
                if !current_row.is_empty() {
                    rows.push(current_row.clone());
                }
            } else if tag_trimmed.starts_with("c ") {
                // Cell — check type attribute
                cell_type = if tag_trimmed.contains("t=\"s\"") {
                    "s".to_string() // shared string
                } else {
                    String::new()
                };
            } else if tag_trimmed == "v" {
                in_value = true;
                current_value.clear();
            } else if tag_trimmed == "/v" {
                in_value = false;
                let value = if cell_type == "s" {
                    // Shared string reference
                    if let Ok(idx) = current_value.parse::<usize>() {
                        shared_strings.get(idx).cloned().unwrap_or_default()
                    } else {
                        current_value.clone()
                    }
                } else {
                    current_value.clone()
                };
                current_row.push(value);
            }
        } else if in_value {
            current_value.push(ch);
        }
    }

    // Format as tab-separated values
    rows.iter()
        .map(|row| row.join("\t"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Escape XML special characters.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Simple recursive directory walk (avoids adding walkdir dependency).
fn walkdir(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut results = Vec::new();
    walkdir_inner(dir, max_depth, 0, &mut results);
    results
}

fn walkdir_inner(dir: &Path, max_depth: usize, current_depth: usize, results: &mut Vec<PathBuf>) {
    if current_depth > max_depth {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            results.push(path);
        } else if path.is_dir() && current_depth < max_depth {
            walkdir_inner(&path, max_depth, current_depth + 1, results);
        }
    }
}
