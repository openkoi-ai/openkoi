// src/evaluator/calibration.rs — Score normalizer and calibrator

use super::composite_score;
use crate::core::types::{DimensionScore, Evaluation};
use std::collections::HashMap;

/// Score normalizer that ensures scores from different evaluator types
/// (LLM-based, test runner, static analysis) are comparable.
pub struct ScoreCalibrator {
    /// Running statistics per evaluator source (for z-score normalization).
    history: HashMap<String, ScoreHistory>,
}

/// Tracks rolling score statistics for a single evaluator source.
struct ScoreHistory {
    scores: Vec<f32>,
    max_tracked: usize,
}

impl ScoreHistory {
    fn new(max: usize) -> Self {
        Self {
            scores: Vec::new(),
            max_tracked: max,
        }
    }

    fn record(&mut self, score: f32) {
        if self.scores.len() >= self.max_tracked {
            self.scores.remove(0);
        }
        self.scores.push(score);
    }

    fn mean(&self) -> f32 {
        if self.scores.is_empty() {
            return 0.5;
        }
        self.scores.iter().sum::<f32>() / self.scores.len() as f32
    }

    fn std_dev(&self) -> f32 {
        if self.scores.len() < 2 {
            return 0.15; // Default standard deviation
        }
        let mean = self.mean();
        let variance =
            self.scores.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / self.scores.len() as f32;
        variance.sqrt()
    }

    fn count(&self) -> usize {
        self.scores.len()
    }
}

impl Default for ScoreCalibrator {
    fn default() -> Self {
        Self::new()
    }
}

impl ScoreCalibrator {
    pub fn new() -> Self {
        Self {
            history: HashMap::new(),
        }
    }

    /// Record a raw score from an evaluator source, updating calibration stats.
    pub fn record(&mut self, source: &str, raw_score: f32) {
        self.history
            .entry(source.to_string())
            .or_insert_with(|| ScoreHistory::new(100))
            .record(raw_score);
    }

    /// Normalize a raw score from a given source using z-score normalization.
    pub fn normalize(&self, source: &str, raw_score: f32) -> f32 {
        let history = match self.history.get(source) {
            Some(h) if h.count() >= 5 => h,
            _ => return raw_score, // Not enough data; passthrough
        };

        let mean = history.mean();
        let std_dev = history.std_dev();

        if std_dev < 0.01 {
            return raw_score;
        }

        // Z-score: how many std deviations from the mean
        let z = (raw_score - mean) / std_dev;

        // Map z-score to 0.0-1.0 using logistic function centered at 0.5
        let normalized = 1.0 / (1.0 + (-z * 1.5).exp());

        normalized.clamp(0.0, 1.0)
    }

    /// Calibrate an entire Evaluation, normalizing all dimension scores.
    pub fn calibrate_evaluation(&mut self, eval: &mut Evaluation, source: &str) {
        for dim in &mut eval.dimensions {
            self.record(source, dim.score);
            dim.score = self.normalize(source, dim.score);
        }
        // Recompute composite from calibrated dimension scores
        eval.score = composite_score(&eval.dimensions);
    }

    /// Check cross-evaluator consistency.
    pub fn consistency_spread(dimensions: &[DimensionScore]) -> f32 {
        if dimensions.len() < 2 {
            return 0.0;
        }
        let min = dimensions
            .iter()
            .map(|d| d.score)
            .fold(f32::INFINITY, f32::min);
        let max = dimensions
            .iter()
            .map(|d| d.score)
            .fold(f32::NEG_INFINITY, f32::max);
        max - min
    }

    pub fn stats(&self, source: &str) -> Option<CalibrationStats> {
        self.history.get(source).map(|h| CalibrationStats {
            mean: h.mean(),
            std_dev: h.std_dev(),
            count: h.count(),
        })
    }
}

/// Summary of calibration statistics for an evaluator source.
#[derive(Debug, Clone)]
pub struct CalibrationStats {
    pub mean: f32,
    pub std_dev: f32,
    pub count: usize,
}
