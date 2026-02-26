// src/learner/dedup.rs — Learning deduplication

use super::types::Learning;
use crate::memory::embeddings::text_similarity;
use crate::memory::StoreHandle;

/// Deduplicate new learnings against what's already in the database.
/// If a similar learning exists, reinforce it instead of adding a duplicate.
pub async fn deduplicate(learnings: &mut Vec<Learning>, store: &StoreHandle) {
    let existing = store.query_all_learnings().await.unwrap_or_default();

    let mut to_keep = Vec::new();
    for new in learnings.drain(..) {
        let mut is_dup = false;
        for old in &existing {
            if text_similarity(&new.content, &old.content) > 0.8 {
                // Reinforce existing instead of adding new
                let _ = store.reinforce_learning(old.id.clone()).await;
                is_dup = true;
                break;
            }
        }
        if !is_dup {
            to_keep.push(new);
        }
    }
    learnings.extend(to_keep);
}
