//! Mock implementations for integration tests.
//!
//! Provides configurable mock structs that implement production traits
//! with call tracking and deterministic return values.

use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::supervisor::decisions::DecisionRecord;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Recorded call to a `MockGitProvider` method.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum GitProviderCall {
    /// `create_pr` was called with these params.
    CreatePr(CreatePrParams),
    /// `add_comment` was called with (pr_id, body).
    AddComment { pr_id: String, body: String },
    /// `get_pr_url` was called with pr_id.
    GetPrUrl(String),
}

/// Configurable mock for the [`GitProvider`] trait.
///
/// Use builder methods (`with_create_pr`, `with_add_comment`, `with_get_pr_url`)
/// to configure return values. Call tracking is automatic.
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
}

impl MockGitProvider {
    /// Create a new `MockGitProvider` with default OK responses.
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
            .push(GitProviderCall::GetPrUrl(pr_id.to_string()));
        let guard = self.get_pr_url_result.lock().unwrap();
        match &*guard {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Clone a `GitProviderError` (which doesn't derive Clone).
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

/// Recorded call to a `MockNotifier` method.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// `notify_story` was called.
    Story(StoryNotification),
    /// `notify_run_summary` was called.
    Summary(RunSummary),
}

/// Configurable mock for the [`Notifier`] trait.
///
/// Captures all calls for later assertion. Returns `Ok(())` by default.
pub struct MockNotifier {
    calls: Arc<Mutex<Vec<NotifierCall>>>,
    story_result: Arc<Mutex<Result<(), NotifierError>>>,
    summary_result: Arc<Mutex<Result<(), NotifierError>>>,
}

impl MockNotifier {
    /// Create a new `MockNotifier` that succeeds on all calls.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            story_result: Arc::new(Mutex::new(Ok(()))),
            summary_result: Arc::new(Mutex::new(Ok(()))),
        }
    }

    /// Configure `notify_story` to return an error.
    pub fn with_story_error(self, err: NotifierError) -> Self {
        *self.story_result.lock().unwrap() = Err(err);
        self
    }

    /// Configure `notify_run_summary` to return an error.
    #[allow(dead_code)]
    pub fn with_summary_error(self, err: NotifierError) -> Self {
        *self.summary_result.lock().unwrap() = Err(err);
        self
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
        let guard = self.story_result.lock().unwrap();
        match &*guard {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_notifier_error(e)),
        }
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::Summary(summary.clone()));
        let guard = self.summary_result.lock().unwrap();
        match &*guard {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_notifier_error(e)),
        }
    }
}

/// Clone a `NotifierError` (which doesn't derive Clone).
fn clone_notifier_error(e: &NotifierError) -> NotifierError {
    match e {
        NotifierError::HttpRequest { reason } => NotifierError::HttpRequest {
            reason: reason.clone(),
        },
        NotifierError::ApiError { status, body } => NotifierError::ApiError {
            status: *status,
            body: body.clone(),
        },
        NotifierError::ResponseParse { reason } => NotifierError::ResponseParse {
            reason: reason.clone(),
        },
        NotifierError::Disabled => NotifierError::Disabled,
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Recorded call to `MockSessionRunner::run`.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    /// The story key passed to `run`.
    pub story_key: String,
}

/// Standalone mock for `SessionRunner`.
///
/// Not trait-based — mirrors the public API surface with configurable outcomes.
pub struct MockSessionRunner {
    outcome: Arc<Mutex<SessionOutcome>>,
    calls: Arc<Mutex<Vec<SessionRunCall>>>,
}

impl MockSessionRunner {
    /// Create a new `MockSessionRunner` returning `Completed` by default.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(SessionOutcome::Completed {
                story_key: "test".into(),
                branch: "story/test".into(),
                decisions: Vec::new(),
                pr_context: None,
                pr_how_to_test: None,
                pr_additional_info: None,
            })),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome(self, outcome: SessionOutcome) -> Self {
        *self.outcome.lock().unwrap() = outcome;
        self
    }

    /// Simulate running a session for a story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        self.calls.lock().unwrap().push(SessionRunCall {
            story_key: story.story_key.clone(),
        });
        let guard = self.outcome.lock().unwrap();
        clone_session_outcome(&guard)
    }

    /// Check and recover WAL — always returns `None` for mock.
    pub async fn check_and_recover_wal(&self) -> Option<()> {
        None
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<SessionRunCall> {
        self.calls.lock().unwrap().clone()
    }
}

/// Clone a `SessionOutcome` (which doesn't derive Clone).
fn clone_session_outcome(o: &SessionOutcome) -> SessionOutcome {
    match o {
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
            decisions: clone_decisions(decisions),
            pr_context: pr_context.clone(),
            pr_how_to_test: pr_how_to_test.clone(),
            pr_additional_info: pr_additional_info.clone(),
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

/// Clone a vector of `DecisionRecord`.
fn clone_decisions(decisions: &[DecisionRecord]) -> Vec<DecisionRecord> {
    decisions
        .iter()
        .map(|d| DecisionRecord {
            question: d.question.clone(),
            context: d.context.clone(),
            answer: d.answer.clone(),
            reasoning: d.reasoning.clone(),
            alternatives: d.alternatives.clone(),
            source: d.source.clone(),
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
    /// The story key passed to `run`.
    pub story_key: String,
}

/// Standalone mock for `ReviewRunner`.
///
/// Not trait-based — mirrors the public API surface with configurable outcomes.
pub struct MockReviewRunner {
    outcome: Arc<Mutex<ReviewOutcome>>,
    calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl MockReviewRunner {
    /// Create a new `MockReviewRunner` returning `Completed` by default.
    pub fn new() -> Self {
        Self {
            outcome: Arc::new(Mutex::new(ReviewOutcome::Completed {
                story_key: "test".into(),
                branch: "story/test".into(),
                report: "LGTM".into(),
            })),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome(self, outcome: ReviewOutcome) -> Self {
        *self.outcome.lock().unwrap() = outcome;
        self
    }

    /// Simulate running a review for a story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        self.calls.lock().unwrap().push(ReviewRunCall {
            story_key: story.story_key.clone(),
        });
        let guard = self.outcome.lock().unwrap();
        clone_review_outcome(&guard)
    }

    /// Return all recorded calls.
    pub fn calls(&self) -> Vec<ReviewRunCall> {
        self.calls.lock().unwrap().clone()
    }
}

/// Clone a `ReviewOutcome` (which doesn't derive Clone).
fn clone_review_outcome(o: &ReviewOutcome) -> ReviewOutcome {
    match o {
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
