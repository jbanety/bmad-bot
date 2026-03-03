//! Self-verification tests for fixture builders.

use super::helpers::fixtures::*;

use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// 7.5 — Fixture builders produce valid data structures
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());
    // Validate passes without error
    config.validate().expect("Config should be valid");
    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert!(config.code_review_enabled);
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_paths_use_provided_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(dir.path());
    let dir_str = dir.path().display().to_string();
    assert_eq!(config.bmad_paths.project_root, dir_str);
    assert_eq!(config.bmad_paths.output_folder, dir_str);
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
fn test_make_test_secrets_tokens_contain_do_not_use() {
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
    let story = make_test_story(
        "7-1-integration-test-infrastructure",
        "integration test infrastructure",
        vec!["6-4-context-window".into()],
    );
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test-infrastructure");
    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.label, "integration test infrastructure");
    assert_eq!(
        story.branch_name,
        "story/7-1-integration-test-infrastructure"
    );
    assert_eq!(
        story.specs_path.to_str().unwrap(),
        "_bmad-output/implementation-artifacts/7-1-integration-test-infrastructure.md"
    );
    assert_eq!(story.dependencies, vec!["6-4-context-window".to_string()]);
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_empty_deps() {
    let story = make_test_story("1-1-hello", "hello", vec![]);
    assert!(story.dependencies.is_empty());
    assert_eq!(story.epic_num, 1);
    assert_eq!(story.story_num, 1);
}

// ---------------------------------------------------------------------------
// 7.6 — write_sprint_status writes parseable YAML
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sprint_status(
        dir.path(),
        vec![
            ("epic-1", "in-progress"),
            ("1-1-story-slug", "ready-for-dev"),
            ("1-2-another-story", "backlog"),
            ("epic-1-retrospective", "optional"),
        ],
    );
    let path = dir.path().join("sprint-status.yaml");
    assert!(path.exists(), "sprint-status.yaml should exist");

    // Parse with SprintStatusFile::load
    let sprint = SprintStatusFile::load(&path, dir.path())
        .expect("Should parse sprint-status.yaml");
    let stories = sprint.stories();
    assert_eq!(stories.len(), 2, "Should have 2 story entries (excludes epics and retros)");
    assert_eq!(stories[0].story_key, "1-1-story-slug");
    assert_eq!(stories[0].status, "ready-for-dev");
    assert_eq!(stories[1].story_key, "1-2-another-story");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let dir = tempfile::tempdir().expect("tempdir");
    write_sprint_status(
        dir.path(),
        vec![
            ("epic-2", "backlog"),
            ("2-1-first", "done"),
            ("epic-2-retrospective", "optional"),
        ],
    );
    let path = dir.path().join("sprint-status.yaml");
    let sprint = SprintStatusFile::load(&path, dir.path())
        .expect("Should parse");
    // entries() includes all entries (epics, stories, retros)
    assert_eq!(sprint.entry_count(), 3);
    // stories() filters to only story entries
    assert_eq!(sprint.stories().len(), 1);
}

// ---------------------------------------------------------------------------
// 7.7 — write_wal_file writes parseable WAL YAML
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let dir = tempfile::tempdir().expect("tempdir");
    let story = make_test_story("4-2-session-setup", "session setup", vec![]);
    let state = SessionState {
        story_id: story.story_id.clone(),
        story_key: story.story_key.clone(),
        branch: story.branch_name.clone(),
        started_at: "2026-02-08T10:00:00Z".into(),
        last_activity: "2026-02-08T10:05:00Z".into(),
        provider: "anthropic".into(),
        model: "test-model".into(),
        branch_name: story.branch_name.clone(),
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
    write_wal_file(dir.path(), &state);

    let wal_path = dir.path().join(".bmad-bot-session.yaml");
    assert!(wal_path.exists(), "WAL file should exist");

    // Read back and verify
    let content = std::fs::read_to_string(&wal_path).expect("read WAL");
    let loaded: SessionState =
        serde_yml::from_str(&content).expect("Should parse WAL YAML");
    assert_eq!(loaded.story_id, "4.2");
    assert_eq!(loaded.story_key, "4-2-session-setup");
    assert_eq!(loaded.provider, "anthropic");
    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[0].content, "DS");
    assert_eq!(loaded.base_branch, "main");
}

#[test]
fn test_write_wal_file_empty_chat_history() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = SessionState {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        last_activity: "2026-01-01T00:00:00Z".into(),
        provider: "openai".into(),
        model: "gpt-4o".into(),
        branch_name: String::new(),
        base_branch: String::new(),
        chat_history: vec![],
    };
    write_wal_file(dir.path(), &state);

    let content =
        std::fs::read_to_string(dir.path().join(".bmad-bot-session.yaml")).expect("read");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse");
    assert!(loaded.chat_history.is_empty());
}

// ---------------------------------------------------------------------------
// 7.8 — create_test_repo creates a valid git repo with HEAD commit
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_initializes_git_repo() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    // .git directory exists
    assert!(
        dir.path().join(".git").exists(),
        ".git directory should exist"
    );

    // HEAD exists and points to a commit
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git rev-parse failed");
    assert!(output.status.success(), "HEAD should be a valid commit");

    // Branch is "main"
    let branch_output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir.path())
        .output()
        .expect("git branch failed");
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();
    assert_eq!(branch, "main", "Default branch should be 'main'");
}

#[test]
fn test_create_test_repo_has_initial_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    create_test_repo(dir.path());

    let output = std::process::Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(dir.path())
        .output()
        .expect("git log failed");
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(
        log.contains("initial commit"),
        "Should have initial commit message"
    );
}
