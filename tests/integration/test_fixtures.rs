//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures::*;

use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert!(!config.bmad_paths.project_root.is_empty());
}

#[test]
fn test_make_test_config_passes_validation() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    // BotConfig::validate() checks provider names, non-empty fields, etc.
    assert!(config.validate().is_ok());
}

#[test]
fn test_make_test_config_paths_use_temp_dir() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = make_test_config(dir.path());
    let dir_str = dir.path().display().to_string();

    assert!(config.bmad_paths.project_root.starts_with(&dir_str));
    assert!(config.bmad_paths.output_folder.starts_with(&dir_str));
    assert!(config
        .bmad_paths
        .planning_artifacts
        .starts_with(&dir_str));
    assert!(config
        .bmad_paths
        .implementation_artifacts
        .starts_with(&dir_str));
}

// ---------------------------------------------------------------------------
// make_test_secrets tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_has_all_tokens() {
    let secrets = make_test_secrets();

    assert!(secrets.anthropic_api_key.is_some());
    assert!(secrets.openai_api_key.is_some());
    assert!(secrets.github_copilot_oauth_token.is_some());
    assert!(secrets.github_token.is_some());
    assert!(secrets.gitlab_token.is_some());
    assert!(secrets.telegram_bot_token.is_some());
}

#[test]
fn test_make_test_secrets_contains_do_not_use_marker() {
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
// make_test_story tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key_correctly() {
    let story = make_test_story("7-1-integration-test", "Integration Test", vec![]);

    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "Integration Test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
    assert_eq!(
        story.specs_path,
        std::path::PathBuf::from(
            "_bmad-output/implementation-artifacts/7-1-integration-test.md"
        )
    );
    assert!(story.dependencies.is_empty());
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_with_dependencies() {
    let story = make_test_story(
        "7-2-config-tests",
        "Config Tests",
        vec!["7-1-integration-test".into()],
    );

    assert_eq!(story.dependencies.len(), 1);
    assert_eq!(story.dependencies[0], "7-1-integration-test");
}

#[test]
fn test_make_test_story_multi_digit_epic() {
    let story = make_test_story("10-3-large-epic-story", "Large Epic", vec![]);
    assert_eq!(story.epic_num, 10);
    assert_eq!(story.story_num, 3);
    assert_eq!(story.story_id, "10.3");
}

// ---------------------------------------------------------------------------
// write_sprint_status tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = write_sprint_status(
        dir.path(),
        vec![
            ("epic-1", "in-progress"),
            ("1-1-story-slug", "ready-for-dev"),
            ("1-2-another-story", "backlog"),
            ("epic-1-retrospective", "optional"),
        ],
    );

    assert!(path.exists());

    // Parse with the real SprintStatusFile::load
    let result = SprintStatusFile::load(&path, dir.path());
    assert!(result.is_ok(), "SprintStatusFile::load failed: {result:?}");

    let sprint = result.unwrap();
    let stories = sprint.stories();
    assert_eq!(stories.len(), 2, "Should have 2 stories (epics/retros filtered)");
    assert_eq!(stories[0].story_key, "1-1-story-slug");
    assert_eq!(stories[1].story_key, "1-2-another-story");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = write_sprint_status(
        dir.path(),
        vec![
            ("epic-1", "in-progress"),
            ("1-1-slug", "done"),
            ("epic-1-retrospective", "optional"),
        ],
    );

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("epic-1: in-progress"));
    assert!(content.contains("1-1-slug: done"));
    assert!(content.contains("epic-1-retrospective: optional"));
}

#[test]
fn test_write_sprint_status_eligible_stories() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = write_sprint_status(
        dir.path(),
        vec![
            ("epic-1", "in-progress"),
            ("1-1-done-story", "done"),
            ("1-2-ready-story", "ready-for-dev"),
            ("1-3-blocked-story", "blocked"),
        ],
    );

    let sprint = SprintStatusFile::load(&path, dir.path()).unwrap();
    let eligible = sprint.eligible_stories();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "1-2-ready-story");
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let state = SessionState {
        story_id: "7.1".into(),
        story_key: "7-1-test".into(),
        branch: "story/7-1-test".into(),
        started_at: "2026-02-08T10:00:00Z".into(),
        last_activity: "2026-02-08T10:05:00Z".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-4-20250514".into(),
        branch_name: "story/7-1-test".into(),
        base_branch: "main".into(),
        chat_history: vec![
            ChatMessage {
                role: "user".into(),
                content: "hello".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "hi there".into(),
            },
        ],
    };

    let path = write_wal_file(dir.path(), &state);
    assert!(path.exists());

    // Parse back
    let content = std::fs::read_to_string(&path).unwrap();
    let loaded: SessionState = serde_yml::from_str(&content).expect("WAL YAML should parse");

    assert_eq!(loaded.story_id, "7.1");
    assert_eq!(loaded.story_key, "7-1-test");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[1].content, "hi there");
}

#[test]
fn test_write_wal_file_empty_chat_history() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let state = SessionState {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        last_activity: "2026-01-01T00:00:00Z".into(),
        provider: "openai".into(),
        model: "gpt-4o".into(),
        branch_name: "story/1-1-test".into(),
        base_branch: "main".into(),
        chat_history: vec![],
    };

    let path = write_wal_file(dir.path(), &state);
    let content = std::fs::read_to_string(&path).unwrap();
    let loaded: SessionState = serde_yml::from_str(&content).expect("WAL YAML should parse");
    assert!(loaded.chat_history.is_empty());
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_initializes_git_repo() {
    let dir = tempfile::tempdir().expect("create temp dir");
    create_test_repo(dir.path());

    // Verify .git directory exists
    assert!(dir.path().join(".git").exists());
}

#[test]
fn test_create_test_repo_has_head_commit() {
    let dir = tempfile::tempdir().expect("create temp dir");
    create_test_repo(dir.path());

    // Verify HEAD points to a commit
    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(dir.path())
        .output()
        .expect("git log failed");

    assert!(output.status.success());
    let log_line = String::from_utf8_lossy(&output.stdout);
    assert!(
        log_line.contains("initial commit"),
        "HEAD should contain 'initial commit', got: {log_line}"
    );
}

#[test]
fn test_create_test_repo_main_branch_exists() {
    let dir = tempfile::tempdir().expect("create temp dir");
    create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["branch", "--list", "main"])
        .current_dir(dir.path())
        .output()
        .expect("git branch failed");

    assert!(output.status.success());
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.contains("main"),
        "Should have 'main' branch, got: {branches}"
    );
}
