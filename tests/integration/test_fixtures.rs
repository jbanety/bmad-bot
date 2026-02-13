//! Self-verification tests for fixture builders.

use crate::helpers::fixtures::*;

use bmad_bot::session::SessionState;
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config (Task 7.5)
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
// make_test_secrets (Task 7.5)
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
fn test_make_test_secrets_uses_dummy_keys() {
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
// make_test_story (Task 7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key_correctly() {
    let story = make_test_story("7-1-infra", "infra", vec![]);

    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-infra");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "infra");
    assert_eq!(story.branch_name, "story/7-1-infra");
    assert_eq!(story.status, "ready-for-dev");
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["6-1-telegram".to_string(), "6-2-retry".to_string()];
    let story = make_test_story("7-2-config", "config tests", deps.clone());

    assert_eq!(story.dependencies, deps);
    assert_eq!(story.story_id, "7.2");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("8-3-grep", "grep tool", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/8-3-grep.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status (Task 7.6)
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    assert!(path.exists());
}

#[test]
fn test_write_sprint_status_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), &entries);

    // Parse via SprintStatusFile to verify format compatibility
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("should parse");
    let stories = sprint.stories();

    // Only actual stories (not epics or retrospectives)
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[0].status, "done");
    assert_eq!(stories[1].story_key, "1-2-cli");
    assert_eq!(stories[1].status, "ready-for-dev");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-2", "backlog"),
        ("2-1-polling", "ready-for-dev"),
        ("epic-2-retrospective", "optional"),
    ];

    let path = write_sprint_status(dir.path(), &entries);
    let content = std::fs::read_to_string(&path).expect("read");

    assert!(content.contains("epic-2: backlog"));
    assert!(content.contains("2-1-polling: ready-for-dev"));
    assert!(content.contains("epic-2-retrospective: optional"));
}

// ---------------------------------------------------------------------------
// write_wal_file (Task 7.7)
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("4-2-agent", "agent setup", vec![]);
    let state = SessionState::new(&story, "anthropic", "test-model");

    let path = write_wal_file(dir.path(), &state);
    assert!(path.exists());
    assert_eq!(path.file_name().unwrap(), ".bmad-bot-session.yaml");
}

#[test]
fn test_write_wal_file_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("4-2-agent", "agent setup", vec![]);
    let mut state = SessionState::new(&story, "anthropic", "test-model");
    state.set_branch_info("story/4-2-agent", "main");

    let path = write_wal_file(dir.path(), &state);
    let content = std::fs::read_to_string(&path).expect("read");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse YAML");

    assert_eq!(loaded.story_id, "4.2");
    assert_eq!(loaded.story_key, "4-2-agent");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.model, "test-model");
    assert_eq!(loaded.branch_name, "story/4-2-agent");
    assert_eq!(loaded.base_branch, "main");
}

#[test]
fn test_write_wal_file_with_chat_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("6-3-wal", "wal test", vec![]);
    let mut state = SessionState::new(&story, "openai", "gpt-4o");
    state.add_user_message("hello");
    state.add_assistant_message("hi there");

    let path = write_wal_file(dir.path(), &state);
    let content = std::fs::read_to_string(&path).expect("read");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse");

    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[0].content, "hello");
    assert_eq!(loaded.chat_history[1].role, "assistant");
    assert_eq!(loaded.chat_history[1].content, "hi there");
}

// ---------------------------------------------------------------------------
// create_test_repo (Task 7.8)
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_has_head_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // Verify HEAD exists by running git log
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["log", "--oneline", "-1"])
        .output()
        .expect("git log");
    assert!(output.status.success());
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(log.contains("initial commit"));
}

#[test]
fn test_create_test_repo_is_valid_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // Verify it's a valid repo by running git status
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["status", "--porcelain"])
        .output()
        .expect("git status");
    assert!(output.status.success());
}
