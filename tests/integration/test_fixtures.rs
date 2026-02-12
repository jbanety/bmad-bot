//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures::*;

use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::{SprintStatusFile, StoryInfo};

use std::path::PathBuf;

// ---------------------------------------------------------------------------
// make_test_config
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_has_valid_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_uses_dir_for_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());

    assert_eq!(
        config.bmad_paths.implementation_artifacts,
        dir.path().display().to_string()
    );
}

#[test]
fn test_make_test_config_validates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());
    // Should not panic — config is valid
    config.validate().expect("config should be valid");
}

// ---------------------------------------------------------------------------
// make_test_secrets
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_all_present() {
    let secrets = make_test_secrets();

    assert!(secrets.anthropic_api_key.is_some());
    assert!(secrets.openai_api_key.is_some());
    assert!(secrets.github_copilot_oauth_token.is_some());
    assert!(secrets.github_token.is_some());
    assert!(secrets.gitlab_token.is_some());
    assert!(secrets.telegram_bot_token.is_some());
}

#[test]
fn test_make_test_secrets_contain_do_not_use_marker() {
    let secrets = make_test_secrets();
    assert!(secrets
        .anthropic_api_key
        .as_ref()
        .unwrap()
        .contains("DO-NOT-USE"));
    assert!(secrets
        .github_token
        .as_ref()
        .unwrap()
        .contains("DO-NOT-USE"));
}

// ---------------------------------------------------------------------------
// make_test_story
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key() {
    let story = make_test_story(
        "7-1-integration-test-infrastructure",
        "integration test infrastructure",
        vec![],
    );

    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test-infrastructure");
    assert_eq!(story.label, "integration test infrastructure");
    assert_eq!(
        story.branch_name,
        "story/7-1-integration-test-infrastructure"
    );
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["6-1-telegram".to_string(), "6-2-retry".to_string()];
    let story = make_test_story("7-2-config-tests", "config tests", deps);

    assert_eq!(story.dependencies.len(), 2);
    assert_eq!(story.dependencies[0], "6-1-telegram");
    assert_eq!(story.dependencies[1], "6-2-retry");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("1-2-cli-framework", "cli framework", vec![]);

    assert_eq!(
        story.specs_path,
        PathBuf::from("_bmad-output/implementation-artifacts/1-2-cli-framework.md")
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    assert!(path.exists());
}

#[test]
fn test_write_sprint_status_produces_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), &entries);

    // Use the real SprintStatusFile parser
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");
    let stories = sprint.stories();
    assert_eq!(stories.len(), 2); // Only stories, not epics or retros
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[1].story_key, "1-2-cli-framework");
}

#[test]
fn test_write_sprint_status_contains_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-slug", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    let content = std::fs::read_to_string(&path).expect("read");

    assert!(content.contains("epic-1: in-progress"));
    assert!(content.contains("1-1-slug: ready-for-dev"));
    assert!(content.contains("epic-1-retrospective: optional"));
}

#[test]
fn test_write_sprint_status_eligible_stories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("1-3-init-cmd", "backlog"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("parse");
    let eligible = sprint.eligible_stories();

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "1-2-cli-framework");
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_write_wal_file_creates_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("4-2-agent-session", "agent session", vec![]);
    let state = SessionState::new(&story, "anthropic", "claude-sonnet-4-20250514");

    let path = write_wal_file(dir.path(), &state);
    assert!(path.exists());
}

#[tokio::test]
async fn test_write_wal_file_produces_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("4-2-agent-session", "agent session", vec![]);
    let mut state = SessionState::new(&story, "anthropic", "claude-sonnet-4-20250514");
    state.chat_history.push(ChatMessage {
        role: "user".into(),
        content: "hello".into(),
    });
    state.chat_history.push(ChatMessage {
        role: "assistant".into(),
        content: "hi there".into(),
    });

    let path = write_wal_file(dir.path(), &state);

    // Load back using the real SessionState parser
    let loaded = SessionState::load(&path).await.expect("should load");
    assert_eq!(loaded.story_key, "4-2-agent-session");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[1].content, "hi there");
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_initializes_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    // Repository should be valid
    assert!(!repo.is_bare());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn test_create_test_repo_has_initial_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    // HEAD should resolve to a commit
    let head = repo.head().expect("HEAD should exist");
    let commit = head.peel_to_commit().expect("should resolve to commit");
    assert_eq!(commit.message().unwrap(), "initial commit");
}

#[test]
fn test_create_test_repo_has_head() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    assert!(!repo.head_detached().expect("should check HEAD"));
    assert!(repo.head().is_ok());
}
