// src/skills/types.rs — Skill type definitions

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A loaded skill entry in the registry.
#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub kind: SkillKind,
    pub description: String,
    pub source: SkillSource,
    pub path: Option<PathBuf>,
    pub metadata: SkillMetadata,
    pub embedding: Option<Vec<f32>>,
    pub approved: bool,
}

impl SkillEntry {
    pub fn is_approved(&self) -> bool {
        self.approved || self.source != SkillSource::PatternProposed
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillKind {
    Task,
    Evaluator,
}

impl Default for SkillKind {
    fn default() -> Self {
        Self::Task
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SkillSource {
    OpenKoiBundled,
    OpenKoiManaged,
    OpenClawBundled,
    WorkspaceProject,
    UserGlobal,
    PatternProposed,
}

/// Metadata parsed from SKILL.md YAML frontmatter.
#[derive(Debug, Clone, Default)]
pub struct SkillMetadata {
    pub categories: Vec<String>,
    pub dimensions: Vec<DimensionDef>,
    pub os: Option<Vec<String>>,
    pub requires_bins: Option<Vec<String>>,
    pub requires_env: Option<Vec<String>>,
    pub trigger: Option<TriggerDef>,
    pub schema_version: u32,
}

/// Dimension definition for evaluator skills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DimensionDef {
    pub name: String,
    pub weight: f32,
    #[serde(default)]
    pub description: String,
}

/// Trigger definition for scheduled/event-based skills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerDef {
    #[serde(rename = "type")]
    pub trigger_type: String,
    pub schedule: Option<serde_json::Value>,
}

/// Raw frontmatter parsed from a SKILL.md file.
#[derive(Debug, Clone, Deserialize)]
pub struct RawFrontmatter {
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<SkillKind>,
    pub description: Option<String>,
    #[serde(default)]
    pub metadata: Option<RawMetadata>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawMetadata {
    #[serde(default)]
    pub categories: Option<Vec<String>>,
    #[serde(default)]
    pub dimensions: Option<Vec<DimensionDef>>,
    #[serde(default)]
    pub os: Option<Vec<String>>,
    #[serde(default)]
    pub requires_bins: Option<Vec<String>>,
    #[serde(default)]
    pub requires_env: Option<Vec<String>>,
    #[serde(default)]
    pub trigger: Option<TriggerDef>,
    #[serde(default)]
    pub schema_version: Option<u32>,
    // OpenClaw-compatible block
    #[serde(default)]
    pub openclaw: Option<serde_json::Value>,
    // OpenKoi extensions
    #[serde(default)]
    pub openkoi: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(source: SkillSource, approved: bool) -> SkillEntry {
        SkillEntry {
            name: "test".into(),
            kind: SkillKind::Task,
            description: "desc".into(),
            source,
            path: None,
            metadata: SkillMetadata::default(),
            embedding: None,
            approved,
        }
    }

    // ─── SkillEntry::is_approved ────────────────────────────────

    #[test]
    fn test_is_approved_bundled_not_explicitly_approved() {
        let e = make_entry(SkillSource::OpenKoiBundled, false);
        assert!(e.is_approved()); // non-PatternProposed → always approved
    }

    #[test]
    fn test_is_approved_pattern_proposed_not_approved() {
        let e = make_entry(SkillSource::PatternProposed, false);
        assert!(!e.is_approved());
    }

    #[test]
    fn test_is_approved_pattern_proposed_approved() {
        let e = make_entry(SkillSource::PatternProposed, true);
        assert!(e.is_approved());
    }

    #[test]
    fn test_is_approved_user_global() {
        let e = make_entry(SkillSource::UserGlobal, false);
        assert!(e.is_approved());
    }

    // ─── SkillKind ──────────────────────────────────────────────

    #[test]
    fn test_skill_kind_default() {
        let k = SkillKind::default();
        assert_eq!(k, SkillKind::Task);
    }

    #[test]
    fn test_skill_kind_equality() {
        assert_eq!(SkillKind::Task, SkillKind::Task);
        assert_eq!(SkillKind::Evaluator, SkillKind::Evaluator);
        assert_ne!(SkillKind::Task, SkillKind::Evaluator);
    }

    #[test]
    fn test_skill_kind_serde_roundtrip() {
        let kind = SkillKind::Evaluator;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"evaluator\"");
        let back: SkillKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, SkillKind::Evaluator);
    }

    // ─── SkillSource ────────────────────────────────────────────

    #[test]
    fn test_skill_source_equality() {
        assert_eq!(SkillSource::OpenKoiBundled, SkillSource::OpenKoiBundled);
        assert_ne!(SkillSource::OpenKoiBundled, SkillSource::PatternProposed);
    }

    // ─── SkillMetadata defaults ─────────────────────────────────

    #[test]
    fn test_metadata_defaults() {
        let m = SkillMetadata::default();
        assert!(m.categories.is_empty());
        assert!(m.dimensions.is_empty());
        assert!(m.os.is_none());
        assert!(m.requires_bins.is_none());
        assert!(m.requires_env.is_none());
        assert!(m.trigger.is_none());
        assert_eq!(m.schema_version, 0);
    }

    // ─── DimensionDef ───────────────────────────────────────────

    #[test]
    fn test_dimension_def_serde() {
        let d = DimensionDef {
            name: "correctness".into(),
            weight: 0.4,
            description: "Is the code correct?".into(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: DimensionDef = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "correctness");
        assert!((back.weight - 0.4).abs() < f32::EPSILON);
    }

    #[test]
    fn test_dimension_def_default_description() {
        let json = r#"{"name":"style","weight":0.2}"#;
        let d: DimensionDef = serde_json::from_str(json).unwrap();
        assert_eq!(d.description, ""); // #[serde(default)]
    }

    // ─── TriggerDef ─────────────────────────────────────────────

    #[test]
    fn test_trigger_def_serde() {
        let json = r#"{"type":"cron","schedule":"0 9 * * *"}"#;
        let t: TriggerDef = serde_json::from_str(json).unwrap();
        assert_eq!(t.trigger_type, "cron");
        assert!(t.schedule.is_some());
    }

    // ─── RawFrontmatter ─────────────────────────────────────────

    #[test]
    fn test_raw_frontmatter_minimal() {
        let yaml = "name: my-skill\n";
        let fm: RawFrontmatter = serde_yml::from_str(yaml).unwrap();
        assert_eq!(fm.name, Some("my-skill".into()));
        assert!(fm.kind.is_none());
        assert!(fm.description.is_none());
    }

    #[test]
    fn test_raw_frontmatter_full() {
        let yaml = r#"
name: code-review
kind: evaluator
description: Reviews code quality
metadata:
  categories:
    - coding
    - review
  schema_version: 2
"#;
        let fm: RawFrontmatter = serde_yml::from_str(yaml).unwrap();
        assert_eq!(fm.name, Some("code-review".into()));
        assert_eq!(fm.kind, Some(SkillKind::Evaluator));
        let meta = fm.metadata.unwrap();
        assert_eq!(meta.categories.unwrap().len(), 2);
        assert_eq!(meta.schema_version, Some(2));
    }
}
