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
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert_eq!(config.llm.dev.provider, "anthropic");
    assert_eq!(config.llm.review.provider, "anthropic");
    assert_eq!(config.llm.supervisor.provider, "anthropic");
    assert!(!config.notifications.telegram.enabled);
}

#[test]
fn test_make_test_config_uses_provided_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());

    assert!(config.bmad_paths.project_root.contains(dir.path().to_str().unwrap()));
    assert!(config
        .bmad_paths
        .implementation_artifacts
        .contains("implementation-artifacts"));
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
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_with_dependencies() {
    let story = make_test_story(
        "7-2-config-tests",
        "config tests",
        vec!["7-1-infra".into()],
    );

    assert_eq!(story.dependencies.len(), 1);
    assert_eq!(story.dependencies[0], "7-1-infra");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("1-1-scaffold", "scaffold", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/1-1-scaffold.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status tests
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

    write_sprint_status(dir.path(), &entries);

    let content = std::fs::read_to_string(dir.path().join("sprint-status.yaml")).expect("read");
    assert!(content.contains("development_status:"));
    assert!(content.contains("epic-1: in-progress"));
    assert!(content.contains("1-1-scaffold: done"));
    assert!(content.contains("1-2-cli: ready-for-dev"));
    assert!(content.contains("epic-1-retrospective: optional"));
}

#[test]
fn test_write_sprint_status_loadable_by_sprint_status_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffold", "done"),
        ("1-2-cli", "ready-for-dev"),
    ];

    write_sprint_status(dir.path(), &entries);

    let path = dir.path().join("sprint-status.yaml");
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("load sprint status");
    let stories = sprint.stories();

    // Should find 2 stories (skips epic entries)
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-scaffold");
    assert_eq!(stories[1].story_key, "1-2-cli");
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("4-2-agent-session", "agent session", vec![]);

    let state = SessionState {
        story_id: story.story_id.clone(),
        story_key: story.story_key.clone(),
        branch: story.branch_name.clone(),
        started_at: "2026-01-01T00:00:00Z".into(),
        last_activity: "2026-01-01T00:05:00Z".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-4-20250514".into(),
        branch_name: story.branch_name.clone(),
        base_branch: "main".into(),
        chat_history: vec![
            ChatMessage {
                role: "user".into(),
                content: "hello".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "hi".into(),
            },
        ],
    };

    write_wal_file(dir.path(), &state);

    let content =
        std::fs::read_to_string(dir.path().join(".bmad-bot-session.yaml")).expect("read WAL");
    assert!(content.contains("story_key") && content.contains("4-2-agent-session"));
    assert!(content.contains("provider") && content.contains("anthropic"));
}

#[test]
fn test_write_wal_file_roundtrips_via_serde() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = SessionState {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        last_activity: "2026-01-01T00:01:00Z".into(),
        provider: "anthropic".into(),
        model: "test-model".into(),
        branch_name: "story/1-1-test".into(),
        base_branch: "main".into(),
        chat_history: vec![],
    };

    write_wal_file(dir.path(), &state);

    let content =
        std::fs::read_to_string(dir.path().join(".bmad-bot-session.yaml")).expect("read WAL");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse WAL");

    assert_eq!(loaded.story_key, "1-1-test");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.base_branch, "main");
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_initializes_valid_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    // HEAD should exist and point to a commit
    let head = repo.head().expect("HEAD should exist");
    assert!(head.is_branch());
}

#[test]
fn test_create_test_repo_has_initial_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    let head = repo.head().expect("HEAD");
    let commit = head.peel_to_commit().expect("commit");
    assert_eq!(commit.message().unwrap(), "initial commit");
}
