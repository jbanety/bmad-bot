//! Escalation types for supervisor-to-session communication.
//!
//! When the supervisor cannot answer a question (neither rule engine nor Architect
//! session can provide a confident answer), it signals escalation. These types
//! carry the escalation context from the supervisor tool through to the daemon.
//!
//! - [`EscalationInfo`] — lightweight data carrier stored in the shared escalation
//!   slot (`Arc<Mutex<Option<EscalationInfo>>>`) between the supervisor tool and
//!   the session chat loop.
//! - [`EscalationReport`] — full report returned to the daemon for logging,
//!   notification (Epic 6), and PR creation (Epic 5).

use std::fmt;

use serde::{Deserialize, Serialize};

/// Carries escalation context from the supervisor tool to the session chat loop.
///
/// This struct is stored in the shared escalation slot
/// (`Arc<Mutex<Option<EscalationInfo>>>`) by [`AskSupervisor::call()`] when it
/// returns [`SupervisorError::EscalationRequired`]. The session chat loop checks
/// the slot after each `agent.chat()` turn and extracts the info to build a
/// [`SessionError::SupervisorEscalation`].
///
/// Deliberately lightweight — only the question and reason are needed for the
/// session to construct the full [`EscalationReport`] with additional context
/// (story key, branch name, partial work summary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EscalationInfo {
    /// The original question that could not be answered by the supervisor.
    pub question: String,
    /// Why the question requires human intervention (e.g., "Architect session failed: ...").
    pub reason: String,
}

/// Full escalation report returned to the daemon for logging and notification.
///
/// Built by the session module after escalation cleanup (partial work preservation,
/// sprint-status update). Contains all information needed for:
/// - Structured logging via `tracing`
/// - Telegram notification to the human (Epic 6, Story 6.1)
/// - PR description for partial work (Epic 5)
///
/// Implements [`Serialize`] and [`Deserialize`] for persistence and transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EscalationReport {
    /// The story key that was being worked on (e.g., "3-3-human-escalation").
    pub story_key: String,
    /// The question the supervisor could not answer.
    pub question: String,
    /// Why escalation was necessary.
    pub reason: String,
    /// Git branch name where partial work is preserved.
    pub branch_name: String,
    /// Summary of partial work preserved on the branch.
    pub partial_work_summary: String,
    /// ISO 8601 timestamp of when the escalation occurred.
    pub escalated_at: String,
}

impl EscalationReport {
    /// Create a new escalation report with the current UTC timestamp.
    ///
    /// The `escalated_at` field is set to `chrono::Utc::now().to_rfc3339()`.
    ///
    /// # Arguments
    /// - `story_key` — the story being developed when escalation occurred
    /// - `question` — the unanswered question from the supervisor
    /// - `reason` — why the supervisor could not answer
    /// - `branch_name` — git branch with partial work
    /// - `partial_work_summary` — description of preserved partial work
    pub fn new(
        story_key: String,
        question: String,
        reason: String,
        branch_name: String,
        partial_work_summary: String,
    ) -> Self {
        Self {
            story_key,
            question,
            reason,
            branch_name,
            partial_work_summary,
            escalated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl fmt::Display for EscalationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "⚠️ Escalation Report — Story: {}\n\
             Question: {}\n\
             Reason: {}\n\
             Branch: {}\n\
             Partial Work: {}\n\
             Escalated At: {}",
            self.story_key,
            self.question,
            self.reason,
            self.branch_name,
            self.partial_work_summary,
            self.escalated_at,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // EscalationInfo tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_escalation_info_construction_and_fields() {
        let info = EscalationInfo {
            question: "What DB schema?".to_string(),
            reason: "Architect failed".to_string(),
        };
        assert_eq!(info.question, "What DB schema?");
        assert_eq!(info.reason, "Architect failed");
    }

    #[test]
    fn test_escalation_info_clone() {
        let info = EscalationInfo {
            question: "q".to_string(),
            reason: "r".to_string(),
        };
        let cloned = info.clone();
        assert_eq!(info, cloned);
    }

    #[test]
    fn test_escalation_info_debug() {
        let info = EscalationInfo {
            question: "q".to_string(),
            reason: "r".to_string(),
        };
        let debug = format!("{info:?}");
        assert!(debug.contains("EscalationInfo"));
        assert!(debug.contains("q"));
        assert!(debug.contains("r"));
    }

    #[test]
    fn test_escalation_info_equality() {
        let a = EscalationInfo {
            question: "q".to_string(),
            reason: "r".to_string(),
        };
        let b = EscalationInfo {
            question: "q".to_string(),
            reason: "r".to_string(),
        };
        let c = EscalationInfo {
            question: "different".to_string(),
            reason: "r".to_string(),
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // -----------------------------------------------------------------------
    // EscalationReport tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_escalation_report_new_sets_all_fields() {
        let report = EscalationReport::new(
            "3-3-human-escalation".to_string(),
            "What DB schema?".to_string(),
            "Architect session failed".to_string(),
            "story/3-3-human-escalation".to_string(),
            "Branch: story/3-3, WIP commit: yes".to_string(),
        );
        assert_eq!(report.story_key, "3-3-human-escalation");
        assert_eq!(report.question, "What DB schema?");
        assert_eq!(report.reason, "Architect session failed");
        assert_eq!(report.branch_name, "story/3-3-human-escalation");
        assert_eq!(
            report.partial_work_summary,
            "Branch: story/3-3, WIP commit: yes"
        );
        // escalated_at should be a valid RFC 3339 timestamp
        assert!(!report.escalated_at.is_empty());
    }

    #[test]
    fn test_escalation_report_escalated_at_is_valid_iso8601() {
        let report = EscalationReport::new(
            "key".to_string(),
            "q".to_string(),
            "r".to_string(),
            "b".to_string(),
            "s".to_string(),
        );
        // chrono::DateTime::parse_from_rfc3339 validates ISO 8601 / RFC 3339
        let parsed = chrono::DateTime::parse_from_rfc3339(&report.escalated_at);
        assert!(
            parsed.is_ok(),
            "escalated_at should be valid RFC 3339: {}",
            report.escalated_at
        );
    }

    #[test]
    fn test_escalation_report_display() {
        let report = EscalationReport::new(
            "1-1-scaffolding".to_string(),
            "Which ORM?".to_string(),
            "No rule match".to_string(),
            "story/1-1-scaffolding".to_string(),
            "2 commits, 3 files".to_string(),
        );
        let display = format!("{report}");
        assert!(display.contains("Escalation Report"));
        assert!(display.contains("1-1-scaffolding"));
        assert!(display.contains("Which ORM?"));
        assert!(display.contains("No rule match"));
        assert!(display.contains("story/1-1-scaffolding"));
        assert!(display.contains("2 commits, 3 files"));
        assert!(display.contains("Escalated At:"));
    }

    #[test]
    fn test_escalation_report_serialization_roundtrip() {
        let report = EscalationReport::new(
            "3-3-human-escalation".to_string(),
            "What DB schema?".to_string(),
            "Architect failed".to_string(),
            "story/3-3".to_string(),
            "summary".to_string(),
        );
        let json = serde_json::to_string(&report).expect("serialize");
        let deserialized: EscalationReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(report, deserialized);
    }

    #[test]
    fn test_escalation_report_clone() {
        let report = EscalationReport::new(
            "key".to_string(),
            "q".to_string(),
            "r".to_string(),
            "b".to_string(),
            "s".to_string(),
        );
        let cloned = report.clone();
        assert_eq!(report, cloned);
    }

    #[test]
    fn test_escalation_report_debug() {
        let report = EscalationReport::new(
            "key".to_string(),
            "q".to_string(),
            "r".to_string(),
            "b".to_string(),
            "s".to_string(),
        );
        let debug = format!("{report:?}");
        assert!(debug.contains("EscalationReport"));
        assert!(debug.contains("key"));
    }

    #[test]
    fn test_escalation_report_implements_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<EscalationReport>();
    }

    #[test]
    fn test_escalation_info_implements_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<EscalationInfo>();
    }
}
