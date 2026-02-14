//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures::*;
use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.repo_owner, "test-owner");
    assert_eq!(config.git_provider.repo_name, "test-repo");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert_eq!(config.llm.dev.provider, "anthropic");
    assert_eq!(config.notifications.telegram.enabled, false);
}

#[test]
fn test_make_test_config_uses_dir_for_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());

    assert!(config.bmad_paths.implementation_artifacts.contains(dir.path().to_str().unwrap()));
    assert!(config.bmad_paths.output_folder.contains(dir.path().to_str().unwrap()));
}

// ---------------------------------------------------------------------------
// make_test_secrets tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_all_fields_present() {
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
    assert!(secrets.anthropic_api_key.unwrap().contains("DO-NOT-USE"));
    assert!(secrets.github_token.unwrap().contains("DO-NOT-USE"));
}

// ---------------------------------------------------------------------------
// make_test_story tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key_correctly() {
    let story = make_test_story("7-1-integration-test", "integration test", vec![]);

    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "integration test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
    assert_eq!(story.status, "ready-for-dev");
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["6-1-dep".to_string(), "6-2-dep".to_string()];
    let story = make_test_story("7-2-test", "test", deps.clone());

    assert_eq!(story.dependencies, deps);
}

#[test]
fn test_make_test_story_specs_path_correct() {
    let story = make_test_story("7-1-test", "test", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/7-1-test.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_valid_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-slug", "ready-for-dev"),
        ("1-2-another-story", "done"),
        ("epic-1-retrospective", "optional"),
    ];

    write_sprint_status(dir.path(), entries);

    let path = dir.path().join("sprint-status.yaml");
    assert!(path.exists());

    // Verify it's parseable by SprintStatusFile
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");
    assert!(sprint.entry_count() > 0);
}

#[test]
fn test_write_sprint_status_stories_are_parseable() {
    let dir = tempfile::tempdir().expect("tempdir");

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-test-story", "ready-for-dev"),
        ("1-2-other-story", "done"),
    ];

    write_sprint_status(dir.path(), entries);

    let path = dir.path().join("sprint-status.yaml");
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");
    let stories = sprint.stories();

    assert_eq!(stories.len(), 2); // epics are filtered out
    assert_eq!(stories[0].story_key, "1-1-test-story");
    assert_eq!(stories[0].status, "ready-for-dev");
    assert_eq!(stories[1].story_key, "1-2-other-story");
    assert_eq!(stories[1].status, "done");
}

#[test]
fn test_write_sprint_status_contains_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    write_sprint_status(dir.path(), entries);

    let path = dir.path().join("sprint-status.yaml");
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");

    // All 3 entries present (epics + stories + retrospectives)
    let all_entries = sprint.entries();
    assert_eq!(all_entries.len(), 3);
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_valid_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");

    let story = make_test_story("7-1-test", "test", vec![]);
    let state = SessionState {
        story_id: story.story_id.clone(),
        story_key: story.story_key.clone(),
        branch: story.branch_name.clone(),
        started_at: "2026-02-08T10:00:00Z".into(),
        last_activity: "2026-02-08T10:05:00Z".into(),
        provider: "anthropic".into(),
        model: "test-model".into(),
        branch_name: story.branch_name.clone(),
        base_branch: "main".into(),
        chat_history: vec![
            ChatMessage {
                role: "user".into(),
                content: "Hello".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Hi there!".into(),
            },
        ],
    };

    write_wal_file(dir.path(), &state);

    let path = dir.path().join(".bmad-bot-session.yaml");
    assert!(path.exists());

    // Verify it's parseable back
    let content = std::fs::read_to_string(&path).expect("read");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse");
    assert_eq!(loaded.story_key, "7-1-test");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[1].content, "Hi there!");
}

#[test]
fn test_write_wal_file_empty_history() {
    let dir = tempfile::tempdir().expect("tempdir");

    let state = SessionState {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        started_at: "2026-02-08T10:00:00Z".into(),
        last_activity: "2026-02-08T10:00:00Z".into(),
        provider: "anthropic".into(),
        model: "test".into(),
        branch_name: String::new(),
        base_branch: String::new(),
        chat_history: vec![],
    };

    write_wal_file(dir.path(), &state);

    let path = dir.path().join(".bmad-bot-session.yaml");
    let content = std::fs::read_to_string(&path).expect("read");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse");
    assert!(loaded.chat_history.is_empty());
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_initializes_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // Check .git directory exists
    assert!(dir.path().join(".git").exists());
}

#[test]
fn test_create_test_repo_has_initial_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // Check HEAD points to a valid commit
    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(dir.path())
        .output()
        .expect("git log");

    assert!(output.status.success());
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(log.contains("initial commit"));
}

#[test]
fn test_create_test_repo_main_branch_exists() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["branch", "--list", "main"])
        .current_dir(dir.path())
        .output()
        .expect("git branch");

    assert!(output.status.success());
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(branches.contains("main"));
}
