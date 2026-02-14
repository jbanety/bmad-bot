//! Fixture builders for integration testing.
//!
//! Provides factory functions for building valid test data structures
//! and writing test files to temporary directories.

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::{ChatMessage, SessionState};
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// Config fixtures
// ---------------------------------------------------------------------------

/// Build a valid [`BotConfig`] pointing at the given temp directory.
///
/// Uses sensible test defaults: polling=60s, provider=github, review=enabled.
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
        log_file: "bmad-bot.log".to_string(),
        code_review_enabled: true,
    }
}

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
// Story fixtures
// ---------------------------------------------------------------------------

/// Build a valid [`StoryInfo`] from key, label, and dependency list.
///
/// Parses the key to extract `epic_num` and `story_num`.
/// Example: `make_test_story("7-1-integration-test", "integration test", vec![])`.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    // Parse: {epic_num}-{story_num}-{slug}
    let mut parts = key.splitn(3, '-');
    let epic_str = parts.next().unwrap_or("");
    let story_str = parts.next().unwrap_or("");
    let epic_num: u32 = epic_str
        .parse()
        .unwrap_or_else(|_| panic!("Invalid epic number in story key: {key}"));
    let story_num: u32 = story_str
        .parse()
        .unwrap_or_else(|_| panic!("Invalid story number in story key: {key}"));

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
// File fixtures
// ---------------------------------------------------------------------------

/// Write a valid `sprint-status.yaml` to a temp directory.
///
/// `entries` contains ALL entry types: epics, stories, and retrospectives.
/// All go under `development_status:` as flat key-value pairs.
///
/// Example:
/// ```ignore
/// write_sprint_status(dir, vec![
///     ("epic-1", "in-progress"),
///     ("1-1-story-slug", "ready-for-dev"),
///     ("epic-1-retrospective", "optional"),
/// ]);
/// ```
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) -> std::path::PathBuf {
    let path = dir.join("sprint-status.yaml");

    let mut content = String::new();
    content.push_str("generated: 2026-02-08\n");
    content.push_str("project: test-project\n");
    content.push_str("project_key: TEST\n");
    content.push_str("tracking_system: file-system\n");
    content.push_str(&format!(
        "story_location: \"{}\"\n",
        dir.display()
    ));
    content.push('\n');
    content.push_str("development_status:\n");
    for (key, status) in &entries {
        content.push_str(&format!("  {key}: {status}\n"));
    }

    std::fs::write(&path, &content).expect("Failed to write sprint-status.yaml");
    path
}

/// Write a valid `.bmad-bot-session.yaml` WAL file to a temp directory.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> std::path::PathBuf {
    let path = dir.join(".bmad-bot-session.yaml");
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
    path
}

/// Initialize a git repo with an initial commit in the given directory.
///
/// Uses Git CLI subprocess calls (no `git2` dependency).
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

/// Build a minimal [`SessionState`] for WAL testing.
pub fn make_test_session_state(story_key: &str) -> SessionState {
    let story = make_test_story(story_key, "test story", vec![]);
    SessionState {
        story_id: story.story_id,
        story_key: story.story_key,
        branch: story.branch_name.clone(),
        started_at: "2026-02-08T10:00:00Z".to_string(),
        last_activity: "2026-02-08T10:05:00Z".to_string(),
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
                content: "Working on it".to_string(),
            },
        ],
    }
}
