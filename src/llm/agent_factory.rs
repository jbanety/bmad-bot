//! Centralized LLM provider abstraction — `AgentFactory` + `BuiltAgent` enum dispatch.
//!
//! Since rig's `Chat` trait is not object-safe (associated types, `Self: Sized`),
//! we cannot use `Box<dyn Chat>`. Instead, [`BuiltAgent`] wraps concrete agent types
//! in an enum and dispatches `stream_chat()` via match arms.
//!
//! [`AgentFactory`] centralizes all provider construction: API key resolution
//! and provider configuration happen in one place. Callers
//! (session runner, review runner, supervisor architect) simply call
//! `factory.build(role, preamble, configure_tools)` and get a ready-to-use
//! `BuiltAgent` back.

use crate::config::{BotConfig, BotSecrets, LlmRoleConfig};
use crate::session::agent::streaming_chat;
use crate::session::provider::ProviderError;

use rig::agent::{Agent, AgentBuilder};
use rig::client::CompletionClient;
use rig::completion::Message;
use rig::providers::{anthropic, openai};
use rmcp::model::Tool as McpToolDef;
use rmcp::service::ServerSink;
use std::path::PathBuf;
use std::sync::Arc;

/// Re-export [`ShutdownFlag`] for convenience.
pub use crate::session::agent::ShutdownFlag;

// ---------------------------------------------------------------------------
// LlmRole
// ---------------------------------------------------------------------------

/// Identifies which LLM role is being constructed.
///
/// Each role maps to a separate provider + model pair in [`BotConfig::llm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LlmRole {
    /// Development agent (Amelia) — runs the `dev-story` workflow.
    Dev,
    /// Code review agent — runs the `CR` workflow.
    Review,
    /// Supervisor fallback — answers agent questions via Architect session.
    Supervisor,
    /// Epic review agent — autonomous post-epic retrospective analysis.
    EpicReview,
    /// Story Critic — independent vision guardian for story and decision review.
    Critic,
}

impl std::fmt::Display for LlmRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dev => write!(f, "dev"),
            Self::Review => write!(f, "review"),
            Self::Supervisor => write!(f, "supervisor"),
            Self::EpicReview => write!(f, "epic_review"),
            Self::Critic => write!(f, "critic"),
        }
    }
}

// ---------------------------------------------------------------------------
// BuiltAgent
// ---------------------------------------------------------------------------

/// Provider-erased agent wrapper — dispatches `stream_chat()` via enum match.
///
/// Each variant wraps a concrete rig `Agent<M>` type. Since rig's `Chat` trait
/// is not object-safe, this enum is the canonical way to hold an agent whose
/// provider was determined at runtime.
///
/// ## Variants
///
/// - **Anthropic** — Messages API (native Anthropic provider).
/// - **OpenAiCompatible** — OpenAI-compatible Responses API agent (supports custom base_url).
#[allow(missing_debug_implementations)]
pub enum BuiltAgent {
    /// Anthropic Messages API agent.
    Anthropic(Agent<anthropic::completion::CompletionModel>),
    /// OpenAI-compatible Responses API agent (supports custom base_url).
    OpenAiCompatible(Agent<openai::responses_api::ResponsesCompletionModel>),
}

impl BuiltAgent {
    /// Stream a chat message through the built agent, regardless of provider.
    ///
    /// Delegates to the existing [`streaming_chat()`] function which handles
    /// multi-turn tool calling, streaming chunk accumulation, and cooperative
    /// shutdown.
    ///
    /// # Arguments
    /// - `prompt` — the user message to send.
    /// - `history` — prior conversation messages for context.
    /// - `shutdown` — optional cooperative shutdown flag (pass `None` for
    ///   short-lived sessions like supervisor questions).
    pub async fn stream_chat(
        &self,
        prompt: impl Into<Message> + Send,
        history: Vec<Message>,
        shutdown: Option<&ShutdownFlag>,
        ui: Option<&crate::ui::UiHandle>,
    ) -> Result<(String, Vec<Message>), rig::completion::PromptError> {
        match self {
            Self::Anthropic(agent) => streaming_chat(agent, prompt, history, shutdown, ui).await,
            Self::OpenAiCompatible(agent) => {
                streaming_chat(agent, prompt, history, shutdown, ui).await
            }
        }
    }

    /// Activate a BMAD agent by sending the agent file as the first user message.
    ///
    /// Thin wrapper around [`crate::session::agent::activate_agent()`] that
    /// dispatches to the correct concrete agent type.
    ///
    /// # Arguments
    /// - `project_root` — path to the project root
    /// - `agent_relative_path` — relative path from project root to the agent/skill file
    ///   (e.g. `.claude/skills/bmad-dev-story/SKILL.md` for dev sessions, or
    ///   `_bmad/bmm/agents/architect.md` for persona-based sessions — both remain valid)
    /// - `label` — logging label (e.g. `"dev"`, `"review"`, `"supervisor"`)
    /// - `shutdown` — optional cooperative shutdown flag
    pub async fn activate_agent(
        &self,
        project_root: &str,
        agent_relative_path: &str,
        label: &str,
        shutdown: Option<&ShutdownFlag>,
        ui: Option<&crate::ui::UiHandle>,
    ) -> Result<(Vec<Message>, Vec<crate::session::state::ChatMessage>), String> {
        match self {
            Self::Anthropic(agent) => {
                crate::session::agent::activate_agent(
                    agent,
                    project_root,
                    agent_relative_path,
                    label,
                    shutdown,
                    ui,
                )
                .await
            }
            Self::OpenAiCompatible(agent) => {
                crate::session::agent::activate_agent(
                    agent,
                    project_root,
                    agent_relative_path,
                    label,
                    shutdown,
                    ui,
                )
                .await
            }
        }
    }
}

impl std::fmt::Debug for BuiltAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anthropic(_) => write!(f, "BuiltAgent::Anthropic(..)"),
            Self::OpenAiCompatible(_) => write!(f, "BuiltAgent::OpenAiCompatible(..)"),
        }
    }
}

// We verify Send+Sync at compile time via a const fn trick:
const _: () = {
    // This will fail to compile if BuiltAgent is not Send or Sync.
    fn _assert_send<T: Send>() {}
    fn _assert_sync<T: Sync>() {}
    fn _check() {
        _assert_send::<BuiltAgent>();
        _assert_sync::<BuiltAgent>();
    }
};

// ---------------------------------------------------------------------------
// AgentFactory
// ---------------------------------------------------------------------------

/// Centralized factory for constructing provider-specific [`BuiltAgent`] instances.
///
/// All provider selection, API key resolution, and agent builder configuration
/// is handled here. Callers simply call [`AgentFactory::build()`] with a role,
/// preamble, and tool configurator.
pub struct AgentFactory {
    /// Shared daemon configuration.
    config: Arc<BotConfig>,
    /// Shared secrets (API keys from `.env`).
    secrets: Arc<BotSecrets>,
}

impl std::fmt::Debug for AgentFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentFactory")
            .field("config", &"<BotConfig>")
            .field("secrets", &"<BotSecrets>")
            .finish()
    }
}

impl AgentFactory {
    /// Create a new `AgentFactory`.
    pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>) -> Self {
        Self { config, secrets }
    }

    /// Resolve the [`LlmRoleConfig`] for a given role.
    ///
    /// For [`LlmRole::EpicReview`], falls back to the `review` config when the
    /// `epic_review` provider is empty (zero-config backward compatibility).
    pub fn config_for_role(&self, role: LlmRole) -> &LlmRoleConfig {
        match role {
            LlmRole::Dev => &self.config.llm.dev,
            LlmRole::Review => &self.config.llm.review,
            LlmRole::Supervisor => &self.config.llm.supervisor,
            LlmRole::EpicReview => {
                if self.config.llm.epic_review.provider.is_empty() {
                    &self.config.llm.review // fallback to review config
                } else {
                    &self.config.llm.epic_review
                }
            }
            LlmRole::Critic => {
                if self.config.llm.critic.provider.is_empty() {
                    &self.config.llm.review // fallback to review config
                } else {
                    &self.config.llm.critic
                }
            }
        }
    }

    /// Build a [`BuiltAgent`] for the given role.
    ///
    /// The `configure_tools` closure receives a rig [`AgentBuilder`] and should
    /// attach tools via `.tool(t)` calls, returning the builder. This allows
    /// each call site (session, review, supervisor) to provide its own tool set
    /// while the factory handles provider selection and client construction.
    ///
    /// # Type Parameters
    ///
    /// The closure is generic over the builder type because each provider
    /// produces a different `AgentBuilder<M>`. Internally, the factory calls
    /// the closure inside provider-specific match arms.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] if the API key is missing, the provider is
    /// unsupported, or client construction fails.
    pub async fn build<F>(
        &self,
        role: LlmRole,
        preamble: &str,
        configure_tools: F,
    ) -> Result<BuiltAgent, ProviderError>
    where
        F: AgentConfigurator,
    {
        let role_config = self.config_for_role(role);
        let provider = role_config.provider.as_str();
        let model = &role_config.model;
        let reasoning_effort = role_config.reasoning_effort.as_deref();

        let api_key = resolve_api_key(provider, &self.secrets)?;

        match provider {
            "anthropic" => {
                let mut client_builder = anthropic::Client::builder().api_key(&api_key);

                if let Some(ref url) = role_config.base_url {
                    client_builder = client_builder.base_url(url.trim_end_matches('/'));
                }

                let client: anthropic::Client =
                    client_builder
                        .build()
                        .map_err(|e| ProviderError::ClientCreation {
                            provider: "anthropic".to_string(),
                            reason: e.to_string(),
                        })?;

                let builder = client.agent(model).preamble(preamble);

                if reasoning_effort.is_some() {
                    tracing::warn!(
                        provider = "anthropic",
                        model = %model,
                        role = %role,
                        "reasoning_effort is not supported for Anthropic — ignoring"
                    );
                }

                let agent = configure_tools.configure_anthropic(builder);

                tracing::info!(
                    action = "agent_built",
                    provider = "anthropic",
                    model = %model,
                    role = %role,
                    base_url = role_config.base_url.as_deref().unwrap_or("(default)"),
                    "AgentFactory built agent"
                );

                Ok(BuiltAgent::Anthropic(agent))
            }
            "openai" => {
                let mut client_builder = openai::Client::builder().api_key(&api_key);

                if let Some(ref url) = role_config.base_url {
                    client_builder = client_builder.base_url(url.trim_end_matches('/'));
                }

                let client: openai::Client =
                    client_builder
                        .build()
                        .map_err(|e| ProviderError::ClientCreation {
                            provider: "openai".to_string(),
                            reason: e.to_string(),
                        })?;

                let agent_builder = client.agent(model).preamble(preamble);
                let agent_builder =
                    apply_reasoning_effort(agent_builder, reasoning_effort, "openai", model, role);
                let agent = configure_tools.configure_openai_compatible(agent_builder);

                tracing::info!(
                    action = "agent_built",
                    provider = "openai",
                    model = %model,
                    role = %role,
                    base_url = role_config.base_url.as_deref().unwrap_or("(default)"),
                    reasoning_effort = reasoning_effort.unwrap_or("none"),
                    "AgentFactory built agent (OpenAI-compatible)"
                );

                Ok(BuiltAgent::OpenAiCompatible(agent))
            }
            other => Err(ProviderError::UnsupportedProvider {
                provider: other.to_string(),
            }),
        }
    }

    /// Build a [`BuiltAgent`] with no tools attached.
    ///
    /// Convenience method for call sites that don't need tools (e.g. summarization).
    pub async fn build_bare(
        &self,
        role: LlmRole,
        preamble: &str,
    ) -> Result<BuiltAgent, ProviderError> {
        self.build(role, preamble, NoTools).await
    }

    /// Get a reference to the shared [`BotConfig`].
    pub fn config(&self) -> &BotConfig {
        &self.config
    }

    /// Get a reference to the shared [`BotSecrets`].
    pub fn secrets(&self) -> &BotSecrets {
        &self.secrets
    }

    /// Get the project root path from the config.
    pub fn project_root(&self) -> PathBuf {
        PathBuf::from(&self.config.bmad_paths.project_root)
    }
}

// ---------------------------------------------------------------------------
// AgentConfigurator trait — type-erased tool registration
// ---------------------------------------------------------------------------

/// Trait for configuring tools on agent builders and producing the final `Agent<M>`.
///
/// Because each provider has a different `AgentBuilder<M>` type, and calling
/// `.tool()` changes the builder type from `AgentBuilder` to `AgentBuilderSimple`,
/// the trait methods receive a builder and return the final built `Agent<M>`.
///
/// The most common implementation is [`ToolConfigurator`], which registers the
/// same set of tools on both builder types.
pub trait AgentConfigurator {
    /// Configure tools on an Anthropic agent builder and build the final agent.
    fn configure_anthropic(
        self,
        builder: AgentBuilder<anthropic::completion::CompletionModel>,
    ) -> Agent<anthropic::completion::CompletionModel>;

    /// Configure tools on an OpenAI-compatible Responses API agent builder and build the final agent.
    fn configure_openai_compatible(
        self,
        builder: AgentBuilder<openai::responses_api::ResponsesCompletionModel>,
    ) -> Agent<openai::responses_api::ResponsesCompletionModel>;
}

/// No-tools configurator — returns the builder unchanged.
///
/// Used by [`AgentFactory::build_bare()`] for agents that don't need tools
/// (e.g. summarization agents).
struct NoTools;

impl AgentConfigurator for NoTools {
    fn configure_anthropic(
        self,
        builder: AgentBuilder<anthropic::completion::CompletionModel>,
    ) -> Agent<anthropic::completion::CompletionModel> {
        builder.build()
    }

    fn configure_openai_compatible(
        self,
        builder: AgentBuilder<openai::responses_api::ResponsesCompletionModel>,
    ) -> Agent<openai::responses_api::ResponsesCompletionModel> {
        builder.build()
    }
}

/// Macro to generate an [`AgentConfigurator`] implementation that registers the
/// same set of tools on all three provider builder types.
///
/// # Usage
///
/// ```ignore
/// let configurator = configure_agent_tools!(git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor, ThinkTool);
/// let agent = factory.build(LlmRole::Dev, &preamble, configurator).await?;
/// ```
///
/// Each argument must implement rig's `Tool` trait and be `Clone`.
#[macro_export]
macro_rules! configure_agent_tools {
    ($($tool:expr),+ $(,)?) => {
        $crate::llm::agent_factory::ToolConfigurator {
            tools: ($($tool,)+),
            mcp_servers: vec![],
        }
    };
}

/// A configurator that holds a tuple of tools and registers them on any builder.
///
/// Created via the [`configure_agent_tools!`] macro.
///
/// This struct is generic over the tools tuple. The [`AgentConfigurator`] trait
/// is implemented for specific tuple arities via the macro below.
pub struct ToolConfigurator<T> {
    /// The tools tuple.
    pub tools: T,
    /// MCP server tools and sinks to register alongside native tools.
    ///
    /// Each entry is a `(tools, sink)` pair from a single MCP server.
    /// When empty, no `.rmcp_tools()` calls are made — behavior is identical
    /// to pre-MCP code paths.
    pub mcp_servers: Vec<(Vec<McpToolDef>, ServerSink)>,
}

impl<T> ToolConfigurator<T> {
    /// Inject MCP server tools for registration alongside native tools.
    ///
    /// Pass the output of [`McpManager::tools_for_builder()`] directly.
    /// When `servers` is empty, this is a no-op — no `.rmcp_tools()` calls
    /// are made during agent construction.
    pub fn with_mcp(mut self, servers: Vec<(Vec<McpToolDef>, ServerSink)>) -> Self {
        self.mcp_servers = servers;
        self
    }
}

/// Generate [`AgentConfigurator`] implementations for any tuple arity.
///
/// Each invocation produces an `impl AgentConfigurator for ToolConfigurator<(T1, T2, …)>`
/// that registers the tools on both provider builder types (Anthropic, OpenAI-compatible
/// Responses). Adding or removing tools from a call-site
/// requires zero changes here — the macro already covers arities 1 through 12.
macro_rules! impl_agent_configurator {
    ([$($T:ident),+], [$($t:ident),+]) => {
        impl<$($T),+> AgentConfigurator for ToolConfigurator<($($T,)+)>
        where
            $($T: rig::tool::Tool + Send + Sync + 'static,)+
        {
            fn configure_anthropic(
                self,
                builder: AgentBuilder<anthropic::completion::CompletionModel>,
            ) -> Agent<anthropic::completion::CompletionModel> {
                let ($($t,)+) = self.tools;
                let mut simple = builder$(.tool($t))+;
                for (tools, sink) in self.mcp_servers {
                    simple = simple.rmcp_tools(tools, sink);
                }
                simple.build()
            }

            fn configure_openai_compatible(
                self,
                builder: AgentBuilder<openai::responses_api::ResponsesCompletionModel>,
            ) -> Agent<openai::responses_api::ResponsesCompletionModel> {
                let ($($t,)+) = self.tools;
                let mut simple = builder$(.tool($t))+;
                for (tools, sink) in self.mcp_servers {
                    simple = simple.rmcp_tools(tools, sink);
                }
                simple.build()
            }
        }
    };
}

// Generate AgentConfigurator impls for arities 1–12.
// Any number of tools in configure_agent_tools!() just works.
impl_agent_configurator!([T1], [t1]);
impl_agent_configurator!([T1, T2], [t1, t2]);
impl_agent_configurator!([T1, T2, T3], [t1, t2, t3]);
impl_agent_configurator!([T1, T2, T3, T4], [t1, t2, t3, t4]);
impl_agent_configurator!([T1, T2, T3, T4, T5], [t1, t2, t3, t4, t5]);
impl_agent_configurator!([T1, T2, T3, T4, T5, T6], [t1, t2, t3, t4, t5, t6]);
impl_agent_configurator!([T1, T2, T3, T4, T5, T6, T7], [t1, t2, t3, t4, t5, t6, t7]);
impl_agent_configurator!(
    [T1, T2, T3, T4, T5, T6, T7, T8],
    [t1, t2, t3, t4, t5, t6, t7, t8]
);
impl_agent_configurator!(
    [T1, T2, T3, T4, T5, T6, T7, T8, T9],
    [t1, t2, t3, t4, t5, t6, t7, t8, t9]
);
impl_agent_configurator!(
    [T1, T2, T3, T4, T5, T6, T7, T8, T9, T10],
    [t1, t2, t3, t4, t5, t6, t7, t8, t9, t10]
);
impl_agent_configurator!(
    [T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11],
    [t1, t2, t3, t4, t5, t6, t7, t8, t9, t10, t11]
);
impl_agent_configurator!(
    [T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12],
    [t1, t2, t3, t4, t5, t6, t7, t8, t9, t10, t11, t12]
);

// ---------------------------------------------------------------------------
// apply_reasoning_effort
// ---------------------------------------------------------------------------

/// Apply `reasoning.effort` as `additional_params` on an OpenAI Responses API builder.
///
/// If `effort` is `None`, the builder is returned unchanged. Otherwise, injects:
/// ```json
/// { "reasoning": { "effort": "<value>" } }
/// ```
///
/// Only call this for Responses API builders (OpenAI direct).
fn apply_reasoning_effort<M: rig::completion::CompletionModel>(
    builder: AgentBuilder<M>,
    effort: Option<&str>,
    provider: &str,
    model: &str,
    role: LlmRole,
) -> AgentBuilder<M> {
    match effort {
        Some(level) => {
            tracing::info!(
                provider = provider,
                model = model,
                role = %role,
                reasoning_effort = level,
                "Applying reasoning effort"
            );
            builder.additional_params(serde_json::json!({
                "reasoning": { "effort": level }
            }))
        }
        None => builder,
    }
}

// ---------------------------------------------------------------------------
// resolve_api_key (re-export from provider.rs)
// ---------------------------------------------------------------------------

/// Re-export of [`crate::session::provider::resolve_api_key`] for use by
/// external callers that previously imported it from `session::provider`.
pub use crate::session::provider::resolve_api_key;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // -- LlmRole tests --

    #[test]
    fn test_llm_role_display() {
        assert_eq!(LlmRole::Dev.to_string(), "dev");
        assert_eq!(LlmRole::Review.to_string(), "review");
        assert_eq!(LlmRole::Supervisor.to_string(), "supervisor");
        assert_eq!(LlmRole::EpicReview.to_string(), "epic_review");
        assert_eq!(LlmRole::Critic.to_string(), "critic");
    }

    #[test]
    fn test_llm_role_clone_copy() {
        let role = LlmRole::Dev;
        let cloned = role;
        assert_eq!(role, cloned);
    }

    #[test]
    fn test_llm_role_debug() {
        let debug = format!("{:?}", LlmRole::Dev);
        assert_eq!(debug, "Dev");
        let debug_epic = format!("{:?}", LlmRole::EpicReview);
        assert_eq!(debug_epic, "EpicReview");
        let debug_critic = format!("{:?}", LlmRole::Critic);
        assert!(debug_critic.contains("Critic"));
    }

    #[test]
    fn test_llm_role_eq() {
        assert_eq!(LlmRole::Dev, LlmRole::Dev);
        assert_ne!(LlmRole::Dev, LlmRole::Review);
        assert_ne!(LlmRole::Review, LlmRole::Supervisor);
        assert_ne!(LlmRole::Supervisor, LlmRole::EpicReview);
        assert_eq!(LlmRole::EpicReview, LlmRole::EpicReview);
        assert_eq!(LlmRole::Critic, LlmRole::Critic);
        assert_ne!(LlmRole::Critic, LlmRole::Review);
    }

    // -- AgentFactory tests --

    pub(crate) fn make_test_config() -> BotConfig {
        use crate::config::{
            BmadPathsConfig, GitProviderConfig, LlmConfig, LlmRoleConfig, NotificationConfig,
            TelegramConfig,
        };
        BotConfig {
            polling_interval_secs: 300,
            git_provider: GitProviderConfig {
                provider: "github".to_string(),
                repo_owner: "test-owner".to_string(),
                repo_name: "test-repo".to_string(),
                target_branch: "main".to_string(),
            },
            llm: LlmConfig {
                dev: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "claude-sonnet-4-20250514".to_string(),
                    reasoning_effort: None,
                    base_url: None,
                },
                review: LlmRoleConfig {
                    provider: "openai".to_string(),
                    model: "gpt-4o".to_string(),
                    reasoning_effort: None,
                    base_url: None,
                },
                supervisor: LlmRoleConfig {
                    provider: "openai".to_string(),
                    model: "claude-sonnet-4-20250514".to_string(),
                    reasoning_effort: None,
                    base_url: None,
                },
                epic_review: LlmRoleConfig::default(),
                critic: LlmRoleConfig::default(),
            },
            notifications: NotificationConfig {
                telegram: TelegramConfig {
                    enabled: false,
                    chat_id: String::new(),
                },
            },
            code_review_enabled: true,
            project_brief: None,
            critic_memory_threshold_kb: None,
            bmad_paths: BmadPathsConfig {
                project_root: "/tmp/test-project".to_string(),
                output_folder: "/tmp/test-project/_bmad-output".to_string(),
                planning_artifacts: "/tmp/test-project/_bmad-output/planning-artifacts".to_string(),
                implementation_artifacts: "/tmp/test-project/_bmad-output/implementation-artifacts"
                    .to_string(),
            },
            log_level: "info".to_string(),
            log_format: "pretty".to_string(),
            log_file: "test.log".to_string(),
            ui_mode: "fancy".to_string(),
            ui_verbosity: "normal".to_string(),
            mcp_servers: vec![],
        }
    }

    pub(crate) fn make_test_secrets() -> BotSecrets {
        BotSecrets {
            anthropic_api_key: Some("sk-ant-test-key".to_string()),
            openai_api_key: Some("sk-openai-test-key".to_string()),
            github_token: Some("ghp_test".to_string()),
            gitlab_token: None,
            telegram_bot_token: None,
        }
    }

    fn make_empty_secrets() -> BotSecrets {
        BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        }
    }

    #[test]
    fn test_agent_factory_new() {
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);
        // Should not panic, just validate construction
        assert_eq!(format!("{:?}", factory).contains("AgentFactory"), true);
    }

    #[test]
    fn test_agent_factory_config_for_role_dev() {
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let role_config = factory.config_for_role(LlmRole::Dev);
        assert_eq!(role_config.provider, "anthropic");
        assert_eq!(role_config.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_agent_factory_config_for_role_review() {
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let role_config = factory.config_for_role(LlmRole::Review);
        assert_eq!(role_config.provider, "openai");
        assert_eq!(role_config.model, "gpt-4o");
    }

    #[test]
    fn test_agent_factory_config_for_role_supervisor() {
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let role_config = factory.config_for_role(LlmRole::Supervisor);
        assert_eq!(role_config.provider, "openai");
        assert_eq!(role_config.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_agent_factory_config_for_role_epic_review_fallback() {
        // When epic_review has empty provider, falls back to review config
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let role_config = factory.config_for_role(LlmRole::EpicReview);
        // Should fall back to the review config (openai / gpt-4o)
        assert_eq!(role_config.provider, "openai");
        assert_eq!(role_config.model, "gpt-4o");
    }

    #[test]
    fn test_agent_factory_config_for_role_epic_review_explicit() {
        // When epic_review has an explicit provider, uses its own config
        use crate::config::LlmRoleConfig;
        let mut cfg = make_test_config();
        cfg.llm.epic_review = LlmRoleConfig {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            reasoning_effort: Some("high".to_string()),
            base_url: None,
        };
        let config = Arc::new(cfg);
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let role_config = factory.config_for_role(LlmRole::EpicReview);
        assert_eq!(role_config.provider, "anthropic");
        assert_eq!(role_config.model, "claude-sonnet-4-20250514");
        assert_eq!(role_config.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn test_config_for_role_critic_fallback_to_review() {
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let role_config = factory.config_for_role(LlmRole::Critic);
        assert_eq!(role_config.provider, "openai");
        assert_eq!(role_config.model, "gpt-4o");
    }

    #[test]
    fn test_config_for_role_critic_explicit() {
        use crate::config::LlmRoleConfig;
        let mut cfg = make_test_config();
        cfg.llm.critic = LlmRoleConfig {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            reasoning_effort: None,
            base_url: None,
        };
        let config = Arc::new(cfg);
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let role_config = factory.config_for_role(LlmRole::Critic);
        assert_eq!(role_config.provider, "anthropic");
        assert_eq!(role_config.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_llm_role_critic_display() {
        assert_eq!(LlmRole::Critic.to_string(), "critic");
    }

    #[test]
    fn test_llm_role_critic_debug() {
        let debug = format!("{:?}", LlmRole::Critic);
        assert!(debug.contains("Critic"));
    }

    #[test]
    fn test_llm_role_critic_equality() {
        assert_eq!(LlmRole::Critic, LlmRole::Critic);
        assert_ne!(LlmRole::Critic, LlmRole::Review);
    }

    #[test]
    fn test_llm_role_config_default_has_empty_strings() {
        use crate::config::LlmRoleConfig;
        let default = LlmRoleConfig::default();
        assert!(default.provider.is_empty());
        assert!(default.model.is_empty());
        assert!(default.reasoning_effort.is_none());
        assert!(default.base_url.is_none());
    }

    #[test]
    fn test_agent_factory_project_root() {
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        assert_eq!(factory.project_root(), PathBuf::from("/tmp/test-project"));
    }

    #[tokio::test]
    async fn test_agent_factory_build_missing_api_key() {
        let mut config = make_test_config();
        config.llm.dev.provider = "anthropic".to_string();
        let config = Arc::new(config);
        let secrets = Arc::new(make_empty_secrets());
        let factory = AgentFactory::new(config, secrets);

        let result = factory.build_bare(LlmRole::Dev, "test preamble").await;
        assert!(result.is_err());

        match result.unwrap_err() {
            ProviderError::MissingApiKey { provider, .. } => {
                assert_eq!(provider, "anthropic");
            }
            other => panic!("Expected MissingApiKey, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_agent_factory_build_unsupported_provider() {
        let mut config = make_test_config();
        config.llm.dev.provider = "gemini".to_string();
        let config = Arc::new(config);
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let result = factory.build_bare(LlmRole::Dev, "test preamble").await;
        assert!(result.is_err());

        match result.unwrap_err() {
            ProviderError::UnsupportedProvider { provider } => {
                assert_eq!(provider, "gemini");
            }
            other => panic!("Expected UnsupportedProvider, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_agent_factory_build_anthropic_bare() {
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let result = factory.build_bare(LlmRole::Dev, "test preamble").await;
        assert!(result.is_ok());

        let agent = result.unwrap();
        assert!(matches!(agent, BuiltAgent::Anthropic(_)));
    }

    #[tokio::test]
    async fn test_agent_factory_build_openai_compatible_bare() {
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let result = factory.build_bare(LlmRole::Review, "test preamble").await;
        assert!(result.is_ok());

        let agent = result.unwrap();
        assert!(matches!(agent, BuiltAgent::OpenAiCompatible(_)));
    }

    #[test]
    fn test_agent_factory_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AgentFactory>();
    }

    #[test]
    fn test_built_agent_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BuiltAgent>();
    }

    #[test]
    fn test_agent_factory_debug() {
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);
        let debug = format!("{factory:?}");
        assert!(debug.contains("AgentFactory"));
        assert!(debug.contains("BotConfig"));
    }

    // -- NoTools configurator test --

    #[test]
    fn test_no_tools_configurator() {
        // NoTools should pass through the builder unchanged.
        // We can't easily test this without a real client, but we can
        // verify it compiles and the trait is satisfied.
        fn assert_configurator<T: AgentConfigurator>(_t: T) {}
        assert_configurator(NoTools);
    }

    // -- apply_reasoning_effort tests --

    #[tokio::test]
    async fn test_apply_reasoning_effort_none_returns_builder_unchanged() {
        // When effort is None, additional_params should remain unset.
        // We verify indirectly by building a real Anthropic agent (no API call needed).
        let client: anthropic::Client = anthropic::Client::builder()
            .api_key("sk-test")
            .build()
            .unwrap();
        let builder = client.agent("test-model").preamble("test");
        // Should not panic and should return a valid builder
        let builder =
            apply_reasoning_effort(builder, None, "anthropic", "test-model", LlmRole::Dev);
        let _agent = builder.build();
    }

    #[tokio::test]
    async fn test_apply_reasoning_effort_some_sets_additional_params() {
        // When effort is Some, additional_params should be set with the reasoning object.
        let client: openai::Client = openai::Client::builder()
            .api_key("sk-test")
            .build()
            .unwrap();
        let builder = client.agent("gpt-4o").preamble("test");
        let builder =
            apply_reasoning_effort(builder, Some("high"), "openai", "gpt-4o", LlmRole::Dev);
        // Build succeeds — the additional_params are injected internally
        let _agent = builder.build();
    }

    #[tokio::test]
    async fn test_apply_reasoning_effort_all_valid_levels() {
        for level in &["low", "medium", "high", "xhigh"] {
            let client: openai::Client = openai::Client::builder()
                .api_key("sk-test")
                .build()
                .unwrap();
            let builder = client.agent("gpt-5.2-codex").preamble("test");
            let builder = apply_reasoning_effort(
                builder,
                Some(level),
                "openai",
                "gpt-5.2-codex",
                LlmRole::Dev,
            );
            let _agent = builder.build();
        }
    }

    #[tokio::test]
    async fn test_agent_factory_build_bare_with_reasoning_effort() {
        // Verify that reasoning_effort flows through build_bare for OpenAI provider.
        let mut config = make_test_config();
        config.llm.review.reasoning_effort = Some("high".to_string());
        let config = Arc::new(config);
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let agent = factory.build_bare(LlmRole::Review, "test preamble").await;
        assert!(agent.is_ok(), "Build with reasoning_effort should succeed");
    }

    #[tokio::test]
    async fn test_agent_factory_build_bare_anthropic_ignores_reasoning_effort() {
        // Anthropic provider should succeed even with reasoning_effort set (it's ignored with a warning).
        let mut config = make_test_config();
        config.llm.dev.reasoning_effort = Some("high".to_string());
        let config = Arc::new(config);
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let agent = factory.build_bare(LlmRole::Dev, "test preamble").await;
        assert!(
            agent.is_ok(),
            "Anthropic build should succeed even with reasoning_effort set"
        );
    }

    #[test]
    fn test_config_for_role_reasoning_effort_propagated() {
        let mut config = make_test_config();
        config.llm.dev.reasoning_effort = Some("xhigh".to_string());
        config.llm.review.reasoning_effort = Some("low".to_string());
        // supervisor has None
        let config = Arc::new(config);
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        assert_eq!(
            factory
                .config_for_role(LlmRole::Dev)
                .reasoning_effort
                .as_deref(),
            Some("xhigh")
        );
        assert_eq!(
            factory
                .config_for_role(LlmRole::Review)
                .reasoning_effort
                .as_deref(),
            Some("low")
        );
        assert!(
            factory
                .config_for_role(LlmRole::Supervisor)
                .reasoning_effort
                .is_none()
        );
    }

    // -- Story 9.2: ToolConfigurator MCP integration tests --

    #[tokio::test]
    async fn test_configure_agent_tools_macro_produces_empty_mcp_servers() {
        // The macro must initialize mcp_servers to an empty vec.
        let client: anthropic::Client = anthropic::Client::builder()
            .api_key("sk-test")
            .build()
            .unwrap();
        let builder = client.agent("test-model").preamble("test");

        let configurator = crate::configure_agent_tools!(rig::tools::think::ThinkTool);
        assert!(
            configurator.mcp_servers.is_empty(),
            "configure_agent_tools! must produce empty mcp_servers by default"
        );
        // Verify it still compiles and builds an agent
        let _agent = configurator.configure_anthropic(builder);
    }

    #[tokio::test]
    async fn test_with_mcp_sets_mcp_servers_field() {
        // with_mcp() should store the provided servers vec.
        // We can't easily construct real ServerSink instances without a running
        // MCP server, but we can verify the struct-level plumbing at the type level.
        let configurator = crate::configure_agent_tools!(rig::tools::think::ThinkTool);
        assert!(configurator.mcp_servers.is_empty());

        // Verify with_mcp returns Self and the field type is correct.
        // Using an empty vec is the simplest verification that the builder works.
        let configurator = configurator.with_mcp(vec![]);
        assert!(
            configurator.mcp_servers.is_empty(),
            "with_mcp(vec![]) should set an empty mcp_servers"
        );
    }

    #[tokio::test]
    async fn test_tool_configurator_with_empty_mcp_is_noop() {
        // When mcp_servers is empty, the for loop never executes — behavior
        // must be identical to pre-MCP code (zero .rmcp_tools() calls).
        let client: anthropic::Client = anthropic::Client::builder()
            .api_key("sk-test")
            .build()
            .unwrap();
        let builder = client.agent("test-model").preamble("test");

        let configurator =
            crate::configure_agent_tools!(rig::tools::think::ThinkTool).with_mcp(vec![]);
        // Should build successfully — identical to no-MCP path
        let _agent = configurator.configure_anthropic(builder);
    }
    // -- Story 11.2: base_url and openai-compatible tests --

    #[tokio::test]
    async fn test_agent_factory_build_openai_compatible_with_base_url() {
        let mut config = make_test_config();
        config.llm.review.base_url = Some("http://localhost:11434/v1".to_string());
        let config = Arc::new(config);
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let result = factory.build_bare(LlmRole::Review, "test preamble").await;
        assert!(result.is_ok(), "Build with base_url should succeed");
        assert!(matches!(result.unwrap(), BuiltAgent::OpenAiCompatible(_)));
    }

    #[tokio::test]
    async fn test_agent_factory_build_anthropic_with_base_url() {
        // Anthropic build path should also honour base_url (e.g. Anthropic-compatible proxy).
        let mut config = make_test_config();
        config.llm.dev.base_url = Some("http://localhost:8080/v1".to_string());
        let config = Arc::new(config);
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let result = factory.build_bare(LlmRole::Dev, "test preamble").await;
        assert!(
            result.is_ok(),
            "Anthropic build with base_url should succeed"
        );
        assert!(matches!(result.unwrap(), BuiltAgent::Anthropic(_)));
    }
}
