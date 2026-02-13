//! Mock implementations for integration tests.
//!
//! Provides configurable mocks for `GitProvider`, `Notifier`, `SessionRunner`,
//! and `ReviewRunner`. All mocks are `Send + Sync` via `Arc<Mutex<...>>`.

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

/// Captured call to a `MockGitProvider` method.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    /// A `create_pr` call with its parameters.
    CreatePr(CreatePrParams),
    /// An `add_comment` call with PR ID and body.
    AddComment { pr_id: String, body: String },
    /// A `get_pr_url` call with PR ID.
    GetPrUrl { pr_id: String },
}

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Uses builder pattern for configuring return values. Tracks all calls
/// for assertion.
#[derive(Clone)]
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, String>>>,
    add_comment_result: Arc<Mutex<Result<(), String>>>,
    get_pr_url_result: Arc<Mutex<Result<String, String>>>,
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
    pub fn with_create_pr(self, result: Result<PrInfo, String>) -> Self {
        *self.create_pr_result.lock().expect("lock") = result;
        self
    }

    /// Configure the return value for `add_comment`.
    pub fn with_add_comment(self, result: Result<(), String>) -> Self {
        *self.add_comment_result.lock().expect("lock") = result;
        self
    }

    /// Configure the return value for `get_pr_url`.
    pub fn with_get_pr_url(self, result: Result<String, String>) -> Self {
        *self.get_pr_url_result.lock().expect("lock") = result;
        self
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<GitProviderCall> {
        self.calls.lock().expect("lock").clone()
    }
}

#[async_trait]
impl GitProvider for MockGitProvider {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError> {
        self.calls
            .lock()
            .expect("lock")
            .push(GitProviderCall::CreatePr(params));
        let result = self.create_pr_result.lock().expect("lock").clone();
        result.map_err(|msg| GitProviderError::ApiError {
            status: 500,
            message: msg,
        })
    }

    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        self.calls
            .lock()
            .expect("lock")
            .push(GitProviderCall::AddComment {
                pr_id: pr_id.to_string(),
                body: body.to_string(),
            });
        let result = self.add_comment_result.lock().expect("lock").clone();
        result.map_err(|msg| GitProviderError::ApiError {
            status: 500,
            message: msg,
        })
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .expect("lock")
            .push(GitProviderCall::GetPrUrl {
                pr_id: pr_id.to_string(),
            });
        let result = self.get_pr_url_result.lock().expect("lock").clone();
        result.map_err(|msg| GitProviderError::ApiError {
            status: 500,
            message: msg,
        })
    }
}

// ---------------------------------------------------------------------------
// MockNotifier
// ---------------------------------------------------------------------------

/// Captured call to a `MockNotifier` method.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// A `notify_story` call with its notification payload.
    Story(StoryNotification),
    /// A `notify_run_summary` call with its summary payload.
    RunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] for integration tests.
///
/// Captures every call into a `Vec` for later assertion.
#[derive(Clone)]
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
}

impl MockNotifier {
    /// Create a new `MockNotifier` with an empty call log.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().expect("lock").clone()
    }

    /// Get only the `notify_story` calls.
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

    /// Get only the `notify_run_summary` calls.
    pub fn summary_calls(&self) -> Vec<RunSummary> {
        self.calls
            .lock()
            .expect("lock")
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
            .expect("lock")
            .push(NotifierCall::Story(notification.clone()));
        Ok(())
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .expect("lock")
            .push(NotifierCall::RunSummary(summary.clone()));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Mock session runner for integration tests.
///
/// Returns a configurable [`SessionOutcome`] and tracks calls.
/// Does NOT implement a trait — mirrors the real `SessionRunner` public API surface.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<Option<SessionOutcome>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl MockSessionRunner {
    /// Create a new `MockSessionRunner` that returns the given outcome.
    pub fn new(outcome: SessionOutcome) -> Self {
        Self {
            outcome: Arc::new(Mutex::new(Some(outcome))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Run a mock session for the given story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls
            .lock()
            .expect("lock")
            .push(story.story_key.clone());
        // Take the configured outcome (returns default Failed on subsequent calls)
        self.outcome
            .lock()
            .expect("lock")
            .take()
            .unwrap_or_else(|| SessionOutcome::Failed {
                story_key: story.story_key.clone(),
                error: "MockSessionRunner: no outcome configured".into(),
                decisions: Vec::new(),
            })
    }

    /// Check and recover WAL — always returns `None` for mock.
    pub async fn check_and_recover_wal(&self) -> Option<()> {
        None
    }

    /// Get all story keys that `run` was called with.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("lock").clone()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Mock review runner for integration tests.
///
/// Returns a configurable [`ReviewOutcome`] and tracks calls.
/// Does NOT implement a trait — mirrors the real `ReviewRunner` public API surface.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<Option<ReviewOutcome>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl MockReviewRunner {
    /// Create a new `MockReviewRunner` that returns the given outcome.
    pub fn new(outcome: ReviewOutcome) -> Self {
        Self {
            outcome: Arc::new(Mutex::new(Some(outcome))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Run a mock review for the given story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls
            .lock()
            .expect("lock")
            .push(story.story_key.clone());
        self.outcome
            .lock()
            .expect("lock")
            .take()
            .unwrap_or_else(|| ReviewOutcome::Failed {
                story_key: story.story_key.clone(),
                error: "MockReviewRunner: no outcome configured".into(),
            })
    }

    /// Get all story keys that `run` was called with.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().expect("lock").clone()
    }
}
