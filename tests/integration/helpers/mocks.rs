//! Mock implementations for integration tests.
//!
//! Provides configurable mocks for `GitProvider`, `Notifier`, `SessionRunner`, and `ReviewRunner`.
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

/// Captured call to a `MockGitProvider` method.
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

/// Configurable mock for the `GitProvider` trait.
///
/// Uses builder pattern: `MockGitProvider::new().with_create_pr(Ok(...))`.
/// Tracks all calls for later assertion.
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Option<Result<PrInfo, GitProviderError>>>>,
    add_comment_result: Arc<Mutex<Option<Result<(), GitProviderError>>>>,
    get_pr_url_result: Arc<Mutex<Option<Result<String, GitProviderError>>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new `MockGitProvider` with no configured results.
    /// Calls to unconfigured methods will panic.
    pub fn new() -> Self {
        Self {
            create_pr_result: Arc::new(Mutex::new(None)),
            add_comment_result: Arc::new(Mutex::new(None)),
            get_pr_url_result: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the result for `create_pr`.
    pub fn with_create_pr(self, result: Result<PrInfo, GitProviderError>) -> Self {
        *self.create_pr_result.lock().unwrap() = Some(result);
        self
    }

    /// Configure the result for `add_comment`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        *self.add_comment_result.lock().unwrap() = Some(result);
        self
    }

    /// Configure the result for `get_pr_url`.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        *self.get_pr_url_result.lock().unwrap() = Some(result);
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
        self.create_pr_result
            .lock()
            .unwrap()
            .take()
            .expect("MockGitProvider::create_pr called but no result configured")
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
            .expect("MockGitProvider::add_comment called but no result configured")
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
            .expect("MockGitProvider::get_pr_url called but no result configured")
    }
}

// ---------------------------------------------------------------------------
// MockNotifier
// ---------------------------------------------------------------------------

/// Captured call to a `MockNotifier` method.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    Story(StoryNotification),
    RunSummary(RunSummary),
}

/// Configurable mock for the `Notifier` trait.
///
/// Captures all calls into a `Vec` for assertion. Always returns `Ok(())`.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
}

impl MockNotifier {
    /// Create a new `MockNotifier`.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Return only `Story` calls.
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

    /// Return only `RunSummary` calls.
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
    pub story_key: String,
}

/// Standalone mock for `SessionRunner`.
///
/// Not trait-based (the real `SessionRunner` is a concrete struct).
/// Returns a configurable `SessionOutcome`.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<Option<SessionOutcome>>>,
    wal_recovery: Arc<Mutex<Option<()>>>,
    run_calls: Arc<Mutex<Vec<SessionRunCall>>>,
    wal_calls: Arc<Mutex<Vec<()>>>,
}

impl MockSessionRunner {
    /// Create a new `MockSessionRunner` with a configured outcome.
    pub fn new(outcome: SessionOutcome) -> Self {
        Self {
            outcome: Arc::new(Mutex::new(Some(outcome))),
            wal_recovery: Arc::new(Mutex::new(None)),
            run_calls: Arc::new(Mutex::new(Vec::new())),
            wal_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Simulate running a session for a story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.run_calls.lock().unwrap().push(SessionRunCall {
            story_key: story.story_key.clone(),
        });
        self.outcome
            .lock()
            .unwrap()
            .take()
            .expect("MockSessionRunner::run called but no outcome configured")
    }

    /// Simulate WAL recovery check — always returns `None`.
    pub async fn check_and_recover_wal(&self) -> Option<()> {
        self.wal_calls.lock().unwrap().push(());
        self.wal_recovery.lock().unwrap().take()
    }

    /// Return all `run` calls.
    pub fn run_calls(&self) -> Vec<SessionRunCall> {
        self.run_calls.lock().unwrap().clone()
    }

    /// Return the number of `check_and_recover_wal` calls.
    pub fn wal_call_count(&self) -> usize {
        self.wal_calls.lock().unwrap().len()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Captured call to `MockReviewRunner::run`.
#[derive(Debug, Clone)]
pub struct ReviewRunCall {
    pub story_key: String,
}

/// Standalone mock for `ReviewRunner`.
///
/// Not trait-based (the real `ReviewRunner` is a concrete struct).
/// Returns a configurable `ReviewOutcome`.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<Option<ReviewOutcome>>>,
    run_calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create a new `MockReviewRunner` with a configured outcome.
    pub fn new(outcome: ReviewOutcome) -> Self {
        Self {
            outcome: Arc::new(Mutex::new(Some(outcome))),
            run_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Simulate running a code review for a story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.run_calls.lock().unwrap().push(ReviewRunCall {
            story_key: story.story_key.clone(),
        });
        self.outcome
            .lock()
            .unwrap()
            .take()
            .expect("MockReviewRunner::run called but no outcome configured")
    }

    /// Return all `run` calls.
    pub fn run_calls(&self) -> Vec<ReviewRunCall> {
        self.run_calls.lock().unwrap().clone()
    }
}
