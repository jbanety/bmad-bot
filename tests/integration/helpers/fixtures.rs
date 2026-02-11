//! Fixture builders for integration tests.
//!
//! Each builder produces valid data structures with sensible defaults.
//! All filesystem-writing fixtures require a `&Path` (typically from `tempdir()`).

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::state::SessionState;
use bmad_bot::watcher::StoryInfo;

/// Build a valid [`BotConfig`] with sensible defaults for testing.
///
/// Uses the provided `dir` as the base for all path fields.
///
/// Defaults: polling=60s, provider=github, review=enabled.
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
            },
            review: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
            },
            supervisor: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
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

/// Build a valid [`BotSecrets`] with dummy tokens for all providers.
///
/// **Never use these tokens for real API calls.**
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

/// Build a valid [`StoryInfo`] from a key, label, and dependency list.
///
/// Parses `key` (e.g., `"7-1-integration-test-infrastructure"`) to extract
/// `epic_num`, `story_num`, and `label`.
///
/// The `label` parameter overrides the slug-derived label. Pass an empty string
/// to use the slug-derived label instead.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let story_num: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let slug = parts.next().unwrap_or("");

    let derived_label = if label.is_empty() {
        slug.replace('-', " ")
    } else {
        label.to_string()
    };

    let story_id = format!("{epic_num}.{story_num}");
    let branch_name = format!("story/{key}");
    let specs_path =
        std::path::PathBuf::from(format!("_bmad-output/implementation-artifacts/{key}.md"));

    StoryInfo {
        story_id,
        story_key: key.to_string(),
        epic_num,
        story_num,
        label: derived_label,
        branch_name,
        specs_path,
        dependencies: deps,
        status: "ready-for-dev".to_string(),
    }
}

/// Write a valid `sprint-status.yaml` to the given directory.
///
/// `entries` is a list of `(key, status)` pairs. All entry types are supported:
/// epic entries (`"epic-1", "in-progress"`), story entries (`"1-1-slug", "ready-for-dev"`),
/// and retrospective entries (`"epic-1-retrospective", "optional"`).
///
/// All entries are written under `development_status:` as flat key-value pairs.
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) {
    let mut yaml = String::from(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n\
         story_location: \".\"\n\n\
         development_status:\n",
    );

    for (key, status) in entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }

    let path = dir.join("sprint-status.yaml");
    std::fs::write(path, yaml).expect("write sprint-status.yaml");
}

/// Write a valid WAL (`.bmad-bot-session.yaml`) file to the given directory.
///
/// Serializes the provided `SessionState` to YAML.
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let yaml = serde_yml::to_string(state).expect("serialize SessionState");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(path, yaml).expect("write WAL file");
}

/// Initialize a git repo with an initial commit in the given directory.
///
/// Uses Git CLI (`git init`, `git commit --allow-empty`) — no `git2` dependency.
/// Requires `git` to be installed on the host.
pub fn create_test_repo(dir: &Path) {
    let run = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .expect("git command failed to start");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };

    run(&["init"]);
    run(&["commit", "--allow-empty", "-m", "initial commit"]);
}
