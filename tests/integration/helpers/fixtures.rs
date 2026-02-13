//! Fixture builders for integration tests.
//!
//! Every function produces valid, self-contained test data. No real API keys.
//! Uses `tempfile::TempDir` for filesystem isolation.

use std::path::Path;
use std::process::Command;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::{SessionState};
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// make_test_config
// ---------------------------------------------------------------------------

/// Build a valid [`BotConfig`] rooted at the given directory.
///
/// Uses sensible defaults: polling=60s, provider=github, review=enabled.
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
            planning_artifacts: dir.join("_bmad-output/planning-artifacts").display().to_string(),
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

// ---------------------------------------------------------------------------
// make_test_secrets
// ---------------------------------------------------------------------------

/// Build a [`BotSecrets`] with dummy tokens. Never real keys.
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

/// Build a valid [`StoryInfo`] from a key, label, and dependency list.
///
/// Key format: `"{epic}-{story}-{slug}"` (e.g., `"7-1-infra"`).
/// Parses `epic_num`, `story_num`, and derives `story_id`, `branch_name`, `specs_path`.
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

// ---------------------------------------------------------------------------
// write_sprint_status
// ---------------------------------------------------------------------------

/// Write a valid `sprint-status.yaml` to `dir` with given entries.
///
/// `entries` is a `Vec<(&str, &str)>` of `(key, status)` pairs — supports epics,
/// stories, and retrospectives. All go under `development_status:`.
///
/// Returns the path to the written file.
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) -> std::path::PathBuf {
    let mut yaml = String::from(
        "generated: 2026-01-01\n\
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
    std::fs::write(&path, &yaml).expect("write sprint-status.yaml");
    path
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file to `dir` from a [`SessionState`].
///
/// Returns the path to the written file.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> std::path::PathBuf {
    let path = dir.join(".bmad-bot-session.yaml");
    let yaml = serde_yml::to_string(state).expect("serialize SessionState");
    std::fs::write(&path, &yaml).expect("write WAL file");
    path
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

/// Initialize a git repo with an initial commit in `dir` using CLI `git`.
///
/// Returns `()` — use `git2` if a repo handle is needed (requires adding it as a dependency).
pub fn create_test_repo(dir: &Path) {
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git command");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    };

    // Init
    let init_out = Command::new("git")
        .args(["init", dir.to_str().unwrap()])
        .output()
        .expect("git init");
    assert!(init_out.status.success(), "git init failed");

    // Configure identity
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);

    // Initial empty commit so HEAD exists
    run(&["commit", "--allow-empty", "-m", "initial commit"]);
}
