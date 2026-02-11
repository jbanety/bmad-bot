//! Mock implementations for integration tests.
//!
//! Provides configurable mocks for `GitProvider`, `Notifier`, `SessionRunner`, and `ReviewRunner`.
//! All mocks use `Arc<Mutex<...>>` for interior mutability and are `Send + Sync`.

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
    /// A `create_pr` call with the captured params.
    CreatePr(CreatePrParams),
    /// An `add_comment` call with `(pr_id, body)`.
    AddComment(String, String),
    /// A `get_pr_url` call with the PR ID.
    GetPrUrl(String),
}

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Configure return values via builder methods. Tracks all calls for assertion.
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new `MockGitProvider` with default OK responses.
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
        *self.create_pr_result.lock().expect("lock") = result;
        self
    }

    /// Configure the return value for `add_comment`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        *self.add_comment_result.lock().expect("lock") = result;
        self
    }

    /// Configure the return value for `get_pr_url`.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        *self.get_pr_url_result.lock().expect("lock") = result;
        self
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<GitProviderCall> {
        self.calls.lock().expect("lock").clone()
    }
}

/// Clone the inner result for return. `GitProviderError` is not `Clone`, so we must
/// re-create from the stored variant.
fn clone_pr_result(r: &Result<PrInfo, GitProviderError>) -> Result<PrInfo, GitProviderError> {
    match r {
        Ok(info) => Ok(info.clone()),
        Err(_) => Err(GitProviderError::ApiError {
            status: 500,
            message: "mock error".into(),
        }),
    }
}

fn clone_comment_result(r: &Result<(), GitProviderError>) -> Result<(), GitProviderError> {
    match r {
        Ok(()) => Ok(()),
        Err(_) => Err(GitProviderError::ApiError {
            status: 500,
            message: "mock error".into(),
        }),
    }
}

fn clone_url_result(r: &Result<String, GitProviderError>) -> Result<String, GitProviderError> {
    match r {
        Ok(url) => Ok(url.clone()),
        Err(_) => Err(GitProviderError::ApiError {
            status: 500,
            message: "mock error".into(),
        }),
    }
}

#[async_trait]
impl GitProvider for MockGitProvider {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError> {
        self.calls
            .lock()
            .expect("lock")
            .push(GitProviderCall::CreatePr(params));
        let guard = self.create_pr_result.lock().expect("lock");
        clone_pr_result(&guard)
    }

    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        self.calls
            .lock()
            .expect("lock")
            .push(GitProviderCall::AddComment(pr_id.into(), body.into()));
        let guard = self.add_comment_result.lock().expect("lock");
        clone_comment_result(&guard)
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .expect("lock")
            .push(GitProviderCall::GetPrUrl(pr_id.into()));
        let guard = self.get_pr_url_result.lock().expect("lock");
        clone_url_result(&guard)
    }
}

// ---------------------------------------------------------------------------
// MockNotifier
// ---------------------------------------------------------------------------

/// Captured call record for `MockNotifier`.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// A `notify_story` call.
    Story(StoryNotification),
    /// A `notify_run_summary` call.
    Summary(RunSummary),
}

/// Mock implementation of [`Notifier`] that captures all calls for assertion.
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

    /// Configure `notify_story` to return an error.
    pub fn with_story_error(self, err: NotifierError) -> Self {
        *self.story_result.lock().expect("lock") = Err(err);
        self
    }

    /// Configure `notify_run_summary` to return an error.
    pub fn with_summary_error(self, err: NotifierError) -> Self {
        *self.summary_result.lock().expect("lock") = Err(err);
        self
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().expect("lock").clone()
    }

    /// Return only `notify_story` calls.
    pub fn story_calls(&self) -> Vec<StoryNotification> {
        self.calls
            .lock()
            .expect("lock")
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
            .expect("lock")
            .iter()
            .filter_map(|c| match c {
                NotifierCall::Summary(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
}

fn clone_notifier_result(r: &Result<(), NotifierError>) -> Result<(), NotifierError> {
    match r {
        Ok(()) => Ok(()),
        Err(_) => Err(NotifierError::HttpRequest {
            reason: "mock error".into(),
        }),
    }
}

#[async_trait]
impl Notifier for MockNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .expect("lock")
            .push(NotifierCall::Story(notification.clone()));
        let guard = self.story_result.lock().expect("lock");
        clone_notifier_result(&guard)
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .expect("lock")
            .push(NotifierCall::Summary(summary.clone()));
        let guard = self.summary_result.lock().expect("lock");
        clone_notifier_result(&guard)
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Captured call record for `MockSessionRunner`.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    /// The story key that was passed to `run`.
    pub story_key: String,
}

/// Mock session runner that returns a configurable `SessionOutcome`.
///
/// This is a standalone mock — it does NOT implement a shared trait with the
/// real `SessionRunner` (the codebase doesn't define one). Story 7.4 will
/// address injection into `StoryPipeline`.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<SessionOutcome>>,
    run_calls: Arc<Mutex<Vec<SessionRunCall>>>,
    wal_recovery: Arc<Mutex<Option<()>>>,
    wal_calls: Arc<Mutex<u32>>,
}

impl MockSessionRunner {
    /// Create a new `MockSessionRunner` that returns `Completed` by default.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(SessionOutcome::Completed {
                story_key: "test".into(),
                branch: "story/test".into(),
                decisions: Vec::new(),
            })),
            run_calls: Arc::new(Mutex::new(Vec::new())),
            wal_recovery: Arc::new(Mutex::new(None)),
            wal_calls: Arc::new(Mutex::new(0)),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome(self, outcome: SessionOutcome) -> Self {
        *self.outcome.lock().expect("lock") = outcome;
        self
    }

    /// Run the mock session for a story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.run_calls.lock().expect("lock").push(SessionRunCall {
            story_key: story.story_key.clone(),
        });
        let guard = self.outcome.lock().expect("lock");
        // Create a new outcome matching the stored variant
        match &*guard {
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

    /// Check for WAL recovery (always returns `None` in mock).
    pub async fn check_and_recover_wal(&self) -> Option<()> {
        *self.wal_calls.lock().expect("lock") += 1;
        self.wal_recovery.lock().expect("lock").clone()
    }

    /// Return all `run` calls.
    pub fn run_calls(&self) -> Vec<SessionRunCall> {
        self.run_calls.lock().expect("lock").clone()
    }

    /// Return the number of times `check_and_recover_wal` was called.
    pub fn wal_call_count(&self) -> u32 {
        *self.wal_calls.lock().expect("lock")
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Captured call record for `MockReviewRunner`.
#[derive(Debug, Clone)]
pub struct ReviewRunCall {
    /// The story key that was passed to `run`.
    pub story_key: String,
}

/// Mock review runner that returns a configurable `ReviewOutcome`.
///
/// Standalone mock — same rationale as `MockSessionRunner`.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<ReviewOutcome>>,
    run_calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create a new `MockReviewRunner` that returns `Completed` by default.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(ReviewOutcome::Completed {
                story_key: "test".into(),
                branch: "story/test".into(),
                report: "LGTM".into(),
            })),
            run_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome(self, outcome: ReviewOutcome) -> Self {
        *self.outcome.lock().expect("lock") = outcome;
        self
    }

    /// Run the mock review for a story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.run_calls.lock().expect("lock").push(ReviewRunCall {
            story_key: story.story_key.clone(),
        });
        let guard = self.outcome.lock().expect("lock");
        match &*guard {
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

    /// Return all `run` calls.
    pub fn run_calls(&self) -> Vec<ReviewRunCall> {
        self.run_calls.lock().expect("lock").clone()
    }
}
