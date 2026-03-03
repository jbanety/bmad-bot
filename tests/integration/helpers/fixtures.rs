//! Fixture builder functions for integration tests.
//!
//! Every fixture uses sensible defaults and takes minimal parameters.
//! All filesystem fixtures use caller-provided temp directories (no global state).

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// make_test_config
// ---------------------------------------------------------------------------

/// Build a valid `BotConfig` using the provided temp directory for all paths.
///
/// Defaults: polling=60s, provider=github, review=enabled, log=json/info.
pub fn make_test_config(dir: &Path) -> BotConfig {
    let dir_str = dir.to_string_lossy().to_string();
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
                provider: "openai".to_string(),
                model: "gpt-4o".to_string(),
                reasoning_effort: None,
            },
        },
        notifications: NotificationConfig {
            telegram: TelegramConfig {
                enabled: false,
                chat_id: "test-chat-id".to_string(),
            },
        },
        bmad_paths: BmadPathsConfig {
            project_root: dir_str.clone(),
            output_folder: format!("{dir_str}/_bmad-output"),
            planning_artifacts: format!("{dir_str}/_bmad-output/planning-artifacts"),
            implementation_artifacts: format!("{dir_str}/_bmad-output/implementation-artifacts"),
        },
        log_format: "json".to_string(),
        log_level: "info".to_string(),
        log_file: format!("{dir_str}/bmad-bot.log"),
        code_review_enabled: true,
        mcp_servers: vec![],
    }
}

// ---------------------------------------------------------------------------
// make_test_secrets
// ---------------------------------------------------------------------------

/// Build a `BotSecrets` with dummy tokens for all providers.
///
/// All keys are clearly marked as test-only — never real credentials.
pub fn make_test_secrets() -> BotSecrets {
    BotSecrets {
        anthropic_api_key: Some("test-anthropic-key-DO-NOT-USE".to_string()),
        openai_api_key: Some("test-openai-key-DO-NOT-USE".to_string()),
        github_copilot_oauth_token: Some("test-github-copilot-token-DO-NOT-USE".to_string()),
        github_token: Some("test-github-token-DO-NOT-USE".to_string()),
        gitlab_token: Some("test-gitlab-token-DO-NOT-USE".to_string()),
        telegram_bot_token: Some("test-telegram-token-DO-NOT-USE".to_string()),
    }
}

// ---------------------------------------------------------------------------
// make_test_story
// ---------------------------------------------------------------------------

/// Build a valid `StoryInfo` from a key like `"7-1-integration-test-infrastructure"`.
///
/// Parses the key to extract epic_num, story_num, and label.
/// `deps` is a list of story keys this story depends on.
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

// ---------------------------------------------------------------------------
// write_sprint_status
// ---------------------------------------------------------------------------

/// Write a valid `sprint-status.yaml` to the given directory.
///
/// `entries` is a list of `(key, status)` pairs. ALL entry types are supported:
/// epics (`"epic-1"`, `"in-progress"`), stories (`"1-1-slug"`, `"ready-for-dev"`),
/// and retrospectives (`"epic-1-retrospective"`, `"optional"`).
///
/// All go under `development_status:` as flat key-value pairs.
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) {
    let mut yaml = String::from(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n",
    );
    yaml.push_str(&format!(
        "story_location: \"{}\"\n\n",
        dir.to_string_lossy()
    ));
    yaml.push_str("development_status:\n");
    for (key, status) in entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write sprint-status.yaml");
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file to the given directory.
///
/// Accepts a `SessionState` and serializes it to YAML.
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState to YAML");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

/// Initialize a git repository with an initial commit in the given directory.
///
/// Uses Git CLI subprocess calls (git2 was removed from the project in Story 4.4).
/// Creates a repo with user config, an empty initial commit, and ensures the
/// default branch is named `"main"`.
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
