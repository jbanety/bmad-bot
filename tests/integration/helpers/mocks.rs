//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.
//! Builder pattern for configurable return values.

use async_trait::async_trait;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::runner::RecoveryInfo;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Recorded call to a `MockGitProvider` method.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    /// `create_pr` was invoked with these params.
    CreatePr(CreatePrParams),
    /// `add_comment` was invoked with `(pr_id, body)`.
    AddComment(String, String),
    /// `get_pr_url` was invoked with `pr_id`.
    GetPrUrl(String),
}

/// Mock `GitProvider` — configurable return values + call tracking.
#[derive(Clone)]
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new `MockGitProvider` with sensible defaults (all succeed).
    pub fn new() -> Self {
        Self {
            create_pr_result: Arc::new(Mutex::new(Ok(PrInfo {
                id: "1".into(),
                url: "https://github.com/test/test/pull/1".into(),
                number: 1,
            }))),
            add_comment_result: Arc::new(Mutex::new(Ok(()))),
            get_pr_url_result: Arc::new(Mutex::new(Ok(
                "https://github.com/test/test/pull/1".into(),
            ))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the result returned by `create_pr`.
    pub fn with_create_pr(self, result: Result<PrInfo, GitProviderError>) -> Self {
        *self.create_pr_result.lock().unwrap() = result;
        self
    }

    /// Configure the result returned by `add_comment`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        *self.add_comment_result.lock().unwrap() = result;
        self
    }

    /// Configure the result returned by `get_pr_url`.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        *self.get_pr_url_result.lock().unwrap() = result;
        self
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<GitProviderCall> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl GitProvider for MockGitProvider {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::CreatePr(params));
        let guard = self.create_pr_result.lock().unwrap();
        match &*guard {
            Ok(info) => Ok(info.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::AddComment(pr_id.into(), body.into()));
        let guard = self.add_comment_result.lock().unwrap();
        match &*guard {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::GetPrUrl(pr_id.into()));
        let guard = self.get_pr_url_result.lock().unwrap();
        match &*guard {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Clone a `GitProviderError` (thiserror enums don't derive `Clone`).
fn clone_git_provider_error(e: &GitProviderError) -> GitProviderError {
    // Re-create based on Display — safe because we control the mock values
    GitProviderError::ApiError {
        status: 0,
        message: format!("{e}"),
    }
}

// ---------------------------------------------------------------------------
// MockNotifier
// ---------------------------------------------------------------------------

/// Recorded call to a `MockNotifier` method.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// `notify_story` was invoked.
    Story(StoryNotification),
    /// `notify_run_summary` was invoked.
    Summary(RunSummary),
}

/// Mock `Notifier` — captures all calls for assertions.
#[derive(Clone)]
pub struct MockNotifier {
    captured: Arc<Mutex<Vec<NotifierCall>>>,
}

impl MockNotifier {
    /// Create a new `MockNotifier`.
    pub fn new() -> Self {
        Self {
            captured: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.captured.lock().unwrap().clone()
    }

    /// Return only `Story` calls.
    pub fn story_calls(&self) -> Vec<StoryNotification> {
        self.captured
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                NotifierCall::Story(n) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    /// Return only `Summary` calls.
    pub fn summary_calls(&self) -> Vec<RunSummary> {
        self.captured
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                NotifierCall::Summary(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
}

#[async_trait]
impl Notifier for MockNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        self.captured
            .lock()
            .unwrap()
            .push(NotifierCall::Story(notification.clone()));
        Ok(())
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.captured
            .lock()
            .unwrap()
            .push(NotifierCall::Summary(summary.clone()));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Recorded call to `MockSessionRunner::run`.
#[derive(Debug, Clone)]
pub struct SessionRunnerCall {
    /// The story key passed to `run`.
    pub story_key: String,
}

/// Mock session runner — returns configurable `SessionOutcome`.
pub struct MockSessionRunner {
    outcome_factory: Box<dyn Fn(&StoryInfo) -> SessionOutcome + Send + Sync>,
    calls: Arc<Mutex<Vec<SessionRunnerCall>>>,
}

impl MockSessionRunner {
    /// Create with a default `Completed` outcome.
    pub fn new() -> Self {
        Self {
            outcome_factory: Box::new(|story| SessionOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                decisions: Vec::new(),
                pr_context: None,
                pr_how_to_test: None,
                pr_additional_info: None,
            }),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome factory.
    pub fn with_outcome<F>(mut self, f: F) -> Self
    where
        F: Fn(&StoryInfo) -> SessionOutcome + Send + Sync + 'static,
    {
        self.outcome_factory = Box::new(f);
        self
    }

    /// Run the mock session.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunnerCall {
            story_key: story.story_key.clone(),
        });
        (self.outcome_factory)(story)
    }

    /// Check and recover WAL — always returns None for mock.
    pub async fn check_and_recover_wal(&self) -> Option<RecoveryInfo> {
        None
    }

    /// Return recorded calls.
    pub fn calls(&self) -> Vec<SessionRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Recorded call to `MockReviewRunner::run`.
#[derive(Debug, Clone)]
pub struct ReviewRunnerCall {
    /// The story key passed to `run`.
    pub story_key: String,
}

/// Mock review runner — returns configurable `ReviewOutcome`.
pub struct MockReviewRunner {
    outcome_factory: Box<dyn Fn(&StoryInfo) -> ReviewOutcome + Send + Sync>,
    calls: Arc<Mutex<Vec<ReviewRunnerCall>>>,
}

impl MockReviewRunner {
    /// Create with a default `Completed` outcome.
    pub fn new() -> Self {
        Self {
            outcome_factory: Box::new(|story| ReviewOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                report: "Mock review report".into(),
            }),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome factory.
    pub fn with_outcome<F>(mut self, f: F) -> Self
    where
        F: Fn(&StoryInfo) -> ReviewOutcome + Send + Sync + 'static,
    {
        self.outcome_factory = Box::new(f);
        self
    }

    /// Run the mock review.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunnerCall {
            story_key: story.story_key.clone(),
        });
        (self.outcome_factory)(story)
    }

    /// Return recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}
