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
// MockGitProvider
// ---------------------------------------------------------------------------

/// Captured call to a MockGitProvider method.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    CreatePr(CreatePrParams),
    AddComment {
        pr_id: String,
        body: String,
    },
    GetPrUrl(String),
}

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Configure return values with the builder methods (`with_create_pr`, etc.)
/// and inspect captured calls via [`calls()`](Self::calls).
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new mock with default `Ok` responses.
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
        *self.create_pr_result.lock().unwrap() = result;
        self
    }

    /// Configure the return value for `add_comment`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        *self.add_comment_result.lock().unwrap() = result;
        self
    }

    /// Configure the return value for `get_pr_url`.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        *self.get_pr_url_result.lock().unwrap() = result;
        self
    }

    /// Return all captured calls.
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
            .push(GitProviderCall::GetPrUrl(pr_id.to_string()));
        let guard = self.get_pr_url_result.lock().unwrap();
        match &*guard {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Clone a `GitProviderError` — reconstruct since `Clone` is not derived.
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

/// Captured notification call.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    Story(StoryNotification),
    RunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] that captures all calls for assertions.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
}

impl MockNotifier {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Return all captured calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Return only story notification calls.
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

    /// Return only run summary calls.
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
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Captured call to MockSessionRunner.
#[derive(Debug, Clone)]
pub enum SessionRunnerCall {
    Run(String), // story_key
    CheckAndRecoverWal,
}

/// Mock session runner — standalone struct that returns configurable `SessionOutcome`.
///
/// Does NOT implement a trait (the real `SessionRunner` is a concrete struct).
pub struct MockSessionRunner {
    outcome: Arc<Mutex<Option<SessionOutcome>>>,
    calls: Arc<Mutex<Vec<SessionRunnerCall>>>,
}

impl MockSessionRunner {
    /// Create a new mock that returns `SessionOutcome::Completed` by default.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome to return from `run`.
    pub fn with_outcome(self, outcome: SessionOutcome) -> Self {
        *self.outcome.lock().unwrap() = Some(outcome);
        self
    }

    /// Run a session for the given story, returning the configured outcome.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(SessionRunnerCall::Run(story.story_key.clone()));
        let guard = self.outcome.lock().unwrap();
        match &*guard {
            Some(SessionOutcome::Completed {
                story_key,
                branch,
                decisions,
                pr_context,
                pr_how_to_test,
                pr_additional_info,
            }) => SessionOutcome::Completed {
                story_key: story_key.clone(),
                branch: branch.clone(),
                decisions: decisions.clone(),
                pr_context: pr_context.clone(),
                pr_how_to_test: pr_how_to_test.clone(),
                pr_additional_info: pr_additional_info.clone(),
            },
            Some(SessionOutcome::Escalated { report, decisions }) => {
                SessionOutcome::Escalated {
                    report: report.clone(),
                    decisions: decisions.clone(),
                }
            }
            Some(SessionOutcome::Failed {
                story_key,
                error,
                decisions,
            }) => SessionOutcome::Failed {
                story_key: story_key.clone(),
                error: error.clone(),
                decisions: decisions.clone(),
            },
            None => SessionOutcome::Completed {
                story_key: story.story_key.clone(),
                branch: story.branch_name.clone(),
                decisions: vec![],
                pr_context: None,
                pr_how_to_test: None,
                pr_additional_info: None,
            },
        }
    }

    /// Check for WAL recovery — always returns None in mock.
    pub async fn check_and_recover_wal(&self) -> Option<()> {
        self.calls
            .lock()
            .unwrap()
            .push(SessionRunnerCall::CheckAndRecoverWal);
        None
    }

    /// Return all captured calls.
    pub fn calls(&self) -> Vec<SessionRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Captured call to MockReviewRunner.
#[derive(Debug, Clone)]
pub enum ReviewRunnerCall {
    Run(String), // story_key
}

/// Mock review runner — standalone struct that returns configurable `ReviewOutcome`.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<Option<ReviewOutcome>>>,
    calls: Arc<Mutex<Vec<ReviewRunnerCall>>>,
}

impl MockReviewRunner {
    /// Create a new mock that returns `ReviewOutcome::Skipped` by default.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(None)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome to return from `run`.
    pub fn with_outcome(self, outcome: ReviewOutcome) -> Self {
        *self.outcome.lock().unwrap() = Some(outcome);
        self
    }

    /// Run a review for the given story, returning the configured outcome.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls
            .lock()
            .unwrap()
            .push(ReviewRunnerCall::Run(story.story_key.clone()));
        let guard = self.outcome.lock().unwrap();
        match &*guard {
            Some(ReviewOutcome::Completed {
                story_key,
                branch,
                report,
            }) => ReviewOutcome::Completed {
                story_key: story_key.clone(),
                branch: branch.clone(),
                report: report.clone(),
            },
            Some(ReviewOutcome::Failed { story_key, error }) => ReviewOutcome::Failed {
                story_key: story_key.clone(),
                error: error.clone(),
            },
            Some(ReviewOutcome::Skipped { reason }) => ReviewOutcome::Skipped {
                reason: reason.clone(),
            },
            None => ReviewOutcome::Skipped {
                reason: "mock: no outcome configured".into(),
            },
        }
    }

    /// Return all captured calls.
    pub fn calls(&self) -> Vec<ReviewRunnerCall> {
        self.calls.lock().unwrap().clone()
    }
}
