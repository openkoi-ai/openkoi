// src/util.rs — Shared utility functions

/// Truncate a string for display/logging (UTF-8 safe).
///
/// Returns a substring of at most `max_len` bytes, ensuring the cut
/// point falls on a valid UTF-8 character boundary.
pub fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Truncate a string for CLI display, appending an ellipsis (`…`) when shortened.
///
/// Returns an owned `String`. Uses `floor_char_boundary` for safe UTF-8 splitting.
pub fn truncate_display(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let boundary = s.floor_char_boundary(max.saturating_sub(1));
        format!("{}\u{2026}", &s[..boundary])
    }
}

/// Render a progress bar of `width` characters using block elements.
///
/// `ratio` should be between 0.0 and 1.0.
pub fn progress_bar(ratio: f64, width: usize) -> String {
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(empty))
}

/// Format a timestamp string as a human-friendly relative or compact display.
///
/// - Within the last minute: "just now"
/// - Within the last hour: "X min ago"
/// - Within the last 24h: "X hours ago"
/// - Within the last 7 days: "X days ago"
/// - Older: "YYYY-MM-DD HH:MM"
///
/// Accepts RFC 3339, "YYYY-MM-DD HH:MM:SS", or raw strings (returned as-is on parse failure).
pub fn format_relative_time(ts: &str) -> String {
    use chrono::{NaiveDateTime, Utc};

    let parsed = chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|ndt| ndt.and_utc())
        });

    let dt = match parsed {
        Some(dt) => dt,
        None => {
            // Best-effort: return truncated raw string
            return ts[..ts.len().min(19)].to_string();
        }
    };

    let now = Utc::now();
    let diff = now.signed_duration_since(dt);

    if diff.num_seconds() < 0 {
        // Future timestamps — just show compact format
        return dt.format("%Y-%m-%d %H:%M").to_string();
    }

    let secs = diff.num_seconds();
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        let mins = secs / 60;
        format!("{} min ago", mins)
    } else if secs < 86400 {
        let hours = secs / 3600;
        if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", hours)
        }
    } else if secs < 604800 {
        let days = secs / 86400;
        if days == 1 {
            "yesterday".to_string()
        } else {
            format!("{} days ago", days)
        }
    } else {
        dt.format("%Y-%m-%d %H:%M").to_string()
    }
}

/// Format a timestamp as compact "YYYY-MM-DD HH:MM" display.
///
/// Accepts RFC 3339 or "YYYY-MM-DD HH:MM:SS". Falls back to truncated raw string.
pub fn format_timestamp(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| ts[..ts.len().min(19)].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate_str("hello world", 5), "hello");
    }

    #[test]
    fn test_truncate_multibyte() {
        // "café" is 5 bytes (é = 2 bytes), truncating at 4 should not split é
        let s = "café";
        let t = truncate_str(s, 4);
        assert_eq!(t, "caf");
    }

    #[test]
    fn test_truncate_empty() {
        assert_eq!(truncate_str("", 5), "");
    }

    #[test]
    fn test_truncate_zero_max() {
        assert_eq!(truncate_str("hello", 0), "");
    }

    #[test]
    fn test_format_timestamp_rfc3339() {
        let result = format_timestamp("2024-01-15T10:30:00Z");
        assert_eq!(result, "2024-01-15 10:30");
    }

    #[test]
    fn test_format_timestamp_fallback() {
        let result = format_timestamp("not-a-timestamp-at-all");
        // Truncates to 19 chars (same width as "YYYY-MM-DD HH:MM:SS")
        assert_eq!(result, "not-a-timestamp-at-");
    }

    #[test]
    fn test_format_relative_time_just_now() {
        let now = chrono::Utc::now().to_rfc3339();
        assert_eq!(format_relative_time(&now), "just now");
    }

    #[test]
    fn test_format_relative_time_old() {
        let result = format_relative_time("2020-01-01T00:00:00Z");
        assert_eq!(result, "2020-01-01 00:00");
    }

    #[test]
    fn test_format_relative_time_unparseable() {
        let result = format_relative_time("garbage");
        assert_eq!(result, "garbage");
    }

    #[test]
    fn test_format_relative_time_sqlite_format() {
        // SQLite datetime('now') format: "2024-01-15 10:30:00"
        let result = format_relative_time("2020-06-15 12:00:00");
        assert_eq!(result, "2020-06-15 12:00");
    }
}
