//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures::*;

use bmad_bot::session::state::{ChatMessage, SessionState};
use bmad_bot::watcher::{SprintStatusFile, StoryInfo};

// ---------------------------------------------------------------------------
// make_test_config
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
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_paths_use_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());
    let dir_str = dir.path().display().to_string();

    assert_eq!(config.bmad_paths.project_root, dir_str);
    assert_eq!(config.bmad_paths.output_folder, dir_str);
    assert_eq!(config.bmad_paths.implementation_artifacts, dir_str);
}

#[test]
fn test_make_test_config_validates_successfully() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());
    assert!(config.validate().is_ok());
}

// ---------------------------------------------------------------------------
// make_test_secrets
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
fn test_make_test_secrets_tokens_are_clearly_fake() {
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
fn test_make_test_story_parses_key_correctly() {
    let story = make_test_story("7-1-integration-test-infra", "", vec![]);

    assert_eq!(story.story_key, "7-1-integration-test-infra");
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "integration test infra");
    assert_eq!(story.branch_name, "story/7-1-integration-test-infra");
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_custom_label() {
    let story = make_test_story("1-1-test", "Custom Label", vec![]);
    assert_eq!(story.label, "Custom Label");
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["1-1-first".into(), "1-2-second".into()];
    let story = make_test_story("1-3-third", "", deps);

    assert_eq!(story.dependencies.len(), 2);
    assert_eq!(story.dependencies[0], "1-1-first");
    assert_eq!(story.dependencies[1], "1-2-second");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("2-1-watcher", "", vec![]);
    assert_eq!(
        story.specs_path.display().to_string(),
        "_bmad-output/implementation-artifacts/2-1-watcher.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_sprint_status(
        dir.path(),
        &[
            ("epic-1", "in-progress"),
            ("1-1-story-slug", "ready-for-dev"),
            ("1-2-another-story", "backlog"),
            ("epic-1-retrospective", "optional"),
        ],
    );

    assert!(path.exists());

    // Verify it parses via SprintStatusFile::load
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("load sprint status");
    let stories = sprint.stories();

    // stories() filters out epics and retrospectives
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-story-slug");
    assert_eq!(stories[0].status, "ready-for-dev");
    assert_eq!(stories[1].story_key, "1-2-another-story");
    assert_eq!(stories[1].status, "backlog");
}

#[test]
fn test_write_sprint_status_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_sprint_status(
        dir.path(),
        &[
            ("epic-1", "in-progress"),
            ("1-1-slug", "ready-for-dev"),
            ("epic-1-retrospective", "optional"),
            ("epic-2", "backlog"),
            ("2-1-another", "backlog"),
        ],
    );

    let sprint = SprintStatusFile::load(&path, dir.path()).expect("load");
    // entries() returns ALL entries including epics and retrospectives
    assert_eq!(sprint.entry_count(), 5);
    // stories() returns only story entries
    assert_eq!(sprint.stories().len(), 2);
}

#[test]
fn test_write_sprint_status_empty_entries() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = write_sprint_status(dir.path(), &[]);

    let sprint = SprintStatusFile::load(&path, dir.path()).expect("load");
    assert_eq!(sprint.stories().len(), 0);
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("1-1-test", "", vec![]);
    let state = SessionState::new(&story, "anthropic", "test-model");

    let path = write_wal_file(dir.path(), &state);
    assert!(path.exists());

    // Verify the YAML can be read back
    let content = std::fs::read_to_string(&path).expect("read WAL");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse WAL YAML");

    assert_eq!(loaded.story_id, "1.1");
    assert_eq!(loaded.story_key, "1-1-test");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.model, "test-model");
    assert!(loaded.chat_history.is_empty());
}

#[test]
fn test_write_wal_file_with_chat_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("2-1-watcher", "", vec![]);
    let mut state = SessionState::new(&story, "openai", "gpt-4o");
    state.chat_history.push(ChatMessage {
        role: "user".into(),
        content: "Implement the watcher".into(),
    });
    state.chat_history.push(ChatMessage {
        role: "assistant".into(),
        content: "I'll implement the watcher now.".into(),
    });

    let path = write_wal_file(dir.path(), &state);
    let content = std::fs::read_to_string(&path).expect("read WAL");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse WAL YAML");

    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[1].role, "assistant");
}

#[test]
fn test_write_wal_file_with_branch_info() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("3-1-supervisor", "", vec![]);
    let mut state = SessionState::new(&story, "anthropic", "claude-sonnet-4-20250514");
    state.set_branch_info("story/3-1-supervisor", "main");

    let path = write_wal_file(dir.path(), &state);
    let content = std::fs::read_to_string(&path).expect("read WAL");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse WAL YAML");

    assert_eq!(loaded.branch_name, "story/3-1-supervisor");
    assert_eq!(loaded.base_branch, "main");
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_initializes_valid_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    // Repo should exist
    assert!(!repo.is_bare());
    assert!(dir.path().join(".git").exists());
}

#[test]
fn test_create_test_repo_has_head_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    // HEAD should resolve to a commit
    let head = repo.head().expect("HEAD exists");
    assert!(head.peel_to_commit().is_ok());
}

#[test]
fn test_create_test_repo_head_commit_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = create_test_repo(dir.path());

    let head = repo.head().expect("HEAD");
    let commit = head.peel_to_commit().expect("commit");
    assert_eq!(commit.message().unwrap(), "initial commit");
}
