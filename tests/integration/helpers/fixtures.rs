//! Fixture builder functions for integration tests.
//!
//! Provides helpers to create valid test data: configs, secrets, stories,
//! sprint-status YAML files, WAL files, and git repositories.

use std::path::Path;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::session::state::{ChatMessage, SessionState};
use bmad_bot::watcher::StoryInfo;

/// Build a valid `BotConfig` with sensible test defaults.
///
/// Uses the provided directory as the base for all path fields.
/// Polling interval: 60s, provider: github, review: enabled.
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
    }
}

/// Build `BotSecrets` with dummy tokens — never real keys.
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

/// Build a `StoryInfo` from a key, label, and dependency list.
///
/// Parses the key to extract `epic_num`, `story_num`, and slug. The `label` parameter
/// overrides the slug-derived label if non-empty; otherwise the slug is used.
///
/// # Arguments
/// - `key` — e.g., `"7-1-integration-test-infrastructure"`
/// - `label` — human-readable label; if empty, derived from the key slug
/// - `deps` — list of story keys this story depends on
///
/// # Panics
/// Panics if the key does not match the expected `{epic}-{story}-{slug}` format.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts
        .next()
        .expect("key must have epic number")
        .parse()
        .expect("epic_num must be numeric");
    let story_num: u32 = parts
        .next()
        .expect("key must have story number")
        .parse()
        .expect("story_num must be numeric");
    let slug = parts.next().unwrap_or("");

    let effective_label = if label.is_empty() {
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
        label: effective_label,
        branch_name,
        specs_path,
        dependencies: deps,
        status: "ready-for-dev".to_string(),
    }
}

/// Write a valid `sprint-status.yaml` to a temp directory.
///
/// # Arguments
/// - `dir` — directory to write the file to
/// - `stories` — list of `(key, status)` entries, including epics and retrospectives
///
/// All entries are written under `development_status:` as flat key-value pairs.
pub fn write_sprint_status(dir: &Path, stories: &[(&str, &str)]) {
    let mut content = String::from(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n\
         story_location: \".\"\n\n\
         development_status:\n",
    );

    for (key, status) in stories {
        content.push_str(&format!("  {key}: {status}\n"));
    }

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &content).expect("write sprint-status.yaml");
}

/// Write a valid `.bmad-bot-session.yaml` WAL file to a directory.
///
/// # Arguments
/// - `dir` — directory to write the file to
/// - `state` — the session state to serialize
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let yaml = serde_yml::to_string(state).expect("serialize SessionState");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(&path, &yaml).expect("write WAL file");
}

/// Initialize a git repo with an initial commit in the given directory.
///
/// Uses `git2` to create a repo, write an empty tree, and make an initial commit
/// so that `HEAD` exists.
///
/// # Returns
/// The initialized `git2::Repository`.
pub fn create_test_repo(dir: &Path) -> git2::Repository {
    let repo = git2::Repository::init(dir).expect("git init");
    let sig = git2::Signature::now("Test", "test@test.com").expect("signature");
    let tree_id = repo
        .index()
        .expect("index")
        .write_tree()
        .expect("write tree");
    {
        let tree = repo.find_tree(tree_id).expect("find tree");
        repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
            .expect("commit");
    }
    repo
}

/// Create a `SessionState` for testing WAL file operations.
///
/// # Arguments
/// - `story_key` — the story key to embed in the state
pub fn make_test_session_state(story_key: &str) -> SessionState {
    let story = make_test_story(story_key, "", Vec::new());
    let mut state = SessionState::new(&story, "anthropic", "claude-sonnet-4-20250514");
    state.set_branch_info(&format!("story/{story_key}"), "main");
    state.chat_history.push(ChatMessage {
        role: "user".to_string(),
        content: "DS".to_string(),
    });
    state
}
