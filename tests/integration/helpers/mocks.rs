//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.

use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::pipeline::{CodeReviewer, DevRunner};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::runner::RecoveryInfo;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Captured call to `MockGitProvider` for assertion.
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

/// Mock implementation of [`GitProvider`] that returns configurable values
/// and tracks all calls for assertion.
#[derive(Clone)]
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new mock with default Ok results.
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

    /// Return captured `create_pr` parameters.
    pub fn captured_create_pr_params(&self) -> Vec<CreatePrParams> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                GitProviderCall::CreatePr(params) => Some(params.clone()),
                _ => None,
            })
            .collect()
    }

    /// Return captured `add_comment` calls as `(pr_id, body)` pairs.
    pub fn captured_add_comment_calls(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                GitProviderCall::AddComment { pr_id, body } => {
                    Some((pr_id.clone(), body.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Return the number of `create_pr` calls.
    pub fn create_pr_call_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, GitProviderCall::CreatePr(_)))
            .count()
    }

    /// Return the number of `add_comment` calls.
    pub fn add_comment_call_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, GitProviderCall::AddComment { .. }))
            .count()
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
            .push(GitProviderCall::GetPrUrl {
                pr_id: pr_id.to_string(),
            });
        let result = self.get_pr_url_result.lock().unwrap();
        match &*result {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Helper to clone a `GitProviderError` (not `Clone` by default).
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

/// Captured notification call.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    Story(StoryNotification),
    RunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] that captures all calls for assertion.
#[derive(Clone)]
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
    story_result: Arc<Mutex<Result<(), NotifierError>>>,
    summary_result: Arc<Mutex<Result<(), NotifierError>>>,
}

impl MockNotifier {
    /// Create a new mock that succeeds on all calls.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            story_result: Arc::new(Mutex::new(Ok(()))),
            summary_result: Arc::new(Mutex::new(Ok(()))),
        }
    }

    /// Configure `notify_story` to return an error.
    pub fn with_story_error(self, err: NotifierError) -> Self {
        *self.story_result.lock().unwrap() = Err(err);
        self
    }

    /// Configure `notify_run_summary` to return an error.
    pub fn with_summary_error(self, err: NotifierError) -> Self {
        *self.summary_result.lock().unwrap() = Err(err);
        self
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Return only story notification calls.
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

    /// Return only run summary calls.
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

    /// Return the number of `notify_story` calls.
    pub fn story_notification_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, NotifierCall::Story(_)))
            .count()
    }

    /// Return the number of `notify_run_summary` calls.
    pub fn run_summary_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, NotifierCall::RunSummary(_)))
            .count()
    }
}

#[async_trait]
impl Notifier for MockNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::Story(notification.clone()));
        let result = self.story_result.lock().unwrap();
        match &*result {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_notifier_error(e)),
        }
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::RunSummary(summary.clone()));
        let result = self.summary_result.lock().unwrap();
        match &*result {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_notifier_error(e)),
        }
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

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Captured session runner call.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    pub story_key: String,
}

/// Mock session runner that returns a configurable `SessionOutcome`.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<Option<SessionOutcome>>>,
    calls: Arc<Mutex<Vec<SessionRunCall>>>,
}

impl MockSessionRunner {
    /// Create a new mock returning `Completed` by default.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome to return.
    pub fn with_outcome(self, outcome: SessionOutcome) -> Self {
        *self.outcome.lock().unwrap() = Some(outcome);
        self
    }

    /// Run the mock session.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunCall {
            story_key: story.story_key.clone(),
        });
        let stored = self.outcome.lock().unwrap();
        if let Some(outcome) = stored.as_ref() {
            return clone_session_outcome(outcome);
        }
        SessionOutcome::Completed {
            story_key: story.story_key.clone(),
            branch: story.branch_name.clone(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        }
    }

    /// Check and recover WAL — always returns `None` for the mock.
    pub async fn check_and_recover_wal(&self) -> Option<RecoveryInfo> {
        None
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<SessionRunCall> {
        self.calls.lock().unwrap().clone()
    }
}

fn clone_session_outcome(outcome: &SessionOutcome) -> SessionOutcome {
    match outcome {
        SessionOutcome::Completed {
            story_key,
            branch,
            decisions,
            pr_context,
            pr_how_to_test,
            pr_additional_info,
        } => SessionOutcome::Completed {
            story_key: story_key.clone(),
            branch: branch.clone(),
            decisions: decisions.clone(),
            pr_context: pr_context.clone(),
            pr_how_to_test: pr_how_to_test.clone(),
            pr_additional_info: pr_additional_info.clone(),
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
pub struct ReviewRunCall {
    pub story_key: String,
}

/// Mock review runner that returns a configurable `ReviewOutcome`.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<Option<ReviewOutcome>>>,
    calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create a new mock returning `Skipped` by default.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome to return.
    pub fn with_outcome(self, outcome: ReviewOutcome) -> Self {
        *self.outcome.lock().unwrap() = Some(outcome);
        self
    }

    /// Run the mock review.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunCall {
            story_key: story.story_key.clone(),
        });
        let stored = self.outcome.lock().unwrap();
        stored
            .as_ref()
            .map(clone_review_outcome)
            .unwrap_or(ReviewOutcome::Skipped {
                reason: "mock default".into(),
            })
    }


    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunCall> {
        self.calls.lock().unwrap().clone()
    }
}

fn clone_review_outcome(outcome: &ReviewOutcome) -> ReviewOutcome {
    match outcome {
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

// ---------------------------------------------------------------------------
// MockDevRunner
// ---------------------------------------------------------------------------

/// Mock implementation of [`DevRunner`] using `VecDeque` for multi-call support.
pub struct MockDevRunner {
    outcomes: Mutex<VecDeque<SessionOutcome>>,
    call_count: AtomicUsize,
}

impl MockDevRunner {
    /// Single-call mock.
    pub fn with_outcome(outcome: SessionOutcome) -> Self {
        let mut q = VecDeque::new();
        q.push_back(outcome);
        Self {
            outcomes: Mutex::new(q),
            call_count: AtomicUsize::new(0),
        }
    }

    /// Multi-call mock — pops outcomes in order per call.
    pub fn with_outcomes(outcomes: Vec<SessionOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            call_count: AtomicUsize::new(0),
        }
    }

    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl DevRunner for MockDevRunner {
    async fn run_dev_session(&self, _story: &StoryInfo) -> SessionOutcome {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockDevRunner: no more outcomes — add more via with_outcomes()")
    }
}

// ---------------------------------------------------------------------------
// MockCodeReviewer
// ---------------------------------------------------------------------------

/// Mock implementation of [`CodeReviewer`] using `VecDeque` for multi-call support.
pub struct MockCodeReviewer {
    outcomes: Mutex<VecDeque<ReviewOutcome>>,
    call_count: AtomicUsize,
}

impl MockCodeReviewer {
    pub fn with_outcome(outcome: ReviewOutcome) -> Self {
        let mut q = VecDeque::new();
        q.push_back(outcome);
        Self {
            outcomes: Mutex::new(q),
            call_count: AtomicUsize::new(0),
        }
    }

    pub fn never_called() -> Self {
        Self {
            outcomes: Mutex::new(VecDeque::new()),
            call_count: AtomicUsize::new(0),
        }
    }

    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl CodeReviewer for MockCodeReviewer {
    async fn run_review(&self, _story: &StoryInfo) -> ReviewOutcome {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockCodeReviewer: no more outcomes (or never_called() was used)")
    }
}
