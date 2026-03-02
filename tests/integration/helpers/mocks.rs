//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.
//! Builder-pattern configuration keeps tests readable.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::pipeline::{CodeReviewer, DevRunner};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Captured call information for `MockGitProvider`.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    /// `create_pr` was called with these params.
    CreatePr(CreatePrParams),
    /// `add_comment` was called with `(pr_id, body)`.
    AddComment(String, String),
    /// `get_pr_url` was called with `pr_id`.
    GetPrUrl(String),
}

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Configurable return values via builder methods. Tracks all calls for assertions.
/// `Clone` shares inner `Arc` state — both copies see the same captured calls.
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl Clone for MockGitProvider {
    fn clone(&self) -> Self {
        Self {
            create_pr_result: Arc::clone(&self.create_pr_result),
            add_comment_result: Arc::clone(&self.add_comment_result),
            get_pr_url_result: Arc::clone(&self.get_pr_url_result),
            calls: Arc::clone(&self.calls),
        }
    }
}

impl MockGitProvider {
    /// Create a new mock with default `Ok` responses.
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

    /// Return a snapshot of all calls made to this mock.
    pub fn calls(&self) -> Vec<GitProviderCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Return all captured `create_pr` parameters.
    pub fn captured_create_pr_params(&self) -> Vec<CreatePrParams> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                GitProviderCall::CreatePr(p) => Some(p.clone()),
                _ => None,
            })
            .collect()
    }

    /// Return all captured `add_comment` calls as `(pr_id, body)` pairs.
    pub fn captured_add_comment_calls(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                GitProviderCall::AddComment(pr_id, body) => {
                    Some((pr_id.clone(), body.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Count of `create_pr` calls.
    pub fn create_pr_call_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, GitProviderCall::CreatePr(_)))
            .count()
    }

    /// Count of `add_comment` calls.
    pub fn add_comment_call_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, GitProviderCall::AddComment(_, _)))
            .count()
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
            .push(GitProviderCall::AddComment(pr_id.into(), body.into()));
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
            .push(GitProviderCall::GetPrUrl(pr_id.into()));
        let result = self.get_pr_url_result.lock().unwrap();
        match &*result {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Helper to clone a `GitProviderError` since it doesn't derive Clone.
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

/// Captured notification call information.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// `notify_story` was called.
    Story(StoryNotification),
    /// `notify_run_summary` was called.
    Summary(RunSummary),
}

/// Mock implementation of [`Notifier`] for integration tests.
///
/// Captures all calls into a `Vec` for assertion. Unlike `NoopNotifier`,
/// this mock preserves the data for inspection.
/// `Clone` shares inner `Arc` state — both copies see the same captured calls.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
    /// When `Some`, `notify_story` returns this error.
    story_error: Arc<Mutex<Option<String>>>,
}

impl Clone for MockNotifier {
    fn clone(&self) -> Self {
        Self {
            calls: Arc::clone(&self.calls),
            story_error: Arc::clone(&self.story_error),
        }
    }
}

impl MockNotifier {
    /// Create a new mock notifier with empty call history.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            story_error: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a mock notifier that returns an error on `notify_story`.
    ///
    /// Note: `notify_run_summary` always succeeds on this notifier — only story
    /// notifications fail. This is intentional: tests only need story-level failure.
    pub fn failing(reason: &str) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            story_error: Arc::new(Mutex::new(Some(reason.to_string()))),
        }
    }

    /// Return all captured calls.
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
                NotifierCall::Summary(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    /// Count of `notify_story` calls.
    pub fn story_notification_count(&self) -> usize {
        self.story_calls().len()
    }

    /// Count of `notify_run_summary` calls.
    pub fn run_summary_count(&self) -> usize {
        self.summary_calls().len()
    }
}

#[async_trait]
impl Notifier for MockNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::Story(notification.clone()));
        if let Some(reason) = self.story_error.lock().unwrap().as_ref() {
            return Err(NotifierError::HttpRequest {
                reason: reason.clone(),
            });
        }
        Ok(())
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::Summary(summary.clone()));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Captured session runner call information.
#[derive(Debug, Clone)]
pub struct SessionRunnerCall {
    /// The story key that was passed to `run`.
    pub story_key: String,
}

/// Mock session runner for integration tests.
///
/// Returns a configurable `SessionOutcome`. Does NOT implement a shared trait
/// with the real `SessionRunner` (see dev notes — Story 7.4 will address injection).
pub struct MockSessionRunner {
    outcome_fn: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> SessionOutcome + Send>>>,
    calls: Arc<Mutex<Vec<SessionRunnerCall>>>,
}

impl MockSessionRunner {
    /// Create a mock that always returns `SessionOutcome::Completed` with defaults.
    pub fn new() -> Self {
        Self {
            outcome_fn: Arc::new(Mutex::new(Box::new(|story| SessionOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                decisions: vec![],
                pr_context: None,
                pr_how_to_test: None,
                pr_additional_info: None,
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure a custom outcome function.
    pub fn with_outcome<F>(self, f: F) -> Self
    where
        F: Fn(&StoryInfo) -> SessionOutcome + Send + 'static,
    {
        *self.outcome_fn.lock().unwrap() = Box::new(f);
        self
    }

    /// Run the mock session for a story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunnerCall {
            story_key: story.story_key.clone(),
        });
        let f = self.outcome_fn.lock().unwrap();
        f(story)
    }

    /// Check for WAL recovery — always returns `None` (no crash recovery in mocks).
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

/// Captured review runner call information.
#[derive(Debug, Clone)]
pub struct ReviewRunnerCall {
    /// The story key that was passed to `run`.
    pub story_key: String,
}

/// Mock review runner for integration tests.
///
/// Returns a configurable `ReviewOutcome`. Does NOT implement a shared trait
/// with the real `ReviewRunner`.
pub struct MockReviewRunner {
    outcome_fn: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> ReviewOutcome + Send>>>,
    calls: Arc<Mutex<Vec<ReviewRunnerCall>>>,
}

impl MockReviewRunner {
    /// Create a mock that always returns `ReviewOutcome::Skipped`.
    pub fn new() -> Self {
        Self {
            outcome_fn: Arc::new(Mutex::new(Box::new(|_| ReviewOutcome::Skipped {
                reason: "mock review skipped".into(),
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure a custom outcome function.
    pub fn with_outcome<F>(self, f: F) -> Self
    where
        F: Fn(&StoryInfo) -> ReviewOutcome + Send + 'static,
    {
        *self.outcome_fn.lock().unwrap() = Box::new(f);
        self
    }

    /// Run the mock review for a story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunnerCall {
            story_key: story.story_key.clone(),
        });
        let f = self.outcome_fn.lock().unwrap();
        f(story)
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockDevRunner (implements pipeline::DevRunner trait)
// ---------------------------------------------------------------------------

/// Mock implementation of [`DevRunner`] for pipeline integration tests.
///
/// Uses `VecDeque<SessionOutcome>` to support multi-call scenarios
/// (e.g., `process_eligible_stories` with multiple stories).
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

    /// Number of times `run_dev_session` was called.
    #[allow(dead_code)]
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
// MockCodeReviewer (implements pipeline::CodeReviewer trait)
// ---------------------------------------------------------------------------

/// Mock implementation of [`CodeReviewer`] for pipeline integration tests.
///
/// Uses `VecDeque<ReviewOutcome>` plus `Arc<AtomicUsize>` call counter.
/// `Clone` shares the same `Arc` state — both copies see the same call count,
/// enabling assertion after the mock is boxed into the pipeline.
pub struct MockCodeReviewer {
    outcomes: Mutex<VecDeque<ReviewOutcome>>,
    call_count: Arc<AtomicUsize>,
}

impl Clone for MockCodeReviewer {
    fn clone(&self) -> Self {
        // Share the same VecDeque is NOT desired here (outcomes are consumed),
        // but sharing the call_count Arc IS desired for post-build assertions.
        Self {
            outcomes: Mutex::new(VecDeque::new()),
            call_count: Arc::clone(&self.call_count),
        }
    }
}

impl MockCodeReviewer {
    /// Single-call mock.
    pub fn with_outcome(outcome: ReviewOutcome) -> Self {
        let mut q = VecDeque::new();
        q.push_back(outcome);
        Self {
            outcomes: Mutex::new(q),
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Create a mock that panics if called (asserts zero calls).
    pub fn never_called() -> Self {
        Self {
            outcomes: Mutex::new(VecDeque::new()),
            call_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Number of times `run_review` was called.
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
