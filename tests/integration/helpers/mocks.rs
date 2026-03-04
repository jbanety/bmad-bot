//! Mock implementations for integration tests.
//!
//! Provides configurable test doubles for `GitProvider`, `Notifier`,
//! and standalone mocks for `SessionRunner` / `ReviewRunner`.
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::runner::RecoveryInfo;
use bmad_bot::session::SessionOutcome;
use bmad_bot::supervisor::decisions::DecisionRecord;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Recorded call to a `GitProvider` method.
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

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Configurable via builder methods (`with_create_pr`, `with_add_comment`, `with_get_pr_url`).
/// Every call is recorded for later assertion.
#[derive(Clone)]
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new mock with default success responses.
    pub fn new() -> Self {
        Self {
            create_pr_result: Arc::new(Mutex::new(Ok(PrInfo {
                id: "1".into(),
                url: "https://github.com/test/test/pull/1".into(),
                number: 1,
            }))),
            add_comment_result: Arc::new(Mutex::new(Ok(()))),
            get_pr_url_result: Arc::new(Mutex::new(Ok(
                "https://github.com/test/test/pull/1".into()
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

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<GitProviderCall> {
        self.calls.lock().unwrap().clone()
    }
}

/// Helper: clone a `Result<PrInfo, GitProviderError>` by re-creating error variants.
fn clone_pr_result(r: &Result<PrInfo, GitProviderError>) -> Result<PrInfo, GitProviderError> {
    match r {
        Ok(pr) => Ok(pr.clone()),
        Err(e) => Err(clone_git_provider_error(e)),
    }
}

fn clone_comment_result(r: &Result<(), GitProviderError>) -> Result<(), GitProviderError> {
    match r {
        Ok(()) => Ok(()),
        Err(e) => Err(clone_git_provider_error(e)),
    }
}

fn clone_url_result(r: &Result<String, GitProviderError>) -> Result<String, GitProviderError> {
    match r {
        Ok(s) => Ok(s.clone()),
        Err(e) => Err(clone_git_provider_error(e)),
    }
}

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

#[async_trait]
impl GitProvider for MockGitProvider {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::CreatePr(params));
        let guard = self.create_pr_result.lock().unwrap();
        clone_pr_result(&guard)
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
        clone_comment_result(&guard)
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::GetPrUrl {
                pr_id: pr_id.to_string(),
            });
        let guard = self.get_pr_url_result.lock().unwrap();
        clone_url_result(&guard)
    }
}

// ---------------------------------------------------------------------------
// MockNotifier
// ---------------------------------------------------------------------------

/// Recorded call to a `Notifier` method.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    NotifyStory(StoryNotification),
    NotifyRunSummary(RunSummary),
}

/// Mock implementation of [`Notifier`] that captures all calls for assertion.
#[derive(Clone)]
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

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Return only `NotifyStory` calls.
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

    /// Return only `NotifyRunSummary` calls.
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

/// Recorded call to `MockSessionRunner::run`.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    pub story_key: String,
}

/// Mock session runner returning configurable [`SessionOutcome`].
///
/// Does NOT implement a shared trait with the real `SessionRunner` — the codebase
/// doesn't define one. This mock mirrors the public API surface for test injection.
pub struct MockSessionRunner {
    outcome_factory: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> SessionOutcome + Send + Sync>>>,
    calls: Arc<Mutex<Vec<SessionRunCall>>>,
}

impl MockSessionRunner {
    /// Create a mock that always returns `SessionOutcome::Completed` with empty decisions.
    pub fn new_completed() -> Self {
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(|story: &StoryInfo| {
                SessionOutcome::Completed {
                    story_key: story.story_key.clone(),
                    branch: story.branch_name.clone(),
                    decisions: vec![],
                    pr_context: None,
                    pr_how_to_test: None,
                    pr_additional_info: None,
                }
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a mock that always returns `SessionOutcome::Failed`.
    pub fn new_failed(error_msg: &str) -> Self {
        let error = error_msg.to_string();
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(move |story: &StoryInfo| {
                SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: error.clone(),
                    decisions: vec![],
                }
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a mock with a custom outcome factory.
    pub fn with_outcome<F>(f: F) -> Self
    where
        F: Fn(&StoryInfo) -> SessionOutcome + Send + Sync + 'static,
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

    /// Check for WAL recovery — always returns None (mock has no WAL).
    pub async fn check_and_recover_wal(&self) -> Option<RecoveryInfo> {
        None
    }

    /// Return all recorded run calls.
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

/// Mock review runner returning configurable [`ReviewOutcome`].
pub struct MockReviewRunner {
    outcome_factory: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> ReviewOutcome + Send + Sync>>>,
    calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create a mock that always returns `ReviewOutcome::Completed`.
    pub fn new_completed() -> Self {
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(|story: &StoryInfo| {
                ReviewOutcome::Completed {
                    story_key: story.story_key.clone(),
                    branch: story.branch_name.clone(),
                    report: "Mock review report — all looks good.".to_string(),
                }
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a mock that always returns `ReviewOutcome::Failed`.
    pub fn new_failed(error_msg: &str) -> Self {
        let error = error_msg.to_string();
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(move |story: &StoryInfo| {
                ReviewOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: error.clone(),
                }
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a mock that always returns `ReviewOutcome::Skipped`.
    pub fn new_skipped(reason: &str) -> Self {
        let reason = reason.to_string();
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(move |_story: &StoryInfo| {
                ReviewOutcome::Skipped {
                    reason: reason.clone(),
                }
            }))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Create a mock with a custom outcome factory.
    pub fn with_outcome<F>(f: F) -> Self
    where
        F: Fn(&StoryInfo) -> ReviewOutcome + Send + Sync + 'static,
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

    /// Return all recorded run calls.
    pub fn calls(&self) -> Vec<ReviewRunCall> {
        self.calls.lock().unwrap().clone()
    }
}
