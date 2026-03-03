//! Fixture builders for integration tests.
//!
//! Each builder produces a valid data structure with sensible defaults.
//! All filesystem operations use `tempfile::tempdir()` for isolation.

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    McpServerConfig, NotificationConfig, TelegramConfig,
};
use bmad_bot::session::SessionState;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// make_test_config (Task 6.1)
// ---------------------------------------------------------------------------

/// Build a complete valid [`BotConfig`] using the provided temp directory.
///
/// Defaults: polling=60s, provider=github, review=enabled.
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
                model: "test-dev-model".to_string(),
                reasoning_effort: None,
            },
            review: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-review-model".to_string(),
                reasoning_effort: None,
            },
            supervisor: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-supervisor-model".to_string(),
                reasoning_effort: None,
            },
        },
        notifications: NotificationConfig {
            telegram: TelegramConfig {
                enabled: true,
                chat_id: "test-chat-id".to_string(),
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
        mcp_servers: Vec::<McpServerConfig>::new(),
    }
}

// ---------------------------------------------------------------------------
// make_test_secrets (Task 6.2)
// ---------------------------------------------------------------------------

/// Build [`BotSecrets`] with dummy tokens. NEVER real keys.
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

// ---------------------------------------------------------------------------
// make_test_story (Task 6.3)
// ---------------------------------------------------------------------------

/// Build a valid [`StoryInfo`] from a key like `"7-1-integration-test-infrastructure"`.
///
/// Parses the key to extract `epic_num`, `story_num`, `label`.
/// Dependencies are passed as a `Vec<String>`.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    // Parse epic_num and story_num from key
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let story_num: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);

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

// ---------------------------------------------------------------------------
// write_sprint_status (Task 6.4)
// ---------------------------------------------------------------------------

/// Write a valid `sprint-status.yaml` to `dir` with given entries.
///
/// `entries` is a `Vec<(&str, &str)>` of `(key, status)` pairs that go under
/// `development_status:`. Accepts epic, story, and retrospective entries.
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) {
    let mut yaml = format!(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n\
         story_location: \"{}\"\n\
         \n\
         development_status:\n",
        dir.display()
    );
    for (key, status) in &entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }
    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write sprint-status.yaml");
}

// ---------------------------------------------------------------------------
// write_wal_file (Task 6.5)
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file from a [`SessionState`].
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let path = dir.join(".bmad-bot-session.yaml");
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
}

// ---------------------------------------------------------------------------
// create_test_repo (Task 6.6)
// ---------------------------------------------------------------------------

/// Initialize a git repo with an initial commit in the given directory.
///
/// Uses Git CLI subprocess calls (no `git2`).
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
