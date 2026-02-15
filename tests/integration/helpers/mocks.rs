//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.

use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::runner::RecoveryInfo;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Captured call record for `MockGitProvider`.
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
pub struct MockGitProvider {
    create_pr_results: Arc<Mutex<VecDeque<Result<PrInfo, GitProviderError>>>>,
    add_comment_results: Arc<Mutex<VecDeque<Result<(), GitProviderError>>>>,
    get_pr_url_results: Arc<Mutex<VecDeque<Result<String, GitProviderError>>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new mock with no configured return values.
    /// Each method will panic if called without a configured result.
    pub fn new() -> Self {
        Self {
            create_pr_results: Arc::new(Mutex::new(VecDeque::new())),
            add_comment_results: Arc::new(Mutex::new(VecDeque::new())),
            get_pr_url_results: Arc::new(Mutex::new(VecDeque::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the return value for `create_pr`.
    pub fn with_create_pr(self, result: Result<PrInfo, GitProviderError>) -> Self {
        self.create_pr_results.lock().unwrap().push_back(result);
        self
    }

    /// Configure the return value for `add_comment`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        self.add_comment_results.lock().unwrap().push_back(result);
        self
    }

    /// Configure the return value for `get_pr_url`.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        self.get_pr_url_results.lock().unwrap().push_back(result);
        self
    }

    /// Get all recorded calls.
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
        self.create_pr_results
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockGitProvider::create_pr called without configured result")
    }

    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::AddComment {
                pr_id: pr_id.to_string(),
                body: body.to_string(),
            });
        self.add_comment_results
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockGitProvider::add_comment called without configured result")
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::GetPrUrl {
                pr_id: pr_id.to_string(),
            });
        self.get_pr_url_results
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockGitProvider::get_pr_url called without configured result")
    }
}

// ---------------------------------------------------------------------------
// MockNotifier
// ---------------------------------------------------------------------------

/// Captured notification call for `MockNotifier`.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    NotifyStory(StoryNotification),
    NotifyRunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] that captures all calls for assertion.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
}

impl MockNotifier {
    /// Create a new mock notifier.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get all captured calls.
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
                NotifierCall::NotifyStory(n) => Some(n.clone()),
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
                NotifierCall::NotifyRunSummary(s) => Some(s.clone()),
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
            .push(NotifierCall::NotifyStory(notification.clone()));
        Ok(())
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::NotifyRunSummary(summary.clone()));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Captured session runner call.
#[derive(Debug, Clone)]
pub struct SessionRunnerCall {
    pub story_key: String,
}

/// Mock session runner that returns configurable `SessionOutcome`.
///
/// Note: This is a standalone mock struct — `SessionRunner` in the codebase
/// is a concrete struct, not a trait. Story 7.4 will address injection.
pub struct MockSessionRunner {
    outcomes: Arc<Mutex<VecDeque<SessionOutcome>>>,
    calls: Arc<Mutex<Vec<SessionRunnerCall>>>,
}

impl MockSessionRunner {
    /// Create a new mock with a configured outcome.
    pub fn new(outcome: SessionOutcome) -> Self {
        Self {
            outcomes: Arc::new(Mutex::new(VecDeque::from([outcome]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Run the mock session for a story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunnerCall {
            story_key: story.story_key.clone(),
        });
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockSessionRunner::run called without configured outcome")
    }

    /// Check and recover WAL — always returns None for mock.
    pub async fn check_and_recover_wal(&self) -> Option<RecoveryInfo> {
        None
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<SessionRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Captured review runner call.
#[derive(Debug, Clone)]
pub struct ReviewRunnerCall {
    pub story_key: String,
}

/// Mock review runner that returns configurable `ReviewOutcome`.
///
/// Note: This is a standalone mock struct — `ReviewRunner` in the codebase
/// is a concrete struct, not a trait. Story 7.4 will address injection.
pub struct MockReviewRunner {
    outcomes: Arc<Mutex<VecDeque<ReviewOutcome>>>,
    calls: Arc<Mutex<Vec<ReviewRunnerCall>>>,
}

impl MockReviewRunner {
    /// Create a new mock with a configured outcome.
    pub fn new(outcome: ReviewOutcome) -> Self {
        Self {
            outcomes: Arc::new(Mutex::new(VecDeque::from([outcome]))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Run the mock review for a story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunnerCall {
            story_key: story.story_key.clone(),
        });
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockReviewRunner::run called without configured outcome")
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}
