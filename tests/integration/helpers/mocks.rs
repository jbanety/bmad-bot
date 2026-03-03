//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.

use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::pipeline::{CodeReviewer, DevRunner};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::session::runner::RecoveryInfo;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider (Task 2)
// ---------------------------------------------------------------------------

/// Recorded call to a `GitProvider` method.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    CreatePr(CreatePrParams),
    AddComment { pr_id: String, body: String },
    GetPrUrl(String),
}

type GitProviderFactory<T> = Box<dyn Fn() -> Result<T, GitProviderError> + Send>;

/// Mock implementation of [`GitProvider`] with configurable return values and
/// call tracking.
///
/// Implements `Clone` by cloning inner `Arc` handles — both copies share the
/// same capture buffers.
pub struct MockGitProvider {
    create_pr_factory: Arc<Mutex<GitProviderFactory<PrInfo>>>,
    add_comment_factory: Arc<Mutex<GitProviderFactory<()>>>,
    get_pr_url_factory: Arc<Mutex<GitProviderFactory<String>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl Clone for MockGitProvider {
    fn clone(&self) -> Self {
        Self {
            create_pr_factory: Arc::clone(&self.create_pr_factory),
            add_comment_factory: Arc::clone(&self.add_comment_factory),
            get_pr_url_factory: Arc::clone(&self.get_pr_url_factory),
            calls: Arc::clone(&self.calls),
        }
    }
}

impl Default for MockGitProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockGitProvider {
    /// Create a new mock with sensible defaults (all return `Ok`).
    pub fn new() -> Self {
        Self {
            create_pr_factory: Arc::new(Mutex::new(Box::new(|| {
                Ok(PrInfo {
                    id: "1".into(),
                    url: "https://github.com/test/test/pull/1".into(),
                    number: 1,
                })
            }))),
            add_comment_factory: Arc::new(Mutex::new(Box::new(|| Ok(())))),
            get_pr_url_factory: Arc::new(Mutex::new(Box::new(|| {
                Ok("https://github.com/test/test/pull/1".into())
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the result returned by `create_pr`.
    pub fn with_create_pr<F>(self, f: F) -> Self
    where
        F: Fn() -> Result<PrInfo, GitProviderError> + Send + 'static,
    {
        *self.create_pr_factory.lock().unwrap() = Box::new(f);
        self
    }

    /// Configure the result returned by `add_comment`.
    pub fn with_add_comment<F>(self, f: F) -> Self
    where
        F: Fn() -> Result<(), GitProviderError> + Send + 'static,
    {
        *self.add_comment_factory.lock().unwrap() = Box::new(f);
        self
    }

    /// Configure the result returned by `get_pr_url`.
    pub fn with_get_pr_url<F>(self, f: F) -> Self
    where
        F: Fn() -> Result<String, GitProviderError> + Send + 'static,
    {
        *self.get_pr_url_factory.lock().unwrap() = Box::new(f);
        self
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<GitProviderCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Return all `create_pr` call parameters.
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

    /// Return all `add_comment` calls as `(pr_id, body)` pairs.
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
        self.captured_create_pr_params().len()
    }

    /// Count of `add_comment` calls.
    pub fn add_comment_call_count(&self) -> usize {
        self.captured_add_comment_calls().len()
    }
}

#[async_trait]
impl GitProvider for MockGitProvider {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::CreatePr(params));
        let factory = self.create_pr_factory.lock().unwrap();
        factory()
    }

    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::AddComment {
                pr_id: pr_id.to_string(),
                body: body.to_string(),
            });
        let factory = self.add_comment_factory.lock().unwrap();
        factory()
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::GetPrUrl(pr_id.to_string()));
        let factory = self.get_pr_url_factory.lock().unwrap();
        factory()
    }
}

// ---------------------------------------------------------------------------
// MockNotifier (Task 3)
// ---------------------------------------------------------------------------

/// Recorded call to a `Notifier` method.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    Story(StoryNotification),
    RunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] that captures all calls for assertion.
///
/// Implements `Clone` by cloning inner `Arc` handles — both copies share the
/// same capture buffers.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
    /// When `Some`, `notify_story` returns this error instead of `Ok(())`.
    story_error: Arc<Mutex<Option<Box<dyn Fn() -> NotifierError + Send>>>>,
}

impl Clone for MockNotifier {
    fn clone(&self) -> Self {
        Self {
            calls: Arc::clone(&self.calls),
            story_error: Arc::clone(&self.story_error),
        }
    }
}

impl Default for MockNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl MockNotifier {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            story_error: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a mock where `notify_story` always returns the given error.
    pub fn failing_story<F>(f: F) -> Self
    where
        F: Fn() -> NotifierError + Send + 'static,
    {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            story_error: Arc::new(Mutex::new(Some(Box::new(f)))),
        }
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Return only `notify_story` calls.
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

    /// Return only `notify_run_summary` calls.
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
    async fn notify_story(
        &self,
        notification: &StoryNotification,
    ) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::Story(notification.clone()));
        // Check if we should return an error
        if let Some(err_factory) = self.story_error.lock().unwrap().as_ref() {
            return Err(err_factory());
        }
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
// MockSessionRunner (Task 4)
// ---------------------------------------------------------------------------

/// Recorded call to `MockSessionRunner::run`.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    pub story_key: String,
}

/// Mock session runner — returns a configurable [`SessionOutcome`].
///
/// # Scope
/// This is a **standalone struct** that mirrors the public method signatures of
/// the real `SessionRunner`. It is NOT a trait implementation because `Pipeline`
/// uses `SessionRunner` as a concrete type, not a boxed trait. These mocks are
/// intended for unit-testing code that calls `.run()` / `.check_and_recover_wal()`
/// in isolation, not for substitution into a live `Pipeline` instance.
pub struct MockSessionRunner {
    outcome_factory: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> SessionOutcome + Send>>>,
    calls: Arc<Mutex<Vec<SessionRunCall>>>,
}

impl MockSessionRunner {
    /// Create with a factory that produces a `Completed` outcome.
    pub fn completed() -> Self {
        Self::with_factory(|story| SessionOutcome::Completed {
            story_key: story.story_key.clone(),
            branch: story.branch_name.clone(),
            decisions: vec![],
            pr_context: None,
            pr_how_to_test: None,
            pr_additional_info: None,
        })
    }

    /// Create with a factory that produces an `Escalated` outcome.
    pub fn escalated() -> Self {
        Self::with_factory(|story| SessionOutcome::Escalated {
            report: bmad_bot::session::escalation::EscalationReport::new(
                story.story_key.clone(),
                "test question".into(),
                "test reason".into(),
                story.branch_name.clone(),
                "partial work".into(),
            ),
            decisions: vec![],
        })
    }

    /// Create with a factory that produces a `Failed` outcome.
    pub fn failed(error: &str) -> Self {
        let error = error.to_string();
        Self::with_factory(move |story| SessionOutcome::Failed {
            story_key: story.story_key.clone(),
            error: error.clone(),
            decisions: vec![],
        })
    }

    /// Create with a custom factory function.
    pub fn with_factory<F>(f: F) -> Self
    where
        F: Fn(&StoryInfo) -> SessionOutcome + Send + 'static,
    {
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(f))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Run the mock session for the given story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunCall {
            story_key: story.story_key.clone(),
        });
        let factory = self.outcome_factory.lock().unwrap();
        factory(story)
    }

    /// Check and recover WAL — always returns `None` (no crash in mock).
    ///
    /// Returns the correct [`RecoveryInfo`] type to match the real
    /// `SessionRunner::check_and_recover_wal` signature.
    pub async fn check_and_recover_wal(&self) -> Option<RecoveryInfo> {
        None
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<SessionRunCall> {
        self.calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner (Task 5)
// ---------------------------------------------------------------------------

/// Recorded call to `MockReviewRunner::run`.
#[derive(Debug, Clone)]
pub struct ReviewRunCall {
    pub story_key: String,
}

/// Mock review runner — returns a configurable [`ReviewOutcome`].
///
/// # Scope
/// This is a **standalone struct** that mirrors the public method signatures of
/// the real `ReviewRunner`. It is NOT a trait implementation because `Pipeline`
/// uses `ReviewRunner` as a concrete type, not a boxed trait. These mocks are
/// intended for unit-testing code that calls `.run()` in isolation.
pub struct MockReviewRunner {
    outcome_factory: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> ReviewOutcome + Send>>>,
    calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create a mock that always returns `Completed`.
    pub fn completed() -> Self {
        Self::with_factory(|story| ReviewOutcome::Completed {
            story_key: story.story_key.clone(),
            branch: story.branch_name.clone(),
            report: "Mock review report".into(),
        })
    }

    /// Create a mock that always returns `Skipped`.
    pub fn skipped(reason: &str) -> Self {
        let reason = reason.to_string();
        Self::with_factory(move |_| ReviewOutcome::Skipped {
            reason: reason.clone(),
        })
    }

    /// Create a mock that always returns `Failed`.
    pub fn failed(error: &str) -> Self {
        let error = error.to_string();
        Self::with_factory(move |story| ReviewOutcome::Failed {
            story_key: story.story_key.clone(),
            error: error.clone(),
        })
    }

    /// Create with a custom factory function.
    pub fn with_factory<F>(f: F) -> Self
    where
        F: Fn(&StoryInfo) -> ReviewOutcome + Send + 'static,
    {
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(f))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Run the mock review for the given story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunCall {
            story_key: story.story_key.clone(),
        });
        let factory = self.outcome_factory.lock().unwrap();
        factory(story)
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunCall> {
        self.calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockDevRunner — implements DevRunner trait (Story 7.4 Task 1)
// ---------------------------------------------------------------------------

/// Mock dev runner for integration tests — implements [`DevRunner`].
///
/// Uses `VecDeque<SessionOutcome>` to support multi-call scenarios
/// (e.g., `process_eligible_stories`). `SessionOutcome` is NOT Clone,
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
// MockCodeReviewer — implements CodeReviewer trait (Story 7.4 Task 1)
// ---------------------------------------------------------------------------

/// Mock code reviewer for integration tests — implements [`CodeReviewer`].
///
/// Two modes:
/// - `with_outcome(o)` — returns the configured outcome when called.
/// - `never_called()` — **asserts** that `run_review` is never invoked.
///   Any call causes an immediate test panic with a clear message.
///   This enforces AC assertions like "MockCodeReviewer is NOT called".
pub struct MockCodeReviewer {
    outcomes: Mutex<VecDeque<ReviewOutcome>>,
    call_count: AtomicUsize,
    /// When `true`, any call to `run_review` is a test failure.
    never_called_mode: bool,
}

impl MockCodeReviewer {
    pub fn with_outcome(outcome: ReviewOutcome) -> Self {
        let mut q = VecDeque::new();
        q.push_back(outcome);
        Self {
            outcomes: Mutex::new(q),
            call_count: AtomicUsize::new(0),
            never_called_mode: false,
        }
    }

    /// Configure the mock to **assert** it is never called.
    ///
    /// Any invocation of `run_review` will panic with a descriptive message,
    /// causing the test to fail. Use this to enforce AC assertions such as
    /// "MockCodeReviewer is NOT called (review skipped)" (AC #4) or
    /// "MockCodeReviewer is NOT called (no PR means no point running review)" (AC #5).
    pub fn never_called() -> Self {
        Self {
            outcomes: Mutex::new(VecDeque::new()),
            call_count: AtomicUsize::new(0),
            never_called_mode: true,
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
        assert!(
            !self.never_called_mode,
            "MockCodeReviewer: run_review() was invoked but this mock was configured with \
             never_called() — the code reviewer must NOT be called in this test scenario. \
             Check that code_review_enabled=false is set or that the pipeline path \
             correctly skips review (e.g. after PR creation failure)."
        );
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockCodeReviewer: no more outcomes queued — add one via with_outcome()")
    }
}
