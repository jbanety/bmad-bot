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

use futures::StreamExt;

/// Maximum tool-call rounds allowed per single prompt in the streaming loop.
const STREAMING_MAX_TURNS: usize = 100;

/// Send a prompt via streaming and collect the complete text response (review variant).
///
/// Same pattern as `session::runner::streaming_chat` but returns a `PromptError`
/// compatible error type for use in the review chat loop.
async fn streaming_review_chat<A, M>(
    agent: &A,
    prompt: impl Into<Message> + Send,
    history: Vec<Message>,
) -> Result<String, rig::completion::PromptError>
where
    A: StreamingChat<M, M::StreamingResponse>,
    M: CompletionModel + 'static,
    M::StreamingResponse: Clone + Unpin + GetTokenUsage,
{
    let mut stream = agent
        .stream_chat(prompt, history)
        .multi_turn(STREAMING_MAX_TURNS)
        .await;

    let mut acc = String::new();

    loop {
        let Some(chunk) = stream.next().await else {
            break;
        };

        match chunk {
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(
                Text { text },
            ))) => {
                acc.push_str(&text);
            }
            Ok(MultiTurnStreamItem::FinalResponse(_)) => {
                break;
            }
            Err(e) => {
                return Err(rig::completion::PromptError::CompletionError(
                    rig::completion::CompletionError::ResponseError(e.to_string()),
                ));
            }
            _ => continue,
        }
    }

    Ok(acc)
}
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::completion::{Chat, CompletionModel, GetTokenUsage, Message};
use rig::message::Text;
use rig::providers::{anthropic, openai};
use rig::streaming::{StreamedAssistantContent, StreamingChat};

use crate::auth::github_copilot::{CopilotTokenCache, ReqwestCopilotHttpClient};
use crate::config::{BotConfig, BotSecrets};
use crate::llm_logging::{log_llm_error, log_llm_request, log_llm_response};
use crate::session::analyzer::{ResponseAction, ResponseAnalyzer};
use crate::session::provider::{ProviderError, copilot_headers, resolve_api_key};
use crate::supervisor::decisions::DecisionLog;
use crate::supervisor::{AskSupervisor, EscalationSlot};
use crate::tools::{FsTool, GitTool, TerminalTool};
use crate::watcher::StoryInfo;

/// Maximum chat turns for a review session (safety net).
const MAX_REVIEW_TURNS: usize = 100;

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

    /// Failed to read the BMAD dev agent persona file.
    #[error("Preamble load failed for '{path}': {reason}")]
    PreambleLoadFailed {
        /// Path to the file that could not be read.
        path: String,
        /// Why reading failed.
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
pub struct ReviewRunner {
    /// Shared daemon configuration.
    config: Arc<BotConfig>,
    /// Shared secrets (API keys loaded from `.env`).
    secrets: Arc<BotSecrets>,
    /// Stateless response analyzer (constructed once, reused).
    analyzer: ResponseAnalyzer,
}

impl ReviewRunner {
    /// Create a new review runner.
    pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>) -> Self {
        Self {
            config,
            secrets,
            analyzer: ResponseAnalyzer::new(),
        }
    }

    /// Run a code review session for the given story.
    ///
    /// This method NEVER panics or returns an unhandled error. All failures
    /// are caught and returned as [`ReviewOutcome::Skipped`] or [`ReviewOutcome::Failed`].
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        match self.run_inner(story).await {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::error!(
                    action = "review_failed",
                    error = %e,
                    story_key = %story.story_key,
                    "Code review failed — skipping"
                );
                ReviewOutcome::Skipped {
                    reason: e.to_string(),
                }
            }
        }
    }

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

        // 1. Resolve API key for review provider
        let api_key = resolve_api_key(&self.config.llm.review.provider, &self.secrets).map_err(
            |e| match e {
                ProviderError::MissingApiKey { env_var, .. } => ReviewError::ApiKeyMissing {
                    provider: self.config.llm.review.provider.clone(),
                    env_var,
                },
                other => ReviewError::ProviderInit {
                    reason: other.to_string(),
                },
            },
        )?;

        // 2. Load BMAD dev agent preamble (same as SessionRunner)
        let preamble = self.build_preamble()?;

        // 3. Create shared resources
        let escalation_slot: EscalationSlot = Arc::new(std::sync::Mutex::new(None));
        let decision_log = DecisionLog::new();

        let provider = &self.config.llm.review.provider;
        let model = &self.config.llm.review.model;

        // 4. Build agent and run — match on provider because Chat is NOT object-safe
        let outcome = match provider.as_str() {
            "anthropic" => {
                let client: anthropic::Client = anthropic::Client::builder()
                    .api_key(&api_key)
                    .build()
                    .map_err(|e| ReviewError::ProviderInit {
                        reason: e.to_string(),
                    })?;

                let project_root = PathBuf::from(&self.config.bmad_paths.project_root);
                let (git, fs, terminal, supervisor) = self.create_tools(
                    &project_root,
                    escalation_slot.clone(),
                    decision_log.clone(),
                )?;

                let agent = client
                    .agent(model)
                    .preamble(&preamble)
                    .tool(git)
                    .tool(fs)
                    .tool(terminal)
                    .tool(supervisor)
                    .build();

                self.drive_review_session(&agent, story, escalation_slot, decision_log)
                    .await
            }
            "openai" => {
                let client: openai::Client = openai::Client::builder()
                    .api_key(&api_key)
                    .build()
                    .map_err(|e| ReviewError::ProviderInit {
                        reason: e.to_string(),
                    })?;

                let project_root = PathBuf::from(&self.config.bmad_paths.project_root);
                let (git, fs, terminal, supervisor) = self.create_tools(
                    &project_root,
                    escalation_slot.clone(),
                    decision_log.clone(),
                )?;

                let agent = client
                    .agent(model)
                    .preamble(&preamble)
                    .tool(git)
                    .tool(fs)
                    .tool(terminal)
                    .tool(supervisor)
                    .build();

                self.drive_review_session(&agent, story, escalation_slot, decision_log)
                    .await
            }
            "github-copilot" => {
                // Exchange OAuth token for short-lived Copilot session token
                let http_client = ReqwestCopilotHttpClient::new();
                let mut cache = CopilotTokenCache::new();
                let (session_token, base_url) = cache
                    .resolve(&http_client, &api_key)
                    .await
                    .map_err(|e| ReviewError::ProviderInit {
                        reason: format!("Copilot token exchange failed: {e}"),
                    })?;

                let client: openai::CompletionsClient = openai::Client::builder()
                    .api_key(&session_token)
                    .base_url(&base_url)
                    .http_headers(copilot_headers())
                    .build()
                    .map_err(|e| ReviewError::ProviderInit {
                        reason: e.to_string(),
                    })?
                    .completions_api();

                let project_root = PathBuf::from(&self.config.bmad_paths.project_root);
                let (git, fs, terminal, supervisor) = self.create_tools(
                    &project_root,
                    escalation_slot.clone(),
                    decision_log.clone(),
                )?;

                let agent = client
                    .agent(model)
                    .preamble(&preamble)
                    .tool(git)
                    .tool(fs)
                    .tool(terminal)
                    .tool(supervisor)
                    .build();

                self.drive_review_session(&agent, story, escalation_slot, decision_log)
                    .await
            }
            other => Err(ReviewError::UnsupportedProvider {
                provider: other.into(),
            }),
        }?;

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

    /// Build the agent preamble from the BMAD dev agent file with language override.
    ///
    /// Identical to `SessionRunner::build_preamble` — loads `dev.md` and appends
    /// English override for consistency.
    fn build_preamble(&self) -> Result<String, ReviewError> {
        let agent_path =
            Path::new(&self.config.bmad_paths.project_root).join("_bmad/bmm/agents/dev.md");

        let agent_content =
            std::fs::read_to_string(&agent_path).map_err(|e| ReviewError::PreambleLoadFailed {
                path: agent_path.display().to_string(),
                reason: e.to_string(),
            })?;

        Ok(format!(
            "{agent_content}\n\nOVERRIDE: communication_language = English"
        ))
    }

    /// Create the 4 tools for the rig agent: git, filesystem, terminal, ask_supervisor.
    fn create_tools(
        &self,
        project_root: &Path,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
    ) -> Result<(GitTool, FsTool, TerminalTool, AskSupervisor), ReviewError> {
        let git = GitTool::new(project_root.to_path_buf());
        let fs = FsTool::new(project_root.to_path_buf());
        let terminal = TerminalTool::new(project_root.to_path_buf(), TERMINAL_TIMEOUT_SECS);

        let supervisor =
            AskSupervisor::with_architect_from_config(&self.config, escalation_slot, decision_log)
                .map_err(|e| ReviewError::AgentBuildFailed {
                    reason: format!("Failed to create AskSupervisor: {e}"),
                })?;

        Ok((git, fs, terminal, supervisor))
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
    async fn drive_review_session<A, M>(
        &self,
        agent: &A,
        story: &StoryInfo,
        escalation_slot: EscalationSlot,
        _decision_log: DecisionLog,
    ) -> Result<ReviewOutcome, ReviewError>
    where
        A: Chat + StreamingChat<M, M::StreamingResponse>,
        M: CompletionModel + 'static,
        M::StreamingResponse: Clone + Unpin + GetTokenUsage,
    {
        // The CR workflow asks "which story file to review" — reply with the file path
        let story_reply = story.specs_path.display().to_string();

        // Send initial message "CR"
        let initial_message = "CR";
        let history: Vec<Message> = vec![];
        log_llm_request("code-review", 0, initial_message, history.len());
        let response = streaming_review_chat(agent, initial_message, history)
            .await
            .map_err(|e| {
                log_llm_error("code-review", 0, &e);
                ReviewError::ChatFailed {
                    turn: 0,
                    reason: e.to_string(),
                }
            })?;
        log_llm_response("code-review", 0, &response);

        tracing::debug!(
            action = "review_chat_turn",
            turn = 0,
            response_len = %response.len(),
            "Initial review chat turn completed"
        );

        let mut current_response = response;
        let mut turn: usize = 1;
        let mut retries: usize = 0;
        let mut post_review_phase = false;
        let mut chat_history: Vec<(String, String)> =
            vec![(initial_message.to_string(), current_response.clone())];
        const MAX_RETRIES: usize = 3;

        loop {
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
            match streaming_review_chat(agent, reply.as_str(), history).await {
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
            (
                ReviewError::PreambleLoadFailed {
                    path: "/path/to/dev.md".into(),
                    reason: "file not found".into(),
                },
                "Preamble load failed for '/path/to/dev.md': file not found",
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

        let runner = ReviewRunner::new(config.clone(), secrets);
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
