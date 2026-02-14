//! Self-verification tests for fixture builder functions.

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
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_paths_use_provided_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());

    assert!(config.bmad_paths.project_root.contains(&dir.path().display().to_string()));
    assert!(config.bmad_paths.output_folder.contains("_bmad-output"));
    assert!(config.bmad_paths.implementation_artifacts.contains("implementation-artifacts"));
}

// ---------------------------------------------------------------------------
// make_test_secrets tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_produces_all_dummy_tokens() {
    let secrets = make_test_secrets();

    assert!(secrets.anthropic_api_key.as_ref().expect("should be Some").contains("DO-NOT-USE"));
    assert!(secrets.openai_api_key.as_ref().expect("should be Some").contains("DO-NOT-USE"));
    assert!(secrets.github_copilot_oauth_token.as_ref().expect("should be Some").contains("DO-NOT-USE"));
    assert!(secrets.github_token.as_ref().expect("should be Some").contains("DO-NOT-USE"));
    assert!(secrets.gitlab_token.as_ref().expect("should be Some").contains("DO-NOT-USE"));
    assert!(secrets.telegram_bot_token.as_ref().expect("should be Some").contains("DO-NOT-USE"));
}

#[test]
fn test_make_test_secrets_never_empty() {
    let secrets = make_test_secrets();

    assert!(secrets.anthropic_api_key.is_some());
    assert!(secrets.openai_api_key.is_some());
    assert!(secrets.github_copilot_oauth_token.is_some());
    assert!(secrets.github_token.is_some());
    assert!(secrets.gitlab_token.is_some());
    assert!(secrets.telegram_bot_token.is_some());
}

// ---------------------------------------------------------------------------
// make_test_story tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key_correctly() {
    let story = make_test_story("7-1-integration-test", "", vec![]);

    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "integration test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_with_custom_label() {
    let story = make_test_story("1-2-some-slug", "Custom Label", vec![]);
    assert_eq!(story.label, "Custom Label");
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["1-1-first".to_string(), "1-2-second".to_string()];
    let story = make_test_story("1-3-third", "", deps);
    assert_eq!(story.dependencies.len(), 2);
    assert_eq!(story.dependencies[0], "1-1-first");
    assert_eq!(story.dependencies[1], "1-2-second");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("3-2-my-feature", "", vec![]);
    assert_eq!(
        story.specs_path.to_string_lossy(),
        "_bmad-output/implementation-artifacts/3-2-my-feature.md"
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
        ("1-1-first-story", "done"),
        ("1-2-second-story", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), entries);
    assert!(path.exists());

    // Verify it's parseable by SprintStatusFile::load
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");
    assert_eq!(sprint.entry_count(), 4);

    let stories = sprint.stories();
    assert_eq!(stories.len(), 2); // epics and retros are filtered out
    assert_eq!(stories[0].story_key, "1-1-first-story");
    assert_eq!(stories[1].story_key, "1-2-second-story");
}

#[test]
fn test_write_sprint_status_eligible_stories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-done-story", "done"),
        ("1-2-ready-story", "ready-for-dev"),
        ("1-3-blocked-story", "blocked"),
    ];

    let path = write_sprint_status(dir.path(), entries);
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");
    let eligible = sprint.eligible_stories();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "1-2-ready-story");
}

#[test]
fn test_write_sprint_status_preserves_entry_order() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-alpha", "done"),
        ("1-2-beta", "ready-for-dev"),
        ("1-3-gamma", "blocked"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), entries);
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");
    let all_entries = sprint.entries();
    assert_eq!(all_entries[0].0, "epic-1");
    assert_eq!(all_entries[1].0, "1-1-alpha");
    assert_eq!(all_entries[2].0, "1-2-beta");
    assert_eq!(all_entries[3].0, "1-3-gamma");
    assert_eq!(all_entries[4].0, "epic-1-retrospective");
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("4-2-test-session", "", vec![]);
    let state = SessionState::new(&story, "anthropic", "test-model");

    let path = write_wal_file(dir.path(), &state);
    assert!(path.exists());

    // Verify it's parseable
    let content = std::fs::read_to_string(&path).expect("should read");
    let loaded: SessionState = serde_yml::from_str(&content).expect("should parse YAML");
    assert_eq!(loaded.story_id, "4.2");
    assert_eq!(loaded.story_key, "4-2-test-session");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.model, "test-model");
    assert!(loaded.chat_history.is_empty());
}

#[test]
fn test_write_wal_file_preserves_chat_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("1-1-chat-test", "", vec![]);
    let mut state = SessionState::new(&story, "openai", "gpt-4");
    state.add_user_message("Hello");
    state.add_assistant_message("Hi there!");

    let path = write_wal_file(dir.path(), &state);
    let content = std::fs::read_to_string(&path).expect("should read");
    let loaded: SessionState = serde_yml::from_str(&content).expect("should parse YAML");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[0].content, "Hello");
    assert_eq!(loaded.chat_history[1].role, "assistant");
    assert_eq!(loaded.chat_history[1].content, "Hi there!");
}

#[test]
fn test_write_wal_file_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("1-1-path-test", "", vec![]);
    let state = SessionState::new(&story, "test", "test");

    let path = write_wal_file(dir.path(), &state);
    assert_eq!(path.file_name().expect("should have filename").to_string_lossy(), ".bmad-bot-session.yaml");
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_initializes_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // Verify .git directory exists
    assert!(dir.path().join(".git").exists());
}

#[test]
fn test_create_test_repo_has_initial_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // Verify HEAD exists and has at least one commit
    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(dir.path())
        .output()
        .expect("git log failed");

    assert!(output.status.success());
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(log.contains("initial commit"));
}

#[test]
fn test_create_test_repo_main_branch() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // Verify current branch is "main"
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .expect("git branch failed");

    assert!(output.status.success());
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(branch, "main");
}
