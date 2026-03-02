//! Fixture builders for integration tests.
//!
//! Each builder produces a valid, self-contained test artifact.
//! Uses `tempfile` for filesystem isolation — temp dirs are cleaned up on `Drop`.

use std::path::Path;
use std::process::Command;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::SessionState;
use bmad_bot::watcher::StoryInfo;

/// Build a complete valid `BotConfig` using the provided temp directory for all paths.
///
/// Returns a config with sensible defaults for testing:
/// - `polling_interval_secs`: 60
/// - `git_provider`: github, `test-owner/test-repo`, target `main`
/// - `llm`: all roles set to `anthropic`/`test-model`
/// - `notifications`: telegram disabled
/// - `code_review_enabled`: true
pub fn make_test_config(dir: &Path) -> BotConfig {
    let dir_str = dir.to_string_lossy().to_string();
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
            output_folder: format!("{dir_str}/_bmad-output"),
            planning_artifacts: format!("{dir_str}/_bmad-output/planning-artifacts"),
            implementation_artifacts: format!("{dir_str}/_bmad-output/implementation-artifacts"),
        },
        log_format: "pretty".to_string(),
        log_level: "info".to_string(),
        log_file: format!("{dir_str}/bmad-bot.log"),
        mcp_servers: vec![],
    }
}

/// Build a `BotSecrets` with dummy tokens for all providers.
///
/// **Never use real keys.** All values are prefixed with `test-` and suffixed
/// with `-DO-NOT-USE`.
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

/// Build a valid `StoryInfo` from key components.
///
/// # Arguments
/// - `key`: Full story key (e.g., `"7-1-integration-test-infrastructure"`)
/// - `label`: Human-readable label (e.g., `"integration test infrastructure"`)
/// - `deps`: Dependency story keys (e.g., `vec!["6-3-crash-recovery"]`)
///
/// Parses `key` to extract `epic_num` and `story_num`. Status defaults to
/// `"ready-for-dev"`.
pub fn make_test_story(key: &str, label: &str, deps: Vec<&str>) -> StoryInfo {
    let parts: Vec<&str> = key.splitn(3, '-').collect();
    let epic_num: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let story_num: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
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
        dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
        status: "ready-for-dev".to_string(),
    }
}

/// Write a valid `sprint-status.yaml` to the given directory.
///
/// # Arguments
/// - `dir`: Target directory (usually a `tempdir()` path)
/// - `entries`: Flat list of `(key, status)` pairs — supports epic, story, and
///   retrospective entries
///
/// The generated file includes all required top-level fields and a
/// `development_status:` mapping.
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) -> std::path::PathBuf {
    let mut yaml = String::from(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n",
    );
    yaml.push_str(&format!(
        "story_location: \"{}\"\n",
        dir.to_string_lossy()
    ));
    yaml.push_str("\ndevelopment_status:\n");
    for (key, status) in &entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write sprint-status.yaml");
    path
}

/// Write a valid WAL file (`.bmad-bot-session.yaml`) to the given directory.
///
/// # Arguments
/// - `dir`: Target directory
/// - `state`: The `SessionState` to serialize
///
/// Returns the path to the written file.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> std::path::PathBuf {
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
    path
}

/// Initialize a git repository with an initial commit in the given directory.
///
/// Creates a repo with:
/// - `git init`
/// - `user.email` = `test@test.com`, `user.name` = `Test`
/// - An empty initial commit
/// - `main` branch (renamed from whatever default the system uses)
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
