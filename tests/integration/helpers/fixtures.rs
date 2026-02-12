//! Fixture builder functions for integration tests.
//!
//! Provides helpers to create valid test data structures (configs, secrets,
//! stories) and write test artifacts (sprint-status YAML, WAL files, git repos).

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::state::{ChatMessage, SessionState};
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// Config / Secrets builders
// ---------------------------------------------------------------------------

/// Build a complete, valid `BotConfig` with sensible test defaults.
///
/// Uses the provided `dir` for all path fields so tests remain isolated
/// within their temp directory.
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
                model: "test-model".to_string(),
            },
            review: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-model".to_string(),
            },
            supervisor: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-model".to_string(),
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
            output_folder: dir_str.clone(),
            planning_artifacts: dir_str.clone(),
            implementation_artifacts: dir_str,
        },
        log_format: "pretty".to_string(),
        log_level: "info".to_string(),
        log_file: "test.log".to_string(),
        code_review_enabled: true,
    }
}

/// Build a `BotSecrets` with dummy tokens for all providers.
///
/// Tokens are clearly marked DO-NOT-USE to prevent accidental real usage.
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
// StoryInfo builder
// ---------------------------------------------------------------------------

/// Build a valid `StoryInfo` from key, label, and dependencies.
///
/// The `key` should be in format `"{epic}-{story}-{slug}"` (e.g., `"7-1-infra"`).
/// Label can override the auto-derived label; pass `""` for auto-derivation.
/// Dependencies is a list of story keys this story depends on.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    // Parse key: "{epic_num}-{story_num}-{slug}"
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts.next().unwrap_or("1").parse().unwrap_or(1);
    let story_num: u32 = parts.next().unwrap_or("1").parse().unwrap_or(1);
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

// ---------------------------------------------------------------------------
// Sprint-status YAML writer
// ---------------------------------------------------------------------------

/// Write a valid `sprint-status.yaml` to a temp directory.
///
/// The `entries` parameter accepts epic entries, story entries, and
/// retrospective entries — all written as flat key-value pairs under
/// `development_status:`.
///
/// # Example
/// ```ignore
/// write_sprint_status(dir, &[
///     ("epic-1", "in-progress"),
///     ("1-1-story-slug", "ready-for-dev"),
///     ("1-2-another-story", "backlog"),
///     ("epic-1-retrospective", "optional"),
/// ]);
/// ```
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) -> std::path::PathBuf {
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

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &yaml).expect("write sprint-status.yaml");
    path
}

// ---------------------------------------------------------------------------
// WAL file writer
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file to a temp directory.
///
/// Serializes the provided `SessionState` to YAML using `serde_yml`.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> std::path::PathBuf {
    let path = dir.join(".bmad-bot-session.yaml");
    let yaml = serde_yml::to_string(state).expect("serialize SessionState");
    std::fs::write(&path, &yaml).expect("write WAL file");
    path
}

// ---------------------------------------------------------------------------
// Git repo creator
// ---------------------------------------------------------------------------

/// Initialize a git repo with an initial commit in the given directory.
///
/// Returns the `git2::Repository` handle for further manipulation if needed.
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
