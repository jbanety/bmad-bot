//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures::*;

use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert_eq!(config.log_format, "json");
    assert_eq!(config.log_level, "info");
    assert!(config.mcp_servers.is_empty());
}

#[test]
fn test_make_test_config_uses_provided_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());

    assert!(config.bmad_paths.project_root.contains(
        &tmp.path().to_string_lossy().to_string()
    ));
    assert!(config.bmad_paths.output_folder.contains("_bmad-output"));
}

#[test]
fn test_make_test_config_validates() {
    let tmp = tempfile::tempdir().unwrap();
    let config = make_test_config(tmp.path());
    // BotConfig::validate checks provider names, log levels, etc.
    config.validate().expect("Config should be valid");
}

// ---------------------------------------------------------------------------
// make_test_secrets tests
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
fn test_make_test_secrets_contains_do_not_use_markers() {
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
    let story = make_test_story("7-1-integration-test", "Integration Test", vec![]);

    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "Integration Test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_handles_dependencies() {
    let deps = vec!["6-1-notifications".to_string(), "5-1-pr-creation".to_string()];
    let story = make_test_story("7-2-watcher-tests", "Watcher Tests", deps.clone());

    assert_eq!(story.dependencies, deps);
}

#[test]
fn test_make_test_story_specs_path_follows_convention() {
    let story = make_test_story("7-1-integration-test", "Integration Test", vec![]);

    assert_eq!(
        story.specs_path.to_string_lossy(),
        "_bmad-output/implementation-artifacts/7-1-integration-test.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-first-story", "ready-for-dev"),
        ("1-2-second-story", "backlog"),
        ("epic-1-retrospective", "optional"),
    ];

    write_sprint_status(tmp.path(), &entries);

    let path = tmp.path().join("sprint-status.yaml");
    assert!(path.exists(), "sprint-status.yaml should be created");

    // Verify the file is valid YAML by loading it with SprintStatusFile
    let loaded = SprintStatusFile::load(&path, tmp.path())
        .expect("sprint-status.yaml should be parseable");

    let stories = loaded.stories();
    // stories() filters out non-story entries (epics, retrospectives)
    assert!(
        stories.iter().any(|s| s.story_key == "1-1-first-story"),
        "Should contain first story"
    );
}

#[test]
fn test_write_sprint_status_contains_all_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-7", "in-progress"),
        ("7-1-integration-test", "ready-for-dev"),
    ];

    write_sprint_status(tmp.path(), &entries);

    let content =
        std::fs::read_to_string(tmp.path().join("sprint-status.yaml")).unwrap();
    assert!(content.contains("epic-7: in-progress"));
    assert!(content.contains("7-1-integration-test: ready-for-dev"));
    assert!(content.contains("development_status:"));
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let tmp = tempfile::tempdir().unwrap();
    let state = SessionState {
        story_id: "7.1".to_string(),
        story_key: "7-1-integration-test".to_string(),
        branch: "story/7-1-integration-test".to_string(),
        started_at: "2026-02-08T10:00:00Z".to_string(),
        last_activity: "2026-02-08T10:05:00Z".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        branch_name: "story/7-1-integration-test".to_string(),
        base_branch: "main".to_string(),
        chat_history: vec![
            ChatMessage {
                role: "user".to_string(),
                content: "Start working on story 7.1".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "I'll begin implementing the test infrastructure.".to_string(),
            },
        ],
    };

    write_wal_file(tmp.path(), &state);

    let path = tmp.path().join(".bmad-bot-session.yaml");
    assert!(path.exists(), "WAL file should be created");

    // Verify roundtrip by reading it back
    let content = std::fs::read_to_string(&path).unwrap();
    let loaded: SessionState = serde_yml::from_str(&content)
        .expect("WAL file should be parseable YAML");

    assert_eq!(loaded.story_id, "7.1");
    assert_eq!(loaded.story_key, "7-1-integration-test");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.branch_name, "story/7-1-integration-test");
    assert_eq!(loaded.base_branch, "main");
}

#[test]
fn test_write_wal_file_empty_history() {
    let tmp = tempfile::tempdir().unwrap();
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

    write_wal_file(tmp.path(), &state);

    let content = std::fs::read_to_string(tmp.path().join(".bmad-bot-session.yaml")).unwrap();
    let loaded: SessionState = serde_yml::from_str(&content).unwrap();
    assert!(loaded.chat_history.is_empty());
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_creates_valid_git_repo() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_repo(tmp.path());

    // Verify .git directory exists
    assert!(tmp.path().join(".git").exists(), "Should have .git directory");
}

#[test]
fn test_create_test_repo_has_head_commit() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_repo(tmp.path());

    // Verify HEAD exists and has a commit
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .expect("git rev-parse should work");

    assert!(
        output.status.success(),
        "HEAD should resolve to a commit"
    );
    let sha = String::from_utf8_lossy(&output.stdout);
    assert_eq!(sha.trim().len(), 40, "Should be a valid 40-char SHA");
}

#[test]
fn test_create_test_repo_has_main_branch() {
    let tmp = tempfile::tempdir().unwrap();
    create_test_repo(tmp.path());

    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(tmp.path())
        .output()
        .expect("git branch should work");

    assert!(output.status.success());
    let branch = String::from_utf8_lossy(&output.stdout);
    assert_eq!(branch.trim(), "main", "Default branch should be 'main'");
}
