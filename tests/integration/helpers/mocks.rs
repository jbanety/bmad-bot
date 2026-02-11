//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.
//! Follows builder pattern for configurable return values.

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

/// Captured call record for `MockGitProvider`.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    /// A `create_pr` call with the provided params.
    CreatePr(CreatePrParams),
    /// An `add_comment` call with `(pr_id, body)`.
    AddComment(String, String),
    /// A `get_pr_url` call with the PR ID.
    GetPrUrl(String),
}

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Configurable return values via builder methods. Tracks all calls for assertions.
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
                "https://github.com/test/test/pull/1".into(),
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

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<GitProviderCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl Default for MockGitProvider {
    fn default() -> Self {
        Self::new()
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

/// Helper to clone a `GitProviderError` (thiserror types don't implement Clone).
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

/// Captured notification call for `MockNotifier`.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// A `notify_story` call.
    Story(StoryNotification),
    /// A `notify_run_summary` call.
    Summary(RunSummary),
}

/// Mock implementation of [`Notifier`] for integration tests.
///
/// Captures all notification calls for later assertion.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
    story_result: Arc<Mutex<Result<(), NotifierError>>>,
    summary_result: Arc<Mutex<Result<(), NotifierError>>>,
}

impl MockNotifier {
    /// Create a new `MockNotifier` that succeeds on all calls.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            story_result: Arc::new(Mutex::new(Ok(()))),
            summary_result: Arc::new(Mutex::new(Ok(()))),
        }
    }

    /// Configure the return value for `notify_story`.
    pub fn with_story_result(self, result: Result<(), NotifierError>) -> Self {
        *self.story_result.lock().unwrap() = result;
        self
    }

    /// Configure the return value for `notify_run_summary`.
    pub fn with_summary_result(self, result: Result<(), NotifierError>) -> Self {
        *self.summary_result.lock().unwrap() = result;
        self
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Get only `notify_story` calls.
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

    /// Get only `notify_run_summary` calls.
    pub fn summary_calls(&self) -> Vec<RunSummary> {
        self.calls
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

impl Default for MockNotifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to clone a `NotifierError`.
fn clone_notifier_error(e: &NotifierError) -> NotifierError {
    match e {
        NotifierError::HttpRequest { reason } => NotifierError::HttpRequest {
            reason: reason.clone(),
        },
        NotifierError::ApiError { status, body } => NotifierError::ApiError {
            status: *status,
            body: body.clone(),
        },
        NotifierError::ResponseParse { reason } => NotifierError::ResponseParse {
            reason: reason.clone(),
        },
        NotifierError::Disabled => NotifierError::Disabled,
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
            Err(e) => Err(clone_notifier_error(e)),
        }
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::Summary(summary.clone()));
        let guard = self.summary_result.lock().unwrap();
        match &*guard {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_notifier_error(e)),
        }
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Captured session runner call.
#[derive(Debug, Clone)]
pub struct SessionRunnerCall {
    /// The story key that was passed to `run`.
    pub story_key: String,
}

/// Mock session runner for integration tests.
///
/// Returns a configurable `SessionOutcome` and tracks calls.
/// This is a standalone struct — does NOT implement a shared trait with the real `SessionRunner`.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<SessionOutcome>>,
    calls: Arc<Mutex<Vec<SessionRunnerCall>>>,
}

impl MockSessionRunner {
    /// Create a new `MockSessionRunner` with a default `Completed` outcome.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(SessionOutcome::Completed {
                story_key: "test-story".into(),
                branch: "story/test-story".into(),
                decisions: Vec::new(),
            })),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome(self, outcome: SessionOutcome) -> Self {
        *self.outcome.lock().unwrap() = outcome;
        self
    }

    /// Simulate running a session for a story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunnerCall {
            story_key: story.story_key.clone(),
        });
        let guard = self.outcome.lock().unwrap();
        clone_session_outcome(&guard)
    }

    /// Check and recover WAL — always returns `None` for mock.
    pub async fn check_and_recover_wal(
        &self,
    ) -> Option<bmad_bot::session::runner::RecoveryInfo> {
        None
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<SessionRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl Default for MockSessionRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Clone a `SessionOutcome` (not derivable due to inner types).
fn clone_session_outcome(o: &SessionOutcome) -> SessionOutcome {
    match o {
        SessionOutcome::Completed {
            story_key,
            branch,
            decisions,
        } => SessionOutcome::Completed {
            story_key: story_key.clone(),
            branch: branch.clone(),
            decisions: decisions.clone(),
        },
        SessionOutcome::Escalated { report, decisions } => SessionOutcome::Escalated {
            report: report.clone(),
            decisions: decisions.clone(),
        },
        SessionOutcome::Failed {
            story_key,
            error,
            decisions,
        } => SessionOutcome::Failed {
            story_key: story_key.clone(),
            error: error.clone(),
            decisions: decisions.clone(),
        },
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Captured review runner call.
#[derive(Debug, Clone)]
pub struct ReviewRunnerCall {
    /// The story key that was passed to `run`.
    pub story_key: String,
}

/// Mock review runner for integration tests.
///
/// Returns a configurable `ReviewOutcome` and tracks calls.
/// Standalone struct — does NOT implement a shared trait with the real `ReviewRunner`.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<ReviewOutcome>>,
    calls: Arc<Mutex<Vec<ReviewRunnerCall>>>,
}

impl MockReviewRunner {
    /// Create a new `MockReviewRunner` with a default `Completed` outcome.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(ReviewOutcome::Completed {
                story_key: "test-story".into(),
                branch: "story/test-story".into(),
                report: "All good".into(),
            })),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome(self, outcome: ReviewOutcome) -> Self {
        *self.outcome.lock().unwrap() = outcome;
        self
    }

    /// Simulate running a review for a story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunnerCall {
            story_key: story.story_key.clone(),
        });
        let guard = self.outcome.lock().unwrap();
        clone_review_outcome(&guard)
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl Default for MockReviewRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Clone a `ReviewOutcome`.
fn clone_review_outcome(o: &ReviewOutcome) -> ReviewOutcome {
    match o {
        ReviewOutcome::Completed {
            story_key,
            branch,
            report,
        } => ReviewOutcome::Completed {
            story_key: story_key.clone(),
            branch: branch.clone(),
            report: report.clone(),
        },
        ReviewOutcome::Failed { story_key, error } => ReviewOutcome::Failed {
            story_key: story_key.clone(),
            error: error.clone(),
        },
        ReviewOutcome::Skipped { reason } => ReviewOutcome::Skipped {
            reason: reason.clone(),
        },
    }
}
