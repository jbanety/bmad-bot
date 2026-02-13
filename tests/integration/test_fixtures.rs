//! Self-verification tests for fixture builders.
//!
//! Ensures every fixture function produces valid data structures and
//! filesystem artifacts.

use crate::helpers::fixtures;
use bmad_bot::session::SessionState;
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = fixtures::make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert_eq!(config.llm.dev.provider, "anthropic");
    assert_eq!(config.llm.supervisor.provider, "openai");
    assert!(!config.notifications.telegram.enabled);
}

#[test]
fn test_make_test_config_uses_provided_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = fixtures::make_test_config(dir.path());

    assert!(config.bmad_paths.project_root.contains(&dir.path().display().to_string()));
    assert!(config.bmad_paths.implementation_artifacts.contains("implementation-artifacts"));
}

#[test]
fn test_make_test_config_validates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = fixtures::make_test_config(dir.path());
    // validate() checks non-empty fields, valid providers, valid log format/level
    assert!(config.validate().is_ok());
}

// ---------------------------------------------------------------------------
// make_test_secrets tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_all_fields_populated() {
    let secrets = fixtures::make_test_secrets();

    assert!(secrets.anthropic_api_key.is_some());
    assert!(secrets.openai_api_key.is_some());
    assert!(secrets.github_copilot_oauth_token.is_some());
    assert!(secrets.github_token.is_some());
    assert!(secrets.gitlab_token.is_some());
    assert!(secrets.telegram_bot_token.is_some());
}

#[test]
fn test_make_test_secrets_tokens_are_clearly_test() {
    let secrets = fixtures::make_test_secrets();

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
    let story = fixtures::make_test_story(
        "7-1-integration-test",
        "integration test",
        vec!["6-3-crash-recovery".into()],
    );

    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.label, "integration test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
    assert_eq!(story.status, "ready-for-dev");
    assert_eq!(story.dependencies.len(), 1);
    assert_eq!(story.dependencies[0], "6-3-crash-recovery");
}

#[test]
fn test_make_test_story_empty_deps() {
    let story = fixtures::make_test_story("1-1-test", "test", vec![]);
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_specs_path_format() {
    let story = fixtures::make_test_story("3-2-llm-fallback", "llm fallback", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/3-2-llm-fallback.md"
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

    let path = fixtures::write_sprint_status(dir.path(), &entries);

    assert!(path.exists());

    // Verify it's parseable by the production parser
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("parse sprint-status");
    let stories = sprint.stories();

    // stories() filters out epics and retrospectives
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-story-slug");
    assert_eq!(stories[0].status, "ready-for-dev");
    assert_eq!(stories[1].story_key, "1-2-another-story");
    assert_eq!(stories[1].status, "backlog");
}

#[test]
fn test_write_sprint_status_preserves_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");

    let entries = vec![
        ("epic-2", "backlog"),
        ("2-1-feature", "done"),
        ("epic-2-retrospective", "done"),
    ];

    let path = fixtures::write_sprint_status(dir.path(), &entries);
    let content = std::fs::read_to_string(&path).expect("read");

    assert!(content.contains("epic-2: backlog"));
    assert!(content.contains("2-1-feature: done"));
    assert!(content.contains("epic-2-retrospective: done"));
}

#[test]
fn test_write_sprint_status_eligible_stories() {
    let dir = tempfile::tempdir().expect("tempdir");

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-first", "done"),
        ("1-2-second", "ready-for-dev"),
        ("1-3-third", "backlog"),
    ];

    let path = fixtures::write_sprint_status(dir.path(), &entries);
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("parse");
    let eligible = sprint.eligible_stories();

    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "1-2-second");
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");

    let state = SessionState::new(
        &fixtures::make_test_story("4-2-session", "session", vec![]),
        "anthropic",
        "claude-sonnet-4",
    );

    let path = fixtures::write_wal_file(dir.path(), &state);

    assert!(path.exists());

    // Verify it's parseable by the production loader
    let loaded = SessionState::load(&path).await.expect("load WAL");
    assert_eq!(loaded.story_key, "4-2-session");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.model, "claude-sonnet-4");
    assert!(loaded.chat_history.is_empty());
}

#[tokio::test]
async fn test_write_wal_file_with_chat_history() {
    let dir = tempfile::tempdir().expect("tempdir");

    let mut state = SessionState::new(
        &fixtures::make_test_story("4-2-session", "session", vec![]),
        "anthropic",
        "claude-sonnet-4",
    );

    state.add_user_message("DS");
    state.add_assistant_message("Starting dev story...");

    let path = fixtures::write_wal_file(dir.path(), &state);
    let loaded = SessionState::load(&path).await.expect("load WAL");

    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[0].content, "DS");
    assert_eq!(loaded.chat_history[1].role, "assistant");
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_has_head_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = fixtures::create_test_repo(dir.path());

    // HEAD should point to a valid commit
    let head = repo.head().expect("HEAD");
    assert!(head.is_branch());

    let commit = head.peel_to_commit().expect("peel to commit");
    assert_eq!(commit.message().unwrap(), "initial commit");
}

#[test]
fn test_create_test_repo_is_valid_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let _repo = fixtures::create_test_repo(dir.path());

    // Verify we can re-open it
    let reopened = git2::Repository::open(dir.path());
    assert!(reopened.is_ok());
}
