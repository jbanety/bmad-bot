//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.
//! Builder pattern for configuration: `MockGitProvider::new().with_create_pr(Ok(...))`.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::supervisor::decisions::DecisionRecord;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Recorded call to MockGitProvider.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    CreatePr(CreatePrParams),
    AddComment {
        pr_id: String,
        body: String,
    },
    GetPrUrl {
        pr_id: String,
    },
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
    /// Create a new mock with default OK results.
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

    /// Configure the result for `create_pr`.
    pub fn with_create_pr(self, result: Result<PrInfo, GitProviderError>) -> Self {
        *self.create_pr_result.lock().unwrap() = result;
        self
    }

    /// Configure the result for `add_comment`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        *self.add_comment_result.lock().unwrap() = result;
        self
    }

    /// Configure the result for `get_pr_url`.
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
            .push(GitProviderCall::AddComment {
                pr_id: pr_id.to_string(),
                body: body.to_string(),
            });
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
            .push(GitProviderCall::GetPrUrl {
                pr_id: pr_id.to_string(),
            });
        let guard = self.get_pr_url_result.lock().unwrap();
        match &*guard {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Clone a `GitProviderError` for returning from mock (errors aren't Clone).
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

/// Recorded call to MockNotifier.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    Story(StoryNotification),
    RunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] for integration tests.
///
/// Captures all notification calls for assertion.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
    story_result: Arc<Mutex<Result<(), NotifierError>>>,
    summary_result: Arc<Mutex<Result<(), NotifierError>>>,
}

impl MockNotifier {
    /// Create a new mock that succeeds by default.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            story_result: Arc::new(Mutex::new(Ok(()))),
            summary_result: Arc::new(Mutex::new(Ok(()))),
        }
    }

    /// Configure `notify_story` to fail.
    pub fn with_story_error(self, err: NotifierError) -> Self {
        *self.story_result.lock().unwrap() = Err(err);
        self
    }

    /// Configure `notify_run_summary` to fail.
    pub fn with_summary_error(self, err: NotifierError) -> Self {
        *self.summary_result.lock().unwrap() = Err(err);
        self
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Get only story notification calls.
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

    /// Get only run summary calls.
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

impl Default for MockNotifier {
    fn default() -> Self {
        Self::new()
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
            .push(NotifierCall::RunSummary(summary.clone()));
        let guard = self.summary_result.lock().unwrap();
        match &*guard {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_notifier_error(e)),
        }
    }
}

/// Clone a `NotifierError` for returning from mock.
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

/// Recorded call to MockSessionRunner.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    pub story_key: String,
}

/// Mock for `SessionRunner` — standalone struct (not trait-based).
///
/// Returns configurable `SessionOutcome` when `run()` is called.
pub struct MockSessionRunner {
    outcome_fn: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> SessionOutcome + Send>>>,
    run_calls: Arc<Mutex<Vec<SessionRunCall>>>,
    wal_recovery: Arc<Mutex<Option<RecoveryInfo>>>,
    wal_calls: Arc<Mutex<Vec<()>>>,
}

/// Placeholder for WAL recovery info returned by mock.
#[derive(Debug, Clone)]
pub struct RecoveryInfo {
    pub story_key: String,
    pub branch: String,
}

impl MockSessionRunner {
    /// Create with a default "Completed" outcome.
    pub fn new() -> Self {
        Self {
            outcome_fn: Arc::new(Mutex::new(Box::new(|story: &StoryInfo| {
                SessionOutcome::Completed {
                    story_key: story.story_key.clone(),
                    branch: story.branch_name.clone(),
                    decisions: Vec::new(),
                    pr_context: None,
                    pr_how_to_test: None,
                    pr_additional_info: None,
                }
            }))),
            run_calls: Arc::new(Mutex::new(Vec::new())),
            wal_recovery: Arc::new(Mutex::new(None)),
            wal_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome for `run()`.
    pub fn with_outcome<F>(self, f: F) -> Self
    where
        F: Fn(&StoryInfo) -> SessionOutcome + Send + 'static,
    {
        *self.outcome_fn.lock().unwrap() = Box::new(f);
        self
    }

    /// Configure WAL recovery to return a value.
    pub fn with_wal_recovery(self, info: RecoveryInfo) -> Self {
        *self.wal_recovery.lock().unwrap() = Some(info);
        self
    }

    /// Simulate `run()`.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.run_calls.lock().unwrap().push(SessionRunCall {
            story_key: story.story_key.clone(),
        });
        let f = self.outcome_fn.lock().unwrap();
        f(story)
    }

    /// Simulate `check_and_recover_wal()`.
    pub async fn check_and_recover_wal(&self) -> Option<RecoveryInfo> {
        self.wal_calls.lock().unwrap().push(());
        self.wal_recovery.lock().unwrap().clone()
    }

    /// Get recorded run calls.
    pub fn run_calls(&self) -> Vec<SessionRunCall> {
        self.run_calls.lock().unwrap().clone()
    }

    /// Get count of WAL check calls.
    pub fn wal_check_count(&self) -> usize {
        self.wal_calls.lock().unwrap().len()
    }
}

impl Default for MockSessionRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Recorded call to MockReviewRunner.
#[derive(Debug, Clone)]
pub struct ReviewRunCall {
    pub story_key: String,
}

/// Mock for `ReviewRunner` — standalone struct (not trait-based).
///
/// Returns configurable `ReviewOutcome` when `run()` is called.
pub struct MockReviewRunner {
    outcome_fn: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> ReviewOutcome + Send>>>,
    run_calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create with a default "Completed" outcome.
    pub fn new() -> Self {
        Self {
            outcome_fn: Arc::new(Mutex::new(Box::new(|story: &StoryInfo| {
                ReviewOutcome::Completed {
                    story_key: story.story_key.clone(),
                    branch: story.branch_name.clone(),
                    report: "Mock review report".into(),
                }
            }))),
            run_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome for `run()`.
    pub fn with_outcome<F>(self, f: F) -> Self
    where
        F: Fn(&StoryInfo) -> ReviewOutcome + Send + 'static,
    {
        *self.outcome_fn.lock().unwrap() = Box::new(f);
        self
    }

    /// Simulate `run()`.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.run_calls.lock().unwrap().push(ReviewRunCall {
            story_key: story.story_key.clone(),
        });
        let f = self.outcome_fn.lock().unwrap();
        f(story)
    }

    /// Get recorded run calls.
    pub fn run_calls(&self) -> Vec<ReviewRunCall> {
        self.run_calls.lock().unwrap().clone()
    }
}

impl Default for MockReviewRunner {
    fn default() -> Self {
        Self::new()
    }
}
