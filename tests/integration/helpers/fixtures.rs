//! Fixture builder functions for integration tests.
//!
//! Provides helpers to build valid test data structures and write
//! test files (sprint-status YAML, WAL files, git repos).

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::StoryInfo;

/// Build a valid `BotConfig` with sensible test defaults.
///
/// Uses the provided `dir` for all path fields. Polling set to 60s,
/// provider=github, code review enabled.
pub fn make_test_config(dir: &Path) -> BotConfig {
    let dir_str = dir.display().to_string();
    BotConfig {
        polling_interval_secs: 60,
        git_provider: GitProviderConfig {
            provider: "github".to_string(),
            repo_owner: "test-owner".to_string(),
            repo_name: "test-repo".to_string(),
            target_branch: "main".to_string(),
        },
        llm: LlmConfig {
            dev: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
                reasoning_effort: None,
            },
            review: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
                reasoning_effort: None,
            },
            supervisor: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
                reasoning_effort: None,
            },
        },
        notifications: NotificationConfig {
            telegram: TelegramConfig {
                enabled: false,
                chat_id: String::new(),
            },
        },
        bmad_paths: BmadPathsConfig {
            project_root: dir_str.clone(),
            output_folder: dir_str.clone(),
            planning_artifacts: dir_str.clone(),
            implementation_artifacts: dir_str,
        },
        log_format: "pretty".to_string(),
        log_level: "info".to_string(),
        log_file: "test.log".to_string(),
        code_review_enabled: true,
    }
}

/// Build a `BotSecrets` with dummy tokens for all providers.
///
/// These are clearly fake tokens that should never be used against real APIs.
pub fn make_test_secrets() -> BotSecrets {
    BotSecrets {
        anthropic_api_key: Some("test-anthropic-key-DO-NOT-USE".into()),
        openai_api_key: Some("test-openai-key-DO-NOT-USE".into()),
        github_copilot_oauth_token: Some("test-ghmodels-key-DO-NOT-USE".into()),
        github_token: Some("test-github-token-DO-NOT-USE".into()),
        gitlab_token: Some("test-gitlab-token-DO-NOT-USE".into()),
        telegram_bot_token: Some("test-telegram-token-DO-NOT-USE".into()),
    }
}

/// Build a valid `StoryInfo` from a key, label, and dependency list.
///
/// Parses the key (e.g., `"7-1-integration-test"`) to extract epic/story numbers.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    let parts: Vec<&str> = key.splitn(3, '-').collect();
    let epic_num: u32 = parts
        .first()
        .and_then(|s| s.parse().ok())
        .expect("key must start with epic number");
    let story_num: u32 = parts
        .get(1)
        .and_then(|s| s.parse().ok())
        .expect("key must have story number as second segment");

    StoryInfo {
        story_id: format!("{epic_num}.{story_num}"),
        story_key: key.to_string(),
        epic_num,
        story_num,
        label: label.to_string(),
        branch_name: format!("story/{key}"),
        specs_path: std::path::PathBuf::from(format!(
            "_bmad-output/implementation-artifacts/{key}.md"
        )),
        dependencies: deps,
        status: "ready-for-dev".to_string(),
    }
}

/// Write a valid `sprint-status.yaml` to `dir` from a list of `(key, status)` entries.
///
/// All entries go under `development_status:` as flat key-value pairs.
/// Supports epic entries, story entries, and retrospective entries.
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) {
    let mut yaml = String::new();
    yaml.push_str("generated: 2026-02-08\n");
    yaml.push_str("project: test-project\n");
    yaml.push_str("project_key: TEST\n");
    yaml.push_str("tracking_system: file-system\n");
    yaml.push_str(&format!(
        "story_location: \"{}\"\n",
        dir.display()
    ));
    yaml.push('\n');
    yaml.push_str("development_status:\n");
    for (key, status) in entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &yaml).expect("write sprint-status.yaml");
}

/// Write a valid `.bmad-bot-session.yaml` WAL file to `dir`.
///
/// Uses the provided `SessionState` for content.
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let yaml = serde_yml::to_string(state).expect("serialize SessionState");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(&path, &yaml).expect("write WAL file");
}

/// Initialize a git repo with an initial commit in `dir`.
///
/// Returns the `git2::Repository` handle.
pub fn create_test_repo(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).expect("git init");
    let sig = git2::Signature::now("Test", "test@test.com").expect("signature");
    let tree_id = repo
        .index()
        .expect("index")
        .write_tree()
        .expect("write tree");
    {
        let tree = repo.find_tree(tree_id).expect("find tree");
        repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
            .expect("commit");
    }
    repo
}

/// Create a minimal `SessionState` for WAL testing.
pub fn make_test_session_state() -> SessionState {
    SessionState {
        story_id: "7.1".to_string(),
        story_key: "7-1-integration-test-infrastructure".to_string(),
        branch: "story/7-1-integration-test-infrastructure".to_string(),
        started_at: "2026-02-08T00:00:00Z".to_string(),
        last_activity: "2026-02-08T00:01:00Z".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        branch_name: "story/7-1-integration-test-infrastructure".to_string(),
        base_branch: "main".to_string(),
        chat_history: vec![
            ChatMessage {
                role: "user".to_string(),
                content: "DS".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Starting story implementation.".to_string(),
            },
        ],
    }
}
