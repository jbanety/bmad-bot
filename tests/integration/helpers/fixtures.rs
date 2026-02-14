//! Fixture builder functions for integration tests.
//!
//! Provides helpers to construct valid test data structures and write
//! test files (sprint-status YAML, WAL files, git repos).

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::state::SessionState;
use bmad_bot::watcher::StoryInfo;

/// Build a complete valid `BotConfig` with sensible test defaults.
///
/// Uses the provided `dir` as the base path for all BMAD artifact paths.
/// Defaults: polling=60, provider=github, review=enabled.
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
    }
}

/// Build a `BotSecrets` with dummy tokens for all providers.
///
/// **Never use real keys.** All values are prefixed with `test-` and suffixed
/// with `-DO-NOT-USE` to make accidental real usage obvious.
pub fn make_test_secrets() -> BotSecrets {
    BotSecrets {
        anthropic_api_key: Some("test-anthropic-key-DO-NOT-USE".into()),
        openai_api_key: Some("test-openai-key-DO-NOT-USE".into()),
        github_copilot_oauth_token: Some("test-ghcopilot-token-DO-NOT-USE".into()),
        github_token: Some("test-github-token-DO-NOT-USE".into()),
        gitlab_token: Some("test-gitlab-token-DO-NOT-USE".into()),
        telegram_bot_token: Some("test-telegram-token-DO-NOT-USE".into()),
    }
}

/// Build a valid `StoryInfo` from a key, label override, and dependency list.
///
/// Parses the `key` to extract epic_num, story_num, and slug. The `label`
/// parameter overrides the auto-derived label. Pass an empty string to use
/// the slug-derived label.
///
/// # Panics
/// Panics if `key` does not follow the `{epic}-{story}-{slug}` pattern.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts
        .next()
        .expect("key must have epic num")
        .parse()
        .expect("epic num must be numeric");
    let story_num: u32 = parts
        .next()
        .expect("key must have story num")
        .parse()
        .expect("story num must be numeric");
    let slug = parts.next().unwrap_or("");

    let derived_label = if label.is_empty() {
        slug.replace('-', " ")
    } else {
        label.to_string()
    };

    StoryInfo {
        story_id: format!("{epic_num}.{story_num}"),
        story_key: key.to_string(),
        epic_num,
        story_num,
        label: derived_label,
        branch_name: format!("story/{key}"),
        specs_path: std::path::PathBuf::from(format!(
            "_bmad-output/implementation-artifacts/{key}.md"
        )),
        dependencies: deps,
        status: "ready-for-dev".to_string(),
    }
}

/// Write a valid `sprint-status.yaml` to the given directory.
///
/// The `entries` parameter is a `Vec<(&str, &str)>` of `(key, status)` pairs.
/// ALL entry types are supported: epics (`"epic-1", "in-progress"`),
/// stories (`"1-1-slug", "ready-for-dev"`), and retrospectives
/// (`"epic-1-retrospective", "optional"`). All go under `development_status:`
/// as flat key-value pairs.
///
/// Returns the path to the written file.
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) -> std::path::PathBuf {
    let mut yaml = String::new();
    yaml.push_str("generated: 2026-02-08\n");
    yaml.push_str("project: test-project\n");
    yaml.push_str("project_key: TEST\n");
    yaml.push_str("tracking_system: file-system\n");
    yaml.push_str(&format!(
        "story_location: \"{}\"\n",
        dir.display()
    ));
    yaml.push_str("\ndevelopment_status:\n");
    for (key, status) in &entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }

    let file_path = dir.join("sprint-status.yaml");
    std::fs::write(&file_path, &yaml).expect("failed to write sprint-status.yaml");
    file_path
}

/// Write a valid WAL session file to the given directory.
///
/// Serializes the provided `SessionState` as YAML to
/// `{dir}/.bmad-bot-session.yaml`.
///
/// Returns the path to the written file.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> std::path::PathBuf {
    let file_path = dir.join(".bmad-bot-session.yaml");
    let yaml = serde_yml::to_string(state).expect("failed to serialize SessionState");
    std::fs::write(&file_path, &yaml).expect("failed to write WAL file");
    file_path
}

/// Initialize a git repository with an initial commit in the given directory.
///
/// Creates a proper git repo using Git CLI commands:
/// `git init`, config user, empty initial commit, rename branch to `main`.
///
/// # Panics
/// Panics if any git command fails.
pub fn create_test_repo(dir: &Path) {
    use std::process::Command;

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
    // Ensure "main" branch exists (default might be "master" depending on git config)
    run(&["branch", "-M", "main"]);
}
