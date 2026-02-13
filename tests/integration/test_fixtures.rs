//! Self-verification tests for fixture builders.

use crate::helpers::fixtures::*;
use bmad_bot::watcher::SprintStatusFile;
use std::path::Path;

// ---------------------------------------------------------------------------
// make_test_config tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert!(config.code_review_enabled);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.target_branch, "main");
    assert_eq!(config.llm.dev.provider, "anthropic");
    assert!(!config.notifications.telegram.enabled);
    assert!(config
        .bmad_paths
        .project_root
        .contains(dir.path().to_str().unwrap()));
}

#[test]
fn test_make_test_config_passes_validation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());
    config.validate().expect("config should validate");
}

// ---------------------------------------------------------------------------
// make_test_secrets tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_has_all_fields() {
    let secrets = make_test_secrets();
    assert!(secrets.anthropic_api_key.is_some());
    assert!(secrets.openai_api_key.is_some());
    assert!(secrets.github_copilot_oauth_token.is_some());
    assert!(secrets.github_token.is_some());
    assert!(secrets.gitlab_token.is_some());
    assert!(secrets.telegram_bot_token.is_some());
}

#[test]
fn test_make_test_secrets_uses_dummy_tokens() {
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
        "Integration Test Infrastructure",
        vec![],
    );
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test-infrastructure");
    assert_eq!(story.branch_name, "story/7-1-integration-test-infrastructure");
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_includes_dependencies() {
    let story = make_test_story(
        "7-2-config-tests",
        "Config Tests",
        vec!["7-1-integration-test-infrastructure".into()],
    );
    assert_eq!(story.dependencies.len(), 1);
    assert_eq!(
        story.dependencies[0],
        "7-1-integration-test-infrastructure"
    );
}

#[test]
fn test_make_test_story_specs_path_correct() {
    let story = make_test_story("3-2-llm-fallback", "LLM Fallback", vec![]);
    assert_eq!(
        story.specs_path,
        std::path::PathBuf::from(
            "_bmad-output/implementation-artifacts/3-2-llm-fallback.md"
        )
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_writes_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-slug", "ready-for-dev"),
        ("1-2-another-story", "done"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    assert!(path.exists());

    let content = std::fs::read_to_string(&path).expect("read");
    assert!(content.contains("development_status:"));
    assert!(content.contains("epic-1: in-progress"));
    assert!(content.contains("1-1-story-slug: ready-for-dev"));
    assert!(content.contains("1-2-another-story: done"));
    assert!(content.contains("epic-1-retrospective: optional"));
}

#[test]
fn test_write_sprint_status_loadable_by_sprint_status_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-slug", "ready-for-dev"),
        ("1-2-another-story", "done"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should load");
    let stories = sprint.stories();
    assert_eq!(stories.len(), 2); // epics filtered out
    assert_eq!(stories[0].story_key, "1-1-story-slug");
    assert_eq!(stories[1].story_key, "1-2-another-story");
}

#[test]
fn test_write_sprint_status_empty_entries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_sprint_status(dir.path(), &[]);

    let content = std::fs::read_to_string(&path).expect("read");
    assert!(content.contains("development_status:"));
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_writes_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("7-1-infra", "Infra", vec![]);
    let state = bmad_bot::session::SessionState::new(&story, "anthropic", "test-model");

    let path = write_wal_file(dir.path(), &state);
    assert!(path.exists());

    let content = std::fs::read_to_string(&path).expect("read");
    // serde_yml may use quotes around some values; check key substrings
    assert!(content.contains("story_key"), "missing story_key field: {content}");
    assert!(content.contains("7-1-infra"), "missing story key value: {content}");
    assert!(content.contains("provider"), "missing provider field: {content}");
    assert!(content.contains("anthropic"), "missing provider value: {content}");
    assert!(content.contains("model"), "missing model field: {content}");
    assert!(content.contains("test-model"), "missing model value: {content}");
}

#[tokio::test]
async fn test_write_wal_file_roundtrips_via_session_state_load() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("7-1-infra", "Infra", vec![]);
    let mut state = bmad_bot::session::SessionState::new(&story, "anthropic", "test-model");
    state.set_branch_info("story/7-1-infra", "main");

    let path = write_wal_file(dir.path(), &state);
    let loaded = bmad_bot::session::SessionState::load(&path)
        .await
        .expect("should load");

    assert_eq!(loaded.story_key, "7-1-infra");
    assert_eq!(loaded.branch_name, "story/7-1-infra");
    assert_eq!(loaded.base_branch, "main");
    assert_eq!(loaded.provider, "anthropic");
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_creates_valid_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    assert!(!repo.is_bare());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn test_create_test_repo_has_head_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    let head = repo.head().expect("HEAD should exist");
    let commit = head.peel_to_commit().expect("should peel to commit");
    assert_eq!(commit.message(), Some("initial commit"));
}

#[test]
fn test_create_test_repo_head_is_on_default_branch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    let head = repo.head().expect("HEAD should exist");
    assert!(head.is_branch());
}
