//! Fixture builders for integration tests.
//!
//! Provides helper functions that create valid test data structures
//! for `BotConfig`, `BotSecrets`, `StoryInfo`, sprint-status YAML,
//! WAL files, and git repos.

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::StoryInfo;

/// Build a complete valid `BotConfig` using the provided temp directory.
///
/// Defaults: polling=60, provider=github, review=enabled, all paths under `dir`.
pub fn make_test_config(dir: &Path) -> BotConfig {
    let dir_str = dir.display().to_string();
    BotConfig {
        polling_interval_secs: 60,
        code_review_enabled: true,
        git_provider: GitProviderConfig {
            provider: "github".to_string(),
            repo_owner: "test-owner".to_string(),
            repo_name: "test-repo".to_string(),
            target_branch: "main".to_string(),
        },
        llm: LlmConfig {
            dev: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-model".to_string(),
                reasoning_effort: None,
            },
            review: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-model".to_string(),
                reasoning_effort: None,
            },
            supervisor: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-model".to_string(),
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
        mcp_servers: vec![],
    }
}

/// Build `BotSecrets` with dummy tokens. Never uses real keys.
pub fn make_test_secrets() -> BotSecrets {
    BotSecrets {
        anthropic_api_key: Some("test-anthropic-key-DO-NOT-USE".into()),
        openai_api_key: Some("test-openai-key-DO-NOT-USE".into()),
        github_copilot_oauth_token: Some("test-ghcopilot-key-DO-NOT-USE".into()),
        github_token: Some("test-github-token-DO-NOT-USE".into()),
        gitlab_token: Some("test-gitlab-token-DO-NOT-USE".into()),
        telegram_bot_token: Some("test-telegram-token-DO-NOT-USE".into()),
    }
}

/// Build a valid `StoryInfo` from a key, label, and dependencies.
///
/// Key format: `"{epic}-{story}-{slug}"` (e.g. `"7-1-integration-test"`).
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    let parts: Vec<&str> = key.splitn(3, '-').collect();
    let epic_num: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(1);
    let story_num: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);

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

/// Write a valid `sprint-status.yaml` to a temp directory.
///
/// `entries` is a list of `(key, status)` pairs written under `development_status:`.
/// Accepts epic entries, story entries, and retrospective entries.
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) {
    let dir_str = dir.display().to_string();
    let mut yaml = format!(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n\
         story_location: \"{dir_str}\"\n\
         \n\
         development_status:\n"
    );
    for (key, status) in entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }
    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, yaml).expect("Failed to write sprint-status.yaml");
}

/// Write a valid `.bmad-bot-session.yaml` WAL file to a temp directory.
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let path = dir.join(".bmad-bot-session.yaml");
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    std::fs::write(&path, yaml).expect("Failed to write WAL file");
}

/// Initialize a git repo with an initial commit in the given directory.
///
/// Uses Git CLI (no git2 dependency). Creates `main` branch.
pub fn create_test_repo(dir: &Path) {
    use std::process::Command;
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init"]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["commit", "--allow-empty", "-m", "initial commit"]);
    // Ensure "main" branch exists (default might be "master" depending on git config)
    run(&["branch", "-M", "main"]);
}

/// Helper to create a minimal `SessionState` for testing WAL writes.
pub fn make_test_session_state(story_key: &str) -> SessionState {
    SessionState {
        story_id: "1.1".to_string(),
        story_key: story_key.to_string(),
        branch: format!("story/{story_key}"),
        started_at: "2026-02-08T10:00:00Z".to_string(),
        last_activity: "2026-02-08T10:05:00Z".to_string(),
        provider: "anthropic".to_string(),
        model: "test-model".to_string(),
        branch_name: format!("story/{story_key}"),
        base_branch: "main".to_string(),
        chat_history: vec![
            ChatMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Hi there!".to_string(),
            },
        ],
    }
}
