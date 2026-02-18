// src/learner/dedup.rs — Learning deduplication

use super::types::Learning;
use crate::memory::embeddings::text_similarity;
use crate::memory::store::Store;

/// Deduplicate new learnings against what's already in the database.
/// If a similar learning exists, reinforce it instead of adding a duplicate.
pub fn deduplicate(learnings: &mut Vec<Learning>, store: &Store) {
    let existing = store.query_all_learnings().unwrap_or_default();

    learnings.retain(|new| {
        for old in &existing {
            if text_similarity(&new.content, &old.content) > 0.8 {
                // Reinforce existing instead of adding new
                let _ = store.reinforce_learning(&old.id);
                return false;
            }
        }
        true
    });
}
