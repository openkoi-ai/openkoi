// src/evaluator/parser.rs — Parse LLM evaluation responses into structured scores

use crate::core::types::*;
use crate::skills::types::DimensionDef;

/// Parsed evaluation result from LLM response text.
pub struct ParsedEval {
    pub dimensions: Vec<DimensionScore>,
    pub findings: Vec<Finding>,
    pub suggestion: String,
}

/// Parse an LLM evaluation response into structured scores and findings.
///
/// Expected format:
/// ```text
/// SCORES:
/// dimension_name: 0.85
/// ...
/// FINDINGS:
/// - [BLOCKER] title: description
/// - [IMPORTANT] title: description
/// SUGGESTION: brief guidance
/// ```
pub fn parse_eval_response(response: &str, expected_dimensions: &[DimensionDef]) -> ParsedEval {
    let mut dimensions = Vec::new();
    let mut findings = Vec::new();
    let mut suggestion = String::new();

    let mut section = Section::None;

    for line in response.lines() {
        let trimmed = line.trim();

        // Section headers
        if trimmed.starts_with("SCORES:") || trimmed.starts_with("## Scores") {
            section = Section::Scores;
            continue;
        }
        if trimmed.starts_with("FINDINGS:") || trimmed.starts_with("## Findings") {
            section = Section::Findings;
            continue;
        }
        if trimmed.starts_with("SUGGESTION:") || trimmed.starts_with("## Suggestion") {
            section = Section::Suggestion;
            let rest = trimmed
                .strip_prefix("SUGGESTION:")
                .or_else(|| trimmed.strip_prefix("## Suggestion"))
                .unwrap_or("")
                .trim();
            if !rest.is_empty() {
                suggestion = rest.to_string();
            }
            continue;
        }

        match section {
            Section::Scores => {
                if let Some((name, score)) = parse_score_line(trimmed) {
                    let weight = expected_dimensions
                        .iter()
                        .find(|d| d.name.eq_ignore_ascii_case(&name))
                        .map(|d| d.weight)
                        .unwrap_or(1.0 / expected_dimensions.len().max(1) as f32);

                    dimensions.push(DimensionScore {
                        dimension: name,
                        score,
                        weight,
                    });
                }
            }
            Section::Findings => {
                if let Some(finding) = parse_finding_line(trimmed, findings.len()) {
                    findings.push(finding);
                }
            }
            Section::Suggestion => {
                if !trimmed.is_empty() {
                    if !suggestion.is_empty() {
                        suggestion.push(' ');
                    }
                    suggestion.push_str(trimmed);
                }
            }
            Section::None => {}
        }
    }

    // If no dimensions were parsed, create defaults from expected
    if dimensions.is_empty() && !expected_dimensions.is_empty() {
        for dim in expected_dimensions {
            dimensions.push(DimensionScore {
                dimension: dim.name.clone(),
                score: 0.75, // Default moderate score
                weight: dim.weight,
            });
        }
    }

    ParsedEval {
        dimensions,
        findings,
        suggestion,
    }
}

enum Section {
    None,
    Scores,
    Findings,
    Suggestion,
}

/// Parse a line like "correctness: 0.85" or "- correctness: 0.85"
pub(crate) fn parse_score_line(line: &str) -> Option<(String, f32)> {
    let line = line.trim_start_matches('-').trim();
    let (name, score_str) = line.split_once(':')?;
    let name = name.trim().to_string();
    let score: f32 = score_str.trim().parse().ok()?;

    if !(0.0..=1.0).contains(&score) {
        return None;
    }

    Some((name, score))
}

/// Parse a finding line like "- [BLOCKER] title: description"
pub(crate) fn parse_finding_line(line: &str, index: usize) -> Option<Finding> {
    let line = line.trim_start_matches('-').trim();
    if line.is_empty() {
        return None;
    }

    // Try to parse [SEVERITY] title: description
    if line.starts_with('[') {
        if let Some(end_bracket) = line.find(']') {
            let severity_str = &line[1..end_bracket];
            let rest = line[end_bracket + 1..].trim();

            let severity = match severity_str.to_uppercase().as_str() {
                "BLOCKER" => Severity::Blocker,
                "IMPORTANT" | "MAJOR" | "HIGH" => Severity::Important,
                "SUGGESTION" | "MINOR" | "LOW" => Severity::Suggestion,
                _ => Severity::Suggestion,
            };

            let (title, description) = rest
                .split_once(':')
                .map(|(t, d)| (t.trim().to_string(), d.trim().to_string()))
                .unwrap_or_else(|| (rest.to_string(), String::new()));

            return Some(Finding {
                id: format!("F{}", index + 1),
                severity,
                dimension: String::new(),
                title,
                description,
                location: None,
                fix: None,
            });
        }
    }

    // Fallback: treat entire line as a suggestion
    Some(Finding {
        id: format!("F{}", index + 1),
        severity: Severity::Suggestion,
        dimension: String::new(),
        title: line.to_string(),
        description: String::new(),
        location: None,
        fix: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dim(name: &str, weight: f32) -> DimensionDef {
        DimensionDef {
            name: name.to_string(),
            weight,
            description: String::new(),
        }
    }

    // ─── parse_score_line tests ─────────────────────────────────

    #[test]
    fn test_parse_score_line_basic() {
        let (name, score) = parse_score_line("correctness: 0.85").unwrap();
        assert_eq!(name, "correctness");
        assert!((score - 0.85).abs() < 0.001);
    }

    #[test]
    fn test_parse_score_line_with_dash() {
        let (name, score) = parse_score_line("- style: 0.70").unwrap();
        assert_eq!(name, "style");
        assert!((score - 0.70).abs() < 0.001);
    }

    #[test]
    fn test_parse_score_line_zero() {
        let (name, score) = parse_score_line("security: 0.0").unwrap();
        assert_eq!(name, "security");
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_parse_score_line_one() {
        let (name, score) = parse_score_line("tests: 1.0").unwrap();
        assert_eq!(name, "tests");
        assert_eq!(score, 1.0);
    }

    #[test]
    fn test_parse_score_line_out_of_range() {
        assert!(parse_score_line("broken: 1.5").is_none());
        assert!(parse_score_line("broken: -0.1").is_none());
    }

    #[test]
    fn test_parse_score_line_not_a_number() {
        assert!(parse_score_line("name: not-a-score").is_none());
    }

    #[test]
    fn test_parse_score_line_no_colon() {
        assert!(parse_score_line("just a sentence").is_none());
    }

    // ─── parse_finding_line tests ───────────────────────────────

    #[test]
    fn test_parse_finding_blocker() {
        let f = parse_finding_line(
            "[BLOCKER] Missing null check: could panic on empty input",
            0,
        )
        .unwrap();
        assert_eq!(f.id, "F1");
        assert_eq!(f.severity, Severity::Blocker);
        assert_eq!(f.title, "Missing null check");
        assert_eq!(f.description, "could panic on empty input");
    }

    #[test]
    fn test_parse_finding_important() {
        let f = parse_finding_line("[IMPORTANT] No error handling: unwrap used", 1).unwrap();
        assert_eq!(f.id, "F2");
        assert_eq!(f.severity, Severity::Important);
    }

    #[test]
    fn test_parse_finding_major() {
        let f = parse_finding_line("[MAJOR] Buffer overflow risk", 0).unwrap();
        assert_eq!(f.severity, Severity::Important); // MAJOR maps to Important
    }

    #[test]
    fn test_parse_finding_suggestion() {
        let f = parse_finding_line("[SUGGESTION] Consider using iterators", 2).unwrap();
        assert_eq!(f.severity, Severity::Suggestion);
    }

    #[test]
    fn test_parse_finding_minor() {
        let f = parse_finding_line("[MINOR] Variable naming: use snake_case", 0).unwrap();
        assert_eq!(f.severity, Severity::Suggestion); // MINOR maps to Suggestion
    }

    #[test]
    fn test_parse_finding_no_description() {
        let f = parse_finding_line("[BLOCKER] Just a title", 0).unwrap();
        assert_eq!(f.title, "Just a title");
        assert!(f.description.is_empty());
    }

    #[test]
    fn test_parse_finding_with_dash_prefix() {
        let f = parse_finding_line("- [BLOCKER] Title: Desc", 0).unwrap();
        assert_eq!(f.severity, Severity::Blocker);
        assert_eq!(f.title, "Title");
    }

    #[test]
    fn test_parse_finding_fallback() {
        let f = parse_finding_line("Some plain text finding", 5).unwrap();
        assert_eq!(f.id, "F6");
        assert_eq!(f.severity, Severity::Suggestion);
        assert_eq!(f.title, "Some plain text finding");
    }

    #[test]
    fn test_parse_finding_empty_line() {
        assert!(parse_finding_line("", 0).is_none());
    }

    #[test]
    fn test_parse_finding_whitespace() {
        assert!(parse_finding_line("   ", 0).is_none());
    }

    // ─── parse_eval_response tests ──────────────────────────────

    #[test]
    fn test_parse_eval_full_response() {
        let response = "\
SCORES:
correctness: 0.90
style: 0.75
security: 0.60
FINDINGS:
- [BLOCKER] SQL injection: User input not sanitized
- [SUGGESTION] Consider using constants
SUGGESTION: Fix the SQL injection vulnerability first.";

        let dims = vec![
            dim("correctness", 0.4),
            dim("style", 0.3),
            dim("security", 0.3),
        ];
        let result = parse_eval_response(response, &dims);

        assert_eq!(result.dimensions.len(), 3);
        assert!((result.dimensions[0].score - 0.90).abs() < 0.001);
        assert!((result.dimensions[0].weight - 0.4).abs() < 0.001);
        assert!((result.dimensions[1].score - 0.75).abs() < 0.001);
        assert!((result.dimensions[2].score - 0.60).abs() < 0.001);

        assert_eq!(result.findings.len(), 2);
        assert_eq!(result.findings[0].severity, Severity::Blocker);
        assert_eq!(result.findings[1].severity, Severity::Suggestion);

        assert!(result.suggestion.contains("SQL injection"));
    }

    #[test]
    fn test_parse_eval_empty_response() {
        let dims = vec![dim("correctness", 0.5), dim("style", 0.5)];
        let result = parse_eval_response("", &dims);

        // Should create defaults from expected dimensions
        assert_eq!(result.dimensions.len(), 2);
        assert!((result.dimensions[0].score - 0.75).abs() < 0.001);
        assert!(result.findings.is_empty());
    }

    #[test]
    fn test_parse_eval_no_expected_dims() {
        let result = parse_eval_response("", &[]);
        assert!(result.dimensions.is_empty());
    }

    #[test]
    fn test_parse_eval_alternative_headers() {
        let response = "\
## Scores
correctness: 0.85
## Findings
- [IMPORTANT] Missing docs
## Suggestion
Add documentation.";

        let dims = vec![dim("correctness", 1.0)];
        let result = parse_eval_response(response, &dims);

        assert_eq!(result.dimensions.len(), 1);
        assert!((result.dimensions[0].score - 0.85).abs() < 0.001);
        assert_eq!(result.findings.len(), 1);
        assert!(result.suggestion.contains("documentation"));
    }

    #[test]
    fn test_parse_eval_unknown_dimension_weight() {
        let response = "\
SCORES:
mystery_dim: 0.80";

        let dims = vec![dim("correctness", 0.5), dim("style", 0.5)];
        let result = parse_eval_response(response, &dims);

        // Unknown dimension gets 1/len weight
        assert_eq!(result.dimensions.len(), 1);
        assert!((result.dimensions[0].weight - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_parse_eval_multiline_suggestion() {
        let response = "\
SUGGESTION: First line.
Second line continues.
Third line too.
SCORES:
x: 0.50";

        let dims = vec![dim("x", 1.0)];
        let result = parse_eval_response(response, &dims);

        // Suggestion captures lines until next section header
        assert!(result.suggestion.contains("First line."));
        // After SCORES: header, parsing switches to scores section
        assert_eq!(result.dimensions.len(), 1);
    }

    #[test]
    fn test_parse_eval_case_insensitive_dimension_match() {
        let response = "\
SCORES:
Correctness: 0.90";

        let dims = vec![dim("correctness", 0.5)];
        let result = parse_eval_response(response, &dims);

        // Should match case-insensitively
        assert_eq!(result.dimensions.len(), 1);
        assert!((result.dimensions[0].weight - 0.5).abs() < 0.001);
    }
}
