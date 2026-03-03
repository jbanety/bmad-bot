//! Integration tests for ResponseAnalyzer & Supervisor Rules interaction.
//!
//! Story 7.10: Validates the two-layer response handling architecture:
//! - Chat loop layer (session::analyzer) — workflow-level interactions
//! - Tool layer (supervisor::rules) — substantive questions via ask_supervisor
//! - Cross-layer (escalation slot bridges tool errors to chat loop)
//! - Decision layer (supervisor::decisions) — decision logging accumulation

// Core types under test
use bmad_bot::session::analyzer::{ResponseAction, ResponseAnalyzer};
use bmad_bot::session::escalation::EscalationInfo;
use bmad_bot::supervisor::architect::MockAnswerProvider;
use bmad_bot::supervisor::decisions::DecisionSource;
use bmad_bot::supervisor::rules::{RuleEngine, RuleResult};
use bmad_bot::supervisor::{AskSupervisor, AskSupervisorArgs, EscalationSlot, SupervisorError};

// Utilities
use rig::tool::Tool;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create an empty escalation slot (no escalation).
fn empty_slot() -> EscalationSlot {
    Arc::new(Mutex::new(None))
}

/// Create a slot pre-filled with escalation info.
fn filled_slot(question: &str, reason: &str) -> EscalationSlot {
    Arc::new(Mutex::new(Some(EscalationInfo {
        question: question.to_string(),
        reason: reason.to_string(),
    })))
}

// ===========================================================================
// Task 2: ResponseAnalyzer completion signal tests (AC: 1)
// ===========================================================================

#[test]
fn test_completion_signal_all_tasks_completed() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("All tasks completed successfully.", &slot, "7-10");
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_completion_signal_implementation_complete() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze(
        "Implementation is complete. All acceptance criteria met.",
        &slot,
        "7-10",
    );
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_completion_signal_story_ready_for_review() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("Story is ready for review.", &slot, "7-10");
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_completion_signal_dev_story_workflow_complete() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("Dev-story workflow complete.", &slot, "7-10");
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_completion_signal_story_marked_as_done() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("Story marked as done.", &slot, "7-10");
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_completion_signal_all_acceptance_criteria_met() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("All acceptance criteria met for the story.", &slot, "7-10");
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_completion_signal_case_insensitive() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("ALL TASKS COMPLETED SUCCESSFULLY!", &slot, "7-10");
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_completion_signal_mixed_case() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("Story Implementation Complete.", &slot, "7-10");
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_no_false_positive_completion_will_complete() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    // "I'll complete the task" should NOT trigger completion
    let action = analyzer.analyze("I'll complete the task next.", &slot, "7-10");
    assert_ne!(action, ResponseAction::Completed);
}

#[test]
fn test_no_false_positive_completion_implementation_of() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    // "implementation of the feature" should NOT trigger completion
    let action = analyzer.analyze(
        "I'm working on the implementation of the feature.",
        &slot,
        "7-10",
    );
    assert_ne!(action, ResponseAction::Completed);
}

#[test]
fn test_no_false_positive_completion_completing_step() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("I am completing step 3 now.", &slot, "7-10");
    assert_ne!(action, ResponseAction::Completed);
}

#[test]
fn test_no_false_positive_completion_ready_for_review_future_tense() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    // Future-tense "ready for review" should NOT trigger completion — the agent
    // is describing future intent, not signaling the story is done now.
    let action = analyzer.analyze("I'll have it ready for review by tomorrow.", &slot, "7-10");
    assert_ne!(
        action,
        ResponseAction::Completed,
        "Future-tense 'ready for review' must not false-positive as a completion signal"
    );
}

// ===========================================================================
// Task 3: ResponseAnalyzer story selection tests (AC: 2)
// ===========================================================================

#[test]
fn test_story_selection_which_story() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("Which story should I implement?", &slot, "7-10-test");
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "7-10-test".to_string()
        }
    );
}

#[test]
fn test_story_selection_what_story() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("What story should I work on?", &slot, "3-2-auth");
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "3-2-auth".to_string()
        }
    );
}

#[test]
fn test_story_selection_story_to_work_on() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze(
        "Can you tell me the story to work on?",
        &slot,
        "1-1-init",
    );
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "1-1-init".to_string()
        }
    );
}

#[test]
fn test_story_selection_provide_the_story() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("Please provide the story file path.", &slot, "5-3-pr");
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "5-3-pr".to_string()
        }
    );
}

#[test]
fn test_story_selection_different_keys_passthrough() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let keys = ["7-10-test", "1-1-init", "3-3-escalation", "99-99-edge"];
    for key in keys {
        let action = analyzer.analyze("Which story should I develop?", &slot, key);
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: key.to_string()
            }
        );
    }
}

#[test]
fn test_story_selection_all_patterns() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let phrases = [
        "which story should I work on?",
        "story to work on please",
        "what story is next?",
        "specify a story for me",
        "provide the story details",
        "what's the story file path?",
        "which story to develop next?",
        "story would you like me to implement?",
    ];
    for phrase in phrases {
        let action = analyzer.analyze(phrase, &slot, "test-key");
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: "test-key".to_string()
            },
            "Story selection not detected for: {phrase}"
        );
    }
}

#[test]
fn test_no_false_positive_story_selection_declarative() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    // Declarative sentences mentioning "story" should NOT trigger story-selection.
    // E.g. an agent narrating about a story rather than asking which one to work on.
    let declarative = "The story would you like to review has already been merged.";
    let action = analyzer.analyze(declarative, &slot, "7-10");
    assert_ne!(
        action,
        ResponseAction::Continue {
            reply: "7-10".to_string()
        },
        "Declarative sentence must not be mistaken for a story-selection question"
    );
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
        RuleResult::NoMatch => panic!("Expected match for 'Should I proceed'"),
    }
}

#[test]
fn test_rule_engine_confirmation_shall_i_continue() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("Shall I continue with the next task?");
    match result {
        RuleResult::Matched { rule_name, answer } => {
            assert_eq!(rule_name, "confirmation_proceed");
            assert_eq!(answer, "Yes, proceed.");
        }
        RuleResult::NoMatch => panic!("Expected match for 'Shall I continue'"),
    }
}

#[test]
fn test_rule_engine_confirmation_various_patterns() {
    let engine = RuleEngine::new();
    let patterns = [
        "should i proceed with this?",
        "shall i continue?",
        "do you want me to implement this?",
        "can i go ahead with the refactor?",
        "may i proceed with the changes?",
        "want me to continue with the next subtask?",
        "should i go ahead and create the file?",
        "shall i proceed to the next task?",
        "ok to proceed?",
    ];
    for pattern in patterns {
        let result = engine.evaluate(pattern);
        assert!(
            matches!(result, RuleResult::Matched { .. }),
            "Expected match for confirmation pattern: {pattern}"
        );
    }
}

#[test]
fn test_rule_engine_permission_should_i_create() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("Should I create the new module file?");
    match result {
        RuleResult::Matched { rule_name, answer } => {
            assert_eq!(rule_name, "permission_action");
            assert_eq!(answer, "Yes, proceed with the action as described.");
        }
        RuleResult::NoMatch => panic!("Expected match for 'Should I create'"),
    }
}

#[test]
fn test_rule_engine_permission_various_patterns() {
    let engine = RuleEngine::new();
    let patterns = [
        "Should I modify the existing handler?",
        "Should I delete the old test file?",
        "Should I update the Cargo.toml?",
        "Should I add a new dependency?",
        "Should I remove the deprecated function?",
        "Should I refactor the module structure?",
        "Should I implement the new trait?",
        "Can I create a helper function?",
        "Can I modify this struct?",
        "Can I delete the unused imports?",
        "Can I update the configuration?",
    ];
    for pattern in patterns {
        let result = engine.evaluate(pattern);
        assert!(
            matches!(
                result,
                RuleResult::Matched {
                    rule_name,
                    ..
                } if rule_name == "permission_action"
            ),
            "Expected permission_action match for: {pattern}"
        );
    }
}

#[test]
fn test_rule_engine_no_llm_involved() {
    // RuleEngine is purely in-memory pattern matching — no architect/LLM involved.
    // This test verifies RuleEngine operates independently: create, evaluate, done.
    let engine = RuleEngine::new();
    assert!(engine.rule_count() >= 6, "RuleEngine should have at least 6 default rules; update this bound if rules are intentionally added"); // no LLM setup
    let result = engine.evaluate("Should I proceed?");
    assert!(matches!(result, RuleResult::Matched { .. }));
    // If we got here without any async/network calls, no LLM was involved.
}

// ===========================================================================
// Task 5: RuleEngine no-match fallthrough tests (AC: 4)
// ===========================================================================

#[test]
fn test_rule_engine_no_match_substantive_question() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("What database schema should I use for user sessions?");
    assert!(matches!(result, RuleResult::NoMatch));
}

#[test]
fn test_rule_engine_no_match_ambiguous_technical() {
    let engine = RuleEngine::new();
    let result =
        engine.evaluate("Should I use PostgreSQL or SQLite for the session store?");
    assert!(matches!(result, RuleResult::NoMatch));
}

#[test]
fn test_rule_engine_no_match_empty_string() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("");
    assert!(matches!(result, RuleResult::NoMatch));
}

#[test]
fn test_rule_engine_no_match_random_text() {
    let engine = RuleEngine::new();
    let result = engine.evaluate("The weather is nice today.");
    assert!(matches!(result, RuleResult::NoMatch));
}

#[test]
fn test_rule_engine_no_match_architectural_question() {
    let engine = RuleEngine::new();
    let result = engine.evaluate(
        "How should I structure the error handling across the supervisor and session modules?",
    );
    assert!(matches!(result, RuleResult::NoMatch));
}

// ===========================================================================
// Task 6: ResponseAnalyzer step-by-step and YOLO detection tests (AC: 5)
// ===========================================================================

#[test]
fn test_step_by_step_detection_basic() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze(
        "I'll work through this step by step...",
        &slot,
        "7-10",
    );
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "Continue with all steps. Do not ask for confirmation between steps."
                .to_string()
        }
    );
}

#[test]
fn test_step_by_step_detection_various_phrases() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let phrases = [
        "Let me walk you through each change.",
        "I'll handle each task individually.",
        "Let me do each step separately.",
        "I'll go one at a time.",
        "One step at a time, here we go.",
        "Let me walk through each file change.",
        "Shall I do them one by one?",
    ];
    for phrase in phrases {
        let action = analyzer.analyze(phrase, &slot, "7-10");
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: "Continue with all steps. Do not ask for confirmation between steps."
                    .to_string()
            },
            "Step-by-step not detected for: {phrase}"
        );
    }
}

#[test]
fn test_yolo_detection_basic() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("Do you want yolo mode or interactive?", &slot, "7-10");
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "Use YOLO mode. Complete all remaining work without asking for confirmation."
                .to_string()
        }
    );
}

#[test]
fn test_yolo_detection_various_phrases() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let phrases = [
        "Should I enable yolo mode?",
        "Do you want batch mode for this?",
        "yolo or interactive?",
        "interactive or batch mode?",
        "[y] YOLO the rest",
    ];
    for phrase in phrases {
        let action = analyzer.analyze(phrase, &slot, "7-10");
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply:
                    "Use YOLO mode. Complete all remaining work without asking for confirmation."
                        .to_string()
            },
            "YOLO not detected for: {phrase}"
        );
    }
}

#[test]
fn test_step_by_step_priority_over_yolo() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    // A message containing both step-by-step and YOLO patterns:
    // step-by-step is priority 4, YOLO is priority 5, so step-by-step wins.
    let action = analyzer.analyze(
        "I'll go step by step, or do you want yolo mode?",
        &slot,
        "7-10",
    );
    assert_eq!(
        action,
        ResponseAction::Continue {
            reply: "Continue with all steps. Do not ask for confirmation between steps."
                .to_string()
        }
    );
}

// ===========================================================================
// Task 7: Cross-module escalation slot integration tests (AC: 6)
// ===========================================================================

#[test]
fn test_escalation_slot_returns_escalated() {
    let analyzer = ResponseAnalyzer::new();
    let slot = filled_slot(
        "What DB schema should I use?",
        "Architect session failed: connection timeout",
    );
    let action = analyzer.analyze("Some response text.", &slot, "7-10");
    assert_eq!(action, ResponseAction::Escalated);
}

#[test]
fn test_escalation_slot_ignores_response_content() {
    let analyzer = ResponseAnalyzer::new();
    let slot = filled_slot("question", "reason");
    // Even with a completion signal, escalation takes priority
    let action = analyzer.analyze("All tasks completed.", &slot, "7-10");
    assert_eq!(action, ResponseAction::Escalated);
}

#[test]
fn test_escalation_takes_priority_over_completion() {
    let analyzer = ResponseAnalyzer::new();
    let slot = filled_slot("q", "r");
    let action = analyzer.analyze(
        "Implementation is complete. All acceptance criteria met.",
        &slot,
        "7-10",
    );
    assert_eq!(action, ResponseAction::Escalated);
}

#[test]
fn test_escalation_takes_priority_over_proceed() {
    let analyzer = ResponseAnalyzer::new();
    let slot = filled_slot("q", "r");
    let action = analyzer.analyze("Should I proceed with the implementation?", &slot, "7-10");
    assert_eq!(action, ResponseAction::Escalated);
}

#[test]
fn test_empty_slot_no_escalation() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let action = analyzer.analyze("Some response text.", &slot, "7-10");
    // Should NOT be escalated — should be default Continue
    assert_ne!(action, ResponseAction::Escalated);
}

// ===========================================================================
// Task 8: AskSupervisor decision logging integration tests (AC: 7, 8)
// ===========================================================================

#[tokio::test]
async fn test_rule_match_records_rule_engine_decision() {
    let supervisor = AskSupervisor::new();
    let args = AskSupervisorArgs {
        question: "Should I proceed with the implementation?".to_string(),
        context: None,
    };
    let result = supervisor.call(args).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Yes, proceed.");

    let log = supervisor.decision_log();
    assert_eq!(log.len(), 1);
    let records = log.records();
    assert!(
        matches!(
            &records[0].source,
            DecisionSource::RuleEngine { rule_name } if rule_name == "confirmation_proceed"
        ),
        "Expected RuleEngine source with confirmation_proceed, got: {:?}",
        records[0].source
    );
}

#[tokio::test]
async fn test_no_match_no_architect_records_escalation() {
    let supervisor = AskSupervisor::new(); // no architect provider
    let args = AskSupervisorArgs {
        question: "What database schema should I use for user sessions?".to_string(),
        context: None,
    };
    let result = supervisor.call(args).await;
    assert!(matches!(
        result,
        Err(SupervisorError::LlmFallbackNotImplemented)
    ));

    let log = supervisor.decision_log();
    assert_eq!(log.len(), 1);
    let records = log.records();
    assert!(
        matches!(&records[0].source, DecisionSource::Escalation),
        "Expected Escalation source, got: {:?}",
        records[0].source
    );
}

#[tokio::test]
async fn test_multiple_calls_accumulate_decisions() {
    let supervisor = AskSupervisor::new();

    // Call 1: rule match
    let args1 = AskSupervisorArgs {
        question: "Should I proceed?".to_string(),
        context: None,
    };
    let _ = supervisor.call(args1).await;

    // Call 2: another rule match
    let args2 = AskSupervisorArgs {
        question: "Shall I continue with the next task?".to_string(),
        context: None,
    };
    let _ = supervisor.call(args2).await;

    // Call 3: no match → escalation
    let args3 = AskSupervisorArgs {
        question: "What NoSQL database should I use?".to_string(),
        context: None,
    };
    let _ = supervisor.call(args3).await;

    let log = supervisor.decision_log();
    assert_eq!(log.len(), 3);
    let records = log.records();
    assert!(matches!(
        &records[0].source,
        DecisionSource::RuleEngine { .. }
    ));
    assert!(matches!(
        &records[1].source,
        DecisionSource::RuleEngine { .. }
    ));
    assert!(matches!(&records[2].source, DecisionSource::Escalation));
}

#[tokio::test]
async fn test_decision_log_len_and_records_reflect_all_calls() {
    let supervisor = AskSupervisor::new();

    let log = supervisor.decision_log();
    assert_eq!(log.len(), 0);
    assert!(log.is_empty());

    let _ = supervisor
        .call(AskSupervisorArgs {
            question: "Should I proceed?".to_string(),
            context: None,
        })
        .await;

    // log is a clone sharing the same Arc — it reflects the new record
    assert_eq!(log.len(), 1);
    assert!(!log.is_empty());
    assert_eq!(log.records().len(), 1);
}

// ===========================================================================
// Task 9: Review pattern integration tests (AC: 9, 10)
// ===========================================================================

#[test]
fn test_review_complete_patterns_trigger_completed() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let phrases = [
        "✅ Review complete",
        "Review complete. 3 issues found.",
        "Code review complete — all checks passed.",
        "Issues Fixed: 5 out of 5",
        "Action Items Created: 2",
        "Sprint status synced with review outcome.",
    ];
    for phrase in phrases {
        let action = analyzer.analyze(phrase, &slot, "7-10");
        assert_eq!(
            action,
            ResponseAction::Completed,
            "Review complete not detected for: {phrase}"
        );
    }
}

#[test]
fn test_review_fix_patterns_trigger_continue_with_1() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    let phrases = [
        "Fix them automatically or create action items?",
        "Choose [1] to fix automatically.",
        "[1] Fix automatically",
        "What should I do with these issues?",
        "What should I do with these findings?",
        // Entries from REVIEW_FIX_PATTERNS that present options 2 and 3 must
        // still auto-select option 1 (fix automatically) — the bot always fixes.
        "Choose [2] create action items",
        "Choose [3] show me details",
    ];
    for phrase in phrases {
        let action = analyzer.analyze(phrase, &slot, "7-10");
        assert_eq!(
            action,
            ResponseAction::Continue {
                reply: "1".to_string()
            },
            "Review fix not detected for: {phrase}"
        );
    }
}

#[test]
fn test_review_complete_priority_over_fix() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    // A full review summary contains "Issues Fixed:" which appears in both
    // REVIEW_COMPLETE_PATTERNS (priority 1.5) and indirectly in fix patterns.
    // Review complete should win.
    let summary = "✅ Review complete\n\nIssues Fixed: 3\nAction Items Created: 0\n\
                   Sprint status synced.";
    let action = analyzer.analyze(summary, &slot, "7-10");
    assert_eq!(action, ResponseAction::Completed);
}

#[test]
fn test_review_complete_does_not_false_positive_on_normal_completion() {
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();
    // "All tasks completed" triggers Completed via COMPLETION_SIGNALS (priority 2),
    // NOT via REVIEW_COMPLETE_PATTERNS (priority 1.5).
    // Both paths return ResponseAction::Completed, so we verify the non-review path
    // still works correctly and that casual "review" mentions don't trigger the
    // review-complete path.
    let action = analyzer.analyze("All tasks completed successfully.", &slot, "7-10");
    assert_eq!(
        action,
        ResponseAction::Completed,
        "Normal completion signal must still return Completed"
    );

    // A sentence using "review" casually (not as a workflow completion marker)
    // must NOT trigger REVIEW_COMPLETE_PATTERNS.
    let non_review = "I'll review the code changes and continue working.";
    let action2 = analyzer.analyze(non_review, &slot, "7-10");
    assert_ne!(
        action2,
        ResponseAction::Completed,
        "Casual use of 'review' must not false-positive as a review-workflow completion"
    );
}

// ===========================================================================
// Task 10: Full pipeline integration tests (AC: 3, 4, 6, 7, 8)
// ===========================================================================

#[tokio::test]
async fn test_full_flow_confirmation_rule_match_no_escalation() {
    // Agent asks confirmation → rule engine matches → analyzer sees no escalation → Continue
    let slot: EscalationSlot = Arc::new(Mutex::new(None));
    let supervisor = AskSupervisor::with_answer_provider_and_slot(
        Box::new(MockAnswerProvider {
            response: String::new(),
            should_fail: false,
        }),
        slot.clone(),
    );

    let args = AskSupervisorArgs {
        question: "Should I proceed with the implementation?".to_string(),
        context: None,
    };
    let result = supervisor.call(args).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Yes, proceed.");

    // Slot should still be empty (no escalation on rule match)
    {
        let guard = slot.lock().expect("slot lock");
        assert!(guard.is_none(), "Slot should be empty after rule match");
    }

    // Analyzer sees no escalation
    let analyzer = ResponseAnalyzer::new();
    let action = analyzer.analyze("Some agent response.", &slot, "7-10");
    assert_ne!(action, ResponseAction::Escalated);

    // Decision was logged as RuleEngine
    let log = supervisor.decision_log();
    assert_eq!(log.len(), 1);
    let records = log.records();
    assert!(matches!(
        &records[0].source,
        DecisionSource::RuleEngine { .. }
    ));
}

#[tokio::test]
async fn test_full_flow_unknown_question_escalation_via_slot() {
    // Agent asks unknown question → rule engine misses → failing MockAnswerProvider →
    // supervisor writes to escalation slot → analyzer reads slot → Escalated
    let slot: EscalationSlot = Arc::new(Mutex::new(None));

    let mock = MockAnswerProvider {
        response: String::new(),
        should_fail: true,
    };

    let supervisor =
        AskSupervisor::with_answer_provider_and_slot(Box::new(mock), slot.clone());

    let args = AskSupervisorArgs {
        question: "What database schema should I use for user sessions?".to_string(),
        context: None,
    };
    let result = supervisor.call(args).await;
    assert!(
        matches!(result, Err(SupervisorError::EscalationRequired { .. })),
        "Expected EscalationRequired, got: {:?}",
        result
    );

    // Verify the slot was written BEFORE the error was returned
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
        assert!(info.reason.contains("Architect session failed"));
    }

    // ResponseAnalyzer reads the same slot — detects escalation at priority 1
    let analyzer = ResponseAnalyzer::new();
    let action = analyzer.analyze(
        "Here is some irrelevant response text",
        &slot,
        "7-10-test-story",
    );
    assert_eq!(action, ResponseAction::Escalated);

    // Verify decision was logged as Escalation
    let log = supervisor.decision_log();
    assert_eq!(log.len(), 1);
    let records = log.records();
    assert!(
        matches!(&records[0].source, DecisionSource::Escalation),
        "Expected Escalation source, got: {:?}",
        records[0].source
    );
}

#[tokio::test]
async fn test_full_flow_completion_no_rule_engine() {
    // Agent signals completion → analyzer returns Completed (rule engine not involved)
    let analyzer = ResponseAnalyzer::new();
    let slot = empty_slot();

    let action = analyzer.analyze(
        "All tasks completed. Story implementation complete. Ready for review.",
        &slot,
        "7-10",
    );
    assert_eq!(action, ResponseAction::Completed);

    // The ResponseAnalyzer and RuleEngine are SEPARATE layers with different scopes:
    // - ResponseAnalyzer: monitors agent responses in the chat loop (chat-level layer)
    // - RuleEngine: processes questions routed through the AskSupervisor *tool* (tool-call layer)
    //
    // They can both recognise similar surface patterns but serve different purposes.
    // Completion detection is ALWAYS handled by the analyzer — the rule engine is never
    // consulted for agent responses in the chat loop, so even if a phrase also happens
    // to fire a rule-engine pattern (e.g. progress_confirmation), it does NOT interfere.
    let engine = RuleEngine::new();
    // "implementation complete" inside a completion report also matches the
    // progress_confirmation rule — this is intentional: the rule engine will reply
    // "Acknowledged. Continue to the next task." if the same text were sent as a
    // tool-invoked question. That is the correct tool-layer behaviour.
    let result = engine.evaluate("All tasks completed. Story implementation complete.");
    assert!(
        matches!(result, RuleResult::Matched { rule_name, .. } if rule_name == "progress_confirmation"),
        "progress_confirmation rule should match a completion report sent as a tool question"
    );
}
