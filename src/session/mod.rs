//! Session module — manages rig agent setup, chat loops, and story execution sessions.
//!
//! This module orchestrates development sessions: setting up the rig agent with
//! tools (git, filesystem, terminal, ask_supervisor), running the chat loop,
//! and handling session outcomes (completion, escalation, failure).
//!
//! Key types:
//! - [`SessionError`] — typed errors for session-level failures
//! - [`SessionOutcome`] — the three possible results of a session run
//! - [`escalation::EscalationInfo`] — lightweight escalation data carrier
//! - [`escalation::EscalationReport`] — full report for daemon/notification

/// Session cleanup: partial work preservation and sprint-status updates.
pub mod cleanup;
/// Escalation types for supervisor-to-session communication.
pub mod escalation;
mod state;

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
    },
    /// Session escalated to human — needs clarification.
    Escalated(EscalationReport),
    /// Session failed with an unrecoverable error.
    Failed {
        /// The story key that failed.
        story_key: String,
        /// Description of the failure.
        error: String,
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
        };
        match outcome {
            SessionOutcome::Completed {
                story_key, branch, ..
            } => {
                assert_eq!(story_key, "1-1-scaffolding");
                assert_eq!(branch, "story/1-1-scaffolding");
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
        let outcome = SessionOutcome::Escalated(report);
        match outcome {
            SessionOutcome::Escalated(r) => {
                assert_eq!(r.story_key, "3-3-human-escalation");
                assert_eq!(r.question, "What DB?");
                assert_eq!(r.reason, "No answer");
                assert_eq!(r.branch_name, "story/3-3");
                assert_eq!(r.partial_work_summary, "summary");
                assert!(!r.escalated_at.is_empty());
            }
            _ => panic!("Expected Escalated variant"),
        }
    }

    #[test]
    fn test_session_outcome_failed() {
        let outcome = SessionOutcome::Failed {
            story_key: "2-1-polling".to_string(),
            error: "timeout".to_string(),
        };
        match outcome {
            SessionOutcome::Failed { story_key, error } => {
                assert_eq!(story_key, "2-1-polling");
                assert_eq!(error, "timeout");
            }
            _ => panic!("Expected Failed variant"),
        }
    }

    #[test]
    fn test_session_outcome_debug() {
        let outcome = SessionOutcome::Completed {
            story_key: "key".to_string(),
            branch: "b".to_string(),
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
