//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
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
pub struct MockGitProvider {
    create_pr_factory: Arc<Mutex<GitProviderFactory<PrInfo>>>,
    add_comment_factory: Arc<Mutex<GitProviderFactory<()>>>,
    get_pr_url_factory: Arc<Mutex<GitProviderFactory<String>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
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
/// Not a trait impl — mirrors the public API surface of the real `SessionRunner`.
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

    /// Check and recover WAL — always returns None (no crash recovery in mock).
    pub async fn check_and_recover_wal(&self) -> Option<()> {
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
/// Not a trait impl — mirrors the public API surface of the real `ReviewRunner`.
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
