//! Fixture builders for integration testing.
//!
//! Provides factory functions to create valid test data structures
//! and write test files to temporary directories.

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::StoryInfo;

/// Build a complete valid `BotConfig` with sensible test defaults.
///
/// Uses the provided `dir` as the base for all path-related fields.
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
            project_root: dir.display().to_string(),
            output_folder: dir.join("_bmad-output").display().to_string(),
            planning_artifacts: dir
                .join("_bmad-output/planning-artifacts")
                .display()
                .to_string(),
            implementation_artifacts: dir
                .join("_bmad-output/implementation-artifacts")
                .display()
                .to_string(),
        },
        log_format: "pretty".to_string(),
        log_level: "info".to_string(),
        log_file: "test.log".to_string(),
        code_review_enabled: true,
        mcp_servers: vec![],
    }
}

/// Build a `BotSecrets` with dummy tokens for all providers.
///
/// These are clearly fake and should never be used against real APIs.
pub fn make_test_secrets() -> BotSecrets {
    BotSecrets {
        anthropic_api_key: Some("test-anthropic-key-DO-NOT-USE".into()),
        openai_api_key: Some("test-openai-key-DO-NOT-USE".into()),
        github_copilot_oauth_token: Some("test-copilot-token-DO-NOT-USE".into()),
        github_token: Some("test-github-token-DO-NOT-USE".into()),
        gitlab_token: Some("test-gitlab-token-DO-NOT-USE".into()),
        telegram_bot_token: Some("test-telegram-token-DO-NOT-USE".into()),
    }
}

/// Build a valid `StoryInfo` from a key, label, and dependency list.
///
/// Parses the key to extract `epic_num`, `story_num`, and derives
/// `story_id`, `branch_name`, and `specs_path`.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let story_num: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);

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

/// Write a valid `sprint-status.yaml` file to the given directory.
///
/// The `entries` parameter accepts all entry types: epic entries
/// (`"epic-1", "in-progress"`), story entries (`"1-1-slug", "ready-for-dev"`),
/// and retrospective entries (`"epic-1-retrospective", "optional"`).
/// All are written under `development_status:` as flat key-value pairs.
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) {
    let mut yaml = String::new();
    yaml.push_str("generated: 2026-02-08\n");
    yaml.push_str("project: test-project\n");
    yaml.push_str("project_key: TEST\n");
    yaml.push_str("tracking_system: file-system\n");
    yaml.push_str(&format!("story_location: \"{}\"\n", dir.display()));
    yaml.push('\n');
    yaml.push_str("development_status:\n");
    for (key, status) in &entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write sprint-status.yaml");
}

/// Write a valid `.bmad-bot-session.yaml` WAL file to the given directory.
///
/// Creates the WAL file from a `SessionState` struct, serialized as YAML.
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
}

/// Initialize a git repository with an initial commit in the given directory.
///
/// Uses Git CLI subprocess calls. Creates a repo with:
/// - `git init`
/// - Configured test user
/// - An initial empty commit
/// - `main` branch as default
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
    // Ensure "main" branch exists regardless of git config default
    run(&["branch", "-M", "main"]);
}
