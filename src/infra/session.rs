// src/infra/session.rs — Session lifecycle management

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::paths;

/// Status of a session in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Active,
    Paused,
    Ended,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Paused => write!(f, "paused"),
            Self::Ended => write!(f, "ended"),
        }
    }
}

impl std::str::FromStr for SessionStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "paused" => Self::Paused,
            "ended" => Self::Ended,
            _ => Self::Active,
        })
    }
}

/// A single entry in the session transcript (chat or task).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub channel: String,
    pub model_provider: Option<String>,
    pub model_id: Option<String>,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub total_tokens: u32,
    pub total_cost_usd: f64,
    pub transcript_path: Option<String>,
}

impl Session {
    /// Create a new active session for the given channel ("cli", "chat", "daemon").
    pub fn new(channel: &str) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            channel: channel.to_string(),
            model_provider: None,
            model_id: None,
            status: SessionStatus::Active,
            created_at: now,
            updated_at: now,
            ended_at: None,
            total_tokens: 0,
            total_cost_usd: 0.0,
            transcript_path: None,
        }
    }

    /// Create a session with a specific model reference attached.
    pub fn with_model(mut self, provider: &str, model: &str) -> Self {
        self.model_provider = Some(provider.to_string());
        self.model_id = Some(model.to_string());
        self
    }

    /// Mark this session as ended.
    pub fn end(&mut self) {
        self.status = SessionStatus::Ended;
        self.ended_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Update running totals after a task completes.
    pub fn accumulate(&mut self, tokens: u32, cost: f64) {
        self.total_tokens += tokens;
        self.total_cost_usd += cost;
        self.updated_at = Utc::now();
    }

    /// Return the session directory under the data dir.
    /// Layout: `<data_dir>/sessions/<session_id>/`
    pub fn session_dir(&self) -> std::path::PathBuf {
        paths::sessions_dir().join(&self.id)
    }

    /// Return the transcript file path for this session.
    pub fn transcript_file(&self) -> std::path::PathBuf {
        self.session_dir().join("transcript.jsonl")
    }

    /// Return the output file path for a given task within this session.
    pub fn task_output_file(&self, task_id: &str) -> std::path::PathBuf {
        self.session_dir().join(format!("{}.md", task_id))
    }

    /// Ensure the session directory exists on disk.
    pub fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.session_dir())
    }

    /// Append a transcript entry to the session's transcript file.
    pub fn append_transcript(&self, entry: &TranscriptEntry) -> anyhow::Result<()> {
        use std::io::Write;

        self.ensure_dir()?;
        let path = self.transcript_file();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let line = serde_json::to_string(entry)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    /// Read all transcript entries from the session file.
    pub fn read_transcript(&self) -> anyhow::Result<Vec<TranscriptEntry>> {
        let path = self.transcript_file();
        if !path.exists() {
            return Ok(vec![]);
        }
        let content = std::fs::read_to_string(&path)?;
        let mut entries = Vec::new();
        for line in content.lines() {
            if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(line) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Save task output content to a file within the session directory.
    pub fn save_task_output(&self, task_id: &str, content: &str) -> anyhow::Result<String> {
        self.ensure_dir()?;
        let path = self.task_output_file(task_id);
        std::fs::write(&path, content)?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Read task output content from the session directory.
    pub fn read_task_output(&self, task_id: &str) -> anyhow::Result<String> {
        let path = self.task_output_file(task_id);
        Ok(std::fs::read_to_string(&path)?)
    }

    /// Build a conversation summary for session resume.
    ///
    /// Strategy: compressed summary of older messages + last N raw messages.
    /// Returns (summary_text, recent_entries) where summary_text is a condensed
    /// version of older exchanges and recent_entries are the last `keep_recent` messages.
    pub fn build_resume_context(
        &self,
        keep_recent: usize,
        max_summary_chars: usize,
    ) -> anyhow::Result<(String, Vec<TranscriptEntry>)> {
        let entries = self.read_transcript()?;
        if entries.is_empty() {
            return Ok((String::new(), vec![]));
        }

        let split_at = entries.len().saturating_sub(keep_recent);
        let (older, recent) = entries.split_at(split_at);

        // Build compressed summary from older messages
        let mut summary = String::new();
        for entry in older {
            let role_tag = match entry.role.as_str() {
                "user" => "Human",
                "assistant" => "Assistant",
                _ => &entry.role,
            };
            // Truncate each older message to keep the summary compact
            let truncated = if entry.content.len() > 200 {
                format!("{}...", &entry.content[..200])
            } else {
                entry.content.clone()
            };
            summary.push_str(&format!("[{}] {}\n", role_tag, truncated));
        }

        // Cap the summary to max_summary_chars, keeping the tail (most recent of the old)
        if summary.len() > max_summary_chars {
            let start = summary.len() - max_summary_chars;
            // Find the next newline after the cut point to avoid breaking mid-line
            let start = summary[start..]
                .find('\n')
                .map(|i| start + i + 1)
                .unwrap_or(start);
            summary = format!(
                "[...earlier conversation omitted...]\n{}",
                &summary[start..]
            );
        }

        Ok((summary, recent.to_vec()))
    }
}
