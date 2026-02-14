//! Fixture builder functions for integration tests.
//!
//! Produces valid data structures with sensible defaults for testing.
//! Uses `tempfile` for filesystem isolation.

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::StoryInfo;
use std::path::Path;

/// Build a valid `BotConfig` with sensible test defaults.
///
/// Uses the provided `dir` for all path fields.
pub fn make_test_config(dir: &Path) -> BotConfig {
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
            project_root: dir
                .parent()
                .unwrap_or(dir)
                .display()
                .to_string(),
            output_folder: dir.display().to_string(),
            planning_artifacts: dir.display().to_string(),
            implementation_artifacts: dir.display().to_string(),
        },
        log_format: "pretty".to_string(),
        log_level: "info".to_string(),
        log_file: "test.log".to_string(),
        code_review_enabled: true,
    }
}

/// Build `BotSecrets` with dummy tokens — NEVER real keys.
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

/// Build a valid `StoryInfo` from key, label, and dependencies.
///
/// Parses `key` (e.g. `"7-1-integration-test"`) to extract epic/story numbers.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    // Parse: {epic_num}-{story_num}-{slug}
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let story_num: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);

    let story_id = format!("{epic_num}.{story_num}");
    let branch_name = format!("story/{key}");
    let specs_path =
        std::path::PathBuf::from(format!("_bmad-output/implementation-artifacts/{key}.md"));

    StoryInfo {
        story_id,
        story_key: key.to_string(),
        epic_num,
        story_num,
        label: label.to_string(),
        branch_name,
        specs_path,
        dependencies: deps,
        status: "ready-for-dev".to_string(),
    }
}

/// Write a valid `sprint-status.yaml` to `dir` with given entries.
///
/// Each entry is a `(key, status)` pair. Entries can be epics, stories,
/// or retrospectives — all go under `development_status:`.
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) {
    let mut yaml = String::from(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n\
         story_location: \".\"\n\
         \n\
         development_status:\n",
    );

    for (key, status) in entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, yaml).expect("Failed to write sprint-status.yaml");
}

/// Write a valid WAL file (`.bmad-bot-session.yaml`) to `dir`.
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(&path, yaml).expect("Failed to write WAL file");
}

/// Initialize a git repo with an initial commit in `dir` via Git CLI.
pub fn create_test_repo(dir: &Path) {
    let run = |args: &[&str]| {
        let output = std::process::Command::new("git")
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
