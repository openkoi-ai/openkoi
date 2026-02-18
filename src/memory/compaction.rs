// src/memory/compaction.rs — Context compaction

use crate::core::token_optimizer::estimate_tokens;
use crate::provider::Message;

/// Compact a message history to fit within a token budget.
///
/// Splits into old (to summarize) and recent (to keep intact).
/// Old messages are replaced with a summary.
pub fn compact(messages: &[Message], max_tokens: u32) -> Vec<Message> {
    let total: u32 = messages.iter().map(|m| estimate_tokens(&m.content)).sum();

    if total <= max_tokens {
        return messages.to_vec();
    }

    // Split: summarize old 2/3, keep recent 1/3
    let split_point = messages.len() * 2 / 3;
    if split_point == 0 {
        return messages.to_vec();
    }

    let (old, recent) = messages.split_at(split_point);

    // Build a simple summary of old messages
    let summary = summarize_messages(old);

    let mut compacted = vec![Message::system(format!("[Compacted history]\n{}", summary))];
    compacted.extend_from_slice(recent);
    compacted
}

/// Create a concise summary of messages.
pub(crate) fn summarize_messages(messages: &[Message]) -> String {
    let mut summary = String::new();
    for msg in messages {
        let role = format!("{:?}", msg.role);
        let content = if msg.content.len() > 200 {
            format!("{}...", &msg.content[..200])
        } else {
            msg.content.clone()
        };
        summary.push_str(&format!("[{}] {}\n", role, content));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg_user(s: &str) -> Message {
        Message::user(s)
    }
    fn msg_asst(s: &str) -> Message {
        Message::assistant(s)
    }

    #[test]
    fn test_compact_within_budget() {
        let msgs = vec![msg_user("hello"), msg_asst("hi")];
        let result = compact(&msgs, 100_000);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "hello");
    }

    #[test]
    fn test_compact_over_budget() {
        // Each message ~50 chars → ~13 tokens, 10 msgs → ~130 tokens
        let msgs: Vec<Message> = (0..10)
            .map(|i| {
                if i % 2 == 0 {
                    msg_user(&format!(
                        "User message number {i} with some padding text here"
                    ))
                } else {
                    msg_asst(&format!(
                        "Assistant reply number {i} with padding text here"
                    ))
                }
            })
            .collect();
        let result = compact(&msgs, 20); // very tight budget
                                         // Should be compacted: summary + last ~3 messages
        assert!(result.len() < msgs.len());
        // First message should be a system summary
        assert!(result[0].content.contains("[Compacted history]"));
    }

    #[test]
    fn test_compact_preserves_recent() {
        let msgs: Vec<Message> = (0..9)
            .map(|i| msg_user(&format!("Message {i} with enough content to use tokens")))
            .collect();
        let result = compact(&msgs, 10); // tight budget forces compaction
                                         // Recent 1/3 (3 messages) should be preserved
        let last_original = &msgs[msgs.len() - 1].content;
        let last_compacted = &result[result.len() - 1].content;
        assert_eq!(last_original, last_compacted);
    }

    #[test]
    fn test_compact_empty() {
        let result = compact(&[], 1000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_compact_single_message() {
        let msgs = vec![msg_user("only one")];
        // split_point = 1 * 2/3 = 0, so returns as-is
        let result = compact(&msgs, 1);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_summarize_messages_truncation() {
        let long_content = "x".repeat(300);
        let msgs = vec![msg_user(&long_content)];
        let summary = summarize_messages(&msgs);
        // Should be truncated to 200 chars + "..."
        assert!(summary.contains("..."));
        assert!(summary.len() < 300);
    }

    #[test]
    fn test_summarize_messages_short() {
        let msgs = vec![msg_user("hello"), msg_asst("world")];
        let summary = summarize_messages(&msgs);
        assert!(summary.contains("hello"));
        assert!(summary.contains("world"));
    }

    #[test]
    fn test_summarize_messages_role_labels() {
        let msgs = vec![msg_user("u"), msg_asst("a")];
        let summary = summarize_messages(&msgs);
        assert!(summary.contains("[User]"));
        assert!(summary.contains("[Assistant]"));
    }
}
