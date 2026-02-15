//! Integration tests for Session WAL (Write-Ahead Log) crash recovery.
//!
//! Tests the full save→recover→parse chain through the public API surface:
//! `SessionState::save()` → `SessionRunner::check_and_recover_wal()` →
//! `story_info_from_wal()` → `SessionState::to_rig_messages()`.
//!
//! Unlike the 20+ unit tests in `src/session/runner.rs` which use `pub(crate)`
//! helpers, these tests construct all types via public APIs — exactly as an
//! external crate would, validating the library contract.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use bmad_bot::config::BotConfig;
use bmad_bot::git_provider::PrInfo;
use bmad_bot::llm::AgentFactory;
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification, StoryStatus};
use bmad_bot::pipeline::StoryPipeline;
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::runner::{story_info_from_wal, SessionRunner};
use bmad_bot::session::{ChatMessage, SessionState, SessionOutcome};
use bmad_bot::watcher::StoryInfo;
use crate::helpers::fixtures::{make_test_config, make_test_secrets, write_wal_file};
use crate::helpers::mocks::MockGitProvider;

// ---------------------------------------------------------------------------
// Fixture helpers (local to WAL tests)
// ---------------------------------------------------------------------------

/// Build a valid `SessionState` for WAL recovery tests.
///
/// Matches AC #1: story_key "1-2-cli", branch "story/1-2-cli", base_branch "main",
/// 4 chat messages (2 user, 2 assistant), provider "anthropic", model "claude-sonnet-4-20250514".
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

/// Derive the expected WAL file path for a given directory.
///
/// Mirrors `SessionRunner::new()` derivation:
/// `Path::new(&config.bmad_paths.implementation_artifacts).join(".bmad-bot-session.yaml")`
fn wal_path(dir: &Path) -> PathBuf {
    dir.join(".bmad-bot-session.yaml")
}

/// Build a `SessionRunner` from a config and test secrets.
///
/// Constructs `AgentFactory` + shutdown flag internally.
fn make_test_runner(config: Arc<BotConfig>) -> SessionRunner {
    let secrets = Arc::new(make_test_secrets());
    let factory = Arc::new(AgentFactory::new(Arc::clone(&config), secrets));
    let shutdown = Arc::new(AtomicBool::new(false));
    SessionRunner::new(config, factory, shutdown)
}

/// Create the implementation-artifacts directory from a config and return its path.
///
/// `make_test_config(dir)` sets `implementation_artifacts` to `{dir}/implementation-artifacts`.
/// This helper creates that subdirectory so WAL files can be written there.
fn ensure_artifacts_dir(config: &BotConfig) -> PathBuf {
    let artifacts_dir = PathBuf::from(&config.bmad_paths.implementation_artifacts);
    std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    artifacts_dir
}

/// Build a recovery runner for pipeline tests.
fn make_recovery_runner(config: Arc<BotConfig>) -> SessionRunner {
    let secrets = Arc::new(make_test_secrets());
    let factory = Arc::new(AgentFactory::new(Arc::clone(&config), secrets));
    let shutdown = Arc::new(AtomicBool::new(false));
    SessionRunner::new(config, factory, shutdown)
}

#[derive(Clone)]
struct CaptureNotifier {
    calls: Arc<std::sync::Mutex<Vec<StoryNotification>>>,
}

impl CaptureNotifier {
    fn new() -> Self {
        Self {
            calls: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn story_calls(&self) -> Vec<StoryNotification> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Notifier for CaptureNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        self.calls.lock().unwrap().push(notification.clone());
        Ok(())
    }

    async fn notify_run_summary(&self, _summary: &RunSummary) -> Result<(), NotifierError> {
        Ok(())
    }
}

struct StaticDevRunner {
    outcome: std::sync::Mutex<Option<SessionOutcome>>,
    calls: AtomicUsize,
}

impl StaticDevRunner {
    fn new(outcome: SessionOutcome) -> Self {
        Self {
            outcome: std::sync::Mutex::new(Some(outcome)),
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl bmad_bot::pipeline::DevRunner for StaticDevRunner {
    async fn run_dev_session(&self, _story: &StoryInfo) -> SessionOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.outcome
            .lock()
            .unwrap()
            .take()
            .expect("StaticDevRunner outcome already consumed")
    }
}

struct CaptureReviewer {
    outcome: std::sync::Mutex<Option<ReviewOutcome>>,
}

impl CaptureReviewer {
    fn new(outcome: ReviewOutcome) -> Self {
        Self {
            outcome: std::sync::Mutex::new(Some(outcome)),
        }
    }
}

#[async_trait::async_trait]
impl bmad_bot::pipeline::CodeReviewer for CaptureReviewer {
    async fn run_review(&self, _story: &StoryInfo) -> ReviewOutcome {
        self.outcome
            .lock()
            .unwrap()
            .take()
            .expect("CaptureReviewer outcome already consumed")
    }
}

// ===========================================================================
// Task 3: Full save→recover→parse integration test (AC #1)
// ===========================================================================

#[tokio::test]
async fn test_wal_recovery_valid_returns_recovery_info() {
    // Arrange: create temp dir, write valid WAL file
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let artifacts_dir = ensure_artifacts_dir(&config);
    let state = make_valid_wal_state();
    write_wal_file(&artifacts_dir, &state).await;

    // Act: construct runner, check for WAL
    let config = Arc::new(config);
    let runner = make_test_runner(Arc::clone(&config));
    let result = runner.check_and_recover_wal().await;

    // Assert: Some(RecoveryInfo) with correct fields
    assert!(result.is_some(), "Should detect valid WAL file");
    let recovery = result.unwrap();

    // Verify state fields (AC #1)
    assert_eq!(recovery.state.story_key, "1-2-cli");
    assert_eq!(recovery.state.provider, "anthropic");
    assert_eq!(recovery.state.model, "claude-sonnet-4-20250514");
    assert_eq!(recovery.state.branch_name, "story/1-2-cli");
    assert_eq!(recovery.state.base_branch, "main");
    assert_eq!(recovery.state.chat_history.len(), 4);

    // Verify story_info fields (from story_info_from_wal)
    assert_eq!(recovery.story_info.story_key, "1-2-cli");
    assert_eq!(recovery.story_info.epic_num, 1);
    assert_eq!(recovery.story_info.story_num, 2);
    assert_eq!(recovery.story_info.label, "cli");
    assert_eq!(recovery.story_info.branch_name, "story/1-2-cli");
    assert_eq!(recovery.story_info.status, "in-progress");
    assert!(recovery.story_info.dependencies.is_empty());
}

#[tokio::test]
async fn test_wal_recovery_story_info_specs_path() {
    // Verify that story_info_from_wal constructs the correct specs_path
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let state = make_valid_wal_state();

    let story_info = story_info_from_wal(&state, &config);
    let expected_specs = format!("{}/1-2-cli.md", config.bmad_paths.implementation_artifacts);
    assert_eq!(story_info.specs_path, PathBuf::from(expected_specs));
}

// ===========================================================================
// Task 4: to_rig_messages conversion test (AC #2)
// ===========================================================================

#[test]
fn test_wal_to_rig_messages_count_and_order() {
    let state = make_valid_wal_state();
    let messages = state.to_rig_messages();

    // Assert length (AC #2: all 4 messages)
    assert_eq!(messages.len(), 4, "Should produce 4 rig messages");

    // Verify ordering via debug format inspection (rig Message internals
    // may not expose role directly — debug format is the reliable way)
    let debug_first = format!("{:?}", messages[0]);
    assert!(
        debug_first.to_lowercase().contains("user") || debug_first.contains("DS"),
        "First message should be user type or contain 'DS', got: {debug_first}"
    );

    let debug_second = format!("{:?}", messages[1]);
    assert!(
        debug_second.to_lowercase().contains("assistant")
            || debug_second.contains("Starting story"),
        "Second message should be assistant type, got: {debug_second}"
    );

    let debug_third = format!("{:?}", messages[2]);
    assert!(
        debug_third.to_lowercase().contains("user") || debug_third.contains("Continue"),
        "Third message should be user type or contain 'Continue.', got: {debug_third}"
    );

    let debug_fourth = format!("{:?}", messages[3]);
    assert!(
        debug_fourth.to_lowercase().contains("assistant")
            || debug_fourth.contains("Implementation complete"),
        "Fourth message should be assistant type, got: {debug_fourth}"
    );
}

// ===========================================================================
// Task 5: Corrupt WAL test (AC #3)
// ===========================================================================

#[tokio::test]
async fn test_wal_corrupt_returns_none_and_deletes_file() {
    // Arrange: write raw garbage to WAL path
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let artifacts_dir = ensure_artifacts_dir(&config);
    let wal = wal_path(&artifacts_dir);
    tokio::fs::write(&wal, "not: [valid: yaml: for: session")
        .await
        .expect("write corrupt WAL");
    assert!(wal.exists(), "Corrupt WAL should exist before check");

    // Act
    let config = Arc::new(config);
    let runner = make_test_runner(config);
    let result = runner.check_and_recover_wal().await;

    // Assert: None returned AND file deleted (AC #3)
    assert!(result.is_none(), "Should return None for corrupt WAL");
    assert!(
        !wal.exists(),
        "Corrupt WAL file should be deleted after check"
    );
}

// ===========================================================================
// Task 6: No WAL test (AC #4)
// ===========================================================================

#[tokio::test]
async fn test_wal_no_file_returns_none() {
    // Arrange: empty temp dir (no WAL file)
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let artifacts_dir = ensure_artifacts_dir(&config);
    assert!(
        !wal_path(&artifacts_dir).exists(),
        "No WAL should exist initially"
    );

    // Act
    let config = Arc::new(config);
    let runner = make_test_runner(config);
    let result = runner.check_and_recover_wal().await;

    // Assert: None returned immediately (AC #4)
    assert!(
        result.is_none(),
        "Should return None when no WAL file exists"
    );
}

// ===========================================================================
// Task 7: Post-recovery pipeline test (AC #5)
// ===========================================================================

// AC #5: Exercise recover_and_process with a real SessionRunner and mocked pipeline dependencies.

#[tokio::test]
async fn test_wal_recover_and_process_executes_pipeline_and_deletes_wal() {
    // AC #5: When WAL exists, recover_and_process executes the pipeline and deletes WAL.
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let artifacts_dir = ensure_artifacts_dir(&config);
    let state = make_valid_wal_state();
    write_wal_file(&artifacts_dir, &state).await;

    let config = Arc::new(config);
    let runner = make_recovery_runner(Arc::clone(&config));

    let outcome_story_key = "1-2-cli".to_string();
    let result_story_key = outcome_story_key.clone();

    let outcome = SessionOutcome::Failed {
        story_key: outcome_story_key,
        error: "Recovery failure for testing".into(),
        decisions: vec![],
    };

    let dev_runner = StaticDevRunner::new(outcome);

    let mock_git = MockGitProvider::new().with_create_pr(Ok(PrInfo {
        id: "42".to_string(),
        url: "https://github.com/test/test/pull/42".to_string(),
        number: 42,
    }));

    let notifier = CaptureNotifier::new();
    let notifier_handle = notifier.clone();
    let git_handle = mock_git.clone();

    let pipeline = StoryPipeline::new_with_components(
        Arc::clone(&config),
        Box::new(mock_git),
        Box::new(notifier),
        Box::new(dev_runner),
        Box::new(CaptureReviewer::new(ReviewOutcome::Skipped {
            reason: "skip".into(),
        })),
        Some(runner),
    );

    let recovery_result = pipeline.recover_and_process().await;
    assert!(recovery_result.is_some(), "Recovery should be processed");
    let result = recovery_result.unwrap();
    assert_eq!(result.status, StoryStatus::Error);
    assert_eq!(result.story_key, result_story_key);
    assert_eq!(result.pr_url, Some("https://github.com/test/test/pull/42".to_string()));

    let pr_params = git_handle.captured_create_pr_params();
    assert_eq!(pr_params.len(), 1);
    assert!(
        pr_params[0].title.contains("[NEEDS REVIEW]"),
        "Expected failure PR title, got: {}",
        pr_params[0].title
    );

    let story_notifications = notifier_handle.story_calls();
    assert_eq!(story_notifications.len(), 1);
    assert_eq!(story_notifications[0].status, StoryStatus::Error);
    assert_eq!(story_notifications[0].story_key, result_story_key);
    assert!(story_notifications[0].pr_url.is_some());

    assert!(
        !wal_path(&artifacts_dir).exists(),
        "WAL file should be deleted after recovery processing"
    );
}

#[tokio::test]
async fn test_wal_recovery_with_real_runner_detects_wal() {
    // Validate that a real SessionRunner detects WAL files correctly
    // through the check_and_recover_wal() public API.
    // This proves the recovery→pipeline boundary: if check_and_recover_wal()
    // returns Some, the pipeline would proceed with resume_session().
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let artifacts_dir = ensure_artifacts_dir(&config);
    let state = make_valid_wal_state();
    write_wal_file(&artifacts_dir, &state).await;

    let config = Arc::new(config);
    let runner = make_test_runner(Arc::clone(&config));

    // First call: WAL exists → Some
    let recovery = runner.check_and_recover_wal().await;
    assert!(recovery.is_some(), "WAL should be detected");
    let info = recovery.unwrap();
    assert_eq!(info.story_info.story_key, "1-2-cli");
}

// ===========================================================================
// Task 8: Recovery-first priority test (AC #6)
// ===========================================================================

#[tokio::test]
async fn test_wal_recovery_priority_wal_present() {
    // AC #6: When WAL exists, recovery is processed FIRST before polling.
    // Validate via SessionRunner: check_and_recover_wal() returns Some.
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let artifacts_dir = ensure_artifacts_dir(&config);
    write_wal_file(&artifacts_dir, &make_valid_wal_state()).await;

    let config = Arc::new(config);
    let runner = make_test_runner(config);

    let result = runner.check_and_recover_wal().await;
    assert!(
        result.is_some(),
        "WAL present → recovery should be detected before polling"
    );
}

#[tokio::test]
async fn test_wal_recovery_priority_no_wal() {
    // AC #6 inverse: No WAL → recovery returns None → daemon proceeds to polling.
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let _artifacts_dir = ensure_artifacts_dir(&config);

    let config = Arc::new(config);
    let runner = make_test_runner(config);

    let result = runner.check_and_recover_wal().await;
    assert!(
        result.is_none(),
        "No WAL → daemon should proceed to polling"
    );
}

#[tokio::test]
async fn test_wal_pipeline_recover_and_process_no_wal() {
    // Pipeline-level: recover_and_process() returns None when no WAL is present
    // (confirms daemon proceeds to polling when there is no recovery work).
    use bmad_bot::session::SessionOutcome;

    use crate::helpers::fixtures::PipelineTestBuilder;

    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let _artifacts_dir = ensure_artifacts_dir(&config);

    let (pipeline, _notifier, _git) = PipelineTestBuilder::new()
        .with_session(SessionOutcome::Completed {
            story_key: "test".into(),
            branch: "story/test".into(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        })
        .build_with_config(config);

    let result = pipeline.recover_and_process().await;
    assert!(result.is_none(), "No WAL → None → proceed to poll");
}

// ===========================================================================
// Task 9: Legacy WAL backward compatibility test (supplementary)
// ===========================================================================

#[tokio::test]
async fn test_wal_legacy_branch_fallback() {
    // Pre-4.3 WAL: branch_name is empty, but `branch` field is populated.
    // story_info_from_wal should fall back to `branch` value.
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let artifacts_dir = ensure_artifacts_dir(&config);

    let state = SessionState {
        story_id: "4.2".to_string(),
        story_key: "4-2-agent-session-setup-chat-loop".to_string(),
        branch: "story/4-2-agent-session-setup-chat-loop".to_string(),
        started_at: "2026-01-15T08:00:00+00:00".to_string(),
        last_activity: "2026-01-15T08:30:00+00:00".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        branch_name: String::new(), // empty — legacy format
        base_branch: String::new(), // empty — legacy format
        chat_history: vec![ChatMessage {
            role: "user".to_string(),
            content: "DS".to_string(),
        }],
    };
    write_wal_file(&artifacts_dir, &state).await;

    let config = Arc::new(config);
    let runner = make_test_runner(Arc::clone(&config));
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Legacy WAL should be recoverable");

    // branch_name in state is empty (legacy)
    assert!(
        recovery.state.branch_name.is_empty(),
        "Legacy WAL should have empty branch_name"
    );

    // story_info should fall back to state.branch
    assert_eq!(
        recovery.story_info.branch_name,
        "story/4-2-agent-session-setup-chat-loop",
        "story_info should use legacy `branch` field as fallback"
    );
    assert_eq!(recovery.story_info.epic_num, 4);
    assert_eq!(recovery.story_info.story_num, 2);
    assert_eq!(
        recovery.story_info.label,
        "agent-session-setup-chat-loop"
    );
}

// ===========================================================================
// Task 10: Forward-compatibility test (supplementary)
// ===========================================================================

#[tokio::test]
async fn test_wal_forward_compat_unknown_fields() {
    // WAL with extra unknown YAML fields should still parse
    // (serde deserializes without #[serde(deny_unknown_fields)])
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let artifacts_dir = ensure_artifacts_dir(&config);

    // Write valid WAL first, then append unknown fields via raw YAML
    let state = make_valid_wal_state();
    let mut yaml = serde_yml::to_string(&state).expect("serialize");
    yaml.push_str("extra_field: \"unknown_value\"\n");
    yaml.push_str("another_future_field: 42\n");

    let wal = wal_path(&artifacts_dir);
    tokio::fs::write(&wal, &yaml)
        .await
        .expect("write augmented WAL");

    let config = Arc::new(config);
    let runner = make_test_runner(config);
    let result = runner.check_and_recover_wal().await;

    assert!(
        result.is_some(),
        "WAL with unknown fields should still parse (forward-compatible)"
    );
    let recovery = result.unwrap();
    assert_eq!(recovery.state.story_key, "1-2-cli");
    assert_eq!(recovery.state.chat_history.len(), 4);
}
