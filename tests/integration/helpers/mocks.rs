//! Mock implementations for integration tests.
//!
//! Provides configurable mocks for `GitProvider`, `Notifier`, `SessionRunner`,
//! and `ReviewRunner` — all `Send + Sync` safe.

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

/// Captured call to `MockGitProvider`.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    /// A `create_pr` call with its parameters.
    CreatePr(CreatePrParams),
    /// An `add_comment` call with `(pr_id, body)`.
    AddComment(String, String),
    /// A `get_pr_url` call with `pr_id`.
    GetPrUrl(String),
}

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Uses builder pattern for configuring return values. Tracks all calls
/// for assertion in tests.
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

    /// Get a snapshot of all recorded calls.
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

/// Clone a `GitProviderError` (thiserror variants aren't Clone).
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

/// Captured call to `MockNotifier`.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// A `notify_story` call with its notification payload.
    Story(StoryNotification),
    /// A `notify_run_summary` call with its summary payload.
    RunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] for integration tests.
///
/// Captures all calls into a `Vec` for later assertion — unlike `NoopNotifier`
/// which silently discards everything.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
}

impl MockNotifier {
    /// Create a new `MockNotifier` with empty call history.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
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
        Ok(())
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::RunSummary(summary.clone()));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Captured call to `MockSessionRunner::run`.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    /// The story key that was passed.
    pub story_key: String,
}

/// Mock for `SessionRunner` — returns a configurable `SessionOutcome`.
///
/// This is a standalone struct (not implementing a shared trait with the real
/// `SessionRunner`). Story 7.4 will address trait abstraction for pipeline injection.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<Option<SessionOutcome>>>,
    calls: Arc<Mutex<Vec<SessionRunCall>>>,
}

impl MockSessionRunner {
    /// Create with a default `Completed` outcome.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(Some(SessionOutcome::Completed {
                story_key: "test".into(),
                branch: "story/test".into(),
                decisions: vec![],
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome(self, outcome: SessionOutcome) -> Self {
        *self.outcome.lock().unwrap() = Some(outcome);
        self
    }

    /// Simulate running a session — returns the configured outcome.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunCall {
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

    /// Check whether WAL recovery is needed — always returns `None` for the mock.
    pub async fn check_and_recover_wal(&self) -> Option<()> {
        None
    }

    /// Get all recorded `run` calls.
    pub fn calls(&self) -> Vec<SessionRunCall> {
        self.calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Captured call to `MockReviewRunner::run`.
#[derive(Debug, Clone)]
pub struct ReviewRunCall {
    /// The story key that was passed.
    pub story_key: String,
}

/// Mock for `ReviewRunner` — returns a configurable `ReviewOutcome`.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<Option<ReviewOutcome>>>,
    calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create with a default `Completed` outcome.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(Some(ReviewOutcome::Completed {
                story_key: "test".into(),
                branch: "story/test".into(),
                report: "All good".into(),
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome(self, outcome: ReviewOutcome) -> Self {
        *self.outcome.lock().unwrap() = Some(outcome);
        self
    }

    /// Simulate running a review — returns the configured outcome.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunCall {
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

    /// Get all recorded `run` calls.
    pub fn calls(&self) -> Vec<ReviewRunCall> {
        self.calls.lock().unwrap().clone()
    }
}
