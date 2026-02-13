//! Mock implementations of core traits for integration tests.
//!
//! Each mock is `Send + Sync` and uses `Arc<Mutex<...>>` for interior mutability.
//! Builder pattern: `MockX::new().with_method(return_value)`.

use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// Result factory type for mocks
// ---------------------------------------------------------------------------

/// A thread-safe factory that produces a result on each call.
/// Stores a `Vec` of results; pops the first one each time, returning a default when empty.
type ResultFactory<T, E> = Arc<Mutex<Vec<Result<T, E>>>>;

fn take_or_default<T, E>(factory: &ResultFactory<T, E>, default: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    let mut guard = factory.lock().unwrap();
    if guard.is_empty() {
        default()
    } else {
        guard.remove(0)
    }
}

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Recorded call to `MockGitProvider` for assertion.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    CreatePr(CreatePrParams),
    AddComment { pr_id: String, body: String },
    GetPrUrl(String),
}

/// Mock implementation of [`GitProvider`].
///
/// Configure return values via builder methods. Tracks every call for later assertion.
pub struct MockGitProvider {
    create_pr_results: ResultFactory<PrInfo, GitProviderError>,
    add_comment_results: ResultFactory<(), GitProviderError>,
    get_pr_url_results: ResultFactory<String, GitProviderError>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    pub fn new() -> Self {
        Self {
            create_pr_results: Arc::new(Mutex::new(Vec::new())),
            add_comment_results: Arc::new(Mutex::new(Vec::new())),
            get_pr_url_results: Arc::new(Mutex::new(Vec::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the return value for the next `create_pr` call.
    pub fn with_create_pr(self, result: Result<PrInfo, GitProviderError>) -> Self {
        self.create_pr_results.lock().unwrap().push(result);
        self
    }

    /// Configure the return value for the next `add_comment` call.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        self.add_comment_results.lock().unwrap().push(result);
        self
    }

    /// Configure the return value for the next `get_pr_url` call.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        self.get_pr_url_results.lock().unwrap().push(result);
        self
    }

    /// Return a clone of all recorded calls.
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
        take_or_default(&self.create_pr_results, || {
            Ok(PrInfo {
                id: "mock-1".into(),
                url: "https://mock/pr/1".into(),
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
        take_or_default(&self.add_comment_results, || Ok(()))
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::GetPrUrl(pr_id.to_string()));
        let pr_id_owned = pr_id.to_string();
        take_or_default(&self.get_pr_url_results, move || {
            Ok(format!("https://mock/pr/{pr_id_owned}"))
        })
    }
}

// ---------------------------------------------------------------------------
// MockNotifier
// ---------------------------------------------------------------------------

/// Recorded call to `MockNotifier` for assertion.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    Story(StoryNotification),
    RunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`].
///
/// Captures every call into a `Vec` so tests can assert on content.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
}

impl MockNotifier {
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

/// Recorded call to `MockSessionRunner::run`.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    pub story_key: String,
}

/// Mock for the `SessionRunner` struct.
///
/// Not trait-based — mirrors the public API surface of the real `SessionRunner`.
pub struct MockSessionRunner {
    outcomes: Arc<Mutex<Vec<SessionOutcome>>>,
    calls: Arc<Mutex<Vec<SessionRunCall>>>,
}

impl MockSessionRunner {
    pub fn new() -> Self {
        Self {
            outcomes: Arc::new(Mutex::new(Vec::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome that the next `run` call will return.
    pub fn with_outcome(self, outcome: SessionOutcome) -> Self {
        self.outcomes.lock().unwrap().push(outcome);
        self
    }

    /// Run the mock session, returning the configured outcome.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunCall {
            story_key: story.story_key.clone(),
        });
        let mut guard = self.outcomes.lock().unwrap();
        if guard.is_empty() {
            SessionOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                decisions: vec![],
                pr_context: None,
                pr_how_to_test: None,
                pr_additional_info: None,
            }
        } else {
            guard.remove(0)
        }
    }

    /// Check and recover from WAL — always returns `None` for mock.
    pub async fn check_and_recover_wal(
        &self,
    ) -> Option<bmad_bot::session::runner::RecoveryInfo> {
        None
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<SessionRunCall> {
        self.calls.lock().unwrap().clone()
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

/// Mock for the `ReviewRunner` struct.
///
/// Not trait-based — mirrors the public API surface of the real `ReviewRunner`.
pub struct MockReviewRunner {
    outcomes: Arc<Mutex<Vec<ReviewOutcome>>>,
    calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    pub fn new() -> Self {
        Self {
            outcomes: Arc::new(Mutex::new(Vec::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome that the next `run` call will return.
    pub fn with_outcome(self, outcome: ReviewOutcome) -> Self {
        self.outcomes.lock().unwrap().push(outcome);
        self
    }

    /// Run the mock review, returning the configured outcome.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunCall {
            story_key: story.story_key.clone(),
        });
        let mut guard = self.outcomes.lock().unwrap();
        if guard.is_empty() {
            ReviewOutcome::Skipped {
                reason: "mock default".into(),
            }
        } else {
            guard.remove(0)
        }
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunCall> {
        self.calls.lock().unwrap().clone()
    }
}
