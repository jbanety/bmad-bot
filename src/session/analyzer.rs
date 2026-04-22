//! Response analyzer — essential-detection engine for LLM agent responses.
//!
//! The [`ResponseAnalyzer`] examines each agent response and determines the
//! appropriate [`ResponseAction`] for the session chat loop. It focuses on
//! essential detections only: workflow completion (via the `<<BMAD_JOB_DONE>>`
//! sentinel and fuzzy fallbacks) and supervisor escalation. Persona-driven
//! menu auto-responses have been removed — skills provide their own protocol.
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
    ///
    /// Used by the consultation mechanism (Story 13.3) to inject critic /
    /// adversarial findings into the chat loop. Not constructed by the analyzer
    /// itself — callers build it when injecting findings.
    Continue {
        /// The reply message to send back to the agent.
        reply: String,
    },

    /// The agent signaled workflow completion — exit the chat loop successfully.
    Completed,

    /// Escalation detected via the shared escalation slot — exit the chat loop.
    Escalated,

    /// The analyzer has nothing specific to say. Call sites decide what to send
    /// (currently `"Continue."`). Kept distinct from `Continue { reply }` so
    /// future callers can inject consultation/critic findings via `Continue`
    /// while still using the generic no-op path when no guidance is needed.
    NoReply,
}

/// Deterministic sentinel token emitted by the agent when its workflow is done.
///
/// Injected in the system preamble (`agent::build_preamble`). The agent is
/// instructed to emit this exact string on its own line as the last thing in
/// its final message. This is checked at **priority 0** — before any fuzzy
/// pattern matching — so it works identically across all LLM providers/models.
pub const JOB_DONE_SENTINEL: &str = "<<BMAD_JOB_DONE>>";

/// Strip agent protocol artifacts from a response before posting to external systems.
///
/// Removes:
/// - `<pr-summary>...</pr-summary>` blocks (including content)
/// - `<<BMAD_JOB_DONE>>` sentinel tokens
/// - Resulting leading/trailing whitespace and excessive blank lines
///
/// Use this to clean agent output before posting as PR comments, notifications,
/// or any user-facing context where protocol markers should not be visible.
pub fn strip_agent_artifacts(text: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    static RE_PR_SUMMARY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?si)<pr-summary>.*?</pr-summary>").unwrap());
    static RE_EXCESSIVE_NEWLINES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());
    // Escape bare `#N` references that GitHub would auto-link to issues/PRs.
    // Uses a capture group for the preceding character instead of lookbehind
    // (Rust regex crate does not support look-around).
    // Group 1 = char before `#` (or empty at start), Group 2 = digit sequence.
    // Preserves HTML entities (&#123;) and URL fragments (/path#1) by only
    // matching when preceded by whitespace, line-start, or common punctuation.
    static RE_GITHUB_ISSUE_REF: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(^|[\s(,:\[])#(\d+)").unwrap());

    let cleaned = RE_PR_SUMMARY.replace_all(text, "");
    let cleaned = cleaned.replace(JOB_DONE_SENTINEL, "");
    // Escape #N → \#N to prevent GitHub auto-linking to issues/PRs
    let cleaned = RE_GITHUB_ISSUE_REF.replace_all(&cleaned, r"$1\#$2");
    let cleaned = RE_EXCESSIVE_NEWLINES.replace_all(&cleaned, "\n\n");
    cleaned.trim().to_string()
}

/// Review completion patterns (case-insensitive).
///
/// Detects CR workflow step 5 completion output. Checked at priority 1.5
/// (after escalation, before dev-session completion signals) as a fuzzy
/// fallback in case the review skill doesn't emit the `<<BMAD_JOB_DONE>>`
/// sentinel reliably.
const REVIEW_COMPLETE_PATTERNS: &[&str] = &[
    "review complete",
    "✅ review complete",
    "code review complete",
    "issues fixed:",
    "action items created:",
    "sprint status synced",
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

/// Stateless response analyzer for the session chat loop.
///
/// Examines agent responses using a strict priority order:
///
/// - **Sentinel** — `<<BMAD_JOB_DONE>>` deterministic token (model-agnostic)
/// - **Escalation** — supervisor escalation slot check
/// - **Review completion** — `REVIEW_COMPLETE_PATTERNS` fuzzy fallback
/// - **Completion signals** — dev-session completion phrases (substring)
/// - **Completion regex** — `COMPLETION_REGEX_PATTERNS` for variable text
/// - **Default** — `NoReply`, call sites decide what (if anything) to send
///
/// Substring matching is case-insensitive.
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
    ///
    /// # Priority Order
    ///
    /// - **Sentinel** — `<<BMAD_JOB_DONE>>` → `Completed`
    /// - **Escalation** — slot contains `Some(EscalationInfo)` → `Escalated`
    /// - **Review completion** — `REVIEW_COMPLETE_PATTERNS` → `Completed`
    /// - **Completion signals** — substring match → `Completed`
    /// - **Completion regex** — regex match → `Completed`
    /// - **Default** — `NoReply`, analyzer has no specific reply to send
    pub fn analyze(&self, response: &str, escalation_slot: &EscalationSlot) -> ResponseAction {
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

        // Priority 2.5: Completion detection (regex)
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

        // Default: no specific reply — call sites decide what to send
        tracing::debug!(
            action = "response_analysis",
            priority = "default",
            result = "no_reply",
            "No pattern matched — returning NoReply"
        );
        ResponseAction::NoReply
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
            analyzer.analyze(response, &slot),
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
            analyzer.analyze(response, &slot),
            ResponseAction::Completed,
            "Sentinel embedded in text should still trigger Completed"
        );
    }

    #[test]
    fn test_analyzer_sentinel_with_surrounding_whitespace() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        let response = "Summary of work done:\n\n  <<BMAD_JOB_DONE>>  \n";
        assert_eq!(analyzer.analyze(response, &slot), ResponseAction::Completed,);
    }

    #[test]
    fn test_analyzer_sentinel_takes_priority_over_escalation() {
        let analyzer = ResponseAnalyzer::new();
        let slot = filled_slot();

        // Sentinel (priority 0) should beat escalation (priority 1)
        let response = "<<BMAD_JOB_DONE>>";
        assert_eq!(
            analyzer.analyze(response, &slot),
            ResponseAction::Completed,
            "Sentinel at priority 0 should take precedence over escalation at priority 1"
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
            let action = analyzer.analyze(response, &slot);
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
            let action = analyzer.analyze(response, &slot);
            assert_eq!(
                action,
                ResponseAction::Completed,
                "Expected Completed for: {response}"
            );
        }
    }

    #[test]
    fn test_analyzer_detects_escalation_from_slot() {
        let analyzer = ResponseAnalyzer::new();
        let slot = filled_slot();

        // Even though the response text looks like a completion signal,
        // escalation takes priority because the slot is filled.
        let action = analyzer.analyze("All tasks completed", &slot);
        assert_eq!(action, ResponseAction::Escalated);
    }

    #[test]
    fn test_analyzer_escalation_takes_priority_over_completion() {
        let analyzer = ResponseAnalyzer::new();
        let slot = filled_slot();

        let action = analyzer.analyze(
            "Story implementation complete and all acceptance criteria met",
            &slot,
        );
        assert_eq!(
            action,
            ResponseAction::Escalated,
            "Escalation should take priority over completion"
        );
    }

    #[test]
    fn test_analyzer_default_is_no_reply() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        let action = analyzer.analyze("I'm working on implementing the database schema.", &slot);
        assert_eq!(action, ResponseAction::NoReply);
    }

    #[test]
    fn test_analyzer_unrecognized_responses_return_no_reply() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // Various unrecognized responses — working text, questions, partial progress
        let cases = vec![
            "Let me think about this...",
            "I'll start by reading the file.",
            "Working on Task 2, currently analyzing the database schema.",
            "Should I use PostgreSQL or MySQL for this?", // question — no longer auto-answered
            "Do you want me to proceed with refactoring?", // proceed pattern — no longer auto-answered
            "Let me walk you through each change step by step.", // step-by-step — no longer auto-answered
            "Would you like YOLO mode or interactive?",          // YOLO — no longer auto-answered
            "Which story would you like me to develop?", // story selection — no longer auto-answered
            "What should I do with these findings? [1] Fix [2] Items", // review fix — no longer auto-answered
            "Here's a partial summary of progress so far.",
        ];

        for response in cases {
            let action = analyzer.analyze(response, &slot);
            assert_eq!(
                action,
                ResponseAction::NoReply,
                "Expected NoReply for unrecognized response: {response}"
            );
        }
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
            let action = analyzer.analyze(response, &slot);
            assert_eq!(
                action,
                ResponseAction::Completed,
                "Expected Completed (case-insensitive) for: {response}"
            );
        }
    }

    #[test]
    fn test_analyzer_completion_various_phrases() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // Each completion signal phrase should trigger Completed
        for signal in COMPLETION_SIGNALS {
            let response = format!("Here is the result: {signal}.");
            let action = analyzer.analyze(&response, &slot);
            assert_eq!(
                action,
                ResponseAction::Completed,
                "Expected Completed for signal: {signal}"
            );
        }
    }

    #[test]
    fn test_analyzer_detects_review_complete() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();
        let response = "✅ Review Complete!\n\n**Story Status:** done\n**Issues Fixed:** 3\n**Action Items Created:** 0\n\nCode review complete!";
        let action = analyzer.analyze(response, &slot);
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
        let action = analyzer.analyze(response, &slot);
        assert_ne!(
            action,
            ResponseAction::Completed,
            "Should not false-positive on 'complete' in normal text"
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
            let action = analyzer.analyze(response, &slot);
            assert_ne!(
                action,
                ResponseAction::Completed,
                "Should NOT trigger completion for: {response}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // strip_agent_artifacts tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_strip_agent_artifacts_removes_pr_summary_block() {
        let input = "Some review text.\n\n<pr-summary>\n<context>\nStuff\n</context>\n</pr-summary>\n\n<<BMAD_JOB_DONE>>";
        let cleaned = strip_agent_artifacts(input);
        assert_eq!(cleaned, "Some review text.");
    }

    #[test]
    fn test_strip_agent_artifacts_removes_sentinel_only() {
        let input = "All done.\n\n<<BMAD_JOB_DONE>>";
        let cleaned = strip_agent_artifacts(input);
        assert_eq!(cleaned, "All done.");
    }

    #[test]
    fn test_strip_agent_artifacts_preserves_normal_text() {
        let input = "This is a normal review comment with no artifacts.";
        let cleaned = strip_agent_artifacts(input);
        assert_eq!(cleaned, input);
    }

    #[test]
    fn test_strip_agent_artifacts_escapes_github_issue_refs() {
        // AC #3 style references must be escaped to prevent GitHub auto-linking
        let input = "AC #3 is met. See AC #12 for details.";
        let cleaned = strip_agent_artifacts(input);
        assert_eq!(cleaned, r"AC \#3 is met. See AC \#12 for details.");
    }

    #[test]
    fn test_strip_agent_artifacts_escapes_bare_hash_refs() {
        let input = "Finding #1: missing error handling\nFinding #2: dead code";
        let cleaned = strip_agent_artifacts(input);
        assert!(cleaned.contains(r"\#1"), "Should escape #1, got: {cleaned}");
        assert!(cleaned.contains(r"\#2"), "Should escape #2, got: {cleaned}");
    }

    #[test]
    fn test_strip_agent_artifacts_preserves_html_entities_and_url_fragments() {
        // &#123; and /path#section should NOT be escaped
        let input = "See &#123; and https://example.com/page#section";
        let cleaned = strip_agent_artifacts(input);
        assert_eq!(cleaned, input);
    }

    #[test]
    fn test_strip_agent_artifacts_escapes_markdown_heading_hash_number() {
        // Headings like "### Findings" should NOT be affected (no digit after #)
        // But "task #5" in text should be escaped
        let input = "### Findings\n- task #5 incomplete";
        let cleaned = strip_agent_artifacts(input);
        assert!(
            cleaned.contains("### Findings"),
            "Markdown headings should be preserved, got: {cleaned}"
        );
        assert!(cleaned.contains(r"\#5"), "Should escape #5, got: {cleaned}");
    }

    #[test]
    fn test_strip_agent_artifacts_escapes_hash_at_line_start() {
        // #42 at the very start of the text should be escaped
        let input = "#42 is the main issue";
        let cleaned = strip_agent_artifacts(input);
        assert_eq!(cleaned, r"\#42 is the main issue");
    }

    #[test]
    fn test_strip_agent_artifacts_escapes_hash_after_open_paren() {
        let input = "see (#7 and #8)";
        let cleaned = strip_agent_artifacts(input);
        assert!(
            cleaned.contains(r"\#7"),
            "Should escape #7 after '(', got: {cleaned}"
        );
    }

    #[test]
    fn test_strip_agent_artifacts_collapses_excessive_newlines() {
        let input = "Before.\n\n\n\n\n<<BMAD_JOB_DONE>>\n\n\n\nAfter.";
        let cleaned = strip_agent_artifacts(input);
        assert_eq!(cleaned, "Before.\n\nAfter.");
    }

    #[test]
    fn test_strip_agent_artifacts_handles_empty_input() {
        assert_eq!(strip_agent_artifacts(""), "");
    }

    #[test]
    fn test_strip_agent_artifacts_pr_summary_multiline() {
        let input = "## Code Review Summary\n\nFindings listed.\n\n<pr-summary>\n<context>\nCtx\n</context>\n<how-to-test>\nTest\n</how-to-test>\n<additional-info>\nInfo\n</additional-info>\n</pr-summary>\n\n<<BMAD_JOB_DONE>>";
        let cleaned = strip_agent_artifacts(input);
        assert_eq!(cleaned, "## Code Review Summary\n\nFindings listed.");
    }

    // -----------------------------------------------------------------------
    // Completion regex tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_analyzer_detects_story_complete_regex() {
        let analyzer = ResponseAnalyzer::new();
        let slot = empty_slot();

        // Exact pattern from real agent output
        let response = "✅ **Story 7.1: Integration Test Infrastructure & Fixtures — COMPLETE**\n\nSummary: ...";
        let action = analyzer.analyze(response, &slot);
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
            let action = analyzer.analyze(response, &slot);
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
            let action = analyzer.analyze(response, &slot);
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
        let action = analyzer.analyze(response, &slot);
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
