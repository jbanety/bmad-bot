//! Self-verification tests for fixture builders.

use crate::helpers::fixtures::*;

use bmad_bot::session::ChatMessage;
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config tests
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let dir = tempfile::tempdir().unwrap();
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
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_paths_use_provided_dir() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path());

    assert!(config.bmad_paths.project_root.contains(dir.path().to_str().unwrap()));
    assert!(config
        .bmad_paths
        .implementation_artifacts
        .contains("implementation-artifacts"));
}

#[test]
fn test_make_test_config_passes_validation() {
    let dir = tempfile::tempdir().unwrap();
    let config = make_test_config(dir.path());
    // BotConfig::validate() checks all required fields
    config.validate().expect("Config should pass validation");
}

// ---------------------------------------------------------------------------
// make_test_secrets tests
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
fn test_make_test_secrets_tokens_are_dummy() {
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
    let story = make_test_story("7-1-integration-test", "integration test", vec![]);

    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.label, "integration test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
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
    let story = make_test_story("1-3-init-cmd", "init cmd", vec![]);
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/1-3-init-cmd.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status tests
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sprint_status(
        dir.path(),
        vec![
            ("epic-1", "in-progress"),
            ("1-1-story-slug", "ready-for-dev"),
            ("1-2-another-story", "backlog"),
            ("epic-1-retrospective", "optional"),
        ],
    );

    assert!(path.exists());

    // Verify it's parseable by SprintStatusFile::load
    let sprint = SprintStatusFile::load(&path, dir.path())
        .expect("Sprint status should be parseable");

    // Should have stories (filtering out epics and retros)
    let stories = sprint.stories();
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-story-slug");
    assert_eq!(stories[0].status, "ready-for-dev");
    assert_eq!(stories[1].story_key, "1-2-another-story");
    assert_eq!(stories[1].status, "backlog");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sprint_status(
        dir.path(),
        vec![
            ("epic-1", "in-progress"),
            ("1-1-a", "done"),
            ("epic-1-retrospective", "optional"),
            ("epic-2", "backlog"),
            ("2-1-b", "ready-for-dev"),
        ],
    );

    let sprint = SprintStatusFile::load(&path, dir.path()).unwrap();

    // entries() returns ALL entries including epics and retros
    let entries = sprint.entries();
    assert_eq!(entries.len(), 5);

    // stories() filters to only actual story entries
    let stories = sprint.stories();
    assert_eq!(stories.len(), 2);
}

#[test]
fn test_write_sprint_status_eligible_stories() {
    let dir = tempfile::tempdir().unwrap();
    write_sprint_status(
        dir.path(),
        vec![
            ("epic-1", "in-progress"),
            ("1-1-a", "done"),
            ("1-2-b", "ready-for-dev"),
            ("1-3-c", "backlog"),
        ],
    );

    let path = dir.path().join("sprint-status.yaml");
    let sprint = SprintStatusFile::load(&path, dir.path()).unwrap();

    let eligible = sprint.eligible_stories();
    assert_eq!(eligible.len(), 1);
    assert_eq!(eligible[0].story_key, "1-2-b");
}

// ---------------------------------------------------------------------------
// write_wal_file tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_test_session_state("4-2-agent-session");

    let path = write_wal_file(dir.path(), &state);
    assert!(path.exists());

    // Verify it's parseable by SessionState::load
    let loaded = bmad_bot::session::SessionState::load(&path)
        .await
        .expect("WAL file should be parseable");

    assert_eq!(loaded.story_key, "4-2-agent-session");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[1].role, "assistant");
}

#[test]
fn test_write_wal_file_contains_expected_fields() {
    let dir = tempfile::tempdir().unwrap();
    let state = make_test_session_state("3-1-supervisor");

    let path = write_wal_file(dir.path(), &state);
    let content = std::fs::read_to_string(&path).unwrap();

    assert!(content.contains("3-1-supervisor"), "should contain story key: {content}");
    assert!(content.contains("3.1") || content.contains("'3.1'") || content.contains("\"3.1\""), "should contain story_id: {content}");
    assert!(content.contains("anthropic"), "should contain provider: {content}");
    assert!(content.contains("branch_name"), "should contain branch_name: {content}");
    assert!(content.contains("base_branch"), "should contain base_branch: {content}");
    assert!(content.contains("main"), "should contain main: {content}");
}

#[test]
fn test_make_test_session_state_builds_correctly() {
    let state = make_test_session_state("5-1-git-provider");

    assert_eq!(state.story_key, "5-1-git-provider");
    assert_eq!(state.story_id, "5.1");
    assert_eq!(state.branch, "story/5-1-git-provider");
    assert_eq!(state.branch_name, "story/5-1-git-provider");
    assert_eq!(state.base_branch, "main");
    assert_eq!(state.provider, "anthropic");
    assert_eq!(state.chat_history.len(), 2);
    assert_eq!(
        state.chat_history[0],
        ChatMessage {
            role: "user".to_string(),
            content: "Implement the feature".to_string(),
        }
    );
}

// ---------------------------------------------------------------------------
// create_test_repo tests
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_creates_valid_git_repo() {
    let dir = tempfile::tempdir().unwrap();
    create_test_repo(dir.path());

    // Should have a .git directory
    assert!(dir.path().join(".git").exists());
}

#[test]
fn test_create_test_repo_has_initial_commit() {
    let dir = tempfile::tempdir().unwrap();
    create_test_repo(dir.path());

    // HEAD should exist and point to a valid commit
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
    let dir = tempfile::tempdir().unwrap();
    create_test_repo(dir.path());

    // "main" branch should exist
    let output = std::process::Command::new("git")
        .args(["branch", "--list", "main"])
        .current_dir(dir.path())
        .output()
        .expect("git branch failed");
    assert!(output.status.success());
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(branches.contains("main"));
}
