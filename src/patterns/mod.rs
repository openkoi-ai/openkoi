// src/patterns/mod.rs — Daily usage pattern learning

pub mod event_logger;
pub mod miner;
pub mod skill_proposer;

pub use event_logger::{EventLogger, EventType, UsageEvent};
pub use miner::{DetectedPattern, PatternMiner};
pub use skill_proposer::{SkillProposal, SkillProposer};
