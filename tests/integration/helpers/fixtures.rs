//! Fixture builder functions for integration tests.
//!
//! Provides helpers to construct valid test data structures and write
//! test files to temporary directories.

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

/// Build a complete valid `BotConfig` using the provided temp directory for paths.
///
/// Uses sensible defaults: polling=60, provider=github, review=enabled.
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

// ---------------------------------------------------------------------------
// make_test_secrets
// ---------------------------------------------------------------------------

/// Build a `BotSecrets` with dummy tokens for all providers.
///
/// **Never uses real keys** — all values are clearly marked as test-only.
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

// ---------------------------------------------------------------------------
// make_test_story
// ---------------------------------------------------------------------------

/// Build a valid `StoryInfo` from a key, label, and dependency list.
///
/// Parses `key` to extract `epic_num`, `story_num`, and derives `story_id`,
/// `branch_name`, and `specs_path`.
///
/// # Example
/// ```ignore
/// let story = make_test_story("7-1-integration-test", "integration test", &[]);
/// assert_eq!(story.story_id, "7.1");
/// assert_eq!(story.branch_name, "story/7-1-integration-test");
/// ```
pub fn make_test_story(key: &str, label: &str, deps: &[&str]) -> StoryInfo {
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let story_num: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);

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
        dependencies: deps.iter().map(|d| d.to_string()).collect(),
        status: "ready-for-dev".to_string(),
    }
}

// ---------------------------------------------------------------------------
// write_sprint_status
// ---------------------------------------------------------------------------

/// Write a valid `sprint-status.yaml` to a temp directory with given entries.
///
/// `entries` is a list of `(key, status)` pairs. ALL entry types are supported:
/// epics (`"epic-1", "in-progress"`), stories (`"1-1-slug", "ready-for-dev"`),
/// and retrospectives (`"epic-1-retrospective", "optional"`).
///
/// Returns the path to the written file.
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
// write_wal_file
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file to a temp directory.
///
/// Serializes the provided `SessionState` as YAML.
/// Returns the path to the written file.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> std::path::PathBuf {
    let yaml = serde_yml::to_string(state).expect("serialize SessionState to YAML");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(&path, &yaml).expect("write WAL file");
    path
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

/// Initialize a git repository with an initial commit in a temp directory.
///
/// Uses the `git` CLI. Returns the path to the repo root.
///
/// # Panics
/// Panics if git is not available or commands fail.
pub fn create_test_repo(dir: &Path) -> std::path::PathBuf {
    use std::process::Command;

    // git init
    let status = Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .expect("git init");
    assert!(status.status.success(), "git init failed");

    // Configure user for commits
    let status = Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir)
        .output()
        .expect("git config email");
    assert!(status.status.success(), "git config email failed");

    let status = Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .expect("git config name");
    assert!(status.status.success(), "git config name failed");

    // Create an initial commit (empty tree)
    let status = Command::new("git")
        .args(["commit", "--allow-empty", "-m", "initial commit"])
        .current_dir(dir)
        .output()
        .expect("git commit");
    assert!(status.status.success(), "git commit failed");

    dir.to_path_buf()
}

// ---------------------------------------------------------------------------
// Helper: make_session_state (for write_wal_file convenience)
// ---------------------------------------------------------------------------

/// Create a minimal `SessionState` for testing WAL writing.
///
/// Uses the provided story info to populate fields. Chat history starts empty.
pub fn make_session_state(story: &StoryInfo) -> SessionState {
    let now = chrono::Utc::now().to_rfc3339();
    SessionState {
        story_id: story.story_id.clone(),
        story_key: story.story_key.clone(),
        branch: story.branch_name.clone(),
        started_at: now.clone(),
        last_activity: now,
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        branch_name: String::new(),
        base_branch: String::new(),
        chat_history: Vec::new(),
    }
}

/// Add chat messages to a `SessionState` for testing.
pub fn add_chat_messages(state: &mut SessionState, messages: &[(&str, &str)]) {
    for (role, content) in messages {
        state.chat_history.push(ChatMessage {
            role: role.to_string(),
            content: content.to_string(),
        });
    }
}
