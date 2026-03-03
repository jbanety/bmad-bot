//! Fixture builder functions for integration tests.
//!
//! Provides helpers to construct valid test data structures without touching
//! real APIs or persistent state. All filesystem operations use `tempfile`
//! directories that auto-clean on drop.

use std::path::{Path, PathBuf};

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::SessionState;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// impl_artifacts_dir
// ---------------------------------------------------------------------------

/// Create the `_bmad-output/implementation-artifacts` subdirectory under `root`
/// and return its path.  `make_test_config(root)` sets
/// `bmad_paths.implementation_artifacts` to this location.
pub fn impl_artifacts_dir(root: &Path) -> PathBuf {
    let dir = root.join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&dir).expect("create impl artifacts dir");
    dir
}

// ---------------------------------------------------------------------------
// make_test_config
// ---------------------------------------------------------------------------

/// Build a complete valid [`BotConfig`] rooted at the given temp directory.
///
/// Sensible defaults: polling=60, provider=github, review=enabled.
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
        mcp_servers: vec![],
    }
}

// ---------------------------------------------------------------------------
// make_test_secrets
// ---------------------------------------------------------------------------

/// Build [`BotSecrets`] with dummy tokens — NEVER real keys.
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

// ---------------------------------------------------------------------------
// make_test_story
// ---------------------------------------------------------------------------

/// Build a valid [`StoryInfo`] from a story key, label, and dependency list.
///
/// The key format is `"{epic_num}-{story_num}-{slug}"` (e.g., `"7-1-integration-test"`).
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    let parts: Vec<&str> = key.splitn(3, '-').collect();
    let epic_num: u32 = parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
    let story_num: u32 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
    let story_id = format!("{epic_num}.{story_num}");
    let branch_name = format!("story/{key}");
    let specs_path =
        PathBuf::from(format!("_bmad-output/implementation-artifacts/{key}.md"));

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

// ---------------------------------------------------------------------------
// write_sprint_status
// ---------------------------------------------------------------------------

/// Write a valid `sprint-status.yaml` to `dir` with the given story entries.
///
/// Each entry is a `(key, status)` tuple. Supports epics, stories, and retrospectives.
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) -> PathBuf {
    let path = dir.join("sprint-status.yaml");
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
    for (key, status) in &entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }
    std::fs::write(&path, &yaml).expect("Failed to write sprint-status.yaml");
    path
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file to `dir`.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> PathBuf {
    let path = dir.join(".bmad-bot-session.yaml");
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
    path
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

/// Initialize a git repo with an initial commit in `dir` via Git CLI.
///
/// Creates a bare repo with one empty commit on the `main` branch.
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
