//! Fixture builders for integration tests.
//!
//! Provides helper functions to create valid test data structures
//! without touching real files, APIs, or services.

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::SessionState;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// make_test_config
// ---------------------------------------------------------------------------

/// Build a valid `BotConfig` with sensible test defaults.
///
/// Uses the provided `dir` for all path-related config fields.
/// Defaults: polling=60, provider=github, code_review_enabled=true.
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

// ---------------------------------------------------------------------------
// make_test_secrets
// ---------------------------------------------------------------------------

/// Build a `BotSecrets` with dummy tokens. Never use real keys.
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

/// Build a `StoryInfo` from a key, label, and dependency list.
///
/// Parses the key to extract `epic_num`, `story_num`, and `label`.
/// Defaults to `ready-for-dev` status.
///
/// # Panics
/// Panics if `key` cannot be parsed as `{epic_num}-{story_num}-{slug}`.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    // Parse: {epic_num}-{story_num}-{slug}
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts.next().expect("missing epic_num").parse().expect("invalid epic_num");
    let story_num: u32 = parts.next().expect("missing story_num").parse().expect("invalid story_num");

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
// write_sprint_status
// ---------------------------------------------------------------------------

/// Write a valid `sprint-status.yaml` to `dir` with the given entries.
///
/// `entries` is a slice of `(key, status)` pairs that go under `development_status:`.
/// Supports all entry types: epics, stories, and retrospectives.
///
/// Returns the path to the written file.
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) -> std::path::PathBuf {
    let path = dir.join("sprint-status.yaml");

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

    std::fs::write(&path, &yaml).expect("write sprint-status.yaml");
    path
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file to `dir` from a `SessionState`.
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

/// Initialize a git repo with an initial commit in the given directory.
///
/// Returns the `git2::Repository` handle.
pub fn create_test_repo(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).expect("git init");
    // Create initial commit (required for HEAD to exist)
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
