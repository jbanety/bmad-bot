//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::session::SessionOutcome;
use bmad_bot::review::ReviewOutcome;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Recorded call to a `MockGitProvider` method.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    CreatePr(CreatePrParams),
    AddComment { pr_id: String, body: String },
    GetPrUrl(String),
}

/// Mock implementation of [`GitProvider`] with configurable return values and call tracking.
///
/// Uses a builder pattern:
/// ```ignore
/// MockGitProvider::new()
///     .with_create_pr(Ok(PrInfo { id: "1".into(), url: "https://...".into(), number: 1 }))
///     .with_add_comment(Ok(()))
/// ```
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
                id: "mock-pr-1".to_string(),
                url: "https://github.com/test/repo/pull/1".to_string(),
                number: 1,
            }))),
            add_comment_result: Arc::new(Mutex::new(Ok(()))),
            get_pr_url_result: Arc::new(Mutex::new(Ok(
                "https://github.com/test/repo/pull/1".to_string(),
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
        let result = self.create_pr_result.lock().unwrap();
        match &*result {
            Ok(info) => Ok(info.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::AddComment {
                pr_id: pr_id.to_string(),
                body: body.to_string(),
            });
        let result = self.add_comment_result.lock().unwrap();
        match &*result {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::GetPrUrl(pr_id.to_string()));
        let result = self.get_pr_url_result.lock().unwrap();
        match &*result {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Clone a `GitProviderError` for returning from mock methods.
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

/// Recorded call to a `MockNotifier` method.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    NotifyStory(StoryNotification),
    NotifyRunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] that captures all calls for assertion.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
    notify_story_result: Arc<Mutex<Result<(), NotifierError>>>,
    notify_run_summary_result: Arc<Mutex<Result<(), NotifierError>>>,
}

impl MockNotifier {
    /// Create a new `MockNotifier` with default success responses.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            notify_story_result: Arc::new(Mutex::new(Ok(()))),
            notify_run_summary_result: Arc::new(Mutex::new(Ok(()))),
        }
    }

    /// Configure the return value for `notify_story`.
    pub fn with_notify_story(self, result: Result<(), NotifierError>) -> Self {
        *self.notify_story_result.lock().unwrap() = result;
        self
    }

    /// Configure the return value for `notify_run_summary`.
    pub fn with_notify_run_summary(self, result: Result<(), NotifierError>) -> Self {
        *self.notify_run_summary_result.lock().unwrap() = result;
        self
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Return only the `NotifyStory` calls.
    pub fn story_calls(&self) -> Vec<StoryNotification> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                NotifierCall::NotifyStory(n) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    /// Return only the `NotifyRunSummary` calls.
    pub fn summary_calls(&self) -> Vec<RunSummary> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                NotifierCall::NotifyRunSummary(s) => Some(s.clone()),
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
            .push(NotifierCall::NotifyStory(notification.clone()));
        let result = self.notify_story_result.lock().unwrap();
        match &*result {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_notifier_error(e)),
        }
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::NotifyRunSummary(summary.clone()));
        let result = self.notify_run_summary_result.lock().unwrap();
        match &*result {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_notifier_error(e)),
        }
    }
}

/// Clone a `NotifierError` for returning from mock methods.
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

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Recorded call to `MockSessionRunner::run`.
#[derive(Debug, Clone)]
pub struct SessionRunnerCall {
    pub story_key: String,
}

/// Mock session runner that returns a configurable `SessionOutcome`.
///
/// This is a standalone mock — it does NOT implement a shared trait with the real
/// `SessionRunner` (the codebase doesn't define one). Story 7.4 will address
/// injection into `StoryPipeline`.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<Option<MockSessionOutcome>>>,
    calls: Arc<Mutex<Vec<SessionRunnerCall>>>,
}

/// Simplified outcome for mock configuration (avoids non-Clone fields in real SessionOutcome).
#[derive(Debug, Clone)]
pub enum MockSessionOutcome {
    Completed { story_key: String, branch: String },
    Escalated { story_key: String, reason: String },
    Failed { story_key: String, error: String },
}

impl MockSessionRunner {
    /// Create a new `MockSessionRunner` with a default `Completed` outcome.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(Some(MockSessionOutcome::Completed {
                story_key: "test-story".to_string(),
                branch: "story/test-story".to_string(),
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome to return.
    pub fn with_outcome(self, outcome: MockSessionOutcome) -> Self {
        *self.outcome.lock().unwrap() = Some(outcome);
        self
    }

    /// Simulate running a session for the given story.
    pub async fn run(&self, story: &StoryInfo) -> MockSessionOutcome {
        self.calls.lock().unwrap().push(SessionRunnerCall {
            story_key: story.story_key.clone(),
        });
        self.outcome
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(MockSessionOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
            })
    }

    /// Simulate WAL recovery check — always returns None.
    pub async fn check_and_recover_wal(&self) -> Option<()> {
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

/// Recorded call to `MockReviewRunner::run`.
#[derive(Debug, Clone)]
pub struct ReviewRunnerCall {
    pub story_key: String,
}

/// Simplified outcome for mock review configuration.
#[derive(Debug, Clone)]
pub enum MockReviewOutcome {
    Completed { story_key: String, branch: String, report: String },
    Failed { story_key: String, error: String },
    Skipped { story_key: String, reason: String },
}

/// Mock review runner that returns a configurable `ReviewOutcome`.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<Option<MockReviewOutcome>>>,
    calls: Arc<Mutex<Vec<ReviewRunnerCall>>>,
}

impl MockReviewRunner {
    /// Create a new `MockReviewRunner` with a default `Completed` outcome.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(Some(MockReviewOutcome::Completed {
                story_key: "test-story".to_string(),
                branch: "story/test-story".to_string(),
                report: "LGTM".to_string(),
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome to return.
    pub fn with_outcome(self, outcome: MockReviewOutcome) -> Self {
        *self.outcome.lock().unwrap() = Some(outcome);
        self
    }

    /// Simulate running a review for the given story.
    pub async fn run(&self, story: &StoryInfo) -> MockReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunnerCall {
            story_key: story.story_key.clone(),
        });
        self.outcome
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(MockReviewOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                report: "LGTM".to_string(),
            })
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}
