//! Fixture builders for integration tests.
//!
//! Each builder produces valid data structures with sensible defaults.
//! Uses `tempfile` directories for all filesystem operations.

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
/// `dir` is the temp directory used for all path-related fields.
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
            output_folder: dir.display().to_string(),
            planning_artifacts: dir.display().to_string(),
            implementation_artifacts: dir.display().to_string(),
        },
        log_format: "pretty".to_string(),
        log_level: "info".to_string(),
        log_file: "test.log".to_string(),
        code_review_enabled: true,
        mcp_servers: vec![],
    }
}

/// Build `BotSecrets` with dummy tokens — never real API keys.
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
/// `key` format: `"{epic_num}-{story_num}-{slug}"` (e.g. `"7-1-integration-test"`).
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    // Parse key → epic_num, story_num
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts
        .next()
        .expect("key missing epic_num")
        .parse()
        .expect("epic_num not a number");
    let story_num: u32 = parts
        .next()
        .expect("key missing story_num")
        .parse()
        .expect("story_num not a number");

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

/// Write a valid `sprint-status.yaml` to `dir` from a list of `(key, status)` entries.
///
/// Entries go under `development_status:` as flat key-value pairs. Accepts epics,
/// stories, and retrospectives — all are valid entries.
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) -> std::path::PathBuf {
    let mut yaml = String::from(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n",
    );
    yaml.push_str(&format!("story_location: \"{}\"\n", dir.display()));
    yaml.push_str("\ndevelopment_status:\n");

    for (key, status) in &entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write sprint-status.yaml");
    path
}

/// Write a valid `.bmad-bot-session.yaml` WAL file to `dir` from a `SessionState`.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> std::path::PathBuf {
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
    path
}

/// Build a `SessionState` with sensible defaults for testing.
pub fn make_test_session_state(story_key: &str) -> SessionState {
    let story = make_test_story(story_key, "test story", vec![]);
    SessionState {
        story_id: story.story_id,
        story_key: story.story_key.clone(),
        branch: story.branch_name.clone(),
        started_at: "2026-01-01T00:00:00Z".to_string(),
        last_activity: "2026-01-01T00:01:00Z".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        branch_name: story.branch_name,
        base_branch: "main".to_string(),
        chat_history: vec![
            ChatMessage {
                role: "user".to_string(),
                content: "Implement the feature".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "I will implement the feature now.".to_string(),
            },
        ],
    }
}

/// Initialize a git repo with an initial commit in `dir` using Git CLI.
pub fn create_test_repo(dir: &Path) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed to execute");
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
    // Ensure "main" branch exists regardless of git default branch config
    run(&["branch", "-M", "main"]);
}
