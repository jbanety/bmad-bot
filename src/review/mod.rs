//! Code review session — launches a separate LLM for adversarial code review.
//!
//! The [`ReviewRunner`] launches a new rig agent session using the review LLM
//! provider/model from config. It loads the same BMAD dev agent persona and
//! sends `"CR"` as the initial command to trigger the code review workflow.
//!
//! **Design:** The daemon does NOT implement code review logic. The BMAD dev
//! agent already has a complete adversarial code review workflow (`CR` command).
//! The daemon is simply a launcher with a post-review phase that asks the agent
//! to commit fixes and produce a report.
//!
//! Key types:
//! - [`ReviewError`] — typed errors for review-level failures
//! - [`ReviewOutcome`] — the three possible results of a review run
//! - [`ReviewRunner`] — the main review lifecycle manager

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use rig::completion::Message;
use rig::tools::think::ThinkTool;

use crate::config::{BotConfig, BotSecrets};
use crate::llm::agent_factory::{AgentFactory, BuiltAgent, LlmRole};
use crate::llm::logging::{log_llm_error, log_llm_request, log_llm_response};
use crate::session::analyzer::{ResponseAction, ResponseAnalyzer};
use crate::session::dev_agent::{self, ShutdownFlag};
use crate::session::provider::ProviderError;
use crate::supervisor::decisions::DecisionLog;
use crate::supervisor::{AskSupervisor, EscalationSlot};
use crate::tools::{
    EditFileTool, FindPathTool, GitTool, GrepTool, ListDirectoryTool, ReadFileTool, TerminalTool,
};
use crate::watcher::StoryInfo;

/// Type alias for the standard 8-tool set returned by [`ReviewRunner::create_tools()`].
///
/// Avoids `clippy::type_complexity` on the 8-element tuple.
type ReviewToolSet = (
    GitTool,
    ReadFileTool,
    EditFileTool,
    GrepTool,
    FindPathTool,
    ListDirectoryTool,
    TerminalTool,
    AskSupervisor,
);

/// Maximum chat turns for a review session (safety net).
const MAX_REVIEW_TURNS: usize = 100;

/// Maximum number of full session retries for transient errors (e.g. malformed tool calls).
///
/// When the LLM sends malformed tool call arguments (e.g. two JSON objects concatenated),
/// rig includes the broken tool call in conversation history, causing the API to reject
/// all subsequent turns with 400 "invalid_tool_call_format". The only recovery is to
/// retry with a fresh session (clean history). This constant limits how many times we retry.
const MAX_SESSION_RETRIES: usize = 2;

/// Terminal tool timeout in seconds for commands executed by the review agent.
const TERMINAL_TIMEOUT_SECS: u64 = 30;

/// Post-review message sent after the CR workflow completes.
///
/// Asks the agent to commit review fixes with descriptive messages and produce
/// a markdown review report suitable for posting as a PR comment.
const POST_REVIEW_MESSAGE: &str = "Commit all your review fixes with descriptive commit messages \
    that reference the findings. Then provide a complete markdown summary of your code review \
    (findings, severity, fixes applied, remaining concerns) suitable for posting as a PR comment.";

/// Errors originating from the review module.
///
/// Each variant carries structured context for logging and error handling.
/// Uses `thiserror` for `Display` and `Error` derive — no `anyhow` in this module.
#[derive(Debug, thiserror::Error)]
pub enum ReviewError {
    /// LLM client construction failed.
    #[error("Provider initialization failed: {reason}")]
    ProviderInit {
        /// Why the provider could not be initialized.
        reason: String,
    },

    /// Review provider API key not set in `.env`.
    #[error("API key missing for provider '{provider}' (env var: {env_var})")]
    ApiKeyMissing {
        /// The provider name.
        provider: String,
        /// The expected environment variable.
        env_var: String,
    },

    /// Unknown provider name in config.
    #[error("Unsupported provider: {provider}")]
    UnsupportedProvider {
        /// The unsupported provider name.
        provider: String,
    },

    /// Chat turn error during the review session.
    #[error("Chat failed at turn {turn}: {reason}")]
    ChatFailed {
        /// Which turn failed.
        turn: usize,
        /// Description of the failure.
        reason: String,
    },

    /// Rig agent construction failed.
    #[error("Agent build failed: {reason}")]
    AgentBuildFailed {
        /// Why the agent could not be built.
        reason: String,
    },
}

/// Result of a code review session.
///
/// Returned by [`ReviewRunner::run`]. The orchestrator handles each variant:
/// - [`Completed`](ReviewOutcome::Completed) → post report as PR comment
/// - [`Failed`](ReviewOutcome::Failed) → log error, proceed to PR creation anyway
/// - [`Skipped`](ReviewOutcome::Skipped) → proceed to PR creation, note skip in description
#[derive(Debug)]
pub enum ReviewOutcome {
    /// CR workflow finished successfully — review report captured for PR comment.
    Completed {
        /// The story key that was reviewed.
        story_key: String,
        /// Git branch with the reviewed (and possibly fixed) code.
        branch: String,
        /// Markdown review report authored by the review agent.
        report: String,
    },
    /// Review session crashed — non-blocking, proceed to PR creation.
    Failed {
        /// The story key that was being reviewed.
        story_key: String,
        /// Description of the failure.
        error: String,
    },
    /// Review was skipped — provider down, config disabled, etc.
    Skipped {
        /// Why the review was skipped.
        reason: String,
    },
}

/// Code review runner — manages the lifecycle of a single review session.
///
/// Constructed once per daemon run and reused. Each call to [`run()`](Self::run)
/// creates a fresh agent and chat loop for one story's code review.
///
/// **Critical design rule:** [`run()`](Self::run) NEVER panics or returns an
/// unhandled error. All failures are caught and returned as
/// [`ReviewOutcome::Skipped`] or [`ReviewOutcome::Failed`].
/// Manages code review lifecycle: build agent, run CR workflow, capture report.
#[derive(Debug)]
pub struct ReviewRunner {
    /// Shared bot configuration.
    config: Arc<BotConfig>,
    /// Shared secrets (API keys loaded from `.env`).
    secrets: Arc<BotSecrets>,
    /// Centralized agent construction factory.
    agent_factory: Arc<AgentFactory>,
    /// Stateless response analyzer (constructed once, reused).
    analyzer: ResponseAnalyzer,
    /// Cooperative shutdown flag — checked between streaming chunks and chat turns.
    shutdown: ShutdownFlag,
}

impl ReviewRunner {
    /// Create a new review runner.
    pub fn new(
        config: Arc<BotConfig>,
        secrets: Arc<BotSecrets>,
        agent_factory: Arc<AgentFactory>,
        shutdown: ShutdownFlag,
    ) -> Self {
        Self {
            config,
            secrets,
            agent_factory,
            analyzer: ResponseAnalyzer::new(),
            shutdown,
        }
    }

    /// Run a code review session for the given story.
    ///
    /// This method NEVER panics or returns an unhandled error. All failures
    /// are caught and returned as [`ReviewOutcome::Skipped`] or [`ReviewOutcome::Failed`].
    ///
    /// **Retry logic:** If the session fails due to a malformed tool call (rig/API bug
    /// where concatenated JSON args poison the conversation history), the entire session
    /// is retried from scratch with a clean agent and history.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        for attempt in 0..=MAX_SESSION_RETRIES {
            match self.run_inner(story).await {
                Ok(outcome) => return outcome,
                Err(e) => {
                    let error_str = e.to_string();
                    let is_retryable = is_retryable_review_error(&error_str);

                    if is_retryable && attempt < MAX_SESSION_RETRIES {
                        tracing::warn!(
                            action = "review_retry",
                            attempt = attempt + 1,
                            max_retries = MAX_SESSION_RETRIES,
                            error = %error_str,
                            story_key = %story.story_key,
                            "Transient error — retrying with fresh session"
                        );
                        continue;
                    }

                    tracing::error!(
                        action = "review_failed",
                        error = %e,
                        story_key = %story.story_key,
                        attempt = attempt + 1,
                        "Code review failed — skipping"
                    );
                    return ReviewOutcome::Skipped {
                        reason: e.to_string(),
                    };
                }
            }
        }

        // Unreachable — the loop always returns on the last attempt
        ReviewOutcome::Skipped {
            reason: "Review exhausted all retries".to_string(),
        }
    }
}

/// Check if a review error is transient and worth retrying with a fresh session.
///
/// Covers two categories:
/// 1. **Poisoned history** — malformed tool call args that make the entire
///    conversation history invalid (rig/API bug workaround)
/// 2. **Network errors** — transient HTTP failures (connection reset, timeout,
///    incomplete message) that are likely to succeed on retry
fn is_retryable_review_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    // Poisoned history (rig sends malformed tool call args in conversation history)
    lower.contains("invalid_tool_call_format")
        || lower.contains("not in a valid json format")
        // Transient network errors
        || lower.contains("incompletemessage")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("error sending request")
        || lower.contains("eof while parsing")
        // Transient API errors
        || lower.contains("status code 429")
        || lower.contains("status code 500")
        || lower.contains("status code 502")
        || lower.contains("status code 503")
        || lower.contains("status code 504")
}

impl ReviewRunner {
    /// Inner implementation that can fail with [`ReviewError`].
    async fn run_inner(&self, story: &StoryInfo) -> Result<ReviewOutcome, ReviewError> {
        let span = tracing::info_span!(
            "review_session",
            story_id = %story.story_id,
            story_key = %story.story_key,
        );
        let _guard = span.enter();

        tracing::info!(
            action = "review_start",
            story_key = %story.story_key,
            "Starting code review session"
        );

        // 1. Build generic preamble (same as SessionRunner)
        let preamble = dev_agent::build_preamble();

        // 2. Create shared resources
        let escalation_slot: EscalationSlot = Arc::new(std::sync::Mutex::new(None));
        let decision_log = DecisionLog::new();

        // 3. Build agent via AgentFactory — single call replaces 3-arm provider match
        let project_root = PathBuf::from(&self.config.bmad_paths.project_root);
        let (git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor) =
            self.create_tools(&project_root, escalation_slot.clone(), decision_log.clone())?;

        let agent = self
            .agent_factory
            .build(
                LlmRole::Review,
                &preamble,
                crate::configure_agent_tools!(
                    git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor,
                    ThinkTool
                ),
            )
            .await
            .map_err(|e| match e {
                ProviderError::MissingApiKey { env_var, .. } => ReviewError::ApiKeyMissing {
                    provider: self.config.llm.review.provider.clone(),
                    env_var,
                },
                other => ReviewError::ProviderInit {
                    reason: other.to_string(),
                },
            })?;

        // 4. Drive the review session
        let outcome = self
            .drive_review_session(&agent, story, escalation_slot, decision_log)
            .await?;

        let outcome_type = match &outcome {
            ReviewOutcome::Completed { .. } => "completed",
            ReviewOutcome::Failed { .. } => "failed",
            ReviewOutcome::Skipped { .. } => "skipped",
        };

        tracing::info!(
            action = "review_end",
            outcome = %outcome_type,
            "Code review session ended"
        );

        Ok(outcome)
    }

    /// Create the 8 tools for the rig agent: 7 custom tools + ask_supervisor.
    fn create_tools(
        &self,
        project_root: &Path,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
    ) -> Result<ReviewToolSet, ReviewError> {
        let git = GitTool::new(project_root.to_path_buf());
        let read_file = ReadFileTool::new(project_root.to_path_buf());
        let edit_file = EditFileTool::new(project_root.to_path_buf());
        let grep = GrepTool::new(project_root.to_path_buf());
        let find_path = FindPathTool::new(project_root.to_path_buf());
        let list_dir = ListDirectoryTool::new(project_root.to_path_buf());
        let terminal = TerminalTool::new(project_root.to_path_buf(), TERMINAL_TIMEOUT_SECS);

        let supervisor = AskSupervisor::with_architect_from_config(
            &self.config,
            Some(Arc::clone(&self.agent_factory)),
            escalation_slot,
            decision_log,
        )
        .map_err(|e| ReviewError::AgentBuildFailed {
            reason: format!("Failed to create AskSupervisor: {e}"),
        })?;

        Ok((
            git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor,
        ))
    }

    /// Drive the review session chat loop.
    ///
    /// Two-phase approach:
    /// 1. **Normal phase:** Send `"CR"`, analyze responses with `ResponseAnalyzer`,
    ///    auto-respond to workflow prompts. On `Completed` → enter post-review phase.
    /// 2. **Post-review phase:** Send `POST_REVIEW_MESSAGE`, capture the agent's
    ///    next response as the review report. Return `ReviewOutcome::Completed`.
    ///
    /// The `story_reply` parameter for the analyzer uses `story.specs_path` (file path),
    /// NOT the story key, because the CR workflow asks for the story file path.
    async fn drive_review_session(
        &self,
        agent: &BuiltAgent,
        story: &StoryInfo,
        escalation_slot: EscalationSlot,
        _decision_log: DecisionLog,
    ) -> Result<ReviewOutcome, ReviewError> {
        // The CR workflow asks "which story file to review" — reply with the file path
        let story_reply = story.specs_path.display().to_string();

        // Activate agent: send dev.md as user message (same flow as dev session)
        let (activation_rig_history, _activation_chat_history) = agent
            .activate_agent(
                &self.config.bmad_paths.project_root,
                "_bmad/bmm/agents/dev.md",
                "code-review",
                Some(&self.shutdown),
            )
            .await
            .map_err(|e| {
                tracing::error!(action = "review_activation_failed", error = %e);
                ReviewError::AgentBuildFailed {
                    reason: format!("Agent activation failed: {e}"),
                }
            })?;

        // Send "CR" with English language override (same pattern as DS in dev session)
        let initial_message = "IMPORTANT: ALL communication MUST be in English regardless of config file settings. Execute [CR]";
        log_llm_request(
            "code-review",
            1,
            initial_message,
            activation_rig_history.len(),
        );
        let response = agent
            .stream_chat(
                initial_message,
                activation_rig_history,
                Some(&self.shutdown),
            )
            .await
            .map_err(|e| {
                log_llm_error("code-review", 1, &e);
                ReviewError::ChatFailed {
                    turn: 1,
                    reason: e.to_string(),
                }
            })?;
        log_llm_response("code-review", 1, &response);

        tracing::debug!(
            action = "review_chat_turn",
            turn = 1,
            response_len = %response.len(),
            "Initial review chat turn completed (post-activation)"
        );

        let mut current_response = response;
        let mut turn: usize = 2;
        let mut retries: usize = 0;
        let mut post_review_phase = false;

        // Safety: check shutdown before entering the loop
        if self.shutdown.load(Ordering::Relaxed) {
            return Ok(ReviewOutcome::Failed {
                story_key: story.story_key.clone(),
                error: "Shutdown requested (Ctrl+C)".to_string(),
            });
        }
        let mut chat_history: Vec<(String, String)> =
            vec![(initial_message.to_string(), current_response.clone())];
        const MAX_RETRIES: usize = 3;

        loop {
            // Cooperative shutdown check — between chat turns
            if self.shutdown.load(Ordering::Relaxed) {
                tracing::info!(
                    action = "shutdown_requested",
                    turn = %turn,
                    story_key = %story.story_key,
                    "Shutdown requested — exiting review session"
                );
                return Ok(ReviewOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: "Shutdown requested (Ctrl+C)".to_string(),
                });
            }

            // Safety net
            if turn >= MAX_REVIEW_TURNS {
                tracing::warn!(
                    action = "review_max_turns",
                    turns = %turn,
                    "Maximum review turn limit exceeded"
                );
                return Ok(ReviewOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Maximum review turn limit exceeded ({MAX_REVIEW_TURNS})"),
                });
            }

            // Post-review phase: the agent's response IS the report
            if post_review_phase {
                tracing::info!(
                    action = "review_report_captured",
                    story_key = %story.story_key,
                    report_len = %current_response.len(),
                    "Review report captured from agent"
                );
                return Ok(ReviewOutcome::Completed {
                    story_key: story.story_key.clone(),
                    branch: story.branch_name.clone(),
                    report: current_response,
                });
            }

            // Normal phase: analyze response
            let action = self
                .analyzer
                .analyze(&current_response, &escalation_slot, &story_reply);

            let reply = match action {
                ResponseAction::Completed => {
                    tracing::info!(
                        action = "review_cr_complete",
                        turn = %turn,
                        "CR workflow completion detected — entering post-review phase"
                    );
                    post_review_phase = true;
                    POST_REVIEW_MESSAGE.to_string()
                }
                ResponseAction::Escalated => {
                    tracing::warn!(
                        action = "review_escalated",
                        turn = %turn,
                        "Review session escalated — treating as failure"
                    );
                    return Ok(ReviewOutcome::Failed {
                        story_key: story.story_key.clone(),
                        error: "Review session escalated to human".to_string(),
                    });
                }
                ResponseAction::Continue { reply } => reply,
                ResponseAction::NoReply => "Continue.".to_string(),
            };

            // Build rig message history
            let history: Vec<Message> = chat_history
                .iter()
                .flat_map(|(user, assistant)| {
                    vec![Message::user(user), Message::assistant(assistant)]
                })
                .collect();

            log_llm_request("code-review", turn, &reply, history.len());
            match agent
                .stream_chat(reply.as_str(), history, Some(&self.shutdown))
                .await
            {
                Ok(r) => {
                    log_llm_response("code-review", turn, &r);
                    retries = 0;
                    chat_history.push((reply, r.clone()));
                    current_response = r;
                }
                Err(e) => {
                    log_llm_error("code-review", turn, &e);
                    retries += 1;
                    tracing::warn!(
                        action = "review_chat_error",
                        turn = %turn,
                        retries = %retries,
                        error = %e,
                        "Review chat error, will retry"
                    );
                    if retries >= MAX_RETRIES {
                        return Ok(ReviewOutcome::Failed {
                            story_key: story.story_key.clone(),
                            error: format!("Review chat failed after {MAX_RETRIES} retries: {e}"),
                        });
                    }
                    continue;
                }
            }

            tracing::debug!(
                action = "review_chat_turn",
                turn = %turn,
                response_len = %current_response.len(),
                "Review chat turn completed"
            );

            turn += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ReviewError tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_review_error_display_variants() {
        let cases: Vec<(ReviewError, &str)> = vec![
            (
                ReviewError::ProviderInit {
                    reason: "connection refused".into(),
                },
                "Provider initialization failed: connection refused",
            ),
            (
                ReviewError::ApiKeyMissing {
                    provider: "anthropic".into(),
                    env_var: "ANTHROPIC_API_KEY".into(),
                },
                "API key missing for provider 'anthropic' (env var: ANTHROPIC_API_KEY)",
            ),
            (
                ReviewError::UnsupportedProvider {
                    provider: "cohere".into(),
                },
                "Unsupported provider: cohere",
            ),
            (
                ReviewError::ChatFailed {
                    turn: 5,
                    reason: "timeout".into(),
                },
                "Chat failed at turn 5: timeout",
            ),
            (
                ReviewError::AgentBuildFailed {
                    reason: "missing tool".into(),
                },
                "Agent build failed: missing tool",
            ),
        ];

        for (error, expected) in cases {
            let display = format!("{error}");
            assert_eq!(display, expected, "Mismatch for error: {error:?}");
        }
    }

    #[test]
    fn test_review_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ReviewError>();
    }

    // -----------------------------------------------------------------------
    // ReviewOutcome tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_review_outcome_completed_fields() {
        let outcome = ReviewOutcome::Completed {
            story_key: "5-2-code-review".to_string(),
            branch: "story/5-2-code-review".to_string(),
            report: "## Review Summary\n\n3 issues found, 3 fixed.".to_string(),
        };
        match outcome {
            ReviewOutcome::Completed {
                story_key,
                branch,
                report,
                ..
            } => {
                assert_eq!(story_key, "5-2-code-review");
                assert_eq!(branch, "story/5-2-code-review");
                assert!(report.contains("Review Summary"));
                assert!(report.contains("3 issues found"));
            }
            _ => panic!("Expected Completed variant"),
        }
    }

    #[test]
    fn test_review_outcome_skipped_fields() {
        let outcome = ReviewOutcome::Skipped {
            reason: "Provider unavailable".to_string(),
        };
        match outcome {
            ReviewOutcome::Skipped { reason } => {
                assert_eq!(reason, "Provider unavailable");
            }
            _ => panic!("Expected Skipped variant"),
        }
    }

    #[test]
    fn test_review_outcome_failed_fields() {
        let outcome = ReviewOutcome::Failed {
            story_key: "3-1-supervisor".to_string(),
            error: "Chat timeout after 100 turns".to_string(),
        };
        match outcome {
            ReviewOutcome::Failed { story_key, error } => {
                assert_eq!(story_key, "3-1-supervisor");
                assert!(error.contains("100 turns"));
            }
            _ => panic!("Expected Failed variant"),
        }
    }

    #[test]
    fn test_review_outcome_debug() {
        let outcome = ReviewOutcome::Completed {
            story_key: "key".to_string(),
            branch: "b".to_string(),
            report: "report".to_string(),
        };
        let debug = format!("{outcome:?}");
        assert!(debug.contains("Completed"));
        assert!(debug.contains("key"));
    }

    // -----------------------------------------------------------------------
    // ReviewRunner tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_review_runner_new_stores_config() {
        use crate::config::*;

        let config = Arc::new(BotConfig::_test_minimal("pretty", "info"));
        let secrets = Arc::new(BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: None,
            github_copilot_oauth_token: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });

        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let agent_factory = Arc::new(AgentFactory::new(config.clone(), secrets.clone()));
        let runner = ReviewRunner::new(config.clone(), secrets, agent_factory, shutdown);
        // Verify config is stored by checking a known field
        assert_eq!(runner.config.llm.review.provider, "anthropic");
    }

    #[test]
    fn test_review_runner_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ReviewRunner>();
    }

    // -----------------------------------------------------------------------
    // Constants tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_post_review_message_contains_key_instructions() {
        assert!(POST_REVIEW_MESSAGE.contains("commit"));
        assert!(POST_REVIEW_MESSAGE.contains("commit messages"));
        assert!(POST_REVIEW_MESSAGE.contains("findings"));
        assert!(POST_REVIEW_MESSAGE.contains("markdown summary"));
        assert!(POST_REVIEW_MESSAGE.contains("PR comment"));
    }

    #[test]
    fn test_max_review_turns_is_reasonable() {
        assert!(MAX_REVIEW_TURNS >= 50, "Max turns should be at least 50");
        assert!(MAX_REVIEW_TURNS <= 200, "Max turns should be at most 200");
    }
}
