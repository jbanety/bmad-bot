//! Self-verification tests for fixture builders.

use crate::helpers::fixtures::*;

use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config tests (Task 7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_returns_valid_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert_eq!(config.llm.dev.provider, "anthropic");
    assert_eq!(config.llm.review.provider, "anthropic");
    assert_eq!(config.llm.supervisor.provider, "anthropic");
    assert!(!config.notifications.telegram.enabled);
    assert!(config.bmad_paths.project_root.contains(dir.path().to_str().unwrap()));
}

#[test]
fn test_make_test_config_passes_validation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());
    config.validate().expect("config should validate");
}

// ---------------------------------------------------------------------------
// make_test_secrets tests (Task 7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_has_all_keys() {
    let secrets = make_test_secrets();

    assert!(secrets.anthropic_api_key.is_some());
    assert!(secrets.openai_api_key.is_some());
    assert!(secrets.github_copilot_oauth_token.is_some());
    assert!(secrets.github_token.is_some());
    assert!(secrets.gitlab_token.is_some());
    assert!(secrets.telegram_bot_token.is_some());
}

#[test]
fn test_make_test_secrets_uses_dummy_keys() {
    let secrets = make_test_secrets();

    assert!(secrets.anthropic_api_key.as_ref().unwrap().contains("DO-NOT-USE"));
    assert!(secrets.github_token.as_ref().unwrap().contains("DO-NOT-USE"));
}

// ---------------------------------------------------------------------------
// make_test_story tests (Task 7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key() {
    let story = make_test_story("7-1-integration-test", "integration test", vec![]);

    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.label, "integration test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
    assert_eq!(story.status, "ready-for-dev");
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_with_deps() {
    let deps = vec!["6-1-telegram".to_string(), "6-2-retry".to_string()];
    let story = make_test_story("7-2-config", "config", deps.clone());

    assert_eq!(story.dependencies, deps);
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("3-2-llm-fallback", "llm fallback", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/3-2-llm-fallback.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status tests (Task 7.6)
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffold", "done"),
        ("1-2-cli", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    assert!(path.exists());

    // Verify it's parseable by SprintStatusFile
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");
    let stories = sprint.stories();
    assert_eq!(stories.len(), 2); // epics/retros filtered out
    assert_eq!(stories[0].story_key, "1-1-scaffold");
    assert_eq!(stories[0].status, "done");
    assert_eq!(stories[1].story_key, "1-2-cli");
    assert_eq!(stories[1].status, "ready-for-dev");
}

#[test]
fn test_write_sprint_status_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-a", "done"),
        ("epic-1-retrospective", "optional"),
        ("epic-2", "backlog"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");

    // All 4 entries are present but only 1 story
    assert_eq!(sprint.entry_count(), 4);
    assert_eq!(sprint.stories().len(), 1);
}

#[test]
fn test_write_sprint_status_eligible_stories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-done", "done"),
        ("1-2-ready", "ready-for-dev"),
        ("1-3-blocked", "blocked"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");
    let eligible = sprint.eligible_stories();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "1-2-ready");
}

// ---------------------------------------------------------------------------
// write_wal_file tests (Task 7.7)
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = SessionState {
        story_id: "4.2".to_string(),
        story_key: "4-2-agent-session".to_string(),
        branch: "story/4-2-agent-session".to_string(),
        started_at: "2026-02-14T00:00:00Z".to_string(),
        last_activity: "2026-02-14T00:05:00Z".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        branch_name: "story/4-2-agent-session".to_string(),
        base_branch: "main".to_string(),
        chat_history: vec![
            ChatMessage {
                role: "user".to_string(),
                content: "hello".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "hi".to_string(),
            },
        ],
    };

    let path = write_wal_file(dir.path(), &state);
    assert!(path.exists());

    // Verify roundtrip
    let content = std::fs::read_to_string(&path).expect("read WAL");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse WAL");
    assert_eq!(loaded.story_key, "4-2-agent-session");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[1].content, "hi");
    assert_eq!(loaded.branch_name, "story/4-2-agent-session");
    assert_eq!(loaded.base_branch, "main");
}

#[test]
fn test_write_wal_file_empty_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = SessionState {
        story_id: "1.1".to_string(),
        story_key: "1-1-test".to_string(),
        branch: "story/1-1-test".to_string(),
        started_at: "2026-01-01T00:00:00Z".to_string(),
        last_activity: "2026-01-01T00:00:00Z".to_string(),
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        branch_name: String::new(),
        base_branch: String::new(),
        chat_history: vec![],
    };

    let path = write_wal_file(dir.path(), &state);
    let content = std::fs::read_to_string(&path).expect("read WAL");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse WAL");
    assert!(loaded.chat_history.is_empty());
}

// ---------------------------------------------------------------------------
// create_test_repo tests (Task 7.8)
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_has_head_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    // HEAD should exist and point to a commit
    let head = repo.head().expect("HEAD should exist");
    assert!(head.is_branch());
    let commit = head.peel_to_commit().expect("HEAD should be a commit");
    assert_eq!(commit.message().unwrap(), "initial commit");
}

#[test]
fn test_create_test_repo_is_not_bare() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());
    assert!(!repo.is_bare());
}

#[test]
fn test_create_test_repo_has_valid_signature() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());
    let head = repo.head().unwrap();
    let commit = head.peel_to_commit().unwrap();
    let author = commit.author();
    assert_eq!(author.name().unwrap(), "Test");
    assert_eq!(author.email().unwrap(), "test@test.com");
}
