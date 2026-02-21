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
}
