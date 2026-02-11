//! Self-verification tests for fixture builder functions.

use crate::helpers::fixtures::*;

use bmad_bot::session::state::SessionState;
use bmad_bot::watcher::SprintStatusFile;

// ---------------------------------------------------------------------------
// make_test_config
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_config_produces_valid_config() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(tmp.path());

    assert_eq!(config.polling_interval_secs, 60);
    assert_eq!(config.git_provider.provider, "github");
    assert_eq!(config.git_provider.target_branch, "main");
    assert!(config.code_review_enabled);
    assert_eq!(config.llm.dev.provider, "anthropic");
    assert_eq!(config.log_format, "pretty");
    assert_eq!(config.log_level, "info");
}

#[test]
fn test_make_test_config_uses_provided_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(tmp.path());

    assert!(
        config.bmad_paths.implementation_artifacts.contains(
            &tmp.path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        )
    );
}

#[test]
fn test_make_test_config_passes_validation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = make_test_config(tmp.path());
    // BotConfig::validate checks non-empty fields, valid providers, etc.
    config.validate().expect("config should be valid");
}

// ---------------------------------------------------------------------------
// make_test_secrets
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
fn test_make_test_secrets_contain_do_not_use_marker() {
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
// make_test_story
// ---------------------------------------------------------------------------

#[test]
fn test_make_test_story_parses_key() {
    let story = make_test_story("7-1-integration-test", "", vec![]);

    assert_eq!(story.epic_num, 7);
    assert_eq!(story.story_num, 1);
    assert_eq!(story.story_id, "7.1");
    assert_eq!(story.story_key, "7-1-integration-test");
    assert_eq!(story.branch_name, "story/7-1-integration-test");
    assert_eq!(story.label, "integration test");
    assert_eq!(story.status, "ready-for-dev");
}

#[test]
fn test_make_test_story_uses_custom_label() {
    let story = make_test_story("1-2-cli", "CLI Framework", vec![]);
    assert_eq!(story.label, "CLI Framework");
}

#[test]
fn test_make_test_story_with_dependencies() {
    let deps = vec!["1-1-scaffolding".into(), "1-2-cli".into()];
    let story = make_test_story("1-3-init", "init", deps.clone());
    assert_eq!(story.dependencies, deps);
}

#[test]
fn test_make_test_story_specs_path() {
    let story = make_test_story("3-2-llm-fallback", "", vec![]);
    assert_eq!(
        story.specs_path.to_string_lossy(),
        "_bmad-output/implementation-artifacts/3-2-llm-fallback.md"
    );
}

// ---------------------------------------------------------------------------
// write_sprint_status
// ---------------------------------------------------------------------------

#[test]
fn test_write_sprint_status_creates_parseable_yaml() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-scaffolding", "done"),
        ("1-2-cli", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    write_sprint_status(tmp.path(), &entries);

    let path = tmp.path().join("sprint-status.yaml");
    assert!(path.exists(), "sprint-status.yaml should exist");

    // Verify it parses via SprintStatusFile::load
    let sprint = SprintStatusFile::load(&path, tmp.path()).expect("should parse");
    let stories = sprint.stories();

    // stories() filters out epics and retrospectives
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "1-1-scaffolding");
    assert_eq!(stories[0].status, "done");
    assert_eq!(stories[1].story_key, "1-2-cli");
    assert_eq!(stories[1].status, "ready-for-dev");
}

#[test]
fn test_write_sprint_status_includes_all_entry_types() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let entries = vec![
        ("epic-1", "in-progress"),
        ("1-1-story", "ready-for-dev"),
        ("epic-1-retrospective", "optional"),
    ];

    write_sprint_status(tmp.path(), &entries);

    let path = tmp.path().join("sprint-status.yaml");
    let sprint = SprintStatusFile::load(&path, tmp.path()).expect("should parse");

    // entries() returns ALL entries including epics and retros
    assert_eq!(sprint.entry_count(), 3);
}

#[test]
fn test_write_sprint_status_empty_entries() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_sprint_status(tmp.path(), &[]);

    let path = tmp.path().join("sprint-status.yaml");
    assert!(path.exists());
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

#[test]
fn test_write_wal_file_creates_parseable_yaml() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = SessionState {
        story_id: "4.2".into(),
        story_key: "4-2-agent-session".into(),
        branch: "story/4-2-agent-session".into(),
        started_at: "2026-02-10T10:00:00Z".into(),
        last_activity: "2026-02-10T10:30:00Z".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-4-20250514".into(),
        branch_name: "story/4-2-agent-session".into(),
        base_branch: "main".into(),
        chat_history: vec![],
    };

    write_wal_file(tmp.path(), &state);

    let path = tmp.path().join(".bmad-bot-session.yaml");
    assert!(path.exists(), "WAL file should exist");

    // Verify roundtrip
    let content = std::fs::read_to_string(&path).expect("read WAL");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse WAL");
    assert_eq!(loaded.story_key, "4-2-agent-session");
    assert_eq!(loaded.branch_name, "story/4-2-agent-session");
    assert_eq!(loaded.base_branch, "main");
}

#[test]
fn test_write_wal_file_with_chat_history() {
    use bmad_bot::session::state::ChatMessage;

    let tmp = tempfile::tempdir().expect("tempdir");
    let state = SessionState {
        story_id: "1.1".into(),
        story_key: "1-1-test".into(),
        branch: "story/1-1-test".into(),
        started_at: "2026-02-10T10:00:00Z".into(),
        last_activity: "2026-02-10T10:05:00Z".into(),
        provider: "anthropic".into(),
        model: "test-model".into(),
        branch_name: "story/1-1-test".into(),
        base_branch: "main".into(),
        chat_history: vec![
            ChatMessage {
                role: "user".into(),
                content: "DS".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Starting story...".into(),
            },
        ],
    };

    write_wal_file(tmp.path(), &state);

    let path = tmp.path().join(".bmad-bot-session.yaml");
    let content = std::fs::read_to_string(&path).expect("read WAL");
    let loaded: SessionState = serde_yml::from_str(&content).expect("parse WAL");

    assert_eq!(loaded.chat_history.len(), 2);
    assert_eq!(loaded.chat_history[0].role, "user");
    assert_eq!(loaded.chat_history[0].content, "DS");
    assert_eq!(loaded.chat_history[1].role, "assistant");
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

#[test]
fn test_create_test_repo_produces_valid_git_repo() {
    let tmp = tempfile::tempdir().expect("tempdir");
    create_test_repo(tmp.path());

    // Verify it's a git repo with HEAD
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .expect("git rev-parse");

    assert!(
        output.status.success(),
        "git rev-parse HEAD should succeed in initialized repo"
    );

    // Verify there's at least one commit
    let output = std::process::Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(tmp.path())
        .output()
        .expect("git log");

    let log_output = String::from_utf8_lossy(&output.stdout);
    assert!(
        log_output.contains("initial commit"),
        "should have initial commit"
    );
}

#[test]
fn test_create_test_repo_has_dot_git_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    create_test_repo(tmp.path());

    assert!(
        tmp.path().join(".git").exists(),
        ".git directory should exist"
    );
}
