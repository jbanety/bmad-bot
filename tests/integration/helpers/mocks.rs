//! Mock implementations for integration tests.
//!
//! All mocks are `Send + Sync` and use `Arc<Mutex<...>>` for interior mutability.
//! Builder pattern for mock configuration keeps tests readable.

use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use bmad_bot::git_provider::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
use bmad_bot::notifier::{Notifier, NotifierError, RunSummary, StoryNotification};
use bmad_bot::review::ReviewOutcome;
use bmad_bot::session::SessionOutcome;
use bmad_bot::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// MockGitProvider
// ---------------------------------------------------------------------------

/// Captured call to [`GitProvider::create_pr`].
#[derive(Debug, Clone)]
pub struct CreatePrCall {
    /// The parameters passed to `create_pr`.
    pub params: CreatePrParams,
}

/// Captured call to [`GitProvider::add_comment`].
#[derive(Debug, Clone)]
pub struct AddCommentCall {
    /// PR ID passed to `add_comment`.
    pub pr_id: String,
    /// Comment body passed to `add_comment`.
    pub body: String,
}

/// Captured call to [`GitProvider::get_pr_url`].
#[derive(Debug, Clone)]
pub struct GetPrUrlCall {
    /// PR ID passed to `get_pr_url`.
    pub pr_id: String,
}

/// Shared interior state for [`MockGitProvider`].
#[derive(Debug)]
struct MockGitProviderState {
    create_pr_result: Result<PrInfo, GitProviderError>,
    add_comment_result: Result<(), GitProviderError>,
    get_pr_url_result: Result<String, GitProviderError>,
    create_pr_calls: Vec<CreatePrCall>,
    add_comment_calls: Vec<AddCommentCall>,
    get_pr_url_calls: Vec<GetPrUrlCall>,
}

/// Mock implementation of [`GitProvider`] for integration tests.
///
/// Configurable return values via builder pattern. Tracks all calls for assertion.
#[derive(Debug, Clone)]
pub struct MockGitProvider {
    state: Arc<Mutex<MockGitProviderState>>,
}

impl MockGitProvider {
    /// Create a new mock with default `Ok` responses.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockGitProviderState {
                create_pr_result: Ok(PrInfo {
                    id: "1".into(),
                    url: "https://github.com/test/test/pull/1".into(),
                    number: 1,
                }),
                add_comment_result: Ok(()),
                get_pr_url_result: Ok("https://github.com/test/test/pull/1".into()),
                create_pr_calls: Vec::new(),
                add_comment_calls: Vec::new(),
                get_pr_url_calls: Vec::new(),
            })),
        }
    }

    /// Configure the return value for `create_pr`.
    pub fn with_create_pr(self, result: Result<PrInfo, GitProviderError>) -> Self {
        self.state.lock().expect("lock").create_pr_result = result;
        self
    }

    /// Configure the return value for `add_comment`.
    pub fn with_add_comment(self, result: Result<(), GitProviderError>) -> Self {
        self.state.lock().expect("lock").add_comment_result = result;
        self
    }

    /// Configure the return value for `get_pr_url`.
    pub fn with_get_pr_url(self, result: Result<String, GitProviderError>) -> Self {
        self.state.lock().expect("lock").get_pr_url_result = result;
        self
    }

    /// Get all `create_pr` calls made so far.
    pub fn create_pr_calls(&self) -> Vec<CreatePrCall> {
        self.state.lock().expect("lock").create_pr_calls.clone()
    }

    /// Get all `add_comment` calls made so far.
    pub fn add_comment_calls(&self) -> Vec<AddCommentCall> {
        self.state.lock().expect("lock").add_comment_calls.clone()
    }

    /// Get all `get_pr_url` calls made so far.
    pub fn get_pr_url_calls(&self) -> Vec<GetPrUrlCall> {
        self.state.lock().expect("lock").get_pr_url_calls.clone()
    }
}

impl Default for MockGitProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GitProvider for MockGitProvider {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError> {
        let mut state = self.state.lock().expect("lock");
        state.create_pr_calls.push(CreatePrCall {
            params: params.clone(),
        });
        match &state.create_pr_result {
            Ok(info) => Ok(info.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError> {
        let mut state = self.state.lock().expect("lock");
        state.add_comment_calls.push(AddCommentCall {
            pr_id: pr_id.to_string(),
            body: body.to_string(),
        });
        match &state.add_comment_result {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }

    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
        let mut state = self.state.lock().expect("lock");
        state.get_pr_url_calls.push(GetPrUrlCall {
            pr_id: pr_id.to_string(),
        });
        match &state.get_pr_url_result {
            Ok(url) => Ok(url.clone()),
            Err(e) => Err(clone_git_provider_error(e)),
        }
    }
}

/// Clone a `GitProviderError` (thiserror doesn't derive Clone).
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

/// Captured notification call — either story or run summary.
#[derive(Debug, Clone)]
pub enum NotifierCall {
    /// A `notify_story` call.
    Story(StoryNotification),
    /// A `notify_run_summary` call.
    RunSummary(RunSummary),
}

/// Shared interior state for [`MockNotifier`].
#[derive(Debug)]
struct MockNotifierState {
    calls: Vec<NotifierCall>,
    story_result: Result<(), NotifierError>,
    summary_result: Result<(), NotifierError>,
}

/// Mock implementation of [`Notifier`] for integration tests.
///
/// Captures all calls into a `Vec` for assertion. Configurable error responses.
#[derive(Debug, Clone)]
pub struct MockNotifier {
    state: Arc<Mutex<MockNotifierState>>,
}

impl MockNotifier {
    /// Create a new mock that succeeds on all calls.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockNotifierState {
                calls: Vec::new(),
                story_result: Ok(()),
                summary_result: Ok(()),
            })),
        }
    }

    /// Configure the return value for `notify_story`.
    pub fn with_story_result(self, result: Result<(), NotifierError>) -> Self {
        self.state.lock().expect("lock").story_result = result;
        self
    }

    /// Configure the return value for `notify_run_summary`.
    pub fn with_summary_result(self, result: Result<(), NotifierError>) -> Self {
        self.state.lock().expect("lock").summary_result = result;
        self
    }

    /// Get all calls (both story and summary).
    pub fn calls(&self) -> Vec<NotifierCall> {
        self.state.lock().expect("lock").calls.clone()
    }

    /// Get only `notify_story` calls.
    pub fn story_calls(&self) -> Vec<StoryNotification> {
        self.state
            .lock()
            .expect("lock")
            .calls
            .iter()
            .filter_map(|c| match c {
                NotifierCall::Story(n) => Some(n.clone()),
                _ => None,
            })
            .collect()
    }

    /// Get only `notify_run_summary` calls.
    pub fn summary_calls(&self) -> Vec<RunSummary> {
        self.state
            .lock()
            .expect("lock")
            .calls
            .iter()
            .filter_map(|c| match c {
                NotifierCall::RunSummary(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }
}

impl Default for MockNotifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Clone a `NotifierError`.
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

#[async_trait]
impl Notifier for MockNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        let mut state = self.state.lock().expect("lock");
        state.calls.push(NotifierCall::Story(notification.clone()));
        match &state.story_result {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_notifier_error(e)),
        }
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        let mut state = self.state.lock().expect("lock");
        state
            .calls
            .push(NotifierCall::RunSummary(summary.clone()));
        match &state.summary_result {
            Ok(()) => Ok(()),
            Err(e) => Err(clone_notifier_error(e)),
        }
    }
}

// ---------------------------------------------------------------------------
// MockSessionRunner
// ---------------------------------------------------------------------------

/// Captured call to `MockSessionRunner::run`.
#[derive(Debug, Clone)]
pub struct SessionRunCall {
    /// Story key from the `StoryInfo` passed to `run`.
    pub story_key: String,
}

/// Mock session runner for integration tests.
///
/// Returns a configurable [`SessionOutcome`]. Does NOT implement a shared trait
/// with the real `SessionRunner` — matches the public API surface only.
#[derive(Clone)]
pub struct MockSessionRunner {
    outcome_factory: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> SessionOutcome + Send>>>,
    run_calls: Arc<Mutex<Vec<SessionRunCall>>>,
}

impl std::fmt::Debug for MockSessionRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockSessionRunner")
            .field("run_calls", &self.run_calls)
            .finish_non_exhaustive()
    }
}

impl MockSessionRunner {
    /// Create a new mock that always returns `SessionOutcome::Completed`.
    pub fn new() -> Self {
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
            run_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome<F>(self, f: F) -> Self
    where
        F: Fn(&StoryInfo) -> SessionOutcome + Send + 'static,
    {
        *self.outcome_factory.lock().expect("lock") = Box::new(f);
        self
    }

    /// Simulate running a session for a story.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        {
            let mut calls = self.run_calls.lock().expect("lock");
            calls.push(SessionRunCall {
                story_key: story.story_key.clone(),
            });
        }
        let factory = self.outcome_factory.lock().expect("lock");
        factory(story)
    }

    /// Check for WAL recovery — always returns `None` (no crash to recover from).
    pub async fn check_and_recover_wal(&self) -> Option<()> {
        None
    }

    /// Get all `run` calls made so far.
    pub fn run_calls(&self) -> Vec<SessionRunCall> {
        self.run_calls.lock().expect("lock").clone()
    }
}

impl Default for MockSessionRunner {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MockReviewRunner
// ---------------------------------------------------------------------------

/// Captured call to `MockReviewRunner::run`.
#[derive(Debug, Clone)]
pub struct ReviewRunCall {
    /// Story key from the `StoryInfo` passed to `run`.
    pub story_key: String,
}

/// Mock review runner for integration tests.
///
/// Returns a configurable [`ReviewOutcome`]. Does NOT implement a shared trait
/// with the real `ReviewRunner` — matches the public API surface only.
#[derive(Clone)]
pub struct MockReviewRunner {
    outcome_factory: Arc<Mutex<Box<dyn Fn(&StoryInfo) -> ReviewOutcome + Send>>>,
    run_calls: Arc<Mutex<Vec<ReviewRunCall>>>,
}

impl std::fmt::Debug for MockReviewRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockReviewRunner")
            .field("run_calls", &self.run_calls)
            .finish_non_exhaustive()
    }
}

impl MockReviewRunner {
    /// Create a new mock that always returns `ReviewOutcome::Completed`.
    pub fn new() -> Self {
        Self {
            outcome_factory: Arc::new(Mutex::new(Box::new(|story: &StoryInfo| {
                ReviewOutcome::Completed {
                    story_key: story.story_key.clone(),
                    branch: story.branch_name.clone(),
                    report: "Mock review passed.".into(),
                }
            }))),
            run_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Configure the outcome returned by `run`.
    pub fn with_outcome<F>(self, f: F) -> Self
    where
        F: Fn(&StoryInfo) -> ReviewOutcome + Send + 'static,
    {
        *self.outcome_factory.lock().expect("lock") = Box::new(f);
        self
    }

    /// Simulate running a review for a story.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        {
            let mut calls = self.run_calls.lock().expect("lock");
            calls.push(ReviewRunCall {
                story_key: story.story_key.clone(),
            });
        }
        let factory = self.outcome_factory.lock().expect("lock");
        factory(story)
    }

    /// Get all `run` calls made so far.
    pub fn run_calls(&self) -> Vec<ReviewRunCall> {
        self.run_calls.lock().expect("lock").clone()
    }
}

impl Default for MockReviewRunner {
    fn default() -> Self {
        Self::new()
    }
}
