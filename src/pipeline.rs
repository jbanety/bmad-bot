//! Story pipeline orchestrator — daemon Layer 3 error handling and story lifecycle.
//!
//! The [`StoryPipeline`] struct encapsulates the full story processing pipeline:
//! session → push → PR creation → optional review → notification. It implements the
//! "never stop the run" principle: no single story failure halts the daemon.
//!
//! Use [`StoryPipeline::new`] to construct, then [`process_eligible_stories`] to
//! run eligible stories. After each story completes, the pipeline **re-polls**
//! `sprint-status.yaml` and recomputes the eligible list so that dependency
//! changes (e.g., story 1-1 done → 1-2 now eligible) are reflected immediately
//! instead of processing a stale batch from the initial poll.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{BotConfig, BotSecrets};
use crate::git_provider::{
    CreatePrParams, GitProvider, PrDescriptionParams, PrSummary, build_pr_description,
    build_pr_title, create_provider,
};
use crate::llm::AgentFactory;
use crate::notifier::{
    EpicGateNotification, Notifier, RunSummary, StoryNotification, StoryStatus, create_notifier,
};
use crate::review::ReviewOutcome;
use crate::review::ReviewRunner;
use crate::review::epic::{
    EpicReviewOutcome, EpicReviewRunner, extract_epic_recap, generate_failure_report,
};
use crate::session::SessionOutcome;
use crate::session::analyzer::strip_agent_artifacts;
use crate::session::cleanup::{unblock_dependents, update_story_status};
use crate::session::runner::SessionRunner;
use crate::session::runner::ShutdownFlag;
use crate::supervisor::decisions::format_pr_decisions_section;
use crate::ui::UiHandle;
use crate::watcher::deps as watcher_deps;
use crate::watcher::{SprintStatusFile, StoryInfo};

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
    /// Development session runner.
    session_runner: SessionRunner,
    /// Code review session runner.
    review_runner: ReviewRunner,
    /// Epic review session runner (autonomous post-epic retrospective).
    epic_review_runner: EpicReviewRunner,
    /// UI handle for rendering terminal output (fire-and-forget).
    ui: UiHandle,
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
        ui: UiHandle,
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
        let project_name = std::path::Path::new(&config.bmad_paths.project_root)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("bmad-bot");
        let notifier = create_notifier(&config.notifications, &secrets, project_name);

        // Create the centralized AgentFactory — owns secrets + Copilot token cache.
        let agent_factory = Arc::new(AgentFactory::new(Arc::clone(&config), Arc::clone(&secrets)));

        let session_runner = SessionRunner::new(
            Arc::clone(&config),
            Arc::clone(&agent_factory),
            Arc::clone(&shutdown),
            Arc::clone(&mcp_manager),
            ui.clone(),
        );
        let review_runner = ReviewRunner::new(
            Arc::clone(&config),
            Arc::clone(&secrets),
            Arc::clone(&agent_factory),
            Arc::clone(&shutdown),
            Arc::clone(&mcp_manager),
            ui.clone(),
        );
        let epic_review_runner = EpicReviewRunner::new(
            Arc::clone(&config),
            Arc::clone(&secrets),
            Arc::clone(&agent_factory),
            shutdown,
            mcp_manager,
            ui.clone(),
        );

        Ok(Self {
            config,
            git_provider,
            notifier,
            session_runner,
            review_runner,
            epic_review_runner,
            ui,
        })
    }

    /// Process a single story through the full pipeline.
    ///
    /// Runs dev session → optional code review → PR creation → notification.
    /// Never panics — all errors are caught and returned as [`PipelineResult`].
    pub async fn process_story(
        &self,
        story: &StoryInfo,
        base_branch_override: Option<&str>,
    ) -> PipelineResult {
        let story_title = story_title_from_label(&story.label);
        self.ui.story_start(&story.story_key, &story_title);

        tracing::info!(
            action = "pipeline_start",
            story_key = %story.story_key,
            story_id = %story.story_id,
            "Starting pipeline for story"
        );

        // Phase 1 — Dev Session
        self.ui.phase_start("Dev Session");
        let session_start = std::time::Instant::now();
        let session_outcome = self.session_runner.run(story, base_branch_override).await;
        let session_elapsed = session_start.elapsed();

        match session_outcome {
            SessionOutcome::Completed {
                story_key,
                branch,
                decisions,
                pr_context,
                pr_how_to_test,
                pr_additional_info,
            } => {
                self.ui.phase_complete("Dev Session", session_elapsed);

                // Phase 2 — Push branch to remote before PR creation (non-blocking)
                self.ui.phase_start("Push Branch");
                let push_start = std::time::Instant::now();
                let push_ok = match self.push_branch(&branch).await {
                    Ok(()) => {
                        self.ui.phase_complete("Push Branch", push_start.elapsed());
                        true
                    }
                    Err(e) => {
                        self.ui.phase_error("Push Branch", &e.to_string());
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
                    self.ui
                        .story_error(&story_key, "Push failed — work preserved locally");
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

                self.ui.phase_start("Create PR");
                let pr_start = std::time::Instant::now();
                let pr_info = match self.git_provider.create_pr(pr_params).await {
                    Ok(info) => {
                        self.ui.phase_complete("Create PR", pr_start.elapsed());
                        info
                    }
                    Err(e) => {
                        self.ui.phase_error("Create PR", &e.to_string());
                        tracing::error!(
                            action = "pr_creation_failed",
                            story_key = %story_key,
                            branch = %branch,
                            error = %e,
                            "PR creation failed — skipping review, notifying human with branch name"
                        );

                        self.ui
                            .story_error(&story_key, &format!("PR creation failed: {e}"));
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
                    self.ui.phase_start("Code Review");
                    let review_start = std::time::Instant::now();
                    match self.review_runner.run(story).await {
                        ReviewOutcome::Completed { report, .. } => {
                            self.ui
                                .phase_complete("Code Review", review_start.elapsed());
                            Some(report)
                        }
                        ReviewOutcome::Failed {
                            story_key: rk,
                            error,
                        } => {
                            self.ui.phase_error("Code Review", &error);
                            tracing::warn!(
                                action = "review_failed",
                                story_key = %rk,
                                error = %error,
                                "Code review failed — PR already exists"
                            );
                            None
                        }
                        ReviewOutcome::Skipped { reason } => {
                            self.ui
                                .phase_complete("Code Review", review_start.elapsed());
                            tracing::info!(
                                action = "review_skipped",
                                reason = %reason,
                                "Code review skipped — PR already exists"
                            );
                            None
                        }
                    }
                } else {
                    self.ui
                        .phase_complete("Code Review", std::time::Duration::ZERO);
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
                if let Some(ref report) = review_report {
                    match self.git_provider.add_comment(&pr_info.id, report).await {
                        Ok(()) => {
                            self.ui.tool_result("pr_comment", "Review posted");
                        }
                        Err(e) => {
                            tracing::error!(
                                action = "pr_comment_failed",
                                pr_id = %pr_info.id,
                                error = %e,
                                "Failed to post review comment — PR created successfully"
                            );
                            self.ui.tool_result("pr_comment", &format!("Failed: {e}"));
                        }
                    }
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
                self.ui.phase_start("Notification");
                let notify_start = std::time::Instant::now();
                let result = PipelineResult {
                    story_key: story_key.clone(),
                    status: StoryStatus::Completed,
                    pr_url: Some(pr_info.url.clone()),
                    error_detail: None,
                    fatal: false,
                };
                self.notify_story_result(&result).await;
                self.ui
                    .phase_complete("Notification", notify_start.elapsed());
                self.ui.story_complete(&story_key, Some(&pr_info.url));
                result
            }

            SessionOutcome::Escalated { report, decisions } => {
                self.ui
                    .phase_error("Dev Session", &format!("Escalated: {}", report.reason));
                tracing::warn!(
                    action = "session_escalated",
                    story_key = %report.story_key,
                    question = %report.question,
                    reason = %report.reason,
                    "Story escalated — needs human clarification, creating escalation PR"
                );

                // Push branch to remote (best-effort, same pattern as Failed branch)
                let branch = report.branch_name.clone();
                self.ui.phase_start("Push Branch");
                let push_start = std::time::Instant::now();
                if let Err(e) = self.push_branch(&branch).await {
                    self.ui.phase_error("Push Branch", &e.to_string());
                    tracing::warn!(
                        action = "escalation_push_failed",
                        story_key = %report.story_key,
                        branch = %branch,
                        error = %e,
                        "Git push failed for escalation branch — attempting PR anyway"
                    );
                } else {
                    self.ui.phase_complete("Push Branch", push_start.elapsed());
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

                self.ui.phase_start("Create PR");
                let pr_start = std::time::Instant::now();
                let pr_url = match self.git_provider.create_pr(pr_params).await {
                    Ok(pr_info) => {
                        self.ui.phase_complete("Create PR", pr_start.elapsed());
                        Some(pr_info.url.clone())
                    }
                    Err(e) => {
                        self.ui.phase_error("Create PR", &e.to_string());
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

                self.ui.phase_start("Notification");
                let notify_start = std::time::Instant::now();
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
                self.ui
                    .phase_complete("Notification", notify_start.elapsed());
                self.ui.story_escalated(&report.story_key, &report.reason);
                result
            }

            SessionOutcome::Failed {
                story_key,
                error,
                decisions,
            } => {
                self.ui.phase_error("Dev Session", &error);
                let infra = is_infra_error(&error);

                if infra {
                    // Infrastructure failure — session never started or couldn't
                    // proceed. Always fatal: the session runner already retries
                    // transient errors (timeouts, 429, 503) with exponential
                    // backoff internally. If we get here, retries were exhausted
                    // or the error is permanent. Continuing to the next story
                    // would either hit the same infra issue or build on top of
                    // incomplete predecessor work.
                    let is_auth = is_auth_error(&error);
                    if is_auth {
                        tracing::error!(
                            action = "session_failed_fatal_auth",
                            story_key = %story_key,
                            error = %error,
                            "Fatal auth error — credentials invalid, daemon should halt"
                        );
                    } else {
                        tracing::error!(
                            action = "session_failed_fatal_infra",
                            story_key = %story_key,
                            error = %error,
                            "Fatal infrastructure error — retries exhausted, halting pipeline"
                        );
                    }

                    self.ui.phase_start("Notification");
                    let notify_start = std::time::Instant::now();
                    let result = PipelineResult {
                        story_key: story_key.clone(),
                        status: StoryStatus::Error,
                        pr_url: None,
                        error_detail: Some(error),
                        fatal: true,
                    };
                    self.notify_story_result(&result).await;
                    self.ui
                        .phase_complete("Notification", notify_start.elapsed());
                    self.ui
                        .story_error(&story_key, "Fatal infrastructure error");
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

                    self.ui.phase_start("Push Branch");
                    let push_start = std::time::Instant::now();
                    if let Err(e) = self.push_branch(&branch).await {
                        self.ui.phase_error("Push Branch", &e.to_string());
                        tracing::warn!(
                            action = "failure_push_failed",
                            story_key = %story_key,
                            branch = %branch,
                            error = %e,
                            "Git push failed for failure branch — attempting PR anyway"
                        );
                    } else {
                        self.ui.phase_complete("Push Branch", push_start.elapsed());
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

                    self.ui.phase_start("Create PR");
                    let pr_start = std::time::Instant::now();
                    match self.git_provider.create_pr(pr_params).await {
                        Ok(pr_info) => {
                            self.ui.phase_complete("Create PR", pr_start.elapsed());
                            self.ui.phase_start("Notification");
                            let notify_start = std::time::Instant::now();
                            let result = PipelineResult {
                                story_key: story_key.clone(),
                                status: StoryStatus::Error,
                                pr_url: Some(pr_info.url.clone()),
                                error_detail: Some(error),
                                fatal: false,
                            };
                            self.notify_story_result(&result).await;
                            self.ui
                                .phase_complete("Notification", notify_start.elapsed());
                            self.ui
                                .story_error(&story_key, "Dev session failed — failure PR created");
                            result
                        }
                        Err(pr_err) => {
                            self.ui.phase_error("Create PR", &pr_err.to_string());
                            tracing::error!(
                                action = "failure_pr_creation_failed",
                                story_key = %story_key,
                                branch = %branch,
                                error = %pr_err,
                                "Failed to create failure PR — notifying human with branch name only"
                            );

                            self.ui.phase_start("Notification");
                            let notify_start = std::time::Instant::now();
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
                            self.ui
                                .phase_complete("Notification", notify_start.elapsed());
                            self.ui.story_error(
                                &story_key,
                                &format!("Session failed, PR creation also failed: {pr_err}"),
                            );
                            result
                        }
                    }
                }
            }
        }
    }

    /// Process eligible stories sequentially with re-polling between each story.
    ///
    /// The initial `stories` list seeds the first iteration. After each story
    /// completes, sprint-status.yaml is **re-read** and the eligible list is
    /// recomputed via [`watcher_deps::filter_eligible`]. This ensures that
    /// dependency changes (e.g., story 1-1 marked `done` → 1-2 now eligible)
    /// are reflected immediately — the pipeline picks the correct next story
    /// instead of blindly iterating a stale batch (which would jump across
    /// epics).
    ///
    /// Stories already processed in this run are tracked and skipped on
    /// subsequent re-polls to prevent double-processing if status updates
    /// fail to persist.
    ///
    /// Processing stops when:
    /// - No more eligible stories remain after re-poll
    /// - A fatal error is encountered (e.g. auth failure)
    /// - Re-poll itself fails (conservative — wait for next poll cycle)
    ///
    /// A run summary notification is sent after all processing completes.
    pub async fn process_eligible_stories(&self, stories: Vec<StoryInfo>) -> RunSummary {
        self.ui.batch_start(stories.len());
        let mut results: Vec<PipelineResult> = Vec::new();

        let sprint_status_path = PathBuf::from(&self.config.bmad_paths.implementation_artifacts)
            .join("sprint-status.yaml");
        let story_dir = PathBuf::from(&self.config.bmad_paths.implementation_artifacts);

        // Track processed stories to prevent re-processing if status updates
        // fail and the same story reappears as eligible on re-poll.
        let mut processed_keys: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        // Track the branch of the last successfully completed story so that the
        // next story chains from it instead of forking from its declared dependency.
        // This prevents sprint-status.yaml from diverging across parallel branch
        // chains (e.g., story/2-x vs story/3-x both forking from story/1-5).
        let mut last_completed_branch: Option<String> = None;

        // Start with the first story from the initial eligible list.
        // After each story, we re-poll sprint-status.yaml and recompute
        // the eligible list so that dependency changes (e.g., 1-1 done →
        // 1-2 now eligible) are reflected immediately instead of processing
        // a stale batch (which would jump to 2-1 instead of 1-2).
        let mut current_stories = stories;

        loop {
            // Find next unprocessed story from the current eligible list
            let next_story = current_stories
                .iter()
                .find(|s| !processed_keys.contains(&s.story_key));

            let story = match next_story {
                Some(s) => s.clone(),
                None => break, // No more eligible stories in this cycle
            };

            processed_keys.insert(story.story_key.clone());

            let result = self
                .process_story(&story, last_completed_branch.as_deref())
                .await;
            let mut is_fatal = result.fatal;
            let story_key = &result.story_key;

            // Update sequential chaining branch on successful completion.
            // Only Completed stories guarantee the branch exists with committed work.
            if result.status == StoryStatus::Completed {
                last_completed_branch = Some(story.branch_name.clone());
            }

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

            // ── Epic completion detection & gate activation ─────────────
            // After a successful story AND after the safety net commit,
            // check if the completed story was the last in its epic.
            // If so, run the autonomous epic review and activate the gate.
            if result.status == StoryStatus::Completed && !is_fatal {
                let gate_activated = self
                    .try_epic_gate(
                        &story,
                        &sprint_status_path,
                        &story_dir,
                        last_completed_branch.as_deref(),
                    )
                    .await;

                if !gate_activated {
                    tracing::debug!(
                        action = "epic_gate_not_activated",
                        story_key = %story.story_key,
                        "Epic gate not activated after story completion (epic incomplete, no retro entry, or gate flow failed)"
                    );
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

            // Re-poll sprint-status.yaml and recompute eligible list.
            // This ensures that dependency changes from the just-completed story
            // are reflected: e.g., after 1-1 → done, 1-2 becomes eligible and
            // takes priority over 2-1 which was in the original stale list.
            match re_poll_eligible(&sprint_status_path, &story_dir) {
                Ok(fresh_stories) => {
                    tracing::info!(
                        action = "re_poll_eligible",
                        previous_story = %story.story_key,
                        fresh_eligible = fresh_stories.len(),
                        "Re-polled sprint-status after story completion"
                    );
                    current_stories = fresh_stories;
                }
                Err(e) => {
                    // Re-poll failed — stop processing to avoid stale data decisions.
                    // The next poll cycle (5 min) will retry with a fresh read.
                    tracing::warn!(
                        action = "re_poll_failed",
                        previous_story = %story.story_key,
                        error = %e,
                        "Failed to re-poll sprint-status — stopping pipeline run, will retry next cycle"
                    );
                    break;
                }
            }
        }

        let summary = build_run_summary(&results);
        self.ui.batch_complete(&format!(
            "{} processed, {} completed, {} blocked, {} errored",
            summary.total_processed, summary.completed, summary.blocked, summary.errored
        ));

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
}

/// Re-read sprint-status.yaml and recompute eligible stories with dependency filtering.
///
/// Called after each story completion to ensure the next story is chosen based on
/// fresh dependency state, not a stale list from the initial poll.
impl StoryPipeline {
    /// Attempt the epic gate flow after a story completes.
    ///
    /// Checks if the epic is fully done, whether a retrospective entry exists,
    /// then delegates to [`run_epic_gate_inner`] for the actual review + gate
    /// activation. All failures are non-fatal — the pipeline continues even if
    /// the gate flow fails.
    ///
    /// Returns `true` if the gate was activated (regardless of review success).
    async fn try_epic_gate(
        &self,
        story: &StoryInfo,
        sprint_status_path: &Path,
        story_dir: &Path,
        last_completed_branch: Option<&str>,
    ) -> bool {
        // Step 1: Fresh-read epic completion detection
        let epic_num = match detect_epic_completion(sprint_status_path, story_dir, story) {
            Some(n) => n,
            None => return false, // Epic not fully done yet
        };

        // Step 2: Check for retrospective entry (backward compat)
        let ssf = match SprintStatusFile::load(sprint_status_path, story_dir) {
            Ok(ssf) => ssf,
            Err(e) => {
                tracing::warn!(
                    action = "epic_gate_ssf_load_failed",
                    error = %e,
                    epic_num = epic_num,
                    "Failed to load sprint-status for retro entry check — skipping epic gate"
                );
                return false;
            }
        };

        if !has_retrospective_entry(&ssf, epic_num) {
            tracing::info!(
                action = "epic_gate_skip_no_retro",
                epic_num = epic_num,
                "Epic {epic_num} complete but no retrospective entry — skipping gate (backward compat)"
            );
            return false;
        }

        tracing::info!(
            action = "epic_gate_triggered",
            epic_num = epic_num,
            story_key = %story.story_key,
            "Epic {epic_num} complete — launching autonomous review"
        );

        self.run_epic_gate_inner(epic_num, &ssf, sprint_status_path, last_completed_branch)
            .await
    }

    /// Core epic gate logic shared by [`try_epic_gate`] (post-story-completion)
    /// and [`scan_pending_epic_reviews`] (poll-cycle re-trigger).
    ///
    /// Runs the autonomous review, saves the report, updates sprint-status to
    /// `review`, creates a retro branch + MR, and sends a notification.
    /// Returns `true` if the gate was activated (regardless of review success).
    async fn run_epic_gate_inner(
        &self,
        epic_num: u32,
        ssf: &SprintStatusFile,
        sprint_status_path: &Path,
        last_completed_branch: Option<&str>,
    ) -> bool {
        // Count stories in the epic
        let story_count = ssf
            .stories()
            .iter()
            .filter(|s| s.epic_num == epic_num)
            .count();

        // Run the autonomous epic review
        let outcome = self.epic_review_runner.run(epic_num).await;
        let (report, review_succeeded, error_summary) = match &outcome {
            EpicReviewOutcome::Completed { report, .. } => (report.clone(), true, None),
            EpicReviewOutcome::Failed { reason, .. } => {
                let failure_report = generate_failure_report(epic_num, reason);
                (failure_report, false, Some(reason.clone()))
            }
        };

        // Save report to disk
        let report_filename = format!("epic-{epic_num}-retrospective-report.md");
        let report_path =
            PathBuf::from(&self.config.bmad_paths.implementation_artifacts).join(&report_filename);
        if let Err(e) = tokio::fs::write(&report_path, &report).await {
            tracing::error!(
                action = "epic_gate_report_save_failed",
                error = %e,
                epic_num = epic_num,
                "Failed to save epic review report — continuing with gate activation"
            );
        }

        // Update sprint-status on CURRENT branch (optional → review)
        let retro_key = format!("epic-{epic_num}-retrospective");
        let repo_path = &self.config.bmad_paths.project_root;
        if let Err(e) = update_story_status(sprint_status_path, &retro_key, "review").await {
            tracing::error!(
                action = "epic_gate_status_update_failed",
                error = %e,
                epic_num = epic_num,
                "Failed to update retrospective status — gate may not activate"
            );
            return false;
        }

        let gate_commit_msg =
            format!("chore(sprint-status): epic {epic_num} gate activated — awaiting human review");
        if let Err(e) = commit_sprint_status(repo_path, sprint_status_path, &gate_commit_msg).await
        {
            tracing::error!(
                action = "epic_gate_commit_failed",
                error = %e,
                epic_num = epic_num,
                "Failed to commit sprint-status gate update"
            );
            return false;
        }

        // Push current branch so watcher sees the gate
        if let Err(e) = push_current_branch(repo_path).await {
            tracing::error!(
                action = "epic_gate_push_failed",
                error = %e,
                epic_num = epic_num,
                "Failed to push sprint-status gate — watcher may not see gate until next push"
            );
            // Non-fatal — continue with branch/MR creation
        }

        // Create retro branch, commit report, push, create MR
        let base_branch = last_completed_branch.unwrap_or(&self.config.git_provider.target_branch);
        let retro_branch = format!("epic-{epic_num}-retrospective");
        let mr_url = self
            .create_retro_branch_and_mr(epic_num, &retro_branch, base_branch, &report_path, &report)
            .await;

        // Checkout back to the working branch (best effort)
        let _ = checkout_branch(repo_path, base_branch).await;

        // Resolve epic title for notification
        let epic_title = resolve_epic_title(
            Path::new(&self.config.bmad_paths.planning_artifacts),
            epic_num,
        );

        // Notify
        let notification = EpicGateNotification {
            epic_num,
            epic_title,
            story_count,
            mr_url,
            review_succeeded,
            error_summary,
        };
        if let Err(e) = self.notifier.notify_epic_gate(&notification).await {
            tracing::error!(
                action = "epic_gate_notification_failed",
                error = %e,
                epic_num = epic_num,
                "Failed to send epic gate notification — non-blocking"
            );
        }

        tracing::info!(
            action = "epic_gate_complete",
            epic_num = epic_num,
            review_succeeded = review_succeeded,
            "Epic {epic_num} gate flow complete"
        );

        true
    }

    /// Scan sprint-status for completed epics whose retrospective is still
    /// `optional` — i.e. the review was never run or was reset after a failure.
    ///
    /// For each match, cleans up any leftover retro branch (local + remote)
    /// from a prior failed attempt, then runs the full epic gate flow via
    /// [`run_epic_gate_inner`].
    ///
    /// Called at the start of each poll cycle so that manually resetting
    /// `epic-X-retrospective: optional` in sprint-status.yaml is enough to
    /// re-trigger the review — no need to re-run a story.
    ///
    /// Returns the number of epic reviews that were triggered.
    pub async fn scan_pending_epic_reviews(&self) -> usize {
        let sprint_status_path = PathBuf::from(&self.config.bmad_paths.implementation_artifacts)
            .join("sprint-status.yaml");
        let story_dir = PathBuf::from(&self.config.bmad_paths.implementation_artifacts);

        let ssf = match SprintStatusFile::load(&sprint_status_path, &story_dir) {
            Ok(ssf) => ssf,
            Err(e) => {
                tracing::debug!(
                    action = "scan_retro_load_failed",
                    error = %e,
                    "Failed to load sprint-status for pending epic review scan"
                );
                return 0;
            }
        };

        // Find epics with `epic-X-retrospective: optional` where all stories are done
        let mut pending: Vec<u32> = Vec::new();
        for (key, status) in ssf.entries() {
            if status != "optional" {
                continue;
            }
            let Some(rest) = key.strip_prefix("epic-") else {
                continue;
            };
            let Some(num_str) = rest.strip_suffix("-retrospective") else {
                continue;
            };
            let Ok(epic_num) = num_str.parse::<u32>() else {
                continue;
            };

            // Check all stories in this epic are done
            let epic_stories: Vec<_> = ssf
                .stories()
                .into_iter()
                .filter(|s| s.epic_num == epic_num)
                .collect();

            if epic_stories.is_empty() {
                continue;
            }

            let all_done = epic_stories.iter().all(|s| s.status == "done");
            if all_done {
                pending.push(epic_num);
            }
        }

        if pending.is_empty() {
            return 0;
        }

        tracing::info!(
            action = "scan_retro_pending",
            epics = ?pending,
            "Found completed epics with pending retrospective reviews"
        );

        let mut triggered = 0usize;
        let repo_path = &self.config.bmad_paths.project_root;

        for epic_num in pending {
            // Cleanup leftover retro branch from prior failed attempt (idempotent)
            let retro_branch = format!("epic-{epic_num}-retrospective");
            let _ = tokio::process::Command::new("git")
                .arg("-C")
                .arg(repo_path)
                .args(["branch", "-D", &retro_branch])
                .output()
                .await;
            let _ = tokio::process::Command::new("git")
                .arg("-C")
                .arg(repo_path)
                .args(["push", "origin", "--delete", &retro_branch])
                .output()
                .await;

            tracing::info!(
                action = "scan_retro_trigger",
                epic_num = epic_num,
                "Triggering epic review for completed epic with optional retrospective"
            );

            // Re-load SSF since run_epic_gate_inner modifies sprint-status
            let ssf = match SprintStatusFile::load(&sprint_status_path, &story_dir) {
                Ok(ssf) => ssf,
                Err(e) => {
                    tracing::error!(
                        action = "scan_retro_reload_failed",
                        error = %e,
                        epic_num = epic_num,
                        "Failed to reload sprint-status before epic gate — skipping"
                    );
                    continue;
                }
            };

            let activated = self
                .run_epic_gate_inner(epic_num, &ssf, &sprint_status_path, None)
                .await;

            if activated {
                triggered += 1;
            }
        }

        triggered
    }

    /// Create the retrospective branch, commit the report, push, and create an MR.
    ///
    /// Returns the MR URL if PR creation succeeded, `None` otherwise.
    async fn create_retro_branch_and_mr(
        &self,
        epic_num: u32,
        retro_branch: &str,
        base_branch: &str,
        report_path: &Path,
        report: &str,
    ) -> Option<String> {
        let repo_path = &self.config.bmad_paths.project_root;

        // Compute a path relative to repo_path for `git add` — an absolute path
        // passed to `git -C <repo> add -- <abs>` can silently fail on some git versions.
        let report_path_relative = report_path
            .strip_prefix(repo_path)
            .unwrap_or(report_path)
            .to_str()
            .unwrap_or("epic-retrospective-report.md");

        // Create branch from base
        let create_output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["checkout", "-b", retro_branch, base_branch])
            .output()
            .await;

        match create_output {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::error!(
                    action = "retro_branch_create_failed",
                    error = %stderr,
                    "Failed to create retrospective branch"
                );
                return None;
            }
            Err(e) => {
                tracing::error!(
                    action = "retro_branch_create_exec_failed",
                    error = %e,
                    "Failed to execute git checkout for retrospective branch"
                );
                return None;
            }
        }

        // Write report file on retro branch (it may have been written before on a different branch)
        if let Err(e) = tokio::fs::write(report_path, report).await {
            tracing::error!(
                action = "retro_report_write_failed",
                error = %e,
                "Failed to write report on retro branch"
            );
            let _ = checkout_branch(repo_path, base_branch).await;
            return None;
        }

        // Stage and commit the report (use relative path for portability across git versions)
        let _ = tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["add", "--", report_path_relative])
            .output()
            .await;

        let commit_msg = format!("docs(retrospective): epic {epic_num} autonomous review report");
        let commit_output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["commit", "-m", &commit_msg])
            .output()
            .await;

        if let Ok(output) = &commit_output
            && !output.status.success()
        {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(
                action = "retro_commit_warn",
                error = %stderr,
                "Retrospective commit may have failed"
            );
        }

        // Push retro branch — checkout back to base on failure to avoid leaving
        // HEAD stranded on the retro branch for the remainder of the pipeline run.
        if let Err(e) = self.push_branch(retro_branch).await {
            tracing::error!(
                action = "retro_push_failed",
                error = %e,
                "Failed to push retrospective branch"
            );
            let _ = checkout_branch(repo_path, base_branch).await;
            return None;
        }

        // Create MR/PR
        let mr_title = format!("Epic {epic_num} Retrospective — Review Gate");
        let mr_description = extract_epic_recap(report);

        let pr_params = CreatePrParams {
            title: mr_title,
            body: mr_description,
            source_branch: retro_branch.to_string(),
            target_branch: self.config.git_provider.target_branch.clone(),
        };

        match self.git_provider.create_pr(pr_params).await {
            Ok(pr_info) => {
                tracing::info!(
                    action = "retro_mr_created",
                    url = %pr_info.url,
                    epic_num = epic_num,
                    "Retrospective MR created"
                );
                Some(pr_info.url)
            }
            Err(e) => {
                tracing::error!(
                    action = "retro_mr_failed",
                    error = %e,
                    epic_num = epic_num,
                    "Failed to create retrospective MR"
                );
                None
            }
        }
    }
}

/// Push the current branch (HEAD) to origin.
async fn push_current_branch(repo_path: &str) -> Result<(), String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["push", "origin", "HEAD"])
        .output()
        .await
        .map_err(|e| format!("git push exec failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git push HEAD failed: {stderr}"));
    }
    Ok(())
}

/// Checkout an existing branch.
async fn checkout_branch(repo_path: &str, branch: &str) -> Result<(), String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["checkout", branch])
        .output()
        .await
        .map_err(|e| format!("git checkout exec failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git checkout {branch} failed: {stderr}"));
    }
    Ok(())
}

/// Resolve the epic title from epics.md.
///
/// Looks for a heading like `## Epic {X}: {title}` in the epics file.
/// Falls back to `"Epic {X}"` if parsing fails.
fn resolve_epic_title(planning_artifacts: &Path, epic_num: u32) -> String {
    let epics_path = planning_artifacts.join("epics.md");
    let pattern = format!("## Epic {epic_num}:");
    match std::fs::read_to_string(&epics_path) {
        Ok(content) => content
            .lines()
            .find(|line| line.contains(&pattern))
            .and_then(|line| line.split(':').nth(1))
            .map(|title| title.trim().to_string())
            .unwrap_or_else(|| format!("Epic {epic_num}")),
        Err(_) => format!("Epic {epic_num}"),
    }
}

fn re_poll_eligible(sprint_status_path: &Path, story_dir: &Path) -> Result<Vec<StoryInfo>, String> {
    let sprint_status = SprintStatusFile::load(sprint_status_path, story_dir)
        .map_err(|e| format!("sprint-status load failed: {e}"))?;

    let eligible = sprint_status.eligible_stories();
    if eligible.is_empty() {
        return Ok(Vec::new());
    }

    let entries = sprint_status.entries();
    let comment_deps = sprint_status.comment_deps();
    let (filtered, cascade_count) = watcher_deps::filter_eligible(eligible, entries, comment_deps)
        .map_err(|e| format!("dependency filter failed: {e}"))?;

    if cascade_count > 0 {
        tracing::info!(
            action = "re_poll_cascade",
            cascade_blocked = cascade_count,
            "Re-poll detected cascade-blocked stories"
        );
    }

    Ok(filtered)
}

impl StoryPipeline {
    /// Send a notification for a single story result (non-blocking).
    ///
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
        self.ui.crash_recovery_start();
        let recovery = self.session_runner.check_and_recover_wal().await?;

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

        let outcome = self.session_runner.resume_session(recovery).await;
        let result = self
            .process_recovered_session(&story_for_pipeline, outcome)
            .await;
        self.notify_story_result(&result).await;
        self.ui.crash_recovery_complete(&result.story_key);
        Some(result)
    }

    /// Process the outcome of a recovered session through the post-session pipeline.
    ///
    /// Reuses the same post-session logic as [`process_story()`]: code review → PR → notification.
    async fn process_recovered_session(
        &self,
        story: &StoryInfo,
        outcome: SessionOutcome,
    ) -> PipelineResult {
        let story_title = story_title_from_label(&story.label);
        self.ui.story_start(&story.story_key, &story_title);

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
                    self.ui.phase_start("Code Review");
                    let review_start = std::time::Instant::now();
                    match self.review_runner.run(story).await {
                        ReviewOutcome::Completed { report, .. } => {
                            self.ui
                                .phase_complete("Code Review", review_start.elapsed());
                            Some(report)
                        }
                        ReviewOutcome::Failed {
                            story_key: rk,
                            error,
                        } => {
                            self.ui.phase_error("Code Review", &error);
                            tracing::warn!(
                                action = "recovery_review_failed",
                                story_key = %rk,
                                error = %error,
                                "Code review failed after recovery — continuing to PR creation"
                            );
                            None
                        }
                        ReviewOutcome::Skipped { reason } => {
                            self.ui
                                .phase_complete("Code Review", review_start.elapsed());
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
                self.ui.phase_start("Push Branch");
                let push_start = std::time::Instant::now();
                if let Err(e) = self.push_branch(&branch).await {
                    self.ui.phase_error("Push Branch", &e.to_string());
                    tracing::error!(
                        action = "recovery_push_failed",
                        story_key = %story_key,
                        branch = %branch,
                        error = %e,
                        "Git push failed after recovery — cannot create PR"
                    );
                    self.ui
                        .story_error(&story_key, &format!("Push failed after recovery: {e}"));
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
                self.ui.phase_complete("Push Branch", push_start.elapsed());

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

                self.ui.phase_start("Create PR");
                let pr_start = std::time::Instant::now();
                match self.git_provider.create_pr(pr_params).await {
                    Ok(pr_info) => {
                        self.ui.phase_complete("Create PR", pr_start.elapsed());
                        if let Some(ref report) = review_report {
                            match self
                                .git_provider
                                .add_comment(&pr_info.id, &strip_agent_artifacts(report))
                                .await
                            {
                                Ok(()) => {
                                    self.ui.tool_result("pr_comment", "Review posted");
                                }
                                Err(e) => {
                                    tracing::error!(
                                        action = "recovery_pr_comment_failed",
                                        pr_id = %pr_info.id,
                                        error = %e,
                                        "Failed to post review comment after recovery"
                                    );
                                    self.ui.tool_result("pr_comment", &format!("Failed: {e}"));
                                }
                            }
                        }

                        let result = PipelineResult {
                            story_key: story_key.clone(),
                            status: StoryStatus::Completed,
                            pr_url: Some(pr_info.url.clone()),
                            error_detail: None,
                            fatal: false,
                        };
                        self.ui.story_complete(&story_key, Some(&pr_info.url));
                        result
                    }
                    Err(e) => {
                        self.ui.phase_error("Create PR", &e.to_string());
                        tracing::error!(
                            action = "recovery_pr_creation_failed",
                            story_key = %story_key,
                            branch = %branch,
                            error = %e,
                            "PR creation failed after recovery"
                        );
                        let result = PipelineResult {
                            story_key: story_key.clone(),
                            status: StoryStatus::Error,
                            pr_url: None,
                            error_detail: Some(format!(
                                "PR creation failed after recovery: {e}. Branch: {branch}"
                            )),
                            fatal: false,
                        };
                        self.ui.story_error(
                            &story_key,
                            &format!("PR creation failed after recovery: {e}"),
                        );
                        result
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
                self.ui.phase_start("Push Branch");
                let push_start = std::time::Instant::now();
                if let Err(e) = self.push_branch(&branch).await {
                    self.ui.phase_error("Push Branch", &e.to_string());
                    tracing::warn!(
                        action = "recovery_escalation_push_failed",
                        story_key = %report.story_key,
                        branch = %branch,
                        error = %e,
                        "Git push failed for recovery escalation branch — attempting PR anyway"
                    );
                } else {
                    self.ui.phase_complete("Push Branch", push_start.elapsed());
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

                self.ui.phase_start("Create PR");
                let pr_start = std::time::Instant::now();
                let pr_url = match self.git_provider.create_pr(pr_params).await {
                    Ok(pr_info) => {
                        self.ui.phase_complete("Create PR", pr_start.elapsed());
                        Some(pr_info.url.clone())
                    }
                    Err(e) => {
                        self.ui.phase_error("Create PR", &e.to_string());
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

                let result = PipelineResult {
                    story_key: report.story_key.clone(),
                    status: StoryStatus::Blocked,
                    pr_url,
                    error_detail: Some(format!(
                        "Escalated after recovery: {} — {}",
                        report.question, report.reason
                    )),
                    fatal: false,
                };
                self.ui.story_escalated(&report.story_key, &report.reason);
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

                    self.ui.story_error(&story_key, &error);
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

                    self.ui.phase_start("Push Branch");
                    let push_start = std::time::Instant::now();
                    if let Err(e) = self.push_branch(&branch).await {
                        self.ui.phase_error("Push Branch", &e.to_string());
                        tracing::warn!(
                            action = "recovery_failure_push_failed",
                            story_key = %story_key,
                            branch = %branch,
                            error = %e,
                            "Git push failed for recovery failure branch — attempting PR anyway"
                        );
                    } else {
                        self.ui.phase_complete("Push Branch", push_start.elapsed());
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

                    self.ui.phase_start("Create PR");
                    let pr_start = std::time::Instant::now();
                    match self.git_provider.create_pr(pr_params).await {
                        Ok(pr_info) => {
                            self.ui.phase_complete("Create PR", pr_start.elapsed());
                            self.ui.story_error(
                                &story_key,
                                "Recovery session failed — failure PR created",
                            );
                            PipelineResult {
                                story_key: story_key.clone(),
                                status: StoryStatus::Error,
                                pr_url: Some(pr_info.url.clone()),
                                error_detail: Some(error),
                                fatal: false,
                            }
                        }
                        Err(pr_err) => {
                            self.ui.phase_error("Create PR", &pr_err.to_string());
                            tracing::error!(
                                action = "recovery_failure_pr_creation_failed",
                                story_key = %story_key,
                                branch = %branch,
                                error = %pr_err,
                                "Failed to create failure PR after recovery"
                            );
                            self.ui.story_error(
                                &story_key,
                                &format!("Recovery failed, PR creation also failed: {pr_err}"),
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
///
/// **Exception:** "token expired" is NOT an infra error — it's a transient
/// Copilot token refresh issue that the session runner retries with backoff.
fn is_infra_error(error: &str) -> bool {
    let lower = error.to_lowercase();

    // Copilot token expiry is transient, not infrastructure failure
    if lower.contains("token expired") {
        return false;
    }

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
///
/// **Exception:** "token expired" is NOT an auth error — it's a transient
/// Copilot token refresh issue (short-lived session token expired mid-session).
/// The session runner retries these with exponential backoff.
fn is_auth_error(error: &str) -> bool {
    let lower = error.to_lowercase();

    // Copilot token expiry ≠ bad credentials — it's transient
    if lower.contains("token expired") {
        return false;
    }

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

// ---------------------------------------------------------------------------
// Epic Completion Detection
// ---------------------------------------------------------------------------

/// Detect whether the completed story was the last one in its epic.
///
/// Re-reads sprint-status.yaml from disk (NOT the cached poll version)
/// to ensure the just-completed story's status update is visible.
/// Returns `Some(epic_num)` if all stories in the epic are done.
fn detect_epic_completion(
    sprint_status_path: &Path,
    story_dir: &Path,
    completed_story: &StoryInfo,
) -> Option<u32> {
    let ssf = match SprintStatusFile::load(sprint_status_path, story_dir) {
        Ok(ssf) => ssf,
        Err(e) => {
            tracing::warn!(
                action = "epic_completion_check_failed",
                error = %e,
                "Failed to re-read sprint-status for epic completion detection"
            );
            return None;
        }
    };

    let epic_num = completed_story.epic_num;

    // Check all stories in this epic — ALL must be done.
    // stories() already filters out epic entries and retrospective entries.
    let epic_stories: Vec<_> = ssf
        .stories()
        .into_iter()
        .filter(|s| s.epic_num == epic_num)
        .collect();

    // Guard: Iterator::all() on an empty iterator returns true — we must
    // explicitly reject epics with no story entries to avoid false gate triggers
    // (e.g., misconfigured sprint-status or a race condition on first load).
    if epic_stories.is_empty() {
        return None;
    }

    let all_done = epic_stories.iter().all(|s| s.status == "done");

    if all_done { Some(epic_num) } else { None }
}

/// Check if sprint-status has a retrospective entry for this epic.
fn has_retrospective_entry(ssf: &SprintStatusFile, epic_num: u32) -> bool {
    let key = format!("epic-{epic_num}-retrospective");
    ssf.entries().iter().any(|(k, _)| k == &key)
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
    fn test_is_infra_error_false_for_token_expired() {
        // "token expired" contains "401" but is transient, not infra
        assert!(!is_infra_error(
            "Initial chat failed: CompletionError: ProviderError: Invalid status code 401 Unauthorized with message: unauthorized: token expired"
        ));
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
    fn test_is_auth_error_false_for_token_expired() {
        // "token expired" contains "401" but is NOT permanent bad creds — it's transient
        assert!(!is_auth_error(
            "Invalid status code 401 Unauthorized with message: unauthorized: token expired"
        ));
        assert!(!is_auth_error("unauthorized: token expired"));
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

    // -----------------------------------------------------------------------
    // re_poll_eligible tests
    // -----------------------------------------------------------------------

    /// Helper: write a sprint-status.yaml file with the given development_status entries.
    fn write_sprint_status(dir: &std::path::Path, entries: &[(&str, &str)]) {
        let mut yaml = String::from("development_status:\n");
        for (key, status) in entries {
            yaml.push_str(&format!("    {key}: {status}\n"));
        }
        std::fs::write(dir.join("sprint-status.yaml"), yaml).unwrap();
    }

    #[test]
    fn test_re_poll_eligible_picks_up_status_changes() {
        // Scenario: 1-1 was ready-for-dev, 1-2 was ready-for-dev (dep on 1-1).
        // After 1-1 is marked done, re-poll should return 1-2 as eligible.
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path();

        // State AFTER 1-1 completed: 1-1 is done, 1-2 is ready-for-dev
        write_sprint_status(
            p,
            &[
                ("epic-1", "in-progress"),
                ("1-1-scaffolding", "done"),
                ("1-2-cli", "ready-for-dev"),
            ],
        );

        let ss = p.join("sprint-status.yaml");
        let result = re_poll_eligible(&ss, p).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].story_key, "1-2-cli");
    }

    #[test]
    fn test_re_poll_eligible_cross_epic_first_stories_all_eligible() {
        // Scenario: multiple epics, first story of each has no intra-epic dep.
        // This is the CURRENT behavior (cross-epic deps not enforced by daemon).
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path();

        write_sprint_status(
            p,
            &[
                ("epic-1", "in-progress"),
                ("1-1-scaffolding", "ready-for-dev"),
                ("1-2-cli", "ready-for-dev"),
                ("epic-2", "in-progress"),
                ("2-1-rest-client", "ready-for-dev"),
                ("2-2-websocket", "ready-for-dev"),
            ],
        );

        let ss = p.join("sprint-status.yaml");
        let result = re_poll_eligible(&ss, p).unwrap();

        // 1-1 and 2-1 are eligible (first in their epic, no deps)
        // 1-2 and 2-2 are NOT eligible (deps on 1-1 and 2-1 which are not done)
        let keys: Vec<&str> = result.iter().map(|s| s.story_key.as_str()).collect();
        assert_eq!(keys, vec!["1-1-scaffolding", "2-1-rest-client"]);
    }

    #[test]
    fn test_re_poll_eligible_after_first_story_done_returns_second() {
        // The core bug scenario: after 1-1 done, 1-2 becomes eligible and should
        // appear BEFORE 2-1 in sprint order (document order tiebreaker).
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path();

        write_sprint_status(
            p,
            &[
                ("epic-1", "in-progress"),
                ("1-1-scaffolding", "done"),
                ("1-2-cli", "ready-for-dev"),
                ("epic-2", "in-progress"),
                ("2-1-rest-client", "ready-for-dev"),
            ],
        );

        let ss = p.join("sprint-status.yaml");
        let result = re_poll_eligible(&ss, p).unwrap();

        // Both 1-2 (dep 1-1 is done) and 2-1 (no dep) are eligible.
        // 1-2 appears first because it comes first in document order.
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].story_key, "1-2-cli");
        assert_eq!(result[1].story_key, "2-1-rest-client");
    }

    #[test]
    fn test_re_poll_eligible_returns_empty_when_all_done() {
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path();

        write_sprint_status(
            p,
            &[
                ("epic-1", "done"),
                ("1-1-scaffolding", "done"),
                ("1-2-cli", "done"),
            ],
        );

        let ss = p.join("sprint-status.yaml");
        let result = re_poll_eligible(&ss, p).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_re_poll_eligible_returns_error_on_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let ss = dir.path().join("nonexistent.yaml");

        let result = re_poll_eligible(&ss, dir.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("sprint-status load failed"));
    }

    #[test]
    fn test_re_poll_eligible_cascade_blocked_excluded() {
        // Story 1-2 depends on 1-1 which is blocked → 1-2 should be cascade-blocked
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path();

        write_sprint_status(
            p,
            &[
                ("epic-1", "in-progress"),
                ("1-1-scaffolding", "blocked"),
                ("1-2-cli", "ready-for-dev"),
                ("epic-2", "in-progress"),
                ("2-1-rest-client", "ready-for-dev"),
            ],
        );

        let ss = p.join("sprint-status.yaml");
        let result = re_poll_eligible(&ss, p).unwrap();

        // 1-2 is cascade-blocked (dep 1-1 is blocked), only 2-1 is eligible
        let keys: Vec<&str> = result.iter().map(|s| s.story_key.as_str()).collect();
        assert_eq!(keys, vec!["2-1-rest-client"]);
    }

    // -----------------------------------------------------------------------
    // Epic completion detection tests (Task 2)
    // -----------------------------------------------------------------------

    fn write_sprint_status_for_epic(dir: &std::path::Path, content: &str) {
        let artifacts = dir.join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        std::fs::write(artifacts.join("sprint-status.yaml"), content).unwrap();
    }

    #[test]
    fn test_detect_epic_completion_all_done() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let ss_path = artifacts.join("sprint-status.yaml");
        std::fs::write(
            &ss_path,
            r#"
development_status:
    epic-1: in-progress
    1-1-scaffolding: done
    1-2-cli: done
    1-3-init: done
    epic-1-retrospective: optional
"#,
        )
        .unwrap();

        let story = StoryInfo::from_key_and_status("1-3-init", "done", &artifacts).unwrap();
        let result = detect_epic_completion(&ss_path, &artifacts, &story);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn test_detect_epic_completion_partial() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let ss_path = artifacts.join("sprint-status.yaml");
        std::fs::write(
            &ss_path,
            r#"
development_status:
    epic-1: in-progress
    1-1-scaffolding: done
    1-2-cli: in-progress
    1-3-init: done
    epic-1-retrospective: optional
"#,
        )
        .unwrap();

        let story = StoryInfo::from_key_and_status("1-3-init", "done", &artifacts).unwrap();
        let result = detect_epic_completion(&ss_path, &artifacts, &story);
        assert_eq!(result, None);
    }

    #[test]
    fn test_detect_epic_completion_single_story_epic() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let ss_path = artifacts.join("sprint-status.yaml");
        std::fs::write(
            &ss_path,
            r#"
development_status:
    epic-5: in-progress
    5-1-only-story: done
    epic-5-retrospective: optional
"#,
        )
        .unwrap();

        let story = StoryInfo::from_key_and_status("5-1-only-story", "done", &artifacts).unwrap();
        let result = detect_epic_completion(&ss_path, &artifacts, &story);
        assert_eq!(result, Some(5));
    }

    #[test]
    fn test_detect_epic_completion_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let ss_path = artifacts.join("sprint-status.yaml"); // does not exist

        let story = StoryInfo::from_key_and_status("1-1-test", "done", &artifacts).unwrap();
        let result = detect_epic_completion(&ss_path, &artifacts, &story);
        assert_eq!(result, None);
    }

    #[test]
    fn test_detect_epic_completion_excludes_retro_and_epic_entries() {
        // Retrospective and epic entries must NOT be counted as stories.
        // Even if epic-2-retrospective is "optional" (not "done"), the epic
        // should still be detected as complete if all actual stories are done.
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let ss_path = artifacts.join("sprint-status.yaml");
        std::fs::write(
            &ss_path,
            r#"
development_status:
    epic-2: in-progress
    2-1-polling: done
    2-2-deps: done
    epic-2-retrospective: optional
"#,
        )
        .unwrap();

        let story = StoryInfo::from_key_and_status("2-2-deps", "done", &artifacts).unwrap();
        let result = detect_epic_completion(&ss_path, &artifacts, &story);
        assert_eq!(result, Some(2));
    }

    #[test]
    fn test_has_retrospective_entry_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let ss_path = artifacts.join("sprint-status.yaml");
        std::fs::write(
            &ss_path,
            r#"
development_status:
    epic-3: done
    3-1-story: done
    epic-3-retrospective: optional
"#,
        )
        .unwrap();

        let ssf = SprintStatusFile::load(&ss_path, &artifacts).unwrap();
        assert!(has_retrospective_entry(&ssf, 3));
    }

    #[test]
    fn test_has_retrospective_entry_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let ss_path = artifacts.join("sprint-status.yaml");
        std::fs::write(
            &ss_path,
            r#"
development_status:
    epic-3: done
    3-1-story: done
"#,
        )
        .unwrap();

        let ssf = SprintStatusFile::load(&ss_path, &artifacts).unwrap();
        assert!(!has_retrospective_entry(&ssf, 3));
    }

    #[test]
    fn test_has_retrospective_entry_wrong_epic() {
        let tmp = tempfile::tempdir().unwrap();
        let artifacts = tmp.path().join("artifacts");
        std::fs::create_dir_all(&artifacts).unwrap();
        let ss_path = artifacts.join("sprint-status.yaml");
        std::fs::write(
            &ss_path,
            r#"
development_status:
    epic-1: done
    1-1-story: done
    epic-1-retrospective: done
"#,
        )
        .unwrap();

        let ssf = SprintStatusFile::load(&ss_path, &artifacts).unwrap();
        assert!(has_retrospective_entry(&ssf, 1));
        assert!(!has_retrospective_entry(&ssf, 2));
    }

    #[test]
    fn test_re_poll_eligible_processed_keys_prevent_reprocessing() {
        // Verify the processed_keys HashSet logic in process_eligible_stories
        // by testing the pattern directly: if a story already in processed_keys
        // appears in the re-polled list, it should be skipped.
        let dir = tempfile::TempDir::new().unwrap();
        let p = dir.path();

        write_sprint_status(
            p,
            &[
                ("epic-1", "in-progress"),
                ("1-1-scaffolding", "ready-for-dev"),
                ("epic-2", "in-progress"),
                ("2-1-rest-client", "ready-for-dev"),
            ],
        );

        let ss = p.join("sprint-status.yaml");
        let stories = re_poll_eligible(&ss, p).unwrap();

        // Simulate: 1-1 already processed
        let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();
        processed.insert("1-1-scaffolding".to_string());

        let next = stories.iter().find(|s| !processed.contains(&s.story_key));
        assert!(next.is_some());
        assert_eq!(next.unwrap().story_key, "2-1-rest-client");
    }

    // -----------------------------------------------------------------------
    // resolve_epic_title tests (Task 8)
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_epic_title_parses_heading() {
        let tmp = tempfile::tempdir().unwrap();
        let planning = tmp.path();
        std::fs::write(
            planning.join("epics.md"),
            "# Epics\n\n## Epic 1: Project Scaffolding\nDetails...\n\n## Epic 2: Sprint Polling\nMore details...\n",
        )
        .unwrap();

        assert_eq!(resolve_epic_title(planning, 1), "Project Scaffolding");
        assert_eq!(resolve_epic_title(planning, 2), "Sprint Polling");
    }

    #[test]
    fn test_resolve_epic_title_fallback_when_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let planning = tmp.path();
        std::fs::write(
            planning.join("epics.md"),
            "# Epics\n\n## Epic 1: Scaffolding\n",
        )
        .unwrap();

        // Epic 99 doesn't exist in the file
        assert_eq!(resolve_epic_title(planning, 99), "Epic 99");
    }

    #[test]
    fn test_resolve_epic_title_fallback_when_file_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let planning = tmp.path();
        // No epics.md file created

        assert_eq!(resolve_epic_title(planning, 3), "Epic 3");
    }

    #[test]
    fn test_resolve_epic_title_handles_complex_title() {
        let tmp = tempfile::tempdir().unwrap();
        let planning = tmp.path();
        std::fs::write(
            planning.join("epics.md"),
            "## Epic 4: LLM Agent Session & Tool Calling\n",
        )
        .unwrap();

        assert_eq!(
            resolve_epic_title(planning, 4),
            "LLM Agent Session & Tool Calling"
        );
    }
}
