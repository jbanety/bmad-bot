//! Self-verification tests for fixture builders.

use crate::helpers::fixtures;
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
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");

    // Validate passes
    config.validate().expect("config should validate");
}

#[test]
fn test_make_test_config_uses_temp_dir_paths() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = fixtures::make_test_config(dir.path());

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
fn test_make_test_secrets_has_all_fields() {
    let secrets = fixtures::make_test_secrets();

    assert!(secrets.anthropic_api_key.is_some());
    assert!(secrets.openai_api_key.is_some());
    assert!(secrets.github_copilot_oauth_token.is_some());
    assert!(secrets.github_token.is_some());
    assert!(secrets.gitlab_token.is_some());
    assert!(secrets.telegram_bot_token.is_some());
}

#[test]
fn test_make_test_secrets_uses_dummy_tokens() {
    let secrets = fixtures::make_test_secrets();

    let anthropic = secrets.anthropic_api_key.unwrap();
    assert!(anthropic.contains("DO-NOT-USE"), "token should be marked as test-only");
}

// ---------------------------------------------------------------------------
// make_test_story tests
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
}

#[test]
fn test_make_test_story_with_dependencies() {
    let story = fixtures::make_test_story(
        "2-3-cascade",
        "cascade",
        vec!["2-1-first".into(), "2-2-second".into()],
    );

    assert_eq!(story.dependencies.len(), 2);
    assert_eq!(story.dependencies[0], "2-1-first");
    assert_eq!(story.dependencies[1], "2-2-second");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = fixtures::make_test_story("1-1-test", "test", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/1-1-test.md"
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
        ("1-1-test-story", "ready-for-dev"),
        ("1-2-another-story", "done"),
        ("epic-1-retrospective", "optional"),
    ];

    fixtures::write_sprint_status(dir.path(), &entries);

    let file_path = dir.path().join("sprint-status.yaml");
    assert!(file_path.exists(), "sprint-status.yaml should exist");

    // Parse with SprintStatusFile::load
    let sprint_status = SprintStatusFile::load(&file_path, dir.path())
        .expect("should parse sprint status");
    let stories = sprint_status.stories();
    assert_eq!(stories.len(), 2, "should have 2 stories (epics/retros filtered)");
    assert_eq!(stories[0].story_key, "1-1-test-story");
    assert_eq!(stories[0].status, "ready-for-dev");
    assert_eq!(stories[1].story_key, "1-2-another-story");
    assert_eq!(stories[1].status, "done");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-2", "backlog"),
        ("2-1-story-a", "in-progress"),
        ("epic-2-retrospective", "optional"),
    ];

    fixtures::write_sprint_status(dir.path(), &entries);

    let content =
        std::fs::read_to_string(dir.path().join("sprint-status.yaml")).expect("read file");
    assert!(content.contains("epic-2: backlog"));
    assert!(content.contains("2-1-story-a: in-progress"));
    assert!(content.contains("epic-2-retrospective: optional"));
}

#[test]
fn test_write_sprint_status_entry_count() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-a", "done"),
        ("1-2-b", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    fixtures::write_sprint_status(dir.path(), &entries);

    let file_path = dir.path().join("sprint-status.yaml");
    let sprint_status =
        SprintStatusFile::load(&file_path, dir.path()).expect("should parse");
    assert_eq!(sprint_status.entry_count(), 4);
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = fixtures::make_test_story("3-1-wal-test", "wal test", vec![]);

    let state = bmad_bot::session::SessionState::new(&story, "anthropic", "claude-sonnet-4-20250514");

    fixtures::write_wal_file(dir.path(), &state);

    let file_path = dir.path().join(".bmad-bot-session.yaml");
    assert!(file_path.exists(), "WAL file should exist");

    // Load it back
    let loaded = bmad_bot::session::SessionState::load(&file_path)
        .await
        .expect("should load WAL file");
    assert_eq!(loaded.story_key, "3-1-wal-test");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.model, "claude-sonnet-4-20250514");
    assert!(loaded.chat_history.is_empty());
}

#[test]
fn test_write_wal_file_roundtrip_with_messages() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = fixtures::make_test_story("4-2-chat", "chat", vec![]);

    let mut state = bmad_bot::session::SessionState::new(&story, "openai", "gpt-4o");
    state.chat_history.push(ChatMessage {
        role: "user".into(),
        content: "Hello".into(),
    });
    state.chat_history.push(ChatMessage {
        role: "assistant".into(),
        content: "Hi there!".into(),
    });

    fixtures::write_wal_file(dir.path(), &state);

    // Read raw YAML and verify structure
    let content =
        std::fs::read_to_string(dir.path().join(".bmad-bot-session.yaml")).expect("read WAL");
    assert!(content.contains("4-2-chat"), "YAML should contain story key. Content: {content}");
    assert!(content.contains("role: user") || content.contains("role: \"user\""), "YAML should contain user role");
    assert!(content.contains("Hello"), "YAML should contain message content");
}

use bmad_bot::session::ChatMessage;

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_produces_valid_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixtures::create_test_repo(dir.path());

    // Verify .git directory exists
    assert!(dir.path().join(".git").exists(), ".git should exist");

    // Verify HEAD commit exists via git log
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
fn test_create_test_repo_has_main_branch() {
    let dir = tempfile::tempdir().expect("tempdir");
    fixtures::create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .expect("git branch failed");
    assert!(output.status.success());
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(branch, "main");
}
