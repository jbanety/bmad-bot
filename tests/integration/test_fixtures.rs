//! Self-verification tests for fixture builders.

use bmad_bot::session::SessionState;
use bmad_bot::watcher::SprintStatusFile;

use crate::helpers::fixtures::*;

// -----------------------------------------------------------------------
// Task 7.5 — Fixture builders produce valid data structures
// -----------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert!(config.code_review_enabled);
    assert_eq!(config.git_provider.target_branch, "main");
    assert_eq!(config.llm.dev.provider, "anthropic");
    assert_eq!(config.log_format, "pretty");
    assert!(!config.notifications.telegram.enabled);
}

#[test]
fn test_make_test_config_uses_provided_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());
    let dir_str = dir.path().display().to_string();

    assert_eq!(config.bmad_paths.project_root, dir_str);
    assert_eq!(config.bmad_paths.implementation_artifacts, dir_str);
}

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
fn test_make_test_secrets_uses_dummy_values() {
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
    assert!(story.dependencies.is_empty());
}

#[test]
fn test_make_test_story_with_dependencies() {
    let story = make_test_story(
        "2-3-cascade",
        "cascade blocking",
        vec!["2-1-polling".into(), "2-2-deps".into()],
    );

    assert_eq!(story.dependencies.len(), 2);
    assert_eq!(story.dependencies[0], "2-1-polling");
    assert_eq!(story.dependencies[1], "2-2-deps");
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("3-1-supervisor", "supervisor", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/3-1-supervisor.md"
    );
}

// -----------------------------------------------------------------------
// Task 7.6 — write_sprint_status writes parseable YAML
// -----------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sprint_status(
        dir.path(),
        &[
            ("epic-1", "in-progress"),
            ("1-1-story-slug", "ready-for-dev"),
            ("1-2-another-story", "backlog"),
            ("epic-1-retrospective", "optional"),
        ],
    );

    let path = dir.path().join("sprint-status.yaml");
    assert!(path.exists());

    // Parse with the real SprintStatusFile::load
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("load should succeed");
    let stories = sprint.stories();

    // Should only include actual stories (not epics or retrospectives)
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-story-slug");
    assert_eq!(stories[0].status, "ready-for-dev");
    assert_eq!(stories[1].story_key, "1-2-another-story");
    assert_eq!(stories[1].status, "backlog");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sprint_status(
        dir.path(),
        &[
            ("epic-7", "in-progress"),
            ("7-1-fixtures", "ready-for-dev"),
            ("7-2-config", "blocked"),
            ("epic-7-retrospective", "optional"),
        ],
    );

    let path = dir.path().join("sprint-status.yaml");
    let sprint = SprintStatusFile::load(&path, dir.path()).expect("load should succeed");

    // entries() returns all entries including epics and retros
    assert_eq!(sprint.entry_count(), 4);

    // stories() filters to just stories
    let stories = sprint.stories();
    assert_eq!(stories.len(), 2);
}

// -----------------------------------------------------------------------
// Task 7.7 — write_wal_file writes parseable WAL YAML
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = make_test_session_state("1-1-test-story");

    write_wal_file(dir.path(), &state);

    let path = dir.path().join(".bmad-bot-session.yaml");
    assert!(path.exists());

    // Parse with the real SessionState::load
    let loaded = SessionState::load(&path).await.expect("load should succeed");
    assert_eq!(loaded.story_key, "1-1-test-story");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[0].content, "Hello");
    assert_eq!(loaded.chat_history[1].role, "assistant");
    assert_eq!(loaded.branch_name, "story/1-1-test-story");
    assert_eq!(loaded.base_branch, "main");
}

// -----------------------------------------------------------------------
// Task 7.8 — create_test_repo creates a valid git repo
// -----------------------------------------------------------------------

#[test]
fn test_create_test_repo_initializes_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // Verify .git directory exists
    assert!(dir.path().join(".git").exists());
}

#[test]
fn test_create_test_repo_has_head_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // Verify HEAD points to a commit
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git rev-parse failed");

    assert!(output.status.success(), "HEAD should point to a commit");
    let sha = String::from_utf8_lossy(&output.stdout);
    assert_eq!(sha.trim().len(), 40, "SHA should be 40 hex chars");
}

#[test]
fn test_create_test_repo_has_main_branch() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .expect("git branch failed");

    assert!(output.status.success());
    let branch = String::from_utf8_lossy(&output.stdout);
    assert_eq!(branch.trim(), "main");
}
