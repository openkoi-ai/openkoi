// src/core/parliament.rs — Society of Mind deliberation (single structured prompt)
//
// The Parliament is five agencies debating within a single LLM inference.
// It is not a multi-agent swarm. It is structured perspective-taking.

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::provider::{ChatRequest, Message, ModelProvider};
use crate::soul::sovereign::SovereignDirective;

// ─── Types ──────────────────────────────────────────────────────────────────

/// The verdict an agency can issue on a proposed plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Verdict {
    /// Proceed as planned.
    Approve,
    /// Proceed, but with a specific caveat to address.
    ApproveWithCaveat(String),
    /// Do not proceed. Explain why.
    Block(String),
}

impl Verdict {
    pub fn is_block(&self) -> bool {
        matches!(self, Verdict::Block(_))
    }

    pub fn symbol(&self) -> &str {
        match self {
            Verdict::Approve => "✅",
            Verdict::ApproveWithCaveat(_) => "⚠️",
            Verdict::Block(_) => "⛔",
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Verdict::Approve => "APPROVE",
            Verdict::ApproveWithCaveat(_) => "APPROVE+",
            Verdict::Block(_) => "BLOCK",
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Verdict::Approve => write!(f, "APPROVE"),
            Verdict::ApproveWithCaveat(c) => write!(f, "APPROVE — {c}"),
            Verdict::Block(r) => write!(f, "BLOCK — {r}"),
        }
    }
}

/// A single agency's assessment of the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgencyAssessment {
    pub agency: Agency,
    pub verdict: Verdict,
    pub reasoning: String,
}

/// The five agencies that make up the Parliament.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Agency {
    Guardian,
    Economist,
    Empath,
    Scholar,
    Strategist,
}

impl Agency {
    pub fn symbol(&self) -> &str {
        match self {
            Agency::Guardian => "🛡️",
            Agency::Economist => "💰",
            Agency::Empath => "💚",
            Agency::Scholar => "📚",
            Agency::Strategist => "🎯",
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Agency::Guardian => "Guardian",
            Agency::Economist => "Economist",
            Agency::Empath => "Empath",
            Agency::Scholar => "Scholar",
            Agency::Strategist => "Strategist",
        }
    }

    pub fn concern(&self) -> &str {
        match self {
            Agency::Guardian => "Is this safe? Can it be undone?",
            Agency::Economist => "Is this worth the cost in time and tokens?",
            Agency::Empath => "How will the human feel about this response?",
            Agency::Scholar => "Is this actually true and well-sourced?",
            Agency::Strategist => "Does this serve the human's trajectory, or just today's task?",
        }
    }
}

/// The full result of a parliamentary deliberation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deliberation {
    /// Per-agency assessments.
    pub assessments: Vec<AgencyAssessment>,
    /// The Strategist's final synthesis.
    pub synthesis: String,
    /// Whether the Parliament approves proceeding.
    pub approved: bool,
    /// Caveats that must be applied to the plan.
    pub caveats: Vec<String>,
    /// Reasons for any blocks.
    pub blocks: Vec<String>,
}

impl Deliberation {
    /// Build from parsed assessments. The Strategist's assessment is the synthesis.
    pub fn from_assessments(assessments: Vec<AgencyAssessment>) -> Self {
        let mut caveats = Vec::new();
        let mut blocks = Vec::new();

        for a in &assessments {
            match &a.verdict {
                Verdict::ApproveWithCaveat(c) => caveats.push(c.clone()),
                Verdict::Block(r) => blocks.push(r.clone()),
                Verdict::Approve => {}
            }
        }

        let synthesis = assessments
            .iter()
            .find(|a| a.agency == Agency::Strategist)
            .map(|a| a.reasoning.clone())
            .unwrap_or_else(|| "No strategist assessment available.".into());

        let approved = blocks.is_empty();

        Self {
            assessments,
            synthesis,
            approved,
            caveats,
            blocks,
        }
    }
}

// ─── Parliament ─────────────────────────────────────────────────────────────

/// The Parliament: five agencies deliberating in a single structured prompt.
pub struct Parliament {
    provider: Arc<dyn ModelProvider>,
    model_id: String,
}

impl Parliament {
    pub fn new(provider: Arc<dyn ModelProvider>, model_id: String) -> Self {
        Self { provider, model_id }
    }

    /// Run a full parliamentary deliberation on a proposed plan.
    ///
    /// This is ONE LLM inference, not five. The agencies are perspectives
    /// injected as structured context for the model to reason through.
    pub async fn deliberate(
        &self,
        directive: &SovereignDirective,
        task_description: &str,
    ) -> Result<Deliberation> {
        let prompt = format!(
            "You are the Parliament of an AI agent's mind. You contain five agencies, \
             each evaluating a proposed task from a different perspective.\n\n\
             ## Sovereign Directive\n{directive}\n\n\
             ## Proposed Task\n{task}\n\n\
             ## Your Agencies\n\
             For each agency, provide:\n\
             1. A 1-sentence assessment\n\
             2. A verdict: APPROVE, APPROVE_WITH_CAVEAT, or BLOCK\n\
             3. If caveat or block, explain why\n\n\
             ### 🛡️ Guardian (Safety & Reversibility)\n\
             Consider: Is this action reversible? What's the blast radius if it goes wrong?\n\n\
             ### 💰 Economist (Cost & Efficiency)\n\
             Consider: What will this cost in time, tokens, and API calls? Is there a cheaper way?\n\n\
             ### 💚 Empath (Human Context)\n\
             Consider: What is the human's likely emotional state? How should the tone be?\n\n\
             ### 📚 Scholar (Truth & Accuracy)\n\
             Consider: Will the result be factually reliable? Should confidence be hedged?\n\n\
             ### 🎯 Strategist (Synthesis)\n\
             Consider: Weighing all perspectives, should we proceed? What trajectory does this serve?\n\n\
             ## Output Format\n\
             Respond in this exact format (one block per agency, no extra text):\n\n\
             GUARDIAN: <verdict> | <1-sentence reasoning>\n\
             ECONOMIST: <verdict> | <1-sentence reasoning>\n\
             EMPATH: <verdict> | <1-sentence reasoning>\n\
             SCHOLAR: <verdict> | <1-sentence reasoning>\n\
             STRATEGIST: <verdict> | <1-2 sentence synthesis>\n\n\
             Verdicts must be exactly: APPROVE, APPROVE_WITH_CAVEAT, or BLOCK\n\
             For APPROVE_WITH_CAVEAT, add: CAVEAT: <what to address>\n\
             For BLOCK, add: REASON: <why to stop>",
            directive = directive.text,
            task = task_description,
        );

        let response = self
            .provider
            .chat(ChatRequest {
                model: self.model_id.clone(),
                messages: vec![Message::user(prompt)],
                max_tokens: Some(500),
                temperature: Some(0.2),
                ..Default::default()
            })
            .await?;

        let assessments = parse_deliberation(&response.content);
        Ok(Deliberation::from_assessments(assessments))
    }
}

/// Build a static fallback deliberation (no LLM call, all approve).
pub fn static_deliberation() -> Deliberation {
    let assessments = vec![
        AgencyAssessment {
            agency: Agency::Guardian,
            verdict: Verdict::Approve,
            reasoning: "No safety concerns detected.".into(),
        },
        AgencyAssessment {
            agency: Agency::Economist,
            verdict: Verdict::Approve,
            reasoning: "Standard task cost.".into(),
        },
        AgencyAssessment {
            agency: Agency::Empath,
            verdict: Verdict::Approve,
            reasoning: "Neutral context.".into(),
        },
        AgencyAssessment {
            agency: Agency::Scholar,
            verdict: Verdict::Approve,
            reasoning: "No epistemic concerns.".into(),
        },
        AgencyAssessment {
            agency: Agency::Strategist,
            verdict: Verdict::Approve,
            reasoning: "Proceed as planned.".into(),
        },
    ];
    Deliberation::from_assessments(assessments)
}

// ─── Parser ─────────────────────────────────────────────────────────────────

/// Parse the LLM response into structured assessments.
fn parse_deliberation(content: &str) -> Vec<AgencyAssessment> {
    let mut assessments = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Match lines like "GUARDIAN: APPROVE | Safe to proceed"
        let (agency, rest) = match line.split_once(':') {
            Some((a, r)) => (a.trim().to_uppercase(), r.trim().to_string()),
            None => continue,
        };

        let agency = match agency.as_str() {
            "GUARDIAN" | "🛡️ GUARDIAN" => Agency::Guardian,
            "ECONOMIST" | "💰 ECONOMIST" => Agency::Economist,
            "EMPATH" | "💚 EMPATH" => Agency::Empath,
            "SCHOLAR" | "📚 SCHOLAR" => Agency::Scholar,
            "STRATEGIST" | "🎯 STRATEGIST" => Agency::Strategist,
            _ => continue,
        };

        let (verdict, reasoning) = parse_verdict_and_reasoning(&rest);

        assessments.push(AgencyAssessment {
            agency,
            verdict,
            reasoning,
        });
    }

    // If parsing failed to get all 5, fill in missing agencies with APPROVE
    for expected in &[
        Agency::Guardian,
        Agency::Economist,
        Agency::Empath,
        Agency::Scholar,
        Agency::Strategist,
    ] {
        if !assessments.iter().any(|a| &a.agency == expected) {
            assessments.push(AgencyAssessment {
                agency: expected.clone(),
                verdict: Verdict::Approve,
                reasoning: "No assessment provided (defaulted to approve).".into(),
            });
        }
    }

    assessments
}

fn parse_verdict_and_reasoning(text: &str) -> (Verdict, String) {
    let text = text.trim();

    // Split on '|' to separate verdict from reasoning
    let (verdict_part, reasoning_part) = match text.split_once('|') {
        Some((v, r)) => (v.trim(), r.trim().to_string()),
        None => (text, String::new()),
    };

    let verdict_upper = verdict_part.to_uppercase();

    if verdict_upper.contains("BLOCK") {
        // Look for "REASON: ..." in the reasoning
        let reason = if let Some(r) = reasoning_part.strip_prefix("REASON:") {
            r.trim().to_string()
        } else if !reasoning_part.is_empty() {
            reasoning_part.clone()
        } else {
            "Blocked without specific reason.".into()
        };
        (Verdict::Block(reason.clone()), reason)
    } else if verdict_upper.contains("CAVEAT") || verdict_upper.contains("APPROVE_WITH") {
        // Look for "CAVEAT: ..." in the reasoning
        let caveat = if let Some(c) = reasoning_part.strip_prefix("CAVEAT:") {
            c.trim().to_string()
        } else if !reasoning_part.is_empty() {
            reasoning_part.clone()
        } else {
            "Unspecified caveat.".into()
        };
        (Verdict::ApproveWithCaveat(caveat.clone()), caveat)
    } else {
        // APPROVE
        let reasoning = if reasoning_part.is_empty() {
            "Approved.".into()
        } else {
            reasoning_part
        };
        (Verdict::Approve, reasoning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_approve() {
        let content = "\
GUARDIAN: APPROVE | Safe to proceed
ECONOMIST: APPROVE | Low cost task
EMPATH: APPROVE | Neutral context
SCHOLAR: APPROVE | No accuracy concerns
STRATEGIST: APPROVE | Proceed as planned";

        let assessments = parse_deliberation(content);
        assert_eq!(assessments.len(), 5);
        assert!(assessments.iter().all(|a| a.verdict == Verdict::Approve));
    }

    #[test]
    fn test_parse_block() {
        let content = "\
GUARDIAN: BLOCK | REASON: This deletes production data
ECONOMIST: APPROVE | Low cost
EMPATH: APPROVE | Neutral
SCHOLAR: APPROVE | Fine
STRATEGIST: BLOCK | REASON: Guardian block cannot be overridden";

        let assessments = parse_deliberation(content);
        let guardian = assessments.iter().find(|a| a.agency == Agency::Guardian).unwrap();
        assert!(guardian.verdict.is_block());

        let delib = Deliberation::from_assessments(assessments);
        assert!(!delib.approved);
        assert_eq!(delib.blocks.len(), 2);
    }

    #[test]
    fn test_parse_caveat() {
        let content = "\
GUARDIAN: APPROVE | Safe
ECONOMIST: APPROVE_WITH_CAVEAT | CAVEAT: Consider batching to reduce API calls
EMPATH: APPROVE | Good
SCHOLAR: APPROVE_WITH_CAVEAT | CAVEAT: Verify source reliability
STRATEGIST: APPROVE | Proceed with caveats";

        let assessments = parse_deliberation(content);
        let delib = Deliberation::from_assessments(assessments);
        assert!(delib.approved);
        assert_eq!(delib.caveats.len(), 2);
    }

    #[test]
    fn test_parse_missing_agencies_filled() {
        let content = "GUARDIAN: APPROVE | Safe";
        let assessments = parse_deliberation(content);
        assert_eq!(assessments.len(), 5);
    }

    #[test]
    fn test_deliberation_approved_when_no_blocks() {
        let assessments = vec![
            AgencyAssessment {
                agency: Agency::Guardian,
                verdict: Verdict::Approve,
                reasoning: "Safe".into(),
            },
            AgencyAssessment {
                agency: Agency::Strategist,
                verdict: Verdict::ApproveWithCaveat("Check first".into()),
                reasoning: "Check first".into(),
            },
        ];
        let delib = Deliberation::from_assessments(assessments);
        assert!(delib.approved);
        assert_eq!(delib.caveats.len(), 1);
    }

    #[test]
    fn test_verdict_display() {
        assert_eq!(format!("{}", Verdict::Approve), "APPROVE");
        assert_eq!(
            format!("{}", Verdict::ApproveWithCaveat("check".into())),
            "APPROVE — check"
        );
        assert_eq!(
            format!("{}", Verdict::Block("danger".into())),
            "BLOCK — danger"
        );
    }

    #[test]
    fn test_static_deliberation() {
        let d = static_deliberation();
        assert!(d.approved);
        assert_eq!(d.assessments.len(), 5);
        assert!(d.blocks.is_empty());
        assert!(d.caveats.is_empty());
    }
}
