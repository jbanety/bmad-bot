//! BMAD Architect session for supervisor LLM fallback.
//!
//! When the rule engine cannot answer an agent question, the supervisor
//! creates a fresh BMAD Architect agent session. The daemon acts as a
//! simulated human, driving a multi-turn conversation:
//!
//! 0. Activate the agent by sending `architect.md` as a user message (BMAD activation)
//! 1. Send `"Execute [CH]"` with English language override to enter free chat mode
//! 2. Send `"Load the project context"` so the Architect loads docs via tools
//! 3. Send the developer's question — capture and return the Architect's answer
//!
//! The Architect agent is equipped with the same tools as the dev agent
//! (read_file, edit_file, grep, find_path, list_directory, git, terminal,
//! ThinkTool) minus `ask_supervisor` (would cause infinite recursion).
//!
//! Each `ask()` call creates an entirely fresh session (no persistence).

use crate::config::{BotConfig, BotSecrets};
use crate::llm::agent_factory::{AgentFactory, BuiltAgent, LlmRole};
use crate::llm::logging::{log_llm_error, log_llm_request, log_llm_response};
use crate::session::agent::{build_preamble, create_base_tools, ShutdownFlag};
use crate::ui::UiHandle;
use async_trait::async_trait;
use rig::completion::Message;
use std::path::PathBuf;
use std::sync::Arc;

use rig::tools::think::ThinkTool;

/// Relative path from project root to the architect agent file.
const ARCHITECT_AGENT_PATH: &str = "_bmad/bmm/agents/architect.md";

// -----------------------------------------------------------------------
// Error type
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// Error type
// -----------------------------------------------------------------------

/// Errors from the Architect session lifecycle.
#[derive(Debug, thiserror::Error)]
pub enum ArchitectSessionError {
    /// The `architect.md` agent file was not found at the expected path.
    #[error("Architect agent file not found: {path}")]
    AgentFileNotFound {
        /// Path where the file was expected.
        path: String,
    },

    /// I/O error reading the `architect.md` agent file.
    #[error("Failed to read architect agent file '{path}': {reason}")]
    AgentFileReadFailed {
        /// Path to the agent file.
        path: String,
        /// Description of the I/O error.
        reason: String,
    },

    /// Failed to create the rig provider client.
    #[error("Provider initialization failed: {reason}")]
    ProviderInit {
        /// Description of the initialization failure.
        reason: String,
    },

    /// The required API key environment variable is not set.
    #[error("API key missing: environment variable '{env_var}' is not set")]
    ApiKeyMissing {
        /// Name of the environment variable that was expected.
        env_var: String,
    },

    /// The configured provider string is not recognised.
    #[error("Unsupported LLM provider: '{provider}' — expected 'anthropic' or 'openai'")]
    UnsupportedProvider {
        /// The provider string from config.
        provider: String,
    },

    /// A chat turn failed during the Architect session.
    #[error("Chat turn {turn} failed: {reason}")]
    ChatFailed {
        /// Which turn (1-based) failed.
        turn: u32,
        /// Description of the failure.
        reason: String,
    },

    /// The Architect returned an empty response.
    #[error("Architect returned empty response")]
    NoResponse,
}

// -----------------------------------------------------------------------
// AnswerProvider trait — enables mock testing
// -----------------------------------------------------------------------

/// Trait abstracting the supervisor's LLM fallback capability.
///
/// `ArchitectSession` implements this for real LLM calls.
/// Tests use `MockAnswerProvider` for deterministic responses.
#[async_trait]
pub trait AnswerProvider: Send + Sync + std::fmt::Debug {
    /// Ask the provider a question with optional context.
    /// Returns the answer string on success.
    async fn ask(
        &self,
        question: &str,
        context: Option<&str>,
    ) -> Result<String, ArchitectSessionError>;
}

// -----------------------------------------------------------------------
// ArchitectSession
// -----------------------------------------------------------------------

/// Holds the configuration needed to create fresh BMAD Architect sessions on demand.
///
/// Each `ask()` call creates a brand-new rig agent via [`AgentFactory`], drives a
/// 4-turn conversation (activation → CH → context → question), and discards the
/// session. No state is persisted between calls.
#[derive(Debug)]
pub struct ArchitectSession {
    /// Centralized agent construction factory.
    agent_factory: Arc<AgentFactory>,
    /// Project root path — for the ReadFile tool boundary.
    project_root: PathBuf,
    /// MCP server manager — provides external tool capabilities.
    mcp_manager: Arc<crate::mcp::McpManager>,
}

impl ArchitectSession {
    /// Create a new `ArchitectSession` from the daemon's [`BotConfig`].
    ///
    /// This reads the `architect.md` file and stores a reference to the
    /// [`AgentFactory`]. It does **not** create a rig agent — that happens
    /// per question in [`ask()`](AnswerProvider::ask).
    ///
    /// # Legacy constructor
    ///
    /// The previous constructor resolved API keys from the environment directly.
    /// Now the `AgentFactory` handles all provider setup (API keys, token
    /// exchange, API format detection).
    pub fn new(config: &BotConfig) -> Result<Self, ArchitectSessionError> {
        Self::new_with_factory(config, None, Arc::new(crate::mcp::McpManager::empty()))
    }

    /// Create a new `ArchitectSession` with an explicit [`AgentFactory`].
    ///
    /// When `factory` is `None`, a new factory is created from the config and
    /// environment secrets (backward-compatible with the legacy constructor).
    pub fn new_with_factory(
        config: &BotConfig,
        factory: Option<Arc<AgentFactory>>,
        mcp_manager: Arc<crate::mcp::McpManager>,
    ) -> Result<Self, ArchitectSessionError> {
        // 1. Validate architect.md exists (fail-fast)
        let agent_path = PathBuf::from(&config.bmad_paths.project_root).join(ARCHITECT_AGENT_PATH);

        let agent_path_str = agent_path.display().to_string();

        if !agent_path.exists() {
            return Err(ArchitectSessionError::AgentFileNotFound {
                path: agent_path_str,
            });
        }

        // 2. Create or use provided AgentFactory
        let agent_factory = match factory {
            Some(f) => f,
            None => {
                // Legacy path: create a factory from environment secrets.
                // The env_var_for_provider() helper validates the provider name
                // and the factory will resolve the key when build() is called.
                let provider = &config.llm.supervisor.provider;
                let env_var = match provider.as_str() {
                    "anthropic" => "ANTHROPIC_API_KEY",
                    "openai" => "OPENAI_API_KEY",
                    other => {
                        return Err(ArchitectSessionError::UnsupportedProvider {
                            provider: other.to_string(),
                        });
                    }
                };

                // Validate the key exists (fail-fast at construction time)
                let api_key =
                    std::env::var(env_var).map_err(|_| ArchitectSessionError::ApiKeyMissing {
                        env_var: env_var.to_string(),
                    })?;
                if api_key.is_empty() {
                    return Err(ArchitectSessionError::ApiKeyMissing {
                        env_var: env_var.to_string(),
                    });
                }

                let secrets = Arc::new(crate::config::BotSecrets {
                    anthropic_api_key: if provider == "anthropic" {
                        Some(api_key.clone())
                    } else {
                        std::env::var("ANTHROPIC_API_KEY").ok()
                    },
                    openai_api_key: if provider == "openai" {
                        Some(api_key.clone())
                    } else {
                        std::env::var("OPENAI_API_KEY").ok()
                    },
                    github_token: std::env::var("GITHUB_TOKEN").ok(),
                    gitlab_token: std::env::var("GITLAB_TOKEN").ok(),
                    telegram_bot_token: std::env::var("TELEGRAM_BOT_TOKEN").ok(),
                });

                // Build a temporary BotConfig-compatible Arc for the factory
                Arc::new(AgentFactory::new(Arc::new((*config).clone()), secrets))
            }
        };

        // 3. Resolve project root
        let project_root = PathBuf::from(&config.bmad_paths.project_root);

        Ok(Self {
            agent_factory,
            project_root,
            mcp_manager,
        })
    }

    /// Build the question message for the final turn of the Architect session.
    fn build_question_message(question: &str, context: Option<&str>) -> String {
        match context {
            Some(ctx) => format!(
                "A developer agent working on this project has the following question: {question}\n\n\
                 Additional context from the developer: {ctx}"
            ),
            None => format!(
                "A developer agent working on this project has the following question: {question}"
            ),
        }
    }

    /// Drive a multi-turn chat with the given agent after activation.
    ///
    /// Expects the agent to already be activated (activation history provided).
    /// Sends 3 turns: "Execute [CH]" → "Load the project context" → question.
    /// Returns the Architect's response from the question turn.
    async fn drive_conversation(
        agent: &BuiltAgent,
        mut chat_history: Vec<Message>,
        question: &str,
        context: Option<&str>,
    ) -> Result<String, ArchitectSessionError> {
        // Turn 1: Enter free chat mode with English language override
        let ch_msg = "IMPORTANT: ALL communication MUST be in English regardless of config file settings. Execute [CH]";
        tracing::warn!(
            action = "supervisor_fallback",
            turn = 1,
            "Architect session turn — entering CH mode"
        );
        log_llm_request("supervisor", 1, ch_msg, chat_history.len());
        let (response, _) = agent
            .stream_chat(ch_msg, chat_history.clone(), None, None)
            .await
            .map_err(|e| {
                log_llm_error("supervisor", 1, &e);
                ArchitectSessionError::ChatFailed {
                    turn: 1,
                    reason: e.to_string(),
                }
            })?;
        log_llm_response("supervisor", 1, &response);
        chat_history.push(Message::user(ch_msg));
        chat_history.push(Message::assistant(&response));

        // Turn 2: Load project context
        tracing::warn!(
            action = "supervisor_fallback",
            turn = 2,
            "Architect session turn — loading project context"
        );
        log_llm_request(
            "supervisor",
            2,
            "Load the project context",
            chat_history.len(),
        );
        let (response, _) = agent
            .stream_chat("Load the project context", chat_history.clone(), None, None)
            .await
            .map_err(|e| {
                log_llm_error("supervisor", 2, &e);
                ArchitectSessionError::ChatFailed {
                    turn: 2,
                    reason: e.to_string(),
                }
            })?;
        log_llm_response("supervisor", 2, &response);
        chat_history.push(Message::user("Load the project context"));
        chat_history.push(Message::assistant(&response));

        // Turn 3: Ask the developer's question
        let question_msg = Self::build_question_message(question, context);
        tracing::warn!(
            action = "supervisor_fallback",
            turn = 3,
            question = %question,
            "Architect session turn — asking developer question"
        );
        log_llm_request("supervisor", 3, &question_msg, chat_history.len());
        let (answer, _) = agent
            .stream_chat(question_msg.as_str(), chat_history, None, None)
            .await
            .map_err(|e| {
                log_llm_error("supervisor", 3, &e);
                ArchitectSessionError::ChatFailed {
                    turn: 3,
                    reason: e.to_string(),
                }
            })?;
        log_llm_response("supervisor", 3, &answer);

        if answer.trim().is_empty() {
            return Err(ArchitectSessionError::NoResponse);
        }

        tracing::info!(
            action = "supervisor_fallback_response",
            response_len = answer.len(),
            "Architect answered"
        );

        Ok(answer)
    }
}

#[async_trait]
impl AnswerProvider for ArchitectSession {
    async fn ask(
        &self,
        question: &str,
        context: Option<&str>,
    ) -> Result<String, ArchitectSessionError> {
        // All standard tools except ask_supervisor (would recurse).
        let (git, read_file, edit_file, grep, find_path, list_dir, terminal) =
            create_base_tools(&self.project_root);
        let project_root_str = self.project_root.display().to_string();

        // Build the agent via AgentFactory with the generic preamble (tool rules,
        // English override). The architect persona is sent as a user message during
        // activation — same pattern as agent.rs.
        let mcp_data = self.mcp_manager.tools_for_builder().await;
        let mcp_tool_names = crate::mcp::extract_mcp_tool_names(&mcp_data);
        let supervisor_model = &self
            .agent_factory
            .config_for_role(LlmRole::Supervisor)
            .model;
        let preamble = build_preamble(&mcp_tool_names, supervisor_model);
        let agent = self
            .agent_factory
            .build(
                LlmRole::Supervisor,
                &preamble,
                // TODO (post-12.4): Migrate the Architect off the 4-turn scripted handshake.
                //
                // The current flow (architect.md activation → [CH] → load-context → question)
                // does not map to spawn_agent's single-message contract. Per architecture.md
                // Decision 10 (_bmad-output/planning-artifacts/architecture.md:664–694), the
                // Architect is a supervisor-fallback AnswerProvider, not an LLM-initiated
                // delegation target — so a straight spawn_agent migration would also conflate
                // the two delegation flavors. Migration becomes feasible once either:
                //   (a) spawn_agent gains a multi-turn activation API, or
                //   (b) the Architect adopts a skill-based activation path (Story 12.1
                //       precedent for dev sessions).
                // Tracked by Story 12.4 AC-7.
                crate::configure_agent_tools!(
                    git, read_file, edit_file, grep, find_path, list_dir, terminal, ThinkTool
                )
                .with_mcp(mcp_data),
            )
            .await
            .map_err(|e| ArchitectSessionError::ProviderInit {
                reason: format!("Agent build failed: {e}"),
            })?;

        // Activate: send architect.md as user message (BMAD activation flow)
        let (activation_history, _chat_history) = agent
            .activate_agent(
                &project_root_str,
                ARCHITECT_AGENT_PATH,
                "supervisor",
                None,
                None,
            )
            .await
            .map_err(|e| ArchitectSessionError::ChatFailed {
                turn: 0,
                reason: format!("Agent activation failed: {e}"),
            })?;

        // Drive CH → context → question with the activated history
        Self::drive_conversation(&agent, activation_history, question, context).await
    }
}

// -----------------------------------------------------------------------
// MockAnswerProvider — for unit tests
// -----------------------------------------------------------------------

/// Mock implementation of [`AnswerProvider`] for unit tests.
///
/// Returns a fixed response string, or an error if `should_fail` is set.
#[derive(Debug)]
pub struct MockAnswerProvider {
    /// The response to return on success.
    pub response: String,
    /// If `true`, `ask()` returns an error instead of the response.
    pub should_fail: bool,
}

#[async_trait]
impl AnswerProvider for MockAnswerProvider {
    async fn ask(
        &self,
        question: &str,
        _context: Option<&str>,
    ) -> Result<String, ArchitectSessionError> {
        if self.should_fail {
            Err(ArchitectSessionError::ChatFailed {
                turn: 3,
                reason: format!("Mock failure for question: {question}"),
            })
        } else {
            Ok(self.response.clone())
        }
    }
}

// -----------------------------------------------------------------------
// SdkAnswerProvider — SDK subprocess-based supervisor fallback
// -----------------------------------------------------------------------

/// SDK-based implementation of [`AnswerProvider`] for supervisor LLM fallback.
///
/// Spawns a CLI subprocess (Claude Code or Codex) with a combined prompt
/// containing the architect persona, English override, and the developer's
/// question. No supervisor MCP is injected (prevents recursion).
#[derive(Debug)]
pub struct SdkAnswerProvider {
    config: Arc<BotConfig>,
    secrets: Arc<BotSecrets>,
    config_path: PathBuf,
    shutdown: ShutdownFlag,
    ui: UiHandle,
}

impl SdkAnswerProvider {
    /// Create a new SDK-based answer provider.
    pub fn new(
        config: Arc<BotConfig>,
        secrets: Arc<BotSecrets>,
        config_path: PathBuf,
        shutdown: ShutdownFlag,
        ui: UiHandle,
    ) -> Self {
        Self {
            config,
            secrets,
            config_path,
            shutdown,
            ui,
        }
    }

    fn build_prompt(question: &str, context: Option<&str>) -> String {
        let mut prompt = String::from(
            "You are Winston, a Senior Architect. A developer agent working on this project \
             needs your expert guidance. IMPORTANT: ALL communication MUST be in English.\n\n\
             Use your tools to explore the codebase and project documentation as needed to \
             give an accurate, well-informed answer.\n\n",
        );

        prompt.push_str(&format!("**Developer's question:** {question}\n"));

        if let Some(ctx) = context {
            prompt.push_str(&format!("\n**Additional context:** {ctx}\n"));
        }

        prompt.push_str(
            "\nProvide a clear, actionable answer. If you need to explore files to answer \
             accurately, do so before responding.",
        );

        prompt
    }
}

#[async_trait]
impl AnswerProvider for SdkAnswerProvider {
    async fn ask(
        &self,
        question: &str,
        context: Option<&str>,
    ) -> Result<String, ArchitectSessionError> {
        use crate::runtime::sdk::SdkRuntime;

        let role_config = crate::runtime::resolve_role_config(&self.config.llm, &LlmRole::Supervisor);
        let project_root = std::path::Path::new(&self.config.bmad_paths.project_root);

        let prompt = Self::build_prompt(question, context);

        let session_config = match role_config.provider.as_str() {
            "claude-code" => crate::runtime::sdk_claude::build_claude_code_config(
                role_config,
                project_root,
                &prompt,
                None, // No MCP supervisor — prevents recursion
            ),
            "codex" => crate::runtime::sdk_codex::build_codex_config(
                role_config,
                project_root,
                &prompt,
            ),
            other => {
                return Err(ArchitectSessionError::UnsupportedProvider {
                    provider: other.to_string(),
                });
            }
        };

        tracing::info!(
            provider = %role_config.provider,
            model = %role_config.model,
            "Supervisor SDK fallback: spawning subprocess"
        );

        let sdk_runtime = SdkRuntime::new(
            Arc::clone(&self.config),
            Arc::clone(&self.secrets),
            self.config_path.clone(),
            Arc::clone(&self.shutdown),
            self.ui.clone(),
        );

        let is_claude = role_config.provider == "claude-code";
        let parser = move |line: &str| -> Option<crate::runtime::sdk::SdkOutputEvent> {
            if is_claude {
                crate::runtime::sdk_claude::parse_claude_code_line(line)
            } else {
                crate::runtime::sdk_codex::parse_codex_line(line)
            }
        };

        let result = sdk_runtime
            .execute_session(session_config, parser)
            .await
            .map_err(|e| ArchitectSessionError::ChatFailed {
                turn: 1,
                reason: format!("SDK subprocess failed: {e}"),
            })?;

        if result.exit_code != Some(0) {
            let error = result
                .stream_error
                .or_else(|| {
                    if result.stderr.is_empty() {
                        None
                    } else {
                        Some(result.stderr.clone())
                    }
                })
                .unwrap_or_else(|| format!("exit code {:?}", result.exit_code));
            return Err(ArchitectSessionError::ChatFailed {
                turn: 1,
                reason: format!("SDK subprocess failed: {error}"),
            });
        }

        let answer = result
            .completion_text
            .filter(|t| !t.trim().is_empty())
            .ok_or(ArchitectSessionError::NoResponse)?;

        tracing::info!(
            response_len = answer.len(),
            "Supervisor SDK fallback answered"
        );

        Ok(answer)
    }
}

// -----------------------------------------------------------------------
// build_answer_provider — factory for API vs SDK dispatch
// -----------------------------------------------------------------------

/// Build the appropriate [`AnswerProvider`] based on the supervisor role config.
///
/// - SDK provider (claude-code, codex) → [`SdkAnswerProvider`]
/// - API provider (anthropic, openai) → [`ArchitectSession`]
#[allow(clippy::too_many_arguments)]
pub fn build_answer_provider(
    config: &BotConfig,
    config_arc: Arc<BotConfig>,
    secrets: Arc<BotSecrets>,
    config_path: PathBuf,
    shutdown: ShutdownFlag,
    ui: UiHandle,
    factory: Option<Arc<AgentFactory>>,
    mcp_manager: Arc<crate::mcp::McpManager>,
) -> Result<Box<dyn AnswerProvider>, ArchitectSessionError> {
    let role_config = crate::runtime::resolve_role_config(&config.llm, &LlmRole::Supervisor);

    if role_config.is_sdk_provider() {
        tracing::info!(
            provider = %role_config.provider,
            "Supervisor LLM fallback: using SDK provider"
        );
        Ok(Box::new(SdkAnswerProvider::new(
            config_arc,
            secrets,
            config_path,
            shutdown,
            ui,
        )))
    } else {
        tracing::info!(
            provider = %role_config.provider,
            "Supervisor LLM fallback: using API provider"
        );
        Ok(Box::new(ArchitectSession::new_with_factory(
            config, factory, mcp_manager,
        )?))
    }
}

// -----------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Build a minimal BotConfig pointing at the given temp dir as project_root.
    fn make_test_config(dir: &TempDir, provider: &str, write_agent: bool) -> BotConfig {
        if write_agent {
            let agents_dir = dir.path().join("_bmad/bmm/agents");
            std::fs::create_dir_all(&agents_dir).unwrap();
            std::fs::write(
                agents_dir.join("architect.md"),
                "# Architect Agent\nTest persona content",
            )
            .unwrap();
        }

        let mut config = BotConfig::_test_minimal("pretty", "info");
        config.bmad_paths.project_root = dir.path().display().to_string();
        config.llm.supervisor.provider = provider.to_string();
        config.llm.supervisor.model = "test-model".to_string();
        config
    }

    #[test]
    fn test_architect_session_missing_agent_file() {
        let dir = TempDir::new().unwrap();
        let config = make_test_config(&dir, "openai", false);
        let result = ArchitectSession::new(&config);
        assert!(matches!(
            result.unwrap_err(),
            ArchitectSessionError::AgentFileNotFound { .. }
        ));
    }

    #[test]
    fn test_architect_session_missing_api_key() {
        let dir = TempDir::new().unwrap();
        let config = make_test_config(&dir, "openai", true);
        // Ensure env var is NOT set
        // SAFETY: test-only, single-threaded test runner for this module
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
        let result = ArchitectSession::new(&config);
        assert!(matches!(
            result.unwrap_err(),
            ArchitectSessionError::ApiKeyMissing { .. }
        ));
    }

    #[test]
    fn test_architect_session_unsupported_provider() {
        let dir = TempDir::new().unwrap();
        // Set a fake key env var to get past the API key check
        // SAFETY: test-only, single-threaded test runner for this module
        unsafe { std::env::set_var("TEST_SUPERVISOR_KEY_3_2", "fake-key") };
        let config = make_test_config(&dir, "unsupported-provider", true);
        let result = ArchitectSession::new(&config);
        assert!(matches!(
            result.unwrap_err(),
            ArchitectSessionError::UnsupportedProvider { .. }
        ));
        unsafe { std::env::remove_var("TEST_SUPERVISOR_KEY_3_2") };
    }

    #[test]
    fn test_architect_session_error_display() {
        let err = ArchitectSessionError::AgentFileNotFound {
            path: "/some/path".to_string(),
        };
        assert!(err.to_string().contains("/some/path"));

        let err = ArchitectSessionError::AgentFileReadFailed {
            path: "/some/path".to_string(),
            reason: "permission denied".to_string(),
        };
        assert!(err.to_string().contains("/some/path"));
        assert!(err.to_string().contains("permission denied"));

        let err = ArchitectSessionError::ChatFailed {
            turn: 2,
            reason: "timeout".to_string(),
        };
        assert!(err.to_string().contains("2"));
        assert!(err.to_string().contains("timeout"));

        let err = ArchitectSessionError::ApiKeyMissing {
            env_var: "OPENAI_API_KEY".to_string(),
        };
        assert!(err.to_string().contains("OPENAI_API_KEY"));

        let err = ArchitectSessionError::UnsupportedProvider {
            provider: "deepseek".to_string(),
        };
        assert!(err.to_string().contains("deepseek"));

        let err = ArchitectSessionError::ProviderInit {
            reason: "bad config".to_string(),
        };
        assert!(err.to_string().contains("bad config"));

        let err = ArchitectSessionError::NoResponse;
        assert!(err.to_string().contains("empty response"));
    }

    #[test]
    fn test_architect_session_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ArchitectSessionError>();
    }

    #[test]
    fn test_build_question_message_without_context() {
        let msg = ArchitectSession::build_question_message("How should I handle auth?", None);
        assert!(msg.contains("How should I handle auth?"));
        assert!(!msg.contains("Additional context"));
    }

    #[test]
    fn test_build_question_message_with_context() {
        let msg = ArchitectSession::build_question_message(
            "How should I handle auth?",
            Some("Using JWT tokens currently"),
        );
        assert!(msg.contains("How should I handle auth?"));
        assert!(msg.contains("Additional context"));
        assert!(msg.contains("JWT tokens"));
    }

    #[test]
    fn test_architect_agent_path_constant() {
        assert_eq!(ARCHITECT_AGENT_PATH, "_bmad/bmm/agents/architect.md");
    }

    #[tokio::test]
    async fn test_mock_answer_provider_success() {
        let mock = MockAnswerProvider {
            response: "Use JWT with refresh tokens".to_string(),
            should_fail: false,
        };
        let result = mock.ask("How should I handle auth?", None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Use JWT with refresh tokens");
    }

    #[tokio::test]
    async fn test_mock_answer_provider_with_context() {
        let mock = MockAnswerProvider {
            response: "Consider OAuth2".to_string(),
            should_fail: false,
        };
        let result = mock
            .ask("How should I handle auth?", Some("enterprise context"))
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Consider OAuth2");
    }

    #[tokio::test]
    async fn test_mock_answer_provider_failure() {
        let mock = MockAnswerProvider {
            response: "unused".to_string(),
            should_fail: true,
        };
        let result = mock.ask("How should I handle auth?", None).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ArchitectSessionError::ChatFailed { turn, reason } => {
                assert_eq!(turn, 3);
                assert!(reason.contains("Mock failure"));
            }
            other => panic!("Expected ChatFailed, got: {other}"),
        }
    }

    #[test]
    fn test_answer_provider_is_object_safe() {
        // Verify we can construct a Box<dyn AnswerProvider>
        let mock: Box<dyn AnswerProvider> = Box::new(MockAnswerProvider {
            response: "test".to_string(),
            should_fail: false,
        });
        // Just verifying it compiles and the Debug impl works
        let _debug = format!("{:?}", mock);
    }

    // -- SdkAnswerProvider tests --

    #[test]
    fn test_sdk_answer_provider_build_prompt_without_context() {
        let prompt = SdkAnswerProvider::build_prompt("What pattern should I follow?", None);
        assert!(prompt.contains("Winston"));
        assert!(prompt.contains("What pattern should I follow?"));
        assert!(!prompt.contains("Additional context"));
    }

    #[test]
    fn test_sdk_answer_provider_build_prompt_with_context() {
        let prompt = SdkAnswerProvider::build_prompt(
            "What pattern should I follow?",
            Some("Using the observer pattern"),
        );
        assert!(prompt.contains("Winston"));
        assert!(prompt.contains("What pattern should I follow?"));
        assert!(prompt.contains("Additional context"));
        assert!(prompt.contains("observer pattern"));
    }

    #[test]
    fn test_sdk_answer_provider_is_debug() {
        let config = Arc::new(BotConfig::_test_minimal("pretty", "info"));
        let secrets = Arc::new(crate::config::BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let provider = SdkAnswerProvider::new(
            config,
            secrets,
            PathBuf::from("test.yaml"),
            shutdown,
            crate::ui::UiHandle::null(),
        );
        let debug = format!("{:?}", provider);
        assert!(debug.contains("SdkAnswerProvider"));
    }

    #[test]
    fn test_sdk_answer_provider_implements_answer_provider() {
        let config = Arc::new(BotConfig::_test_minimal("pretty", "info"));
        let secrets = Arc::new(crate::config::BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let provider = SdkAnswerProvider::new(
            config,
            secrets,
            PathBuf::from("test.yaml"),
            shutdown,
            crate::ui::UiHandle::null(),
        );
        let _boxed: Box<dyn AnswerProvider> = Box::new(provider);
    }

    // -- build_answer_provider tests --

    #[test]
    fn test_build_answer_provider_returns_sdk_for_sdk_provider() {
        let dir = TempDir::new().unwrap();
        let mut config = make_test_config(&dir, "claude-code", true);
        config.llm.supervisor.provider = "claude-code".to_string();
        let config_arc = Arc::new(config.clone());
        let secrets = Arc::new(crate::config::BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result = build_answer_provider(
            &config,
            config_arc,
            secrets,
            PathBuf::from("test.yaml"),
            shutdown,
            crate::ui::UiHandle::null(),
            None,
            Arc::new(crate::mcp::McpManager::empty()),
        );
        assert!(result.is_ok());
        let debug = format!("{:?}", result.unwrap());
        assert!(debug.contains("SdkAnswerProvider"));
    }

    #[test]
    fn test_build_answer_provider_returns_architect_for_api_provider() {
        let dir = TempDir::new().unwrap();
        let config = make_test_config(&dir, "anthropic", true);
        let config_arc = Arc::new(config.clone());
        let secrets = Arc::new(crate::config::BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let factory = Arc::new(AgentFactory::new(
            Arc::clone(&config_arc),
            Arc::clone(&secrets),
        ));
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let result = build_answer_provider(
            &config,
            config_arc,
            secrets,
            PathBuf::from("test.yaml"),
            shutdown,
            crate::ui::UiHandle::null(),
            Some(factory),
            Arc::new(crate::mcp::McpManager::empty()),
        );
        assert!(result.is_ok());
        let debug = format!("{:?}", result.unwrap());
        assert!(debug.contains("ArchitectSession"));
    }
}
