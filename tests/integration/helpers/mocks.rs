//! Mock implementations for integration tests.
//!
//! Provides configurable mock structs for `GitProvider`, `Notifier`,
//! `SessionRunner`, and `ReviewRunner`. All mocks are `Send + Sync`.

use async_trait::async_trait;
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

/// Recorded call to a `MockGitProvider` method.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    /// `create_pr` was called with these params.
    CreatePr(CreatePrParams),
    /// `add_comment` was called with (pr_id, body).
    AddComment { pr_id: String, body: String },
    /// `get_pr_url` was called with pr_id.
    GetPrUrl(String),
}

/// Configurable mock for the `GitProvider` trait.
///
/// Uses `Arc<Mutex<...>>` for interior mutability to satisfy `Send + Sync`.
/// Builder pattern: `MockGitProvider::new().with_create_pr(Ok(...))`.
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new mock with default Ok responses.
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
        *self.create_pr_result.lock().expect("lock poisoned") = result;
        self
    }

    /// Configure the return value for `add_comment`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        *self.add_comment_result.lock().expect("lock poisoned") = result;
        self
    }

    /// Configure the return value for `get_pr_url`.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        *self.get_pr_url_result.lock().expect("lock poisoned") = result;
        self
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<GitProviderCall> {
        self.calls.lock().expect("lock poisoned").clone()
    }
}

#[async_trait]
impl GitProvider for MockGitProvider {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError> {
        self.calls
            .lock()
            .expect("lock poisoned")
            .push(GitProviderCall::CreatePr(params));
        let guard = self.create_pr_result.lock().expect("lock poisoned");
        match &*guard {
            Ok(info) => Ok(info.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        self.calls
            .lock()
            .expect("lock poisoned")
            .push(GitProviderCall::AddComment {
                pr_id: pr_id.to_string(),
                body: body.to_string(),
            });
        let guard = self.add_comment_result.lock().expect("lock poisoned");
        match &*guard {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .expect("lock poisoned")
            .push(GitProviderCall::GetPrUrl(pr_id.to_string()));
        let guard = self.get_pr_url_result.lock().expect("lock poisoned");
        match &*guard {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Clone a `GitProviderError` by reconstructing each variant.
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

/// Recorded call to a `MockNotifier` method.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// `notify_story` was called.
    Story(StoryNotification),
    /// `notify_run_summary` was called.
    Summary(RunSummary),
}

/// Configurable mock for the `Notifier` trait.
///
/// Captures all calls into a `Vec` for later assertion. Always returns `Ok(())`.
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

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().expect("lock poisoned").clone()
    }

    /// Get only the story notification calls.
    pub fn story_calls(&self) -> Vec<StoryNotification> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                NotifierCall::Story(n) => Some(n),
                _ => None,
            })
            .collect()
    }

    /// Get only the run summary calls.
    pub fn summary_calls(&self) -> Vec<RunSummary> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                NotifierCall::Summary(s) => Some(s),
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
            .expect("lock poisoned")
            .push(NotifierCall::Story(notification.clone()));
        Ok(())
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .expect("lock poisoned")
            .push(NotifierCall::Summary(summary.clone()));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Recorded call to `MockSessionRunner::run`.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    /// The story key that was passed.
    pub story_key: String,
}

/// Standalone mock for session runner.
///
/// Returns a configurable `SessionOutcome`. Does NOT implement a shared trait
/// with the real `SessionRunner` (codebase doesn't define one).
pub struct MockSessionRunner {
    outcome_factory: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> SessionOutcome + Send>>>,
    run_calls: Arc<Mutex<Vec<SessionRunCall>>>,
    wal_recovery: Arc<Mutex<Option<RecoveryInfo>>>,
}

impl MockSessionRunner {
    /// Create a mock that returns `Completed` for every story.
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
            wal_recovery: Arc::new(Mutex::new(None)),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome<F>(self, factory: F) -> Self
    where
        F: Fn(&StoryInfo) -> SessionOutcome + Send + 'static,
    {
        *self.outcome_factory.lock().expect("lock poisoned") = Box::new(factory);
        self
    }

    /// Run the mock session for a story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.run_calls
            .lock()
            .expect("lock poisoned")
            .push(SessionRunCall {
                story_key: story.story_key.clone(),
            });
        let factory = self.outcome_factory.lock().expect("lock poisoned");
        factory(story)
    }

    /// Check for WAL recovery (always returns None in mock).
    pub async fn check_and_recover_wal(&self) -> Option<RecoveryInfo> {
        self.wal_recovery
            .lock()
            .expect("lock poisoned")
            .take()
    }

    /// Get all recorded run calls.
    pub fn run_calls(&self) -> Vec<SessionRunCall> {
        self.run_calls.lock().expect("lock poisoned").clone()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Recorded call to `MockReviewRunner::run`.
#[derive(Debug, Clone)]
pub struct ReviewRunCall {
    /// The story key that was passed.
    pub story_key: String,
}

/// Standalone mock for review runner.
///
/// Returns a configurable `ReviewOutcome`. Does NOT implement a shared trait
/// with the real `ReviewRunner`.
pub struct MockReviewRunner {
    outcome_factory: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> ReviewOutcome + Send>>>,
    run_calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create a mock that returns `Completed` for every story.
    pub fn new() -> Self {
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(|story| ReviewOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                report: "Mock review passed.".to_string(),
            }))),
            run_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome<F>(self, factory: F) -> Self
    where
        F: Fn(&StoryInfo) -> ReviewOutcome + Send + 'static,
    {
        *self.outcome_factory.lock().expect("lock poisoned") = Box::new(factory);
        self
    }

    /// Run the mock review for a story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.run_calls
            .lock()
            .expect("lock poisoned")
            .push(ReviewRunCall {
                story_key: story.story_key.clone(),
            });
        let factory = self.outcome_factory.lock().expect("lock poisoned");
        factory(story)
    }

    /// Get all recorded run calls.
    pub fn run_calls(&self) -> Vec<ReviewRunCall> {
        self.run_calls.lock().expect("lock poisoned").clone()
    }
}
