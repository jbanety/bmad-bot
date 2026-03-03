//! Mock implementations for integration testing.
//!
//! Provides configurable mock structs for `GitProvider`, `Notifier`,
//! `SessionRunner`, and `ReviewRunner`. All mocks are `Send + Sync`.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Recorded call to `MockGitProvider`.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    /// A `create_pr` call with captured params.
    CreatePr(CreatePrParams),
    /// An `add_comment` call with (pr_id, body).
    AddComment(String, String),
    /// A `get_pr_url` call with pr_id.
    GetPrUrl(String),
}

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Uses builder pattern for configurable return values and tracks all calls
/// for assertion.
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new `MockGitProvider` with default success responses.
    pub fn new() -> Self {
        Self {
            create_pr_result: Arc::new(Mutex::new(Ok(PrInfo {
                id: "1".into(),
                url: "https://github.com/test/test/pull/1".into(),
                number: 1,
            }))),
            add_comment_result: Arc::new(Mutex::new(Ok(()))),
            get_pr_url_result: Arc::new(Mutex::new(Ok(
                "https://github.com/test/test/pull/1".into()
            ))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the return value for `create_pr`.
    pub fn with_create_pr(self, result: Result<PrInfo, GitProviderError>) -> Self {
        *self.create_pr_result.lock().unwrap() = result;
        self
    }

    /// Configure the return value for `add_comment`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        *self.add_comment_result.lock().unwrap() = result;
        self
    }

    /// Configure the return value for `get_pr_url`.
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

/// Clone a `GitProviderError` (it doesn't implement Clone).
fn clone_git_provider_error(e: &GitProviderError) -> GitProviderError {
    match e {
        GitProviderError::ApiError { status, message } => GitProviderError::ApiError {
            status: *status,
            message: message.clone(),
        },
        GitProviderError::AuthenticationFailed { reason } => {
            GitProviderError::AuthenticationFailed {
                reason: reason.clone(),
            }
        }
        GitProviderError::BranchNotFound { branch } => GitProviderError::BranchNotFound {
            branch: branch.clone(),
        },
        GitProviderError::DuplicatePr { branch, details } => GitProviderError::DuplicatePr {
            branch: branch.clone(),
            details: details.clone(),
        },
        GitProviderError::ValidationFailed { message, details } => {
            GitProviderError::ValidationFailed {
                message: message.clone(),
                details: details.clone(),
            }
        }
        GitProviderError::RateLimited { retry_after_secs } => GitProviderError::RateLimited {
            retry_after_secs: *retry_after_secs,
        },
        GitProviderError::NetworkError { reason } => GitProviderError::NetworkError {
            reason: reason.clone(),
        },
        GitProviderError::InvalidResponse { reason } => GitProviderError::InvalidResponse {
            reason: reason.clone(),
        },
        GitProviderError::InvalidPrId { pr_id } => GitProviderError::InvalidPrId {
            pr_id: pr_id.clone(),
        },
        GitProviderError::ProviderNotConfigured { provider } => {
            GitProviderError::ProviderNotConfigured {
                provider: provider.clone(),
            }
        }
        GitProviderError::BuildError { reason } => GitProviderError::BuildError {
            reason: reason.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// MockNotifier
// ---------------------------------------------------------------------------

/// Recorded notification call.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// A `notify_story` call with captured notification.
    Story(StoryNotification),
    /// A `notify_run_summary` call with captured summary.
    RunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] for integration tests.
///
/// Captures all notification calls into a `Vec` for assertion.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
    story_result: Arc<Mutex<Result<(), NotifierError>>>,
    summary_result: Arc<Mutex<Result<(), NotifierError>>>,
}

impl MockNotifier {
    /// Create a new `MockNotifier` with default success responses.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            story_result: Arc::new(Mutex::new(Ok(()))),
            summary_result: Arc::new(Mutex::new(Ok(()))),
        }
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Return only the `notify_story` calls.
    pub fn story_calls(&self) -> Vec<StoryNotification> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                NotifierCall::Story(n) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    /// Return only the `notify_run_summary` calls.
    pub fn summary_calls(&self) -> Vec<RunSummary> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                NotifierCall::RunSummary(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
}

#[async_trait]
impl Notifier for MockNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::Story(notification.clone()));
        let guard = self.story_result.lock().unwrap();
        match &*guard {
            Ok(()) => Ok(()),
            Err(_) => Err(NotifierError::Disabled),
        }
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::RunSummary(summary.clone()));
        let guard = self.summary_result.lock().unwrap();
        match &*guard {
            Ok(()) => Ok(()),
            Err(_) => Err(NotifierError::Disabled),
        }
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Recorded call to `MockSessionRunner`.
#[derive(Debug, Clone)]
pub struct SessionRunnerCall {
    /// The story key from the `StoryInfo` that was passed.
    pub story_key: String,
}

/// Mock session runner for integration tests.
///
/// Returns a configurable `SessionOutcome` and tracks calls.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<Option<SessionOutcome>>>,
    calls: Arc<Mutex<Vec<SessionRunnerCall>>>,
}

impl MockSessionRunner {
    /// Create a new `MockSessionRunner` with a default `Completed` outcome.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(Some(SessionOutcome::Completed {
                story_key: "test".into(),
                branch: "story/test".into(),
                decisions: vec![],
                pr_context: None,
                pr_how_to_test: None,
                pr_additional_info: None,
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome for the next `run` call.
    pub fn with_outcome(self, outcome: SessionOutcome) -> Self {
        *self.outcome.lock().unwrap() = Some(outcome);
        self
    }

    /// Simulate running a session for the given story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunnerCall {
            story_key: story.story_key.clone(),
        });
        self.outcome
            .lock()
            .unwrap()
            .take()
            .unwrap_or(SessionOutcome::Failed {
                story_key: story.story_key.clone(),
                error: "MockSessionRunner: no outcome configured".into(),
                decisions: vec![],
            })
    }

    /// Check and recover WAL — always returns `None` for mock.
    pub async fn check_and_recover_wal(&self) -> Option<bmad_bot::session::runner::RecoveryInfo> {
        None
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<SessionRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Recorded call to `MockReviewRunner`.
#[derive(Debug, Clone)]
pub struct ReviewRunnerCall {
    /// The story key from the `StoryInfo` that was passed.
    pub story_key: String,
}

/// Mock review runner for integration tests.
///
/// Returns a configurable `ReviewOutcome` and tracks calls.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<Option<ReviewOutcome>>>,
    calls: Arc<Mutex<Vec<ReviewRunnerCall>>>,
}

impl MockReviewRunner {
    /// Create a new `MockReviewRunner` with a default `Skipped` outcome.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(Some(ReviewOutcome::Skipped {
                reason: "mock: review skipped by default".into(),
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome for the next `run` call.
    pub fn with_outcome(self, outcome: ReviewOutcome) -> Self {
        *self.outcome.lock().unwrap() = Some(outcome);
        self
    }

    /// Simulate running a review for the given story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunnerCall {
            story_key: story.story_key.clone(),
        });
        self.outcome
            .lock()
            .unwrap()
            .take()
            .unwrap_or(ReviewOutcome::Failed {
                story_key: story.story_key.clone(),
                error: "MockReviewRunner: no outcome configured".into(),
            })
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}
