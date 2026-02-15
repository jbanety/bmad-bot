//! Self-verification tests for fixture builder functions.

use std::path::PathBuf;

use crate::helpers::fixtures::*;

use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert_eq!(config.llm.dev.provider, "anthropic");
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_paths_use_dir() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path());

    assert!(
        config
            .bmad_paths
            .implementation_artifacts
            .contains(dir.path().to_str().unwrap()),
        "implementation_artifacts should contain temp dir path"
    );
}

#[test]
fn test_make_test_config_validates_successfully() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path());
    config.validate().expect("config should be valid");
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
fn test_make_test_secrets_tokens_are_not_real() {
    let secrets = make_test_secrets();
    assert!(
        secrets
            .anthropic_api_key
            .as_ref()
            .unwrap()
            .contains("DO-NOT-USE"),
        "test tokens must contain DO-NOT-USE"
    );
}

// ---------------------------------------------------------------------------
// make_test_story tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key() {
    let story = make_test_story("7-1-integration-test-infrastructure", "integration test infrastructure", vec![]);

    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test-infrastructure");
    assert_eq!(story.label, "integration test infrastructure");
    assert_eq!(story.branch_name, "story/7-1-integration-test-infrastructure");
    assert_eq!(
        story.specs_path,
        PathBuf::from("_bmad-output/implementation-artifacts/7-1-integration-test-infrastructure.md")
    );
    assert_eq!(story.status, "ready-for-dev");
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["1-1-scaffolding".to_string(), "1-2-cli".to_string()];
    let story = make_test_story("1-3-init", "init command", deps.clone());

    assert_eq!(story.dependencies, deps);
}

// ---------------------------------------------------------------------------
// write_sprint_status tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let dir = tempfile::tempdir().unwrap();

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-a", "ready-for-dev"),
        ("1-2-story-b", "done"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    assert!(path.exists(), "sprint-status.yaml should exist");

    // Verify it can be loaded by the real parser
    let status = SprintStatusFile::load(&path, dir.path())
        .expect("should parse as valid sprint status");

    let stories = status.stories();
    assert_eq!(stories.len(), 2, "should find 2 stories (not epics/retros)");
    assert_eq!(stories[0].story_key, "1-1-story-a");
    assert_eq!(stories[0].status, "ready-for-dev");
    assert_eq!(stories[1].story_key, "1-2-story-b");
    assert_eq!(stories[1].status, "done");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().unwrap();

    let entries = vec![
        ("epic-2", "backlog"),
        ("2-1-first", "ready-for-dev"),
        ("epic-2-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    let status = SprintStatusFile::load(&path, dir.path()).unwrap();

    // entries() returns all key-value pairs
    let all_entries = status.entries();
    assert_eq!(all_entries.len(), 3, "should have 3 entries total");

    // stories() should filter to just story entries
    let stories = status.stories();
    assert_eq!(stories.len(), 1);
    assert_eq!(stories[0].story_key, "2-1-first");
}

#[test]
fn test_write_sprint_status_eligible_stories() {
    let dir = tempfile::tempdir().unwrap();

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-done-story", "done"),
        ("1-2-ready-story", "ready-for-dev"),
        ("1-3-blocked-story", "blocked"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    let status = SprintStatusFile::load(&path, dir.path()).unwrap();

    let eligible = status.eligible_stories();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "1-2-ready-story");
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().unwrap();

    let state = SessionState {
        story_id: "4.2".into(),
        story_key: "4-2-agent-session".into(),
        branch: "story/4-2-agent-session".into(),
        started_at: "2026-01-15T10:00:00Z".into(),
        last_activity: "2026-01-15T10:30:00Z".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-4-20250514".into(),
        branch_name: "story/4-2-agent-session".into(),
        base_branch: "main".into(),
        chat_history: vec![
            ChatMessage {
                role: "user".into(),
                content: "Implement task 1".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Working on task 1...".into(),
            },
        ],
    };

    let path = write_wal_file(dir.path(), &state);
    assert!(path.exists(), "WAL file should exist");

    // Verify it can be deserialized back
    let content = std::fs::read_to_string(&path).unwrap();
    let loaded: SessionState = serde_yml::from_str(&content).expect("should parse WAL YAML");

    assert_eq!(loaded.story_id, "4.2");
    assert_eq!(loaded.story_key, "4-2-agent-session");
    assert_eq!(loaded.branch, "story/4-2-agent-session");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[1].content, "Working on task 1...");
    assert_eq!(loaded.branch_name, "story/4-2-agent-session");
    assert_eq!(loaded.base_branch, "main");
}

#[test]
fn test_write_wal_file_empty_chat_history() {
    let dir = tempfile::tempdir().unwrap();

    let state = SessionState {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        last_activity: "2026-01-01T00:00:00Z".into(),
        provider: "openai".into(),
        model: "gpt-4o".into(),
        branch_name: String::new(),
        base_branch: String::new(),
        chat_history: vec![],
    };

    let path = write_wal_file(dir.path(), &state);
    let content = std::fs::read_to_string(&path).unwrap();
    let loaded: SessionState = serde_yml::from_str(&content).unwrap();

    assert!(loaded.chat_history.is_empty());
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_produces_valid_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    create_test_repo(dir.path());

    // Verify .git directory exists
    assert!(
        dir.path().join(".git").exists(),
        ".git directory should exist"
    );

    // Verify HEAD commit exists
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git rev-parse failed");
    assert!(output.status.success(), "HEAD should resolve to a commit");

    // Verify main branch exists
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .expect("git branch failed");
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(branch, "main", "current branch should be 'main'");
}

#[test]
fn test_create_test_repo_has_user_config() {
    let dir = tempfile::tempdir().unwrap();
    create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["config", "user.email"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let email = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(email, "test@test.com");
}
