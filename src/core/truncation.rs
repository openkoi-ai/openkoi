// src/core/truncation.rs — Tool output truncation
//
// Limits tool outputs to prevent context blowup.
// Full output is saved to disk for reference.

use crate::infra::paths;
use std::path::PathBuf;

/// Maximum lines in a tool output before truncation.
const MAX_LINES: usize = 2000;

/// Maximum bytes in a tool output before truncation.
const MAX_BYTES: usize = 50 * 1024; // 50KB

/// Retention period for saved full outputs.
const _RETENTION_DAYS: u64 = 7;

/// Result of a truncation operation.
pub struct TruncationResult {
    /// The (possibly truncated) content.
    pub content: String,
    /// Whether the content was truncated.
    pub was_truncated: bool,
    /// Path to the full output file, if truncation occurred.
    pub full_output_path: Option<PathBuf>,
    /// Original size in bytes.
    pub original_bytes: usize,
    /// Original number of lines.
    pub original_lines: usize,
}

/// Truncate tool output if it exceeds limits.
///
/// When truncated, the full output is saved to `~/.openkoi/tool-output/{hash}.txt`
/// and the returned content includes a note about the truncation.
pub fn truncate_tool_output(content: &str) -> TruncationResult {
    let original_bytes = content.len();
    let original_lines = content.lines().count();

    if original_bytes <= MAX_BYTES && original_lines <= MAX_LINES {
        return TruncationResult {
            content: content.to_string(),
            was_truncated: false,
            full_output_path: None,
            original_bytes,
            original_lines,
        };
    }

    // Save full output to disk
    let hash = simple_hash(content);
    let full_path = save_full_output(&hash, content);

    // Truncate by lines first, then by bytes
    let truncated = if original_lines > MAX_LINES {
        let lines: Vec<&str> = content.lines().take(MAX_LINES).collect();
        lines.join("\n")
    } else {
        content.to_string()
    };

    let truncated = if truncated.len() > MAX_BYTES {
        truncated[..MAX_BYTES].to_string()
    } else {
        truncated
    };

    let note = format!(
        "\n\n[Output truncated: {} bytes, {} lines. Full output saved to {}]",
        original_bytes,
        original_lines,
        full_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "disk".into())
    );

    TruncationResult {
        content: format!("{}{}", truncated, note),
        was_truncated: true,
        full_output_path: full_path,
        original_bytes,
        original_lines,
    }
}

/// Simple hash for deduplication of saved outputs.
fn simple_hash(content: &str) -> String {
    // Use a basic checksum — not cryptographic, just for filename uniqueness.
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for byte in content.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    format!("{:016x}", hash)
}

/// Save full output to disk, returning the file path.
fn save_full_output(hash: &str, content: &str) -> Option<PathBuf> {
    let dir = paths::data_dir().join("tool-output");
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    let path = dir.join(format!("{}.txt", hash));
    if std::fs::write(&path, content).is_ok() {
        Some(path)
    } else {
        None
    }
}

/// Clean up old tool output files beyond the retention period.
pub fn cleanup_old_outputs() -> u32 {
    let dir = paths::data_dir().join("tool-output");
    if !dir.exists() {
        return 0;
    }

    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs(_RETENTION_DAYS * 24 * 3600);

    let mut removed = 0u32;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if let Ok(modified) = metadata.modified() {
                    if modified < cutoff {
                        let _ = std::fs::remove_file(entry.path());
                        removed += 1;
                    }
                }
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_output_not_truncated() {
        let result = truncate_tool_output("hello world");
        assert!(!result.was_truncated);
        assert_eq!(result.content, "hello world");
        assert!(result.full_output_path.is_none());
    }

    #[test]
    fn test_long_bytes_truncated() {
        let long = "x".repeat(60_000);
        let result = truncate_tool_output(&long);
        assert!(result.was_truncated);
        assert!(result.content.len() < 60_000);
        assert!(result.content.contains("[Output truncated:"));
        assert_eq!(result.original_bytes, 60_000);
    }

    #[test]
    fn test_many_lines_truncated() {
        let many_lines: String = (0..3000).map(|i| format!("line {}\n", i)).collect();
        let result = truncate_tool_output(&many_lines);
        assert!(result.was_truncated);
        assert!(result.original_lines >= 3000);
        // Content should have at most MAX_LINES actual data lines
        let content_lines = result.content.lines().count();
        // +2 for the truncation note lines
        assert!(content_lines <= MAX_LINES + 3);
    }

    #[test]
    fn test_exactly_at_limit_not_truncated() {
        let content = "x".repeat(MAX_BYTES);
        let result = truncate_tool_output(&content);
        assert!(!result.was_truncated);
    }

    #[test]
    fn test_simple_hash_deterministic() {
        let h1 = simple_hash("hello");
        let h2 = simple_hash("hello");
        let h3 = simple_hash("world");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }
}
