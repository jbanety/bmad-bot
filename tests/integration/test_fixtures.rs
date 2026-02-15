//! Self-verification tests for fixture builders.

use crate::helpers::fixtures::*;
use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_has_sensible_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert!(config.code_review_enabled);
    assert_eq!(config.git_provider.target_branch, "main");
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_paths_use_provided_dir() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path());

    assert!(config.bmad_paths.project_root.contains(dir.path().to_str().unwrap()));
    assert!(config.bmad_paths.output_folder.contains("_bmad-output"));
    assert!(config
        .bmad_paths
        .implementation_artifacts
        .contains("implementation-artifacts"));
}

#[test]
fn test_make_test_config_validates_successfully() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path());
    assert!(config.validate().is_ok());
}

// ---------------------------------------------------------------------------
// make_test_secrets tests
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

    // Verify they contain DO-NOT-USE markers
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
    let story = make_test_story("7-1-integration-test", "integration test", vec![]);

    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "integration test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["7-1-infra".to_string()];
    let story = make_test_story("7-2-tests", "tests", deps);
    assert_eq!(story.dependencies.len(), 1);
    assert_eq!(story.dependencies[0], "7-1-infra");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("1-1-scaffolding", "scaffolding", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/1-1-scaffolding.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-slug", "ready-for-dev"),
        ("1-2-another-story", "done"),
        ("epic-1-retrospective", "optional"),
    ];
    write_sprint_status(dir.path(), &entries);

    let yaml_path = dir.path().join("sprint-status.yaml");
    assert!(yaml_path.exists());

    // Verify it's valid YAML that SprintStatusFile can load
    let ssf = SprintStatusFile::load(&yaml_path, dir.path()).unwrap();
    let stories = ssf.stories();
    assert_eq!(stories.len(), 2); // Only story entries, not epics/retros
    assert_eq!(stories[0].story_key, "1-1-story-slug");
    assert_eq!(stories[0].status, "ready-for-dev");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().unwrap();
    let entries = vec![
        ("epic-2", "backlog"),
        ("2-1-watcher", "in-progress"),
        ("epic-2-retrospective", "optional"),
    ];
    write_sprint_status(dir.path(), &entries);

    let yaml_path = dir.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&yaml_path, dir.path()).unwrap();

    // entries() includes all types
    assert_eq!(ssf.entry_count(), 3);

    // stories() filters to story entries only
    let stories = ssf.stories();
    assert_eq!(stories.len(), 1);
    assert_eq!(stories[0].story_key, "2-1-watcher");
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let state = SessionState {
        story_id: "4.2".into(),
        story_key: "4-2-agent-session".into(),
        branch: "story/4-2-agent-session".into(),
        started_at: "2026-02-08T12:00:00Z".into(),
        last_activity: "2026-02-08T12:30:00Z".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-4-20250514".into(),
        branch_name: "story/4-2-agent-session".into(),
        base_branch: "main".into(),
        chat_history: vec![
            ChatMessage {
                role: "user".into(),
                content: "hello".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "hi".into(),
            },
        ],
    };
    write_wal_file(dir.path(), &state);

    let wal_path = dir.path().join(".bmad-bot-session.yaml");
    assert!(wal_path.exists());

    // Verify it round-trips
    let content = std::fs::read_to_string(&wal_path).unwrap();
    let loaded: SessionState = serde_yml::from_str(&content).unwrap();
    assert_eq!(loaded.story_id, "4.2");
    assert_eq!(loaded.story_key, "4-2-agent-session");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[0].content, "hello");
    assert_eq!(loaded.branch_name, "story/4-2-agent-session");
    assert_eq!(loaded.base_branch, "main");
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_creates_valid_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    create_test_repo(dir.path());

    // Verify .git directory exists
    assert!(dir.path().join(".git").exists());

    // Verify HEAD commit exists
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "HEAD commit should exist");

    // Verify we're on "main" branch
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(branch, "main");
}

#[test]
fn test_create_test_repo_has_commit_message() {
    let dir = tempfile::tempdir().unwrap();
    create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(log.contains("initial commit"));
}
