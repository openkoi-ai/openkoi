// src/skills/eligibility.rs — Skill eligibility checks

use super::types::{SkillEntry, SkillSource};

/// Check if a skill is eligible to be used in the current environment.
pub fn is_eligible(skill: &SkillEntry) -> bool {
    // OS check
    if let Some(os_list) = &skill.metadata.os {
        if !os_list.iter().any(|os| os == std::env::consts::OS) {
            return false;
        }
    }

    // Required binaries
    if let Some(bins) = &skill.metadata.requires_bins {
        for bin in bins {
            if which::which(bin).is_err() {
                return false;
            }
        }
    }

    // Required env vars
    if let Some(envs) = &skill.metadata.requires_env {
        for env in envs {
            if std::env::var(env).is_err() {
                return false;
            }
        }
    }

    // Pattern-proposed skills need explicit approval
    if skill.source == SkillSource::PatternProposed {
        return skill.is_approved();
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::{SkillKind, SkillMetadata};

    fn make_skill(source: SkillSource, approved: bool) -> SkillEntry {
        SkillEntry {
            name: "test-skill".into(),
            kind: SkillKind::Task,
            description: "A test skill".into(),
            source,
            path: None,
            metadata: SkillMetadata::default(),
            embedding: None,
            approved,
        }
    }

    #[test]
    fn test_eligible_bundled_skill() {
        let skill = make_skill(SkillSource::OpenKoiBundled, false);
        assert!(is_eligible(&skill));
    }

    #[test]
    fn test_eligible_user_global_skill() {
        let skill = make_skill(SkillSource::UserGlobal, false);
        assert!(is_eligible(&skill));
    }

    #[test]
    fn test_pattern_proposed_not_approved() {
        let skill = make_skill(SkillSource::PatternProposed, false);
        assert!(!is_eligible(&skill));
    }

    #[test]
    fn test_pattern_proposed_approved() {
        let skill = make_skill(SkillSource::PatternProposed, true);
        assert!(is_eligible(&skill));
    }

    #[test]
    fn test_wrong_os_ineligible() {
        let mut skill = make_skill(SkillSource::OpenKoiBundled, false);
        skill.metadata.os = Some(vec!["nonexistent-os".into()]);
        assert!(!is_eligible(&skill));
    }

    #[test]
    fn test_correct_os_eligible() {
        let mut skill = make_skill(SkillSource::OpenKoiBundled, false);
        skill.metadata.os = Some(vec![std::env::consts::OS.into()]);
        assert!(is_eligible(&skill));
    }

    #[test]
    fn test_missing_required_binary() {
        let mut skill = make_skill(SkillSource::OpenKoiBundled, false);
        skill.metadata.requires_bins = Some(vec!["this-binary-does-not-exist-xyz123".into()]);
        assert!(!is_eligible(&skill));
    }

    #[test]
    fn test_missing_required_env() {
        let mut skill = make_skill(SkillSource::OpenKoiBundled, false);
        skill.metadata.requires_env = Some(vec!["OPENKOI_NONEXISTENT_ENV_VAR_XYZ123".into()]);
        assert!(!is_eligible(&skill));
    }

    #[test]
    fn test_no_constraints_eligible() {
        let skill = make_skill(SkillSource::WorkspaceProject, false);
        assert!(is_eligible(&skill));
    }
}
