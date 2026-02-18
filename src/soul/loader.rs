// src/soul/loader.rs — Load soul from workspace/user/default with priority chain

use std::path::{Path, PathBuf};

use crate::infra::paths;

const DEFAULT_SOUL: &str = include_str!("../../templates/SOUL.md");
const MAX_SOUL_CHARS: usize = 20_000;

/// The loaded soul document.
#[derive(Debug, Clone)]
pub struct Soul {
    pub raw: String,
    pub source: SoulSource,
}

/// Where the soul was loaded from.
#[derive(Debug, Clone)]
pub enum SoulSource {
    /// Built-in template (serial entrepreneur)
    Default,
    /// ~/.openkoi/SOUL.md (user-edited)
    UserFile(PathBuf),
    /// .openkoi/SOUL.md (project-specific)
    WorkspaceFile(PathBuf),
}

impl std::fmt::Display for SoulSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::UserFile(p) => write!(f, "user:{}", p.display()),
            Self::WorkspaceFile(p) => write!(f, "workspace:{}", p.display()),
        }
    }
}

/// Load the soul with priority: workspace > user > default.
pub fn load_soul() -> Soul {
    // 1. Project-level soul (highest priority)
    let workspace_soul = Path::new(".openkoi/SOUL.md");
    if workspace_soul.exists() {
        if let Ok(content) = std::fs::read_to_string(workspace_soul) {
            return Soul {
                raw: truncate(&content, MAX_SOUL_CHARS),
                source: SoulSource::WorkspaceFile(workspace_soul.into()),
            };
        }
    }

    // 2. User-level soul
    let user_soul = paths::soul_path();
    if user_soul.exists() {
        if let Ok(content) = std::fs::read_to_string(&user_soul) {
            return Soul {
                raw: truncate(&content, MAX_SOUL_CHARS),
                source: SoulSource::UserFile(user_soul),
            };
        }
    }

    // 3. Default (built-in)
    Soul {
        raw: DEFAULT_SOUL.to_string(),
        source: SoulSource::Default,
    }
}

/// Build the soul section for injection into system prompts.
pub fn build_soul_prompt(soul: &Soul) -> String {
    let mut prompt = String::new();
    prompt.push_str("# Identity\n\n");
    prompt.push_str(&soul.raw);
    prompt.push_str("\n\n");
    prompt.push_str(
        "Embody this identity. Let it shape your reasoning, tone, and \
         tradeoffs \u{2014} not just your words.\n\n",
    );
    prompt
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        s.chars().take(max_chars).collect()
    }
}
