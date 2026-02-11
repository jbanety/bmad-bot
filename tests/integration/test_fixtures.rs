//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures;

use bmad_bot::session::SessionState;
use bmad_bot::watcher::SprintStatusFile;


// ---------------------------------------------------------------------------
// make_test_config tests (Task 7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_has_sensible_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = fixtures::make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert!(config.code_review_enabled);
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_uses_provided_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = fixtures::make_test_config(dir.path());

    assert_eq!(
        config.bmad_paths.implementation_artifacts,
        dir.path().display().to_string()
    );
}

#[test]
fn test_make_test_config_passes_validation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = fixtures::make_test_config(dir.path());
    // BotConfig::validate checks all required fields
    config.validate().expect("config should be valid");
}

// ---------------------------------------------------------------------------
// make_test_secrets tests (Task 7.5)
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
fn test_make_test_secrets_uses_dummy_values() {
    let secrets = fixtures::make_test_secrets();

    // All keys should contain "DO-NOT-USE" to prevent accidental real API calls
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
// make_test_story tests (Task 7.5)
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key_correctly() {
    let story = fixtures::make_test_story("7-1-integration-test", "integration test", &[]);

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
        "7-2-config-tests",
        "config tests",
        &["7-1-integration-test"],
    );

    assert_eq!(story.dependencies.len(), 1);
    assert_eq!(story.dependencies[0], "7-1-integration-test");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = fixtures::make_test_story("3-1-supervisor", "supervisor", &[]);

    assert_eq!(
        story.specs_path,
        std::path::PathBuf::from("_bmad-output/implementation-artifacts/3-1-supervisor.md")
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status tests (Task 7.6)
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fixtures::write_sprint_status(
        dir.path(),
        &[
            ("epic-1", "in-progress"),
            ("1-1-test-story", "ready-for-dev"),
            ("epic-1-retrospective", "optional"),
        ],
    );

    assert!(path.exists());
    assert!(path.ends_with("sprint-status.yaml"));
}

#[test]
fn test_write_sprint_status_writes_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fixtures::write_sprint_status(
        dir.path(),
        &[
            ("epic-1", "in-progress"),
            ("1-1-test-story", "ready-for-dev"),
            ("1-2-another-story", "backlog"),
            ("epic-1-retrospective", "optional"),
        ],
    );

    // SprintStatusFile::load expects (path, story_dir)
    let sprint = SprintStatusFile::load(&path, dir.path())
        .expect("should parse sprint-status.yaml");

    // Should have story entries (excluding epics and retros)
    let stories = sprint.stories();
    assert_eq!(stories.len(), 2, "Should find 2 story entries");
    assert_eq!(stories[0].story_key, "1-1-test-story");
    assert_eq!(stories[1].story_key, "1-2-another-story");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fixtures::write_sprint_status(
        dir.path(),
        &[
            ("epic-1", "in-progress"),
            ("1-1-slug", "ready-for-dev"),
            ("epic-1-retrospective", "optional"),
        ],
    );

    let sprint = SprintStatusFile::load(&path, dir.path())
        .expect("should parse");
    // entries() returns all entries including epics and retros
    assert_eq!(sprint.entry_count(), 3);
}

#[test]
fn test_write_sprint_status_eligible_stories() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = fixtures::write_sprint_status(
        dir.path(),
        &[
            ("epic-1", "in-progress"),
            ("1-1-slug", "ready-for-dev"),
            ("1-2-slug", "backlog"),
        ],
    );

    let sprint = SprintStatusFile::load(&path, dir.path())
        .expect("should parse");
    let eligible = sprint.eligible_stories();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "1-1-slug");
}

// ---------------------------------------------------------------------------
// write_wal_file tests (Task 7.7)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_write_wal_file_creates_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = fixtures::make_test_story("4-2-session", "session", &[]);
    let state = fixtures::make_session_state(&story);

    let path = fixtures::write_wal_file(dir.path(), &state);

    assert!(path.exists());
    assert!(path.ends_with(".bmad-bot-session.yaml"));
}

#[tokio::test]
async fn test_write_wal_file_writes_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = fixtures::make_test_story("4-2-session", "session", &[]);
    let mut state = fixtures::make_session_state(&story);
    fixtures::add_chat_messages(
        &mut state,
        &[("user", "Hello"), ("assistant", "Hi there!")],
    );

    let path = fixtures::write_wal_file(dir.path(), &state);

    // Read back and verify
    let loaded = SessionState::load(&path)
        .await
        .expect("should load WAL file");

    assert_eq!(loaded.story_id, "4.2");
    assert_eq!(loaded.story_key, "4-2-session");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[0].content, "Hello");
}

#[tokio::test]
async fn test_write_wal_file_with_branch_info() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = fixtures::make_test_story("4-3-branch", "branch", &[]);
    let mut state = fixtures::make_session_state(&story);
    state.branch_name = "story/4-3-branch".into();
    state.base_branch = "main".into();

    let path = fixtures::write_wal_file(dir.path(), &state);

    let loaded = SessionState::load(&path)
        .await
        .expect("should load WAL file");

    assert_eq!(loaded.branch_name, "story/4-3-branch");
    assert_eq!(loaded.base_branch, "main");
}

// ---------------------------------------------------------------------------
// create_test_repo tests (Task 7.8)
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_initializes_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo_path = fixtures::create_test_repo(dir.path());

    assert!(repo_path.join(".git").exists(), ".git directory should exist");
}

#[test]
fn test_create_test_repo_has_head_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo_path = fixtures::create_test_repo(dir.path());

    // Verify HEAD points to a valid commit
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&repo_path)
        .output()
        .expect("git rev-parse HEAD");

    assert!(
        output.status.success(),
        "HEAD should point to a valid commit"
    );
    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(sha.len(), 40, "SHA should be 40 hex characters");
}

#[test]
fn test_create_test_repo_commit_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo_path = fixtures::create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(&repo_path)
        .output()
        .expect("git log");

    let log = String::from_utf8_lossy(&output.stdout);
    assert!(
        log.contains("initial commit"),
        "Should have 'initial commit' message, got: {}",
        log
    );
}
