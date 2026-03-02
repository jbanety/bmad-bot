//! BMAD Architect session for supervisor LLM fallback.
//!
//! When the rule engine cannot answer an agent question, the supervisor
//! creates a fresh BMAD Architect agent session. The daemon acts as a
//! simulated human, driving a multi-turn conversation:
//!
//! 0. Activate the agent by sending `architect.md` as a user message (BMAD activation)
//! 1. Send `"Execute [CH]"` with English language override to enter free chat mode
//! 2. Send `"Load the project context"` so the Architect loads docs via `ReadFile`
//! 3. Send the developer's question — capture and return the Architect's answer
//!
//! Each `ask()` call creates an entirely fresh session (no persistence).

use crate::config::BotConfig;
use crate::llm::agent_factory::{AgentFactory, BuiltAgent, LlmRole};
use crate::llm::logging::{log_llm_error, log_llm_request, log_llm_response};
use crate::session::dev_agent::build_preamble;
use async_trait::async_trait;
use rig::completion::Message;
use std::path::PathBuf;
use std::sync::Arc;

use super::read_tool::ReadFile;

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
    #[error(
        "Unsupported LLM provider: '{provider}' — expected 'anthropic', 'openai', or 'github-copilot'"
    )]
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
    /// Now the `AgentFactory` handles all provider setup (API keys, Copilot token
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
                    "github-copilot" => "GITHUB_COPILOT_OAUTH_TOKEN",
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
                    github_copilot_oauth_token: if provider == "github-copilot" {
                        Some(api_key)
                    } else {
                        std::env::var("GITHUB_COPILOT_OAUTH_TOKEN").ok()
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
            .stream_chat(ch_msg, chat_history.clone(), None)
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
            .stream_chat("Load the project context", chat_history.clone(), None)
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
            .stream_chat(question_msg.as_str(), chat_history, None)
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
        let read_tool = ReadFile::new(self.project_root.clone());
        let project_root_str = self.project_root.display().to_string();

        // Build the agent via AgentFactory with the generic preamble (tool rules,
        // English override). The architect persona is sent as a user message during
        // activation — same pattern as dev_agent.rs.
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
                crate::configure_agent_tools!(read_tool).with_mcp(mcp_data),
            )
            .await
            .map_err(|e| ArchitectSessionError::ProviderInit {
                reason: format!("Agent build failed: {e}"),
            })?;

        // Activate: send architect.md as user message (BMAD activation flow)
        let (activation_history, _chat_history) = agent
            .activate_agent(&project_root_str, ARCHITECT_AGENT_PATH, "supervisor", None)
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
}
