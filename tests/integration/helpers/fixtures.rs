//! Fixture builders for integration tests.
//!
//! Each builder produces a valid, self-contained test artifact.
//! Uses `tempfile` for filesystem isolation — temp dirs are cleaned up on `Drop`.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};
use bmad_bot::pipeline::{CodeReviewer, DevRunner, StoryPipeline};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::session::SessionState;
use bmad_bot::watcher::StoryInfo;

use super::mocks::{MockCodeReviewer, MockDevRunner, MockGitProvider, MockNotifier};

/// Build a complete valid `BotConfig` using the provided temp directory for all paths.
///
/// Returns a config with sensible defaults for testing:
/// - `polling_interval_secs`: 60
/// - `git_provider`: github, `test-owner/test-repo`, target `main`
/// - `llm`: all roles set to `anthropic`/`test-model`
/// - `notifications`: telegram disabled
/// - `code_review_enabled`: true
pub fn make_test_config(dir: &Path) -> BotConfig {
    let dir_str = dir.to_string_lossy().to_string();
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
            project_root: dir_str.clone(),
            output_folder: format!("{dir_str}/_bmad-output"),
            planning_artifacts: format!("{dir_str}/_bmad-output/planning-artifacts"),
            implementation_artifacts: format!("{dir_str}/_bmad-output/implementation-artifacts"),
        },
        log_format: "pretty".to_string(),
        log_level: "info".to_string(),
        log_file: format!("{dir_str}/bmad-bot.log"),
        mcp_servers: vec![],
    }
}

/// Build a `BotSecrets` with dummy tokens for all providers.
///
/// **Never use real keys.** All values are prefixed with `test-` and suffixed
/// with `-DO-NOT-USE`.
pub fn make_test_secrets() -> BotSecrets {
    BotSecrets {
        anthropic_api_key: Some("test-anthropic-key-DO-NOT-USE".into()),
        openai_api_key: Some("test-openai-key-DO-NOT-USE".into()),
        github_copilot_oauth_token: Some("test-copilot-token-DO-NOT-USE".into()),
        github_token: Some("test-github-token-DO-NOT-USE".into()),
        gitlab_token: Some("test-gitlab-token-DO-NOT-USE".into()),
        telegram_bot_token: Some("test-telegram-token-DO-NOT-USE".into()),
    }
}

/// Build a valid `StoryInfo` from key components.
///
/// # Arguments
/// - `key`: Full story key (e.g., `"7-1-integration-test-infrastructure"`)
/// - `label`: Human-readable label (e.g., `"integration test infrastructure"`)
/// - `deps`: Dependency story keys (e.g., `vec!["6-3-crash-recovery"]`)
///
/// Parses `key` to extract `epic_num` and `story_num`. Status defaults to
/// `"ready-for-dev"`.
pub fn make_test_story(key: &str, label: &str, deps: Vec<&str>) -> StoryInfo {
    let parts: Vec<&str> = key.splitn(3, '-').collect();
    let epic_num: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let story_num: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
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
        dependencies: deps.into_iter().map(|s| s.to_string()).collect(),
        status: "ready-for-dev".to_string(),
    }
}

/// Write a valid `sprint-status.yaml` to the given directory.
///
/// # Arguments
/// - `dir`: Target directory (usually a `tempdir()` path)
/// - `entries`: Flat list of `(key, status)` pairs — supports epic, story, and
///   retrospective entries
///
/// The generated file includes all required top-level fields and a
/// `development_status:` mapping.
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) -> std::path::PathBuf {
    let mut yaml = String::from(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n",
    );
    yaml.push_str(&format!(
        "story_location: \"{}\"\n",
        dir.to_string_lossy()
    ));
    yaml.push_str("\ndevelopment_status:\n");
    for (key, status) in &entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write sprint-status.yaml");
    path
}

/// Write a valid WAL file (`.bmad-bot-session.yaml`) to the given directory.
///
/// # Arguments
/// - `dir`: Target directory
/// - `state`: The `SessionState` to serialize
///
/// Returns the path to the written file.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> std::path::PathBuf {
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
    path
}

/// Initialize a git repository with an initial commit in the given directory.
///
/// Creates a repo with:
/// - `git init`
/// - `user.email` = `test@test.com`, `user.name` = `Test`
/// - An empty initial commit
/// - `main` branch (renamed from whatever default the system uses)
pub fn create_test_repo(dir: &Path) {
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

/// Set up a git repository with a local bare remote for pipeline tests.
///
/// Creates:
/// 1. A bare "remote" repo at `{parent}/remote.git`
/// 2. A working clone at `{parent}/work` with `origin` pointing to the bare repo
/// 3. Optionally creates story branches with a dummy commit
///
/// Returns the working directory path (to use as `project_root` in config).
pub fn create_pipeline_git_env(parent: &Path, branches: &[&str]) -> std::path::PathBuf {
    let bare_dir = parent.join("remote.git");
    let work_dir = parent.join("work");

    std::fs::create_dir_all(&bare_dir).expect("create bare dir");

    // Init bare repo
    let run_in = |dir: &Path, args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {} in {} failed: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    };

    run_in(&bare_dir, &["init", "--bare"]);

    // Clone bare repo to working dir
    let output = Command::new("git")
        .args(["clone", bare_dir.to_str().unwrap(), work_dir.to_str().unwrap()])
        .output()
        .expect("git clone failed");
    assert!(
        output.status.success(),
        "git clone failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    run_in(&work_dir, &["config", "user.email", "test@test.com"]);
    run_in(&work_dir, &["config", "user.name", "Test"]);

    // Create initial commit on main and push
    run_in(&work_dir, &["commit", "--allow-empty", "-m", "initial commit"]);
    run_in(&work_dir, &["branch", "-M", "main"]);
    run_in(&work_dir, &["push", "-u", "origin", "main"]);

    // Create story branches with a dummy commit each
    for branch in branches {
        run_in(&work_dir, &["checkout", "-b", branch]);
        run_in(&work_dir, &["commit", "--allow-empty", "-m", &format!("work on {branch}")]);
        run_in(&work_dir, &["checkout", "main"]);
    }

    work_dir
}

// ---------------------------------------------------------------------------
// PipelineTestBuilder
// ---------------------------------------------------------------------------

/// Builder for constructing `StoryPipeline` with mock dependencies.
///
/// After `build()`, the returned mock handles share state with the pipeline’s
/// copies via `Arc` — assertions on the handles reflect calls made through the pipeline.
pub struct PipelineTestBuilder {
    config: BotConfig,
    session_outcomes: Vec<SessionOutcome>,
    review_outcome: Option<ReviewOutcome>,
    mock_git: MockGitProvider,
    mock_notifier: MockNotifier,
    /// Temp dir that owns the git repo — keep alive until test completes.
    _temp_dir: Option<tempfile::TempDir>,
}

impl PipelineTestBuilder {
    /// Create a new builder with a real git repo (bare remote + working clone).
    ///
    /// The `branches` parameter specifies story branches to create (e.g., `["story/4-1-rig-tools"]`).
    /// The `project_root` in the config will point to the working clone.
    pub fn new_with_git(branches: &[&str]) -> Self {
        let temp_dir = tempfile::tempdir().expect("create tempdir");
        let work_dir = create_pipeline_git_env(temp_dir.path(), branches);
        Self {
            config: make_test_config(&work_dir),
            session_outcomes: vec![],
            review_outcome: None,
            mock_git: MockGitProvider::new(),
            mock_notifier: MockNotifier::new(),
            _temp_dir: Some(temp_dir),
        }
    }

    /// Create a new builder with sensible defaults (no git repo).
    pub fn new() -> Self {
        let tmp = std::env::temp_dir().join("pipeline-test-default");
        Self {
            config: make_test_config(&tmp),
            session_outcomes: vec![],
            review_outcome: None,
            mock_git: MockGitProvider::new(),
            mock_notifier: MockNotifier::new(),
            _temp_dir: None,
        }
    }

    /// Enable or disable code review in config.
    pub fn with_code_review(mut self, enabled: bool) -> Self {
        self.config.code_review_enabled = enabled;
        self
    }

    /// Set a single session outcome.
    pub fn with_session(mut self, outcome: SessionOutcome) -> Self {
        self.session_outcomes = vec![outcome];
        self
    }

    /// Set multiple session outcomes (for batch processing).
    pub fn with_sessions(mut self, outcomes: Vec<SessionOutcome>) -> Self {
        self.session_outcomes = outcomes;
        self
    }

    /// Set the review outcome.
    pub fn with_review(mut self, outcome: ReviewOutcome) -> Self {
        self.review_outcome = Some(outcome);
        self
    }

    /// Replace the default mock git provider.
    pub fn with_git_provider(mut self, mock: MockGitProvider) -> Self {
        self.mock_git = mock;
        self
    }

    /// Replace the default mock notifier.
    pub fn with_notifier(mut self, mock: MockNotifier) -> Self {
        self.mock_notifier = mock;
        self
    }

    /// Build the pipeline, returning shared handles for assertions.
    ///
    /// **Important:** The returned tuple includes a `TempDir` guard. The git repo is
    /// deleted when this guard is dropped. Keep it alive for the duration of the test.
    pub fn build(self) -> (StoryPipeline, MockNotifier, MockGitProvider, Option<tempfile::TempDir>) {
        let notifier_for_assertions = self.mock_notifier.clone();
        let git_for_assertions = self.mock_git.clone();

        let dev_runner: Box<dyn DevRunner> = if self.session_outcomes.len() <= 1 {
            let outcome = self
                .session_outcomes
                .into_iter()
                .next()
                .unwrap_or_else(|| SessionOutcome::Completed {
                    story_key: "test".to_string(),
                    branch: "story/test".to_string(),
                    decisions: vec![],
                    pr_context: None,
                    pr_how_to_test: None,
                    pr_additional_info: None,
                });
            Box::new(MockDevRunner::with_outcome(outcome))
        } else {
            Box::new(MockDevRunner::with_outcomes(self.session_outcomes))
        };

        let code_reviewer: Box<dyn CodeReviewer> = match self.review_outcome {
            Some(o) => Box::new(MockCodeReviewer::with_outcome(o)),
            None => Box::new(MockCodeReviewer::never_called()),
        };

        let pipeline = StoryPipeline::new_with_components(
            Arc::new(self.config),
            Box::new(self.mock_git),
            Box::new(self.mock_notifier),
            dev_runner,
            code_reviewer,
        );

        (pipeline, notifier_for_assertions, git_for_assertions, self._temp_dir)
    }
}
