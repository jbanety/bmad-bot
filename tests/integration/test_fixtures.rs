//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures::*;

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
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_uses_provided_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());

    let dir_str = dir.path().display().to_string();
    assert_eq!(config.bmad_paths.project_root, dir_str);
    assert_eq!(config.bmad_paths.implementation_artifacts, dir_str);
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
fn test_make_test_secrets_contains_do_not_use_marker() {
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
    let story = make_test_story("7-1-integration-test", "Integration Test", vec![]);

    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.label, "Integration Test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
    assert_eq!(story.status, "ready-for-dev");
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["6-1-telegram".into(), "6-2-retry".into()];
    let story = make_test_story("7-1-infra", "Infra", deps.clone());

    assert_eq!(story.dependencies, deps);
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("7-1-infra", "Infra", vec![]);
    assert_eq!(
        story.specs_path.to_str().expect("path"),
        "_bmad-output/implementation-artifacts/7-1-infra.md"
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
        ("1-1-story-slug", "ready-for-dev"),
        ("1-2-another-story", "backlog"),
        ("epic-1-retrospective", "optional"),
    ];

    write_sprint_status(dir.path(), &entries);

    let path = dir.path().join("sprint-status.yaml");
    assert!(path.exists(), "sprint-status.yaml should exist");

    let content = std::fs::read_to_string(&path).expect("read");
    assert!(content.contains("development_status:"));
    assert!(content.contains("epic-1: in-progress"));
    assert!(content.contains("1-1-story-slug: ready-for-dev"));
    assert!(content.contains("1-2-another-story: backlog"));
    assert!(content.contains("epic-1-retrospective: optional"));
}

#[test]
fn test_write_sprint_status_parseable_by_sprint_status_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    write_sprint_status(dir.path(), &entries);

    let path = dir.path().join("sprint-status.yaml");
    let sprint_file =
        bmad_bot::watcher::SprintStatusFile::load(&path, dir.path()).expect("load sprint status");

    let stories = sprint_file.stories();
    assert_eq!(stories.len(), 2, "should parse 2 story entries");
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[1].story_key, "1-2-cli-framework");
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = make_test_session_state();

    write_wal_file(dir.path(), &state);

    let path = dir.path().join(".bmad-bot-session.yaml");
    assert!(path.exists(), "WAL file should exist");

    let content = std::fs::read_to_string(&path).expect("read");
    assert!(content.contains("story_id: '7.1'") || content.contains("story_id: \"7.1\""));
    assert!(content.contains("7-1-integration-test-infrastructure"));
}

#[test]
fn test_write_wal_file_roundtrips_through_session_state() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = make_test_session_state();

    write_wal_file(dir.path(), &state);

    let path = dir.path().join(".bmad-bot-session.yaml");
    let content = std::fs::read_to_string(&path).expect("read");
    let loaded: bmad_bot::session::SessionState =
        serde_yml::from_str(&content).expect("parse YAML");

    assert_eq!(loaded.story_id, state.story_id);
    assert_eq!(loaded.story_key, state.story_key);
    assert_eq!(loaded.branch, state.branch);
    assert_eq!(loaded.provider, state.provider);
    assert_eq!(loaded.model, state.model);
    assert_eq!(loaded.chat_history.len(), state.chat_history.len());
    assert_eq!(loaded.branch_name, state.branch_name);
    assert_eq!(loaded.base_branch, state.base_branch);
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_creates_valid_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    // Verify it's a valid repo by checking HEAD exists
    let head = repo.head().expect("HEAD should exist");
    assert!(head.is_branch(), "HEAD should point to a branch");
}

#[test]
fn test_create_test_repo_has_initial_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    let head = repo.head().expect("HEAD");
    let commit = head.peel_to_commit().expect("peel to commit");
    assert_eq!(commit.message().expect("message"), "initial commit");
}

#[test]
fn test_create_test_repo_has_git_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _repo = create_test_repo(dir.path());

    let git_dir = dir.path().join(".git");
    assert!(git_dir.exists(), ".git directory should exist");
}
