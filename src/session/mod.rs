//! Session module — manages rig agent setup, chat loops, branch management, and story execution sessions.
//!
//! This module orchestrates development sessions: resolving the correct base branch,
//! creating/reusing story branches, setting up the rig agent with tools
//! (git, filesystem, terminal, ask_supervisor), running the chat loop,
//! and handling session outcomes (completion, escalation, failure).
//!
//! Key types:
//! - [`SessionError`] — typed errors for session-level failures
//! - [`SessionOutcome`] — the three possible results of a session run
//! - [`SessionRunner`] — the main session lifecycle manager
//! - [`ResponseAnalyzer`] — pattern-matching engine for agent responses
//! - [`SessionState`] — WAL state for crash recovery
//! - [`BranchAction`] — result of branch setup (created or reused)
//! - [`BranchError`] — typed errors for branch operations
//! - [`escalation::EscalationInfo`] — lightweight escalation data carrier
//! - [`escalation::EscalationReport`] — full report returned to the daemon for logging,
//!   notification (Epic 6), and PR creation (Epic 5).

/// Shared agent activation — preamble, tool construction, activation, and streaming chat.
pub mod agent;
/// Response analyzer — pattern matching for workflow interactions.
pub mod analyzer;
/// Daemon-orchestrated consultation mechanism (Architecture Decision 10).
pub mod consultation;
/// Branch management — dependency-aware branch chaining for story sessions.
pub mod branch;
/// Session cleanup: partial work preservation and sprint-status updates.
pub mod cleanup;
/// Escalation types for supervisor-to-session communication.
pub mod escalation;
/// LLM provider factory — multi-provider support.
pub mod provider;
/// Session runner — build agent, chat loop, lifecycle management.
pub mod runner;
/// WAL state file — session persistence for crash recovery.
pub(crate) mod state;

use crate::supervisor::decisions::DecisionRecord;
use escalation::EscalationReport;

/// Errors originating from the session module.
///
/// These are typed errors — no `anyhow::Result` in session or supervisor modules.
/// Each variant carries structured context for logging and error handling.
///
/// Implements `std::error::Error + Send + Sync` via `thiserror`.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The supervisor escalated a question to the human — the session must stop.
    ///
    /// This is NOT a failure — it is a deliberate, correct decision by the
    /// supervisor that human input is required. The session handles this by
    /// preserving partial work and updating sprint-status.
    #[error("Supervisor escalation required for story {story_key}: {question}")]
    SupervisorEscalation {
        /// The story being developed when escalation occurred.
        story_key: String,
        /// The question the supervisor could not answer.
        question: String,
        /// Why escalation was necessary.
        reason: String,
    },

    /// The chat loop encountered an unrecoverable error.
    #[error("Chat loop failed: {reason}")]
    ChatFailed {
        /// Description of the chat failure.
        reason: String,
    },

    /// A rig tool returned an error that the session cannot recover from.
    #[error("Tool error: {reason}")]
    ToolError {
        /// Description of the tool failure.
        reason: String,
    },

    /// Failed to read or write a session state file (WAL, sprint-status, etc.).
    #[error("State file operation failed: {reason}")]
    StateFileFailed {
        /// Description of the state file failure.
        reason: String,
    },

    /// A git operation failed during session execution.
    #[error("Git error: {reason}")]
    GitError {
        /// Description of the git failure.
        reason: String,
    },
}

/// Result of a development session run.
///
/// Returned to the daemon main loop after a session completes. The daemon
/// handles each variant differently:
/// - [`Completed`](SessionOutcome::Completed) → proceed to code review / PR creation (Epic 5)
/// - [`Escalated`](SessionOutcome::Escalated) → store report for notification (Epic 6), next poll cycle
/// - [`Failed`](SessionOutcome::Failed) → create PR with partial work and failure description (FR23)
#[derive(Debug)]
pub enum SessionOutcome {
    /// Session completed successfully — story is done, PR ready.
    Completed {
        /// The story key that was completed.
        story_key: String,
        /// Git branch with the completed work.
        branch: String,
        /// All supervisor decisions made during the session.
        decisions: Vec<DecisionRecord>,
        /// Agent-generated context paragraph for the PR description (raw text).
        /// `None` if the agent didn't produce a summary (crash, timeout, parse failure).
        pr_context: Option<String>,
        /// Agent-generated testing instructions for the PR description (raw text).
        pr_how_to_test: Option<String>,
        /// Agent-generated additional info for the PR description (raw text).
        pr_additional_info: Option<String>,
    },
    /// Session escalated to human — needs clarification.
    Escalated {
        /// Full escalation report for logging and notification.
        report: EscalationReport,
        /// All supervisor decisions made during the session (includes the escalation record).
        decisions: Vec<DecisionRecord>,
    },
    /// Session failed with an unrecoverable error.
    Failed {
        /// The story key that failed.
        story_key: String,
        /// Description of the failure.
        error: String,
        /// All supervisor decisions made during the session.
        decisions: Vec<DecisionRecord>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // SessionError tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_session_error_supervisor_escalation_display() {
        let err = SessionError::SupervisorEscalation {
            story_key: "3-3-human-escalation".to_string(),
            question: "What DB schema?".to_string(),
            reason: "Architect failed".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("3-3-human-escalation"));
        assert!(display.contains("What DB schema?"));
    }

    #[test]
    fn test_session_error_chat_failed_display() {
        let err = SessionError::ChatFailed {
            reason: "connection lost".to_string(),
        };
        assert!(format!("{err}").contains("connection lost"));
    }

    #[test]
    fn test_session_error_tool_error_display() {
        let err = SessionError::ToolError {
            reason: "git push rejected".to_string(),
        };
        assert!(format!("{err}").contains("git push rejected"));
    }

    #[test]
    fn test_session_error_state_file_failed_display() {
        let err = SessionError::StateFileFailed {
            reason: "permission denied".to_string(),
        };
        assert!(format!("{err}").contains("permission denied"));
    }

    #[test]
    fn test_session_error_git_error_display() {
        let err = SessionError::GitError {
            reason: "merge conflict".to_string(),
        };
        assert!(format!("{err}").contains("merge conflict"));
    }

    #[test]
    fn test_session_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SessionError>();
    }

    #[test]
    fn test_session_error_is_std_error() {
        fn assert_error<T: std::error::Error>() {}
        assert_error::<SessionError>();
    }

    // -----------------------------------------------------------------------
    // SessionOutcome tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_session_outcome_completed() {
        let outcome = SessionOutcome::Completed {
            story_key: "1-1-scaffolding".to_string(),
            branch: "story/1-1-scaffolding".to_string(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        };
        match outcome {
            SessionOutcome::Completed {
                story_key,
                branch,
                decisions,
                pr_context,
                pr_how_to_test,
                pr_additional_info,
                ..
            } => {
                assert_eq!(story_key, "1-1-scaffolding");
                assert_eq!(branch, "story/1-1-scaffolding");
                assert!(decisions.is_empty());
                assert!(pr_context.is_none());
                assert!(pr_how_to_test.is_none());
                assert!(pr_additional_info.is_none());
            }
            _ => panic!("Expected Completed variant"),
        }
    }

    #[test]
    fn test_session_outcome_completed_with_decisions() {
        use crate::supervisor::decisions::{DecisionRecord, DecisionSource};
        let decisions = vec![DecisionRecord::new(
            "Proceed?".to_string(),
            None,
            "Yes.".to_string(),
            DecisionSource::RuleEngine {
                rule_name: "confirm".to_string(),
            },
            "Matched".to_string(),
            vec![],
        )];
        let outcome = SessionOutcome::Completed {
            story_key: "1-1-scaffolding".to_string(),
            branch: "story/1-1".to_string(),
            decisions,
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        };
        match outcome {
            SessionOutcome::Completed { decisions, .. } => {
                assert_eq!(decisions.len(), 1);
                assert_eq!(decisions[0].question, "Proceed?");
            }
            _ => panic!("Expected Completed variant"),
        }
    }

    #[test]
    fn test_session_outcome_escalated_carries_report() {
        let report = EscalationReport::new(
            "3-3-human-escalation".to_string(),
            "What DB?".to_string(),
            "No answer".to_string(),
            "story/3-3".to_string(),
            "summary".to_string(),
        );
        let outcome = SessionOutcome::Escalated {
            report,
            decisions: vec![],
        };
        match outcome {
            SessionOutcome::Escalated {
                report: r,
                decisions,
            } => {
                assert_eq!(r.story_key, "3-3-human-escalation");
                assert_eq!(r.question, "What DB?");
                assert_eq!(r.reason, "No answer");
                assert_eq!(r.branch_name, "story/3-3");
                assert_eq!(r.partial_work_summary, "summary");
                assert!(!r.escalated_at.is_empty());
                assert!(decisions.is_empty());
            }
            _ => panic!("Expected Escalated variant"),
        }
    }

    #[test]
    fn test_session_outcome_escalated_with_decisions() {
        use crate::supervisor::decisions::{DecisionRecord, DecisionSource};
        let report = EscalationReport::new(
            "3-3-test".to_string(),
            "q".to_string(),
            "r".to_string(),
            "b".to_string(),
            "s".to_string(),
        );
        let decisions = vec![
            DecisionRecord::new(
                "q1".to_string(),
                None,
                "a1".to_string(),
                DecisionSource::RuleEngine {
                    rule_name: "rule1".to_string(),
                },
                "r1".to_string(),
                vec![],
            ),
            DecisionRecord::new(
                "q2".to_string(),
                None,
                String::new(),
                DecisionSource::Escalation,
                "escalated".to_string(),
                vec!["no match".to_string()],
            ),
        ];
        let outcome = SessionOutcome::Escalated { report, decisions };
        match outcome {
            SessionOutcome::Escalated { decisions, .. } => {
                assert_eq!(decisions.len(), 2);
                assert_eq!(decisions[1].source, DecisionSource::Escalation);
            }
            _ => panic!("Expected Escalated variant"),
        }
    }

    #[test]
    fn test_session_outcome_failed() {
        let outcome = SessionOutcome::Failed {
            story_key: "2-1-polling".to_string(),
            error: "timeout".to_string(),
            decisions: vec![],
        };
        match outcome {
            SessionOutcome::Failed {
                story_key,
                error,
                decisions,
            } => {
                assert_eq!(story_key, "2-1-polling");
                assert_eq!(error, "timeout");
                assert!(decisions.is_empty());
            }
            _ => panic!("Expected Failed variant"),
        }
    }

    #[test]
    fn test_session_outcome_completed_with_pr_summary_fields() {
        let outcome = SessionOutcome::Completed {
            story_key: "5-4-enriched-pr".to_string(),
            branch: "story/5-4-enriched-pr".to_string(),
            decisions: vec![],
            pr_context: Some("Implemented enriched PR descriptions.".to_string()),
            pr_how_to_test: Some("Run cargo test.".to_string()),
            pr_additional_info: Some("Added regex dep.".to_string()),
        };
        match outcome {
            SessionOutcome::Completed {
                pr_context,
                pr_how_to_test,
                pr_additional_info,
                ..
            } => {
                assert_eq!(
                    pr_context.as_deref(),
                    Some("Implemented enriched PR descriptions.")
                );
                assert_eq!(pr_how_to_test.as_deref(), Some("Run cargo test."));
                assert_eq!(pr_additional_info.as_deref(), Some("Added regex dep."));
            }
            _ => panic!("Expected Completed variant"),
        }
    }

    #[test]
    fn test_session_outcome_debug() {
        let outcome = SessionOutcome::Completed {
            story_key: "key".to_string(),
            branch: "b".to_string(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        };
        let debug = format!("{outcome:?}");
        assert!(debug.contains("Completed"));
        assert!(debug.contains("key"));
    }

    // -----------------------------------------------------------------------
    // Shared escalation slot tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_escalation_slot_write_read_across_threads() {
        use crate::session::escalation::EscalationInfo;
        use std::sync::{Arc, Mutex};

        let slot: Arc<Mutex<Option<EscalationInfo>>> = Arc::new(Mutex::new(None));
        let writer_slot = Arc::clone(&slot);

        // Simulate supervisor writing to the slot from another thread
        let handle = std::thread::spawn(move || {
            let mut guard = writer_slot.lock().expect("lock");
            *guard = Some(EscalationInfo {
                question: "Which ORM?".to_string(),
                reason: "Architect session timed out".to_string(),
            });
        });

        handle.join().expect("writer thread");

        // Session reads the slot
        let guard = slot.lock().expect("lock");
        let info = guard.as_ref().expect("slot should contain EscalationInfo");
        assert_eq!(info.question, "Which ORM?");
        assert_eq!(info.reason, "Architect session timed out");
    }

    #[test]
    fn test_escalation_slot_initially_none() {
        use crate::session::escalation::EscalationInfo;
        use std::sync::{Arc, Mutex};

        let slot: Arc<Mutex<Option<EscalationInfo>>> = Arc::new(Mutex::new(None));
        let guard = slot.lock().expect("lock");
        assert!(guard.is_none());
    }
}
