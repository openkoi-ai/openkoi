// src/skills/registry.rs — Skill registry

use std::collections::HashMap;

use super::frontmatter::parse_skill_md;
use super::loader;
use super::types::*;

/// Central registry of all loaded skills.
pub struct SkillRegistry {
    skills: Vec<SkillEntry>,
    bodies: HashMap<String, String>,
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillRegistry {
    /// Create a new registry by loading all skills from all sources.
    pub fn new() -> Self {
        let skills = loader::load_all_skills();
        Self {
            skills,
            bodies: HashMap::new(),
        }
    }

    /// Create an empty registry (for testing).
    pub fn empty() -> Self {
        Self {
            skills: Vec::new(),
            bodies: HashMap::new(),
        }
    }

    /// Get all skills of a given kind.
    pub fn get_by_kind(&self, kind: SkillKind) -> Vec<SkillEntry> {
        self.skills
            .iter()
            .filter(|s| s.kind == kind)
            .cloned()
            .collect()
    }

    /// Get a skill by name.
    pub fn get_by_name(&self, name: &str) -> Option<&SkillEntry> {
        self.skills.iter().find(|s| s.name == name)
    }

    /// Get all skills.
    pub fn all(&self) -> &[SkillEntry] {
        &self.skills
    }

    /// Load the body (markdown content after frontmatter) of a skill.
    pub fn load_body(&self, skill: &SkillEntry) -> anyhow::Result<String> {
        // Check cache first
        if let Some(body) = self.bodies.get(&skill.name) {
            return Ok(body.clone());
        }

        // Load from file
        if let Some(path) = &skill.path {
            let content = std::fs::read_to_string(path)?;
            let (_frontmatter, body) = parse_skill_md(&content)?;
            return Ok(body);
        }

        // Try bundled content
        let bundled = [
            ("general", include_str!("../../evaluators/general/SKILL.md")),
            (
                "code-review",
                include_str!("../../evaluators/code-review/SKILL.md"),
            ),
            (
                "prose-quality",
                include_str!("../../evaluators/prose-quality/SKILL.md"),
            ),
            (
                "sql-safety",
                include_str!("../../evaluators/sql-safety/SKILL.md"),
            ),
            (
                "api-design",
                include_str!("../../evaluators/api-design/SKILL.md"),
            ),
            (
                "test-quality",
                include_str!("../../evaluators/test-quality/SKILL.md"),
            ),
        ];

        for (name, content) in &bundled {
            if *name == skill.name {
                let (_frontmatter, body) = parse_skill_md(content)?;
                return Ok(body);
            }
        }

        Err(anyhow::anyhow!("Skill body not found for '{}'", skill.name))
    }

    /// Count of skills by kind.
    pub fn count(&self, kind: SkillKind) -> usize {
        self.skills.iter().filter(|s| s.kind == kind).count()
    }

    /// Add a skill to the registry (used for testing or dynamic additions).
    pub fn add(&mut self, skill: SkillEntry) {
        self.skills.push(skill);
    }
}
