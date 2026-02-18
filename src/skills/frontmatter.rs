// src/skills/frontmatter.rs — YAML frontmatter parser for SKILL.md files

use super::types::*;

/// Parse a SKILL.md file into its frontmatter and body.
///
/// Format:
/// ```text
/// ---
/// name: skill-name
/// kind: task | evaluator
/// description: ...
/// metadata:
///   categories: [...]
///   dimensions: [...]
/// ---
/// # Body content (markdown)
/// ```
pub fn parse_skill_md(content: &str) -> anyhow::Result<(RawFrontmatter, String)> {
    // Find frontmatter delimiters
    if !content.starts_with("---") {
        return Err(anyhow::anyhow!(
            "SKILL.md must start with --- (YAML frontmatter)"
        ));
    }

    let after_first = &content[3..];
    let end_idx = after_first
        .find("\n---")
        .ok_or_else(|| anyhow::anyhow!("Missing closing --- for YAML frontmatter"))?;

    let yaml_str = &after_first[..end_idx];
    let body_start = 3 + end_idx + 4; // skip "---\n" at start and "\n---" at end
    let body = if body_start < content.len() {
        content[body_start..].trim().to_string()
    } else {
        String::new()
    };

    let frontmatter: RawFrontmatter = serde_yml::from_str(yaml_str)?;

    Ok((frontmatter, body))
}

/// Convert raw frontmatter into structured metadata.
pub fn frontmatter_to_metadata(raw: &RawFrontmatter) -> SkillMetadata {
    let meta = raw.metadata.as_ref();

    SkillMetadata {
        categories: meta.and_then(|m| m.categories.clone()).unwrap_or_default(),
        dimensions: meta.and_then(|m| m.dimensions.clone()).unwrap_or_default(),
        os: meta.and_then(|m| m.os.clone()),
        requires_bins: meta.and_then(|m| m.requires_bins.clone()),
        requires_env: meta.and_then(|m| m.requires_env.clone()),
        trigger: meta.and_then(|m| m.trigger.clone()),
        schema_version: meta.and_then(|m| m.schema_version).unwrap_or(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_skill() {
        let content = r#"---
name: code-review
kind: evaluator
description: Reviews code for quality
metadata:
  categories:
    - rust
    - python
  schema_version: 1
---
# Code Review

This skill reviews code for quality issues.
"#;
        let (fm, body) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name.as_deref(), Some("code-review"));
        assert_eq!(fm.kind, Some(SkillKind::Evaluator));
        assert_eq!(fm.description.as_deref(), Some("Reviews code for quality"));
        assert!(body.contains("Code Review"));
        assert!(body.contains("reviews code"));

        let meta = frontmatter_to_metadata(&fm);
        assert_eq!(meta.categories, vec!["rust", "python"]);
        assert_eq!(meta.schema_version, 1);
    }

    #[test]
    fn test_parse_with_dimensions() {
        let content = r#"---
name: test-quality
kind: evaluator
description: Evaluates test quality
metadata:
  categories:
    - testing
  dimensions:
    - name: coverage
      weight: 0.4
      description: Test coverage breadth
    - name: assertions
      weight: 0.3
      description: Assertion quality
---
Body here.
"#;
        let (fm, _body) = parse_skill_md(content).unwrap();
        let meta = frontmatter_to_metadata(&fm);
        assert_eq!(meta.dimensions.len(), 2);
        assert_eq!(meta.dimensions[0].name, "coverage");
        assert!((meta.dimensions[0].weight - 0.4).abs() < f32::EPSILON);
        assert_eq!(meta.dimensions[1].name, "assertions");
    }

    #[test]
    fn test_parse_minimal() {
        let content = "---\nname: simple\n---\nBody.";
        let (fm, body) = parse_skill_md(content).unwrap();
        assert_eq!(fm.name.as_deref(), Some("simple"));
        assert_eq!(fm.kind, None);
        assert_eq!(body, "Body.");
    }

    #[test]
    fn test_parse_missing_frontmatter() {
        let content = "# No frontmatter here";
        let result = parse_skill_md(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_closing_delimiter() {
        let content = "---\nname: broken\nNo closing delimiter";
        let result = parse_skill_md(content);
        assert!(result.is_err());
    }

    #[test]
    fn test_frontmatter_to_metadata_defaults() {
        let fm = RawFrontmatter {
            name: Some("test".into()),
            kind: None,
            description: None,
            metadata: None,
        };
        let meta = frontmatter_to_metadata(&fm);
        assert!(meta.categories.is_empty());
        assert!(meta.dimensions.is_empty());
        assert!(meta.os.is_none());
        assert!(meta.requires_bins.is_none());
        assert_eq!(meta.schema_version, 1);
    }
}
