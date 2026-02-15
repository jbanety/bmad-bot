//! Fixture builders for integration tests.
//!
//! All builders produce valid data structures with sensible defaults.
//! Use `tempfile::tempdir()` for any test that touches the filesystem.

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

/// Build a valid `BotConfig` using the provided temp directory for paths.
///
/// Defaults: polling=60, provider=github, review=enabled.
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

/// Build a `BotSecrets` with dummy tokens (never real keys).
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

/// Build a valid `StoryInfo` from a key, label, and dependency list.
///
/// Key format: `{epic_num}-{story_num}-{slug}` (e.g., `"7-1-integration-test-infrastructure"`).
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
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
        dependencies: deps,
        status: "ready-for-dev".to_string(),
    }
}

/// Write a valid `sprint-status.yaml` to the given directory.
///
/// `entries` is a list of `(key, status)` pairs that go under `development_status:`.
/// Accepts epic entries, story entries, and retrospective entries.
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) {
    let story_location = dir.display();
    let mut yaml = format!(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n\
         story_location: \"{story_location}\"\n\
         \n\
         development_status:\n",
    );
    for (key, status) in entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }
    let path = dir.join("sprint-status.yaml");
    std::fs::write(path, yaml).expect("Failed to write sprint-status.yaml");
}

/// Write a valid WAL file (`.bmad-bot-session.yaml`) to the given directory.
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(path, yaml).expect("Failed to write WAL file");
}

/// Initialize a git repo with an initial commit in the given directory.
///
/// Uses Git CLI (no `git2` dependency). Creates a "main" branch.
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

/// Create a git repo with a local bare "remote" that supports push.
///
/// Returns `(work_dir, bare_dir)` where `work_dir` has `origin` pointing to `bare_dir`.
/// A story branch is created and a dummy commit pushed so that `git push origin {branch}` works.
pub fn create_test_repo_with_remote(work_dir: &Path, bare_dir: &Path, branch_name: &str) {
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

    // 1. Init bare repo
    run_in(bare_dir, &["init", "--bare"]);

    // 2. Init work repo
    run_in(work_dir, &["init"]);
    run_in(work_dir, &["config", "user.email", "test@test.com"]);
    run_in(work_dir, &["config", "user.name", "Test"]);
    run_in(
        work_dir,
        &["remote", "add", "origin", bare_dir.to_str().unwrap()],
    );
    run_in(
        work_dir,
        &["commit", "--allow-empty", "-m", "initial commit"],
    );
    run_in(work_dir, &["branch", "-M", "main"]);
    run_in(work_dir, &["push", "-u", "origin", "main"]);

    // 3. Create story branch with a commit
    run_in(work_dir, &["checkout", "-b", branch_name]);
    run_in(
        work_dir,
        &["commit", "--allow-empty", "-m", "story work"],
    );
}

// ---------------------------------------------------------------------------
// PipelineTestBuilder
// ---------------------------------------------------------------------------

/// Builder for constructing `StoryPipeline` instances with mock dependencies.
///
/// `build()` returns the pipeline plus cloned mock handles that share
/// internal `Arc` state with the pipeline's copies — enabling post-run assertions.
pub struct PipelineTestBuilder {
    config: BotConfig,
    session_outcomes: Vec<SessionOutcome>,
    review_outcome: Option<ReviewOutcome>,
    mock_git: MockGitProvider,
    mock_notifier: MockNotifier,
}

impl PipelineTestBuilder {
    /// Create builder with sensible defaults: review enabled, default mocks.
    pub fn new() -> Self {
        let tmp = std::env::temp_dir().join("pipeline-test-default");
        Self {
            config: make_test_config(&tmp),
            session_outcomes: vec![],
            review_outcome: None,
            mock_git: MockGitProvider::new(),
            mock_notifier: MockNotifier::new(),
        }
    }

    pub fn with_config(mut self, config: BotConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_code_review(mut self, enabled: bool) -> Self {
        self.config.code_review_enabled = enabled;
        self
    }

    pub fn with_session(mut self, outcome: SessionOutcome) -> Self {
        self.session_outcomes = vec![outcome];
        self
    }

    pub fn with_sessions(mut self, outcomes: Vec<SessionOutcome>) -> Self {
        self.session_outcomes = outcomes;
        self
    }

    pub fn with_review(mut self, outcome: ReviewOutcome) -> Self {
        self.review_outcome = Some(outcome);
        self
    }

    pub fn with_git_provider(mut self, mock: MockGitProvider) -> Self {
        self.mock_git = mock;
        self
    }

    pub fn with_notifier(mut self, mock: MockNotifier) -> Self {
        self.mock_notifier = mock;
        self
    }

    /// Build the pipeline and return assertion handles.
    ///
    /// Returns `(StoryPipeline, MockNotifier, MockGitProvider)` where the mock
    /// handles share internal state with the pipeline's copies.
    pub fn build(self) -> (StoryPipeline, MockNotifier, MockGitProvider) {
        let notifier_for_assertions = self.mock_notifier.clone();
        let git_for_assertions = self.mock_git.clone();

        let dev_runner: Box<dyn DevRunner> = if self.session_outcomes.len() <= 1 {
            let outcome = self.session_outcomes.into_iter().next().unwrap_or(
                SessionOutcome::Completed {
                    story_key: "test".into(),
                    branch: "story/test".into(),
                    decisions: vec![],
                    pr_context: None,
                    pr_how_to_test: None,
                    pr_additional_info: None,
                },
            );
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

        (pipeline, notifier_for_assertions, git_for_assertions)
    }
}
