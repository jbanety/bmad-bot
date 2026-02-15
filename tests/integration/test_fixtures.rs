//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures::*;
use bmad_bot::session::{ChatMessage, SessionState};

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
fn test_make_test_config_paths_use_provided_dir() {
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
fn test_make_test_secrets_values_are_clearly_test_only() {
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
    let story = make_test_story("7-1-integration-test-infra", "integration test infra", vec![]);

    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test-infra");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "integration test infra");
    assert_eq!(story.branch_name, "story/7-1-integration-test-infra");
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["6-1-some-story".to_string(), "6-2-another".to_string()];
    let story = make_test_story("7-2-config-tests", "config tests", deps);

    assert_eq!(story.dependencies.len(), 2);
    assert_eq!(story.dependencies[0], "6-1-some-story");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("3-1-supervisor", "supervisor", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/3-1-supervisor.md"
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

    let content = std::fs::read_to_string(dir.path().join("sprint-status.yaml"))
        .expect("should be readable");

    // Verify it's valid YAML
    let parsed: serde_yml::Value =
        serde_yml::from_str(&content).expect("should be valid YAML");

    // Check development_status section
    let dev_status = parsed["development_status"].as_mapping().expect("should be a mapping");
    let epic_val = dev_status
        .get(&serde_yml::Value::String("epic-1".into()))
        .expect("epic-1 should exist");
    assert_eq!(epic_val.as_str().unwrap(), "in-progress");

    let story_val = dev_status
        .get(&serde_yml::Value::String("1-1-story-slug".into()))
        .expect("story should exist");
    assert_eq!(story_val.as_str().unwrap(), "ready-for-dev");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-2", "backlog"),
        ("2-1-first-story", "done"),
        ("epic-2-retrospective", "done"),
    ];

    write_sprint_status(dir.path(), &entries);

    let content = std::fs::read_to_string(dir.path().join("sprint-status.yaml"))
        .expect("should be readable");

    assert!(content.contains("epic-2: backlog"));
    assert!(content.contains("2-1-first-story: done"));
    assert!(content.contains("epic-2-retrospective: done"));
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = SessionState {
        story_id: "4.2".to_string(),
        story_key: "4-2-agent-session".to_string(),
        branch: "story/4-2-agent-session".to_string(),
        started_at: "2026-02-08T10:00:00Z".to_string(),
        last_activity: "2026-02-08T10:05:00Z".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        branch_name: "story/4-2-agent-session".to_string(),
        base_branch: "main".to_string(),
        chat_history: vec![
            ChatMessage {
                role: "user".to_string(),
                content: "DS".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Starting implementation".to_string(),
            },
        ],
    };

    write_wal_file(dir.path(), &state);

    // Read back and parse
    let content = std::fs::read_to_string(dir.path().join(".bmad-bot-session.yaml"))
        .expect("should be readable");
    let parsed: SessionState =
        serde_yml::from_str(&content).expect("should deserialize back");

    assert_eq!(parsed.story_id, "4.2");
    assert_eq!(parsed.story_key, "4-2-agent-session");
    assert_eq!(parsed.chat_history.len(), 2);
    assert_eq!(parsed.chat_history[0].role, "user");
    assert_eq!(parsed.chat_history[0].content, "DS");
    assert_eq!(parsed.branch_name, "story/4-2-agent-session");
    assert_eq!(parsed.base_branch, "main");
}

#[test]
fn test_write_wal_file_empty_chat_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = SessionState {
        story_id: "1.1".to_string(),
        story_key: "1-1-test".to_string(),
        branch: "story/1-1-test".to_string(),
        started_at: "2026-02-08T10:00:00Z".to_string(),
        last_activity: "2026-02-08T10:00:00Z".to_string(),
        provider: "anthropic".to_string(),
        model: "test".to_string(),
        branch_name: String::new(),
        base_branch: String::new(),
        chat_history: vec![],
    };

    write_wal_file(dir.path(), &state);

    let content = std::fs::read_to_string(dir.path().join(".bmad-bot-session.yaml"))
        .expect("should be readable");
    let parsed: SessionState =
        serde_yml::from_str(&content).expect("should deserialize");
    assert!(parsed.chat_history.is_empty());
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_initializes_valid_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // Verify .git directory exists
    assert!(dir.path().join(".git").exists());

    // Verify HEAD commit exists
    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(dir.path())
        .output()
        .expect("git log should work");
    assert!(output.status.success());
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(log.contains("initial commit"));
}

#[test]
fn test_create_test_repo_has_main_branch() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .expect("git branch should work");
    assert!(output.status.success());
    let branch = String::from_utf8_lossy(&output.stdout);
    assert_eq!(branch.trim(), "main");
}
