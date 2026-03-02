//! Integration tests for Session WAL crash recovery.
//!
//! Tests the full public API chain:
//! `SessionState::save()` → `SessionRunner::check_and_recover_wal()` →
//! `story_info_from_wal()` → `SessionState::to_rig_messages()`.
//!
//! These tests validate the cross-module boundary from an external crate
//! perspective (`use bmad_bot::session::*`), complementing the 20+ unit tests
//! in `src/session/runner.rs` that use internal (`pub(crate)`) helpers.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use bmad_bot::config::{BotConfig, BotSecrets};
use bmad_bot::llm::AgentFactory;
use bmad_bot::mcp::McpManager;
use bmad_bot::pipeline::StoryPipeline;
use bmad_bot::session::runner::SessionRunner;
use bmad_bot::session::{ChatMessage, SessionOutcome, SessionState};
use crate::helpers::fixtures::{make_test_config, make_test_secrets, make_test_story};
use crate::helpers::mocks::{MockCodeReviewer, MockDevRunner, MockGitProvider, MockNotifier};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a valid `SessionState` matching AC #1 spec:
/// - `story_key: "1-2-cli"`, `branch_name: "story/1-2-cli"`, `base_branch: "main"`
/// - 4 chat messages (2 user, 2 assistant)
/// - `provider: "anthropic"`, `model: "claude-sonnet-4-20250514"`
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

/// Write a `SessionState` to disk as a WAL file using the public `save()` API.
///
/// The WAL file is written to `{dir}/.bmad-bot-session.yaml` via atomic rename.
async fn write_wal_to_dir(dir: &Path, state: &SessionState) {
    let wal = wal_path(dir);
    state.save(&wal).await.expect("Failed to save WAL file");
}

/// Build a `BotConfig` with `implementation_artifacts` pointing at the given directory.
///
/// This ensures `SessionRunner` derives its WAL path to `{dir}/.bmad-bot-session.yaml`.
fn make_wal_test_config(dir: &Path) -> Arc<BotConfig> {
    let mut config = make_test_config(dir);
    // Override implementation_artifacts to point directly at `dir` so the WAL
    // file path is `{dir}/.bmad-bot-session.yaml` — matching `write_wal_to_dir`.
    config.bmad_paths.implementation_artifacts = dir.to_string_lossy().to_string();
    Arc::new(config)
}

/// Build dummy `BotSecrets` for tests.
fn make_wal_test_secrets() -> Arc<BotSecrets> {
    Arc::new(make_test_secrets())
}

/// Construct a `SessionRunner` from config + secrets for WAL tests.
///
/// `check_and_recover_wal()` only touches the filesystem — it never calls the LLM.
fn make_test_runner(config: Arc<BotConfig>) -> SessionRunner {
    let secrets = make_wal_test_secrets();
    let factory = Arc::new(AgentFactory::new(Arc::clone(&config), secrets));
    let shutdown = Arc::new(AtomicBool::new(false));
    let mcp = Arc::new(McpManager::empty());
    SessionRunner::new(config, factory, shutdown, mcp)
}

/// Derive the WAL file path for a given directory.
///
/// Mirrors `SessionRunner::new()` internal derivation:
/// `Path::new(&config.bmad_paths.implementation_artifacts).join(".bmad-bot-session.yaml")`
fn wal_path(dir: &Path) -> PathBuf {
    dir.join(".bmad-bot-session.yaml")
}

// ---------------------------------------------------------------------------
// Task 3: Full save→recover→parse integration test (AC #1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_recovery_valid_returns_recovery_info() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let state = make_valid_wal_state();
    write_wal_to_dir(dir.path(), &state).await;

    let config = make_wal_test_config(dir.path());
    let runner = make_test_runner(config);

    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Expected Some(RecoveryInfo)");

    // Verify StoryInfo fields (AC #1)
    assert_eq!(recovery.story_info.story_key, "1-2-cli");
    assert_eq!(recovery.story_info.epic_num, 1);
    assert_eq!(recovery.story_info.story_num, 2);
    assert_eq!(recovery.story_info.label, "cli");
    assert_eq!(recovery.story_info.branch_name, "story/1-2-cli");
    assert_eq!(recovery.story_info.status, "in-progress");
    assert!(recovery.story_info.dependencies.is_empty());

    // Verify SessionState fields (AC #1)
    assert_eq!(recovery.state.provider, "anthropic");
    assert_eq!(recovery.state.model, "claude-sonnet-4-20250514");
    assert_eq!(recovery.state.chat_history.len(), 4);
    assert_eq!(recovery.state.base_branch, "main");
    assert_eq!(recovery.state.branch_name, "story/1-2-cli");
}

// ---------------------------------------------------------------------------
// Task 4: to_rig_messages conversion test (AC #2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_to_rig_messages_conversion() {
    let state = make_valid_wal_state();

    let messages = state.to_rig_messages();
    assert_eq!(messages.len(), 4, "Expected 4 rig messages from chat_history");

    // Verify ordering via debug format — first message should be user type
    let debug_first = format!("{:?}", messages[0]);
    assert!(
        debug_first.to_lowercase().contains("user")
            || debug_first.to_lowercase().contains("ds"),
        "First message should be user type or contain 'DS', got: {debug_first}"
    );
}

#[tokio::test]
async fn test_wal_to_rig_messages_preserves_order() {
    let mut state = make_valid_wal_state();
    // Add a distinguishing message at the end
    state.chat_history.push(ChatMessage {
        role: "user".to_string(),
        content: "FINAL_MARKER".to_string(),
    });

    let messages = state.to_rig_messages();
    assert_eq!(messages.len(), 5);

    // Last message should contain our marker
    let debug_last = format!("{:?}", messages[4]);
    assert!(
        debug_last.contains("FINAL_MARKER"),
        "Last message should contain FINAL_MARKER, got: {debug_last}"
    );
}

// ---------------------------------------------------------------------------
// Task 5: Corrupt WAL test (AC #3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_recovery_corrupt_deletes_and_returns_none() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let wal = wal_path(dir.path());

    // Write garbage to WAL file
    tokio::fs::write(&wal, "!!!CORRUPT{{{not yaml at all}}}!!!")
        .await
        .expect("write corrupt WAL");

    assert!(wal.exists(), "WAL file should exist before recovery");

    let config = make_wal_test_config(dir.path());
    let runner = make_test_runner(config);

    let result = runner.check_and_recover_wal().await;
    assert!(result.is_none(), "Corrupt WAL should return None");

    // AC #3: corrupt WAL file should be deleted
    assert!(
        !wal.exists(),
        "Corrupt WAL file should be deleted after failed recovery"
    );
}

// ---------------------------------------------------------------------------
// Task 6: No-WAL test (AC #4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_recovery_no_file_returns_none() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let wal = wal_path(dir.path());
    assert!(!wal.exists(), "WAL file should not exist in fresh dir");

    let config = make_wal_test_config(dir.path());
    let runner = make_test_runner(config);

    let result = runner.check_and_recover_wal().await;
    assert!(result.is_none(), "No WAL file should return None");
}

// ---------------------------------------------------------------------------
// Task 7: Post-recovery pipeline test (AC #5)
// ---------------------------------------------------------------------------

/// Helper: build a `StoryPipeline` with a real git repo and mock components.
///
/// Returns the pipeline, mock handles for assertions, and a `TempDir` guard
/// that must be kept alive for the test duration.
fn build_pipeline_with_git(
    branches: &[&str],
    mock_git: MockGitProvider,
) -> (
    StoryPipeline,
    MockNotifier,
    MockGitProvider,
    MockCodeReviewer,
    tempfile::TempDir,
) {
    use crate::helpers::fixtures::create_pipeline_git_env;

    let temp_dir = tempfile::tempdir().expect("create temp dir");
    let work_dir = create_pipeline_git_env(temp_dir.path(), branches);
    let mut config = make_test_config(&work_dir);
    config.code_review_enabled = false;

    let dev_runner = MockDevRunner::with_outcome(SessionOutcome::Completed {
        story_key: "1-2-cli".to_string(),
        branch: "story/1-2-cli".to_string(),
        decisions: vec![],
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    });

    let reviewer = MockCodeReviewer::never_called();
    let reviewer_for_assertions = reviewer.clone();
    let notifier = MockNotifier::new();
    let notifier_for_assertions = notifier.clone();
    let git_for_assertions = mock_git.clone();

    let pipeline = StoryPipeline::new_with_components(
        Arc::new(config),
        Box::new(mock_git),
        Box::new(notifier),
        Box::new(dev_runner),
        Box::new(reviewer),
    );

    (
        pipeline,
        notifier_for_assertions,
        git_for_assertions,
        reviewer_for_assertions,
        temp_dir,
    )
}

#[tokio::test]
async fn test_wal_pipeline_completed_outcome_creates_pr() {
    use bmad_bot::git_provider::PrInfo;

    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "PR-42".to_string(),
        url: "https://github.com/test/test/pull/42".to_string(),
        number: 42,
    }));

    let (pipeline, notifier, git, _reviewer, _dir) =
        build_pipeline_with_git(&["story/1-2-cli"], mock_git);

    let story = make_test_story("1-2-cli", "cli", vec![]);
    let outcome = SessionOutcome::Completed {
        story_key: "1-2-cli".to_string(),
        branch: "story/1-2-cli".to_string(),
        decisions: vec![],
        pr_context: None,
        pr_how_to_test: None,
        pr_additional_info: None,
    };

    let result = pipeline.process_recovered_session(&story, outcome).await;

    assert_eq!(result.story_key, "1-2-cli");
    assert_eq!(
        result.status,
        bmad_bot::notifier::StoryStatus::Completed,
        "Completed outcome should produce Completed status"
    );
    assert!(result.pr_url.is_some(), "PR URL should be set");
    assert_eq!(
        git.create_pr_call_count(),
        1,
        "create_pr should be called once"
    );
    // Notifier is called by the pipeline wrapper, not process_recovered_session directly
    let _ = notifier;
}

#[tokio::test]
async fn test_wal_pipeline_failed_outcome_creates_failure_pr() {
    use bmad_bot::git_provider::PrInfo;

    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "PR-99".to_string(),
        url: "https://github.com/test/test/pull/99".to_string(),
        number: 99,
    }));

    let (pipeline, _notifier, git, _reviewer, _dir) =
        build_pipeline_with_git(&["story/1-2-cli"], mock_git);

    let story = make_test_story("1-2-cli", "cli", vec![]);
    let outcome = SessionOutcome::Failed {
        story_key: "1-2-cli".to_string(),
        error: "Session crashed during implementation".to_string(),
        decisions: vec![],
    };

    let result = pipeline.process_recovered_session(&story, outcome).await;

    assert_eq!(result.story_key, "1-2-cli");
    assert_eq!(
        result.status,
        bmad_bot::notifier::StoryStatus::Error,
        "Failed outcome should produce Error status"
    );
    // Non-infra failure → failure PR is created
    assert!(
        result.pr_url.is_some(),
        "Failure PR URL should be set for non-infra errors"
    );
    assert_eq!(
        git.create_pr_call_count(),
        1,
        "create_pr should be called once for failure PR"
    );
}

#[tokio::test]
async fn test_wal_pipeline_escalated_outcome_creates_escalation_pr() {
    use bmad_bot::git_provider::PrInfo;
    use bmad_bot::session::escalation::EscalationReport;

    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "PR-100".to_string(),
        url: "https://github.com/test/test/pull/100".to_string(),
        number: 100,
    }));

    let (pipeline, _notifier, git, _reviewer, _dir) =
        build_pipeline_with_git(&["story/1-2-cli"], mock_git);

    let story = make_test_story("1-2-cli", "cli", vec![]);
    let outcome = SessionOutcome::Escalated {
        report: EscalationReport {
            story_key: "1-2-cli".to_string(),
            question: "How should authentication work?".to_string(),
            reason: "Architecture decision needed".to_string(),
            partial_work_summary: "Implemented basic structure".to_string(),
            branch_name: "story/1-2-cli".to_string(),
            escalated_at: "2026-02-08T10:00:00+00:00".to_string(),
        },
        decisions: vec![],
    };

    let result = pipeline.process_recovered_session(&story, outcome).await;

    assert_eq!(result.story_key, "1-2-cli");
    assert_eq!(
        result.status,
        bmad_bot::notifier::StoryStatus::Blocked,
        "Escalated outcome should produce Blocked status"
    );
    // Escalated → creates escalation PR (push best-effort + PR)
    assert!(
        result.pr_url.is_some(),
        "Escalation PR URL should be set"
    );
    assert_eq!(
        git.create_pr_call_count(),
        1,
        "create_pr should be called once for escalation PR"
    );
}

// ---------------------------------------------------------------------------
// Task 8: Recovery-first priority test (AC #6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_recovery_first_with_wal_present() {
    // With WAL present, check_and_recover_wal should detect it before any polling
    let dir = tempfile::tempdir().expect("create temp dir");
    let state = make_valid_wal_state();
    write_wal_to_dir(dir.path(), &state).await;

    let config = make_wal_test_config(dir.path());
    let runner = make_test_runner(config);

    // This simulates the daemon startup: check WAL first
    let recovery = runner.check_and_recover_wal().await;
    assert!(
        recovery.is_some(),
        "WAL should be detected on startup check"
    );
}

#[tokio::test]
async fn test_wal_recovery_first_no_wal_proceeds_to_polling() {
    // With no WAL, check returns None → daemon can proceed to story polling
    let dir = tempfile::tempdir().expect("create temp dir");

    let config = make_wal_test_config(dir.path());
    let runner = make_test_runner(config);

    let recovery = runner.check_and_recover_wal().await;
    assert!(
        recovery.is_none(),
        "No WAL → None → daemon proceeds to polling"
    );
}

#[tokio::test]
async fn test_wal_pipeline_recover_and_process_returns_none_without_session_runner() {
    // new_with_components() sets session_runner_for_recovery = None
    // → recover_and_process() returns None (daemon proceeds to polling)
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());

    let pipeline = StoryPipeline::new_with_components(
        Arc::new(config),
        Box::new(MockGitProvider::new()),
        Box::new(MockNotifier::new()),
        Box::new(MockDevRunner::with_outcome(SessionOutcome::Completed {
            story_key: "x".to_string(),
            branch: "story/x".to_string(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        })),
        Box::new(MockCodeReviewer::never_called()),
    );

    let result = pipeline.recover_and_process().await;
    assert!(
        result.is_none(),
        "recover_and_process should return None when session_runner_for_recovery is None"
    );
}

// ---------------------------------------------------------------------------
// Task 9: Legacy WAL backward compatibility test (supplementary)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_legacy_branch_field_fallback() {
    let dir = tempfile::tempdir().expect("create temp dir");

    // Create state with empty branch_name but populated branch (pre-4.3 format)
    let mut state = make_valid_wal_state();
    state.branch_name = String::new(); // empty → should fall back to `branch`
    state.branch = "story/1-2-cli-legacy".to_string();

    write_wal_to_dir(dir.path(), &state).await;

    let config = make_wal_test_config(dir.path());
    let runner = make_test_runner(config);

    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Recovery should succeed for legacy WAL");

    assert_eq!(
        recovery.story_info.branch_name, "story/1-2-cli-legacy",
        "story_info.branch_name should fall back to legacy `branch` field"
    );
}

// ---------------------------------------------------------------------------
// Task 10: Forward-compatibility test (supplementary)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_forward_compat_unknown_fields_ignored() {
    let dir = tempfile::tempdir().expect("create temp dir");

    // Write a valid WAL then append unknown YAML fields
    let state = make_valid_wal_state();
    let mut yaml = serde_yml::to_string(&state).expect("serialize WAL state");
    yaml.push_str("extra_field: \"unknown\"\n");
    yaml.push_str("future_version: 99\n");

    let wal = wal_path(dir.path());
    tokio::fs::write(&wal, &yaml)
        .await
        .expect("write WAL with extra fields");

    let config = make_wal_test_config(dir.path());
    let runner = make_test_runner(config);

    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Recovery should succeed despite unknown YAML fields");

    assert_eq!(recovery.story_info.story_key, "1-2-cli");
    assert_eq!(recovery.state.chat_history.len(), 4);
}
