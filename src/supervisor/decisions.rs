//! Decision logging and traceability for the supervisor.
//!
//! Every supervisor decision (rule engine match, LLM fallback answer,
//! or human escalation) is recorded as a [`DecisionRecord`]. Records are
//! accumulated in a thread-safe [`DecisionLog`] during a session, then
//! written to a markdown file and optionally included in PR descriptions.
//!
//! Key types:
//! - [`DecisionSource`] — how the answer was determined (rule engine, LLM, escalation)
//! - [`DecisionRecord`] — a single decision with full audit trail
//! - [`DecisionLog`] — thread-safe in-memory session log (`Arc<Mutex<Vec<DecisionRecord>>>`)
//! - [`DecisionError`] — errors from file writing operations
//!
//! Key functions:
//! - [`write_decisions_file`] — writes all decisions to a markdown file
//! - [`format_pr_decisions_section`] — generates a compact table for PR descriptions

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Source that provided the answer for a supervisor decision.
///
/// Each variant represents one tier of the supervisor's three-tier pipeline:
/// 1. [`RuleEngine`](DecisionSource::RuleEngine) — deterministic, free, fast
/// 2. [`LlmFallback`](DecisionSource::LlmFallback) — context-aware, costs tokens
/// 3. [`Escalation`](DecisionSource::Escalation) — human input required
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DecisionSource {
    /// Answer came from the deterministic rule engine.
    RuleEngine {
        /// Name of the matched rule (e.g., `"confirmation_proceed"`).
        rule_name: String,
    },
    /// Answer came from the BMAD Architect LLM session with project context.
    LlmFallback,
    /// Neither rule engine nor LLM could answer — escalated to human.
    Escalation,
}

impl fmt::Display for DecisionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecisionSource::RuleEngine { rule_name } => {
                write!(f, "Rule Engine ({rule_name})")
            }
            DecisionSource::LlmFallback => write!(f, "LLM Fallback (Architect)"),
            DecisionSource::Escalation => write!(f, "Escalation"),
        }
    }
}

/// A single supervisor decision record with full audit trail.
///
/// Created every time the supervisor answers a question (or escalates).
/// Accumulated during a session via [`DecisionLog`] and written to a
/// decisions file at `_bmad-output/implementation-artifacts/{story_key}-DECISIONS.md`.
///
/// **Used by:**
/// - Story 3.4: Decision file writing and session accumulation
/// - Epic 5 Story 5.1: PR description "Supervisor Decisions" section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    /// The agent's original question.
    pub question: String,
    /// Optional additional context provided with the question.
    pub context: Option<String>,
    /// The answer provided (empty string for escalation).
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

impl DecisionRecord {
    /// Create a new decision record with the current UTC timestamp.
    ///
    /// The `timestamp` field is set to `chrono::Utc::now().to_rfc3339()`.
    ///
    /// # Arguments
    /// - `question` — the agent's original question
    /// - `context` — optional additional context
    /// - `answer` — the answer provided (empty for escalation)
    /// - `source` — how the answer was determined
    /// - `reasoning` — why this answer was given
    /// - `alternatives` — alternative answers that were considered
    pub fn new(
        question: String,
        context: Option<String>,
        answer: String,
        source: DecisionSource,
        reasoning: String,
        alternatives: Vec<String>,
    ) -> Self {
        Self {
            question,
            context,
            answer,
            source,
            reasoning,
            alternatives,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl fmt::Display for DecisionRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let truncated_question = truncate_str(&self.question, 60);
        let alternatives = if self.alternatives.is_empty() {
            "None".to_string()
        } else {
            self.alternatives.join("; ")
        };
        write!(
            f,
            "### Decision: {truncated_question}\n\
             - **Source:** {source}\n\
             - **Question:** {question}\n\
             - **Answer:** {answer}\n\
             - **Reasoning:** {reasoning}\n\
             - **Alternatives:** {alternatives}\n\
             - **Time:** {timestamp}",
            truncated_question = truncated_question,
            source = self.source,
            question = self.question,
            answer = if self.answer.is_empty() {
                "⚠️ Escalated"
            } else {
                &self.answer
            },
            reasoning = self.reasoning,
            alternatives = alternatives,
            timestamp = self.timestamp,
        )
    }
}

/// Thread-safe in-memory log of all supervisor decisions for the current session.
///
/// Uses `Arc<Mutex<Vec<DecisionRecord>>>` for thread-safe access. The mutex is
/// held only for the brief duration of `Vec::push()` — contention is negligible
/// since supervisor calls are infrequent (at most a few dozen per session).
///
/// The `DecisionLog` is `Clone` — cloning produces a handle to the same inner
/// `Arc`, so the session module and supervisor tool share the same log.
#[derive(Debug, Clone)]
pub struct DecisionLog {
    /// The inner thread-safe records list.
    records: Arc<Mutex<Vec<DecisionRecord>>>,
}

impl DecisionLog {
    /// Create a new empty decision log.
    pub fn new() -> Self {
        Self {
            records: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Append a decision record to the log.
    ///
    /// Locks the mutex briefly to push the record. If the mutex is poisoned
    /// (extremely unlikely — only possible if a thread panicked while holding
    /// the lock), the record is silently dropped and logged via `tracing::error!()`.
    /// Decision recording must **never** cause `call()` to fail.
    pub fn record(&self, record: DecisionRecord) {
        match self.records.lock() {
            Ok(mut records) => {
                tracing::debug!(
                    action = "decision_recorded",
                    source = %record.source,
                    question_len = record.question.len(),
                    "Supervisor decision recorded"
                );
                records.push(record);
            }
            Err(e) => {
                tracing::error!(
                    action = "decision_record_failed",
                    error = %e,
                    "Failed to record decision — mutex poisoned (decision dropped)"
                );
            }
        }
    }

    /// Returns a clone of all recorded decisions.
    ///
    /// Used at session end to write the decisions file and generate the PR section.
    /// Returns an empty vec if the mutex is poisoned.
    pub fn records(&self) -> Vec<DecisionRecord> {
        match self.records.lock() {
            Ok(records) => records.clone(),
            Err(e) => {
                tracing::error!(
                    action = "decision_records_read_failed",
                    error = %e,
                    "Failed to read decisions — mutex poisoned (returning empty)"
                );
                Vec::new()
            }
        }
    }

    /// Returns the number of recorded decisions.
    pub fn len(&self) -> usize {
        self.records.lock().map(|r| r.len()).unwrap_or(0)
    }

    /// Returns `true` if no decisions have been recorded.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for DecisionLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors from decision file writing operations.
///
/// These are non-fatal — decision file writing is best-effort. A failure
/// here should be logged but must never block session completion.
#[derive(Debug, thiserror::Error)]
pub enum DecisionError {
    /// Failed to write the decisions markdown file.
    #[error("Failed to write decisions file at '{path}': {reason}")]
    WriteFailed {
        /// The file path that was targeted.
        path: String,
        /// Description of the write failure.
        reason: String,
    },
    /// Failed to create the parent directory for the decisions file.
    #[error("Failed to create decisions directory: {reason}")]
    DirectoryCreation {
        /// Description of the directory creation failure.
        reason: String,
    },
}

/// Writes all decisions to a markdown file at the given path.
///
/// The file follows the convention:
/// `_bmad-output/implementation-artifacts/{story_key}-DECISIONS.md`
///
/// If `decisions` is empty, the file is NOT created and a debug log is emitted.
/// Parent directories are created automatically if they don't exist.
///
/// # Errors
/// Returns [`DecisionError`] if directory creation or file writing fails.
pub async fn write_decisions_file(
    decisions: &[DecisionRecord],
    output_path: &Path,
    story_key: &str,
) -> Result<(), DecisionError> {
    if decisions.is_empty() {
        tracing::debug!(
            action = "decisions_file_skipped",
            story_id = %story_key,
            "No decisions to write"
        );
        return Ok(());
    }

    // Ensure parent directory exists
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| DecisionError::DirectoryCreation {
                reason: format!("{e}"),
            })?;
    }

    // Count by source
    let mut rule_count = 0usize;
    let mut llm_count = 0usize;
    let mut escalation_count = 0usize;
    for d in decisions {
        match &d.source {
            DecisionSource::RuleEngine { .. } => rule_count += 1,
            DecisionSource::LlmFallback => llm_count += 1,
            DecisionSource::Escalation => escalation_count += 1,
        }
    }

    let date = decisions
        .first()
        .map(|d| d.timestamp.as_str())
        .unwrap_or("unknown");

    let mut content = format!(
        "# 🤖 Supervisor Decisions — {story_key}\n\n\
         **Session Date:** {date}\n\
         **Total Decisions:** {total}\n\
         **By Source:** {rule_count} rule engine, {llm_count} LLM fallback, {escalation_count} escalation\n\n\
         ---\n",
        total = decisions.len(),
    );

    for (i, decision) in decisions.iter().enumerate() {
        content.push_str(&format!("\n## Decision {}\n\n{decision}\n\n---\n", i + 1));
    }

    tokio::fs::write(output_path, &content)
        .await
        .map_err(|e| DecisionError::WriteFailed {
            path: output_path.display().to_string(),
            reason: format!("{e}"),
        })?;

    tracing::info!(
        action = "decisions_file_written",
        path = %output_path.display(),
        count = decisions.len(),
        "Decisions file written"
    );

    Ok(())
}

/// Generates a compact markdown table of decisions for PR descriptions.
///
/// This function is pure (no I/O, not async) — it formats data that Epic 5
/// will include in the PR body's "🤖 Supervisor Decisions" section.
///
/// Long questions and answers are truncated to 80 characters for readability.
/// If `decisions` is empty, returns a "no decisions" message.
pub fn format_pr_decisions_section(decisions: &[DecisionRecord]) -> String {
    if decisions.is_empty() {
        return "## 🤖 Supervisor Decisions\n\nNo supervisor decisions were made during this session.\n".to_string();
    }

    let mut output = String::from(
        "## 🤖 Supervisor Decisions\n\n\
         | # | Source | Question | Decision | Reasoning |\n\
         |---|--------|----------|----------|-----------|\n",
    );

    for (i, d) in decisions.iter().enumerate() {
        let source = match &d.source {
            DecisionSource::RuleEngine { rule_name } => format!("Rule Engine ({rule_name})"),
            DecisionSource::LlmFallback => "LLM Fallback".to_string(),
            DecisionSource::Escalation => "Escalation".to_string(),
        };

        let question = escape_pipe(&truncate_str(&d.question, 80));
        let answer = if d.answer.is_empty() {
            "⚠️ Escalated".to_string()
        } else {
            escape_pipe(&truncate_str(&d.answer, 80))
        };
        let reasoning = escape_pipe(&truncate_str(&d.reasoning, 80));

        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            i + 1,
            source,
            question,
            answer,
            reasoning,
        ));
    }

    output.push_str(&format!(
        "\n*{} decisions made during this session.*\n",
        decisions.len()
    ));

    output
}

/// Truncate a string to the given max length, appending `...` if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{truncated}...")
    }
}

/// Escape pipe characters in a string for markdown table cells.
fn escape_pipe(s: &str) -> String {
    s.replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // DecisionSource tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_decision_source_display_rule_engine() {
        let source = DecisionSource::RuleEngine {
            rule_name: "confirmation_proceed".to_string(),
        };
        assert_eq!(format!("{source}"), "Rule Engine (confirmation_proceed)");
    }

    #[test]
    fn test_decision_source_display_llm_fallback() {
        let source = DecisionSource::LlmFallback;
        assert_eq!(format!("{source}"), "LLM Fallback (Architect)");
    }

    #[test]
    fn test_decision_source_display_escalation() {
        let source = DecisionSource::Escalation;
        assert_eq!(format!("{source}"), "Escalation");
    }

    #[test]
    fn test_decision_source_equality() {
        assert_eq!(DecisionSource::LlmFallback, DecisionSource::LlmFallback);
        assert_eq!(DecisionSource::Escalation, DecisionSource::Escalation);
        assert_ne!(DecisionSource::LlmFallback, DecisionSource::Escalation);
        assert_eq!(
            DecisionSource::RuleEngine {
                rule_name: "a".to_string()
            },
            DecisionSource::RuleEngine {
                rule_name: "a".to_string()
            }
        );
        assert_ne!(
            DecisionSource::RuleEngine {
                rule_name: "a".to_string()
            },
            DecisionSource::RuleEngine {
                rule_name: "b".to_string()
            }
        );
    }

    #[test]
    fn test_decision_source_serialization_roundtrip() {
        let sources = vec![
            DecisionSource::RuleEngine {
                rule_name: "test_rule".to_string(),
            },
            DecisionSource::LlmFallback,
            DecisionSource::Escalation,
        ];
        for source in sources {
            let json = serde_json::to_string(&source).expect("serialize");
            let deserialized: DecisionSource = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(source, deserialized);
        }
    }

    // -----------------------------------------------------------------------
    // DecisionRecord tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_decision_record_new_sets_all_fields() {
        let record = DecisionRecord::new(
            "Should I proceed?".to_string(),
            Some("Working on task 3".to_string()),
            "Yes, proceed.".to_string(),
            DecisionSource::RuleEngine {
                rule_name: "confirmation".to_string(),
            },
            "Matched confirmation pattern".to_string(),
            vec!["Wait for approval".to_string()],
        );
        assert_eq!(record.question, "Should I proceed?");
        assert_eq!(record.context.as_deref(), Some("Working on task 3"));
        assert_eq!(record.answer, "Yes, proceed.");
        assert_eq!(
            record.source,
            DecisionSource::RuleEngine {
                rule_name: "confirmation".to_string()
            }
        );
        assert_eq!(record.reasoning, "Matched confirmation pattern");
        assert_eq!(record.alternatives, vec!["Wait for approval"]);
        assert!(!record.timestamp.is_empty());
    }

    #[test]
    fn test_decision_record_timestamp_is_valid_iso8601() {
        let record = DecisionRecord::new(
            "q".to_string(),
            None,
            "a".to_string(),
            DecisionSource::LlmFallback,
            "r".to_string(),
            vec![],
        );
        let parsed = chrono::DateTime::parse_from_rfc3339(&record.timestamp);
        assert!(
            parsed.is_ok(),
            "timestamp should be valid RFC 3339: {}",
            record.timestamp
        );
    }

    #[test]
    fn test_decision_record_display_rule_engine() {
        let record = DecisionRecord::new(
            "Should I proceed?".to_string(),
            None,
            "Yes, proceed.".to_string(),
            DecisionSource::RuleEngine {
                rule_name: "confirm".to_string(),
            },
            "Matched confirmation pattern".to_string(),
            vec![],
        );
        let display = format!("{record}");
        assert!(display.contains("### Decision:"));
        assert!(display.contains("Should I proceed?"));
        assert!(display.contains("Rule Engine (confirm)"));
        assert!(display.contains("Yes, proceed."));
        assert!(display.contains("Matched confirmation pattern"));
        assert!(display.contains("Alternatives:** None"));
    }

    #[test]
    fn test_decision_record_display_escalation_shows_warning() {
        let record = DecisionRecord::new(
            "What DB schema?".to_string(),
            None,
            String::new(),
            DecisionSource::Escalation,
            "Escalated to human".to_string(),
            vec!["Rule engine missed".to_string()],
        );
        let display = format!("{record}");
        assert!(display.contains("⚠️ Escalated"));
        assert!(display.contains("Escalation"));
        assert!(display.contains("Rule engine missed"));
    }

    #[test]
    fn test_decision_record_display_with_alternatives() {
        let record = DecisionRecord::new(
            "q".to_string(),
            None,
            "a".to_string(),
            DecisionSource::LlmFallback,
            "r".to_string(),
            vec!["alt1".to_string(), "alt2".to_string()],
        );
        let display = format!("{record}");
        assert!(display.contains("alt1; alt2"));
    }

    #[test]
    fn test_decision_record_serialization_roundtrip() {
        let record = DecisionRecord::new(
            "question".to_string(),
            Some("ctx".to_string()),
            "answer".to_string(),
            DecisionSource::LlmFallback,
            "reasoning".to_string(),
            vec!["alt".to_string()],
        );
        let json = serde_json::to_string(&record).expect("serialize");
        let deserialized: DecisionRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.question, "question");
        assert_eq!(deserialized.context.as_deref(), Some("ctx"));
        assert_eq!(deserialized.answer, "answer");
        assert_eq!(deserialized.source, DecisionSource::LlmFallback);
        assert_eq!(deserialized.reasoning, "reasoning");
        assert_eq!(deserialized.alternatives, vec!["alt"]);
    }

    #[test]
    fn test_decision_record_clone() {
        let record = DecisionRecord::new(
            "q".to_string(),
            None,
            "a".to_string(),
            DecisionSource::Escalation,
            "r".to_string(),
            vec![],
        );
        let cloned = record.clone();
        assert_eq!(cloned.question, record.question);
        assert_eq!(cloned.answer, record.answer);
    }

    #[test]
    fn test_decision_record_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DecisionRecord>();
    }

    // -----------------------------------------------------------------------
    // DecisionLog tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_decision_log_new_is_empty() {
        let log = DecisionLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert!(log.records().is_empty());
    }

    #[test]
    fn test_decision_log_default_is_empty() {
        let log = DecisionLog::default();
        assert!(log.is_empty());
    }

    #[test]
    fn test_decision_log_record_appends() {
        let log = DecisionLog::new();
        let record = DecisionRecord::new(
            "q".to_string(),
            None,
            "a".to_string(),
            DecisionSource::LlmFallback,
            "r".to_string(),
            vec![],
        );
        log.record(record);
        assert_eq!(log.len(), 1);
        assert!(!log.is_empty());
        let records = log.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].question, "q");
    }

    #[test]
    fn test_decision_log_preserves_insertion_order() {
        let log = DecisionLog::new();
        for i in 0..5 {
            log.record(DecisionRecord::new(
                format!("question-{i}"),
                None,
                format!("answer-{i}"),
                DecisionSource::LlmFallback,
                "r".to_string(),
                vec![],
            ));
        }
        let records = log.records();
        assert_eq!(records.len(), 5);
        for (i, r) in records.iter().enumerate() {
            assert_eq!(r.question, format!("question-{i}"));
        }
    }

    #[test]
    fn test_decision_log_concurrent_writes() {
        let log = DecisionLog::new();
        let mut handles = vec![];

        for i in 0..10 {
            let log_clone = log.clone();
            let handle = std::thread::spawn(move || {
                log_clone.record(DecisionRecord::new(
                    format!("thread-{i}"),
                    None,
                    "a".to_string(),
                    DecisionSource::RuleEngine {
                        rule_name: format!("rule-{i}"),
                    },
                    "r".to_string(),
                    vec![],
                ));
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().expect("thread should not panic");
        }

        assert_eq!(log.len(), 10);
        let records = log.records();
        assert_eq!(records.len(), 10);
        // All 10 threads should have recorded — order may vary but all present
        let mut questions: Vec<String> = records.iter().map(|r| r.question.clone()).collect();
        questions.sort();
        for i in 0..10 {
            assert_eq!(questions[i], format!("thread-{i}"));
        }
    }

    #[test]
    fn test_decision_log_clone_shares_arc() {
        let log1 = DecisionLog::new();
        let log2 = log1.clone();

        log1.record(DecisionRecord::new(
            "from-log1".to_string(),
            None,
            "a".to_string(),
            DecisionSource::LlmFallback,
            "r".to_string(),
            vec![],
        ));

        // log2 should see the record written via log1
        assert_eq!(log2.len(), 1);
        assert_eq!(log2.records()[0].question, "from-log1");

        log2.record(DecisionRecord::new(
            "from-log2".to_string(),
            None,
            "a".to_string(),
            DecisionSource::Escalation,
            "r".to_string(),
            vec![],
        ));

        // Both see 2 records
        assert_eq!(log1.len(), 2);
        assert_eq!(log2.len(), 2);
    }

    // -----------------------------------------------------------------------
    // DecisionError tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_decision_error_write_failed_display() {
        let err = DecisionError::WriteFailed {
            path: "/tmp/decisions.md".to_string(),
            reason: "permission denied".to_string(),
        };
        let display = format!("{err}");
        assert!(display.contains("/tmp/decisions.md"));
        assert!(display.contains("permission denied"));
    }

    #[test]
    fn test_decision_error_directory_creation_display() {
        let err = DecisionError::DirectoryCreation {
            reason: "read-only filesystem".to_string(),
        };
        assert!(format!("{err}").contains("read-only filesystem"));
    }

    #[test]
    fn test_decision_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DecisionError>();
    }

    // -----------------------------------------------------------------------
    // write_decisions_file tests
    // -----------------------------------------------------------------------

    fn make_test_decisions() -> Vec<DecisionRecord> {
        vec![
            DecisionRecord::new(
                "Should I proceed?".to_string(),
                None,
                "Yes, proceed.".to_string(),
                DecisionSource::RuleEngine {
                    rule_name: "confirmation".to_string(),
                },
                "Matched confirmation pattern".to_string(),
                vec![],
            ),
            DecisionRecord::new(
                "What auth approach?".to_string(),
                Some("Using JWT".to_string()),
                "Use refresh token rotation.".to_string(),
                DecisionSource::LlmFallback,
                "Architect session analysis".to_string(),
                vec!["Rule engine had no match".to_string()],
            ),
            DecisionRecord::new(
                "What about database X?".to_string(),
                None,
                String::new(),
                DecisionSource::Escalation,
                "Rule engine + Architect failed".to_string(),
                vec![
                    "Rule engine had no match".to_string(),
                    "Architect session failed".to_string(),
                ],
            ),
        ]
    }

    #[tokio::test]
    async fn test_write_decisions_file_happy_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test-DECISIONS.md");
        let decisions = make_test_decisions();

        write_decisions_file(&decisions, &path, "3-4-test")
            .await
            .expect("should succeed");

        let content = tokio::fs::read_to_string(&path).await.expect("read");
        assert!(content.contains("# 🤖 Supervisor Decisions — 3-4-test"));
        assert!(content.contains("**Total Decisions:** 3"));
        assert!(content.contains("1 rule engine, 1 LLM fallback, 1 escalation"));
        assert!(content.contains("## Decision 1"));
        assert!(content.contains("## Decision 2"));
        assert!(content.contains("## Decision 3"));
        assert!(content.contains("Should I proceed?"));
        assert!(content.contains("What auth approach?"));
        assert!(content.contains("What about database X?"));
        assert!(content.contains("⚠️ Escalated"));
    }

    #[tokio::test]
    async fn test_write_decisions_file_empty_decisions_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty-DECISIONS.md");

        write_decisions_file(&[], &path, "test")
            .await
            .expect("should succeed");

        // File should NOT exist
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn test_write_decisions_file_creates_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("deep").join("DECISIONS.md");
        let decisions = vec![DecisionRecord::new(
            "q".to_string(),
            None,
            "a".to_string(),
            DecisionSource::LlmFallback,
            "r".to_string(),
            vec![],
        )];

        write_decisions_file(&decisions, &path, "test")
            .await
            .expect("should succeed");

        assert!(path.exists());
    }

    #[tokio::test]
    async fn test_write_decisions_file_invalid_path_returns_error() {
        let path = Path::new("/nonexistent/readonly/impossible/DECISIONS.md");
        let decisions = vec![DecisionRecord::new(
            "q".to_string(),
            None,
            "a".to_string(),
            DecisionSource::LlmFallback,
            "r".to_string(),
            vec![],
        )];

        let result = write_decisions_file(&decisions, path, "test").await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // format_pr_decisions_section tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_pr_decisions_section_happy_path() {
        let decisions = make_test_decisions();
        let section = format_pr_decisions_section(&decisions);

        assert!(section.contains("## 🤖 Supervisor Decisions"));
        assert!(section.contains("| # | Source | Question | Decision | Reasoning |"));
        assert!(section.contains("| 1 |"));
        assert!(section.contains("| 2 |"));
        assert!(section.contains("| 3 |"));
        assert!(section.contains("Rule Engine (confirmation)"));
        assert!(section.contains("LLM Fallback"));
        assert!(section.contains("Escalation"));
        assert!(section.contains("Should I proceed?"));
        assert!(section.contains("⚠️ Escalated"));
        assert!(section.contains("*3 decisions made during this session.*"));
    }

    #[test]
    fn test_format_pr_decisions_section_empty() {
        let section = format_pr_decisions_section(&[]);
        assert!(section.contains("No supervisor decisions were made during this session."));
    }

    #[test]
    fn test_format_pr_decisions_section_truncates_long_text() {
        let long_question = "a".repeat(200);
        let decisions = vec![DecisionRecord::new(
            long_question.clone(),
            None,
            "a".repeat(200),
            DecisionSource::LlmFallback,
            "a".repeat(200),
            vec![],
        )];

        let section = format_pr_decisions_section(&decisions);
        // The original 200-char question should NOT appear verbatim
        assert!(!section.contains(&long_question));
        // But a truncated version should
        assert!(section.contains("..."));
    }

    #[test]
    fn test_format_pr_decisions_section_single_decision() {
        let decisions = vec![DecisionRecord::new(
            "Proceed?".to_string(),
            None,
            "Yes.".to_string(),
            DecisionSource::RuleEngine {
                rule_name: "confirm".to_string(),
            },
            "Pattern match".to_string(),
            vec![],
        )];
        let section = format_pr_decisions_section(&decisions);
        assert!(section.contains("*1 decisions made during this session.*"));
        assert!(section.contains("| 1 |"));
        assert!(!section.contains("| 2 |"));
    }

    // -----------------------------------------------------------------------
    // Helper function tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_truncate_str_short_string() {
        assert_eq!(truncate_str("hello", 80), "hello");
    }

    #[test]
    fn test_truncate_str_exact_length() {
        let s = "a".repeat(80);
        assert_eq!(truncate_str(&s, 80), s);
    }

    #[test]
    fn test_truncate_str_long_string() {
        let s = "a".repeat(100);
        let truncated = truncate_str(&s, 80);
        assert!(truncated.ends_with("..."));
        assert_eq!(truncated.len(), 80);
    }

    #[test]
    fn test_escape_pipe() {
        assert_eq!(escape_pipe("no pipes"), "no pipes");
        assert_eq!(escape_pipe("a|b|c"), "a\\|b\\|c");
    }
}
