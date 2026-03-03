//! Story pipeline orchestrator — daemon Layer 3 error handling and story lifecycle.
//!
//! The [`StoryPipeline`] struct encapsulates the full story processing pipeline:
//! session → push → PR creation → optional review → notification. It implements the
//! "never stop the run" principle: no single story failure halts the daemon.
//!
//! Use [`StoryPipeline::new`] to construct, then [`process_eligible_stories`] to
//! run a batch of stories from the watcher.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use crate::config::{BotConfig, BotSecrets};
use crate::git_provider::{
    CreatePrParams, GitProvider, PrDescriptionParams, PrSummary, build_pr_description,
    build_pr_title, create_provider,
};
use crate::llm::AgentFactory;
use crate::notifier::{Notifier, RunSummary, StoryNotification, StoryStatus, create_notifier};
use crate::review::ReviewOutcome;
use crate::review::ReviewRunner;
use crate::session::SessionOutcome;
use crate::session::analyzer::strip_agent_artifacts;
use crate::session::cleanup::{unblock_dependents, update_story_status};
use crate::session::runner::SessionRunner;
use crate::session::runner::ShutdownFlag;
use crate::supervisor::decisions::format_pr_decisions_section;
use crate::watcher::StoryInfo;

// ---------------------------------------------------------------------------
// DevRunner / CodeReviewer traits (dependency injection for testing)
// ---------------------------------------------------------------------------

/// Trait abstraction for dev session execution.
///
/// Implemented by [`SessionRunner`] in production and by [`MockDevRunner`] in tests.
/// Allows [`StoryPipeline`] to accept injected test doubles.
#[async_trait]
pub trait DevRunner: Send + Sync {
    /// Execute a development session for the given story.
    async fn run_dev_session(&self, story: &StoryInfo) -> SessionOutcome;
}

/// Trait abstraction for code review execution.
///
/// Implemented by [`ReviewRunner`] in production and by [`MockCodeReviewer`] in tests.
#[async_trait]
pub trait CodeReviewer: Send + Sync {
    /// Execute a code review for the given story.
    async fn run_review(&self, story: &StoryInfo) -> ReviewOutcome;
}

#[async_trait]
impl DevRunner for SessionRunner {
    async fn run_dev_session(&self, story: &StoryInfo) -> SessionOutcome {
        self.run(story).await
    }
}

#[async_trait]
impl CodeReviewer for ReviewRunner {
    async fn run_review(&self, story: &StoryInfo) -> ReviewOutcome {
        self.run(story).await
    }
}

// ---------------------------------------------------------------------------
// PipelineError
// ---------------------------------------------------------------------------

/// Typed error enum for pipeline-level failures.
///
/// Follows the project-wide pattern of `{ reason: String }` fields.
/// No `#[from]` on external errors — all mapped via `.map_err()`.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    /// Pipeline construction failure (git provider init, notifier init).
    #[error("Pipeline initialization failed: {reason}")]
    Init {
        /// Description of the initialization failure.
        reason: String,
    },

    /// Session returned `Failed` for a story.
    #[error("Session failed for story {story_key}: {error}")]
    Session {
        /// The story key that failed.
        story_key: String,
        /// Description of the session failure.
        error: String,
    },

    /// Review returned `Failed` for a story.
    #[error("Review failed for story {story_key}: {error}")]
    Review {
        /// The story key that failed review.
        story_key: String,
        /// Description of the review failure.
        error: String,
    },

    /// Git provider failed to create a PR.
    #[error("PR creation failed for story {story_key} (branch: {branch}): {reason}")]
    PrCreation {
        /// The story key for which PR creation failed.
        story_key: String,
        /// The branch containing the work.
        branch: String,
        /// Description of the PR creation failure.
        reason: String,
    },

    /// Failed to post a comment on a PR (non-blocking).
    #[error("PR comment failed for PR {pr_id}: {reason}")]
    PrComment {
        /// The PR identifier.
        pr_id: String,
        /// Description of the comment failure.
        reason: String,
    },

    /// Notification delivery failed (always non-blocking).
    #[error("Notification failed: {reason}")]
    Notification {
        /// Description of the notification failure.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// PipelineResult
// ---------------------------------------------------------------------------

/// Outcome of processing a single story through the pipeline.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// The story key that was processed.
    pub story_key: String,
    /// Final status of the story after pipeline processing.
    pub status: StoryStatus,
    /// URL to the created PR, if one was successfully created.
    pub pr_url: Option<String>,
    /// Error or context detail, if applicable.
    pub error_detail: Option<String>,
    /// When `true`, this error is fatal — the daemon should halt immediately.
    ///
    /// Set for authentication failures and other infrastructure errors where
    /// continuing to process stories would be pointless (same creds, same result).
    pub fatal: bool,
}

// ---------------------------------------------------------------------------
// StoryPipeline
// ---------------------------------------------------------------------------

/// Daemon orchestration layer — processes stories through the full pipeline.
///
/// Encapsulates session runner, review runner, git provider, and notifier.
/// Constructed once per daemon run via [`StoryPipeline::new`].
pub struct StoryPipeline {
    /// Shared daemon configuration.
    config: Arc<BotConfig>,
    /// Git hosting provider (GitHub or GitLab).
    git_provider: Box<dyn GitProvider>,
    /// Notification sender (Telegram or Noop).
    notifier: Box<dyn Notifier>,
    /// Development session runner (trait object for testability).
    dev_runner: Box<dyn DevRunner>,
    /// Code review session runner (trait object for testability).
    code_reviewer: Box<dyn CodeReviewer>,
    /// Concrete session runner for WAL recovery (check_and_recover_wal + resume_session).
    /// Set by new(), None in new_with_components(). Recovery returns None when absent.
    session_runner_for_recovery: Option<SessionRunner>,
}

impl StoryPipeline {
    /// Create a new pipeline from configuration and secrets.
    ///
    /// Initializes the git provider, notifier, session runner, and review runner.
    ///
    /// # Errors
    /// Returns [`PipelineError::InitFailed`] if the git provider cannot be created
    /// (e.g., unsupported provider type or missing token).
    pub fn new(
        config: Arc<BotConfig>,
        secrets: Arc<BotSecrets>,
        shutdown: ShutdownFlag,
        mcp_manager: Arc<crate::mcp::McpManager>,
    ) -> Result<Self, PipelineError> {
        // Extract the correct token for the configured git provider
        let token = match config.git_provider.provider.as_str() {
            "github" => secrets.github_token.as_deref().unwrap_or(""),
            "gitlab" => secrets.gitlab_token.as_deref().unwrap_or(""),
            other => {
                return Err(PipelineError::Init {
                    reason: format!("Unsupported git provider: {other}"),
                });
            }
        };

        let git_provider =
            create_provider(&config.git_provider, token).map_err(|e| PipelineError::Init {
                reason: e.to_string(),
            })?;

        // Factory never fails — returns NoopNotifier as fallback
        let notifier = create_notifier(&config.notifications, &secrets);

        // Create the centralized AgentFactory — owns secrets + Copilot token cache.
        let agent_factory = Arc::new(AgentFactory::new(Arc::clone(&config), Arc::clone(&secrets)));

        let session_runner = SessionRunner::new(
            Arc::clone(&config),
            Arc::clone(&agent_factory),
            Arc::clone(&shutdown),
            Arc::clone(&mcp_manager),
        );
        let review_runner = ReviewRunner::new(
            Arc::clone(&config),
            Arc::clone(&secrets),
            Arc::clone(&agent_factory),
            shutdown,
            Arc::clone(&mcp_manager),
        );

        // Second SessionRunner instance for WAL recovery — cheap (no state, no network).
        let session_runner_for_recovery = SessionRunner::new(
            Arc::clone(&config),
            Arc::clone(&agent_factory),
            // Recovery doesn't need a live shutdown signal; use a dummy flag.
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
            mcp_manager,
        );

        Ok(Self {
            config,
            git_provider,
            notifier,
            dev_runner: Box::new(session_runner),
            code_reviewer: Box::new(review_runner),
            session_runner_for_recovery: Some(session_runner_for_recovery),
        })
    }

    /// Construct a pipeline with pre-built dependencies (for integration tests).
    ///
    /// Accepts trait objects for all pluggable components, enabling full mock injection.
    /// `session_runner_for_recovery` is `None` — recovery returns `None` in tests.
    pub fn new_with_components(
        config: Arc<BotConfig>,
        git_provider: Box<dyn GitProvider>,
        notifier: Box<dyn Notifier>,
        dev_runner: Box<dyn DevRunner>,
        code_reviewer: Box<dyn CodeReviewer>,
    ) -> Self {
        Self {
            config,
            git_provider,
            notifier,
            dev_runner,
            code_reviewer,
            session_runner_for_recovery: None,
        }
    }

    /// Process a single story through the full pipeline.
    ///
    /// Runs dev session → optional code review → PR creation → notification.
    /// Never panics — all errors are caught and returned as [`PipelineResult`].
    pub async fn process_story(&self, story: &StoryInfo) -> PipelineResult {
        let story_title = story_title_from_label(&story.label);

        tracing::info!(
            action = "pipeline_start",
            story_key = %story.story_key,
            story_id = %story.story_id,
            "Starting pipeline for story"
        );

        // Phase 1 — Dev Session
        let session_outcome = self.dev_runner.run_dev_session(story).await;

        match session_outcome {
            SessionOutcome::Completed {
                story_key,
                branch,
                decisions,
                pr_context,
                pr_how_to_test,
                pr_additional_info,
            } => {
                // Phase 2 — Push branch to remote before PR creation (non-blocking)
                let push_ok = match self.push_branch(&branch).await {
                    Ok(()) => true,
                    Err(e) => {
                        tracing::warn!(
                            action = "push_failed",
                            story_key = %story_key,
                            branch = %branch,
                            error = %e,
                            "Git push failed — work preserved locally, skipping PR/review"
                        );
                        false
                    }
                };

                if !push_ok {
                    // Work is committed locally on the branch — skip PR/review, mark Completed
                    let result = PipelineResult {
                        story_key: story_key.clone(),
                        status: StoryStatus::Completed,
                        pr_url: None,
                        error_detail: Some(format!(
                            "Push failed — work preserved on local branch: {branch}"
                        )),
                        fatal: false,
                    };
                    self.notify_story_result(&result).await;
                    return result;
                }

                // Phase 3 — Create PR (before review so it's visible immediately)
                let decisions_section = format_pr_decisions_section(&decisions);
                let pr_summary = pr_context.map(|ctx| PrSummary {
                    context: ctx,
                    how_to_test: pr_how_to_test.unwrap_or_default(),
                    additional_info: pr_additional_info.unwrap_or_default(),
                });
                let pr_title = build_pr_title(&story_key, &story_title, false);
                let pr_body = build_pr_description(&PrDescriptionParams {
                    story_key: story_key.clone(),
                    story_title: story_title.clone(),
                    outcome_summary: "completed successfully".to_string(),
                    decisions_section,
                    failure_details: None,
                    pr_summary,
                });
                let pr_params = CreatePrParams {
                    title: pr_title,
                    body: pr_body,
                    source_branch: branch.clone(),
                    target_branch: self.config.git_provider.target_branch.clone(),
                };

                let pr_info = match self.git_provider.create_pr(pr_params).await {
                    Ok(info) => info,
                    Err(e) => {
                        tracing::error!(
                            action = "pr_creation_failed",
                            story_key = %story_key,
                            branch = %branch,
                            error = %e,
                            "PR creation failed — skipping review, notifying human with branch name"
                        );

                        let result = PipelineResult {
                            story_key: story_key.clone(),
                            status: StoryStatus::Error,
                            pr_url: None,
                            error_detail: Some(format!(
                                "PR creation failed: {e}. Branch: {branch}"
                            )),
                            fatal: false,
                        };
                        self.notify_story_result(&result).await;
                        return result;
                    }
                };

                // Phase 4 — Code Review (optional, on existing PR)
                let review_report = if self.config.code_review_enabled {
                    match self.code_reviewer.run_review(story).await {
                        ReviewOutcome::Completed { report, .. } => Some(report),
                        ReviewOutcome::Failed {
                            story_key: rk,
                            error,
                        } => {
                            tracing::warn!(
                                action = "review_failed",
                                story_key = %rk,
                                error = %error,
                                "Code review failed — PR already exists"
                            );
                            None
                        }
                        ReviewOutcome::Skipped { reason } => {
                            tracing::info!(
                                action = "review_skipped",
                                reason = %reason,
                                "Code review skipped — PR already exists"
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                // Phase 5 — Push review fix commits to update PR (if review ran)
                if review_report.is_some()
                    && let Err(e) = self.push_branch(&branch).await
                {
                    tracing::warn!(
                        action = "review_push_failed",
                        story_key = %story_key,
                        branch = %branch,
                        error = %e,
                        "Failed to push review fix commits — PR still exists with dev commits"
                    );
                }

                // Phase 6 — Post review comment on PR (non-blocking)
                // Report is already formatted by build_review_comment() — no stripping needed.
                if let Some(ref report) = review_report
                    && let Err(e) = self.git_provider.add_comment(&pr_info.id, report).await
                {
                    tracing::error!(
                        action = "pr_comment_failed",
                        pr_id = %pr_info.id,
                        error = %e,
                        "Failed to post review comment — PR created successfully"
                    );
                }

                // Phase 7 — Mark story done in sprint-status.yaml, commit & push
                let sprint_status_path =
                    Path::new(&self.config.bmad_paths.implementation_artifacts)
                        .join("sprint-status.yaml");
                if sprint_status_path.exists() {
                    if let Err(e) =
                        update_story_status(&sprint_status_path, &story_key, "done").await
                    {
                        tracing::warn!(
                            action = "sprint_status_update_failed",
                            story_key = %story_key,
                            error = %e,
                            "Failed to mark story as done in sprint-status.yaml"
                        );
                    } else {
                        // Unblock dependent stories (blocked → ready-for-dev)
                        match unblock_dependents(&sprint_status_path, &story_key).await {
                            Ok(unblocked) if !unblocked.is_empty() => {
                                tracing::info!(
                                    action = "dependents_unblocked",
                                    story_key = %story_key,
                                    unblocked_count = unblocked.len(),
                                    unblocked = %unblocked.join(", "),
                                    "Unblocked dependent stories"
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    action = "unblock_dependents_failed",
                                    story_key = %story_key,
                                    error = %e,
                                    "Failed to unblock dependent stories"
                                );
                            }
                            _ => {}
                        }

                        // Commit & push — MUST succeed or next story's checkout discards changes
                        let commit_msg = format!(
                            "chore(sprint-status): mark {story_key} done, unblock dependents"
                        );
                        match commit_sprint_status(
                            &self.config.bmad_paths.project_root,
                            &sprint_status_path,
                            &commit_msg,
                        )
                        .await
                        {
                            Ok(()) => {
                                if let Err(e) = self.push_branch(&branch).await {
                                    tracing::warn!(
                                        action = "sprint_status_push_failed",
                                        story_key = %story_key,
                                        error = %e,
                                        "Failed to push sprint-status commit — PR may not reflect done status"
                                    );
                                }
                            }
                            Err(e) => {
                                // Cannot persist "done" status — halt pipeline to prevent
                                // infinite loop (next checkout would discard changes, watcher
                                // would re-select already-completed stories).
                                tracing::error!(
                                    action = "sprint_status_commit_failed",
                                    story_key = %story_key,
                                    error = %e,
                                    "CRITICAL: sprint-status commit failed — halting pipeline to prevent infinite loop"
                                );
                                self.notify_story_result(&PipelineResult {
                                    story_key: story_key.clone(),
                                    status: StoryStatus::Error,
                                    pr_url: Some(pr_info.url.clone()),
                                    error_detail: Some(format!(
                                        "Story completed and PR created, but sprint-status commit failed: {e}. \
                                         Pipeline halted to prevent infinite loop. Fix git config and restart."
                                    )),
                                    fatal: true,
                                }).await;
                                return PipelineResult {
                                    story_key: story_key.clone(),
                                    status: StoryStatus::Error,
                                    pr_url: Some(pr_info.url.clone()),
                                    error_detail: Some(format!("sprint-status commit failed: {e}")),
                                    fatal: true,
                                };
                            }
                        }
                    }
                }

                // Phase 8 — Notify
                let result = PipelineResult {
                    story_key: story_key.clone(),
                    status: StoryStatus::Completed,
                    pr_url: Some(pr_info.url.clone()),
                    error_detail: None,
                    fatal: false,
                };
                self.notify_story_result(&result).await;
                result
            }

            SessionOutcome::Escalated { report, decisions } => {
                tracing::warn!(
                    action = "session_escalated",
                    story_key = %report.story_key,
                    question = %report.question,
                    reason = %report.reason,
                    "Story escalated — needs human clarification, creating escalation PR"
                );

                // Push branch to remote (best-effort, same pattern as Failed branch)
                let branch = report.branch_name.clone();
                if let Err(e) = self.push_branch(&branch).await {
                    tracing::warn!(
                        action = "escalation_push_failed",
                        story_key = %report.story_key,
                        branch = %branch,
                        error = %e,
                        "Git push failed for escalation branch — attempting PR anyway"
                    );
                }

                // Build PrSummary from EscalationReport fields
                let partial_work = if report.partial_work_summary.is_empty() {
                    "No partial work summary available.".to_string()
                } else {
                    format!("Partial work summary: {}", report.partial_work_summary)
                };
                let pr_summary = PrSummary {
                    context: format!(
                        "Session escalated to human. Question: {}. Reason: {}",
                        report.question, report.reason
                    ),
                    how_to_test: "N/A — session was escalated and requires human clarification."
                        .to_string(),
                    additional_info: partial_work,
                };

                let decisions_section = format_pr_decisions_section(&decisions);
                let pr_title = build_pr_title(&report.story_key, &story_title, true);
                let pr_body = build_pr_description(&PrDescriptionParams {
                    story_key: report.story_key.clone(),
                    story_title: story_title.clone(),
                    outcome_summary: "escalated — needs clarification".to_string(),
                    decisions_section,
                    failure_details: Some(format!(
                        "**Question:** {}\n**Reason:** {}",
                        report.question, report.reason
                    )),
                    pr_summary: Some(pr_summary),
                });
                let pr_params = CreatePrParams {
                    title: pr_title,
                    body: pr_body,
                    source_branch: branch.clone(),
                    target_branch: self.config.git_provider.target_branch.clone(),
                };

                let pr_url = match self.git_provider.create_pr(pr_params).await {
                    Ok(pr_info) => Some(pr_info.url.clone()),
                    Err(e) => {
                        tracing::error!(
                            action = "escalation_pr_creation_failed",
                            story_key = %report.story_key,
                            branch = %branch,
                            error = %e,
                            "Failed to create escalation PR — notifying human with branch name only"
                        );
                        None
                    }
                };

                let result = PipelineResult {
                    story_key: report.story_key.clone(),
                    status: StoryStatus::Blocked,
                    pr_url,
                    error_detail: Some(format!(
                        "Escalated: {} — {}",
                        report.question, report.reason
                    )),
                    fatal: false,
                };
                self.notify_story_result(&result).await;
                result
            }

            SessionOutcome::Failed {
                story_key,
                error,
                decisions,
            } => {
                let fatal = is_auth_error(&error);
                let infra = is_infra_error(&error);

                if infra {
                    // Infrastructure failure — session never started, no partial work.
                    // Skip PR creation entirely (there's nothing on the branch).
                    if fatal {
                        tracing::error!(
                            action = "session_failed_fatal",
                            story_key = %story_key,
                            error = %error,
                            "Fatal infrastructure error — daemon should halt"
                        );
                    } else {
                        tracing::error!(
                            action = "session_failed_infra",
                            story_key = %story_key,
                            error = %error,
                            "Infrastructure error — no partial work, skipping failure PR"
                        );
                    }

                    let result = PipelineResult {
                        story_key: story_key.clone(),
                        status: StoryStatus::Error,
                        pr_url: None,
                        error_detail: Some(error),
                        fatal,
                    };
                    self.notify_story_result(&result).await;
                    result
                } else {
                    tracing::error!(
                        action = "session_failed",
                        story_key = %story_key,
                        error = %error,
                        "Dev session failed mid-work — creating failure PR to preserve partial work"
                    );

                    // Session crashed mid-work — failure PR preserves partial code
                    let branch = format!("story/{story_key}");

                    if let Err(e) = self.push_branch(&branch).await {
                        tracing::warn!(
                            action = "failure_push_failed",
                            story_key = %story_key,
                            branch = %branch,
                            error = %e,
                            "Git push failed for failure branch — attempting PR anyway"
                        );
                    }

                    let decisions_section = format_pr_decisions_section(&decisions);
                    let pr_title = build_pr_title(&story_key, &story_title, true);
                    let pr_body = build_pr_description(&PrDescriptionParams {
                        story_key: story_key.clone(),
                        story_title: story_title.clone(),
                        outcome_summary: "failed".to_string(),
                        decisions_section,
                        failure_details: Some(error.clone()),
                        pr_summary: None,
                    });
                    let pr_params = CreatePrParams {
                        title: pr_title,
                        body: pr_body,
                        source_branch: branch.clone(),
                        target_branch: self.config.git_provider.target_branch.clone(),
                    };

                    match self.git_provider.create_pr(pr_params).await {
                        Ok(pr_info) => {
                            let result = PipelineResult {
                                story_key: story_key.clone(),
                                status: StoryStatus::Error,
                                pr_url: Some(pr_info.url.clone()),
                                error_detail: Some(error),
                                fatal: false,
                            };
                            self.notify_story_result(&result).await;
                            result
                        }
                        Err(pr_err) => {
                            tracing::error!(
                                action = "failure_pr_creation_failed",
                                story_key = %story_key,
                                branch = %branch,
                                error = %pr_err,
                                "Failed to create failure PR — notifying human with branch name only"
                            );

                            let result = PipelineResult {
                                story_key: story_key.clone(),
                                status: StoryStatus::Error,
                                pr_url: None,
                                error_detail: Some(format!(
                                    "Session failed: {error}. PR creation also failed: {pr_err}. Branch: {branch}"
                                )),
                                fatal: false,
                            };
                            self.notify_story_result(&result).await;
                            result
                        }
                    }
                }
            }
        }
    }

    /// Process all eligible stories sequentially, then send a run summary.
    ///
    /// Stories are processed in the order received from the watcher (dependency-sorted).
    /// After all stories are processed, a run summary notification is sent.
    ///
    /// If any story returns a fatal error (e.g. auth failure), processing stops
    /// immediately — remaining stories are skipped. The [`RunSummary::fatal`] flag
    /// is set so the caller can halt the daemon.
    pub async fn process_eligible_stories(&self, stories: Vec<StoryInfo>) -> RunSummary {
        let mut results: Vec<PipelineResult> = Vec::with_capacity(stories.len());

        let sprint_status_path = PathBuf::from(&self.config.bmad_paths.implementation_artifacts)
            .join("sprint-status.yaml");

        for story in &stories {
            let result = self.process_story(story).await;
            let mut is_fatal = result.fatal;
            let story_key = &result.story_key;

            // Safety net: if sprint-status.yaml has uncommitted changes after
            // processing a story, commit them NOW before the next story's
            // `git checkout` discards them. This prevents the infinite loop
            // where completed stories revert to ready-for-dev.
            if sprint_status_path.exists() && !is_fatal {
                let repo_path = &self.config.bmad_paths.project_root;
                if has_uncommitted_sprint_status(repo_path, &sprint_status_path).await {
                    let commit_msg =
                        format!("chore(sprint-status): persist status updates after {story_key}");
                    match commit_sprint_status(repo_path, &sprint_status_path, &commit_msg).await {
                        Ok(()) => {
                            tracing::info!(
                                action = "sprint_status_safety_commit",
                                story_key = %story_key,
                                "Safety net: committed uncommitted sprint-status changes before next story"
                            );
                        }
                        Err(e) => {
                            // Safety net also failed — halt to prevent infinite loop
                            tracing::error!(
                                action = "sprint_status_safety_commit_failed",
                                story_key = %story_key,
                                error = %e,
                                "CRITICAL: safety net commit also failed — halting pipeline"
                            );
                            is_fatal = true;
                        }
                    }
                }
            }

            results.push(result);

            if is_fatal {
                tracing::error!(
                    action = "pipeline_halt",
                    story_key = %story.story_key,
                    "Fatal error detected — stopping pipeline, skipping remaining stories"
                );
                break;
            }
        }

        let summary = build_run_summary(&results);

        // Send run summary notification (non-blocking)
        if let Err(e) = self.notifier.notify_run_summary(&summary).await {
            tracing::error!(
                action = "run_summary_notification_failed",
                error = %e,
                "Failed to send run summary notification"
            );
        }

        summary
    }

    /// Send a notification for a single story result (non-blocking).
    /// Push a local branch to the remote using git CLI.
    ///
    /// Uses `--force-with-lease` because story branches are single-developer
    /// branches that may be rebased/reset between daemon runs. This is safe
    /// and avoids non-fast-forward rejections from previous attempts.
    ///
    /// Authentication is inherited from the user's git configuration (SSH agent,
    /// credential helper, osxkeychain, etc.). No HTTPS URL construction or
    /// credential callback needed.
    async fn push_branch(&self, branch: &str) -> Result<(), PipelineError> {
        let repo_path = PathBuf::from(&self.config.bmad_paths.project_root);

        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .args(["push", "--force-with-lease", "origin", branch])
            .output()
            .await
            .map_err(|e| PipelineError::Init {
                reason: format!("Failed to execute git push: {e}"),
            })?;

        if output.status.success() {
            tracing::info!(
                action = "branch_pushed",
                branch = %branch,
                "Branch pushed to origin"
            );
            return Ok(());
        }

        // Push failed — check for stale/rejected refs and attempt recovery
        let stderr = String::from_utf8_lossy(&output.stderr);
        let is_stale = stderr.contains("stale info")
            || stderr.contains("[rejected]")
            || stderr.contains("non-fast-forward");

        if !is_stale {
            return Err(PipelineError::PrCreation {
                story_key: String::new(),
                branch: branch.to_string(),
                reason: format!("Git push failed: {stderr}"),
            });
        }

        // Stale refs detected — prune and retry once
        tracing::info!(
            action = "push_stale_detected",
            branch = %branch,
            "Push rejected due to stale refs — pruning and retrying"
        );

        let _ = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .args(["remote", "prune", "origin"])
            .output()
            .await;

        let retry = tokio::process::Command::new("git")
            .arg("-C")
            .arg(&repo_path)
            .args(["push", "--force-with-lease", "origin", branch])
            .output()
            .await
            .map_err(|e| PipelineError::Init {
                reason: format!("Failed to execute git push (retry): {e}"),
            })?;

        if retry.status.success() {
            tracing::info!(
                action = "branch_pushed",
                branch = %branch,
                "Branch pushed to origin after prune"
            );
            return Ok(());
        }

        let retry_stderr = String::from_utf8_lossy(&retry.stderr);
        Err(PipelineError::PrCreation {
            story_key: String::new(),
            branch: branch.to_string(),
            reason: format!("Git push failed after prune: {retry_stderr}"),
        })
    }

    async fn notify_story_result(&self, result: &PipelineResult) {
        let notification = StoryNotification {
            story_id: result
                .story_key
                .split('-')
                .take(2)
                .collect::<Vec<_>>()
                .join("."),
            story_key: result.story_key.clone(),
            status: result.status.clone(),
            pr_url: result.pr_url.clone(),
            reason: result.error_detail.clone(),
        };

        if let Err(e) = self.notifier.notify_story(&notification).await {
            tracing::error!(
                action = "notification_failed",
                story_key = %result.story_key,
                error = %e,
                "Telegram notification failed — continuing"
            );
        }
    }

    /// Check for an interrupted session WAL and process recovery if needed.
    ///
    /// Returns `Some(PipelineResult)` if a WAL was found and recovery was attempted,
    /// or `None` for a clean start (no WAL). Recovery failure does NOT prevent the
    /// daemon from entering the polling loop — all errors are handled internally.
    ///
    /// **Critical:** This must be called BEFORE the polling loop starts. The daemon
    /// must not poll for new stories while a recovered session is in progress.
    pub async fn recover_and_process(&self) -> Option<PipelineResult> {
        let runner = self.session_runner_for_recovery.as_ref()?;
        let recovery = runner.check_and_recover_wal().await?;

        // Clone StoryInfo fields BEFORE consuming recovery (SessionState has no Clone)
        let story_for_pipeline = StoryInfo {
            story_id: recovery.story_info.story_id.clone(),
            story_key: recovery.story_info.story_key.clone(),
            epic_num: recovery.story_info.epic_num,
            story_num: recovery.story_info.story_num,
            label: recovery.story_info.label.clone(),
            branch_name: recovery.story_info.branch_name.clone(),
            specs_path: recovery.story_info.specs_path.clone(),
            dependencies: vec![],
            status: "in-progress".to_string(),
        };

        let outcome = runner.resume_session(recovery).await;
        let result = self
            .process_recovered_session(&story_for_pipeline, outcome)
            .await;
        self.notify_story_result(&result).await;
        Some(result)
    }

    /// Process the outcome of a recovered session through the post-session pipeline.
    ///
    /// Reuses the same post-session logic as [`process_story()`]: code review → PR → notification.
    ///
    /// Public for integration test access. Production code calls this via
    /// [`recover_and_process()`](Self::recover_and_process).
    pub async fn process_recovered_session(
        &self,
        story: &StoryInfo,
        outcome: SessionOutcome,
    ) -> PipelineResult {
        let story_title = story_title_from_label(&story.label);

        match outcome {
            SessionOutcome::Completed {
                story_key,
                branch,
                decisions,
                pr_context,
                pr_how_to_test,
                pr_additional_info,
            } => {
                // Optional code review
                let review_report = if self.config.code_review_enabled {
                    match self.code_reviewer.run_review(story).await {
                        ReviewOutcome::Completed { report, .. } => Some(report),
                        ReviewOutcome::Failed {
                            story_key: rk,
                            error,
                        } => {
                            tracing::warn!(
                                action = "recovery_review_failed",
                                story_key = %rk,
                                error = %error,
                                "Code review failed after recovery — continuing to PR creation"
                            );
                            None
                        }
                        ReviewOutcome::Skipped { reason } => {
                            tracing::info!(
                                action = "recovery_review_skipped",
                                reason = %reason,
                                "Code review skipped after recovery — continuing to PR creation"
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                // Push branch before PR creation
                if let Err(e) = self.push_branch(&branch).await {
                    tracing::error!(
                        action = "recovery_push_failed",
                        story_key = %story_key,
                        branch = %branch,
                        error = %e,
                        "Git push failed after recovery — cannot create PR"
                    );
                    return PipelineResult {
                        story_key: story_key.clone(),
                        status: StoryStatus::Error,
                        pr_url: None,
                        error_detail: Some(format!(
                            "Git push failed after recovery: {e}. Branch: {branch}"
                        )),
                        fatal: false,
                    };
                }

                // Success PR
                let decisions_section = format_pr_decisions_section(&decisions);
                let pr_summary = pr_context.map(|ctx| PrSummary {
                    context: ctx,
                    how_to_test: pr_how_to_test.unwrap_or_default(),
                    additional_info: pr_additional_info.unwrap_or_default(),
                });
                let pr_title = build_pr_title(&story_key, &story_title, false);
                let pr_body = build_pr_description(&PrDescriptionParams {
                    story_key: story_key.clone(),
                    story_title: story_title.clone(),
                    outcome_summary: "completed successfully (recovered from crash)".to_string(),
                    decisions_section,
                    failure_details: None,
                    pr_summary,
                });
                let pr_params = CreatePrParams {
                    title: pr_title,
                    body: pr_body,
                    source_branch: branch.clone(),
                    target_branch: self.config.git_provider.target_branch.clone(),
                };

                match self.git_provider.create_pr(pr_params).await {
                    Ok(pr_info) => {
                        if let Some(ref report) = review_report
                            && let Err(e) = self
                                .git_provider
                                .add_comment(&pr_info.id, &strip_agent_artifacts(report))
                                .await
                        {
                            tracing::error!(
                                action = "recovery_pr_comment_failed",
                                pr_id = %pr_info.id,
                                error = %e,
                                "Failed to post review comment after recovery"
                            );
                        }

                        PipelineResult {
                            story_key: story_key.clone(),
                            status: StoryStatus::Completed,
                            pr_url: Some(pr_info.url.clone()),
                            error_detail: None,
                            fatal: false,
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            action = "recovery_pr_creation_failed",
                            story_key = %story_key,
                            branch = %branch,
                            error = %e,
                            "PR creation failed after recovery"
                        );
                        PipelineResult {
                            story_key: story_key.clone(),
                            status: StoryStatus::Error,
                            pr_url: None,
                            error_detail: Some(format!(
                                "PR creation failed after recovery: {e}. Branch: {branch}"
                            )),
                            fatal: false,
                        }
                    }
                }
            }

            SessionOutcome::Escalated { report, decisions } => {
                tracing::warn!(
                    action = "recovery_session_escalated",
                    story_key = %report.story_key,
                    question = %report.question,
                    "Recovered session escalated — needs human clarification, creating escalation PR"
                );

                // Push branch to remote (best-effort)
                let branch = report.branch_name.clone();
                if let Err(e) = self.push_branch(&branch).await {
                    tracing::warn!(
                        action = "recovery_escalation_push_failed",
                        story_key = %report.story_key,
                        branch = %branch,
                        error = %e,
                        "Git push failed for recovery escalation branch — attempting PR anyway"
                    );
                }

                // Build PrSummary from EscalationReport fields
                let partial_work = if report.partial_work_summary.is_empty() {
                    "No partial work summary available.".to_string()
                } else {
                    format!("Partial work summary: {}", report.partial_work_summary)
                };
                let pr_summary = PrSummary {
                    context: format!(
                        "Session escalated to human. Question: {}. Reason: {}",
                        report.question, report.reason
                    ),
                    how_to_test: "N/A — session was escalated and requires human clarification."
                        .to_string(),
                    additional_info: partial_work,
                };

                let decisions_section = format_pr_decisions_section(&decisions);
                let pr_title = build_pr_title(&report.story_key, &story_title, true);
                let pr_body = build_pr_description(&PrDescriptionParams {
                    story_key: report.story_key.clone(),
                    story_title: story_title.clone(),
                    outcome_summary: "escalated — needs clarification (recovered from crash)"
                        .to_string(),
                    decisions_section,
                    failure_details: Some(format!(
                        "**Question:** {}\n**Reason:** {}",
                        report.question, report.reason
                    )),
                    pr_summary: Some(pr_summary),
                });
                let pr_params = CreatePrParams {
                    title: pr_title,
                    body: pr_body,
                    source_branch: branch.clone(),
                    target_branch: self.config.git_provider.target_branch.clone(),
                };

                let pr_url = match self.git_provider.create_pr(pr_params).await {
                    Ok(pr_info) => Some(pr_info.url.clone()),
                    Err(e) => {
                        tracing::error!(
                            action = "recovery_escalation_pr_creation_failed",
                            story_key = %report.story_key,
                            branch = %branch,
                            error = %e,
                            "Failed to create escalation PR after recovery"
                        );
                        None
                    }
                };

                PipelineResult {
                    story_key: report.story_key.clone(),
                    status: StoryStatus::Blocked,
                    pr_url,
                    error_detail: Some(format!(
                        "Escalated after recovery: {} — {}",
                        report.question, report.reason
                    )),
                    fatal: false,
                }
            }

            SessionOutcome::Failed {
                story_key,
                error,
                decisions,
            } => {
                let fatal = is_auth_error(&error);
                let infra = is_infra_error(&error);

                if infra {
                    if fatal {
                        tracing::error!(
                            action = "recovery_session_failed_fatal",
                            story_key = %story_key,
                            error = %error,
                            "Fatal infrastructure error during recovery — daemon should halt"
                        );
                    } else {
                        tracing::error!(
                            action = "recovery_session_failed_infra",
                            story_key = %story_key,
                            error = %error,
                            "Infrastructure error during recovery — no partial work, skipping failure PR"
                        );
                    }

                    PipelineResult {
                        story_key: story_key.clone(),
                        status: StoryStatus::Error,
                        pr_url: None,
                        error_detail: Some(error),
                        fatal,
                    }
                } else {
                    tracing::error!(
                        action = "recovery_session_failed",
                        story_key = %story_key,
                        error = %error,
                        "Recovered session failed mid-work — creating failure PR"
                    );

                    // Failure PR (push partial work first)
                    let branch = format!("story/{story_key}");

                    if let Err(e) = self.push_branch(&branch).await {
                        tracing::warn!(
                            action = "recovery_failure_push_failed",
                            story_key = %story_key,
                            branch = %branch,
                            error = %e,
                            "Git push failed for recovery failure branch — attempting PR anyway"
                        );
                    }

                    let decisions_section = format_pr_decisions_section(&decisions);
                    let pr_title = build_pr_title(&story_key, &story_title, true);
                    let pr_body = build_pr_description(&PrDescriptionParams {
                        story_key: story_key.clone(),
                        story_title: story_title.clone(),
                        outcome_summary: "failed (crash recovery attempted)".to_string(),
                        decisions_section,
                        failure_details: Some(error.clone()),
                        pr_summary: None,
                    });
                    let pr_params = CreatePrParams {
                        title: pr_title,
                        body: pr_body,
                        source_branch: branch.clone(),
                        target_branch: self.config.git_provider.target_branch.clone(),
                    };

                    match self.git_provider.create_pr(pr_params).await {
                        Ok(pr_info) => PipelineResult {
                            story_key: story_key.clone(),
                            status: StoryStatus::Error,
                            pr_url: Some(pr_info.url.clone()),
                            error_detail: Some(error),
                            fatal: false,
                        },
                        Err(pr_err) => {
                            tracing::error!(
                                action = "recovery_failure_pr_creation_failed",
                                story_key = %story_key,
                                branch = %branch,
                                error = %pr_err,
                                "Failed to create failure PR after recovery"
                            );
                            PipelineResult {
                                story_key: story_key.clone(),
                                status: StoryStatus::Error,
                                pr_url: None,
                                error_detail: Some(format!(
                                    "Recovery session failed: {error}. PR creation also failed: {pr_err}. Branch: {branch}"
                                )),
                                fatal: false,
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sprint-status git helpers
// ---------------------------------------------------------------------------

/// Check whether sprint-status.yaml has uncommitted changes in the working tree.
///
/// Returns `true` if the file is dirty (modified but not committed). Uses
/// `git diff --name-only` which is cheap and non-destructive.
async fn has_uncommitted_sprint_status(repo_path: &str, sprint_status_path: &Path) -> bool {
    // Check staged + unstaged changes
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["status", "--porcelain", "--"])
        .arg(sprint_status_path)
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            !stdout.trim().is_empty()
        }
        _ => false, // If git fails, assume clean — don't block the pipeline
    }
}

/// Robustly commit sprint-status.yaml changes.
///
/// Respects the user's full git configuration (hooks, GPG signing, etc.).
/// Checks for staged changes before committing to avoid "nothing to commit"
/// failures. Captures and logs stderr on failure so the root cause is
/// diagnosable.
///
/// This is critical for preventing the infinite-loop bug where `git checkout`
/// for the next story discards uncommitted sprint-status changes, causing
/// completed stories to revert to `ready-for-dev`.
async fn commit_sprint_status(
    repo_path: &str,
    sprint_status_path: &Path,
    commit_msg: &str,
) -> Result<(), String> {
    let path_str = sprint_status_path.to_str().unwrap_or("sprint-status.yaml");

    // Stage the file
    let add_output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["add", "--", path_str])
        .output()
        .await
        .map_err(|e| format!("git add exec failed: {e}"))?;

    if !add_output.status.success() {
        let stderr = String::from_utf8_lossy(&add_output.stderr);
        return Err(format!("git add failed: {stderr}"));
    }

    // Check if there's actually something to commit (avoid "nothing to commit" failure)
    let diff_output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["diff", "--cached", "--quiet", "--", path_str])
        .output()
        .await
        .map_err(|e| format!("git diff --cached exec failed: {e}"))?;

    if diff_output.status.success() {
        // Exit code 0 means no staged changes — nothing to commit
        tracing::debug!(
            action = "sprint_status_no_changes",
            "sprint-status.yaml has no staged changes to commit"
        );
        return Ok(());
    }

    // Commit with --no-verify (skip hooks) and --no-gpg-sign (skip signing)
    let commit_output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["commit", "-m", commit_msg])
        .output()
        .await
        .map_err(|e| format!("git commit exec failed: {e}"))?;

    if !commit_output.status.success() {
        let stderr = String::from_utf8_lossy(&commit_output.stderr);
        let stdout = String::from_utf8_lossy(&commit_output.stdout);
        return Err(format!(
            "git commit failed (exit {}): stderr={stderr}, stdout={stdout}",
            commit_output.status
        ));
    }

    tracing::info!(
        action = "sprint_status_committed",
        "sprint-status.yaml changes committed successfully"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Helper Functions
// ---------------------------------------------------------------------------

/// Detect infrastructure errors where the session never started.
///
/// These errors mean no partial work exists on the branch — creating a failure
/// PR would be pointless noise. Covers auth failures, config errors, branch setup
/// failures, and provider setup issues.
fn is_infra_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("token exchange failed")
        || lower.contains("authentication failed")
        || lower.contains("bad credentials")
        || lower.contains("http 401")
        || lower.contains("http 403")
        || lower.contains("provider setup failed")
        || lower.contains("agent build failed")
        || lower.contains("recovery agent build failed")
        || lower.contains("branch setup failed")
        || lower.contains("branch setup panicked")
        || lower.contains("failed to resolve api key")
        || lower.contains("unsupported provider")
        || lower.contains("initial chat failed")
        || lower.contains("agent activation failed")
        || lower.contains("wal creation failed")
}

/// Detect authentication errors that should halt the daemon entirely.
///
/// If credentials are invalid, every subsequent story will fail the same way.
/// The daemon should stop, notify the human, and wait for creds to be fixed.
fn is_auth_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("token exchange failed")
        || lower.contains("authentication failed")
        || lower.contains("bad credentials")
        || lower.contains("http 401")
        || lower.contains("http 403")
        || lower.contains("failed to resolve api key")
}

/// Convert a kebab-case label to a human-readable title.
///
/// Splits on `-`, capitalizes the first letter of each word, and joins with spaces.
///
/// # Examples
/// - `"telegram-notifications"` → `"Telegram Notifications"`
/// - `"scaffolding"` → `"Scaffolding"`
/// - `"http-retry-error-resilience"` → `"Http Retry Error Resilience"`
fn story_title_from_label(label: &str) -> String {
    label
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build a [`RunSummary`] from a slice of [`PipelineResult`]s.
fn build_run_summary(results: &[PipelineResult]) -> RunSummary {
    let mut completed = 0usize;
    let mut blocked = 0usize;
    let mut errored = 0usize;
    let fatal = results.iter().any(|r| r.fatal);

    let stories: Vec<StoryNotification> = results
        .iter()
        .map(|r| {
            match r.status {
                StoryStatus::Completed => completed += 1,
                StoryStatus::Blocked => blocked += 1,
                StoryStatus::Error => errored += 1,
            }

            StoryNotification {
                story_id: r.story_key.split('-').take(2).collect::<Vec<_>>().join("."),
                story_key: r.story_key.clone(),
                status: r.status.clone(),
                pr_url: r.pr_url.clone(),
                reason: r.error_detail.clone(),
            }
        })
        .collect();

    RunSummary {
        total_processed: results.len(),
        stories,
        completed,
        blocked,
        errored,
        fatal,
    }
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // PipelineResult construction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_pipeline_result_completed_fields() {
        let result = PipelineResult {
            story_key: "1-1-scaffolding".to_string(),
            status: StoryStatus::Completed,
            pr_url: Some("https://github.com/test/repo/pull/1".to_string()),
            error_detail: None,
            fatal: false,
        };
        assert_eq!(result.story_key, "1-1-scaffolding");
        assert_eq!(result.status, StoryStatus::Completed);
        assert!(result.pr_url.is_some());
        assert!(result.error_detail.is_none());
        assert!(!result.fatal);
    }

    #[test]
    fn test_pipeline_result_failed_fields() {
        let result = PipelineResult {
            story_key: "6-2-http-retry".to_string(),
            status: StoryStatus::Error,
            pr_url: None,
            error_detail: Some("LLM provider down".to_string()),
            fatal: false,
        };
        assert_eq!(result.status, StoryStatus::Error);
        assert!(result.pr_url.is_none());
        assert_eq!(result.error_detail.as_deref(), Some("LLM provider down"));
        assert!(!result.fatal);
    }

    #[test]
    fn test_pipeline_result_blocked_fields() {
        let result = PipelineResult {
            story_key: "3-3-escalation".to_string(),
            status: StoryStatus::Blocked,
            pr_url: None,
            error_detail: Some("Escalated: question — reason".to_string()),
            fatal: false,
        };
        assert_eq!(result.status, StoryStatus::Blocked);
        assert!(
            result
                .error_detail
                .as_deref()
                .unwrap()
                .contains("Escalated")
        );
        assert!(!result.fatal);
    }

    #[test]
    fn test_pipeline_result_fatal_auth_error() {
        let result = PipelineResult {
            story_key: "7-1-integration-tests".to_string(),
            status: StoryStatus::Error,
            pr_url: None,
            error_detail: Some("Copilot token exchange failed: HTTP 401".to_string()),
            fatal: true,
        };
        assert_eq!(result.status, StoryStatus::Error);
        assert!(result.fatal);
        assert!(result.pr_url.is_none());
    }

    #[test]
    fn test_is_infra_error_auth_patterns() {
        assert!(is_infra_error("Copilot token exchange failed: HTTP 401"));
        assert!(is_infra_error("Authentication failed: bad token"));
        assert!(is_infra_error("Bad credentials"));
        assert!(is_infra_error("HTTP 401"));
        assert!(is_infra_error("HTTP 403"));
        assert!(is_infra_error("Provider setup failed: missing key"));
        assert!(is_infra_error("Agent build failed: connection refused"));
        assert!(is_infra_error("Branch setup failed: git error"));
        assert!(is_infra_error("Branch setup panicked: thread panic"));
        assert!(is_infra_error("Failed to resolve API key"));
        assert!(is_infra_error("Unsupported provider in WAL: foobar"));
        assert!(is_infra_error(
            "Initial chat failed: CompletionError: 503 Service Unavailable"
        ));
        assert!(is_infra_error("Agent activation failed: connection reset"));
        assert!(is_infra_error("WAL creation failed: permission denied"));
    }

    #[test]
    fn test_is_infra_error_false_for_session_crashes() {
        assert!(!is_infra_error("Maximum turn limit exceeded (300)"));
        assert!(!is_infra_error("Chat loop failed: connection lost"));
        assert!(!is_infra_error("context_length_exceeded"));
        assert!(!is_infra_error("OOM killed"));
    }

    #[test]
    fn test_is_auth_error_subset_of_infra() {
        assert!(is_auth_error("Copilot token exchange failed: HTTP 401"));
        assert!(is_auth_error("Authentication failed: expired"));
        assert!(is_auth_error("Bad credentials"));
        assert!(is_auth_error("HTTP 401"));
        assert!(is_auth_error("HTTP 403"));
        assert!(is_auth_error("Failed to resolve API key"));
        // infra but NOT auth:
        assert!(!is_auth_error("Agent build failed: timeout"));
        assert!(!is_auth_error("Branch setup failed: conflict"));
        assert!(!is_auth_error("Unsupported provider in WAL: xyz"));
    }

    #[test]
    fn test_run_summary_fatal_flag_propagated() {
        let results = vec![
            PipelineResult {
                story_key: "a".to_string(),
                status: StoryStatus::Completed,
                pr_url: Some("url".to_string()),
                error_detail: None,
                fatal: false,
            },
            PipelineResult {
                story_key: "b".to_string(),
                status: StoryStatus::Error,
                pr_url: None,
                error_detail: Some("token exchange failed: HTTP 401".to_string()),
                fatal: true,
            },
        ];
        let summary = build_run_summary(&results);
        assert!(summary.fatal, "RunSummary should propagate fatal flag");
        assert_eq!(summary.total_processed, 2);
        assert_eq!(summary.errored, 1);
    }

    #[test]
    fn test_run_summary_no_fatal_when_all_ok() {
        let results = vec![PipelineResult {
            story_key: "a".to_string(),
            status: StoryStatus::Completed,
            pr_url: Some("url".to_string()),
            error_detail: None,
            fatal: false,
        }];
        let summary = build_run_summary(&results);
        assert!(!summary.fatal);
    }

    // -----------------------------------------------------------------------
    // PipelineError display tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_pipeline_error_display_init() {
        let err = PipelineError::Init {
            reason: "unsupported provider".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("initialization failed"));
        assert!(msg.contains("unsupported provider"));
    }

    #[test]
    fn test_pipeline_error_display_session() {
        let err = PipelineError::Session {
            story_key: "1-1-scaffolding".to_string(),
            error: "timeout".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("1-1-scaffolding"));
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn test_pipeline_error_display_pr_creation() {
        let err = PipelineError::PrCreation {
            story_key: "2-1-polling".to_string(),
            branch: "story/2-1-polling".to_string(),
            reason: "403 Forbidden".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("2-1-polling"));
        assert!(msg.contains("story/2-1-polling"));
        assert!(msg.contains("403 Forbidden"));
    }

    #[test]
    fn test_pipeline_error_display_notification() {
        let err = PipelineError::Notification {
            reason: "Telegram API timeout".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("Notification failed"));
        assert!(msg.contains("Telegram API timeout"));
    }

    #[test]
    fn test_pipeline_error_display_review() {
        let err = PipelineError::Review {
            story_key: "5-2-review".to_string(),
            error: "agent crash".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("5-2-review"));
        assert!(msg.contains("agent crash"));
    }

    #[test]
    fn test_pipeline_error_display_pr_comment() {
        let err = PipelineError::PrComment {
            pr_id: "42".to_string(),
            reason: "rate limited".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("42"));
        assert!(msg.contains("rate limited"));
    }

    #[test]
    fn test_pipeline_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PipelineError>();
    }

    // -----------------------------------------------------------------------
    // StoryPipeline Send + Sync
    // -----------------------------------------------------------------------

    #[test]
    fn test_story_pipeline_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<StoryPipeline>();
    }

    // -----------------------------------------------------------------------
    // story_title_from_label tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_story_title_from_label_simple() {
        assert_eq!(
            story_title_from_label("telegram-notifications"),
            "Telegram Notifications"
        );
    }

    #[test]
    fn test_story_title_from_label_single_word() {
        assert_eq!(story_title_from_label("scaffolding"), "Scaffolding");
    }

    #[test]
    fn test_story_title_from_label_multi_word() {
        assert_eq!(
            story_title_from_label("http-retry-error-resilience"),
            "Http Retry Error Resilience"
        );
    }

    #[test]
    fn test_story_title_from_label_empty() {
        assert_eq!(story_title_from_label(""), "");
    }

    // -----------------------------------------------------------------------
    // build_run_summary tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_summary_from_pipeline_results() {
        let results = vec![
            PipelineResult {
                story_key: "1-1-scaffolding".to_string(),
                status: StoryStatus::Completed,
                pr_url: Some("https://github.com/test/repo/pull/1".to_string()),
                error_detail: None,
                fatal: false,
            },
            PipelineResult {
                story_key: "2-1-polling".to_string(),
                status: StoryStatus::Error,
                pr_url: None,
                error_detail: Some("timeout".to_string()),
                fatal: false,
            },
            PipelineResult {
                story_key: "3-3-escalation".to_string(),
                status: StoryStatus::Blocked,
                pr_url: None,
                error_detail: Some("Escalated".to_string()),
                fatal: false,
            },
        ];

        let summary = build_run_summary(&results);
        assert_eq!(summary.total_processed, 3);
        assert_eq!(summary.completed, 1);
        assert_eq!(summary.errored, 1);
        assert_eq!(summary.blocked, 1);
        assert_eq!(summary.stories.len(), 3);
        assert!(!summary.fatal);
        assert_eq!(summary.stories[0].story_key, "1-1-scaffolding");
        assert_eq!(summary.stories[0].status, StoryStatus::Completed);
        assert_eq!(summary.stories[1].status, StoryStatus::Error);
        assert_eq!(summary.stories[2].status, StoryStatus::Blocked);
    }

    #[test]
    fn test_run_summary_all_completed() {
        let results = vec![
            PipelineResult {
                story_key: "a".to_string(),
                status: StoryStatus::Completed,
                pr_url: Some("url1".to_string()),
                error_detail: None,
                fatal: false,
            },
            PipelineResult {
                story_key: "b".to_string(),
                status: StoryStatus::Completed,
                pr_url: Some("url2".to_string()),
                error_detail: None,
                fatal: false,
            },
        ];

        let summary = build_run_summary(&results);
        assert_eq!(summary.total_processed, 2);
        assert_eq!(summary.completed, 2);
        assert_eq!(summary.blocked, 0);
        assert_eq!(summary.errored, 0);
    }

    #[test]
    fn test_run_summary_empty() {
        let results: Vec<PipelineResult> = vec![];
        let summary = build_run_summary(&results);
        assert_eq!(summary.total_processed, 0);
        assert_eq!(summary.completed, 0);
        assert_eq!(summary.blocked, 0);
        assert_eq!(summary.errored, 0);
        assert!(summary.stories.is_empty());
    }

    #[test]
    fn test_run_summary_story_id_extraction() {
        let results = vec![PipelineResult {
            story_key: "6-1-telegram-notifications".to_string(),
            status: StoryStatus::Completed,
            pr_url: None,
            error_detail: None,
            fatal: false,
        }];

        let summary = build_run_summary(&results);
        assert_eq!(summary.stories[0].story_id, "6.1");
    }

    // -----------------------------------------------------------------------
    // commit_sprint_status / has_uncommitted_sprint_status tests
    // -----------------------------------------------------------------------

    /// Helper: init a minimal git repo and return the TempDir.
    fn init_test_repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let p = dir.path();

        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["init"])
            .output()
            .expect("git init");
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .expect("git config email");
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["config", "user.name", "Test"])
            .output()
            .expect("git config name");

        // Initial commit so HEAD exists
        let f = p.join("README.md");
        std::fs::write(&f, "# test\n").unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["add", "."])
            .output()
            .expect("git add");
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["commit", "--no-gpg-sign", "-m", "init"])
            .output()
            .expect("git commit");

        dir
    }

    #[tokio::test]
    async fn test_has_uncommitted_sprint_status_clean() {
        let dir = init_test_repo();
        let p = dir.path();
        let ss = p.join("sprint-status.yaml");
        std::fs::write(&ss, "story-1: done\n").unwrap();

        // Stage and commit so it's clean
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["add", "sprint-status.yaml"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["commit", "--no-gpg-sign", "-m", "add ss"])
            .output()
            .unwrap();

        let dirty = has_uncommitted_sprint_status(p.to_str().unwrap(), &ss).await;
        assert!(!dirty, "File should be clean after commit");
    }

    #[tokio::test]
    async fn test_has_uncommitted_sprint_status_dirty() {
        let dir = init_test_repo();
        let p = dir.path();
        let ss = p.join("sprint-status.yaml");

        // Commit initial version
        std::fs::write(&ss, "story-1: in-progress\n").unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["commit", "--no-gpg-sign", "-m", "add ss"])
            .output()
            .unwrap();

        // Modify without committing
        std::fs::write(&ss, "story-1: done\n").unwrap();

        let dirty = has_uncommitted_sprint_status(p.to_str().unwrap(), &ss).await;
        assert!(dirty, "File should be dirty after modification");
    }

    #[tokio::test]
    async fn test_commit_sprint_status_happy_path() {
        let dir = init_test_repo();
        let p = dir.path();
        let ss = p.join("sprint-status.yaml");

        // Create and commit initial version
        std::fs::write(&ss, "story-1: in-progress\n").unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["commit", "--no-gpg-sign", "-m", "init ss"])
            .output()
            .unwrap();

        // Modify file (simulating Phase 7 write)
        std::fs::write(&ss, "story-1: done\n").unwrap();
        assert!(has_uncommitted_sprint_status(p.to_str().unwrap(), &ss).await);

        // Commit via our helper
        let result = commit_sprint_status(p.to_str().unwrap(), &ss, "chore: mark done").await;
        assert!(result.is_ok(), "commit should succeed: {result:?}");

        // File should now be clean
        assert!(
            !has_uncommitted_sprint_status(p.to_str().unwrap(), &ss).await,
            "File should be clean after commit_sprint_status"
        );
    }

    #[tokio::test]
    async fn test_commit_sprint_status_no_changes_is_ok() {
        let dir = init_test_repo();
        let p = dir.path();
        let ss = p.join("sprint-status.yaml");

        // Create and commit — no subsequent modification
        std::fs::write(&ss, "story-1: done\n").unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["commit", "--no-gpg-sign", "-m", "init"])
            .output()
            .unwrap();

        // Calling commit_sprint_status when clean should be a no-op Ok
        let result = commit_sprint_status(p.to_str().unwrap(), &ss, "chore: noop").await;
        assert!(result.is_ok(), "no-op commit should succeed: {result:?}");
    }

    #[tokio::test]
    async fn test_commit_sprint_status_survives_branch_checkout() {
        let dir = init_test_repo();
        let p = dir.path();
        let ss = p.join("sprint-status.yaml");

        // Initial sprint-status on main
        std::fs::write(&ss, "7-7: ready-for-dev\n7-8: blocked\n").unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["commit", "--no-gpg-sign", "-m", "init ss"])
            .output()
            .unwrap();

        // Create story branch (simulating story 7-7 session)
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["checkout", "-b", "story/7-7"])
            .output()
            .unwrap();

        // Simulate Phase 7: mark done
        std::fs::write(&ss, "7-7: done\n7-8: ready-for-dev\n").unwrap();

        // Commit via our robust helper (this is the fix)
        commit_sprint_status(p.to_str().unwrap(), &ss, "chore: mark 7-7 done")
            .await
            .expect("commit should succeed");

        // Now checkout a NEW branch from story/7-7 (simulating next story)
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["checkout", "-b", "story/7-8", "story/7-7"])
            .output()
            .unwrap();

        // The "done" status should SURVIVE the checkout because it was committed
        let content = std::fs::read_to_string(&ss).unwrap();
        assert!(
            content.contains("7-7: done"),
            "7-7 should still be 'done' after checkout to story/7-8, got: {content}"
        );
        assert!(
            content.contains("7-8: ready-for-dev"),
            "7-8 should be 'ready-for-dev' after checkout, got: {content}"
        );
    }

    #[tokio::test]
    async fn test_uncommitted_changes_lost_on_checkout_without_commit() {
        // This test demonstrates the BUG scenario — without commit,
        // sprint-status changes are lost on branch checkout.
        let dir = init_test_repo();
        let p = dir.path();
        let ss = p.join("sprint-status.yaml");

        // Initial sprint-status
        std::fs::write(&ss, "7-7: ready-for-dev\n").unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["commit", "--no-gpg-sign", "-m", "init ss"])
            .output()
            .unwrap();

        // Create story branch
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["checkout", "-b", "story/7-7"])
            .output()
            .unwrap();

        // Agent commits "review" status during session
        std::fs::write(&ss, "7-7: review\n").unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["commit", "--no-gpg-sign", "-m", "agent: review"])
            .output()
            .unwrap();

        // Phase 7 writes "done" but does NOT commit (the old bug)
        std::fs::write(&ss, "7-7: done\n").unwrap();

        // Create new branch from story/7-7 — this checkouts and resets working tree
        // The uncommitted "done" change should be carried because the file
        // differs from the committed version on story/7-7 ("review" vs "done")
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(p)
            .args(["checkout", "-b", "story/7-8", "story/7-7"])
            .output()
            .unwrap();

        // On some git versions, checkout might discard the change or carry it.
        // If the change survives, content is "done". If lost, content is "review".
        let content = std::fs::read_to_string(&ss).unwrap();
        if content.contains("7-7: review") {
            // Change was LOST — this confirms the bug scenario.
            // The fix (commit_sprint_status) prevents this.
        } else {
            // Change survived (git carried uncommitted changes).
            // This can happen on some platforms/configs — our fix is still
            // correct as a defense-in-depth measure.
        }
        // This test always passes — it documents the behavior, not asserts it,
        // because git's handling of dirty files during checkout varies.
    }
}
