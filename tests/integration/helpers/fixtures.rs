//! Fixture builders for integration tests.
//!
//! Provides factory functions that build valid data structures with sensible
//! defaults. Tests can override individual fields as needed.

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

/// Build a valid [`BotConfig`] with sensible defaults.
///
/// The `dir` parameter is used for all path-based fields so tests can
/// pass a `tempdir().path()` and have all file operations isolated.
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
    }
}

// ---------------------------------------------------------------------------
// make_test_secrets
// ---------------------------------------------------------------------------

/// Build a [`BotSecrets`] with dummy tokens. **Never real keys.**
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
/// The `key` follows the format `"1-2-slug"` — epic_num, story_num, slug.
///
/// # Panics
/// Panics if `key` cannot be parsed as a valid story key.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts
        .next()
        .expect("missing epic_num in key")
        .parse()
        .expect("epic_num not a number");
    let story_num: u32 = parts
        .next()
        .expect("missing story_num in key")
        .parse()
        .expect("story_num not a number");

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

/// Write a valid `sprint-status.yaml` file to `dir`.
///
/// `entries` is a list of `(key, status)` tuples that go under
/// `development_status:`. Accepts epic, story, and retrospective entries.
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

    std::fs::write(&path, &yaml).expect("failed to write sprint-status.yaml");
    path
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file to `dir`.
///
/// Returns the path to the written file.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> std::path::PathBuf {
    let path = dir.join(".bmad-bot-session.yaml");
    let yaml = serde_yml::to_string(state).expect("failed to serialize SessionState");
    std::fs::write(&path, &yaml).expect("failed to write WAL file");
    path
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

/// Initialize a git repository in `dir` with an initial empty commit.
///
/// Creates a valid repo with a `main` branch and one commit.
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
    // Ensure "main" branch exists regardless of git config default
    run(&["branch", "-M", "main"]);
}

// ---------------------------------------------------------------------------
// create_test_repo_with_remote
// ---------------------------------------------------------------------------

/// Initialize a git repo in `work_dir` with a bare remote in `bare_dir`.
///
/// After calling this, `git push --force-with-lease origin <branch>` will succeed
/// for any local branch in `work_dir`. The bare repo acts as `origin`.
///
/// # Panics
/// Panics if any git command fails.
pub fn create_test_repo_with_remote(work_dir: &Path, bare_dir: &Path) {
    use std::process::Command;
    let run_in = |dir: &Path, args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed to execute");
        assert!(
            output.status.success(),
            "git {} in {} failed: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    };

    // 1. Create bare repo as origin
    run_in(bare_dir, &["init", "--bare"]);

    // 2. Init working repo
    run_in(work_dir, &["init"]);
    run_in(work_dir, &["config", "user.email", "test@test.com"]);
    run_in(work_dir, &["config", "user.name", "Test"]);
    run_in(
        work_dir,
        &["commit", "--allow-empty", "-m", "initial commit"],
    );
    run_in(work_dir, &["branch", "-M", "main"]);

    // 3. Add bare repo as origin and push main
    let remote_url = bare_dir.display().to_string();
    run_in(work_dir, &["remote", "add", "origin", &remote_url]);
    run_in(work_dir, &["push", "origin", "main"]);
}

// ---------------------------------------------------------------------------
// PipelineTestBuilder
// ---------------------------------------------------------------------------

use bmad_bot::pipeline::{CodeReviewer, DevRunner, StoryPipeline};
use super::mocks::{MockCodeReviewer, MockDevRunner, MockGitProvider, MockNotifier};
use bmad_bot::session::SessionOutcome;
use bmad_bot::review::ReviewOutcome;
use std::sync::Arc;

/// Builder for constructing a fully-mocked `StoryPipeline` for tests.
///
/// After `build()`, returns the pipeline plus cloned mock handles for assertions.
pub struct PipelineTestBuilder {
    config: BotConfig,
    session_outcomes: Vec<SessionOutcome>,
    review_outcome: Option<ReviewOutcome>,
    mock_git: MockGitProvider,
    mock_notifier: MockNotifier,
}

impl PipelineTestBuilder {
    /// Create a new builder with sensible defaults.
    ///
    /// `dir` is used for path-based config fields. For pipeline tests,
    /// `project_root` is set to `dir` itself (not `dir.parent()`) so that
    /// `push_branch()` finds the git repo.
    pub fn new(dir: &Path) -> Self {
        let mut config = make_test_config(dir);
        // Override project_root to point to the git repo work directory
        config.bmad_paths.project_root = dir.display().to_string();
        Self {
            config,
            session_outcomes: vec![],
            review_outcome: None,
            mock_git: MockGitProvider::new(),
            mock_notifier: MockNotifier::new(),
        }
    }

    /// Enable/disable code review in config.
    pub fn with_code_review(mut self, enabled: bool) -> Self {
        self.config.code_review_enabled = enabled;
        self
    }

    /// Set a single session outcome.
    pub fn with_session(mut self, outcome: SessionOutcome) -> Self {
        self.session_outcomes = vec![outcome];
        self
    }

    /// Set multiple session outcomes (for `process_eligible_stories`).
    pub fn with_sessions(mut self, outcomes: Vec<SessionOutcome>) -> Self {
        self.session_outcomes = outcomes;
        self
    }

    /// Set the review outcome.
    pub fn with_review(mut self, outcome: ReviewOutcome) -> Self {
        self.review_outcome = Some(outcome);
        self
    }

    /// Override the git provider mock.
    pub fn with_git_provider(mut self, mock: MockGitProvider) -> Self {
        self.mock_git = mock;
        self
    }

    /// Override the notifier mock.
    pub fn with_notifier(mut self, mock: MockNotifier) -> Self {
        self.mock_notifier = mock;
        self
    }

    /// Build the pipeline and return it along with assertion handles.
    ///
    /// Returns `(StoryPipeline, MockNotifier, MockGitProvider)` where the returned
    /// mocks share internal state with the ones consumed by the pipeline.
    pub fn build(self) -> (StoryPipeline, MockNotifier, MockGitProvider) {
        let notifier_for_assertions = self.mock_notifier.clone();
        let git_for_assertions = self.mock_git.clone();

        let dev_runner: Box<dyn DevRunner> = if self.session_outcomes.len() <= 1 {
            Box::new(MockDevRunner::with_outcome(
                self.session_outcomes
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| SessionOutcome::Completed {
                        story_key: "test".into(),
                        branch: "story/test".into(),
                        decisions: vec![],
                        pr_context: None,
                        pr_how_to_test: None,
                        pr_additional_info: None,
                    }),
            ))
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

        (pipeline, notifier_for_assertions, git_for_assertions)
    }
}
