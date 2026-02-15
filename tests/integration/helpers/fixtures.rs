//! Fixture builder functions for integration tests.
//!
//! Provides helpers to construct valid test data structures and write
//! test files to temporary directories.

use std::path::Path;
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

// ---------------------------------------------------------------------------
// make_test_config
// ---------------------------------------------------------------------------

/// Build a valid `BotConfig` with sensible test defaults.
///
/// Uses the provided `dir` for all path fields so tests operate
/// in isolated temp directories.
pub fn make_test_config(dir: &Path) -> BotConfig {
    let dir_str = dir.display().to_string();

    let planning_path = Path::new(&dir_str).join("planning-artifacts");
    let implementation_path = Path::new(&dir_str).join("implementation-artifacts");
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
            project_root: dir_str.clone(),
            output_folder: dir_str.clone(),
            planning_artifacts: planning_path.display().to_string(),
            implementation_artifacts: implementation_path.display().to_string(),
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

/// Build a `BotSecrets` with dummy tokens — never real keys.
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

/// Build a valid `StoryInfo` from a key, label, and dependency list.
///
/// `key` format: `"7-1-integration-test-infrastructure"`
/// `label`: human-readable label
/// `deps`: list of story keys this story depends on
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

/// Write a valid `sprint-status.yaml` to a temp directory.
///
/// `entries` is a list of `(key, status)` pairs that go under `development_status:`.
/// Accepts epic entries, story entries, and retrospective entries.
pub fn write_sprint_status(dir: &Path, entries: &[(&str, &str)]) {
    let mut yaml = String::new();
    yaml.push_str("generated: 2026-02-08\n");
    yaml.push_str("project: test-project\n");
    yaml.push_str("project_key: TEST\n");
    yaml.push_str("tracking_system: file-system\n");
    yaml.push_str(&format!(
        "story_location: \"{}\"\n",
        dir.display()
    ));
    yaml.push_str("\ndevelopment_status:\n");

    for (key, status) in entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }

    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write sprint-status.yaml");
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file to a temp directory.
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    let path = dir.join(".bmad-bot-session.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

/// Initialize a git repo with an initial commit in a temp directory.
///
/// Uses Git CLI subprocess calls (no git2).
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

/// Initialize a git repo with a local bare remote so `git push` works.
///
/// Creates the git repo in `repo_dir`, and a bare remote at `repo_dir/../origin.git`.
/// The branch specified by `story_branch` is created with an empty commit.
pub fn create_test_repo_with_remote(repo_dir: &Path, story_branch: &str) {
    use std::process::Command;
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

    // Create a bare "remote" repository
    let remote_dir = repo_dir.parent().unwrap().join("origin.git");
    std::fs::create_dir_all(&remote_dir).unwrap();
    run_in(&remote_dir, &["init", "--bare"]);

    // Initialize the working repo
    run_in(repo_dir, &["init"]);
    run_in(repo_dir, &["config", "user.email", "test@test.com"]);
    run_in(repo_dir, &["config", "user.name", "Test"]);
    run_in(repo_dir, &["commit", "--allow-empty", "-m", "initial commit"]);
    run_in(repo_dir, &["branch", "-M", "main"]);

    // Add the bare repo as "origin"
    run_in(
        repo_dir,
        &["remote", "add", "origin", remote_dir.to_str().unwrap()],
    );
    // Push main to origin so the remote has a branch
    run_in(repo_dir, &["push", "origin", "main"]);

    // Create the story branch with an empty commit
    run_in(repo_dir, &["checkout", "-b", story_branch]);
    run_in(
        repo_dir,
        &["commit", "--allow-empty", "-m", "story work"],
    );
}

// ---------------------------------------------------------------------------
// PipelineTestBuilder
// ---------------------------------------------------------------------------

/// Builder for constructing a `StoryPipeline` with mock dependencies.
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
    /// Create a builder with sensible defaults: code_review_enabled = true.
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

    /// Build the pipeline, returning cloned mock handles for assertions.
    pub fn build(self) -> (StoryPipeline, MockNotifier, MockGitProvider) {
        self.build_inner()
    }

    /// Build the pipeline with a custom config (overrides the builder's config).
    /// Preserves `code_review_enabled` from the builder if it was explicitly set.
    pub fn build_with_config(
        mut self,
        mut config: BotConfig,
    ) -> (StoryPipeline, MockNotifier, MockGitProvider) {
        // Preserve builder's code_review_enabled override
        config.code_review_enabled = self.config.code_review_enabled;
        self.config = config;
        self.build_inner()
    }

    fn build_inner(self) -> (StoryPipeline, MockNotifier, MockGitProvider) {
        let notifier_for_assertions = self.mock_notifier.clone();
        let git_for_assertions = self.mock_git.clone();

        let dev_runner: Box<dyn DevRunner> = if self.session_outcomes.len() <= 1 {
            Box::new(MockDevRunner::with_outcome(
                self.session_outcomes
                    .into_iter()
                    .next()
                    .unwrap_or(SessionOutcome::Completed {
                        story_key: "test-story".into(),
                        branch: "story/test-story".into(),
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
