//! Decision logging and traceability for the supervisor.
//!
//! Every supervisor decision (rule engine match, LLM fallback answer,
//! or human escalation) is recorded as a `DecisionRecord`. The full
//! implementation (session accumulation, file writing, PR section
//! generation) is in Story 3.4.

use serde::{Deserialize, Serialize};

/// Source that provided the answer for a supervisor decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecisionSource {
    /// Answer came from the deterministic rule engine.
    RuleEngine {
        /// Name of the matched rule.
        rule_name: String,
    },
    /// Answer came from the LLM fallback with project context.
    LlmFallback,
    /// Question was escalated to a human.
    HumanEscalation,
}

/// A single supervisor decision record.
///
/// Created every time the supervisor answers a question (or escalates).
/// Accumulated during a session and written to a decisions file at
/// `_bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md`.
///
/// **Forward-compatibility:** This struct is used by:
/// - Story 3.4: Decision file writing and session accumulation
/// - Epic 5 Story 5.1: PR description "Supervisor Decisions" section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// The agent's original question.
    pub question: String,
    /// The answer provided (or escalation reason).
    pub answer: String,
    /// How the answer was determined.
    pub source: DecisionSource,
    /// Reasoning for why this answer was given.
    pub reasoning: String,
    /// Alternative answers that were considered.
    pub alternatives: Vec<String>,
    /// ISO 8601 timestamp of when the decision was made.
    pub timestamp: String,
}

// TODO: Story 3.4 — Add DecisionLog struct for session accumulation
// TODO: Story 3.4 — Add write_decisions_file() for markdown output
// TODO: Story 3.4 — Add to_pr_section() for PR description inclusion
