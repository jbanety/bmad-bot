//! BMAD Architect session for supervisor LLM fallback.
//!
//! When the rule engine cannot answer an agent question, the supervisor
//! creates a fresh BMAD Architect agent session. The daemon acts as a
//! simulated human, driving a multi-turn conversation:
//!
//! 1. Send `"CH"` to enter free chat mode
//! 2. Send `"Load the project context"` so the Architect loads docs via `ReadFile`
//! 3. Send the developer's question — capture and return the Architect's answer
//!
//! Each `ask()` call creates an entirely fresh session (no persistence).

use crate::auth::github_copilot::{CopilotTokenCache, ReqwestCopilotHttpClient};
use crate::config::BotConfig;
use async_trait::async_trait;
use rig::client::CompletionClient;
use rig::completion::{Chat, Message};
use rig::providers::{anthropic, openai};
use std::path::PathBuf;

use super::read_tool::ReadFile;

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
/// Each `ask()` call creates a brand-new rig agent, drives a 3-turn conversation,
/// and discards the session. No state is persisted between calls.
#[derive(Debug)]
pub struct ArchitectSession {
    /// Full content of `architect.md` — used as the agent preamble.
    agent_file_content: String,
    /// Provider name (`"anthropic"`, `"openai"`, `"github-copilot"`).
    provider: String,
    /// Model identifier (e.g. `"claude-sonnet-4-20250514"`, `"gpt-4o"`).
    model: String,
    /// Resolved API key value (read from env at construction time).
    api_key: String,
    /// Project root path — for the ReadFile tool boundary.
    project_root: PathBuf,
}

/// The environment variable name for a given provider.
fn env_var_for_provider(provider: &str) -> Result<&'static str, ArchitectSessionError> {
    match provider {
        "anthropic" => Ok("ANTHROPIC_API_KEY"),
        "openai" => Ok("OPENAI_API_KEY"),
        "github-copilot" => Ok("GITHUB_COPILOT_OAUTH_TOKEN"),
        other => Err(ArchitectSessionError::UnsupportedProvider {
            provider: other.to_string(),
        }),
    }
}

impl ArchitectSession {
    /// Create a new `ArchitectSession` from the daemon's [`BotConfig`].
    ///
    /// This reads the `architect.md` file, resolves the supervisor LLM config,
    /// and reads the API key from the environment. It does **not** create a rig
    /// agent — that happens per question in [`ask()`](AnswerProvider::ask).
    pub fn new(config: &BotConfig) -> Result<Self, ArchitectSessionError> {
        // 1. Resolve path to architect.md
        let agent_path =
            PathBuf::from(&config.bmad_paths.project_root).join("_bmad/bmm/agents/architect.md");

        let agent_path_str = agent_path.display().to_string();

        // 2. Read the full agent file
        let agent_file_content = std::fs::read_to_string(&agent_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ArchitectSessionError::AgentFileNotFound {
                    path: agent_path_str.clone(),
                }
            } else {
                ArchitectSessionError::AgentFileReadFailed {
                    path: agent_path_str.clone(),
                    reason: e.to_string(),
                }
            }
        })?;

        // 3. Read supervisor LLM config
        let provider = config.llm.supervisor.provider.clone();
        let model = config.llm.supervisor.model.clone();

        // 4. Resolve the API key from environment
        let env_var = env_var_for_provider(&provider)?;
        let api_key = std::env::var(env_var).map_err(|_| ArchitectSessionError::ApiKeyMissing {
            env_var: env_var.to_string(),
        })?;

        if api_key.is_empty() {
            return Err(ArchitectSessionError::ApiKeyMissing {
                env_var: env_var.to_string(),
            });
        }

        // 5. Resolve project root
        let project_root = PathBuf::from(&config.bmad_paths.project_root);

        Ok(Self {
            agent_file_content,
            provider,
            model,
            api_key,
            project_root,
        })
    }

    /// Build the question message for Turn 3 of the Architect session.
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

    /// Drive a multi-turn chat with the given agent.
    ///
    /// Sends 3 turns: "CH" → "Load the project context" → question.
    /// Returns the Architect's response from Turn 3.
    async fn drive_conversation<A: Chat>(
        agent: &A,
        question: &str,
        context: Option<&str>,
    ) -> Result<String, ArchitectSessionError> {
        let mut chat_history: Vec<Message> = vec![];

        // Turn 1: Enter free chat mode
        tracing::warn!(
            action = "supervisor_fallback",
            turn = 1,
            "Architect session turn — entering CH mode"
        );
        let response = agent.chat("CH", chat_history.clone()).await.map_err(|e| {
            ArchitectSessionError::ChatFailed {
                turn: 1,
                reason: e.to_string(),
            }
        })?;
        chat_history.push(Message::user("CH"));
        chat_history.push(Message::assistant(&response));

        // Turn 2: Load project context
        tracing::warn!(
            action = "supervisor_fallback",
            turn = 2,
            "Architect session turn — loading project context"
        );
        let response = agent
            .chat("Load the project context", chat_history.clone())
            .await
            .map_err(|e| ArchitectSessionError::ChatFailed {
                turn: 2,
                reason: e.to_string(),
            })?;
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
        let answer = agent.chat(&question_msg, chat_history).await.map_err(|e| {
            ArchitectSessionError::ChatFailed {
                turn: 3,
                reason: e.to_string(),
            }
        })?;

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

        // Build the agent using the configured provider.
        // Each provider returns a different Agent<M> type, so we use
        // match arms that each call drive_conversation() directly —
        // avoiding the need for a trait object over the non-object-safe Chat trait.
        match self.provider.as_str() {
            "anthropic" => {
                let client: anthropic::Client = anthropic::Client::builder()
                    .api_key(&self.api_key)
                    .build()
                    .map_err(|e| ArchitectSessionError::ProviderInit {
                        reason: format!("Anthropic client init failed: {e}"),
                    })?;

                let agent = client
                    .agent(&self.model)
                    .preamble(&self.agent_file_content)
                    .tool(read_tool)
                    .build();

                Self::drive_conversation(&agent, question, context).await
            }
            "openai" => {
                let client: openai::Client = openai::Client::builder()
                    .api_key(&self.api_key)
                    .build()
                    .map_err(|e| ArchitectSessionError::ProviderInit {
                        reason: format!("OpenAI client init failed: {e}"),
                    })?;

                let agent = client
                    .agent(&self.model)
                    .preamble(&self.agent_file_content)
                    .tool(read_tool)
                    .build();

                Self::drive_conversation(&agent, question, context).await
            }
            "github-copilot" => {
                // Exchange the long-lived OAuth token for a short-lived Copilot session token
                // and derive the API base URL from the session token's proxy-ep field.
                let http_client = ReqwestCopilotHttpClient::new();
                let mut cache = CopilotTokenCache::new();
                let (session_token, base_url) = cache
                    .resolve(&http_client, &self.api_key)
                    .await
                    .map_err(|e| ArchitectSessionError::ProviderInit {
                        reason: format!("Copilot token exchange failed: {e}"),
                    })?;

                let client: openai::Client = openai::Client::builder()
                    .api_key(&session_token)
                    .base_url(&base_url)
                    .build()
                    .map_err(|e| ArchitectSessionError::ProviderInit {
                        reason: format!("GitHub Copilot client init failed: {e}"),
                    })?;

                let agent = client
                    .agent(&self.model)
                    .preamble(&self.agent_file_content)
                    .tool(read_tool)
                    .build();

                Self::drive_conversation(&agent, question, context).await
            }
            other => Err(ArchitectSessionError::UnsupportedProvider {
                provider: other.to_string(),
            }),
        }
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
    fn test_env_var_for_provider_known() {
        assert_eq!(
            env_var_for_provider("anthropic").unwrap(),
            "ANTHROPIC_API_KEY"
        );
        assert_eq!(env_var_for_provider("openai").unwrap(), "OPENAI_API_KEY");
        assert_eq!(
            env_var_for_provider("github-copilot").unwrap(),
            "GITHUB_COPILOT_OAUTH_TOKEN"
        );
    }

    #[test]
    fn test_env_var_for_provider_unknown() {
        assert!(matches!(
            env_var_for_provider("deepseek").unwrap_err(),
            ArchitectSessionError::UnsupportedProvider { .. }
        ));
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
