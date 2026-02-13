//! Fixture builder functions for integration tests.
//!
//! Provides helpers to construct valid test data structures
//! without depending on internal `pub(crate)` helpers.

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

/// Build a valid [`BotConfig`] rooted at the given temp directory.
///
/// Uses sensible defaults: polling=60s, provider=github, review=enabled.
/// All paths are relative to `dir`.
pub fn make_test_config(dir: &Path) -> BotConfig {
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

/// Build a valid [`StoryInfo`] from a key, label, and dependencies.
///
/// Parses `key` (e.g., `"7-1-integration-test-infrastructure"`) to extract
/// epic/story numbers. Status defaults to `"ready-for-dev"`.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    let parts: Vec<&str> = key.splitn(3, '-').collect();
    let epic_num: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let story_num: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

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

/// Write a valid `sprint-status.yaml` to a temp directory with given entries.
///
/// `entries` is a slice of `(key, status)` pairs. All go under `development_status:`
/// as flat key-value pairs. Supports epic, story, and retrospective entries.
///
/// Returns the path to the written file.
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) -> std::path::PathBuf {
    let path = dir.join("sprint-status.yaml");
    let mut yaml = String::from(
        "generated: 2026-02-08\nproject: test-project\nproject_key: TEST\ntracking_system: file-system\nstory_location: \".\"\n\ndevelopment_status:\n",
    );
    for (key, status) in entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }
    std::fs::write(&path, &yaml).expect("write sprint-status.yaml");
    path
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file to a temp directory.
///
/// Uses the provided [`SessionState`] for serialization.
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

/// Initialize a git repo with an initial commit in a temp directory.
///
/// Returns the [`git2::Repository`] handle.
pub fn create_test_repo(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).expect("git init");
    let sig = git2::Signature::now("Test", "test@test.com").expect("signature");
    let tree_id = repo
        .index()
        .expect("index")
        .write_tree()
        .expect("write tree");
    let tree = repo.find_tree(tree_id).expect("find tree");
    repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
        .expect("commit");
    drop(tree);
    repo
}
