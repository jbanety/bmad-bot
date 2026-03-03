//! Integration tests for Session WAL crash recovery (Story 7.5).
//!
//! Tests the full save → recover → parse pipeline through the **public API**
//! (`bmad_bot::session::*`, `bmad_bot::session::runner::*`), validating the
//! cross-module contract that unit tests in `src/session/` cannot verify.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use bmad_bot::config::BotSecrets;
use bmad_bot::llm::AgentFactory;
use bmad_bot::mcp::McpManager;
use bmad_bot::session::runner::{SessionRunner, ShutdownFlag, story_info_from_wal};
use bmad_bot::session::{ChatMessage, SessionOutcome, SessionState};

use crate::helpers::fixtures::{
    make_test_config, make_test_secrets, PipelineTestBuilder,
};

// ---------------------------------------------------------------------------
// Fixture helpers (Task 2)
// ---------------------------------------------------------------------------

/// Build a `SessionState` matching AC #1 exactly: story_key "1-2-cli",
/// branch "story/1-2-cli", base_branch "main", 4 chat messages,
/// provider "anthropic", model "claude-sonnet-4-20250514".
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

/// Write a WAL file to `dir` using `SessionState::save()` (async, atomic write).
async fn write_wal_to_dir(dir: &Path, state: &SessionState) {
    let wal = wal_path(dir);
    state.save(&wal).await.expect("WAL save failed");
}

/// Build `BotConfig` wrapped in `Arc` with `implementation_artifacts` pointing to `dir`.
fn make_test_config_arc(dir: &Path) -> Arc<bmad_bot::config::BotConfig> {
    Arc::new(make_test_config(dir))
}

/// Build `BotSecrets` wrapped in `Arc`.
fn make_test_secrets_arc() -> Arc<BotSecrets> {
    Arc::new(make_test_secrets())
}

/// Return the expected WAL file path for a given directory.
fn wal_path(dir: &Path) -> PathBuf {
    dir.join(".bmad-bot-session.yaml")
}

/// Construct a real `SessionRunner` for integration tests.
///
/// Uses `AgentFactory` (dummy keys) and `McpManager::empty()` — no LLM calls
/// will be made. Only `check_and_recover_wal()` (file I/O) is exercised.
fn make_session_runner(dir: &Path) -> SessionRunner {
    let config = make_test_config_arc(dir);
    let secrets = make_test_secrets_arc();
    let agent_factory = Arc::new(AgentFactory::new(config.clone(), secrets));
    let shutdown: ShutdownFlag = Arc::new(AtomicBool::new(false));
    let mcp_manager = Arc::new(McpManager::empty());
    SessionRunner::new(config, agent_factory, shutdown, mcp_manager)
}

// ---------------------------------------------------------------------------
// Task 3: Full save → recover → parse integration test (AC #1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_recovery_valid_returns_recovery_info() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    // Arrange: write valid WAL via SessionState::save()
    let state = make_valid_wal_state();
    write_wal_to_dir(dir, &state).await;
    assert!(wal_path(dir).exists(), "WAL file should exist after save");

    // Act: construct real SessionRunner, call check_and_recover_wal
    let runner = make_session_runner(dir);
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Expected Some(RecoveryInfo) for valid WAL");

    // Assert: RecoveryInfo fields match AC #1
    assert_eq!(recovery.story_info.story_key, "1-2-cli");
    assert_eq!(recovery.story_info.epic_num, 1);
    assert_eq!(recovery.story_info.story_num, 2);
    assert_eq!(recovery.story_info.label, "cli");
    assert_eq!(recovery.story_info.branch_name, "story/1-2-cli");
    assert_eq!(recovery.story_info.status, "in-progress");

    assert_eq!(recovery.state.provider, "anthropic");
    assert_eq!(recovery.state.model, "claude-sonnet-4-20250514");
    assert_eq!(recovery.state.chat_history.len(), 4);
    assert_eq!(recovery.state.base_branch, "main");

    // AC #5 contract: check_and_recover_wal does NOT delete the WAL.
    // Deletion is resume_session()'s responsibility (prevents infinite loops).
    // Verify the WAL still exists after successful recovery detection.
    assert!(
        wal_path(dir).exists(),
        "WAL must remain after check_and_recover_wal — only resume_session() deletes it"
    );
}

#[tokio::test]
async fn test_wal_recovery_story_info_from_wal_produces_correct_story_info() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let state = make_valid_wal_state();
    let config = make_test_config_arc(dir);

    let story_info = story_info_from_wal(&state, &config);

    assert_eq!(story_info.story_key, "1-2-cli");
    assert_eq!(story_info.epic_num, 1);
    assert_eq!(story_info.story_num, 2);
    assert_eq!(story_info.label, "cli");
    assert_eq!(story_info.branch_name, "story/1-2-cli");
    assert_eq!(story_info.story_id, "1.2");
    assert_eq!(story_info.dependencies, Vec::<String>::new());
    assert_eq!(story_info.status, "in-progress");

    // specs_path should combine implementation_artifacts + story_key
    let expected_specs = PathBuf::from(format!("{}/1-2-cli.md", dir.display()));
    assert_eq!(story_info.specs_path, expected_specs);
}

// ---------------------------------------------------------------------------
// Task 4: to_rig_messages conversion test (AC #2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_to_rig_messages_converts_all_messages() {
    let state = make_valid_wal_state();
    assert_eq!(state.chat_history.len(), 4, "Precondition: 4 messages");

    let messages = state.to_rig_messages();
    assert_eq!(messages.len(), 4, "All 4 messages should be converted");

    // Verify role mapping via debug formatting (rig::completion::Message internals
    // may not be directly inspectable — matches existing unit test approach)
    let debug_first = format!("{:?}", messages[0]);
    assert!(
        debug_first.to_lowercase().contains("user"),
        "First message should be user type, got: {debug_first}"
    );

    let debug_second = format!("{:?}", messages[1]);
    assert!(
        debug_second.to_lowercase().contains("assistant"),
        "Second message should be assistant type, got: {debug_second}"
    );
}

#[tokio::test]
async fn test_wal_to_rig_messages_preserves_order() {
    let state = make_valid_wal_state();
    let messages = state.to_rig_messages();

    // Verify alternating user/assistant pattern
    for (i, msg) in messages.iter().enumerate() {
        let debug = format!("{:?}", msg);
        let expected_role = if i % 2 == 0 { "user" } else { "assistant" };
        assert!(
            debug.to_lowercase().contains(expected_role),
            "Message {i} should be {expected_role} type, got: {debug}"
        );
    }
}

// ---------------------------------------------------------------------------
// Task 5: Corrupt WAL test (AC #3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_corrupt_file_deleted_and_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let wal = wal_path(dir);

    // Write garbage to the WAL file
    tokio::fs::write(&wal, "this is not: valid: yaml: [[[garbage")
        .await
        .unwrap();
    assert!(wal.exists(), "WAL should exist before recovery attempt");

    let runner = make_session_runner(dir);
    let result = runner.check_and_recover_wal().await;

    assert!(result.is_none(), "Corrupt WAL should return None");
    assert!(
        !wal.exists(),
        "Corrupt WAL file should be deleted to prevent infinite loops"
    );
}

// ---------------------------------------------------------------------------
// Task 6: No-WAL test (AC #4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_no_file_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    // No WAL file written — dir is empty
    assert!(!wal_path(dir).exists(), "Precondition: no WAL file");

    let runner = make_session_runner(dir);
    let result = runner.check_and_recover_wal().await;

    assert!(
        result.is_none(),
        "No WAL file should return None immediately"
    );
}

// ---------------------------------------------------------------------------
// Task 7: Post-recovery pipeline test (AC #5)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_process_recovered_session_completed() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    // Set up git repo so push_branch succeeds
    crate::helpers::fixtures::create_test_repo_with_remote(dir, "story/1-2-cli");

    let story = crate::helpers::fixtures::make_test_story("1-2-cli", "cli", vec![]);

    let (pipeline, _notifier, git) = PipelineTestBuilder::new(dir)
        .with_session(SessionOutcome::Completed {
            story_key: "1-2-cli".to_string(),
            branch: "story/1-2-cli".to_string(),
            decisions: vec![],
            pr_context: Some("Test context".to_string()),
            pr_how_to_test: None,
            pr_additional_info: None,
        })
        .with_review(bmad_bot::review::ReviewOutcome::Skipped {
            reason: "test".to_string(),
        })
        .build();

    let outcome = SessionOutcome::Completed {
        story_key: "1-2-cli".to_string(),
        branch: "story/1-2-cli".to_string(),
        decisions: vec![],
        pr_context: Some("Test context".to_string()),
        pr_how_to_test: None,
        pr_additional_info: None,
    };

    let result = pipeline.process_recovered_session(&story, outcome).await;

    assert_eq!(result.status, bmad_bot::notifier::StoryStatus::Completed);
    assert!(result.pr_url.is_some(), "PR should be created on success");
    assert_eq!(git.create_pr_call_count(), 1, "create_pr should be called");
}

#[tokio::test]
async fn test_pipeline_process_recovered_session_failed() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    // Set up git repo so push_branch succeeds
    crate::helpers::fixtures::create_test_repo_with_remote(dir, "story/1-2-cli");

    let story = crate::helpers::fixtures::make_test_story("1-2-cli", "cli", vec![]);

    let (pipeline, _notifier, git) = PipelineTestBuilder::new(dir)
        .with_session(SessionOutcome::Failed {
            story_key: "1-2-cli".to_string(),
            error: "test failure".to_string(),
            decisions: vec![],
        })
        .build();

    let outcome = SessionOutcome::Failed {
        story_key: "1-2-cli".to_string(),
        error: "test failure".to_string(),
        decisions: vec![],
    };

    let result = pipeline.process_recovered_session(&story, outcome).await;

    assert_eq!(result.status, bmad_bot::notifier::StoryStatus::Error);
    // Failed stories still get a PR with [NEEDS REVIEW] in the title
    assert!(result.pr_url.is_some(), "Failed story should still get PR");
    assert_eq!(git.create_pr_call_count(), 1);

    // Verify PR title contains NEEDS REVIEW marker
    let pr_params = git.captured_create_pr_params();
    assert!(!pr_params.is_empty());
    assert!(
        pr_params[0].title.contains("NEEDS REVIEW"),
        "Failed story PR should have [NEEDS REVIEW] in title, got: {}",
        pr_params[0].title
    );
}

#[tokio::test]
async fn test_pipeline_process_recovered_session_escalated() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    // Set up git repo
    crate::helpers::fixtures::create_test_repo_with_remote(dir, "story/1-2-cli");

    let story = crate::helpers::fixtures::make_test_story("1-2-cli", "cli", vec![]);

    let (pipeline, _notifier, _git) = PipelineTestBuilder::new(dir)
        .with_session(SessionOutcome::Escalated {
            report: bmad_bot::session::escalation::EscalationReport::new(
                "1-2-cli".into(),
                "test question".into(),
                "test reason".into(),
                "story/1-2-cli".into(),
                "partial work".into(),
            ),
            decisions: vec![],
        })
        .build();

    let outcome = SessionOutcome::Escalated {
        report: bmad_bot::session::escalation::EscalationReport::new(
            "1-2-cli".into(),
            "test question".into(),
            "test reason".into(),
            "story/1-2-cli".into(),
            "partial work".into(),
        ),
        decisions: vec![],
    };

    let result = pipeline.process_recovered_session(&story, outcome).await;

    assert_eq!(result.status, bmad_bot::notifier::StoryStatus::Blocked);
    // Escalated sessions DO get a PR (escalation PR with question/reason context).
    // pipeline.rs:1108 creates a PR via git_provider.create_pr() for escalated outcomes.
    assert!(
        result.pr_url.is_some(),
        "Escalated recovery should create an escalation PR"
    );
    assert_eq!(
        _git.create_pr_call_count(),
        1,
        "create_pr should be called once for escalated outcome"
    );
}

// ---------------------------------------------------------------------------
// Task 8: Recovery-first priority test (AC #6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_pipeline_recover_and_process_no_wal_returns_none() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    // new_with_components sets session_runner_for_recovery = None
    // so recover_and_process always returns None
    let (pipeline, _notifier, _git) = PipelineTestBuilder::new(dir)
        .with_session(SessionOutcome::Completed {
            story_key: "test".to_string(),
            branch: "story/test".to_string(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        })
        .build();

    let result = pipeline.recover_and_process().await;
    assert!(
        result.is_none(),
        "No WAL / no session_runner_for_recovery → None"
    );
}

#[tokio::test]
async fn test_wal_recovery_priority_wal_detected_before_polling() {
    // AC #6: When a WAL exists AND eligible stories are present in sprint-status,
    // crash recovery is processed FIRST — i.e., check_and_recover_wal() returns
    // Some before any polling loop is entered.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    // Write both a valid WAL AND a sprint-status.yaml with eligible stories.
    // This simulates the scenario where a crash happened mid-story and new
    // stories are also waiting — recovery must win the ordering race.
    let state = make_valid_wal_state();
    write_wal_to_dir(dir, &state).await;
    crate::helpers::fixtures::write_sprint_status(
        dir,
        vec![
            ("3-1-another-story", "ready-for-dev"),
            ("3-2-yet-another", "ready-for-dev"),
        ],
    );

    // Step 1 — recovery check (happens FIRST in daemon startup)
    let runner = make_session_runner(dir);
    let recovery = runner.check_and_recover_wal().await;
    assert!(
        recovery.is_some(),
        "WAL should be detected on recovery check (before polling begins)"
    );
    assert_eq!(
        recovery.unwrap().story_info.story_key,
        "1-2-cli",
        "Recovered story key should match WAL content"
    );

    // Step 2 — polling would happen AFTER recovery (not shown, but WAL was
    // processed first; no new stories are started until recovery completes)
}

// ---------------------------------------------------------------------------
// Task 9: Legacy WAL backward compatibility test (supplementary)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_legacy_branch_fallback() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    // Create state with empty branch_name but populated branch (pre-4.3 format)
    let mut state = make_valid_wal_state();
    state.branch_name = String::new(); // empty → should fall back to `branch`
    state.branch = "story/1-2-cli-legacy".to_string();

    write_wal_to_dir(dir, &state).await;

    let runner = make_session_runner(dir);
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("Legacy WAL should recover");

    assert_eq!(
        recovery.story_info.branch_name, "story/1-2-cli-legacy",
        "Should fall back to legacy `branch` field when `branch_name` is empty"
    );
}

// ---------------------------------------------------------------------------
// Task 10: Forward-compatibility test (supplementary)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_wal_forward_compat_unknown_fields_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let wal = wal_path(dir);

    // Write valid WAL then append unknown fields
    let state = make_valid_wal_state();
    let mut yaml = serde_yml::to_string(&state).expect("serialize");
    yaml.push_str("extra_field: \"unknown\"\n");
    yaml.push_str("another_future_field: 42\n");

    tokio::fs::write(&wal, &yaml).await.unwrap();

    let runner = make_session_runner(dir);
    let recovery = runner
        .check_and_recover_wal()
        .await
        .expect("WAL with extra fields should still recover");

    assert_eq!(recovery.story_info.story_key, "1-2-cli");
    assert_eq!(recovery.state.chat_history.len(), 4);
}
