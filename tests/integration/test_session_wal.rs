//! Integration tests for session WAL crash recovery.
//!
//! Tests verify the full save→recover→parse chain through the **public API surface**:
//! `SessionState::save()` → `SessionRunner::check_and_recover_wal()` →
//! `story_info_from_wal()` → `SessionState::to_rig_messages()`.
//!
//! Story 7.5: Session WAL Crash Recovery Integration Tests

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use bmad_bot::config::BotSecrets;
use bmad_bot::llm::AgentFactory;
use bmad_bot::mcp::McpManager;
use bmad_bot::notifier::StoryStatus;
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::escalation::EscalationReport;
use bmad_bot::session::runner::{story_info_from_wal, SessionRunner};
use bmad_bot::session::{ChatMessage, SessionOutcome, SessionState};

use super::helpers::fixtures::{
    create_story_branch, create_test_repo_with_remote, make_test_config, make_test_secrets,
    make_test_story, PipelineTestBuilder,
};
use super::helpers::mocks::GitProviderCall;

// ---------------------------------------------------------------------------
// WAL-specific fixture helpers (Task 2)
// ---------------------------------------------------------------------------

/// Build a valid `SessionState` matching AC #1 exactly:
/// story_key="1-2-cli", branch_name="story/1-2-cli", base_branch="main",
/// provider="anthropic", model="claude-sonnet-4-20250514", 4 chat messages.
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

/// Write a `SessionState` to the WAL path via `SessionState::save()` (async, atomic write).
async fn write_wal_to_dir(dir: &Path, state: &SessionState) {
    let path = wal_path(dir);
    state.save(&path).await.expect("Failed to save WAL state");
}

/// Derive the WAL file path from a directory.
///
/// Mirrors `SessionRunner::new()`: `{implementation_artifacts}/.bmad-bot-session.yaml`.
fn wal_path(dir: &Path) -> PathBuf {
    dir.join(".bmad-bot-session.yaml")
}

/// Build a `SessionRunner` for integration tests.
///
/// Requires `Arc<BotConfig>` pointing `implementation_artifacts` at `dir`.
fn make_test_runner(dir: &Path) -> SessionRunner {
    let config = Arc::new(make_test_config(dir));
    let secrets: Arc<BotSecrets> = Arc::new(make_test_secrets());
    let factory = Arc::new(AgentFactory::new(Arc::clone(&config), secrets));
    let shutdown = Arc::new(AtomicBool::new(false));
    let mcp = Arc::new(McpManager::empty());
    SessionRunner::new(config, factory, shutdown, mcp)
}

/// Set up a temp dir with git repo + remote + story branch, return (tempdir, work_path).
fn setup_git_env(story_branch: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let work = create_test_repo_with_remote(tmp.path());
    create_story_branch(&work, story_branch);
    (tmp, work)
}

// ===========================================================================
// Task 3 — Full save→recover→parse integration test (AC #1)
// ===========================================================================

#[tokio::test]
async fn test_wal_recovery_valid_returns_recovery_info() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    // Arrange: write a valid WAL
    let state = make_valid_wal_state();
    write_wal_to_dir(dir, &state).await;

    // Act: construct runner + recover
    let runner = make_test_runner(dir);
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Expected Some(RecoveryInfo)");

    // Assert: StoryInfo fields
    assert_eq!(recovery.story_info.story_key, "1-2-cli");
    assert_eq!(recovery.story_info.epic_num, 1);
    assert_eq!(recovery.story_info.story_num, 2);
    assert_eq!(recovery.story_info.label, "cli");
    assert_eq!(recovery.story_info.branch_name, "story/1-2-cli");
    assert_eq!(recovery.story_info.status, "in-progress");

    // Assert: SessionState fields
    assert_eq!(recovery.state.provider, "anthropic");
    assert_eq!(recovery.state.model, "claude-sonnet-4-20250514");
    assert_eq!(recovery.state.chat_history.len(), 4);
    assert_eq!(recovery.state.base_branch, "main");
}

#[tokio::test]
async fn test_wal_recovery_story_info_from_wal_produces_correct_story_info() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    let state = make_valid_wal_state();
    let config = Arc::new(make_test_config(dir));
    let story_info = story_info_from_wal(&state, &config);

    assert_eq!(story_info.story_key, "1-2-cli");
    assert_eq!(story_info.story_id, "1.2");
    assert_eq!(story_info.epic_num, 1);
    assert_eq!(story_info.story_num, 2);
    assert_eq!(story_info.label, "cli");
    assert_eq!(story_info.branch_name, "story/1-2-cli");
    assert!(story_info.dependencies.is_empty());
    assert_eq!(story_info.status, "in-progress");
    // specs_path should contain the story key
    assert!(
        story_info
            .specs_path
            .to_str()
            .unwrap()
            .contains("1-2-cli.md"),
        "specs_path should contain 1-2-cli.md, got: {:?}",
        story_info.specs_path
    );
}

// ===========================================================================
// Task 4 — to_rig_messages conversion test (AC #2)
// ===========================================================================

#[tokio::test]
async fn test_wal_to_rig_messages_converts_all_messages() {
    let state = make_valid_wal_state();
    let messages = state.to_rig_messages();

    // AC #2: assert length == 4
    assert_eq!(messages.len(), 4, "Expected 4 messages, got {}", messages.len());

    // Verify message ordering via debug format (rig Message internals may not be directly inspectable)
    let debug_first = format!("{:?}", messages[0]);
    assert!(
        debug_first.to_lowercase().contains("user")
            || debug_first.to_lowercase().contains("ds"),
        "First message should be user type, got: {debug_first}"
    );

    let debug_second = format!("{:?}", messages[1]);
    assert!(
        debug_second.to_lowercase().contains("assistant")
            || debug_second.to_lowercase().contains("starting story"),
        "Second message should be assistant type, got: {debug_second}"
    );
}

#[tokio::test]
async fn test_wal_to_rig_messages_empty_history() {
    let mut state = make_valid_wal_state();
    state.chat_history.clear();
    let messages = state.to_rig_messages();
    assert_eq!(messages.len(), 0);
}

// ===========================================================================
// Task 5 — Corrupt WAL test (AC #3)
// ===========================================================================

#[tokio::test]
async fn test_wal_corrupt_file_returns_none_and_deletes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let path = wal_path(dir);

    // Arrange: write raw garbage to WAL path
    tokio::fs::write(&path, "this is not: valid: yaml: [[[{{{")
        .await
        .expect("write garbage");

    // Sanity: file exists
    assert!(path.exists(), "WAL file should exist before recovery");

    // Act
    let runner = make_test_runner(dir);
    let result = runner.check_and_recover_wal().await;

    // Assert: None returned AND WAL file deleted
    assert!(result.is_none(), "Corrupt WAL should return None");
    assert!(
        !path.exists(),
        "Corrupt WAL file should be deleted after recovery attempt"
    );
}

// ===========================================================================
// Task 6 — No-WAL test (AC #4)
// ===========================================================================

#[tokio::test]
async fn test_wal_no_file_returns_none() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    // Act: empty dir, no WAL file
    let runner = make_test_runner(dir);
    let result = runner.check_and_recover_wal().await;

    // Assert: None returned immediately
    assert!(result.is_none(), "No WAL file should return None");
}

// ===========================================================================
// Task 7 — Post-recovery pipeline test (AC #5)
// ===========================================================================

#[tokio::test]
async fn test_wal_pipeline_completed_recovery() {
    let (_tmp, work) = setup_git_env("story/1-2-cli");
    let story = make_test_story("1-2-cli", "cli", vec![]);

    let review = ReviewOutcome::Completed {
        story_key: "1-2-cli".to_string(),
        branch: "story/1-2-cli".to_string(),
        report: "LGTM".to_string(),
    };

    let (pipeline, _notifier, git, _reviewer) = PipelineTestBuilder::new(&work)
        .with_review(review)
        .build();

    // Call process_recovered_session directly with a Completed outcome
    let outcome = SessionOutcome::Completed {
        story_key: "1-2-cli".to_string(),
        branch: "story/1-2-cli".to_string(),
        decisions: vec![],
        pr_context: Some("Recovered context".to_string()),
        pr_how_to_test: None,
        pr_additional_info: None,
    };
    let result = pipeline
        .process_recovered_session(&story, outcome)
        .await;

    // AC #5: status Completed, pr_url present
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(result.pr_url.is_some(), "PR URL should be present");
    assert!(result.error_detail.is_none());

    // MockGitProvider received create_pr
    let calls = git.calls();
    assert!(
        calls
            .iter()
            .any(|c| matches!(c, GitProviderCall::CreatePr(_))),
        "MockGitProvider should have received create_pr, got: {calls:?}"
    );
}

#[tokio::test]
async fn test_wal_pipeline_failed_recovery_creates_pr() {
    let (_tmp, work) = setup_git_env("story/1-2-cli");
    let story = make_test_story("1-2-cli", "cli", vec![]);

    let outcome = SessionOutcome::Failed {
        story_key: "1-2-cli".to_string(),
        error: "Session crashed mid-work".to_string(),
        decisions: vec![],
    };

    let (pipeline, _notifier, git, _reviewer) = PipelineTestBuilder::new(&work)
        .with_code_review(false)
        .build();

    let result = pipeline
        .process_recovered_session(&story, outcome)
        .await;

    // AC #5: status Error, PR still created for non-infra failures
    assert_eq!(result.status, StoryStatus::Error);
    assert!(
        result.pr_url.is_some(),
        "Failed recovery should still create a PR with partial work"
    );
    assert!(result.error_detail.is_some());

    // Verify MockGitProvider received create_pr with [NEEDS REVIEW] in title
    let calls = git.calls();
    let create_pr_call = calls.iter().find(|c| matches!(c, GitProviderCall::CreatePr(_)));
    assert!(
        create_pr_call.is_some(),
        "MockGitProvider should have received create_pr for failure PR"
    );
    if let Some(GitProviderCall::CreatePr(params)) = create_pr_call {
        assert!(
            params.title.contains("[NEEDS REVIEW]"),
            "Failure PR title should contain [NEEDS REVIEW], got: {}",
            params.title
        );
    }
}

#[tokio::test]
async fn test_wal_pipeline_escalated_recovery() {
    let (_tmp, work) = setup_git_env("story/1-2-cli");
    let story = make_test_story("1-2-cli", "cli", vec![]);

    let outcome = SessionOutcome::Escalated {
        report: EscalationReport {
            story_key: "1-2-cli".to_string(),
            question: "How should auth tokens be stored?".to_string(),
            reason: "Architecture decision needed".to_string(),
            branch_name: "story/1-2-cli".to_string(),
            partial_work_summary: "Partial implementation of CLI".to_string(),
            escalated_at: "2026-02-08T10:30:00+00:00".to_string(),
        },
        decisions: vec![],
    };

    let (pipeline, _notifier, git, _reviewer) = PipelineTestBuilder::new(&work)
        .with_code_review(false)
        .build();

    let result = pipeline
        .process_recovered_session(&story, outcome)
        .await;

    // AC #5: Escalated → status Blocked, escalation PR created
    assert_eq!(result.status, StoryStatus::Blocked);
    // The code creates an escalation PR
    assert!(
        result.pr_url.is_some(),
        "Escalated recovery should create an escalation PR"
    );

    let calls = git.calls();
    assert!(
        calls
            .iter()
            .any(|c| matches!(c, GitProviderCall::CreatePr(_))),
        "MockGitProvider should have received create_pr for escalation"
    );
}

// ===========================================================================
// Task 8 — Recovery-first priority test (AC #6)
// ===========================================================================

#[tokio::test]
async fn test_wal_recovery_first_wal_detected() {
    // AC #6: WAL exists → check_and_recover_wal() returns Some
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    let state = make_valid_wal_state();
    write_wal_to_dir(dir, &state).await;

    let runner = make_test_runner(dir);
    let recovery = runner.check_and_recover_wal().await;

    assert!(
        recovery.is_some(),
        "WAL exists → recovery should be Some (processed first)"
    );
}

#[tokio::test]
async fn test_wal_recovery_first_no_wal_proceeds_to_polling() {
    // AC #6: No WAL → check_and_recover_wal() returns None (daemon proceeds to polling)
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    let runner = make_test_runner(dir);
    let recovery = runner.check_and_recover_wal().await;

    assert!(
        recovery.is_none(),
        "No WAL → recovery should be None (proceed to polling)"
    );
}

#[tokio::test]
async fn test_wal_recover_and_process_returns_none_without_session_runner() {
    // new_with_components() sets session_runner_for_recovery = None
    // so recover_and_process() returns None (safe no-op for test pipelines)
    let (_tmp, work) = setup_git_env("story/1-2-cli");

    let (pipeline, _notifier, _git, _reviewer) = PipelineTestBuilder::new(&work).build();
    let result = pipeline.recover_and_process().await;

    assert!(
        result.is_none(),
        "recover_and_process() via new_with_components() should return None"
    );
}

// ===========================================================================
// Task 9 — Legacy WAL backward compatibility test (supplementary)
// ===========================================================================

#[tokio::test]
async fn test_wal_legacy_branch_fallback() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();

    // Create WAL with empty branch_name but populated branch (pre-4.3 format)
    let state = SessionState {
        story_id: "1.2".to_string(),
        story_key: "1-2-cli".to_string(),
        branch: "story/1-2-cli-legacy".to_string(),
        started_at: "2026-02-08T10:00:00+00:00".to_string(),
        last_activity: "2026-02-08T10:05:00+00:00".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        branch_name: String::new(), // empty — triggers fallback
        base_branch: "main".to_string(),
        chat_history: vec![ChatMessage {
            role: "user".to_string(),
            content: "DS".to_string(),
        }],
    };

    write_wal_to_dir(dir, &state).await;

    let runner = make_test_runner(dir);
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Expected Some(RecoveryInfo)");

    // story_info.branch_name should fall back to the `branch` field value
    assert_eq!(
        recovery.story_info.branch_name, "story/1-2-cli-legacy",
        "branch_name should fall back to legacy branch field"
    );
}

// ===========================================================================
// Task 10 — Forward-compatibility test (supplementary)
// ===========================================================================

#[tokio::test]
async fn test_wal_forward_compat_extra_fields_ignored() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let path = wal_path(dir);

    // Build valid YAML with extra unknown fields appended
    let state = make_valid_wal_state();
    let mut yaml = serde_yml::to_string(&state).expect("serialize");
    yaml.push_str("extra_field: \"unknown_value\"\n");
    yaml.push_str("another_future_field: 42\n");
    tokio::fs::write(&path, &yaml)
        .await
        .expect("write YAML with extra fields");

    let runner = make_test_runner(dir);
    let recovery = runner.check_and_recover_wal().await;

    // serde ignores unknown fields (no deny_unknown_fields) → recovery succeeds
    assert!(
        recovery.is_some(),
        "WAL with extra unknown fields should still recover successfully"
    );
    let recovery = recovery.unwrap();
    assert_eq!(recovery.state.story_key, "1-2-cli");
    assert_eq!(recovery.state.chat_history.len(), 4);
}
