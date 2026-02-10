//! Story pipeline orchestrator — daemon Layer 3 error handling and story lifecycle.
//!
//! The [`StoryPipeline`] struct encapsulates the full story processing pipeline:
//! session → optional review → PR creation → notification. It implements the
//! "never stop the run" principle: no single story failure halts the daemon.
//!
//! Use [`StoryPipeline::new`] to construct, then [`process_eligible_stories`] to
//! run a batch of stories from the watcher.

use std::sync::Arc;

use crate::config::{BotConfig, BotSecrets};
use crate::git_provider::{
    CreatePrParams, GitProvider, PrDescriptionParams, build_pr_description, build_pr_title,
    create_provider,
};
use crate::notifier::{Notifier, RunSummary, StoryNotification, StoryStatus, create_notifier};
use crate::review::ReviewOutcome;
use crate::review::ReviewRunner;
use crate::session::SessionOutcome;
use crate::session::runner::SessionRunner;
use crate::session::runner::ShutdownFlag;
use crate::supervisor::decisions::format_pr_decisions_section;
use crate::watcher::StoryInfo;

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

        let session_runner = SessionRunner::new(
            Arc::clone(&config),
            Arc::clone(&secrets),
            Arc::clone(&shutdown),
        );
        let review_runner = ReviewRunner::new(Arc::clone(&config), Arc::clone(&secrets), shutdown);

        Ok(Self {
            config,
            git_provider,
            notifier,
            session_runner,
            review_runner,
        })
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
        let session_outcome = self.session_runner.run(story).await;

        match session_outcome {
            SessionOutcome::Completed {
                story_key,
                branch,
                decisions,
            } => {
                // Phase 2 — Code Review (optional)
                let review_report = if self.config.code_review_enabled {
                    match self.review_runner.run(story).await {
                        ReviewOutcome::Completed { report, .. } => Some(report),
                        ReviewOutcome::Failed {
                            story_key: rk,
                            error,
                        } => {
                            tracing::warn!(
                                action = "review_failed",
                                story_key = %rk,
                                error = %error,
                                "Code review failed — continuing to PR creation"
                            );
                            None
                        }
                        ReviewOutcome::Skipped { reason } => {
                            tracing::info!(
                                action = "review_skipped",
                                reason = %reason,
                                "Code review skipped — continuing to PR creation"
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                // Phase 4 — Success PR
                let decisions_section = format_pr_decisions_section(&decisions);
                let pr_title = build_pr_title(&story_key, &story_title, false);
                let pr_body = build_pr_description(&PrDescriptionParams {
                    story_key: story_key.clone(),
                    story_title: story_title.clone(),
                    outcome_summary: "completed successfully".to_string(),
                    decisions_section,
                    failure_details: None,
                });
                let pr_params = CreatePrParams {
                    title: pr_title,
                    body: pr_body,
                    source_branch: branch.clone(),
                    target_branch: self.config.git_provider.target_branch.clone(),
                };

                match self.git_provider.create_pr(pr_params).await {
                    Ok(pr_info) => {
                        // Post review comment if available (non-blocking)
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

                        let result = PipelineResult {
                            story_key: story_key.clone(),
                            status: StoryStatus::Completed,
                            pr_url: Some(pr_info.url.clone()),
                            error_detail: None,
                        };
                        self.notify_story_result(&result).await;
                        result
                    }
                    Err(e) => {
                        tracing::error!(
                            action = "pr_creation_failed",
                            story_key = %story_key,
                            branch = %branch,
                            error = %e,
                            "PR creation failed — notifying human with branch name"
                        );

                        let result = PipelineResult {
                            story_key: story_key.clone(),
                            status: StoryStatus::Error,
                            pr_url: None,
                            error_detail: Some(format!(
                                "PR creation failed: {e}. Branch: {branch}"
                            )),
                        };
                        self.notify_story_result(&result).await;
                        result
                    }
                }
            }

            SessionOutcome::Escalated { report, decisions } => {
                tracing::warn!(
                    action = "session_escalated",
                    story_key = %report.story_key,
                    question = %report.question,
                    reason = %report.reason,
                    "Story escalated — needs human clarification"
                );

                let result = PipelineResult {
                    story_key: report.story_key.clone(),
                    status: StoryStatus::Blocked,
                    pr_url: None,
                    error_detail: Some(format!(
                        "Escalated: {} — {}",
                        report.question, report.reason
                    )),
                };
                let _ = &decisions; // decisions tracked by SessionRunner
                self.notify_story_result(&result).await;
                result
            }

            SessionOutcome::Failed {
                story_key,
                error,
                decisions,
            } => {
                tracing::error!(
                    action = "session_failed",
                    story_key = %story_key,
                    error = %error,
                    "Dev session failed — creating failure PR"
                );

                // Phase 3 — Failure PR
                let branch = format!("story/{story_key}");
                let decisions_section = format_pr_decisions_section(&decisions);
                let pr_title = build_pr_title(&story_key, &story_title, true);
                let pr_body = build_pr_description(&PrDescriptionParams {
                    story_key: story_key.clone(),
                    story_title: story_title.clone(),
                    outcome_summary: "failed".to_string(),
                    decisions_section,
                    failure_details: Some(error.clone()),
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
                        };
                        self.notify_story_result(&result).await;
                        result
                    }
                }
            }
        }
    }

    /// Process all eligible stories sequentially, then send a run summary.
    ///
    /// Stories are processed in the order received from the watcher (dependency-sorted).
    /// After all stories are processed, a run summary notification is sent.
    pub async fn process_eligible_stories(&self, stories: Vec<StoryInfo>) -> RunSummary {
        let mut results: Vec<PipelineResult> = Vec::with_capacity(stories.len());

        for story in &stories {
            let result = self.process_story(story).await;
            results.push(result);
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

        match outcome {
            SessionOutcome::Completed {
                story_key,
                branch,
                decisions,
            } => {
                // Optional code review
                let review_report = if self.config.code_review_enabled {
                    match self.review_runner.run(story).await {
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

                // Success PR
                let decisions_section = format_pr_decisions_section(&decisions);
                let pr_title = build_pr_title(&story_key, &story_title, false);
                let pr_body = build_pr_description(&PrDescriptionParams {
                    story_key: story_key.clone(),
                    story_title: story_title.clone(),
                    outcome_summary: "completed successfully (recovered from crash)".to_string(),
                    decisions_section,
                    failure_details: None,
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
                            && let Err(e) = self.git_provider.add_comment(&pr_info.id, report).await
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
                        }
                    }
                }
            }

            SessionOutcome::Escalated { report, decisions } => {
                tracing::warn!(
                    action = "recovery_session_escalated",
                    story_key = %report.story_key,
                    question = %report.question,
                    "Recovered session escalated — needs human clarification"
                );
                let _ = &decisions;
                PipelineResult {
                    story_key: report.story_key.clone(),
                    status: StoryStatus::Blocked,
                    pr_url: None,
                    error_detail: Some(format!(
                        "Escalated after recovery: {} — {}",
                        report.question, report.reason
                    )),
                }
            }

            SessionOutcome::Failed {
                story_key,
                error,
                decisions,
            } => {
                tracing::error!(
                    action = "recovery_session_failed",
                    story_key = %story_key,
                    error = %error,
                    "Recovered session failed — creating failure PR"
                );

                // Failure PR
                let branch = format!("story/{story_key}");
                let decisions_section = format_pr_decisions_section(&decisions);
                let pr_title = build_pr_title(&story_key, &story_title, true);
                let pr_body = build_pr_description(&PrDescriptionParams {
                    story_key: story_key.clone(),
                    story_title: story_title.clone(),
                    outcome_summary: "failed (crash recovery attempted)".to_string(),
                    decisions_section,
                    failure_details: Some(error.clone()),
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
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helper Functions
// ---------------------------------------------------------------------------

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
            story_key: "6-1-telegram-notifications".to_string(),
            status: StoryStatus::Completed,
            pr_url: Some("https://github.com/org/repo/pull/42".to_string()),
            error_detail: None,
        };
        assert_eq!(result.story_key, "6-1-telegram-notifications");
        assert_eq!(result.status, StoryStatus::Completed);
        assert!(result.pr_url.is_some());
        assert!(result.error_detail.is_none());
    }

    #[test]
    fn test_pipeline_result_failed_fields() {
        let result = PipelineResult {
            story_key: "6-2-http-retry".to_string(),
            status: StoryStatus::Error,
            pr_url: None,
            error_detail: Some("LLM provider down".to_string()),
        };
        assert_eq!(result.status, StoryStatus::Error);
        assert!(result.pr_url.is_none());
        assert_eq!(result.error_detail.as_deref(), Some("LLM provider down"));
    }

    #[test]
    fn test_pipeline_result_blocked_fields() {
        let result = PipelineResult {
            story_key: "3-3-escalation".to_string(),
            status: StoryStatus::Blocked,
            pr_url: None,
            error_detail: Some("Needs human input on DB schema".to_string()),
        };
        assert_eq!(result.status, StoryStatus::Blocked);
        assert!(
            result
                .error_detail
                .as_deref()
                .unwrap()
                .contains("DB schema")
        );
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
                story_key: "6-1-telegram-notifications".to_string(),
                status: StoryStatus::Completed,
                pr_url: Some("https://github.com/org/repo/pull/42".to_string()),
                error_detail: None,
            },
            PipelineResult {
                story_key: "6-2-http-retry".to_string(),
                status: StoryStatus::Error,
                pr_url: None,
                error_detail: Some("LLM down".to_string()),
            },
            PipelineResult {
                story_key: "6-3-crash-recovery".to_string(),
                status: StoryStatus::Blocked,
                pr_url: None,
                error_detail: Some("Needs clarification".to_string()),
            },
        ];

        let summary = build_run_summary(&results);
        assert_eq!(summary.total_processed, 3);
        assert_eq!(summary.completed, 1);
        assert_eq!(summary.errored, 1);
        assert_eq!(summary.blocked, 1);
        assert_eq!(summary.stories.len(), 3);
        assert_eq!(summary.stories[0].story_key, "6-1-telegram-notifications");
        assert_eq!(summary.stories[0].status, StoryStatus::Completed);
        assert_eq!(summary.stories[1].status, StoryStatus::Error);
        assert_eq!(summary.stories[2].status, StoryStatus::Blocked);
    }

    #[test]
    fn test_run_summary_all_completed() {
        let results = vec![
            PipelineResult {
                story_key: "1-1-scaffolding".to_string(),
                status: StoryStatus::Completed,
                pr_url: Some("url1".to_string()),
                error_detail: None,
            },
            PipelineResult {
                story_key: "1-2-cli".to_string(),
                status: StoryStatus::Completed,
                pr_url: Some("url2".to_string()),
                error_detail: None,
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
        }];

        let summary = build_run_summary(&results);
        assert_eq!(summary.stories[0].story_id, "6.1");
    }
}
