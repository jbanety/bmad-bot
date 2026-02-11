//! Self-verification tests for fixture builder functions.
//!
//! Validates that all fixture helpers produce valid, parseable data structures.

use crate::helpers::fixtures::*;

use bmad_bot::session::state::SessionState;
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
    assert!(config
        .bmad_paths
        .project_root
        .contains(dir.path().to_str().expect("path")));
}

#[test]
fn test_make_test_config_passes_validation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());
    let result = config.validate();
    assert!(result.is_ok(), "Config validation failed: {result:?}");
}

// ---------------------------------------------------------------------------
// make_test_secrets tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_all_fields_populated() {
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
        .expect("key")
        .contains("DO-NOT-USE"));
    assert!(secrets
        .github_token
        .as_ref()
        .expect("key")
        .contains("DO-NOT-USE"));
}

// ---------------------------------------------------------------------------
// make_test_story tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key_correctly() {
    let story = make_test_story("7-1-integration-test-infrastructure", "", Vec::new());

    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test-infrastructure");
    assert_eq!(story.branch_name, "story/7-1-integration-test-infrastructure");
    assert_eq!(story.label, "integration test infrastructure");
    assert_eq!(story.status, "ready-for-dev");
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_with_custom_label() {
    let story = make_test_story("1-2-cli", "CLI Framework", Vec::new());
    assert_eq!(story.label, "CLI Framework");
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["7-1-infra".to_string(), "7-2-config".to_string()];
    let story = make_test_story("7-3-watcher", "", deps);

    assert_eq!(story.dependencies.len(), 2);
    assert_eq!(story.dependencies[0], "7-1-infra");
    assert_eq!(story.dependencies[1], "7-2-config");
}

#[test]
fn test_make_test_story_specs_path_format() {
    let story = make_test_story("3-1-supervisor", "", Vec::new());
    let path_str = story.specs_path.display().to_string();
    assert!(path_str.contains("3-1-supervisor.md"));
    assert!(path_str.contains("implementation-artifacts"));
}

// ---------------------------------------------------------------------------
// write_sprint_status tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    write_sprint_status(dir.path(), &entries);

    let path = dir.path().join("sprint-status.yaml");
    assert!(path.exists(), "sprint-status.yaml should exist");

    // Verify it's parseable by SprintStatusFile
    let status = SprintStatusFile::load(&path, dir.path());
    assert!(
        status.is_ok(),
        "Should parse: {:?}",
        status.err()
    );

    let status = status.expect("parsed");
    let stories = status.stories();
    // Only story entries, not epics or retrospectives
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[1].story_key, "1-2-cli");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story", "done"),
        ("epic-1-retrospective", "optional"),
    ];

    write_sprint_status(dir.path(), &entries);

    let content =
        std::fs::read_to_string(dir.path().join("sprint-status.yaml")).expect("read");
    assert!(content.contains("epic-1: in-progress"));
    assert!(content.contains("1-1-story: done"));
    assert!(content.contains("epic-1-retrospective: optional"));
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

    write_sprint_status(dir.path(), &entries);

    let path = dir.path().join("sprint-status.yaml");
    let status = SprintStatusFile::load(&path, dir.path()).expect("parsed");
    let eligible = status.eligible_stories();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "1-2-ready");
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = make_test_session_state("7-1-infra");

    write_wal_file(dir.path(), &state);

    let path = dir.path().join(".bmad-bot-session.yaml");
    assert!(path.exists(), "WAL file should exist");

    let content = std::fs::read_to_string(&path).expect("read WAL");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse WAL YAML");

    assert_eq!(loaded.story_key, "7-1-infra");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.model, "claude-sonnet-4-20250514");
    assert_eq!(loaded.branch_name, "story/7-1-infra");
    assert_eq!(loaded.base_branch, "main");
    assert_eq!(loaded.chat_history.len(), 1);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[0].content, "DS");
}

#[test]
fn test_write_wal_file_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = make_test_session_state("3-2-llm-fallback");

    write_wal_file(dir.path(), &state);

    let content = std::fs::read_to_string(dir.path().join(".bmad-bot-session.yaml")).expect("read");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse");

    assert_eq!(loaded.story_id, state.story_id);
    assert_eq!(loaded.story_key, state.story_key);
    assert_eq!(loaded.branch, state.branch);
    assert_eq!(loaded.provider, state.provider);
    assert_eq!(loaded.model, state.model);
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_has_head_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    // Verify HEAD exists and points to a commit
    let head = repo.head();
    assert!(head.is_ok(), "HEAD should exist after initial commit");

    let head_ref = head.expect("HEAD");
    assert!(head_ref.is_branch(), "HEAD should be a branch");

    let commit = head_ref.peel_to_commit();
    assert!(commit.is_ok(), "HEAD should peel to a commit");

    let commit = commit.expect("commit");
    assert_eq!(commit.message(), Some("initial commit"));
}

#[test]
fn test_create_test_repo_is_valid_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _repo = create_test_repo(dir.path());

    // Verify we can open it again
    let reopened = git2::Repository::open(dir.path());
    assert!(reopened.is_ok(), "Should be able to re-open the repo");
}

#[test]
fn test_create_test_repo_is_not_bare() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());
    assert!(!repo.is_bare(), "Test repo should not be bare");
}
