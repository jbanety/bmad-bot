//! Response analyzer — pattern-matching engine for LLM agent responses.
//!
//! The [`ResponseAnalyzer`] examines each agent response and determines the
//! appropriate [`ResponseAction`] for the session chat loop. It uses a
//! deterministic sentinel token (`<<BMAD_JOB_DONE>>`) as the primary completion
//! signal, with case-insensitive substring matching (and a few regex patterns)
//! as fallback, in a strict priority order.
//!
//! **Design principle:** The sentinel token is injected in the system preamble
//! and provides a model-agnostic, deterministic completion signal. The fuzzy
//! patterns are kept as a safety net for models that ignore the instruction.

use crate::supervisor::EscalationSlot;
use regex::Regex;
use std::sync::LazyLock;

/// Action the chat loop should take after analyzing an agent response.
///
/// Returned by [`ResponseAnalyzer::analyze()`] based on priority-ordered
/// pattern matching against the agent's response text and the escalation slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseAction {
    /// Send the contained reply and continue the chat loop.
    Continue {
        /// The reply message to send back to the agent.
        reply: String,
    },

    /// The agent signaled workflow completion — exit the chat loop successfully.
    Completed,

    /// Escalation detected via the shared escalation slot — exit the chat loop.
    Escalated,

    /// Reserved for future streaming/async response support where the agent may
    /// still be processing tool calls. Currently treated as
    /// `Continue("Continue.")` but kept as a distinct variant for
    /// forward-compatibility with rig streaming APIs.
    NoReply,
}

/// Deterministic sentinel token emitted by the agent when its workflow is done.
///
/// Injected in the system preamble (`dev_agent::build_preamble`). The agent is
/// instructed to emit this exact string on its own line as the last thing in
/// its final message. This is checked at **priority 0** — before any fuzzy
/// pattern matching — so it works identically across all LLM providers/models.
pub const JOB_DONE_SENTINEL: &str = "<<BMAD_JOB_DONE>>";

/// Review completion patterns (case-insensitive).
///
/// Detects CR workflow step 5 completion output. Checked at priority 1.5
/// (after escalation, before dev-session completion signals) to avoid
/// the step 5 summary (which contains "Issues Fixed:") from triggering
/// `REVIEW_FIX_PATTERNS` at priority 5.5.
const REVIEW_COMPLETE_PATTERNS: &[&str] = &[
    "review complete",
    "✅ review complete",
    "code review complete",
    "issues fixed:",
    "action items created:",
    "sprint status synced",
];

/// Review fix decision patterns (case-insensitive).
///
/// Detects the CR workflow step 4 prompt asking how to handle findings.
/// Auto-responds with "1" (fix automatically). Checked at priority 5.5
/// (between YOLO and Story Selection) — AFTER `REVIEW_COMPLETE_PATTERNS`
/// to prevent the step 5 summary from accidentally triggering a fix response.
const REVIEW_FIX_PATTERNS: &[&str] = &[
    "fix them automatically",
    "create action items",
    "show me details",
    "choose [1]",
    "choose [2]",
    "choose [3]",
    "[1] fix",
    "[2] create",
    "[3] show",
    "what should i do with these issues",
    "what should i do with these findings",
];

/// Completion signal phrases (case-insensitive).
///
/// These must be specific multi-word phrases to avoid false positives.
/// E.g., "I'll complete the task" should NOT trigger completion, but
/// "All tasks completed successfully" should.
const COMPLETION_SIGNALS: &[&str] = &[
    "all tasks completed",
    "story implementation complete",
    "dev-story workflow complete",
    "story marked as done",
    "implementation is complete",
    "all acceptance criteria met",
    "story is ready for review",
    "all tasks and subtasks are marked",
    "ready for review",
    "proceed with the next story",
    "move on to the next story",
    "next story or a code review",
];

/// Regex-based completion patterns (case-insensitive).
///
/// These catch completion signals that substring matching can't handle,
/// e.g. "Story 7.1: ... — COMPLETE" where there's variable text in between.
static COMPLETION_REGEX_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)story\s+.{1,80}complete",
        r"(?i)run\s+.*code[_-]?review",
    ]
    .iter()
    .filter_map(|p| Regex::new(p).ok())
    .collect()
});

/// Confirmation/proceed patterns (case-insensitive).
const PROCEED_PATTERNS: &[&str] = &[
    "should i proceed",
    "shall i proceed",
    "continue?",
    "ready to move on",
    "shall i continue",
    "should i continue",
    "do you want me to proceed",
    "do you want me to continue",
    "would you like me to proceed",
    "would you like me to continue",
    "may i proceed",
    "can i proceed",
    "want me to go ahead",
];

/// Step-by-step detection patterns (case-insensitive).
const STEP_BY_STEP_PATTERNS: &[&str] = &[
    "step by step",
    "one at a time",
    "one step at a time",
    "walk you through",
    "walk through each",
    "shall i do them one",
    "do each step separately",
    "handle each task individually",
];

/// YOLO/batch mode patterns (case-insensitive).
const YOLO_PATTERNS: &[&str] = &[
    "yolo mode",
    "batch mode",
    "yolo or",
    "interactive or batch",
    "want yolo",
    "enable yolo",
    "[y] yolo",
];

/// Story selection patterns (case-insensitive).
const STORY_SELECTION_PATTERNS: &[&str] = &[
    "which story",
    "story to work on",
    "what story",
    "specify a story",
    "provide the story",
    "story file path",
    "which story to develop",
    "story would you like",
];

/// Stateless response analyzer for the session chat loop.
///
/// Examines agent responses using a strict priority order:
/// 0. **Sentinel** — `<<BMAD_JOB_DONE>>` deterministic token (model-agnostic)
/// 1. Escalation (slot check)
///    - 1.5. Review completion detection (`REVIEW_COMPLETE_PATTERNS`)
/// 2. Completion signals (dev-session) — fuzzy fallback
/// 3. Confirmation/proceed requests
/// 4. Step-by-step detection
/// 5. YOLO/batch mode questions
///    - 5.5. Review fix decision (`REVIEW_FIX_PATTERNS`)
/// 6. Story selection questions
/// 7. Default — "Continue."
///
/// All pattern matching is case-insensitive substring search.
#[derive(Debug)]
pub struct ResponseAnalyzer;

impl ResponseAnalyzer {
    /// Create a new `ResponseAnalyzer`.
    pub fn new() -> Self {
        Self
    }

    /// Analyze an agent response and determine the appropriate action.
    ///
    /// # Arguments
    /// - `response` — the text response from the LLM agent
    /// - `escalation_slot` — shared slot checked for supervisor escalation
    /// - `story_key` — the current story key, used as reply for story selection questions
    ///
    /// # Priority Order
    /// 1. **Escalation** — if `escalation_slot` contains `Some(EscalationInfo)`, return `Escalated`
    /// 2. **Completion** — strong completion signals → `Completed`
    /// 3. **Confirmation** — "Should I proceed?" → `Continue { "Yes, proceed." }`
    /// 4. **Step-by-step** — step-by-step approval → `Continue { "Continue with all steps..." }`
    /// 5. **YOLO/batch** — YOLO mode questions → `Continue { "Use YOLO mode..." }`
    /// 6. **Story selection** — which story → `Continue { story_key }`
    /// 7. **Default** — `Continue { "Continue." }`
    pub fn analyze(
        &self,
        response: &str,
        escalation_slot: &EscalationSlot,
        story_key: &str,
    ) -> ResponseAction {
        // Priority 0: Deterministic sentinel check (model-agnostic)
        if response.contains(JOB_DONE_SENTINEL) {
            tracing::debug!(
                action = "response_analysis",
                priority = 0,
                result = "sentinel_completed",
                sentinel = JOB_DONE_SENTINEL,
                "{} sentinel detected",
                JOB_DONE_SENTINEL
            );
            return ResponseAction::Completed;
        }

        // Priority 1: Escalation check
        {
            let guard = escalation_slot.lock().expect("escalation slot lock");
            if guard.is_some() {
                tracing::debug!(
                    action = "response_analysis",
                    priority = 1,
                    result = "escalated",
                    "Escalation detected in slot"
                );
                return ResponseAction::Escalated;
            }
        }

        let lower = response.to_lowercase();

        // Priority 1.5: Review completion detection
        if REVIEW_COMPLETE_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
        {
            tracing::debug!(
                action = "response_analysis",
                priority = 1.5,
                result = "review_complete",
                "CR workflow completion detected"
            );
            return ResponseAction::Completed;
        }

        // Priority 2: Completion detection (substring)
        if COMPLETION_SIGNALS
            .iter()
            .any(|signal| lower.contains(signal))
        {
            tracing::debug!(
                action = "response_analysis",
                priority = 2,
                result = "completed",
                "Completion signal detected"
            );
            return ResponseAction::Completed;
        }

        // Priority 2b: Completion detection (regex)
        if COMPLETION_REGEX_PATTERNS
            .iter()
            .any(|re| re.is_match(response))
        {
            tracing::debug!(
                action = "response_analysis",
                priority = 2.5,
                result = "completed_regex",
                "Completion regex pattern detected"
            );
            return ResponseAction::Completed;
        }

        // Priority 3: Confirmation/proceed patterns
        if PROCEED_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
        {
            tracing::debug!(
                action = "response_analysis",
                priority = 3,
                result = "proceed",
                "Proceed confirmation detected"
            );
            return ResponseAction::Continue {
                reply: "Yes, proceed.".to_string(),
            };
        }

        // Priority 4: Step-by-step detection
        if STEP_BY_STEP_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
        {
            tracing::debug!(
                action = "response_analysis",
                priority = 4,
                result = "step_by_step",
                "Step-by-step detection"
            );
            return ResponseAction::Continue {
                reply: "Continue with all steps. Do not ask for confirmation between steps."
                    .to_string(),
            };
        }

        // Priority 5: YOLO/mode questions
        if YOLO_PATTERNS.iter().any(|pattern| lower.contains(pattern)) {
            tracing::debug!(
                action = "response_analysis",
                priority = 5,
                result = "yolo",
                "YOLO mode question detected"
            );
            return ResponseAction::Continue {
                reply:
                    "Use YOLO mode. Complete all remaining work without asking for confirmation."
                        .to_string(),
            };
        }

        // Priority 5.5: Review fix decision
        if REVIEW_FIX_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
        {
            tracing::debug!(
                action = "response_analysis",
                priority = 5.5,
                result = "review_fix_decision",
                "Review fix decision detected — auto-fixing"
            );
            return ResponseAction::Continue {
                reply: "1".to_string(),
            };
        }

        // Priority 6: Story selection
        if STORY_SELECTION_PATTERNS
            .iter()
            .any(|pattern| lower.contains(pattern))
        {
            tracing::debug!(
                action = "response_analysis",
                priority = 6,
                result = "story_selection",
                story_key = %story_key,
                "Story selection question detected"
            );
            return ResponseAction::Continue {
                reply: story_key.to_string(),
            };
        }

        // Priority 7: Default
        tracing::debug!(
            action = "response_analysis",
            priority = 7,
            result = "default",
            "No pattern matched — sending default continue"
        );
        ResponseAction::Continue {
            reply: "Continue.".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::escalation::EscalationInfo;
    use std::sync::{Arc, Mutex};

    /// Helper: create an empty escalation slot.
    fn empty_slot() -> EscalationSlot {
        Arc::new(Mutex::new(None))
    }

    /// Helper: create an escalation slot with an EscalationInfo.
    fn filled_slot() -> EscalationSlot {
        Arc::new(Mutex::new(Some(EscalationInfo {
            question: "What ORM should we use?".to_string(),
            reason: "Architect session failed".to_string(),
        })))
    }

    #[test]
    fn test_analyzer_detects_sentinel_completion() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // Sentinel on its own line at the end (expected usage)
        let response = "All done.\n\n<<BMAD_JOB_DONE>>";
        assert_eq!(
            analyzer.analyze(response, &slot, "1-1-test"),
            ResponseAction::Completed,
            "Sentinel on its own line should trigger Completed"
        );
    }

    #[test]
    fn test_analyzer_sentinel_embedded_in_text() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // Sentinel embedded mid-message (not ideal but still valid)
        let response = "Story is done. <<BMAD_JOB_DONE>> That's all.";
        assert_eq!(
            analyzer.analyze(response, &slot, "1-1-test"),
            ResponseAction::Completed,
            "Sentinel embedded in text should still trigger Completed"
        );
    }

    #[test]
    fn test_analyzer_sentinel_with_surrounding_whitespace() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        let response = "Summary of work done:\n\n  <<BMAD_JOB_DONE>>  \n";
        assert_eq!(
            analyzer.analyze(response, &slot, "1-1-test"),
            ResponseAction::Completed,
        );
    }

    #[test]
    fn test_analyzer_sentinel_takes_priority_over_escalation() {
        let analyzer = ResponseAnalyzer::new();
        let slot = filled_slot();

        // Sentinel (priority 0) should beat escalation (priority 1)
        let response = "<<BMAD_JOB_DONE>>";
        assert_eq!(
            analyzer.analyze(response, &slot, "1-1-test"),
            ResponseAction::Completed,
            "Sentinel at priority 0 should take precedence over escalation at priority 1"
        );
    }

    #[test]
    fn test_analyzer_sentinel_takes_priority_over_proceed() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // Both sentinel and proceed pattern present — sentinel wins
        let response = "Should I proceed?\n\n<<BMAD_JOB_DONE>>";
        assert_eq!(
            analyzer.analyze(response, &slot, "1-1-test"),
            ResponseAction::Completed,
            "Sentinel should take priority over proceed patterns"
        );
    }

    #[test]
    fn test_analyzer_no_false_positive_sentinel() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // Partial matches should NOT trigger
        let non_sentinels = vec![
            "BMAD_JOB_DONE",
            "<<BMAD_JOB_DONE",
            "BMAD_JOB_DONE>>",
            "<BMAD_JOB_DONE>",
            "<<bmad_job_done>>",
        ];

        for response in non_sentinels {
            let action = analyzer.analyze(response, &slot, "1-1-test");
            assert_ne!(
                action,
                ResponseAction::Completed,
                "Should NOT match partial/wrong-case sentinel: {response}"
            );
        }
    }

    #[test]
    fn test_analyzer_detects_completion_signal() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        let cases = vec![
            "All tasks completed successfully.",
            "The story implementation complete and ready for review.",
            "Dev-story workflow complete — all ACs met.",
            "Story marked as done. Next steps: code review.",
            "The implementation is complete.",
            "All acceptance criteria met, moving to review.",
            "Story is ready for review now.",
        ];

        for response in cases {
            let action = analyzer.analyze(response, &slot, "4-2-test");
            assert_eq!(
                action,
                ResponseAction::Completed,
                "Expected Completed for: {response}"
            );
        }
    }

    #[test]
    fn test_analyzer_detects_proceed_question() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        let cases = vec![
            "I've finished Task 1. Should I proceed to Task 2?",
            "Ready to move on to the next step.",
            "Shall I continue with the implementation?",
            "Continue? I have more tasks to complete.",
            "Would you like me to proceed with the refactoring?",
        ];

        for response in cases {
            let action = analyzer.analyze(response, &slot, "test-key");
            match &action {
                ResponseAction::Continue { reply } => {
                    assert_eq!(reply, "Yes, proceed.", "Wrong reply for: {response}");
                }
                other => panic!("Expected Continue for: {response}, got: {other:?}"),
            }
        }
    }

    #[test]
    fn test_analyzer_detects_step_by_step() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        let cases = vec![
            "I'll go step by step through each task.",
            "Let me walk you through each change.",
            "I'll handle each task individually.",
        ];

        for response in cases {
            let action = analyzer.analyze(response, &slot, "test-key");
            match &action {
                ResponseAction::Continue { reply } => {
                    assert!(
                        reply.contains("Continue with all steps"),
                        "Wrong reply for: {response}, got: {reply}"
                    );
                }
                other => panic!("Expected Continue for: {response}, got: {other:?}"),
            }
        }
    }

    #[test]
    fn test_analyzer_detects_yolo_question() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        let cases = vec![
            "Would you like to enable YOLO mode for this workflow?",
            "[y] YOLO the rest of this document only",
            "Interactive or batch mode?",
        ];

        for response in cases {
            let action = analyzer.analyze(response, &slot, "test-key");
            match &action {
                ResponseAction::Continue { reply } => {
                    assert!(
                        reply.contains("YOLO mode"),
                        "Wrong reply for: {response}, got: {reply}"
                    );
                }
                other => panic!("Expected Continue for: {response}, got: {other:?}"),
            }
        }
    }

    #[test]
    fn test_analyzer_detects_escalation_from_slot() {
        let analyzer = ResponseAnalyzer::new();
        let slot = filled_slot();

        // Even though the response text looks like a completion signal,
        // escalation takes priority because the slot is filled.
        let action = analyzer.analyze("All tasks completed", &slot, "test-key");
        assert_eq!(action, ResponseAction::Escalated);
    }

    #[test]
    fn test_analyzer_escalation_takes_priority_over_completion() {
        let analyzer = ResponseAnalyzer::new();
        let slot = filled_slot();

        let action = analyzer.analyze(
            "Story implementation complete and all acceptance criteria met",
            &slot,
            "test-key",
        );
        assert_eq!(
            action,
            ResponseAction::Escalated,
            "Escalation should take priority over completion"
        );
    }

    #[test]
    fn test_analyzer_default_continues() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        let action = analyzer.analyze(
            "I'm working on implementing the database schema.",
            &slot,
            "test-key",
        );
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: "Continue.".to_string()
            }
        );
    }

    #[test]
    fn test_analyzer_case_insensitive() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // Test completion signals in various cases
        let cases = vec![
            "ALL TASKS COMPLETED",
            "All Tasks Completed",
            "all tasks completed",
            "STORY IMPLEMENTATION COMPLETE",
            "Story Implementation Complete",
        ];

        for response in cases {
            let action = analyzer.analyze(response, &slot, "test-key");
            assert_eq!(
                action,
                ResponseAction::Completed,
                "Expected Completed (case-insensitive) for: {response}"
            );
        }

        // Test proceed patterns in various cases
        let proceed_cases = vec![
            "SHOULD I PROCEED?",
            "Should I Proceed?",
            "should i proceed?",
        ];

        for response in proceed_cases {
            let action = analyzer.analyze(response, &slot, "test-key");
            match &action {
                ResponseAction::Continue { reply } => {
                    assert_eq!(
                        reply, "Yes, proceed.",
                        "Case-insensitive proceed for: {response}"
                    );
                }
                other => panic!("Expected Continue for: {response}, got: {other:?}"),
            }
        }
    }

    #[test]
    fn test_analyzer_completion_various_phrases() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // Each completion signal phrase should trigger Completed
        for signal in COMPLETION_SIGNALS {
            let response = format!("Here is the result: {signal}.");
            let action = analyzer.analyze(&response, &slot, "test-key");
            assert_eq!(
                action,
                ResponseAction::Completed,
                "Expected Completed for signal: {signal}"
            );
        }
    }

    #[test]
    fn test_analyzer_proceed_various_phrases() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        for pattern in PROCEED_PATTERNS {
            let response = format!("I've done the work. {pattern}");
            let action = analyzer.analyze(&response, &slot, "test-key");
            match &action {
                ResponseAction::Continue { reply } => {
                    assert_eq!(
                        reply, "Yes, proceed.",
                        "Expected proceed reply for pattern: {pattern}"
                    );
                }
                other => panic!("Expected Continue for pattern: {pattern}, got: {other:?}"),
            }
        }
    }

    #[test]
    fn test_analyzer_story_selection_replies_with_story_key() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        let cases = vec![
            "Which story would you like me to work on?",
            "Please provide the story file path to develop.",
            "What story should I develop next?",
        ];

        for response in cases {
            let action = analyzer.analyze(response, &slot, "4-2-agent-session-setup-chat-loop");
            match &action {
                ResponseAction::Continue { reply } => {
                    assert_eq!(
                        reply, "4-2-agent-session-setup-chat-loop",
                        "Expected story_key reply for: {response}"
                    );
                }
                other => panic!("Expected Continue with story_key for: {response}, got: {other:?}"),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Review pattern tests (Story 5.2)
    // -----------------------------------------------------------------------
    #[test]
    fn test_analyzer_detects_review_fix_decision() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();
        // Simulates the CR workflow step 4 prompt
        let response = "Here are the findings.\n\nChoose [1], [2], or specify which issue to examine:\n1. Fix them automatically\n2. Create action items\n3. Show me details";
        let action = analyzer.analyze(response, &slot, "5-2-story");
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: "1".to_string()
            },
            "Should auto-respond with '1' to fix automatically"
        );
    }

    #[test]
    fn test_analyzer_detects_fix_automatically_pattern() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();
        let response = "What should I do with these findings?\n[1] Fix them automatically\n[2] Create action items";
        let action = analyzer.analyze(response, &slot, "key");
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: "1".to_string()
            }
        );
    }

    #[test]
    fn test_analyzer_review_fix_does_not_false_positive() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();
        // Normal text with "fix" should NOT trigger review fix pattern
        let response = "I will fix the compilation error in main.rs and then run the tests.";
        let action = analyzer.analyze(response, &slot, "key");
        // Should fall through to default, NOT match review fix patterns
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: "Continue.".to_string()
            }
        );
    }

    #[test]
    fn test_analyzer_detects_review_complete() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();
        let response = "✅ Review Complete!\n\n**Story Status:** done\n**Issues Fixed:** 3\n**Action Items Created:** 0\n\nCode review complete!";
        let action = analyzer.analyze(response, &slot, "key");
        assert_eq!(
            action,
            ResponseAction::Completed,
            "Should detect review completion"
        );
    }

    #[test]
    fn test_analyzer_review_complete_does_not_false_positive() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();
        // Normal text with "complete" should NOT trigger review complete
        let response = "I need to complete the implementation of the parser module.";
        let action = analyzer.analyze(response, &slot, "key");
        assert_ne!(
            action,
            ResponseAction::Completed,
            "Should not false-positive on 'complete' in normal text"
        );
    }

    #[test]
    fn test_analyzer_review_complete_priority_over_fix_patterns() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();
        // Step 5 output contains "Issues Fixed:" which could match REVIEW_FIX_PATTERNS
        // but REVIEW_COMPLETE_PATTERNS is at priority 1.5, before fix patterns at 5.5
        let response =
            "✅ Review Complete!\n\nIssues Fixed: 5\nAction Items Created: 0\nSprint status synced";
        let action = analyzer.analyze(response, &slot, "key");
        assert_eq!(
            action,
            ResponseAction::Completed,
            "Review complete should fire before fix patterns"
        );
    }

    #[test]
    fn test_analyzer_no_false_positive_completion() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // These should NOT trigger completion
        let non_completion = vec![
            "I'll complete the task now.",
            "Working on implementing the feature.",
            "Task 1 is done, moving to task 2.",
            "This step is finished.",
        ];

        for response in non_completion {
            let action = analyzer.analyze(response, &slot, "test-key");
            assert_ne!(
                action,
                ResponseAction::Completed,
                "Should NOT trigger completion for: {response}"
            );
        }
    }

    #[test]
    fn test_analyzer_detects_story_complete_regex() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // Exact pattern from real agent output
        let response = "✅ **Story 7.1: Integration Test Infrastructure & Fixtures — COMPLETE**\n\nSummary: ...";
        let action = analyzer.analyze(response, &slot, "7-1-test");
        assert_eq!(
            action,
            ResponseAction::Completed,
            "Should detect 'Story 7.1: ... COMPLETE' via regex"
        );
    }

    #[test]
    fn test_analyzer_detects_story_complete_regex_various() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        let completions = vec![
            "Story 7.2 — COMPLETE",
            "**Story 1-1: Scaffolding — Complete**",
            "Story 8.5: Agent Integration — complete!",
            "✅ Story 3.4 is now complete.",
        ];

        for response in completions {
            let action = analyzer.analyze(response, &slot, "key");
            assert_eq!(
                action,
                ResponseAction::Completed,
                "Should detect completion for: {response}"
            );
        }
    }

    #[test]
    fn test_analyzer_detects_run_code_review_regex() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // When the agent mentions "run code-review", it means the dev session
        // is done and it's time for review — this IS a completion signal.
        let responses = vec![
            "💡 **Tip:** For best results, run `code-review` using a different LLM.",
            "You should run code-review now.",
            "Run code_review with a fresh context.",
        ];

        for response in responses {
            let action = analyzer.analyze(response, &slot, "key");
            assert_eq!(
                action,
                ResponseAction::Completed,
                "Should detect run code-review for: {response}"
            );
        }
    }

    #[test]
    fn test_analyzer_story_regex_no_false_positive() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // "story" without "complete" nearby should NOT trigger
        let response = "Working on Story 7.1 task 3 now. Building test infrastructure.";
        let action = analyzer.analyze(response, &slot, "key");
        assert_ne!(
            action,
            ResponseAction::Completed,
            "Should NOT trigger completion for story mention without complete"
        );
    }

    #[test]
    fn test_response_action_debug() {
        let action = ResponseAction::Continue {
            reply: "test".to_string(),
        };
        let debug = format!("{action:?}");
        assert!(debug.contains("Continue"));
        assert!(debug.contains("test"));

        let completed = format!("{:?}", ResponseAction::Completed);
        assert!(completed.contains("Completed"));

        let escalated = format!("{:?}", ResponseAction::Escalated);
        assert!(escalated.contains("Escalated"));

        let no_reply = format!("{:?}", ResponseAction::NoReply);
        assert!(no_reply.contains("NoReply"));
    }

    #[test]
    fn test_response_action_clone() {
        let action = ResponseAction::Continue {
            reply: "Yes, proceed.".to_string(),
        };
        let cloned = action.clone();
        assert_eq!(action, cloned);
    }
}
