//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures::*;
use bmad_bot::session::SessionState;
use bmad_bot::watcher::SprintStatusFile;

// make_test_config tests (Task 7.5 partial)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_has_sensible_defaults() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let config = make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_uses_provided_dir_for_paths() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let config = make_test_config(dir.path());

    assert!(config.bmad_paths.project_root.contains(&dir.path().display().to_string()));
    assert!(config
        .bmad_paths
        .implementation_artifacts
        .contains("implementation-artifacts"));
}

// ---------------------------------------------------------------------------
// make_test_secrets tests (Task 7.5 partial)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_has_all_dummy_tokens() {
    let secrets = make_test_secrets();

    assert!(secrets.anthropic_api_key.is_some());
    assert!(secrets.openai_api_key.is_some());
    assert!(secrets.github_copilot_oauth_token.is_some());
    assert!(secrets.github_token.is_some());
    assert!(secrets.gitlab_token.is_some());
    assert!(secrets.telegram_bot_token.is_some());

    // Verify tokens contain "DO-NOT-USE" safety marker
    assert!(secrets
        .anthropic_api_key
        .as_ref()
        .expect("anthropic api key should be present")
        .contains("DO-NOT-USE"));
    assert!(secrets
        .github_token
        .as_ref()
        .expect("github token should be present")
        .contains("DO-NOT-USE"));
}

// ---------------------------------------------------------------------------
// make_test_story tests (Task 7.5 partial)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key_correctly() {
    let story = make_test_story("7-1-integration-tests", "integration tests", vec![]);

    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-tests");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "integration tests");
    assert_eq!(story.branch_name, "story/7-1-integration-tests");
    assert_eq!(story.status, "ready-for-dev");
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["6-1-telegram".to_string(), "6-2-retry".to_string()];
    let story = make_test_story("7-2-config-tests", "config tests", deps);

    assert_eq!(story.dependencies.len(), 2);
    assert_eq!(story.dependencies[0], "6-1-telegram");
    assert_eq!(story.dependencies[1], "6-2-retry");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("3-2-llm-fallback", "llm fallback", vec![]);

    assert_eq!(
        story.specs_path,
        std::path::PathBuf::from("_bmad-output/implementation-artifacts/3-2-llm-fallback.md")
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status tests (Task 7.6)
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli-framework", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];
    write_sprint_status(dir.path(), &entries);

    let path = dir.path().join("sprint-status.yaml");
    assert!(path.exists(), "sprint-status.yaml should exist");

    // Verify the file is parseable by SprintStatusFile
    let parsed = SprintStatusFile::load(&path, dir.path());
    assert!(parsed.is_ok(), "should parse successfully: {:?}", parsed.err());
}

#[test]
fn test_write_sprint_status_contains_all_entries() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-test-story", "ready-for-dev"),
        ("1-2-another", "done"),
    ];
    write_sprint_status(dir.path(), &entries);

    let content = std::fs::read_to_string(dir.path().join("sprint-status.yaml"))
        .expect("read sprint-status.yaml");
    assert!(content.contains("1-1-test-story: ready-for-dev"));
    assert!(content.contains("1-2-another: done"));
    assert!(content.contains("epic-1: in-progress"));
}

#[test]
fn test_write_sprint_status_stories_filtered_correctly() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-a", "ready-for-dev"),
        ("1-2-story-b", "done"),
        ("epic-1-retrospective", "optional"),
    ];
    write_sprint_status(dir.path(), &entries);

    let path = dir.path().join("sprint-status.yaml");
    let status = SprintStatusFile::load(&path, dir.path()).expect("load sprint-status.yaml");
    let stories = status.stories();

    // Only actual stories, not epics or retrospectives
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-story-a");
    assert_eq!(stories[1].story_key, "1-2-story-b");
}

// ---------------------------------------------------------------------------
// write_wal_file tests (Task 7.7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let story = make_test_story("4-2-agent-session", "agent session", vec![]);
    let state = SessionState::new(&story, "anthropic", "claude-sonnet-4-20250514");

    write_wal_file(dir.path(), &state).await;

    let path = dir.path().join(".bmad-bot-session.yaml");
    assert!(path.exists(), "WAL file should exist");

    // Verify it's parseable
    let content = std::fs::read_to_string(&path).expect("read WAL file");
    let parsed: SessionState = serde_yml::from_str(&content).expect("should parse WAL YAML");
    assert_eq!(parsed.story_key, "4-2-agent-session");
    assert_eq!(parsed.provider, "anthropic");
    assert_eq!(parsed.model, "claude-sonnet-4-20250514");
}

#[tokio::test]
async fn test_write_wal_file_preserves_chat_history() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let story = make_test_story("1-1-test", "test", vec![]);
    let mut state = SessionState::new(&story, "openai", "gpt-4o");
    state.add_user_message("hello");
    state.add_assistant_message("world");

    write_wal_file(dir.path(), &state).await;

    let content = std::fs::read_to_string(dir.path().join(".bmad-bot-session.yaml"))
        .expect("read WAL file");
    let parsed: SessionState = serde_yml::from_str(&content).expect("parse WAL YAML");
    assert_eq!(parsed.chat_history.len(), 2);
    assert_eq!(parsed.chat_history[0].role, "user");
    assert_eq!(parsed.chat_history[0].content, "hello");
    assert_eq!(parsed.chat_history[1].role, "assistant");
    assert_eq!(parsed.chat_history[1].content, "world");
}

// ---------------------------------------------------------------------------
// create_test_repo tests (Task 7.8)
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_creates_valid_git_repo() {
    let dir = tempfile::tempdir().expect("create tempdir");
    create_test_repo(dir.path());

    // Verify .git directory exists
    assert!(dir.path().join(".git").exists(), ".git should exist");
}

#[test]
fn test_create_test_repo_has_initial_commit() {
    let dir = tempfile::tempdir().expect("create tempdir");
    create_test_repo(dir.path());

    // Verify HEAD exists by running git log
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
fn test_create_test_repo_main_branch_exists() {
    let dir = tempfile::tempdir().expect("create tempdir");
    create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["branch", "--list", "main"])
        .current_dir(dir.path())
        .output()
        .expect("git branch failed");
    assert!(output.status.success());
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        branches.contains("main"),
        "main branch should exist, got: {branches}"
    );
}
