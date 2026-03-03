//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.
//! Builder pattern for configuration keeps test code readable.

use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::pipeline::{CodeReviewer, DevRunner};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::session::runner::RecoveryInfo;
use bmad_bot::watcher::StoryInfo;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// MockGitProvider (Task 2)
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

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Builder pattern: configure return values with `with_*` methods,
/// then assert on calls via [`calls()`].
#[derive(Clone)]
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Option<Result<PrInfo, GitProviderError>>>>,
    add_comment_result: Arc<Mutex<Option<Result<(), GitProviderError>>>>,
    get_pr_url_result: Arc<Mutex<Option<Result<String, GitProviderError>>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    pub fn new() -> Self {
        Self {
            create_pr_result: Arc::new(Mutex::new(None)),
            add_comment_result: Arc::new(Mutex::new(None)),
            get_pr_url_result: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the result returned by `create_pr`. **One-shot**: consumed on first call;
    /// subsequent calls return the default success fallback.
    pub fn with_create_pr(self, result: Result<PrInfo, GitProviderError>) -> Self {
        *self.create_pr_result.lock().unwrap() = Some(result);
        self
    }

    /// Configure the result returned by `add_comment`. **One-shot**: consumed on first call;
    /// subsequent calls return `Ok(())`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        *self.add_comment_result.lock().unwrap() = Some(result);
        self
    }

    /// Configure the result returned by `get_pr_url`. **One-shot**: consumed on first call;
    /// subsequent calls return the default URL fallback.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        *self.get_pr_url_result.lock().unwrap() = Some(result);
        self
    }

    /// Returns a snapshot of all recorded calls.
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
        self.create_pr_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| {
                Ok(PrInfo {
                    id: "mock-1".to_string(),
                    url: "https://mock/pr/1".to_string(),
                    number: 1,
                })
            })
    }

    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::AddComment {
                pr_id: pr_id.to_string(),
                body: body.to_string(),
            });
        self.add_comment_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or(Ok(()))
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::GetPrUrl {
                pr_id: pr_id.to_string(),
            });
        self.get_pr_url_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Ok(format!("https://mock/pr/{pr_id}")))
    }
}

// ---------------------------------------------------------------------------
// MockNotifier (Task 3)
// ---------------------------------------------------------------------------

/// Captured call to `MockNotifier` for assertion.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    Story(StoryNotification),
    RunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] for integration tests.
///
/// Captures all `notify_story` and `notify_run_summary` calls into a `Vec`
/// for later assertion.
#[derive(Clone)]
pub struct MockNotifier {
    notify_story_result: Arc<Mutex<Option<Result<(), NotifierError>>>>,
    notify_summary_result: Arc<Mutex<Option<Result<(), NotifierError>>>>,
    calls: Arc<Mutex<Vec<NotifierCall>>>,
}

impl MockNotifier {
    pub fn new() -> Self {
        Self {
            notify_story_result: Arc::new(Mutex::new(None)),
            notify_summary_result: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the result returned by `notify_story`. **One-shot**: consumed on first call;
    /// subsequent calls return `Ok(())`.
    pub fn with_notify_story(self, result: Result<(), NotifierError>) -> Self {
        *self.notify_story_result.lock().unwrap() = Some(result);
        self
    }

    /// Configure the result returned by `notify_run_summary`. **One-shot**: consumed on first call;
    /// subsequent calls return `Ok(())`.
    pub fn with_notify_summary(self, result: Result<(), NotifierError>) -> Self {
        *self.notify_summary_result.lock().unwrap() = Some(result);
        self
    }

    /// Returns a snapshot of all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Returns only the `Story` calls.
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

    /// Returns only the `RunSummary` calls.
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
        self.notify_story_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or(Ok(()))
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::RunSummary(summary.clone()));
        self.notify_summary_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or(Ok(()))
    }
}

// ---------------------------------------------------------------------------
// MockDevRunner (Story 7.4 Task 1)
// ---------------------------------------------------------------------------

/// Mock implementation of [`DevRunner`] for integration tests.
///
/// Uses `VecDeque` to support sequential multi-call scenarios
/// (e.g., `process_eligible_stories`). `SessionOutcome` does NOT derive `Clone`,
/// so outcomes are consumed (popped) on each call.
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
// MockCodeReviewer (Story 7.4 Task 1)
// ---------------------------------------------------------------------------

/// Mock implementation of [`CodeReviewer`] for integration tests.
///
/// Tracks call count and supports sequential outcomes via `VecDeque`.
pub struct MockCodeReviewer {
    outcomes: Mutex<VecDeque<ReviewOutcome>>,
    call_count: Arc<AtomicUsize>,
}

/// Assertion handle for `MockCodeReviewer` — shares interior state via `Arc`.
///
/// Returned from `PipelineTestBuilder::build()` so tests can assert
/// `call_count()` without consuming the mock itself.
#[derive(Clone)]
pub struct MockCodeReviewerHandle {
    call_count: Arc<AtomicUsize>,
}

impl MockCodeReviewerHandle {
    /// Number of times `run_review` was called on the associated mock.
    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

impl MockCodeReviewer {
    pub fn with_outcome(outcome: ReviewOutcome) -> (Self, MockCodeReviewerHandle) {
        let call_count = Arc::new(AtomicUsize::new(0));
        let mut q = VecDeque::new();
        q.push_back(outcome);
        let mock = Self {
            outcomes: Mutex::new(q),
            call_count: Arc::clone(&call_count),
        };
        let handle = MockCodeReviewerHandle { call_count };
        (mock, handle)
    }

    /// Multi-call mock — pops outcomes in order per call.
    pub fn with_outcomes(outcomes: Vec<ReviewOutcome>) -> (Self, MockCodeReviewerHandle) {
        let call_count = Arc::new(AtomicUsize::new(0));
        let mock = Self {
            outcomes: Mutex::new(outcomes.into()),
            call_count: Arc::clone(&call_count),
        };
        let handle = MockCodeReviewerHandle { call_count };
        (mock, handle)
    }

    /// Mock that panics with a clear message if `run_review` is ever called.
    ///
    /// Used when the test asserts the reviewer must NOT be called (e.g. review disabled).
    pub fn never_called() -> (Self, MockCodeReviewerHandle) {
        let call_count = Arc::new(AtomicUsize::new(0));
        let mock = Self {
            outcomes: Mutex::new(VecDeque::new()),
            call_count: Arc::clone(&call_count),
        };
        let handle = MockCodeReviewerHandle { call_count };
        (mock, handle)
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
            .expect("MockCodeReviewer: run_review called but no outcome queued (never_called() or exhausted)")
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner (Task 4)
// ---------------------------------------------------------------------------

/// Captured call to `MockSessionRunner` for assertion.
#[derive(Debug, Clone)]
pub enum SessionRunnerCall {
    Run { story_key: String },
    CheckAndRecoverWal,
}

/// Mock session runner that returns configurable `SessionOutcome`.
///
/// Not trait-based — mirrors the real `SessionRunner` public API surface.
pub struct MockSessionRunner {
    run_result: Arc<Mutex<Option<SessionOutcome>>>,
    recover_result: Arc<Mutex<Option<RecoveryInfo>>>,
    calls: Arc<Mutex<Vec<SessionRunnerCall>>>,
}

impl MockSessionRunner {
    pub fn new() -> Self {
        Self {
            run_result: Arc::new(Mutex::new(None)),
            recover_result: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the `SessionOutcome` returned by `run`. **One-shot**: consumed on first call;
    /// subsequent calls return a default `Completed` outcome.
    pub fn with_run_result(self, outcome: SessionOutcome) -> Self {
        *self.run_result.lock().unwrap() = Some(outcome);
        self
    }

    /// Configure the `RecoveryInfo` returned by `check_and_recover_wal`. **One-shot**: consumed on first call.
    /// Uses the real [`bmad_bot::session::runner::RecoveryInfo`] type for API compatibility.
    pub fn with_recovery(self, info: RecoveryInfo) -> Self {
        *self.recover_result.lock().unwrap() = Some(info);
        self
    }

    /// Simulate running a session for the given story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunnerCall::Run {
            story_key: story.story_key.clone(),
        });
        self.run_result.lock().unwrap().take().unwrap_or_else(|| {
            SessionOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                decisions: vec![],
                pr_context: None,
                pr_how_to_test: None,
                pr_additional_info: None,
            }
        })
    }

    /// Simulate checking for WAL recovery.
    pub async fn check_and_recover_wal(&self) -> Option<RecoveryInfo> {
        self.calls
            .lock()
            .unwrap()
            .push(SessionRunnerCall::CheckAndRecoverWal);
        self.recover_result.lock().unwrap().take()
    }

    /// Returns a snapshot of all recorded calls.
    pub fn calls(&self) -> Vec<SessionRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl Default for MockSessionRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner (Task 5)
// ---------------------------------------------------------------------------

/// Captured call to `MockReviewRunner` for assertion.
#[derive(Debug, Clone)]
pub enum ReviewRunnerCall {
    Run { story_key: String },
}

/// Mock review runner that returns configurable `ReviewOutcome`.
///
/// Not trait-based — mirrors the real `ReviewRunner` public API surface.
pub struct MockReviewRunner {
    run_result: Arc<Mutex<Option<ReviewOutcome>>>,
    calls: Arc<Mutex<Vec<ReviewRunnerCall>>>,
}

impl MockReviewRunner {
    pub fn new() -> Self {
        Self {
            run_result: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the `ReviewOutcome` returned by `run`. **One-shot**: consumed on first call;
    /// subsequent calls return a default `Completed` outcome.
    pub fn with_run_result(self, outcome: ReviewOutcome) -> Self {
        *self.run_result.lock().unwrap() = Some(outcome);
        self
    }

    /// Simulate running a review for the given story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunnerCall::Run {
            story_key: story.story_key.clone(),
        });
        self.run_result.lock().unwrap().take().unwrap_or_else(|| {
            ReviewOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                report: "Mock review report".to_string(),
            }
        })
    }

    /// Returns a snapshot of all recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl Default for MockReviewRunner {
    fn default() -> Self {
        Self::new()
    }
}
