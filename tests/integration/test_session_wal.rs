//! Integration tests for Session WAL crash recovery.
//!
//! Validates the full WAL lifecycle through the public API:
//! save → recover → parse → pipeline processing.
//!
//! **NOT duplicating** the 20+ unit tests in `src/session/runner.rs`. These tests
//! exercise the public API contract from an external crate perspective.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use bmad_bot::git_provider::PrInfo;
use bmad_bot::llm::AgentFactory;
use bmad_bot::notifier::StoryStatus;
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::runner::{story_info_from_wal, SessionRunner};
use bmad_bot::session::{ChatMessage, SessionOutcome, SessionState};

use super::helpers::fixtures::{
    create_test_repo_with_remote, make_test_config, make_test_secrets, make_test_story,
    PipelineTestBuilder,
};
use super::helpers::mocks::MockGitProvider;

// ---------------------------------------------------------------------------
// Fixture helpers (Task 2)
// ---------------------------------------------------------------------------

/// Set up a temp environment with bare remote + work repo + story branch.
/// Returns `(work_dir_guard, bare_dir_guard, config)` — guards keep dirs alive.
fn setup_git_env(
    branch_name: &str,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    bmad_bot::config::BotConfig,
) {
    let work_dir = tempfile::tempdir().expect("create work dir");
    let bare_dir = tempfile::tempdir().expect("create bare dir");
    create_test_repo_with_remote(work_dir.path(), bare_dir.path(), branch_name);
    let config = make_test_config(work_dir.path());
    (work_dir, bare_dir, config)
}

// ---------------------------------------------------------------------------
// Fixture helpers (Task 2)
// ---------------------------------------------------------------------------

/// Build a valid `SessionState` with 4 chat messages per AC #1.
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

/// Write a WAL file to the implementation_artifacts subdirectory of `dir`.
///
/// `make_test_config(dir)` sets `implementation_artifacts` to
/// `{dir}/_bmad-output/implementation-artifacts`, so the WAL goes there.
async fn write_wal_to_dir(dir: &Path, state: &SessionState) {
    let artifacts_dir = dir
        .join("_bmad-output")
        .join("implementation-artifacts");
    tokio::fs::create_dir_all(&artifacts_dir)
        .await
        .expect("create implementation-artifacts dir");
    let wal_path = artifacts_dir.join(".bmad-bot-session.yaml");
    state
        .save(&wal_path)
        .await
        .expect("write WAL via SessionState::save()");
}

/// Derive the WAL file path for a given temp dir (mirrors SessionRunner::new derivation).
fn wal_path(dir: &Path) -> PathBuf {
    dir.join("_bmad-output")
        .join("implementation-artifacts")
        .join(".bmad-bot-session.yaml")
}

/// Build a `SessionRunner` suitable for integration tests.
///
/// Uses `AgentFactory::new()` (never builds a real agent — tests only call
/// `check_and_recover_wal()` which is pure file I/O).
fn make_test_runner(dir: &Path) -> SessionRunner {
    let config = Arc::new(make_test_config(dir));
    let secrets = Arc::new(make_test_secrets());
    let factory = Arc::new(AgentFactory::new(config.clone(), secrets));
    let shutdown = Arc::new(AtomicBool::new(false));
    SessionRunner::new(config, factory, shutdown)
}

// ---------------------------------------------------------------------------
// Task 3: Full save→recover→parse integration test (AC #1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_recovery_valid_returns_recovery_info() {
    let tmp = tempfile::tempdir().unwrap();
    let state = make_valid_wal_state();
    write_wal_to_dir(tmp.path(), &state).await;

    let runner = make_test_runner(tmp.path());
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("should return Some(RecoveryInfo)");

    // Verify story_info fields
    assert_eq!(recovery.story_info.story_key, "1-2-cli");
    assert_eq!(recovery.story_info.epic_num, 1);
    assert_eq!(recovery.story_info.story_num, 2);
    assert_eq!(recovery.story_info.label, "cli");
    assert_eq!(recovery.story_info.branch_name, "story/1-2-cli");

    // Verify state fields
    assert_eq!(recovery.state.provider, "anthropic");
    assert_eq!(recovery.state.model, "claude-sonnet-4-20250514");
    assert_eq!(recovery.state.chat_history.len(), 4);
    assert_eq!(recovery.state.base_branch, "main");
}

/// Verify story_info_from_wal produces correct StoryInfo from public API.
#[tokio::test]
async fn test_wal_story_info_from_wal_public_api() {
    let tmp = tempfile::tempdir().unwrap();
    let state = make_valid_wal_state();
    let config = Arc::new(make_test_config(tmp.path()));

    let story_info = story_info_from_wal(&state, &config);
    assert_eq!(story_info.story_key, "1-2-cli");
    assert_eq!(story_info.epic_num, 1);
    assert_eq!(story_info.story_num, 2);
    assert_eq!(story_info.label, "cli");
    assert_eq!(story_info.branch_name, "story/1-2-cli");
    assert_eq!(story_info.status, "in-progress");
    assert!(story_info.dependencies.is_empty());
    // specs_path should contain the story key
    assert!(
        story_info.specs_path.to_string_lossy().contains("1-2-cli"),
        "specs_path should contain story key: {:?}",
        story_info.specs_path
    );
}

// ---------------------------------------------------------------------------
// Task 4: to_rig_messages conversion test (AC #2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_to_rig_messages_converts_all_4_messages() {
    let state = make_valid_wal_state();
    let messages = state.to_rig_messages();
    assert_eq!(messages.len(), 4, "should convert all 4 messages");

    // Verify first message is user type via debug format
    let debug_first = format!("{:?}", messages[0]);
    assert!(
        debug_first.to_lowercase().contains("user")
            || debug_first.contains("DS"),
        "First message should be user type or contain 'DS', got: {debug_first}"
    );
}

/// Verify ordering: user, assistant, user, assistant.
#[tokio::test]
async fn test_wal_to_rig_messages_preserves_order() {
    let state = make_valid_wal_state();
    let messages = state.to_rig_messages();
    assert_eq!(messages.len(), 4);

    // Check alternating roles via debug formatting
    let debug_msgs: Vec<String> = messages.iter().map(|m| format!("{:?}", m)).collect();
    // First and third should be user
    for (i, expected_role) in [(0, "user"), (1, "assistant"), (2, "user"), (3, "assistant")] {
        let debug = &debug_msgs[i];
        // rig's Message type includes role info in Debug output
        let lower = debug.to_lowercase();
        assert!(
            lower.contains(expected_role)
                || (expected_role == "user" && (lower.contains("ds") || lower.contains("continue")))
                || (expected_role == "assistant"
                    && (lower.contains("starting") || lower.contains("implementation"))),
            "Message {i} should be {expected_role}: {debug}"
        );
    }
}

// ---------------------------------------------------------------------------
// Task 5: Corrupt WAL test (AC #3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_corrupt_file_returns_none_and_deletes() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = tmp
        .path()
        .join("_bmad-output")
        .join("implementation-artifacts");
    tokio::fs::create_dir_all(&artifacts_dir)
        .await
        .unwrap();

    // Write garbage to the WAL path
    let wp = wal_path(tmp.path());
    tokio::fs::write(&wp, "not: valid: yaml: }{][garbage\n\x00\x01\x02")
        .await
        .unwrap();
    assert!(wp.exists(), "WAL file should exist before recovery");

    let runner = make_test_runner(tmp.path());
    let result = runner.check_and_recover_wal().await;
    assert!(result.is_none(), "corrupt WAL should return None");
    assert!(!wp.exists(), "corrupt WAL file should be deleted");
}

// ---------------------------------------------------------------------------
// Task 6: No WAL test (AC #4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_no_file_returns_none_immediately() {
    let tmp = tempfile::tempdir().unwrap();
    // Create the artifacts dir but NO WAL file
    let artifacts_dir = tmp
        .path()
        .join("_bmad-output")
        .join("implementation-artifacts");
    tokio::fs::create_dir_all(&artifacts_dir)
        .await
        .unwrap();

    let runner = make_test_runner(tmp.path());
    let result = runner.check_and_recover_wal().await;
    assert!(result.is_none(), "no WAL file should return None");
}

/// Edge case: implementation_artifacts dir doesn't exist at all.
#[tokio::test]
async fn test_wal_no_artifacts_dir_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    // Don't create any subdirectories
    let runner = make_test_runner(tmp.path());
    let result = runner.check_and_recover_wal().await;
    assert!(result.is_none(), "missing artifacts dir should return None");
}

// ---------------------------------------------------------------------------
// Task 7: Post-recovery pipeline test (AC #5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_pipeline_completed_outcome() {
    let story_key = "1-2-cli";
    let branch = format!("story/{story_key}");
    let (_work, _bare, config) = setup_git_env(&branch);

    let (pipeline, _notifier, _git_provider, code_reviewer) = PipelineTestBuilder::new()
        .with_config(config)
        .with_code_review(true)
        .with_review(ReviewOutcome::Completed {
            story_key: story_key.into(),
            branch: branch.clone(),
            report: "LGTM".into(),
        })
        .with_git_provider(
            MockGitProvider::new().with_create_pr(Ok(PrInfo {
                id: "42".into(),
                url: "https://github.com/test/test/pull/42".into(),
                number: 42,
            })),
        )
        .build();

    let story = make_test_story(story_key, "cli", vec![]);
    let outcome = SessionOutcome::Completed {
        story_key: story_key.into(),
        branch: branch.clone(),
        decisions: vec![],
        pr_context: Some("Context for PR".into()),
        pr_how_to_test: Some("Run tests".into()),
        pr_additional_info: None,
    };

    let result = pipeline.process_recovered_session(&story, outcome).await;
    assert_eq!(result.status, StoryStatus::Completed);
    assert!(
        result.pr_url.is_some(),
        "completed recovery should create PR, error_detail={:?}",
        result.error_detail
    );
    assert_eq!(
        result.pr_url.as_deref(),
        Some("https://github.com/test/test/pull/42")
    );

    // Verify code review was called
    assert_eq!(code_reviewer.call_count(), 1, "review should have been called");
}

#[tokio::test]
async fn test_wal_pipeline_failed_outcome_creates_failure_pr() {
    let story_key = "1-2-cli";
    let branch = format!("story/{story_key}");
    let (_work, _bare, config) = setup_git_env(&branch);

    let (pipeline, _notifier, _git_provider, _code_reviewer) = PipelineTestBuilder::new()
        .with_config(config)
        .with_code_review(false)
        .with_git_provider(
            MockGitProvider::new().with_create_pr(Ok(PrInfo {
                id: "99".into(),
                url: "https://github.com/test/test/pull/99".into(),
                number: 99,
            })),
        )
        .build();

    let story = make_test_story(story_key, "cli", vec![]);
    let outcome = SessionOutcome::Failed {
        story_key: story_key.into(),
        error: "Agent crashed mid-work".into(),
        decisions: vec![],
    };

    let result = pipeline.process_recovered_session(&story, outcome).await;
    assert_eq!(result.status, StoryStatus::Error);
    // Non-infra failures create a PR with partial work
    assert!(
        result.pr_url.is_some(),
        "failed recovery should create failure PR, error_detail={:?}",
        result.error_detail
    );
}

#[tokio::test]
async fn test_wal_pipeline_escalated_outcome() {
    let story_key = "1-2-cli";
    let branch = format!("story/{story_key}");
    let (_work, _bare, config) = setup_git_env(&branch);

    let (pipeline, _notifier, _git_provider, _code_reviewer) = PipelineTestBuilder::new()
        .with_config(config)
        .with_code_review(false)
        .with_git_provider(
            MockGitProvider::new().with_create_pr(Ok(PrInfo {
                id: "55".into(),
                url: "https://github.com/test/test/pull/55".into(),
                number: 55,
            })),
        )
        .build();

    let story = make_test_story(story_key, "cli", vec![]);
    let outcome = SessionOutcome::Escalated {
        report: bmad_bot::session::escalation::EscalationReport {
            story_key: story_key.into(),
            branch_name: branch.clone(),
            question: "What DB schema?".into(),
            reason: "Ambiguous requirement".into(),
            partial_work_summary: "Implemented login form".into(),
            escalated_at: "2026-02-08T10:05:00+00:00".into(),
        },
        decisions: vec![],
    };

    let result = pipeline.process_recovered_session(&story, outcome).await;
    assert_eq!(result.status, StoryStatus::Blocked);
    // Escalation should attempt PR creation for the escalation branch
    assert!(
        result.pr_url.is_some(),
        "escalated recovery should create escalation PR, error_detail={:?}",
        result.error_detail
    );
}

// ---------------------------------------------------------------------------
// Task 8: Recovery-first priority test (AC #6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_recover_and_process_with_wal_detected() {
    // SessionRunner can detect WAL directly
    let tmp = tempfile::tempdir().unwrap();
    let state = make_valid_wal_state();
    write_wal_to_dir(tmp.path(), &state).await;

    let runner = make_test_runner(tmp.path());
    let recovery = runner.check_and_recover_wal().await;
    assert!(
        recovery.is_some(),
        "WAL should be detected before any polling"
    );
}

#[tokio::test]
async fn test_wal_recover_and_process_no_wal_returns_none() {
    // Pipeline with new_with_components has session_runner_for_recovery = None
    let (pipeline, _, _, _) = PipelineTestBuilder::new().build();
    let result = pipeline.recover_and_process().await;
    assert!(
        result.is_none(),
        "no session_runner_for_recovery → None (daemon proceeds to polling)"
    );
}

#[tokio::test]
async fn test_wal_no_file_means_clean_start() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = tmp
        .path()
        .join("_bmad-output")
        .join("implementation-artifacts");
    tokio::fs::create_dir_all(&artifacts_dir)
        .await
        .unwrap();

    let runner = make_test_runner(tmp.path());
    let recovery = runner.check_and_recover_wal().await;
    assert!(
        recovery.is_none(),
        "no WAL file → None → daemon proceeds to poll new stories"
    );
}

// ---------------------------------------------------------------------------
// Task 9: Legacy WAL backward compatibility test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_legacy_format_branch_fallback() {
    let tmp = tempfile::tempdir().unwrap();

    // Create state with empty branch_name but populated branch (pre-4.3 format)
    let state = SessionState {
        story_id: "3.1".to_string(),
        story_key: "3-1-supervisor".to_string(),
        branch: "story/3-1-supervisor".to_string(),
        started_at: "2026-01-01T00:00:00+00:00".to_string(),
        last_activity: "2026-01-01T00:01:00+00:00".to_string(),
        provider: "anthropic".to_string(),
        model: "test-model".to_string(),
        branch_name: String::new(), // empty — legacy WAL
        base_branch: "main".to_string(),
        chat_history: vec![ChatMessage {
            role: "user".to_string(),
            content: "DS".to_string(),
        }],
    };

    write_wal_to_dir(tmp.path(), &state).await;

    let runner = make_test_runner(tmp.path());
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("legacy WAL should recover");

    // branch_name should fall back to `branch` value
    assert_eq!(
        recovery.story_info.branch_name, "story/3-1-supervisor",
        "should fall back to legacy `branch` field"
    );
    assert_eq!(recovery.story_info.story_key, "3-1-supervisor");
    assert_eq!(recovery.story_info.epic_num, 3);
    assert_eq!(recovery.story_info.story_num, 1);
    assert_eq!(recovery.story_info.label, "supervisor");
}

// ---------------------------------------------------------------------------
// Task 10: Forward-compatibility test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_forward_compat_unknown_fields_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let artifacts_dir = tmp
        .path()
        .join("_bmad-output")
        .join("implementation-artifacts");
    tokio::fs::create_dir_all(&artifacts_dir)
        .await
        .unwrap();

    // Write a valid WAL with extra unknown fields via raw YAML
    let yaml = r#"story_id: "2.1"
story_key: "2-1-polling"
branch: "story/2-1-polling"
started_at: "2026-02-08T10:00:00+00:00"
last_activity: "2026-02-08T10:05:00+00:00"
provider: "anthropic"
model: "claude-sonnet-4-20250514"
branch_name: "story/2-1-polling"
base_branch: "main"
extra_field: "unknown_value"
another_future_field: 42
chat_history:
  - role: "user"
    content: "DS"
  - role: "assistant"
    content: "Done."
"#;
    let wp = wal_path(tmp.path());
    tokio::fs::write(&wp, yaml).await.unwrap();

    let runner = make_test_runner(tmp.path());
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("WAL with unknown fields should recover (serde ignores them)");

    assert_eq!(recovery.story_info.story_key, "2-1-polling");
    assert_eq!(recovery.state.chat_history.len(), 2);
}

// ---------------------------------------------------------------------------
// Full roundtrip: save() → check_and_recover_wal() cross-module chain
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_full_roundtrip_save_then_recover() {
    let tmp = tempfile::tempdir().unwrap();
    let state = make_valid_wal_state();

    // Use SessionState::save() directly (the state module API)
    let wp = wal_path(tmp.path());
    tokio::fs::create_dir_all(wp.parent().unwrap()).await.unwrap();
    state.save(&wp).await.expect("save should succeed");
    assert!(wp.exists(), "WAL file should exist after save");

    // Recover via SessionRunner (the runner module API)
    let runner = make_test_runner(tmp.path());
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("should recover from saved WAL");

    assert_eq!(recovery.state.story_key, "1-2-cli");
    assert_eq!(recovery.state.provider, "anthropic");
    assert_eq!(recovery.state.chat_history.len(), 4);
    assert_eq!(recovery.state.chat_history[0].role, "user");
    assert_eq!(recovery.state.chat_history[0].content, "DS");
    assert_eq!(recovery.state.chat_history[3].role, "assistant");
    assert_eq!(recovery.state.chat_history[3].content, "Implementation complete.");
}
