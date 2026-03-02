//! Self-verification tests for fixture builders.

use crate::helpers::fixtures;
use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = fixtures::make_test_config(dir.path());

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

    // Paths should reference the temp directory
    assert!(
        config.bmad_paths.project_root.contains(
            dir.path()
                .to_str()
                .expect("temp path should be valid UTF-8")
        )
    );
}

// ---------------------------------------------------------------------------
// make_test_secrets
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_secrets_has_all_dummy_tokens() {
    let secrets = fixtures::make_test_secrets();

    assert!(secrets.anthropic_api_key.is_some());
    assert!(secrets.openai_api_key.is_some());
    assert!(secrets.github_copilot_oauth_token.is_some());
    assert!(secrets.github_token.is_some());
    assert!(secrets.gitlab_token.is_some());
    assert!(secrets.telegram_bot_token.is_some());

    // All tokens should contain "DO-NOT-USE" safety marker
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
    let story = fixtures::make_test_story("7-1-integration-test", "integration test", vec![]);

    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "integration test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
    assert_eq!(story.status, "ready-for-dev");
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_with_dependencies() {
    let story = fixtures::make_test_story(
        "2-2-dep-resolution",
        "dep resolution",
        vec!["2-1-polling", "1-3-init"],
    );

    assert_eq!(story.dependencies.len(), 2);
    assert_eq!(story.dependencies[0], "2-1-polling");
    assert_eq!(story.dependencies[1], "1-3-init");
}

#[test]
fn test_make_test_story_specs_path_format() {
    let story = fixtures::make_test_story("3-4-decisions", "decisions", vec![]);

    assert_eq!(
        story.specs_path.to_string_lossy(),
        "_bmad-output/implementation-artifacts/3-4-decisions.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status
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

    let path = fixtures::write_sprint_status(dir.path(), entries);

    assert!(path.exists(), "sprint-status.yaml should exist");

    // Verify it's parseable by the actual SprintStatusFile loader
    let content = std::fs::read_to_string(&path).expect("read sprint-status");
    assert!(content.contains("development_status:"));
    assert!(content.contains("1-1-story-slug: ready-for-dev"));
    assert!(content.contains("epic-1: in-progress"));
    assert!(content.contains("epic-1-retrospective: optional"));
}

#[test]
fn test_write_sprint_status_loadable_by_sprint_status_file() {
    let dir = tempfile::tempdir().expect("tempdir");

    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story-slug", "done"),
        ("1-2-another-story", "ready-for-dev"),
    ];

    let path = fixtures::write_sprint_status(dir.path(), entries);

    // Use the actual SprintStatusFile::load to parse
    let loaded = SprintStatusFile::load(&path, dir.path()).expect("SprintStatusFile::load should succeed");
    let stories = loaded.stories();

    // stories() filters out epic and retrospective entries, returns only story entries
    assert_eq!(stories.len(), 2, "Should have 2 story entries");
    assert_eq!(stories[0].story_key, "1-1-story-slug");
    assert_eq!(stories[0].status, "done");
    assert_eq!(stories[1].story_key, "1-2-another-story");
    assert_eq!(stories[1].status, "ready-for-dev");
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");

    let state = SessionState {
        story_id: "4.2".into(),
        story_key: "4-2-agent-session".into(),
        branch: "story/4-2-agent-session".into(),
        started_at: "2026-02-10T10:00:00Z".into(),
        last_activity: "2026-02-10T10:05:00Z".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-4".into(),
        branch_name: "story/4-2-agent-session".into(),
        base_branch: "main".into(),
        chat_history: vec![
            ChatMessage {
                role: "user".into(),
                content: "DS".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Starting implementation...".into(),
            },
        ],
    };

    let path = fixtures::write_wal_file(dir.path(), &state);
    assert!(path.exists(), "WAL file should exist");

    // Read it back and verify
    let content = std::fs::read_to_string(&path).expect("read WAL");
    assert!(content.contains("story_key"));
    assert!(content.contains("4-2-agent-session"));
    assert!(content.contains("provider"));
    assert!(content.contains("anthropic"));

    // Verify it's parseable by serde_yml
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse WAL YAML");
    assert_eq!(loaded.story_key, "4-2-agent-session");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[1].content, "Starting implementation...");
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_creates_valid_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixtures::create_test_repo(dir.path());

    // Verify .git directory exists
    assert!(dir.path().join(".git").exists(), ".git directory should exist");

    // Verify HEAD commit exists
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git rev-parse");
    assert!(
        output.status.success(),
        "HEAD should point to a valid commit"
    );

    // Verify we're on the "main" branch
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .expect("git branch");
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(branch, "main", "Should be on 'main' branch");
}
