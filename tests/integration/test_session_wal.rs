//! Integration tests for Session WAL crash recovery (Story 7.5).
//!
//! Tests the **public API surface** of WAL save → recover → parse, exercising
//! the cross-module boundary: `SessionState::save()` → `SessionRunner::check_and_recover_wal()`
//! → `story_info_from_wal()` → `to_rig_messages()`.
//!
//! Unlike the 20+ unit tests in `src/session/runner.rs` (which use `pub(crate)` internal
//! helpers), these tests construct everything via the library crate's public API — exactly
//! how an external consumer would.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use bmad_bot::config::BotConfig;
use bmad_bot::llm::agent_factory::AgentFactory;
use bmad_bot::mcp::McpManager;
use bmad_bot::notifier::StoryStatus;
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::escalation::EscalationReport;
use bmad_bot::session::runner::{SessionRunner, ShutdownFlag};
use bmad_bot::session::{ChatMessage, SessionOutcome, SessionState};

use crate::helpers::fixtures::{
    impl_artifacts_dir, make_test_config, make_test_secrets, PipelineTestBuilder,
};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a valid `SessionState` representing a crashed session for story "1-2-cli".
///
/// Contains 4 chat messages (2 user, 2 assistant) matching AC #1 exactly.
fn make_valid_wal_state() -> SessionState {
    SessionState {
        story_id: "1.2".to_string(),
        story_key: "1-2-cli".to_string(),
        branch: "story/1-2-cli".to_string(),
        started_at: "2026-02-08T10:00:00+00:00".to_string(),
        last_activity: "2026-02-08T10:05:00+00:00".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        branch_name: "story/1-2-cli".to_string(),
        base_branch: "main".to_string(),
        chat_history: vec![
            ChatMessage {
                role: "user".to_string(),
                content: "DS".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Starting story 1-2-cli...".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Continue.".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Implementation complete.".to_string(),
            },
        ],
    }
}

/// Write a `SessionState` to a WAL file in `dir` using `SessionState::save()`.
///
/// Uses the actual atomic write path (write `.tmp` → rename) to exercise the
/// production save logic, unlike `write_wal_file()` in helpers which serializes
/// directly.
async fn write_wal_to_dir(dir: &Path, state: &SessionState) {
    let wal = wal_path(dir);
    state.save(&wal).await.expect("WAL save must succeed");
}

/// Derive the WAL file path — mirrors `SessionRunner::new()` logic.
fn wal_path(dir: &Path) -> PathBuf {
    dir.join(".bmad-bot-session.yaml")
}

/// Build an `Arc<BotConfig>` with `implementation_artifacts` pointing at `dir`.
fn make_wal_test_config(dir: &Path) -> Arc<BotConfig> {
    let mut config = make_test_config(dir);
    // Override implementation_artifacts to point directly at dir (WAL lives there)
    config.bmad_paths.implementation_artifacts = dir.display().to_string();
    Arc::new(config)
}

/// Build a `SessionRunner` for integration tests.
///
/// Uses real `AgentFactory` and empty `McpManager` — only `check_and_recover_wal()`
/// is called (no LLM interaction).
fn make_session_runner(config: Arc<BotConfig>) -> SessionRunner {
    let secrets = Arc::new(make_test_secrets());
    let agent_factory = Arc::new(AgentFactory::new(config.clone(), secrets));
    let shutdown: ShutdownFlag = Arc::new(AtomicBool::new(false));
    let mcp_manager = Arc::new(McpManager::empty());
    SessionRunner::new(config, agent_factory, shutdown, mcp_manager)
}

// ===========================================================================
// Task 3: Full save → recover → parse integration test (AC #1)
// ===========================================================================

#[tokio::test]
async fn test_wal_recovery_valid_returns_recovery_info() {
    // Arrange
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let config = make_wal_test_config(dir);
    let state = make_valid_wal_state();
    write_wal_to_dir(dir, &state).await;

    let runner = make_session_runner(config);

    // Act
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Should return Some(RecoveryInfo) for valid WAL");

    // Assert — RecoveryInfo.story_info fields
    assert_eq!(recovery.story_info.story_key, "1-2-cli");
    assert_eq!(recovery.story_info.epic_num, 1);
    assert_eq!(recovery.story_info.story_num, 2);
    assert_eq!(recovery.story_info.label, "cli");
    assert_eq!(recovery.story_info.branch_name, "story/1-2-cli");
    assert_eq!(recovery.story_info.status, "in-progress");

    // Assert — RecoveryInfo.state fields
    assert_eq!(recovery.state.provider, "anthropic");
    assert_eq!(recovery.state.model, "claude-sonnet-4-20250514");
    assert_eq!(recovery.state.chat_history.len(), 4);
    assert_eq!(recovery.state.base_branch, "main");
    assert_eq!(recovery.state.story_key, "1-2-cli");
}

// ===========================================================================
// Task 4: to_rig_messages conversion test (AC #2)
// ===========================================================================

#[tokio::test]
async fn test_wal_to_rig_messages_preserves_all_messages() {
    // Arrange
    let state = make_valid_wal_state();

    // Act
    let messages = state.to_rig_messages();

    // Assert — length matches chat_history
    assert_eq!(messages.len(), 4, "Should convert all 4 chat messages");

    // Verify ordering via debug format (rig Message internals may not be directly inspectable)
    let debug_first = format!("{:?}", messages[0]);
    assert!(
        debug_first.to_lowercase().contains("user")
            || debug_first.to_lowercase().contains("ds"),
        "First message should be user type, got: {debug_first}"
    );
}

#[tokio::test]
async fn test_wal_to_rig_messages_empty_history() {
    // Arrange — state with no messages
    let mut state = make_valid_wal_state();
    state.chat_history.clear();

    // Act
    let messages = state.to_rig_messages();

    // Assert
    assert!(messages.is_empty(), "Empty chat_history → empty messages");
}

// ===========================================================================
// Task 5: Corrupt WAL test (AC #3)
// ===========================================================================

#[tokio::test]
async fn test_wal_corrupt_deletes_file_returns_none() {
    // Arrange
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let config = make_wal_test_config(dir);

    // Write garbage YAML to WAL path
    let wal = wal_path(dir);
    tokio::fs::write(&wal, "{{{{not: valid: yaml: [broken")
        .await
        .expect("write corrupt WAL");
    assert!(wal.exists(), "Corrupt WAL should exist before recovery");

    let runner = make_session_runner(config);

    // Act
    let result = runner.check_and_recover_wal().await;

    // Assert
    assert!(result.is_none(), "Corrupt WAL must return None");
    assert!(!wal.exists(), "Corrupt WAL file must be deleted");
}

// ===========================================================================
// Task 6: No-WAL test (AC #4)
// ===========================================================================

#[tokio::test]
async fn test_wal_no_file_returns_none() {
    // Arrange — empty temp dir, no WAL file
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let config = make_wal_test_config(dir);
    let runner = make_session_runner(config);

    // Act
    let result = runner.check_and_recover_wal().await;

    // Assert
    assert!(
        result.is_none(),
        "No WAL file should return None immediately"
    );
}

// ===========================================================================
// Task 7: Post-recovery pipeline test (AC #5)
// ===========================================================================

#[tokio::test]
async fn test_wal_process_recovered_session_completed() {
    // Arrange — pipeline with mocks
    let story = crate::helpers::fixtures::make_test_story("1-2-cli", "cli", vec![]);
    let outcome = SessionOutcome::Completed {
        story_key: "1-2-cli".to_string(),
        branch: "story/1-2-cli".to_string(),
        decisions: vec![],
        pr_context: Some("Recovery context".to_string()),
        pr_how_to_test: None,
        pr_additional_info: None,
    };

    let (pipeline, _notifier, git_provider, _env) = PipelineTestBuilder::new()
        .with_session(SessionOutcome::Completed {
            story_key: "1-2-cli".to_string(),
            branch: "story/1-2-cli".to_string(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        })
        .with_review(ReviewOutcome::Skipped {
            reason: "test".to_string(),
        })
        .build();

    // Act
    let result = pipeline
        .process_recovered_session(&story, outcome)
        .await;

    // Assert
    assert_eq!(result.story_key, "1-2-cli");
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some(), "Completed recovery should create PR");
    assert_eq!(git_provider.create_pr_call_count(), 1);
}

#[tokio::test]
async fn test_wal_process_recovered_session_failed_creates_failure_pr() {
    // Arrange
    let story = crate::helpers::fixtures::make_test_story("1-2-cli", "cli", vec![]);
    let outcome = SessionOutcome::Failed {
        story_key: "1-2-cli".to_string(),
        error: "LLM tool error: file not found".to_string(),
        decisions: vec![],
    };

    let (pipeline, _notifier, git_provider, _env) = PipelineTestBuilder::new()
        .with_session(SessionOutcome::Failed {
            story_key: "1-2-cli".to_string(),
            error: "dummy".to_string(),
            decisions: vec![],
        })
        .build();

    // Act
    let result = pipeline
        .process_recovered_session(&story, outcome)
        .await;

    // Assert — non-infra failure creates a PR with [NEEDS REVIEW]
    assert_eq!(result.story_key, "1-2-cli");
    assert_eq!(result.status, StoryStatus::Error);
    assert!(
        result.pr_url.is_some(),
        "Non-infra failure recovery should create failure PR"
    );
    assert_eq!(git_provider.create_pr_call_count(), 1);

    // Verify PR title contains NEEDS REVIEW marker
    let pr_params = git_provider.captured_create_pr_params();
    assert!(!pr_params.is_empty(), "Should have captured PR params");
    assert!(
        pr_params[0].title.contains("NEEDS REVIEW"),
        "Failure PR title should contain 'NEEDS REVIEW', got: {}",
        pr_params[0].title
    );
}

#[tokio::test]
async fn test_wal_process_recovered_session_escalated_creates_escalation_pr() {
    // Arrange
    let story = crate::helpers::fixtures::make_test_story("1-2-cli", "cli", vec![]);
    let outcome = SessionOutcome::Escalated {
        report: EscalationReport {
            story_key: "1-2-cli".to_string(),
            question: "What database?".to_string(),
            reason: "Not in architecture docs".to_string(),
            branch_name: "story/1-2-cli".to_string(),
            partial_work_summary: "Created stubs".to_string(),
            escalated_at: "2026-02-08T19:00:00+00:00".to_string(),
        },
        decisions: vec![],
    };

    let (pipeline, _notifier, git_provider, _env) = PipelineTestBuilder::new()
        .with_session(SessionOutcome::Escalated {
            report: EscalationReport {
                story_key: "1-2-cli".to_string(),
                question: "dummy".to_string(),
                reason: "dummy".to_string(),
                branch_name: "story/1-2-cli".to_string(),
                partial_work_summary: String::new(),
                escalated_at: "2026-02-08T19:00:00+00:00".to_string(),
            },
            decisions: vec![],
        })
        .build();

    // Act
    let result = pipeline
        .process_recovered_session(&story, outcome)
        .await;

    // Assert — escalation creates PR and status is Blocked
    assert_eq!(result.story_key, "1-2-cli");
    assert_eq!(result.status, StoryStatus::Blocked);
    // Escalation PR is created (best effort)
    assert_eq!(git_provider.create_pr_call_count(), 1);
}

// ===========================================================================
// Task 8: Recovery-first priority test (AC #6)
// ===========================================================================

#[tokio::test]
async fn test_wal_recover_and_process_with_wal() {
    // Arrange — write valid WAL and use a real SessionRunner
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let impl_dir = impl_artifacts_dir(dir);
    let state = make_valid_wal_state();
    let wal = impl_dir.join(".bmad-bot-session.yaml");
    state.save(&wal).await.expect("WAL save");

    let config = Arc::new(make_test_config(dir));
    let runner = make_session_runner(config.clone());

    // Verify WAL is detected
    let recovery = runner.check_and_recover_wal().await;
    assert!(
        recovery.is_some(),
        "WAL should be detected (recovery-first priority)"
    );
    let recovery = recovery.unwrap();
    assert_eq!(recovery.story_info.story_key, "1-2-cli");
}

#[tokio::test]
async fn test_wal_recover_and_process_without_wal_returns_none() {
    // Arrange — no WAL, pipeline via new_with_components (session_runner_for_recovery = None)
    let (pipeline, _notifier, _git_provider, _env) = PipelineTestBuilder::new()
        .with_session(SessionOutcome::Completed {
            story_key: "1-2-cli".to_string(),
            branch: "story/1-2-cli".to_string(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        })
        .build();

    // Act — new_with_components sets session_runner_for_recovery = None
    let result = pipeline.recover_and_process().await;

    // Assert — returns None (daemon proceeds to polling)
    assert!(
        result.is_none(),
        "Pipeline without session_runner_for_recovery should return None"
    );
}

// ===========================================================================
// Task 9: Legacy WAL backward compatibility test
// ===========================================================================

#[tokio::test]
async fn test_wal_legacy_branch_fallback() {
    // Arrange — WAL with empty branch_name but populated legacy branch field
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let config = make_wal_test_config(dir);

    let mut state = make_valid_wal_state();
    state.branch_name = String::new(); // empty — triggers fallback
    state.branch = "story/1-2-cli-legacy".to_string(); // legacy field

    write_wal_to_dir(dir, &state).await;

    let runner = make_session_runner(config);

    // Act
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Should recover from legacy WAL");

    // Assert — branch_name falls back to legacy `branch` value
    assert_eq!(
        recovery.story_info.branch_name, "story/1-2-cli-legacy",
        "story_info.branch_name should fall back to legacy branch field"
    );
}

// ===========================================================================
// Task 10: Forward-compatibility test
// ===========================================================================

#[tokio::test]
async fn test_wal_forward_compat_unknown_fields_ignored() {
    // Arrange — write WAL with extra unknown YAML fields
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let config = make_wal_test_config(dir);

    let state = make_valid_wal_state();
    let mut yaml = serde_yml::to_string(&state).expect("serialize");
    yaml.push_str("extra_field: \"unknown_value\"\n");
    yaml.push_str("another_future_field: 42\n");

    let wal = wal_path(dir);
    tokio::fs::write(&wal, &yaml)
        .await
        .expect("write WAL with extra fields");

    let runner = make_session_runner(config);

    // Act
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Should recover despite unknown fields");

    // Assert — recovery succeeds with correct data
    assert_eq!(recovery.story_info.story_key, "1-2-cli");
    assert_eq!(recovery.state.chat_history.len(), 4);
}
