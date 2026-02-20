// src/skills/loader.rs — Skill loading from multiple sources

use std::path::Path;

use super::frontmatter::{frontmatter_to_metadata, parse_skill_md};
use super::types::*;
use crate::infra::paths;

/// Bundled evaluator skills (embedded in binary via include_str!).
const BUNDLED_EVALUATORS: &[(&str, &str)] = &[
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

/// Bundled task skills (embedded in binary via include_str!).
const BUNDLED_TASKS: &[(&str, &str)] = &[(
    "self-iterate",
    include_str!("../../skills/self-iterate/SKILL.md"),
)];

/// Load all skills from all sources, in precedence order.
pub fn load_all_skills() -> Vec<SkillEntry> {
    let mut skills = Vec::new();

    // 1. Bundled evaluators (lowest precedence)
    skills.extend(load_bundled_evaluators());

    // 2. Bundled task skills
    skills.extend(load_bundled_tasks());

    // 2. Managed skills
    if let Ok(managed) =
        load_from_directory(&paths::managed_skills_dir(), SkillSource::OpenKoiManaged)
    {
        skills.extend(managed);
    }

    // 3. Workspace project skills
    let workspace_skills = Path::new(".agents/skills");
    if workspace_skills.exists() {
        if let Ok(ws) = load_from_directory(workspace_skills, SkillSource::WorkspaceProject) {
            skills.extend(ws);
        }
    }
    let workspace_evaluators = Path::new(".agents/evaluators");
    if workspace_evaluators.exists() {
        if let Ok(ws) = load_from_directory(workspace_evaluators, SkillSource::WorkspaceProject) {
            skills.extend(ws);
        }
    }

    // 4. User global skills
    if let Ok(user) = load_from_directory(&paths::user_skills_dir(), SkillSource::UserGlobal) {
        skills.extend(user);
    }

    // 5. Pattern-proposed skills
    if let Ok(proposed) =
        load_from_directory(&paths::proposed_skills_dir(), SkillSource::PatternProposed)
    {
        skills.extend(proposed);
    }

    skills
}

/// Load bundled evaluator skills from embedded strings.
fn load_bundled_evaluators() -> Vec<SkillEntry> {
    let mut skills = Vec::new();

    for (name, content) in BUNDLED_EVALUATORS {
        match parse_skill_md(content) {
            Ok((frontmatter, _body)) => {
                let metadata = frontmatter_to_metadata(&frontmatter);
                skills.push(SkillEntry {
                    name: name.to_string(),
                    kind: SkillKind::Evaluator,
                    description: frontmatter
                        .description
                        .unwrap_or_else(|| format!("{} evaluator", name)),
                    source: SkillSource::OpenKoiBundled,
                    path: None,
                    metadata,
                    embedding: None,
                    approved: true,
                });
            }
            Err(e) => {
                tracing::warn!("Failed to parse bundled evaluator '{}': {}", name, e);
            }
        }
    }

    skills
}

/// Load bundled task skills from embedded strings.
fn load_bundled_tasks() -> Vec<SkillEntry> {
    let mut skills = Vec::new();

    for (name, content) in BUNDLED_TASKS {
        match parse_skill_md(content) {
            Ok((frontmatter, _body)) => {
                let metadata = frontmatter_to_metadata(&frontmatter);
                skills.push(SkillEntry {
                    name: name.to_string(),
                    kind: SkillKind::Task,
                    description: frontmatter
                        .description
                        .unwrap_or_else(|| format!("{} task", name)),
                    source: SkillSource::OpenKoiBundled,
                    path: None,
                    metadata,
                    embedding: None,
                    approved: true,
                });
            }
            Err(e) => {
                tracing::warn!("Failed to parse bundled task skill '{}': {}", name, e);
            }
        }
    }

    skills
}

/// Load skills from a directory (each subdirectory contains a SKILL.md).
fn load_from_directory(dir: &Path, source: SkillSource) -> anyhow::Result<Vec<SkillEntry>> {
    let mut skills = Vec::new();

    if !dir.exists() {
        return Ok(skills);
    }

    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }

        let skill_path = entry.path().join("SKILL.md");
        if !skill_path.exists() {
            continue;
        }

        match std::fs::read_to_string(&skill_path) {
            Ok(content) => match parse_skill_md(&content) {
                Ok((frontmatter, _body)) => {
                    let metadata = frontmatter_to_metadata(&frontmatter);
                    let name = frontmatter
                        .name
                        .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string());
                    let kind = frontmatter.kind.unwrap_or_default();

                    skills.push(SkillEntry {
                        name,
                        kind,
                        description: frontmatter.description.unwrap_or_default(),
                        source: source.clone(),
                        path: Some(skill_path),
                        metadata,
                        embedding: None,
                        approved: source != SkillSource::PatternProposed,
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse SKILL.md at {}: {}",
                        skill_path.display(),
                        e
                    );
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read {}: {}", skill_path.display(), e);
            }
        }
    }

    Ok(skills)
}
