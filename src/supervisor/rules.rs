//! Supervisor rule engine — deterministic pattern matching for common agent questions.
//!
//! The rule engine is the first tier of the supervisor's three-tier architecture:
//! 1. **Rule engine** (deterministic, free, fast) — THIS MODULE
//! 2. LLM fallback (context-aware) — Story 3.2
//! 3. Human escalation — Story 3.3
//!
//! Rules are evaluated in priority order (first match wins). New rules can be
//! added via `add_rule()` without modifying the tool interface or supervisor
//! module structure (AC #4).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Result of evaluating a question against the rule engine.
#[derive(Debug, Clone)]
pub enum RuleResult {
    /// A rule matched the question — use this answer.
    Matched {
        /// Name of the matched rule (for logging and decision records).
        rule_name: String,
        /// The deterministic answer to return.
        answer: String,
    },
    /// No rule matched — LLM fallback is needed.
    NoMatch,
}

impl fmt::Display for RuleResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleResult::Matched { rule_name, answer } => {
                write!(f, "Matched rule '{}': {}", rule_name, answer)
            }
            RuleResult::NoMatch => write!(f, "NoMatch"),
        }
    }
}

/// Pattern matching strategy for a rule.
///
/// Patterns are evaluated case-insensitively against the question text.
/// Multiple pattern types allow flexible matching from simple substring
/// checks to composite patterns.
///
/// **Design note:** No `Regex` variant — simple string matching is sufficient
/// for known BMAD patterns and avoids a `regex` crate dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RulePattern {
    /// Match if the question contains this substring (case-insensitive).
    Contains(String),
    /// Match if the question starts with any of these prefixes (case-insensitive).
    /// Leading whitespace in the question is trimmed before comparison.
    StartsWithAny(Vec<String>),
    /// Match if any of the sub-patterns match (logical OR).
    AnyOf(Vec<RulePattern>),
}

impl RulePattern {
    /// Evaluate this pattern against a question (case-insensitive).
    pub fn matches(&self, question: &str) -> bool {
        let q_lower = question.to_lowercase();
        match self {
            RulePattern::Contains(substring) => q_lower.contains(&substring.to_lowercase()),
            RulePattern::StartsWithAny(prefixes) => {
                let q_trimmed = q_lower.trim_start();
                prefixes
                    .iter()
                    .any(|p| q_trimmed.starts_with(&p.to_lowercase()))
            }
            RulePattern::AnyOf(patterns) => patterns.iter().any(|p| p.matches(question)),
        }
    }
}

/// A single deterministic rule in the rule engine.
///
/// Rules are evaluated in order — the first match wins. This enables
/// priority ordering: more specific rules should come before general ones.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// Human-readable name for logging and decision records.
    pub name: String,
    /// The pattern to match against the agent's question.
    pub pattern: RulePattern,
    /// The deterministic response to return when matched.
    pub response: String,
    /// Description of what this rule handles (for documentation).
    pub description: String,
}

/// Deterministic rule engine for the supervisor.
///
/// Evaluates agent questions against a list of pattern-based rules.
/// Rules are checked in order — first match wins. This is the "fast, free,
/// deterministic" first layer of the supervisor's three-tier architecture
/// (rule engine → LLM fallback → human escalation).
///
/// **Extensibility (AC #4):** New rules can be added via `add_rule()` without
/// modifying the tool interface or module structure. Rules are also
/// serializable for potential future config-driven rule loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEngine {
    rules: Vec<Rule>,
}

impl RuleEngine {
    /// Create a new RuleEngine loaded with all built-in rules.
    pub fn new() -> Self {
        Self {
            rules: Self::default_rules(),
        }
    }

    /// Evaluate a question against all rules in priority order.
    /// Returns the first matching rule's answer, or `NoMatch`.
    pub fn evaluate(&self, question: &str) -> RuleResult {
        for rule in &self.rules {
            if rule.pattern.matches(question) {
                return RuleResult::Matched {
                    rule_name: rule.name.clone(),
                    answer: rule.response.clone(),
                };
            }
        }
        RuleResult::NoMatch
    }

    /// Add a rule to the engine. New rules are appended at the end
    /// (lowest priority). Use `insert_rule` for priority placement.
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// Insert a rule at a specific position (0-indexed).
    /// Rules before this position have higher priority.
    pub fn insert_rule(&mut self, index: usize, rule: Rule) {
        let idx = index.min(self.rules.len());
        self.rules.insert(idx, rule);
    }

    /// Returns the number of rules in the engine.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Built-in rules covering common BMAD workflow patterns.
    ///
    /// These patterns are derived from the BMAD methodology's interactive
    /// conversation flow. The agent (running dev-story workflow) frequently
    /// asks for confirmation or announces step-by-step plans that should
    /// be short-circuited for autonomous operation.
    ///
    /// **Pattern source:** project-context.md § Supervisor Hybrid Pattern
    fn default_rules() -> Vec<Rule> {
        vec![
            // --- Confirmation patterns ---
            // The agent asks for permission to proceed. In autonomous mode,
            // the answer is always "yes" — the story file IS the approval.
            Rule {
                name: "confirmation_proceed".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::Contains("should i proceed".to_string()),
                    RulePattern::Contains("shall i continue".to_string()),
                    RulePattern::Contains("do you want me to".to_string()),
                    RulePattern::Contains("ready to proceed".to_string()),
                    RulePattern::Contains("can i go ahead".to_string()),
                    RulePattern::Contains("may i proceed".to_string()),
                    RulePattern::Contains("want me to continue".to_string()),
                    RulePattern::Contains("should i go ahead".to_string()),
                    RulePattern::Contains("shall i proceed".to_string()),
                    RulePattern::Contains("ok to proceed".to_string()),
                ]),
                response: "Yes, proceed.".to_string(),
                description: "Matches agent requests for confirmation to proceed with work."
                    .to_string(),
            },
            // --- Permission request patterns ---
            // The agent asks if it should perform a specific action.
            Rule {
                name: "permission_action".to_string(),
                pattern: RulePattern::AnyOf(vec![RulePattern::StartsWithAny(vec![
                    "should i create".to_string(),
                    "should i modify".to_string(),
                    "should i delete".to_string(),
                    "should i update".to_string(),
                    "should i add".to_string(),
                    "should i remove".to_string(),
                    "should i refactor".to_string(),
                    "should i implement".to_string(),
                    "can i create".to_string(),
                    "can i modify".to_string(),
                    "can i delete".to_string(),
                    "can i update".to_string(),
                ])]),
                response: "Yes, proceed with the action as described.".to_string(),
                description:
                    "Matches agent requests for permission to perform specific actions."
                        .to_string(),
            },
            // --- Step-by-step detection ---
            // The agent announces it will explain its plan step-by-step.
            // In autonomous mode, we want execution not explanation.
            Rule {
                name: "step_by_step_detection".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::Contains("step by step".to_string()),
                    RulePattern::Contains("step-by-step".to_string()),
                    RulePattern::Contains("let me break this down".to_string()),
                    RulePattern::Contains("here's my plan".to_string()),
                    RulePattern::Contains("here is my plan".to_string()),
                    RulePattern::Contains("i'll outline".to_string()),
                    RulePattern::Contains("let me outline".to_string()),
                    RulePattern::Contains("my approach will be".to_string()),
                ]),
                response: "Skip the step-by-step breakdown. Execute directly using yolo mode."
                    .to_string(),
                description: "Detects agent announcing a step-by-step plan and redirects to direct execution.".to_string(),
            },
            // --- Story selection ---
            // The agent asks which story to work on.
            Rule {
                name: "story_selection".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::Contains("which story".to_string()),
                    RulePattern::Contains("what story".to_string()),
                    RulePattern::Contains("next story".to_string()),
                    RulePattern::Contains("what should i work on".to_string()),
                    RulePattern::Contains("which task".to_string()),
                ]),
                response: "The story file has been provided in context. Follow the tasks and acceptance criteria in the story file.".to_string(),
                description: "Matches agent questions about which story or task to work on.".to_string(),
            },
            // --- Progress confirmation ---
            // The agent reports completion of a task or subtask.
            Rule {
                name: "progress_confirmation".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::Contains("i've completed".to_string()),
                    RulePattern::Contains("i have completed".to_string()),
                    RulePattern::Contains("i'm done with".to_string()),
                    RulePattern::Contains("i am done with".to_string()),
                    RulePattern::Contains("task complete".to_string()),
                    RulePattern::Contains("finished implementing".to_string()),
                    RulePattern::Contains("implementation complete".to_string()),
                ]),
                response: "Acknowledged. Continue to the next task.".to_string(),
                description: "Matches agent progress reports and acknowledges completion."
                    .to_string(),
            },
            // --- Error/blocker reporting ---
            // The agent reports it's stuck but isn't asking a specific question.
            Rule {
                name: "stuck_general".to_string(),
                pattern: RulePattern::AnyOf(vec![
                    RulePattern::Contains("i'm stuck".to_string()),
                    RulePattern::Contains("i am stuck".to_string()),
                    RulePattern::Contains("i can't figure out".to_string()),
                    RulePattern::Contains("i cannot figure out".to_string()),
                    RulePattern::Contains("i'm blocked".to_string()),
                    RulePattern::Contains("i am blocked".to_string()),
                ]),
                response: "Describe the specific problem including error messages and what you've tried, then proceed with your best judgment based on the story specs and project-context.md.".to_string(),
                description: "Matches vague 'I'm stuck' reports and asks for specifics.".to_string(),
            },
        ]
    }
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- RulePattern tests ---

    #[test]
    fn test_pattern_contains_matches_substring() {
        let pattern = RulePattern::Contains("proceed".to_string());
        assert!(pattern.matches("Should I proceed with the task?"));
        assert!(pattern.matches("SHOULD I PROCEED?"));
        assert!(!pattern.matches("What should I do next?"));
    }

    #[test]
    fn test_pattern_contains_case_insensitive() {
        let pattern = RulePattern::Contains("should i proceed".to_string());
        assert!(pattern.matches("Should I Proceed?"));
        assert!(pattern.matches("SHOULD I PROCEED?"));
        assert!(pattern.matches("should i proceed"));
    }

    #[test]
    fn test_pattern_starts_with_any() {
        let pattern = RulePattern::StartsWithAny(vec![
            "should i create".to_string(),
            "should i modify".to_string(),
        ]);
        assert!(pattern.matches("Should I create a new file?"));
        assert!(pattern.matches("should i modify the config?"));
        assert!(!pattern.matches("Can I create something?"));
    }

    #[test]
    fn test_pattern_starts_with_any_trims_leading_whitespace() {
        let pattern = RulePattern::StartsWithAny(vec!["should i".to_string()]);
        assert!(pattern.matches("  Should I proceed?"));
    }

    #[test]
    fn test_pattern_any_of_matches_first_hit() {
        let pattern = RulePattern::AnyOf(vec![
            RulePattern::Contains("alpha".to_string()),
            RulePattern::Contains("beta".to_string()),
        ]);
        assert!(pattern.matches("this has alpha in it"));
        assert!(pattern.matches("this has beta in it"));
        assert!(!pattern.matches("this has gamma in it"));
    }

    // --- RuleEngine tests ---

    #[test]
    fn test_rule_engine_matches_confirmation() {
        let engine = RuleEngine::new();
        let result = engine.evaluate("Should I proceed with the implementation?");
        match result {
            RuleResult::Matched { rule_name, answer } => {
                assert_eq!(rule_name, "confirmation_proceed");
                assert_eq!(answer, "Yes, proceed.");
            }
            RuleResult::NoMatch => panic!("Expected match for confirmation"),
        }
    }

    #[test]
    fn test_rule_engine_matches_confirmation_variants() {
        let engine = RuleEngine::new();
        let confirmations = vec![
            "Shall I continue with the next task?",
            "Do you want me to implement this?",
            "Ready to proceed?",
            "Can I go ahead with the changes?",
            "May I proceed?",
        ];
        for q in confirmations {
            match engine.evaluate(q) {
                RuleResult::Matched { rule_name, .. } => {
                    assert_eq!(rule_name, "confirmation_proceed", "Failed for: {q}");
                }
                RuleResult::NoMatch => panic!("Expected match for: {q}"),
            }
        }
    }

    #[test]
    fn test_rule_engine_matches_step_by_step() {
        let engine = RuleEngine::new();
        let questions = vec![
            "I'll do this step by step",
            "Let me break this down into parts",
            "Here's my plan for implementing this:",
        ];
        for q in questions {
            match engine.evaluate(q) {
                RuleResult::Matched { rule_name, .. } => {
                    assert_eq!(rule_name, "step_by_step_detection", "Failed for: {q}");
                }
                RuleResult::NoMatch => panic!("Expected match for: {q}"),
            }
        }
    }

    #[test]
    fn test_rule_engine_matches_story_selection() {
        let engine = RuleEngine::new();
        let questions = vec![
            "Which story should I work on?",
            "What's the next story?",
            "What should I work on next?",
        ];
        for q in questions {
            match engine.evaluate(q) {
                RuleResult::Matched { rule_name, .. } => {
                    assert_eq!(rule_name, "story_selection", "Failed for: {q}");
                }
                RuleResult::NoMatch => panic!("Expected match for: {q}"),
            }
        }
    }

    #[test]
    fn test_rule_engine_matches_progress_confirmation() {
        let engine = RuleEngine::new();
        let questions = vec![
            "I've completed the unit tests",
            "I'm done with task 3",
            "Task complete — all tests passing",
            "Finished implementing the error handler",
        ];
        for q in questions {
            match engine.evaluate(q) {
                RuleResult::Matched { rule_name, .. } => {
                    assert_eq!(rule_name, "progress_confirmation", "Failed for: {q}");
                }
                RuleResult::NoMatch => panic!("Expected match for: {q}"),
            }
        }
    }

    #[test]
    fn test_rule_engine_matches_permission_requests() {
        let engine = RuleEngine::new();
        let questions = vec![
            "Should I create a new module for this?",
            "Should I modify the existing struct?",
            "Can I delete the unused test file?",
            "Should I update the Cargo.toml?",
        ];
        for q in questions {
            match engine.evaluate(q) {
                RuleResult::Matched { rule_name, .. } => {
                    assert_eq!(rule_name, "permission_action", "Failed for: {q}");
                }
                RuleResult::NoMatch => panic!("Expected match for: {q}"),
            }
        }
    }

    #[test]
    fn test_rule_engine_returns_no_match_for_unknown() {
        let engine = RuleEngine::new();
        let questions = vec![
            "What is the correct database schema for this table?",
            "How should I handle authentication in this endpoint?",
            "The test is failing with error code 42, what does it mean?",
        ];
        for q in questions {
            match engine.evaluate(q) {
                RuleResult::NoMatch => {} // expected
                RuleResult::Matched { rule_name, .. } => {
                    panic!("Expected NoMatch for '{q}', got match: {rule_name}");
                }
            }
        }
    }

    #[test]
    fn test_rule_engine_case_insensitive() {
        let engine = RuleEngine::new();
        assert!(matches!(
            engine.evaluate("SHOULD I PROCEED?"),
            RuleResult::Matched { .. }
        ));
        assert!(matches!(
            engine.evaluate("should i proceed?"),
            RuleResult::Matched { .. }
        ));
        assert!(matches!(
            engine.evaluate("Should I Proceed?"),
            RuleResult::Matched { .. }
        ));
    }

    #[test]
    fn test_rule_engine_first_match_wins() {
        // Confirmation pattern should match before permission pattern
        // for "Should I proceed?" (contains "should i proceed" AND starts with "should i")
        let engine = RuleEngine::new();
        match engine.evaluate("Should I proceed with creating the file?") {
            RuleResult::Matched { rule_name, .. } => {
                // "should i proceed" is in confirmation_proceed which comes first
                assert_eq!(rule_name, "confirmation_proceed");
            }
            RuleResult::NoMatch => panic!("Expected a match"),
        }
    }

    #[test]
    fn test_rule_engine_add_rule() {
        let mut engine = RuleEngine::new();
        let initial_count = engine.rule_count();

        engine.add_rule(Rule {
            name: "custom_rule".to_string(),
            pattern: RulePattern::Contains("custom pattern".to_string()),
            response: "Custom response".to_string(),
            description: "Test rule".to_string(),
        });

        assert_eq!(engine.rule_count(), initial_count + 1);

        match engine.evaluate("This has a custom pattern in it") {
            RuleResult::Matched { rule_name, answer } => {
                assert_eq!(rule_name, "custom_rule");
                assert_eq!(answer, "Custom response");
            }
            RuleResult::NoMatch => panic!("Expected custom rule to match"),
        }
    }

    #[test]
    fn test_rule_engine_insert_rule() {
        let mut engine = RuleEngine::new();
        let initial_count = engine.rule_count();

        engine.insert_rule(
            0,
            Rule {
                name: "high_priority".to_string(),
                pattern: RulePattern::Contains("proceed".to_string()),
                response: "High priority response".to_string(),
                description: "Inserted at position 0".to_string(),
            },
        );

        assert_eq!(engine.rule_count(), initial_count + 1);

        // The inserted rule should win over the default confirmation_proceed
        // because it's at position 0.
        match engine.evaluate("Should I proceed?") {
            RuleResult::Matched { rule_name, .. } => {
                assert_eq!(rule_name, "high_priority");
            }
            RuleResult::NoMatch => panic!("Expected high_priority rule to match"),
        }
    }

    #[test]
    fn test_rule_engine_insert_rule_clamps_index() {
        let mut engine = RuleEngine::new();
        let count = engine.rule_count();

        // Insert at an index way beyond the end — should clamp to end
        engine.insert_rule(
            9999,
            Rule {
                name: "clamped".to_string(),
                pattern: RulePattern::Contains("clamped test".to_string()),
                response: "Clamped".to_string(),
                description: "Clamped".to_string(),
            },
        );

        assert_eq!(engine.rule_count(), count + 1);

        match engine.evaluate("clamped test question") {
            RuleResult::Matched { rule_name, .. } => {
                assert_eq!(rule_name, "clamped");
            }
            RuleResult::NoMatch => panic!("Expected clamped rule to match"),
        }
    }

    #[test]
    fn test_rule_engine_serializable() {
        let engine = RuleEngine::new();
        let json = serde_json::to_string(&engine).expect("Should serialize");
        let deserialized: RuleEngine = serde_json::from_str(&json).expect("Should deserialize");
        assert_eq!(deserialized.rule_count(), engine.rule_count());
    }

    #[test]
    fn test_rule_engine_matches_stuck_general() {
        let engine = RuleEngine::new();
        match engine.evaluate("I'm stuck on this implementation") {
            RuleResult::Matched { rule_name, answer } => {
                assert_eq!(rule_name, "stuck_general");
                assert!(answer.contains("specific problem"));
            }
            RuleResult::NoMatch => panic!("Expected match for stuck pattern"),
        }
    }

    #[test]
    fn test_rule_result_display() {
        let matched = RuleResult::Matched {
            rule_name: "test".to_string(),
            answer: "yes".to_string(),
        };
        assert!(matched.to_string().contains("test"));

        let no_match = RuleResult::NoMatch;
        assert_eq!(no_match.to_string(), "NoMatch");
    }

    #[test]
    fn test_rule_engine_default_trait() {
        let engine = RuleEngine::default();
        assert!(engine.rule_count() > 0);
        // Default should behave identically to new()
        assert_eq!(engine.rule_count(), RuleEngine::new().rule_count());
    }

    #[test]
    fn test_rule_engine_empty_question_no_match() {
        let engine = RuleEngine::new();
        // An empty question shouldn't match any pattern
        match engine.evaluate("") {
            RuleResult::NoMatch => {} // expected
            RuleResult::Matched { rule_name, .. } => {
                panic!("Expected NoMatch for empty question, got: {rule_name}");
            }
        }
    }
}
