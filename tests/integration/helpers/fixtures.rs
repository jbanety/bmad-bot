//! Fixture builder functions for integration tests.
//!
//! All fixtures produce valid data structures with sensible defaults.
//! Use `tempfile::tempdir()` for any filesystem fixtures.

use std::path::Path;
use std::process::Command;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::StoryInfo;

/// Build a valid `BotConfig` with sensible test defaults.
///
/// Uses the provided `dir` as the base for all path fields.
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
            planning_artifacts: dir.join("_bmad-output/planning-artifacts").display().to_string(),
            implementation_artifacts: dir
                .join("_bmad-output/implementation-artifacts")
                .display()
                .to_string(),
        },
        log_format: "pretty".to_string(),
        log_level: "info".to_string(),
        log_file: "bmad-bot.log".to_string(),
        code_review_enabled: true,
    }
}

/// Build `BotSecrets` with dummy test tokens.
///
/// **Never** use real keys — all values are clearly marked as test-only.
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
/// The key must follow the pattern `{epic_num}-{story_num}-{slug}`.
/// Example: `make_test_story("7-1-test-infra", "test infra", vec![])`.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    // Parse epic_num and story_num from key
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let story_num: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

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

/// Write a valid `sprint-status.yaml` to the given directory.
///
/// `entries` is a list of `(key, status)` pairs. All entry types are supported:
/// epic entries, story entries, and retrospective entries.
///
/// The file is written as `sprint-status.yaml` in `dir`.
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) {
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

    std::fs::write(dir.join("sprint-status.yaml"), &yaml)
        .expect("Failed to write sprint-status.yaml");
}

/// Write a valid WAL session state file to the given directory.
///
/// Writes `.bmad-bot-session.yaml` in `dir` with the provided `SessionState`.
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let yaml =
        serde_yml::to_string(state).expect("Failed to serialize SessionState");
    std::fs::write(dir.join(".bmad-bot-session.yaml"), &yaml)
        .expect("Failed to write WAL file");
}

/// Initialize a git repo with an initial commit in the given directory.
///
/// Uses Git CLI (no `git2`). Creates a repo with `main` branch and one empty commit.
pub fn create_test_repo(dir: &Path) {
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
