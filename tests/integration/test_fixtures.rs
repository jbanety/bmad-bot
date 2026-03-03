//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures::*;

use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config tests (Task 7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_creates_valid_config() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config = make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.repo_owner, "test-owner");
    assert_eq!(config.git_provider.repo_name, "test-repo");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert_eq!(config.llm.dev.provider, "anthropic");
    assert_eq!(config.llm.review.provider, "anthropic");
    assert_eq!(config.llm.supervisor.provider, "anthropic");
    assert!(!config.notifications.telegram.enabled);
}

#[test]
fn test_make_test_config_paths_use_provided_dir() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let config = make_test_config(dir.path());

    assert!(
        config
            .bmad_paths
            .project_root
            .contains(dir.path().to_str().unwrap())
    );
    assert!(
        config
            .bmad_paths
            .implementation_artifacts
            .contains("implementation-artifacts")
    );
}

// ---------------------------------------------------------------------------
// make_test_secrets tests (Task 7.5)
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
fn test_make_test_secrets_uses_dummy_values() {
    let secrets = make_test_secrets();

    assert!(
        secrets
            .anthropic_api_key
            .as_ref()
            .unwrap()
            .contains("DO-NOT-USE")
    );
    assert!(
        secrets
            .github_token
            .as_ref()
            .unwrap()
            .contains("DO-NOT-USE")
    );
}

// ---------------------------------------------------------------------------
// make_test_story tests (Task 7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key() {
    let story = make_test_story(
        "7-1-integration-test-infra",
        "integration test infra",
        vec![],
    );

    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test-infra");
    assert_eq!(story.label, "integration test infra");
    assert_eq!(story.branch_name, "story/7-1-integration-test-infra");
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["6-1-telegram".into(), "6-2-retry".into()];
    let story = make_test_story("7-1-test", "test", deps);

    assert_eq!(story.dependencies.len(), 2);
    assert_eq!(story.dependencies[0], "6-1-telegram");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("7-1-test", "test", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/7-1-test.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status tests (Task 7.6)
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_file() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-slug", "ready-for-dev"),
        ("1-2-another-story", "backlog"),
        ("epic-1-retrospective", "optional"),
    ];
    write_sprint_status(dir.path(), entries);

    let path = dir.path().join("sprint-status.yaml");
    assert!(path.exists());
}

#[test]
fn test_write_sprint_status_parseable_yaml() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-slug", "ready-for-dev"),
        ("1-2-another-story", "done"),
        ("epic-1-retrospective", "optional"),
    ];
    write_sprint_status(dir.path(), entries);

    let path = dir.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, dir.path());
    assert!(
        ssf.is_ok(),
        "Failed to parse sprint-status.yaml: {:?}",
        ssf.err()
    );
}

#[test]
fn test_write_sprint_status_stories_extracted() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-slug", "ready-for-dev"),
        ("1-2-another-story", "done"),
        ("epic-1-retrospective", "optional"),
    ];
    write_sprint_status(dir.path(), entries);

    let path = dir.path().join("sprint-status.yaml");
    let ssf = SprintStatusFile::load(&path, dir.path()).expect("Failed to load sprint status");
    let stories = ssf.stories();
    // Only actual stories (not epics or retrospectives)
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-story-slug");
    assert_eq!(stories[0].status, "ready-for-dev");
    assert_eq!(stories[1].story_key, "1-2-another-story");
    assert_eq!(stories[1].status, "done");
}

// ---------------------------------------------------------------------------
// write_wal_file tests (Task 7.7)
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_file() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let state = SessionState {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        last_activity: "2026-01-01T00:01:00Z".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-4-20250514".into(),
        branch_name: "story/1-1-test".into(),
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

    let path = dir.path().join(".bmad-bot-session.yaml");
    assert!(path.exists());
}

#[test]
fn test_write_wal_file_parseable_yaml() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let state = SessionState {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        last_activity: "2026-01-01T00:01:00Z".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-4-20250514".into(),
        branch_name: "story/1-1-test".into(),
        base_branch: "main".into(),
        chat_history: vec![],
    };
    write_wal_file(dir.path(), &state);

    let path = dir.path().join(".bmad-bot-session.yaml");
    let content = std::fs::read_to_string(&path).expect("Failed to read WAL file");
    let parsed: Result<SessionState, _> = serde_yml::from_str(&content);
    assert!(parsed.is_ok(), "WAL YAML not parseable: {:?}", parsed.err());
    let loaded = parsed.unwrap();
    assert_eq!(loaded.story_key, "1-1-test");
    assert_eq!(loaded.provider, "anthropic");
}

#[test]
fn test_write_wal_file_roundtrips_chat_history() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let state = SessionState {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        last_activity: "2026-01-01T00:01:00Z".into(),
        provider: "anthropic".into(),
        model: "test-model".into(),
        branch_name: "story/1-1-test".into(),
        base_branch: "main".into(),
        chat_history: vec![
            ChatMessage {
                role: "user".into(),
                content: "implement feature X".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "done".into(),
            },
        ],
    };
    write_wal_file(dir.path(), &state);

    let path = dir.path().join(".bmad-bot-session.yaml");
    let content = std::fs::read_to_string(&path).expect("Failed to read WAL file");
    let loaded: SessionState = serde_yml::from_str(&content).expect("Failed to parse WAL");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[0].content, "implement feature X");
    assert_eq!(loaded.chat_history[1].role, "assistant");
}

// ---------------------------------------------------------------------------
// create_test_repo tests (Task 7.8)
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_creates_git_dir() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    create_test_repo(dir.path());

    assert!(dir.path().join(".git").exists());
}

#[test]
fn test_create_test_repo_has_head_commit() {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    create_test_repo(dir.path());

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
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git rev-parse failed");
    assert!(output.status.success());
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(branch, "main");
}
