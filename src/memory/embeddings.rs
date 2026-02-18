// src/memory/embeddings.rs — Vector operations

/// Compute cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

/// Normalize a vector to unit length.
pub fn normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Simple text similarity based on word overlap (Jaccard coefficient).
/// Used for deduplication before embeddings are available.
pub fn text_similarity(a: &str, b: &str) -> f32 {
    let words_a: std::collections::HashSet<String> =
        a.split_whitespace().map(|w| w.to_lowercase()).collect();
    let words_b: std::collections::HashSet<String> =
        b.split_whitespace().map(|w| w.to_lowercase()).collect();

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        return 0.0;
    }

    intersection as f32 / union as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_normalize() {
        let mut v = vec![3.0, 4.0];
        normalize(&mut v);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_normalize_zero_vector() {
        let mut v = vec![0.0, 0.0];
        normalize(&mut v);
        assert_eq!(v, vec![0.0, 0.0]); // unchanged
    }

    #[test]
    fn test_text_similarity_identical() {
        let s = text_similarity("hello world", "hello world");
        assert!((s - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_text_similarity_disjoint() {
        let s = text_similarity("alpha beta", "gamma delta");
        assert_eq!(s, 0.0);
    }

    #[test]
    fn test_text_similarity_partial() {
        let s = text_similarity("hello world foo", "hello world bar");
        // intersection = {hello, world} = 2, union = {hello, world, foo, bar} = 4
        assert!((s - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_text_similarity_both_empty() {
        assert_eq!(text_similarity("", ""), 1.0);
    }
}
