//! Mock implementations for integration tests.
//!
//! Provides configurable test doubles for `GitProvider`, `Notifier`,
//! and standalone mocks for `SessionRunner` / `ReviewRunner`.
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::pipeline::{CodeReviewer, DevRunner};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::escalation::EscalationReport;
use bmad_bot::session::runner::RecoveryInfo;
use bmad_bot::session::SessionOutcome;
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
///
/// Also maintains a shared `event_log` used for cross-mock ordering assertions.
/// [`MockCodeReviewer`] writes `"run_review"` to the same log when review runs,
/// allowing tests to assert `create_pr` happened before `run_review`.
#[derive(Clone)]
pub struct MockGitProvider {
    create_pr_result: Arc<Mutex<Result<PrInfo, GitProviderError>>>,
    add_comment_result: Arc<Mutex<Result<(), GitProviderError>>>,
    get_pr_url_result: Arc<Mutex<Result<String, GitProviderError>>>,
    calls: Arc<Mutex<Vec<GitProviderCall>>>,
    /// Shared event log for cross-mock ordering assertions.
    event_log: Arc<Mutex<Vec<String>>>,
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
            event_log: Arc::new(Mutex::new(Vec::new())),
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

    /// Return all captured `create_pr` call parameters.
    pub fn captured_create_pr_params(&self) -> Vec<CreatePrParams> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                GitProviderCall::CreatePr(params) => Some(params.clone()),
                _ => None,
            })
            .collect()
    }

    /// Return all captured `add_comment` calls as `(pr_id, body)` pairs.
    pub fn captured_add_comment_calls(&self) -> Vec<(String, String)> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| match c {
                GitProviderCall::AddComment { pr_id, body } => {
                    Some((pr_id.clone(), body.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Count of `create_pr` calls.
    pub fn create_pr_call_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, GitProviderCall::CreatePr(_)))
            .count()
    }

    /// Count of `add_comment` calls.
    pub fn add_comment_call_count(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, GitProviderCall::AddComment { .. }))
            .count()
    }

    /// Return a clone of the shared event log Arc for injection into [`MockCodeReviewer`].
    ///
    /// Both mocks write to the same log, enabling ordering assertions like
    /// `create_pr` happened before `run_review`.
    pub fn shared_event_log(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.event_log)
    }

    /// Return a snapshot of the shared event log entries.
    ///
    /// Entries are appended in call order. Possible values:
    /// - `"create_pr"` — from `MockGitProvider::create_pr`
    /// - `"run_review"` — from `MockCodeReviewer::run_review` (when event log is shared)
    pub fn call_events(&self) -> Vec<String> {
        self.event_log.lock().unwrap().clone()
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
        self.event_log
            .lock()
            .unwrap()
            .push("create_pr".to_string());
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
    /// When `true`, `notify_story()` returns `Err(NotifierError::HttpRequest { ... })`.
    fail_notify_story: Arc<std::sync::atomic::AtomicBool>,
}

impl MockNotifier {
    /// Create a new mock notifier.
    pub fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            fail_notify_story: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Create a mock notifier where `notify_story()` always returns an error.
    pub fn failing() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            fail_notify_story: Arc::new(std::sync::atomic::AtomicBool::new(true)),
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

    /// Alias: number of `notify_story` calls.
    pub fn story_notification_count(&self) -> usize {
        self.story_calls().len()
    }

    /// Alias: number of `notify_run_summary` calls.
    pub fn run_summary_count(&self) -> usize {
        self.summary_calls().len()
    }
}

#[async_trait]
impl Notifier for MockNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        self.calls
            .lock()
            .unwrap()
            .push(NotifierCall::NotifyStory(notification.clone()));
        if self
            .fail_notify_story
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(NotifierError::HttpRequest {
                reason: "test error".to_string(),
            });
        }
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

    /// Create a mock that always returns `SessionOutcome::Escalated`.
    pub fn new_escalated(story_key: &str, question: &str) -> Self {
        let report = EscalationReport {
            story_key: story_key.to_string(),
            question: question.to_string(),
            reason: "mock escalation".to_string(),
            branch_name: format!("story/{story_key}"),
            partial_work_summary: "mock partial work".to_string(),
            escalated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(move |_story: &StoryInfo| {
                SessionOutcome::Escalated {
                    report: report.clone(),
                    decisions: vec![],
                }
            }))),
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

// ---------------------------------------------------------------------------
// MockDevRunner (implements DevRunner trait for pipeline integration tests)
// ---------------------------------------------------------------------------

/// Mock implementation of [`DevRunner`] for pipeline integration tests.
///
/// Uses `VecDeque` to support multiple sequential calls (e.g., `process_eligible_stories`).
/// `SessionOutcome` does NOT derive `Clone`, so outcomes are consumed via `pop_front()`.
pub struct MockDevRunner {
    outcomes: Mutex<VecDeque<SessionOutcome>>,
}

impl MockDevRunner {
    /// Single-call mock.
    pub fn with_outcome(outcome: SessionOutcome) -> Self {
        let mut q = VecDeque::new();
        q.push_back(outcome);
        Self {
            outcomes: Mutex::new(q),
        }
    }

    /// Multi-call mock — pops outcomes in order per call.
    pub fn with_outcomes(outcomes: Vec<SessionOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into()),
        }
    }
}

#[async_trait]
impl DevRunner for MockDevRunner {
    async fn run_dev_session(&self, _story: &StoryInfo) -> SessionOutcome {
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockDevRunner: no more outcomes — add more via with_outcomes()")
    }
}

// ---------------------------------------------------------------------------
// MockCodeReviewer (implements CodeReviewer trait for pipeline integration tests)
// ---------------------------------------------------------------------------

/// Mock implementation of [`CodeReviewer`] for pipeline integration tests.
pub struct MockCodeReviewer {
    outcomes: Mutex<VecDeque<ReviewOutcome>>,
    /// Shared event log — wired to [`MockGitProvider::shared_event_log()`] by
    /// [`PipelineTestBuilder`] so ordering assertions work across mock boundaries.
    event_log: Arc<Mutex<Vec<String>>>,
}

impl MockCodeReviewer {
    pub fn with_outcome(outcome: ReviewOutcome) -> Self {
        let mut q = VecDeque::new();
        q.push_back(outcome);
        Self {
            outcomes: Mutex::new(q),
            event_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn never_called() -> Self {
        Self {
            outcomes: Mutex::new(VecDeque::new()),
            event_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Wire this reviewer to the git provider's shared event log for ordering assertions.
    ///
    /// Call this before boxing into `Box<dyn CodeReviewer>`.
    pub fn with_event_log(mut self, log: Arc<Mutex<Vec<String>>>) -> Self {
        self.event_log = log;
        self
    }
}

#[async_trait]
impl CodeReviewer for MockCodeReviewer {
    async fn run_review(&self, _story: &StoryInfo) -> ReviewOutcome {
        self.event_log
            .lock()
            .unwrap()
            .push("run_review".to_string());
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .expect("MockCodeReviewer: no more outcomes (or never_called() was used)")
    }
}
