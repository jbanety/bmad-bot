//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` compatible and use `Arc<Mutex<...>>` for
//! interior mutability in async-safe contexts.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::supervisor::decisions::DecisionRecord;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Recorded call to a `GitProvider` method.
#[derive(Debug, Clone)]
pub enum GitProviderCall {
    /// `create_pr` was called with these params.
    CreatePr(CreatePrParams),
    /// `add_comment` was called with `(pr_id, body)`.
    AddComment(String, String),
    /// `get_pr_url` was called with `pr_id`.
    GetPrUrl(String),
}

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Configurable return values via builder pattern. Tracks all calls for
/// assertion.
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl Default for MockGitProvider {
    fn default() -> Self {
        Self::new()
    }
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
            .push(GitProviderCall::CreatePr(params.clone()));
        let result = self.create_pr_result.lock().unwrap();
        match &*result {
            Ok(info) => Ok(info.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::AddComment(pr_id.into(), body.into()));
        let result = self.add_comment_result.lock().unwrap();
        match &*result {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        self.calls
            .lock()
            .unwrap()
            .push(GitProviderCall::GetPrUrl(pr_id.into()));
        let result = self.get_pr_url_result.lock().unwrap();
        match &*result {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Clone a `GitProviderError` (thiserror types don't implement Clone).
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

/// Recorded call to a `Notifier` method.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// `notify_story` was called.
    Story(StoryNotification),
    /// `notify_run_summary` was called.
    Summary(RunSummary),
}

/// Mock implementation of [`Notifier`] for integration tests.
///
/// Captures all calls into a `Vec` for later assertion.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
}

impl Default for MockNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl MockNotifier {
    /// Create a new empty `MockNotifier`.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get all recorded calls.
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.calls.lock().unwrap().clone()
    }

    /// Get only the story notification calls.
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

    /// Get only the run summary calls.
    pub fn summary_calls(&self) -> Vec<RunSummary> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                NotifierCall::Summary(s) => Some(s.clone()),
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
    /// The story key that was passed to `run`.
    pub story_key: String,
}

/// Mock session runner for integration tests.
///
/// Standalone struct that mimics `SessionRunner::run` and
/// `SessionRunner::check_and_recover_wal` signatures, returning configurable
/// outcomes.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<SessionOutcome>>,
    calls: Arc<Mutex<Vec<SessionRunCall>>>,
}

impl MockSessionRunner {
    /// Create a new mock that returns the given outcome.
    pub fn new(outcome: SessionOutcome) -> Self {
        Self {
            outcome: Arc::new(Mutex::new(outcome)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Run the mock session for a story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunCall {
            story_key: story.story_key.clone(),
        });
        let guard = self.outcome.lock().unwrap();
        clone_session_outcome(&guard)
    }

    /// Check for WAL recovery — always returns None.
    pub async fn check_and_recover_wal(&self) -> Option<()> {
        None
    }

    /// Get all recorded run calls.
    pub fn calls(&self) -> Vec<SessionRunCall> {
        self.calls.lock().unwrap().clone()
    }
}

/// Clone a `SessionOutcome` (it doesn't implement Clone).
fn clone_session_outcome(outcome: &SessionOutcome) -> SessionOutcome {
    match outcome {
        SessionOutcome::Completed {
            story_key,
            branch,
            decisions,
        } => SessionOutcome::Completed {
            story_key: story_key.clone(),
            branch: branch.clone(),
            decisions: clone_decisions(decisions),
        },
        SessionOutcome::Escalated { report, decisions } => SessionOutcome::Escalated {
            report: bmad_bot::session::escalation::EscalationReport {
                story_key: report.story_key.clone(),
                question: report.question.clone(),
                reason: report.reason.clone(),
                branch_name: report.branch_name.clone(),
                partial_work_summary: report.partial_work_summary.clone(),
                escalated_at: report.escalated_at.clone(),
            },
            decisions: clone_decisions(decisions),
        },
        SessionOutcome::Failed {
            story_key,
            error,
            decisions,
        } => SessionOutcome::Failed {
            story_key: story_key.clone(),
            error: error.clone(),
            decisions: clone_decisions(decisions),
        },
    }
}

/// Clone a slice of `DecisionRecord`.
fn clone_decisions(decisions: &[DecisionRecord]) -> Vec<DecisionRecord> {
    decisions
        .iter()
        .map(|d| DecisionRecord {
            question: d.question.clone(),
            context: d.context.clone(),
            answer: d.answer.clone(),
            reasoning: d.reasoning.clone(),
            source: d.source.clone(),
            alternatives: d.alternatives.clone(),
            timestamp: d.timestamp.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Recorded call to `MockReviewRunner::run`.
#[derive(Debug, Clone)]
pub struct ReviewRunCall {
    /// The story key that was passed to `run`.
    pub story_key: String,
}

/// Mock review runner for integration tests.
///
/// Standalone struct that mimics `ReviewRunner::run` signature, returning
/// configurable outcomes.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<ReviewOutcome>>,
    calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create a new mock that returns the given outcome.
    pub fn new(outcome: ReviewOutcome) -> Self {
        Self {
            outcome: Arc::new(Mutex::new(outcome)),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Run the mock review for a story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunCall {
            story_key: story.story_key.clone(),
        });
        let guard = self.outcome.lock().unwrap();
        clone_review_outcome(&guard)
    }

    /// Get all recorded run calls.
    pub fn calls(&self) -> Vec<ReviewRunCall> {
        self.calls.lock().unwrap().clone()
    }
}

/// Clone a `ReviewOutcome` (it doesn't implement Clone).
fn clone_review_outcome(outcome: &ReviewOutcome) -> ReviewOutcome {
    match outcome {
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
