//! Fixture builders for integration tests.
//!
//! Each builder produces a valid data structure with sensible defaults.
//! All filesystem operations use `tempfile::tempdir()` for isolation.

use std::path::Path;
use std::sync::Arc;

use bmad_bot::config::{
    BmadPathsConfig, BotConfig, BotSecrets, GitProviderConfig, LlmConfig, LlmRoleConfig,
    McpServerConfig, NotificationConfig, TelegramConfig,
};
use bmad_bot::pipeline::{CodeReviewer, DevRunner, StoryPipeline};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::session::SessionState;
use bmad_bot::watcher::StoryInfo;

use super::mocks::{MockCodeReviewer, MockCodeReviewerHandle, MockDevRunner, MockGitProvider, MockNotifier};

// ---------------------------------------------------------------------------
// make_test_config (Task 6.1)
// ---------------------------------------------------------------------------

/// Build a complete valid [`BotConfig`] using the provided temp directory.
///
/// Defaults: polling=60s, provider=github, review=enabled.
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
                model: "test-dev-model".to_string(),
                reasoning_effort: None,
            },
            review: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-review-model".to_string(),
                reasoning_effort: None,
            },
            supervisor: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-supervisor-model".to_string(),
                reasoning_effort: None,
            },
        },
        notifications: NotificationConfig {
            telegram: TelegramConfig {
                enabled: true,
                chat_id: "test-chat-id".to_string(),
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
        mcp_servers: Vec::<McpServerConfig>::new(),
    }
}

// ---------------------------------------------------------------------------
// make_test_secrets (Task 6.2)
// ---------------------------------------------------------------------------

/// Build [`BotSecrets`] with dummy tokens. NEVER real keys.
pub fn make_test_secrets() -> BotSecrets {
    BotSecrets {
        anthropic_api_key: Some("test-anthropic-key-DO-NOT-USE".into()),
        openai_api_key: Some("test-openai-key-DO-NOT-USE".into()),
        github_copilot_oauth_token: Some("test-ghcopilot-key-DO-NOT-USE".into()),
        github_token: Some("test-github-token-DO-NOT-USE".into()),
        gitlab_token: Some("test-gitlab-token-DO-NOT-USE".into()),
        telegram_bot_token: Some("test-telegram-token-DO-NOT-USE".into()),
    }
}

// ---------------------------------------------------------------------------
// make_test_story (Task 6.3)
// ---------------------------------------------------------------------------

/// Build a valid [`StoryInfo`] from a key like `"7-1-integration-test-infrastructure"`.
///
/// Parses the key to extract `epic_num`, `story_num`, `label`.
/// Dependencies are passed as a `Vec<String>`.
pub fn make_test_story(key: &str, label: &str, deps: Vec<String>) -> StoryInfo {
    // Parse epic_num and story_num from key
    let mut parts = key.splitn(3, '-');
    let epic_num: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let story_num: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);

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
// write_sprint_status (Task 6.4)
// ---------------------------------------------------------------------------

/// Write a valid `sprint-status.yaml` to `dir` with given entries.
///
/// `entries` is a `Vec<(&str, &str)>` of `(key, status)` pairs that go under
/// `development_status:`. Accepts epic, story, and retrospective entries.
pub fn write_sprint_status(dir: &Path, entries: Vec<(&str, &str)>) {
    let mut yaml = format!(
        "generated: 2026-02-08\n\
         project: test-project\n\
         project_key: TEST\n\
         tracking_system: file-system\n\
         story_location: \"{}\"\n\
         \n\
         development_status:\n",
        dir.display()
    );
    for (key, status) in &entries {
        yaml.push_str(&format!("  {key}: {status}\n"));
    }
    let path = dir.join("sprint-status.yaml");
    std::fs::write(&path, &yaml).expect("Failed to write sprint-status.yaml");
}

// ---------------------------------------------------------------------------
// write_wal_file (Task 6.5)
// ---------------------------------------------------------------------------

/// Write a valid `.bmad-bot-session.yaml` WAL file from a [`SessionState`].
pub fn write_wal_file(dir: &Path, state: &SessionState) {
    let path = dir.join(".bmad-bot-session.yaml");
    let yaml = serde_yml::to_string(state).expect("Failed to serialize SessionState");
    std::fs::write(&path, &yaml).expect("Failed to write WAL file");
}

// ---------------------------------------------------------------------------
// create_test_repo (Task 6.6)
// ---------------------------------------------------------------------------

/// Initialize a git repo with an initial commit in the given directory.
///
/// Uses Git CLI subprocess calls (no `git2`).
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
// create_test_repo_with_remote (Story 7.4)
// ---------------------------------------------------------------------------

/// Create a git repo with a local bare "origin" remote.
///
/// Sets up: bare repo at `dir/remote.git`, working repo at `dir/work`.
/// Returns the path to the working repo (`dir/work`) which should be used as
/// `project_root` in `BotConfig`.
///
/// The working repo has an initial commit on `main` and a configured `origin`
/// remote pointing to the bare repo.
pub fn create_test_repo_with_remote(dir: &Path) -> std::path::PathBuf {
    use std::process::Command;

    let bare_path = dir.join("remote.git");
    let work_path = dir.join("work");

    std::fs::create_dir_all(&bare_path).expect("mkdir bare");
    std::fs::create_dir_all(&work_path).expect("mkdir work");

    let run_at = |dir: &Path, args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(
            output.status.success(),
            "git {} (at {:?}) failed: {}",
            args.join(" "),
            dir,
            String::from_utf8_lossy(&output.stderr)
        );
    };

    // Create bare remote
    run_at(&bare_path, &["init", "--bare"]);

    // Create working repo
    run_at(&work_path, &["init"]);
    run_at(&work_path, &["config", "user.email", "test@test.com"]);
    run_at(&work_path, &["config", "user.name", "Test"]);
    run_at(&work_path, &["commit", "--allow-empty", "-m", "initial commit"]);
    run_at(&work_path, &["branch", "-M", "main"]);

    // Add local bare as origin
    run_at(
        &work_path,
        &["remote", "add", "origin", bare_path.to_str().unwrap()],
    );
    // Push main so bare has it
    run_at(&work_path, &["push", "origin", "main"]);

    work_path
}

/// Create a story branch with a commit in the given repo.
///
/// Branch is created from `main` and contains an empty commit.
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

// ---------------------------------------------------------------------------
// PipelineTestBuilder (Story 7.4 Task 2)
// ---------------------------------------------------------------------------

/// Builder for constructing a `StoryPipeline` with mock dependencies.
///
/// Usage:
/// ```ignore
/// let (pipeline, notifier, git) = PipelineTestBuilder::new(work_dir)
///     .with_session(SessionOutcome::Completed { ... })
///     .with_review(ReviewOutcome::Completed { ... })
///     .build();
/// ```
///
/// `build()` returns `(StoryPipeline, MockNotifier, MockGitProvider)` where the
/// returned mocks share interior state (`Arc<Mutex>`) with the pipeline's copies,
/// enabling assertions on captured calls.
pub struct PipelineTestBuilder {
    config: BotConfig,
    session_outcomes: Vec<SessionOutcome>,
    review_outcomes: Vec<ReviewOutcome>,
    mock_git: MockGitProvider,
    mock_notifier: MockNotifier,
}

impl PipelineTestBuilder {
    /// Create a builder with sensible defaults.
    ///
    /// `project_root` must be a git repo with an `origin` remote
    /// (use [`create_test_repo_with_remote`]).
    pub fn new(project_root: &Path) -> Self {
        let dir_str = project_root.display().to_string();
        let config = BotConfig {
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
                    model: "test-dev-model".to_string(),
                    reasoning_effort: None,
                },
                review: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test-review-model".to_string(),
                    reasoning_effort: None,
                },
                supervisor: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test-supervisor-model".to_string(),
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
                planning_artifacts: dir_str.clone(),
                implementation_artifacts: dir_str,
            },
            log_format: "pretty".to_string(),
            log_level: "info".to_string(),
            log_file: "test.log".to_string(),
            code_review_enabled: true,
            mcp_servers: Vec::<McpServerConfig>::new(),
        };
        Self {
            config,
            session_outcomes: vec![],
            review_outcomes: vec![],
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
        self.review_outcomes.push(outcome);
        self
    }

    pub fn with_reviews(mut self, outcomes: Vec<ReviewOutcome>) -> Self {
        self.review_outcomes = outcomes;
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
    /// Returns `(StoryPipeline, MockNotifier, MockGitProvider, MockCodeReviewerHandle)` where the
    /// mock handles share interior state with the pipeline's copies via `Arc<Mutex<...>>`.
    pub fn build(self) -> (StoryPipeline, MockNotifier, MockGitProvider, MockCodeReviewerHandle) {
        let notifier_for_assertions = self.mock_notifier.clone();
        let git_for_assertions = self.mock_git.clone();

        let dev_runner: Box<dyn DevRunner> = if self.session_outcomes.len() <= 1 {
            let outcome = self.session_outcomes.into_iter().next().unwrap_or_else(|| {
                SessionOutcome::Completed {
                    story_key: "test".to_string(),
                    branch: "story/test".to_string(),
                    decisions: vec![],
                    pr_context: None,
                    pr_how_to_test: None,
                    pr_additional_info: None,
                }
            });
            Box::new(MockDevRunner::with_outcome(outcome))
        } else {
            Box::new(MockDevRunner::with_outcomes(self.session_outcomes))
        };

        let (code_reviewer_mock, reviewer_handle) = if self.review_outcomes.is_empty() {
            MockCodeReviewer::never_called()
        } else if self.review_outcomes.len() == 1 {
            MockCodeReviewer::with_outcome(
                self.review_outcomes.into_iter().next().unwrap(),
            )
        } else {
            MockCodeReviewer::with_outcomes(self.review_outcomes)
        };
        let code_reviewer: Box<dyn CodeReviewer> = Box::new(code_reviewer_mock);

        let pipeline = StoryPipeline::new_with_components(
            Arc::new(self.config),
            Box::new(self.mock_git),
            Box::new(self.mock_notifier),
            dev_runner,
            code_reviewer,
        );

        (pipeline, notifier_for_assertions, git_for_assertions, reviewer_handle)
    }
}
