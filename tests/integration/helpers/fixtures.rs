//! Fixture builder functions for integration tests.
//!
//! Provides helpers to construct valid test data structures without touching
//! real APIs or persistent state. All filesystem operations use `tempfile`
//! directories that auto-clean on drop.

use std::path::{Path, PathBuf};
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

/// Temporary directory wrapper that holds both the working repo and the bare remote.
/// Keeps both `tempfile::TempDir` handles alive for the test lifetime.
pub struct PipelineTestEnv {
    /// Working repo directory (used as project_root).
    pub work_dir: tempfile::TempDir,
    /// Bare remote repo directory (the "origin").
    _remote_dir: tempfile::TempDir,
}

// ---------------------------------------------------------------------------
// impl_artifacts_dir
// ---------------------------------------------------------------------------

/// Create the `_bmad-output/implementation-artifacts` subdirectory under `root`
/// and return its path.  `make_test_config(root)` sets
/// `bmad_paths.implementation_artifacts` to this location.
pub fn impl_artifacts_dir(root: &Path) -> PathBuf {
    let dir = root.join("_bmad-output/implementation-artifacts");
    std::fs::create_dir_all(&dir).expect("create impl artifacts dir");
    dir
}

// ---------------------------------------------------------------------------
// make_test_config
// ---------------------------------------------------------------------------

/// Build a complete valid [`BotConfig`] rooted at the given temp directory.
///
/// Sensible defaults: polling=60, provider=github, review=enabled.
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
            planning_artifacts: dir.join("_bmad-output/planning-artifacts").display().to_string(),
            implementation_artifacts: dir
                .join("_bmad-output/implementation-artifacts")
                .display()
                .to_string(),
        },
        log_format: "pretty".to_string(),
        log_level: "info".to_string(),
        log_file: "bmad-bot.log".to_string(),
        code_review_enabled: true,
        mcp_servers: vec![],
    }
}

// ---------------------------------------------------------------------------
// make_test_secrets
// ---------------------------------------------------------------------------

/// Build [`BotSecrets`] with dummy tokens — NEVER real keys.
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

/// Build a valid [`StoryInfo`] from a story key, label, and dependency list.
///
/// The key format is `"{epic_num}-{story_num}-{slug}"` (e.g., `"7-1-integration-test"`).
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    let parts: Vec<&str> = key.splitn(3, '-').collect();
    let epic_num: u32 = parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
    let story_num: u32 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
    let story_id = format!("{epic_num}.{story_num}");
    let branch_name = format!("story/{key}");
    let specs_path =
        PathBuf::from(format!("_bmad-output/implementation-artifacts/{key}.md"));

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

/// Write a valid `sprint-status.yaml` to `dir` with the given story entries.
///
/// Each entry is a `(key, status)` tuple. Supports epics, stories, and retrospectives.
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) -> PathBuf {
    let path = dir.join("sprint-status.yaml");
    let mut yaml = String::new();
    yaml.push_str("generated: 2026-02-08\n");
    yaml.push_str("project: test-project\n");
    yaml.push_str("project_key: TEST\n");
    yaml.push_str("tracking_system: file-system\n");
    yaml.push_str(&format!(
        "story_location: \"{}\"\n",
        dir.display()
    ));
    yaml.push('\n');
    yaml.push_str("development_status:\n");
    for (key, status) in &entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }
    std::fs::write(&path, &yaml).expect("Failed to write sprint-status.yaml");
    path
}

// ---------------------------------------------------------------------------
// write_wal_file
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file to `dir`.
pub fn write_wal_file(dir: &Path, state: &SessionState) -> PathBuf {
    let path = dir.join(".bmad-bot-session.yaml");
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
    path
}

// ---------------------------------------------------------------------------
// create_test_repo
// ---------------------------------------------------------------------------

/// Initialize a git repo with an initial commit in `dir` via Git CLI.
///
/// Creates a bare repo with one empty commit on the `main` branch.
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

// ---------------------------------------------------------------------------
// create_test_repo_with_remote
// ---------------------------------------------------------------------------

/// Initialize a git working repo with a local bare "origin" remote.
///
/// Returns a [`PipelineTestEnv`] that keeps both directories alive.
/// The working repo has an initial commit on `main` and is configured
/// so that `git push --force-with-lease origin <branch>` succeeds.
pub fn create_test_repo_with_remote() -> PipelineTestEnv {
    use std::process::Command;

    let remote_dir = tempfile::tempdir().expect("create remote temp dir");
    let work_dir = tempfile::tempdir().expect("create work temp dir");

    let run_in = |dir: &Path, args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {} in {:?} failed: {}",
            args.join(" "),
            dir,
            String::from_utf8_lossy(&output.stderr)
        );
    };

    // Create bare remote
    run_in(remote_dir.path(), &["init", "--bare"]);

    // Create working repo
    run_in(work_dir.path(), &["init"]);
    run_in(work_dir.path(), &["config", "user.email", "test@test.com"]);
    run_in(work_dir.path(), &["config", "user.name", "Test"]);
    run_in(
        work_dir.path(),
        &[
            "remote",
            "add",
            "origin",
            remote_dir.path().to_str().unwrap(),
        ],
    );
    run_in(
        work_dir.path(),
        &["commit", "--allow-empty", "-m", "initial commit"],
    );
    run_in(work_dir.path(), &["branch", "-M", "main"]);
    run_in(work_dir.path(), &["push", "-u", "origin", "main"]);

    PipelineTestEnv {
        work_dir,
        _remote_dir: remote_dir,
    }
}

/// Create a story branch in the test repo with at least one commit.
///
/// This ensures `git push --force-with-lease origin <branch>` can succeed.
pub fn create_story_branch(repo_dir: &Path, branch_name: &str) {
    use std::process::Command;
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_dir)
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["checkout", "-b", branch_name]);
    run(&["commit", "--allow-empty", "-m", "story work"]);
}

/// Ergonomic builder for constructing a [`StoryPipeline`] with mock dependencies.
///
/// After `build()`, the returned `MockNotifier` and `MockGitProvider` share
/// internal `Arc<Mutex<Vec<...>>>` capture buffers with the copies inside the
/// pipeline, so test assertions see all calls made during `process_story()`.
pub struct PipelineTestBuilder {
    config: BotConfig,
    session_outcomes: Vec<SessionOutcome>,
    review_outcome: Option<ReviewOutcome>,
    mock_git: MockGitProvider,
    mock_notifier: MockNotifier,
    /// Keep the test environment alive (holds tempdir handles).
    env: PipelineTestEnv,
    /// Story branches to create before building the pipeline.
    branches: Vec<String>,
}

impl PipelineTestBuilder {
    /// Create a builder with sensible defaults (review enabled, success git provider).
    ///
    /// Sets up a git repo with a bare remote so `push_branch()` succeeds.
    pub fn new() -> Self {
        let env = create_test_repo_with_remote();
        let config = make_test_config(env.work_dir.path());
        Self {
            config,
            session_outcomes: vec![],
            review_outcome: None,
            mock_git: MockGitProvider::new(),
            mock_notifier: MockNotifier::new(),
            env,
            branches: vec![],
        }
    }

    pub fn with_code_review(mut self, enabled: bool) -> Self {
        self.config.code_review_enabled = enabled;
        self
    }

    pub fn with_session(mut self, outcome: SessionOutcome) -> Self {
        // Auto-detect branch from outcome for git setup
        match &outcome {
            SessionOutcome::Completed { branch, .. } => {
                self.branches.push(branch.clone());
            }
            SessionOutcome::Failed { story_key, .. } => {
                self.branches.push(format!("story/{story_key}"));
            }
            SessionOutcome::Escalated { report, .. } => {
                self.branches.push(report.branch_name.clone());
            }
        }
        self.session_outcomes = vec![outcome];
        self
    }

    pub fn with_sessions(mut self, outcomes: Vec<SessionOutcome>) -> Self {
        for outcome in &outcomes {
            match outcome {
                SessionOutcome::Completed { branch, .. } => {
                    self.branches.push(branch.clone());
                }
                SessionOutcome::Failed { story_key, .. } => {
                    self.branches.push(format!("story/{story_key}"));
                }
                SessionOutcome::Escalated { report, .. } => {
                    self.branches.push(report.branch_name.clone());
                }
            }
        }
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

    /// Build the pipeline and return handles for assertions.
    ///
    /// Returns `(StoryPipeline, MockNotifier, MockGitProvider, PipelineTestEnv)` where:
    /// - The mock handles share internal `Arc<Mutex<...>>` capture buffers with the
    ///   copies inside the pipeline (all calls are visible for assertion).
    /// - `PipelineTestEnv` holds the temp git directories — the caller MUST bind it
    ///   (e.g. `let _env = env;`) so the tempdirs live for the full test.
    ///
    /// Also creates required story branches in the test git repo so that
    /// `push_branch()` can succeed during `process_story()`.
    pub fn build(self) -> (StoryPipeline, MockNotifier, MockGitProvider, PipelineTestEnv) {
        // Create story branches in the test repo
        let repo_dir = self.env.work_dir.path();
        for branch in &self.branches {
            // Return to main before creating a new branch
            let _ = std::process::Command::new("git")
                .args(["checkout", "main"])
                .current_dir(repo_dir)
                .output();
            create_story_branch(repo_dir, branch);
        }

        let notifier_for_assertions = self.mock_notifier.clone();
        let git_for_assertions = self.mock_git.clone();

        // Wire the shared event log from git → reviewer for ordering assertions.
        let event_log = self.mock_git.shared_event_log();

        let dev_runner: Box<dyn DevRunner> = if self.session_outcomes.len() == 1 {
            let outcome = self
                .session_outcomes
                .into_iter()
                .next()
                .expect("PipelineTestBuilder: at least one session outcome required");
            Box::new(MockDevRunner::with_outcome(outcome))
        } else {
            Box::new(MockDevRunner::with_outcomes(self.session_outcomes))
        };

        let code_reviewer: Box<dyn CodeReviewer> = match self.review_outcome {
            Some(o) => Box::new(MockCodeReviewer::with_outcome(o).with_event_log(event_log)),
            None => Box::new(MockCodeReviewer::never_called().with_event_log(event_log)),
        };

        let pipeline = StoryPipeline::new_with_components(
            Arc::new(self.config),
            Box::new(self.mock_git),
            Box::new(self.mock_notifier),
            dev_runner,
            code_reviewer,
        );

        (pipeline, notifier_for_assertions, git_for_assertions, self.env)
    }
}
