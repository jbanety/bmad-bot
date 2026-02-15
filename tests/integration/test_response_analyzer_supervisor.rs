//! Integration tests for ResponseAnalyzer & Supervisor Rules interaction.
//!
//! Story 7.10: Validates the two-layer response handling architecture:
//! - Chat loop layer: `session::analyzer::ResponseAnalyzer`
//! - Tool layer: `supervisor::rules::RuleEngine`
//! - Cross-layer: Escalation slot bridges tool errors to chat loop
//! - Decision layer: Decision logging accumulation across calls

// Core types under test
use bmad_bot::session::analyzer::{ResponseAction, ResponseAnalyzer};
use bmad_bot::session::escalation::EscalationInfo;
use bmad_bot::supervisor::architect::MockAnswerProvider;
use bmad_bot::supervisor::decisions::{DecisionLog, DecisionSource};
use bmad_bot::supervisor::rules::{RuleEngine, RuleResult};
use bmad_bot::supervisor::{AskSupervisor, AskSupervisorArgs, EscalationSlot, SupervisorError};

use rig::tool::Tool;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn empty_slot() -> EscalationSlot {
    Arc::new(Mutex::new(None))
}

fn filled_slot(question: &str, reason: &str) -> EscalationSlot {
    Arc::new(Mutex::new(Some(EscalationInfo {
        question: question.to_string(),
        reason: reason.to_string(),
    })))
}

const TEST_STORY_KEY: &str = "7-10-test-story";

// ===========================================================================
// Task 2: ResponseAnalyzer completion signal tests (AC: 1)
// ===========================================================================

#[test]
fn test_analyzer_completion_all_tasks_completed() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("All tasks completed successfully.", &slot, TEST_STORY_KEY);
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_analyzer_completion_story_implementation_complete() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze(
        "Story implementation complete. All ACs met.",
        &slot,
        TEST_STORY_KEY,
    );
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_analyzer_completion_implementation_is_complete() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("Implementation is complete.", &slot, TEST_STORY_KEY);
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_analyzer_completion_all_acceptance_criteria_met() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze(
        "All acceptance criteria met for this story.",
        &slot,
        TEST_STORY_KEY,
    );
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_analyzer_completion_ready_for_review() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze(
        "The story is ready for review now.",
        &slot,
        TEST_STORY_KEY,
    );
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_analyzer_completion_dev_story_workflow_complete() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("Dev-story workflow complete.", &slot, TEST_STORY_KEY);
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_analyzer_completion_case_insensitive() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    let action = analyzer.analyze("ALL TASKS COMPLETED successfully", &slot, TEST_STORY_KEY);
    assert_eq!(action, ResponseAction::Completed);

    let action = analyzer.analyze("Implementation Is Complete", &slot, TEST_STORY_KEY);
    assert_eq!(action, ResponseAction::Completed);

    let action = analyzer.analyze("READY FOR REVIEW", &slot, TEST_STORY_KEY);
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_analyzer_no_false_positive_completion_ill_complete() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    // "I'll complete the task" should NOT trigger Completed
    let action = analyzer.analyze("I'll complete the task shortly.", &slot, TEST_STORY_KEY);
    assert_ne!(action, ResponseAction::Completed);
}

#[test]
fn test_analyzer_no_false_positive_completion_implementation_of() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    // "implementation of the feature" does not match any completion signal
    let action = analyzer.analyze(
        "I'll start the implementation of the feature.",
        &slot,
        TEST_STORY_KEY,
    );
    assert_ne!(action, ResponseAction::Completed);
}

#[test]
fn test_analyzer_no_false_positive_completion_complete_word() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    // Generic "complete" without strong signal
    let action = analyzer.analyze(
        "Let me complete this function first.",
        &slot,
        TEST_STORY_KEY,
    );
    assert_ne!(action, ResponseAction::Completed);
}

// ===========================================================================
// Task 3: ResponseAnalyzer story selection tests (AC: 2)
// ===========================================================================

#[test]
fn test_analyzer_story_selection_which_story() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze(
        "Which story should I implement next?",
        &slot,
        "2-1-sprint-status",
    );
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "2-1-sprint-status".to_string()
        }
    );
}

#[test]
fn test_analyzer_story_selection_what_story() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze(
        "What story should I work on?",
        &slot,
        "3-2-llm-fallback",
    );
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "3-2-llm-fallback".to_string()
        }
    );
}

#[test]
fn test_analyzer_story_selection_story_to_work_on() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze(
        "I need a story to work on.",
        &slot,
        "5-1-git-provider",
    );
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "5-1-git-provider".to_string()
        }
    );
}

#[test]
fn test_analyzer_story_selection_all_patterns() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let key = "1-1-scaffolding";

    let patterns = [
        "which story should I pick?",
        "I need a story to work on",
        "what story do you recommend?",
        "Please specify a story for me",
        "provide the story file please",
        "What's the story file path?",
        "which story to develop next?",
        "What story would you like me to work on?",
    ];

    for phrase in &patterns {
        let action = analyzer.analyze(phrase, &slot, key);
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: key.to_string()
            },
            "Story selection should match phrase: {phrase}"
        );
    }
}

#[test]
fn test_analyzer_story_selection_different_keys_passed_through() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    let keys = [
        "1-1-scaffolding",
        "7-10-response-analyzer",
        "4-2-agent-session",
    ];

    for key in &keys {
        let action = analyzer.analyze("Which story should I implement?", &slot, key);
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: key.to_string()
            },
            "Story key '{key}' should be passed through"
        );
    }
}

// ===========================================================================
// Task 4: RuleEngine confirmation tests (AC: 3)
// ===========================================================================

#[test]
fn test_rule_engine_confirmation_should_i_proceed() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("Should I proceed with the implementation?");
    match result {
        RuleResult::Matched { rule_name, answer } => {
            assert_eq!(rule_name, "confirmation_proceed");
            assert_eq!(answer, "Yes, proceed.");
        }
        RuleResult::NoMatch => panic!("Expected Matched for confirmation pattern"),
    }
}

#[test]
fn test_rule_engine_confirmation_shall_i_continue() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("Shall I continue with the next subtask?");
    match result {
        RuleResult::Matched { rule_name, answer } => {
            assert_eq!(rule_name, "confirmation_proceed");
            assert_eq!(answer, "Yes, proceed.");
        }
        RuleResult::NoMatch => panic!("Expected Matched for 'shall i continue'"),
    }
}

#[test]
fn test_rule_engine_permission_should_i_create() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("Should I create the new module?");
    match result {
        RuleResult::Matched { rule_name, answer } => {
            assert_eq!(rule_name, "permission_action");
            assert_eq!(answer, "Yes, proceed with the action as described.");
        }
        RuleResult::NoMatch => panic!("Expected Matched for permission pattern"),
    }
}

#[test]
fn test_rule_engine_permission_should_i_modify() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("Should I modify the existing struct?");
    match result {
        RuleResult::Matched { rule_name, answer } => {
            assert_eq!(rule_name, "permission_action");
            assert_eq!(answer, "Yes, proceed with the action as described.");
        }
        RuleResult::NoMatch => panic!("Expected Matched for 'should i modify'"),
    }
}

#[test]
fn test_rule_engine_confirmation_no_llm_involved() {
    // Verify that rule engine operates purely in-memory with no external calls
    let engine = RuleEngine::new();

    // All these should resolve instantly via rules — no LLM
    let questions = [
        "Should I proceed?",
        "Shall I continue?",
        "Should I create the file?",
        "Do you want me to proceed?",
    ];

    for q in &questions {
        let result = engine.evaluate(q);
        assert!(
            matches!(result, RuleResult::Matched { .. }),
            "Rule engine should match '{q}' without LLM"
        );
    }
}

// ===========================================================================
// Task 5: RuleEngine no-match fallthrough tests (AC: 4)
// ===========================================================================

#[test]
fn test_rule_engine_nomatch_substantive_question() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("What database schema should I use for user sessions?");
    assert!(
        matches!(result, RuleResult::NoMatch),
        "Substantive technical question should not match any rule"
    );
}

#[test]
fn test_rule_engine_nomatch_ambiguous_technical() {
    let engine = RuleEngine::new();
    let result =
        engine.evaluate("How should I handle authentication token refresh in the middleware?");
    assert!(
        matches!(result, RuleResult::NoMatch),
        "Ambiguous technical question should not match any rule"
    );
}

#[test]
fn test_rule_engine_nomatch_empty_string() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("");
    assert!(
        matches!(result, RuleResult::NoMatch),
        "Empty string should return NoMatch"
    );
}

#[test]
fn test_rule_engine_nomatch_random_text() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("The quick brown fox jumps over the lazy dog.");
    assert!(
        matches!(result, RuleResult::NoMatch),
        "Random text should return NoMatch"
    );
}

// ===========================================================================
// Task 6: ResponseAnalyzer step-by-step and YOLO detection tests (AC: 5)
// ===========================================================================

#[test]
fn test_analyzer_step_by_step_detection() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    let phrases = [
        "I'll work through this step by step...",
        "Let me handle these one at a time",
        "I'll walk you through each change",
        "Let me do each step separately",
    ];

    for phrase in &phrases {
        let action = analyzer.analyze(phrase, &slot, TEST_STORY_KEY);
        match &action {
            ResponseAction::Continue { reply } => {
                assert!(
                    reply.to_lowercase().contains("continue")
                        || reply.to_lowercase().contains("step"),
                    "Step-by-step reply should contain directive, got: {reply}"
                );
            }
            other => panic!("Expected Continue for step-by-step phrase '{phrase}', got: {other:?}"),
        }
    }
}

#[test]
fn test_analyzer_yolo_detection() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    let phrases = [
        "Should I use yolo mode?",
        "Do you want batch mode execution?",
        "Want yolo for the rest?",
        "[y] yolo the remaining tasks",
    ];

    for phrase in &phrases {
        let action = analyzer.analyze(phrase, &slot, TEST_STORY_KEY);
        match &action {
            ResponseAction::Continue { reply } => {
                assert!(
                    reply.to_lowercase().contains("yolo")
                        || reply.to_lowercase().contains("confirmation"),
                    "YOLO reply should mention yolo or confirmation, got: {reply}"
                );
            }
            other => panic!("Expected Continue for YOLO phrase '{phrase}', got: {other:?}"),
        }
    }
}

#[test]
fn test_analyzer_step_by_step_priority_over_yolo() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    // A response containing both step-by-step and yolo patterns
    // Step-by-step is priority 4, YOLO is priority 5, so step-by-step wins
    let action = analyzer.analyze(
        "I'll do this step by step. Want yolo mode?",
        &slot,
        TEST_STORY_KEY,
    );

    match &action {
        ResponseAction::Continue { reply } => {
            // Should match step-by-step (priority 4), not YOLO (priority 5)
            assert!(
                reply.to_lowercase().contains("continue")
                    || reply.to_lowercase().contains("step"),
                "Step-by-step should win over YOLO, got: {reply}"
            );
        }
        other => panic!("Expected Continue, got: {other:?}"),
    }
}

// ===========================================================================
// Task 7: Cross-module escalation slot integration tests (AC: 6)
// ===========================================================================

#[test]
fn test_analyzer_escalation_from_slot() {
    let analyzer = ResponseAnalyzer::new();
    let slot = filled_slot("What DB?", "Architect failed");

    let action = analyzer.analyze("Some response text", &slot, TEST_STORY_KEY);
    assert_eq!(action, ResponseAction::Escalated);
}

#[test]
fn test_analyzer_escalation_regardless_of_response_text() {
    let analyzer = ResponseAnalyzer::new();
    let slot = filled_slot("Some question", "Some reason");

    // Even with completion signal text, escalation should win (priority 1 > priority 2)
    let action = analyzer.analyze(
        "Implementation is complete. All tasks completed.",
        &slot,
        TEST_STORY_KEY,
    );
    assert_eq!(action, ResponseAction::Escalated);
}

#[test]
fn test_analyzer_escalation_takes_priority_over_completion() {
    let analyzer = ResponseAnalyzer::new();
    let slot = filled_slot("What schema?", "Architect failed");

    // Escalation (priority 1) should beat completion (priority 2)
    let action = analyzer.analyze("All tasks completed", &slot, TEST_STORY_KEY);
    assert_eq!(action, ResponseAction::Escalated);
}

#[test]
fn test_analyzer_no_escalation_with_empty_slot() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    // With empty slot, regular pattern matching should apply
    let action = analyzer.analyze("Some generic text", &slot, TEST_STORY_KEY);
    assert_ne!(action, ResponseAction::Escalated);
}

// ===========================================================================
// Task 8: AskSupervisor decision logging integration tests (AC: 7, 8)
// ===========================================================================

#[tokio::test]
async fn test_ask_supervisor_rule_match_records_rule_engine_decision() {
    let supervisor = AskSupervisor::new();
    let log = supervisor.decision_log();

    let args = AskSupervisorArgs {
        question: "Should I proceed with the implementation?".to_string(),
        context: None,
    };

    let result = supervisor.call(args).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Yes, proceed.");

    // Verify decision was recorded
    assert_eq!(log.len(), 1);
    let records = log.records();
    match &records[0].source {
        DecisionSource::RuleEngine { rule_name } => {
            assert_eq!(rule_name, "confirmation_proceed");
        }
        other => panic!("Expected RuleEngine decision, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_ask_supervisor_no_match_no_architect_records_escalation() {
    let supervisor = AskSupervisor::new();
    let log = supervisor.decision_log();

    let args = AskSupervisorArgs {
        question: "What database schema should I use?".to_string(),
        context: None,
    };

    let result = supervisor.call(args).await;
    assert!(matches!(
        result,
        Err(SupervisorError::LlmFallbackNotImplemented)
    ));

    // Verify escalation decision was logged
    assert_eq!(log.len(), 1);
    let records = log.records();
    assert!(
        matches!(&records[0].source, DecisionSource::Escalation),
        "Expected Escalation decision source"
    );
}

#[tokio::test]
async fn test_ask_supervisor_multiple_calls_accumulate_decisions() {
    let supervisor = AskSupervisor::new();
    let log = supervisor.decision_log();

    // Call 1: rule match
    let args1 = AskSupervisorArgs {
        question: "Should I proceed?".to_string(),
        context: None,
    };
    let _ = supervisor.call(args1).await;

    // Call 2: no match → escalation
    let args2 = AskSupervisorArgs {
        question: "What DB schema should I use?".to_string(),
        context: None,
    };
    let _ = supervisor.call(args2).await;

    // Call 3: another rule match
    let args3 = AskSupervisorArgs {
        question: "Should I create the file?".to_string(),
        context: None,
    };
    let _ = supervisor.call(args3).await;

    // Should have 3 decisions accumulated
    assert_eq!(log.len(), 3);
    let records = log.records();
    assert!(matches!(
        &records[0].source,
        DecisionSource::RuleEngine { .. }
    ));
    assert!(matches!(&records[1].source, DecisionSource::Escalation));
    assert!(matches!(
        &records[2].source,
        DecisionSource::RuleEngine { .. }
    ));
}

#[tokio::test]
async fn test_ask_supervisor_decision_log_len_and_records_reflect_all_calls() {
    let supervisor = AskSupervisor::new();
    let log = supervisor.decision_log();

    assert_eq!(log.len(), 0);
    assert!(log.is_empty());
    assert!(log.records().is_empty());

    let args = AskSupervisorArgs {
        question: "Should I proceed?".to_string(),
        context: None,
    };
    let _ = supervisor.call(args).await;

    assert_eq!(log.len(), 1);
    assert!(!log.is_empty());
    assert_eq!(log.records().len(), 1);
}

// ===========================================================================
// Task 9: Review pattern integration tests (AC: 9, 10)
// ===========================================================================

#[test]
fn test_analyzer_review_complete_patterns() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    let phrases = [
        "✅ Review complete",
        "Code review complete",
        "Review complete — all checks passed",
        "Issues Fixed: 3 items resolved",
        "Action Items Created: 2",
        "Sprint Status Synced successfully",
    ];

    for phrase in &phrases {
        let action = analyzer.analyze(phrase, &slot, TEST_STORY_KEY);
        assert_eq!(
            action,
            ResponseAction::Completed,
            "Review complete pattern should trigger Completed for: {phrase}"
        );
    }
}

#[test]
fn test_analyzer_review_fix_patterns() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    let phrases = [
        "Fix them automatically",
        "Create action items for the team",
        "Show me details of the findings",
        "Choose [1] to fix automatically",
        "[1] Fix them now",
    ];

    for phrase in &phrases {
        let action = analyzer.analyze(phrase, &slot, TEST_STORY_KEY);
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: "1".to_string()
            },
            "Review fix pattern should trigger Continue(1) for: {phrase}"
        );
    }
}

#[test]
fn test_analyzer_review_complete_priority_over_review_fix() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    // A review summary that contains both "review complete" (priority 1.5)
    // and fix-related text. Review complete should win.
    let summary = "✅ Review complete\n\nIssues Fixed: 5 items resolved\n\nCreate action items for remaining work";
    let action = analyzer.analyze(summary, &slot, TEST_STORY_KEY);
    assert_eq!(
        action,
        ResponseAction::Completed,
        "Review complete (1.5) should take priority over review fix (5.5)"
    );
}

#[test]
fn test_analyzer_review_complete_not_false_positive_on_normal_completion() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    // Normal dev completion signals should NOT trigger via review complete patterns
    // "all tasks completed" is a dev completion signal, not a review pattern
    let action = analyzer.analyze("All tasks completed successfully.", &slot, TEST_STORY_KEY);
    // This should trigger Completed via dev completion signals (priority 2), which is fine
    assert_eq!(action, ResponseAction::Completed);

    // But "I'm reviewing the code" should NOT trigger review complete
    let action = analyzer.analyze(
        "I'm reviewing the code and making improvements.",
        &slot,
        TEST_STORY_KEY,
    );
    assert_ne!(
        action,
        ResponseAction::Completed,
        "Generic review text should not trigger review complete"
    );
}

// ===========================================================================
// Task 10: Full pipeline integration tests (AC: 3, 4, 6, 7, 8)
// ===========================================================================

#[tokio::test]
async fn test_pipeline_confirmation_via_rule_engine_then_analyzer_no_escalation() {
    // Full flow: agent asks confirmation → rule engine matches → analyzer sees no escalation → Continue
    let slot: EscalationSlot = Arc::new(Mutex::new(None));
    let supervisor = AskSupervisor::with_answer_provider_and_slot(
        Box::new(MockAnswerProvider {
            response: String::new(),
            should_fail: false,
        }),
        slot.clone(),
    );

    // Step 1: Rule engine answers the confirmation question
    let args = AskSupervisorArgs {
        question: "Should I proceed with the implementation?".to_string(),
        context: None,
    };
    let result = supervisor.call(args).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Yes, proceed.");

    // Step 2: Escalation slot should still be empty (rule match doesn't escalate)
    {
        let guard = slot.lock().expect("slot lock");
        assert!(
            guard.is_none(),
            "Slot should remain empty after rule match"
        );
    }

    // Step 3: Analyzer sees no escalation — processes response text normally
    let analyzer = ResponseAnalyzer::new();
    let action = analyzer.analyze(
        "Should I proceed with the next task?",
        &slot,
        TEST_STORY_KEY,
    );
    // "Should I proceed" matches PROCEED_PATTERNS → Continue { "Yes, proceed." }
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "Yes, proceed.".to_string()
        }
    );
}

#[tokio::test]
async fn test_pipeline_unknown_question_architect_fails_escalation_detected() {
    // Full flow: agent asks unknown question → rule engine misses → failing MockAnswerProvider
    // → supervisor escalates → analyzer sees escalation slot → Escalated

    // 1. Shared escalation slot — same Arc passed to both supervisor and analyzer
    let slot: EscalationSlot = Arc::new(Mutex::new(None));

    // 2. Failing architect mock — triggers escalation path
    let mock = MockAnswerProvider {
        response: String::new(),
        should_fail: true,
    };

    // 3. Wire AskSupervisor with shared slot
    let supervisor =
        AskSupervisor::with_answer_provider_and_slot(Box::new(mock), slot.clone());

    // 4. Ask a question that doesn't match any rule → architect fails → escalation
    let args = AskSupervisorArgs {
        question: "What database schema should I use for user sessions?".to_string(),
        context: None,
    };
    let result = supervisor.call(args).await;
    assert!(
        matches!(result, Err(SupervisorError::EscalationRequired { .. })),
        "Expected EscalationRequired, got: {result:?}"
    );

    // 5. Verify the slot was written BEFORE the error was returned
    {
        let guard = slot.lock().expect("slot lock");
        assert!(
            guard.is_some(),
            "Escalation slot should contain EscalationInfo"
        );
        let info = guard.as_ref().unwrap();
        assert_eq!(
            info.question,
            "What database schema should I use for user sessions?"
        );
        assert!(
            info.reason.contains("Architect session failed"),
            "Reason should mention architect failure, got: {}",
            info.reason
        );
    }

    // 6. ResponseAnalyzer reads the same slot — detects escalation at priority 1
    let analyzer = ResponseAnalyzer::new();
    let action = analyzer.analyze(
        "Here is some irrelevant response text",
        &slot,
        TEST_STORY_KEY,
    );
    assert_eq!(action, ResponseAction::Escalated);

    // 7. Verify decision was logged
    let log = supervisor.decision_log();
    assert_eq!(log.len(), 1);
    let records = log.records();
    assert!(
        matches!(&records[0].source, DecisionSource::Escalation),
        "Expected Escalation decision source"
    );
}

#[tokio::test]
async fn test_pipeline_completion_signal_rule_engine_not_involved() {
    // Full flow: agent signals completion → analyzer returns Completed (rule engine not involved)
    let slot: EscalationSlot = Arc::new(Mutex::new(None));

    // The analyzer processes completion signals directly — no supervisor involvement
    let analyzer = ResponseAnalyzer::new();
    let action = analyzer.analyze(
        "Implementation is complete. All acceptance criteria met.",
        &slot,
        TEST_STORY_KEY,
    );
    assert_eq!(action, ResponseAction::Completed);

    // The escalation slot should remain empty
    {
        let guard = slot.lock().expect("slot lock");
        assert!(
            guard.is_none(),
            "Slot should remain empty — completion is chat-loop only"
        );
    }
}
