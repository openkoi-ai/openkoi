// src/evaluator/utils.rs — Helper functions for evaluation

use crate::core::types::{DimensionScore, Finding, Severity};

/// Compute weighted composite score from dimension scores.
pub fn composite_score(dimensions: &[DimensionScore]) -> f32 {
    if dimensions.is_empty() {
        return 0.5; // Conservative default when no evaluators run
    }

    let total_weight: f32 = dimensions.iter().map(|d| d.weight).sum();
    if total_weight == 0.0 {
        return dimensions.iter().map(|d| d.score).sum::<f32>() / dimensions.len() as f32;
    }

    dimensions.iter().map(|d| d.score * d.weight).sum::<f32>() / total_weight
}

/// Generate a concise suggestion from findings.
pub fn generate_suggestion(findings: &[Finding]) -> String {
    let critical: Vec<&Finding> = findings
        .iter()
        .filter(|f| f.severity == Severity::Blocker || f.severity == Severity::Important)
        .collect();

    if critical.is_empty() {
        if findings.is_empty() {
            "Maintain current direction. No issues found.".into()
        } else {
            format!(
                "Address {} minor suggestions to further improve quality.",
                findings.len()
            )
        }
    } else {
        format!(
            "Fix {} critical issues: {}",
            critical.len(),
            critical[0].title
        )
    }
}

/// Replace or add a dimension score in the list.
pub fn replace_or_add_dimension(dimensions: &mut Vec<DimensionScore>, new: DimensionScore) {
    if let Some(existing) = dimensions.iter_mut().find(|d| d.dimension == new.dimension) {
        *existing = new;
    } else {
        dimensions.push(new);
    }
}

/// Compute the ratio of changed content between two strings (0.0 = identical, 1.0 = completely different).
pub fn compute_diff_ratio(prev: &str, current: &str) -> f32 {
    let prev_lines: Vec<&str> = prev.lines().collect();
    let curr_lines: Vec<&str> = current.lines().collect();

    let total = prev_lines.len().max(curr_lines.len());
    if total == 0 {
        return 0.0;
    }

    let mut changed = 0usize;
    let max_len = prev_lines.len().max(curr_lines.len());
    for i in 0..max_len {
        let prev_line = prev_lines.get(i).copied().unwrap_or("");
        let curr_line = curr_lines.get(i).copied().unwrap_or("");
        if prev_line != curr_line {
            changed += 1;
        }
    }

    changed as f32 / total as f32
}

/// Truncate text for evaluation prompts.
pub fn truncate_for_eval(text: &str, max_chars: usize) -> &str {
    if text.len() <= max_chars {
        return text;
    }
    match text.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &text[..byte_idx],
        None => text,
    }
}
