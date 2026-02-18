// src/patterns/skill_proposer.rs — Auto-generate skills from detected patterns

use std::sync::Arc;

use anyhow::Result;

use crate::infra::paths;
use crate::patterns::miner::DetectedPattern;
use crate::provider::{ChatRequest, Message, ModelProvider};

/// Proposes new skills from detected patterns.
pub struct SkillProposer {
    model: Arc<dyn ModelProvider>,
}

/// A proposed skill generated from a pattern.
#[derive(Debug, Clone)]
pub struct SkillProposal {
    pub name: String,
    pub confidence: f32,
    pub skill_md: String,
}

impl SkillProposer {
    pub fn new(model: Arc<dyn ModelProvider>) -> Self {
        Self { model }
    }

    /// Generate a SKILL.md proposal from a detected pattern.
    pub async fn propose(&self, pattern: &DetectedPattern) -> Result<SkillProposal> {
        let skill_md = self.generate_skill_md(pattern).await?;
        let name = slugify(&pattern.description);

        // Write to proposed skills directory
        let skill_dir = paths::proposed_skills_dir().join(&name);
        tokio::fs::create_dir_all(&skill_dir).await?;
        tokio::fs::write(skill_dir.join("SKILL.md"), &skill_md).await?;

        Ok(SkillProposal {
            name,
            confidence: pattern.confidence,
            skill_md,
        })
    }

    /// Use the planner model to generate a SKILL.md from a pattern.
    async fn generate_skill_md(&self, pattern: &DetectedPattern) -> Result<String> {
        let response = self
            .model
            .chat(ChatRequest {
                messages: vec![Message::user(format!(
                    "Generate a SKILL.md file for a recurring task pattern.\n\n\
                     Pattern: {desc}\n\
                     Type: {ptype}\n\
                     Frequency: {freq}\n\
                     Confidence: {conf:.2}\n\
                     Samples: {samples}\n\n\
                     Output a complete SKILL.md with YAML frontmatter and instructions.\n\
                     The frontmatter should include: name, description, category, trigger.\n\
                     The body should contain step-by-step instructions for the agent.",
                    desc = pattern.description,
                    ptype = pattern.pattern_type.as_str(),
                    freq = pattern.frequency.as_deref().unwrap_or("unknown"),
                    conf = pattern.confidence,
                    samples = pattern.sample_count,
                ))],
                max_tokens: Some(1500),
                temperature: Some(0.3),
                ..Default::default()
            })
            .await?;

        Ok(response.content)
    }
}

/// Convert a description into a URL/filename-safe slug.
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
