//! Mock implementations for integration tests.
//!
//! All mocks use `Arc<Mutex<...>>` for interior mutability (async-safe, `Send + Sync`).

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

/// Captured call to `MockGitProvider` for test assertions.
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

/// Mock implementation of [`GitProvider`] with configurable return values
/// and call tracking.
///
/// # Builder pattern
/// ```ignore
/// MockGitProvider::new()
///     .with_create_pr(Ok(PrInfo { id: "1".into(), url: "https://...".into(), number: 1 }))
///     .with_add_comment(Ok(()))
/// ```
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new mock with default Ok results.
    pub fn new() -> Self {
        Self {
            create_pr_result: Arc::new(Mutex::new(Ok(PrInfo {
                id: "mock-1".into(),
                url: "https://mock-url.example.com/pr/1".into(),
                number: 1,
            }))),
            add_comment_result: Arc::new(Mutex::new(Ok(()))),
            get_pr_url_result: Arc::new(Mutex::new(Ok(
                "https://mock-url.example.com/pr/1".into(),
            ))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Set the result for `create_pr`.
    pub fn with_create_pr(self, result: Result<PrInfo, GitProviderError>) -> Self {
        *self.create_pr_result.lock().unwrap() = result;
        self
    }

    /// Set the result for `add_comment`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        *self.add_comment_result.lock().unwrap() = result;
        self
    }

    /// Set the result for `get_pr_url`.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        *self.get_pr_url_result.lock().unwrap() = result;
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
        let guard = self.create_pr_result.lock().unwrap();
        match &*guard {
            Ok(info) => Ok(info.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
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
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::GetPrUrl {
                pr_id: pr_id.to_string(),
            });
        let guard = self.get_pr_url_result.lock().unwrap();
        match &*guard {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Clone a `GitProviderError` (the enum doesn't derive Clone).
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

/// Captured call to `MockNotifier` for test assertions.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    Story(StoryNotification),
    RunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] that captures all calls.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
}

impl MockNotifier {
    /// Create a new empty mock notifier.
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

/// Mock session runner — standalone struct mimicking `SessionRunner::run()`.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<SessionOutcome>>,
    run_calls: Arc<Mutex<Vec<String>>>,
    wal_recovery: Arc<Mutex<Option<()>>>,
}

impl MockSessionRunner {
    /// Create a new mock returning a default Completed outcome.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(SessionOutcome::Completed {
                story_key: "mock-story".into(),
                branch: "story/mock-story".into(),
                decisions: Vec::new(),
                pr_context: None,
                pr_how_to_test: None,
                pr_additional_info: None,
            })),
            run_calls: Arc::new(Mutex::new(Vec::new())),
            wal_recovery: Arc::new(Mutex::new(None)),
        }
    }

    /// Set the outcome returned by `run()`.
    pub fn with_outcome(self, outcome: SessionOutcome) -> Self {
        *self.outcome.lock().unwrap() = outcome;
        self
    }

    /// Run a mock session for the given story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.run_calls
            .lock()
            .unwrap()
            .push(story.story_key.clone());
        let guard = self.outcome.lock().unwrap();
        match &*guard {
            SessionOutcome::Completed {
                story_key,
                branch,
                decisions,
                pr_context,
                pr_how_to_test,
                pr_additional_info,
            } => SessionOutcome::Completed {
                story_key: story_key.clone(),
                branch: branch.clone(),
                decisions: decisions.clone(),
                pr_context: pr_context.clone(),
                pr_how_to_test: pr_how_to_test.clone(),
                pr_additional_info: pr_additional_info.clone(),
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

    /// Check for WAL recovery (always returns None in mock).
    pub async fn check_and_recover_wal(&self) -> Option<()> {
        self.wal_recovery.lock().unwrap().clone()
    }

    /// Return story keys that `run()` was called with.
    pub fn run_calls(&self) -> Vec<String> {
        self.run_calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Mock review runner — standalone struct mimicking `ReviewRunner::run()`.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<ReviewOutcome>>,
    run_calls: Arc<Mutex<Vec<String>>>,
}

impl MockReviewRunner {
    /// Create a new mock returning a default Completed outcome.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(ReviewOutcome::Completed {
                story_key: "mock-story".into(),
                branch: "story/mock-story".into(),
                report: "Mock review report".into(),
            })),
            run_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Set the outcome returned by `run()`.
    pub fn with_outcome(self, outcome: ReviewOutcome) -> Self {
        *self.outcome.lock().unwrap() = outcome;
        self
    }

    /// Run a mock review for the given story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.run_calls
            .lock()
            .unwrap()
            .push(story.story_key.clone());
        let guard = self.outcome.lock().unwrap();
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

    /// Return story keys that `run()` was called with.
    pub fn run_calls(&self) -> Vec<String> {
        self.run_calls.lock().unwrap().clone()
    }
}
