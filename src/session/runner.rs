//! Session runner — builds a rig agent, runs the chat loop, and manages lifecycle.
//!
//! The [`SessionRunner`] is the daemon's execution engine for a single story.
//! It follows the architecture's Hybrid Chat Loop + Supervisor Tool pattern:
//!
//! 1. Build a rig agent with the BMAD dev persona and 4 tools
//! 2. Create a WAL state file for crash recovery
//! 3. Run the chat loop: send "DS", then auto-respond to workflow interactions
//! 4. Handle the result: cleanup WAL on success, preserve partial work on failure
//!
//! **Key design constraint:** rig's `Chat` trait is not object-safe, so we use
//! match arms on the provider name to construct and drive concrete agent types
//! (following the established pattern from `supervisor/architect.rs`).

use crate::config::{BotConfig, BotSecrets};
use crate::session::SessionOutcome;
use crate::session::analyzer::{ResponseAction, ResponseAnalyzer};
use crate::session::cleanup::{mark_story_needs_clarification, preserve_partial_work};
use crate::session::escalation::EscalationReport;
use crate::session::provider::{ProviderError, resolve_api_key};
use crate::session::state::SessionState;
use crate::supervisor::decisions::{DecisionLog, write_decisions_file};
use crate::supervisor::{AskSupervisor, EscalationSlot};
use crate::tools::{FsTool, GitTool, TerminalTool};
use crate::watcher::StoryInfo;
use rig::client::CompletionClient;
use rig::completion::{Chat, Message};
use rig::providers::{anthropic, openai};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Maximum number of chat turns before the safety net kicks in.
///
/// Prevents infinite loops if the agent never signals completion. A future
/// improvement could make this configurable via `BotConfig`.
const MAX_CHAT_TURNS: usize = 200;

/// Terminal tool timeout in seconds for commands executed by the agent.
const TERMINAL_TIMEOUT_SECS: u64 = 30;

/// Session runner — manages the full lifecycle of a single story development session.
///
/// Constructed once per daemon run and reused across stories. Each call to
/// [`run()`](Self::run) creates a fresh agent, WAL, and chat loop for one story.
pub struct SessionRunner {
    /// Shared daemon configuration.
    config: Arc<BotConfig>,
    /// Shared secrets (API keys loaded from `.env`).
    secrets: Arc<BotSecrets>,
    /// Path to the WAL state file: `{implementation_artifacts}/.bmad-bot-session.yaml`.
    state_file_path: PathBuf,
    /// Stateless response analyzer (constructed once, reused).
    analyzer: ResponseAnalyzer,
}

impl SessionRunner {
    /// Create a new session runner.
    ///
    /// The `state_file_path` is derived from
    /// `config.bmad_paths.implementation_artifacts` + `/.bmad-bot-session.yaml`.
    pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>) -> Self {
        let state_file_path =
            Path::new(&config.bmad_paths.implementation_artifacts).join(".bmad-bot-session.yaml");
        Self {
            config,
            secrets,
            state_file_path,
            analyzer: ResponseAnalyzer::new(),
        }
    }

    /// Run a full development session for the given story.
    ///
    /// Opens a `tracing::info_span!("story_session")` and drives the entire
    /// session lifecycle: build agent → create WAL → chat loop → cleanup.
    ///
    /// Returns a [`SessionOutcome`] indicating success, escalation, or failure.
    pub async fn run(&self, story: &StoryInfo) -> SessionOutcome {
        let span = tracing::info_span!(
            "story_session",
            story_id = %story.story_id,
            branch = %story.branch_name
        );
        let _guard = span.enter();

        tracing::info!(
            action = "session_start",
            story_key = %story.story_key,
            "Starting dev session"
        );

        // Resolve API key before building agent
        let api_key = match resolve_api_key(&self.config.llm.dev.provider, &self.secrets) {
            Ok(key) => key,
            Err(e) => {
                tracing::error!(
                    action = "session_failed",
                    error = %e,
                    "Failed to resolve API key"
                );
                return SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Provider setup failed: {e}"),
                    decisions: vec![],
                };
            }
        };

        // Create shared resources for supervisor
        let escalation_slot: EscalationSlot = Arc::new(std::sync::Mutex::new(None));
        let decision_log = DecisionLog::new();

        let provider = &self.config.llm.dev.provider;
        let model = &self.config.llm.dev.model;

        // Build agent and run chat loop — match on provider because Chat is not object-safe
        let outcome = match provider.as_str() {
            "anthropic" => {
                match self.build_anthropic_agent(
                    story,
                    &api_key,
                    model,
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(agent) => {
                        self.run_session(
                            &agent,
                            story,
                            provider,
                            model,
                            escalation_slot.clone(),
                            decision_log.clone(),
                        )
                        .await
                    }
                    Err(e) => SessionOutcome::Failed {
                        story_key: story.story_key.clone(),
                        error: format!("Agent build failed: {e}"),
                        decisions: decision_log.records(),
                    },
                }
            }
            "openai" => {
                match self.build_openai_agent(
                    story,
                    &api_key,
                    model,
                    None,
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(agent) => {
                        self.run_session(
                            &agent,
                            story,
                            provider,
                            model,
                            escalation_slot.clone(),
                            decision_log.clone(),
                        )
                        .await
                    }
                    Err(e) => SessionOutcome::Failed {
                        story_key: story.story_key.clone(),
                        error: format!("Agent build failed: {e}"),
                        decisions: decision_log.records(),
                    },
                }
            }
            "github-models" => {
                match self.build_openai_agent(
                    story,
                    &api_key,
                    model,
                    Some("https://models.inference.ai.azure.com"),
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(agent) => {
                        self.run_session(
                            &agent,
                            story,
                            provider,
                            model,
                            escalation_slot.clone(),
                            decision_log.clone(),
                        )
                        .await
                    }
                    Err(e) => SessionOutcome::Failed {
                        story_key: story.story_key.clone(),
                        error: format!("Agent build failed: {e}"),
                        decisions: decision_log.records(),
                    },
                }
            }
            other => SessionOutcome::Failed {
                story_key: story.story_key.clone(),
                error: format!("Unsupported provider: {other}"),
                decisions: vec![],
            },
        };

        let outcome_type = match &outcome {
            SessionOutcome::Completed { .. } => "completed",
            SessionOutcome::Escalated { .. } => "escalated",
            SessionOutcome::Failed { .. } => "failed",
        };

        tracing::info!(
            action = "session_end",
            outcome = %outcome_type,
            "Dev session ended"
        );

        outcome
    }

    /// Build an Anthropic agent with the BMAD dev persona and 4 tools.
    fn build_anthropic_agent(
        &self,
        story: &StoryInfo,
        api_key: &str,
        model: &str,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
    ) -> Result<impl Chat, ProviderError> {
        let preamble = self.build_preamble(story)?;

        let client: anthropic::Client = anthropic::Client::builder()
            .api_key(api_key)
            .build()
            .map_err(|e| ProviderError::ClientCreation {
                provider: "anthropic".to_string(),
                reason: e.to_string(),
            })?;

        let project_root = PathBuf::from(&self.config.bmad_paths.project_root);
        let (git, fs, terminal, supervisor) =
            self.create_tools(&project_root, escalation_slot, decision_log)?;

        let agent = client
            .agent(model)
            .preamble(&preamble)
            .tool(git)
            .tool(fs)
            .tool(terminal)
            .tool(supervisor)
            .build();

        tracing::info!(
            action = "agent_built",
            tools = 4,
            model = %model,
            provider = "anthropic",
            "Rig agent built"
        );

        Ok(agent)
    }

    /// Build an OpenAI-compatible agent (also used for GitHub Models with base URL override).
    fn build_openai_agent(
        &self,
        story: &StoryInfo,
        api_key: &str,
        model: &str,
        base_url: Option<&str>,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
    ) -> Result<impl Chat, ProviderError> {
        let preamble = self.build_preamble(story)?;
        let provider_name = if base_url.is_some() {
            "github-models"
        } else {
            "openai"
        };

        let client: openai::Client = if let Some(url) = base_url {
            openai::Client::builder()
                .api_key(api_key)
                .base_url(url)
                .build()
                .map_err(|e| ProviderError::ClientCreation {
                    provider: provider_name.to_string(),
                    reason: e.to_string(),
                })?
        } else {
            openai::Client::builder()
                .api_key(api_key)
                .build()
                .map_err(|e| ProviderError::ClientCreation {
                    provider: provider_name.to_string(),
                    reason: e.to_string(),
                })?
        };

        let project_root = PathBuf::from(&self.config.bmad_paths.project_root);
        let (git, fs, terminal, supervisor) =
            self.create_tools(&project_root, escalation_slot, decision_log)?;

        let agent = client
            .agent(model)
            .preamble(&preamble)
            .tool(git)
            .tool(fs)
            .tool(terminal)
            .tool(supervisor)
            .build();

        tracing::info!(
            action = "agent_built",
            tools = 4,
            model = %model,
            provider = %provider_name,
            "Rig agent built"
        );

        Ok(agent)
    }

    /// Build the agent preamble from the BMAD dev agent file with language override.
    fn build_preamble(&self, _story: &StoryInfo) -> Result<String, ProviderError> {
        let agent_path =
            Path::new(&self.config.bmad_paths.project_root).join("_bmad/bmm/agents/dev.md");

        let agent_content =
            std::fs::read_to_string(&agent_path).map_err(|e| ProviderError::ClientCreation {
                provider: "preamble".to_string(),
                reason: format!(
                    "Failed to read BMAD dev agent file '{}': {e}",
                    agent_path.display()
                ),
            })?;

        // Append language override — the daemon forces English for consistency
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
    ) -> Result<(GitTool, FsTool, TerminalTool, AskSupervisor), ProviderError> {
        let git = GitTool::new(project_root.to_path_buf());
        let fs = FsTool::new(project_root.to_path_buf());
        let terminal = TerminalTool::new(project_root.to_path_buf(), TERMINAL_TIMEOUT_SECS);

        let supervisor =
            AskSupervisor::with_architect_from_config(&self.config, escalation_slot, decision_log)
                .map_err(|e| ProviderError::ClientCreation {
                    provider: "supervisor".to_string(),
                    reason: format!("Failed to create AskSupervisor: {e}"),
                })?;

        Ok((git, fs, terminal, supervisor))
    }

    /// Run the chat loop with a concrete agent that implements [`Chat`].
    ///
    /// This is the provider-agnostic core: send "DS", analyze responses,
    /// auto-respond, and handle completion/escalation/failure.
    async fn run_session<A: Chat>(
        &self,
        agent: &A,
        story: &StoryInfo,
        provider: &str,
        model: &str,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
    ) -> SessionOutcome {
        // Create WAL state
        let mut state = SessionState::new(story, provider, model);

        // Save initial WAL
        if let Err(e) = state.save(&self.state_file_path).await {
            tracing::error!(action = "wal_write_failed", error = %e, "Failed to create initial WAL");
            return SessionOutcome::Failed {
                story_key: story.story_key.clone(),
                error: format!("WAL creation failed: {e}"),
                decisions: decision_log.records(),
            };
        }

        // Send initial message "DS"
        let initial_message = "DS";
        state.add_user_message(initial_message);

        let history: Vec<Message> = vec![];
        let response = match agent.chat(initial_message, history).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(action = "chat_failed", turn = 0, error = %e, "Initial chat failed");
                let _ = self.handle_failure(story).await;
                return SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Initial chat failed: {e}"),
                    decisions: decision_log.records(),
                };
            }
        };

        state.add_assistant_message(&response);
        let _ = state.save(&self.state_file_path).await;

        tracing::debug!(
            action = "chat_turn",
            turn = 0,
            response_len = %response.len(),
            "Initial chat turn completed"
        );

        // Enter chat loop
        let mut current_response = response;
        let mut turn: usize = 1;
        let mut retries: usize = 0;
        const MAX_RETRIES: usize = 3;

        loop {
            // Safety net — prevent infinite loops
            if turn >= MAX_CHAT_TURNS {
                tracing::warn!(
                    action = "max_turns_exceeded",
                    turns = %turn,
                    "Maximum chat turn limit exceeded"
                );
                let _ = self.handle_failure(story).await;
                return SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Maximum turn limit exceeded ({MAX_CHAT_TURNS})"),
                    decisions: decision_log.records(),
                };
            }

            // Analyze response
            let action =
                self.analyzer
                    .analyze(&current_response, &escalation_slot, &story.story_key);

            match action {
                ResponseAction::Completed => {
                    tracing::info!(
                        action = "session_completed",
                        turn = %turn,
                        story_key = %story.story_key,
                        "Agent signaled workflow completion"
                    );

                    // Write decisions file (best-effort)
                    self.write_decisions(story, &decision_log).await;

                    // Delete WAL on success
                    let _ = SessionState::delete(&self.state_file_path).await;

                    return SessionOutcome::Completed {
                        story_key: story.story_key.clone(),
                        branch: story.branch_name.clone(),
                        decisions: decision_log.records(),
                    };
                }

                ResponseAction::Escalated => {
                    tracing::warn!(
                        action = "session_escalated",
                        turn = %turn,
                        story_key = %story.story_key,
                        "Escalation detected"
                    );

                    // Extract escalation info from slot
                    let escalation_info = {
                        let guard = escalation_slot.lock().expect("escalation slot lock");
                        guard.clone()
                    };

                    let (question, reason) = match &escalation_info {
                        Some(info) => (info.question.clone(), info.reason.clone()),
                        None => (
                            "Unknown escalation".to_string(),
                            "Escalation slot was empty".to_string(),
                        ),
                    };

                    // Preserve partial work
                    let project_root = PathBuf::from(&self.config.bmad_paths.project_root);
                    let partial_summary =
                        preserve_partial_work(&project_root, &story.story_key, &question).await;

                    // Mark story as needs-clarification (best-effort)
                    let sprint_status_path =
                        Path::new(&self.config.bmad_paths.implementation_artifacts)
                            .join("sprint-status.yaml");
                    let _ =
                        mark_story_needs_clarification(&sprint_status_path, &story.story_key).await;

                    // Build escalation report
                    let report = EscalationReport::new(
                        story.story_key.clone(),
                        question,
                        reason,
                        story.branch_name.clone(),
                        partial_summary,
                    );

                    // Write decisions file (best-effort)
                    self.write_decisions(story, &decision_log).await;

                    // Delete WAL (escalation is a known state, not a crash)
                    let _ = SessionState::delete(&self.state_file_path).await;

                    return SessionOutcome::Escalated {
                        report,
                        decisions: decision_log.records(),
                    };
                }

                ResponseAction::NoReply | ResponseAction::Continue { .. } => {
                    // Extract the reply: NoReply defaults to "Continue." for
                    // forward-compatibility with future rig streaming APIs.
                    let reply = match action {
                        ResponseAction::Continue { reply } => reply,
                        _ => "Continue.".to_string(),
                    };

                    state.add_user_message(&reply);
                    let history = state.to_rig_messages();

                    match agent.chat(&reply, history).await {
                        Ok(r) => {
                            retries = 0;
                            state.add_assistant_message(&r);
                            let _ = state.save(&self.state_file_path).await;
                            current_response = r;
                        }
                        Err(e) => {
                            retries += 1;
                            tracing::warn!(
                                action = "chat_error",
                                turn = %turn,
                                retries = %retries,
                                error = %e,
                                "Chat error, will retry"
                            );
                            if retries >= MAX_RETRIES {
                                let _ = self.handle_failure(story).await;
                                self.write_decisions(story, &decision_log).await;
                                return SessionOutcome::Failed {
                                    story_key: story.story_key.clone(),
                                    error: format!("Chat failed after {MAX_RETRIES} retries: {e}"),
                                    decisions: decision_log.records(),
                                };
                            }
                            // Retry with the same response (don't add to history)
                            // Remove the user message we just added
                            state.chat_history.pop();
                            continue;
                        }
                    }
                }
            }

            tracing::debug!(
                action = "chat_turn",
                turn = %turn,
                response_len = %current_response.len(),
                "Chat turn completed"
            );

            turn += 1;
        }
    }

    /// Handle failure: preserve partial work (best-effort).
    async fn handle_failure(&self, story: &StoryInfo) {
        let project_root = PathBuf::from(&self.config.bmad_paths.project_root);
        let _ = preserve_partial_work(&project_root, &story.story_key, "Session failed").await;
    }

    /// Write decisions file at session end (best-effort).
    async fn write_decisions(&self, story: &StoryInfo, decision_log: &DecisionLog) {
        let decisions = decision_log.records();
        if !decisions.is_empty() {
            let decisions_path = Path::new(&self.config.bmad_paths.implementation_artifacts)
                .join(format!("{}-DECISIONS.md", story.story_key));
            let _ = write_decisions_file(&decisions, &decisions_path, &story.story_key).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;

    /// Helper: create a minimal BotConfig for runner tests.
    fn make_runner_test_config(artifacts_dir: &Path) -> BotConfig {
        BotConfig {
            polling_interval_secs: 10,
            git_provider: GitProviderConfig {
                provider: "github".to_string(),
                repo_owner: "test".to_string(),
                repo_name: "test".to_string(),
                target_branch: "main".to_string(),
            },
            llm: LlmConfig {
                dev: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test-model".to_string(),
                },
                review: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test-model".to_string(),
                },
                supervisor: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test-model".to_string(),
                },
            },
            notifications: NotificationConfig {
                telegram: TelegramConfig {
                    enabled: false,
                    chat_id: String::new(),
                },
            },
            bmad_paths: BmadPathsConfig {
                project_root: artifacts_dir
                    .parent()
                    .unwrap_or(artifacts_dir)
                    .display()
                    .to_string(),
                output_folder: artifacts_dir.display().to_string(),
                planning_artifacts: artifacts_dir.display().to_string(),
                implementation_artifacts: artifacts_dir.display().to_string(),
            },
            log_format: "pretty".to_string(),
            log_level: "info".to_string(),
            log_file: "test.log".to_string(),
        }
    }

    /// Helper: create minimal BotSecrets for tests.
    fn make_test_secrets() -> BotSecrets {
        BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: Some("sk-test".to_string()),
            github_models_api_key: Some("gh-test".to_string()),
            github_token: Some("ghp-test".to_string()),
            gitlab_token: None,
            telegram_bot_token: None,
        }
    }

    #[test]
    fn test_session_runner_new_sets_state_file_path() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let secrets = Arc::new(make_test_secrets());

        let runner = SessionRunner::new(config, secrets);

        assert!(
            runner
                .state_file_path
                .to_str()
                .unwrap()
                .contains(".bmad-bot-session.yaml"),
            "State file path should contain .bmad-bot-session.yaml, got: {:?}",
            runner.state_file_path
        );
    }

    #[test]
    fn test_state_file_path_derived_from_config() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let secrets = Arc::new(make_test_secrets());

        let runner = SessionRunner::new(config, secrets);

        let expected = dir.path().join(".bmad-bot-session.yaml");
        assert_eq!(runner.state_file_path, expected);
    }
}
