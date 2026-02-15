//! Mock implementations for integration tests.
//!
//! Provides configurable mock structs for `GitProvider`, `Notifier`,
//! session runner, review runner, `DevRunner`, and `CodeReviewer`. All mocks are `Send + Sync`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::pipeline::{CodeReviewer, DevRunner};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::supervisor::decisions::DecisionRecord;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Recorded call to a `MockGitProvider` method.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    /// `create_pr` was called with these params.
    CreatePr {
        title: String,
        body: String,
        source_branch: String,
        target_branch: String,
    },
    /// `add_comment` was called with these params.
    AddComment { pr_id: String, body: String },
    /// `get_pr_url` was called with this PR ID.
    GetPrUrl { pr_id: String },
}

/// Configurable mock for the [`GitProvider`] trait.
///
/// Builder pattern: call `with_create_pr(...)`, `with_add_comment(...)`, etc.
/// to set return values. The trait impl returns the configured value and records
/// calls for later assertion.
///
/// Implements `Clone` by cloning inner `Arc`s — cloned copies share the same
/// capture buffers, enabling assertion on calls after the pipeline consumes one copy.
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
    /// Create a new `MockGitProvider` with default Ok results.
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

    /// Configure the `create_pr` return value.
    pub fn with_create_pr(self, result: Result<PrInfo, GitProviderError>) -> Self {
        *self.create_pr_result.lock().unwrap() = result;
        self
    }

    /// Configure the `add_comment` return value.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        *self.add_comment_result.lock().unwrap() = result;
        self
    }

    /// Configure the `get_pr_url` return value.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        *self.get_pr_url_result.lock().unwrap() = result;
        self
    }

    /// Return a clone of all recorded calls.
    pub fn calls(&self) -> Vec<GitProviderCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Return captured `create_pr` call params.
    pub fn captured_create_pr_params(&self) -> Vec<CreatePrParams> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                GitProviderCall::CreatePr {
                    title,
                    body,
                    source_branch,
                    target_branch,
                } => Some(CreatePrParams {
                    title: title.clone(),
                    body: body.clone(),
                    source_branch: source_branch.clone(),
                    target_branch: target_branch.clone(),
                }),
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

    /// Count of `create_pr` calls.
    pub fn create_pr_call_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, GitProviderCall::CreatePr { .. }))
            .count()
    }

    /// Count of `add_comment` calls.
    pub fn add_comment_call_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, GitProviderCall::AddComment { .. }))
            .count()
    }
}

#[async_trait]
impl GitProvider for MockGitProvider {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError> {
        self.calls.lock().unwrap().push(GitProviderCall::CreatePr {
            title: params.title.clone(),
            body: params.body.clone(),
            source_branch: params.source_branch.clone(),
            target_branch: params.target_branch.clone(),
        });
        let guard = self.create_pr_result.lock().unwrap();
        match &*guard {
            Ok(info) => Ok(info.clone()),
            Err(e) => Err(mock_git_error(e)),
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
            Err(e) => Err(mock_git_error(e)),
        }
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls.lock().unwrap().push(GitProviderCall::GetPrUrl {
            pr_id: pr_id.to_string(),
        });
        let guard = self.get_pr_url_result.lock().unwrap();
        match &*guard {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(mock_git_error(e)),
        }
    }
}

/// Clone a `GitProviderError` for mock return (errors are not `Clone`).
fn mock_git_error(e: &GitProviderError) -> GitProviderError {
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
    /// `notify_story` was called.
    Story(StoryNotification),
    /// `notify_run_summary` was called.
    Summary(RunSummary),
}

/// Configurable mock for the [`Notifier`] trait.
///
/// Captures every call into a `Vec` for later assertion.
/// Implements `Clone` by cloning inner `Arc`s — same sharing pattern as `MockGitProvider`.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
    story_result: Arc<Mutex<Result<(), NotifierError>>>,
    summary_result: Arc<Mutex<Result<(), NotifierError>>>,
}

impl Clone for MockNotifier {
    fn clone(&self) -> Self {
        Self {
            calls: Arc::clone(&self.calls),
            story_result: Arc::clone(&self.story_result),
            summary_result: Arc::clone(&self.summary_result),
        }
    }
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

    /// Configure the `notify_story` return value.
    pub fn with_story_result(self, result: Result<(), NotifierError>) -> Self {
        *self.story_result.lock().unwrap() = result;
        self
    }

    /// Configure the `notify_run_summary` return value.
    pub fn with_summary_result(self, result: Result<(), NotifierError>) -> Self {
        *self.summary_result.lock().unwrap() = result;
        self
    }

    /// Return a clone of all recorded calls.
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

    /// Count of `notify_story` calls captured.
    pub fn story_notification_count(&self) -> usize {
        self.story_calls().len()
    }

    /// Count of `notify_run_summary` calls captured.
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
            .push(NotifierCall::Summary(summary.clone()));
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

/// Recorded call to `MockSessionRunner::run`.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    pub story_key: String,
}

/// Standalone mock that mimics the `SessionRunner` public API.
///
/// Returns a configurable `SessionOutcome` from `run()`.
pub struct MockSessionRunner {
    outcome_factory: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> SessionOutcome + Send>>>,
    run_calls: Arc<Mutex<Vec<SessionRunCall>>>,
}

impl MockSessionRunner {
    /// Create a mock that returns `SessionOutcome::Completed` for every story.
    pub fn new() -> Self {
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(|story| SessionOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                decisions: Vec::new(),
                pr_context: None,
                pr_how_to_test: None,
                pr_additional_info: None,
            }))),
            run_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run()`.
    pub fn with_outcome<F>(self, f: F) -> Self
    where
        F: Fn(&StoryInfo) -> SessionOutcome + Send + 'static,
    {
        *self.outcome_factory.lock().unwrap() = Box::new(f);
        self
    }

    /// Simulate `SessionRunner::run`.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.run_calls.lock().unwrap().push(SessionRunCall {
            story_key: story.story_key.clone(),
        });
        let factory = self.outcome_factory.lock().unwrap();
        factory(story)
    }

    /// Simulate `SessionRunner::check_and_recover_wal` — always returns `None`.
    pub async fn check_and_recover_wal(&self) -> Option<()> {
        None
    }

    /// Return recorded `run()` calls.
    pub fn run_calls(&self) -> Vec<SessionRunCall> {
        self.run_calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Recorded call to `MockReviewRunner::run`.
#[derive(Debug, Clone)]
pub struct ReviewRunCall {
    pub story_key: String,
}

/// Standalone mock that mimics the `ReviewRunner` public API.
///
/// Returns a configurable `ReviewOutcome` from `run()`.
pub struct MockReviewRunner {
    outcome_factory: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> ReviewOutcome + Send>>>,
    run_calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create a mock that returns `ReviewOutcome::Completed` for every story.
    pub fn new() -> Self {
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(|story| ReviewOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                report: "Mock review report".into(),
            }))),
            run_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run()`.
    pub fn with_outcome<F>(self, f: F) -> Self
    where
        F: Fn(&StoryInfo) -> ReviewOutcome + Send + 'static,
    {
        *self.outcome_factory.lock().unwrap() = Box::new(f);
        self
    }

    /// Simulate `ReviewRunner::run`.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.run_calls.lock().unwrap().push(ReviewRunCall {
            story_key: story.story_key.clone(),
        });
        let factory = self.outcome_factory.lock().unwrap();
        factory(story)
    }

    /// Return recorded `run()` calls.
    pub fn run_calls(&self) -> Vec<ReviewRunCall> {
        self.run_calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockDevRunner (implements pipeline::DevRunner trait)
// ---------------------------------------------------------------------------

/// Mock implementation of [`DevRunner`] for pipeline integration tests.
///
/// Uses `VecDeque<SessionOutcome>` to support `process_eligible_stories()`
/// multi-call scenarios. `SessionOutcome` does not derive `Clone`, so a
/// queue of owned values is the correct pattern.
pub struct MockDevRunner {
    outcomes: Mutex<VecDeque<SessionOutcome>>,
    call_count: AtomicUsize,
}

impl MockDevRunner {
    /// Create a mock that returns a single outcome.
    pub fn with_outcome(outcome: SessionOutcome) -> Self {
        let mut q = VecDeque::new();
        q.push_back(outcome);
        Self {
            outcomes: Mutex::new(q),
            call_count: AtomicUsize::new(0),
        }
    }

    /// Create a mock with multiple sequential outcomes (one per call).
    pub fn with_outcomes(outcomes: Vec<SessionOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
            call_count: AtomicUsize::new(0),
        }
    }

    /// Number of times `run_dev_session` was called.
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
/// Uses `VecDeque<ReviewOutcome>` plus an `AtomicUsize` call counter.
pub struct MockCodeReviewer {
    outcomes: Mutex<VecDeque<ReviewOutcome>>,
    call_count: AtomicUsize,
}

impl MockCodeReviewer {
    /// Create a mock that returns a single outcome.
    pub fn with_outcome(outcome: ReviewOutcome) -> Self {
        let mut q = VecDeque::new();
        q.push_back(outcome);
        Self {
            outcomes: Mutex::new(q),
            call_count: AtomicUsize::new(0),
        }
    }

    /// Create a mock that panics if called — use for review-disabled tests.
    pub fn never_called() -> Self {
        Self {
            outcomes: Mutex::new(VecDeque::new()),
            call_count: AtomicUsize::new(0),
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
