//! Centralized LLM provider abstraction — `AgentFactory` + `BuiltAgent` enum dispatch.
//!
//! Since rig's `Chat` trait is not object-safe (associated types, `Self: Sized`),
//! we cannot use `Box<dyn Chat>`. Instead, [`BuiltAgent`] wraps concrete agent types
//! in an enum and dispatches `stream_chat()` via match arms.
//!
//! [`AgentFactory`] centralizes all provider construction: API key resolution,
//! Copilot token exchange, and API format detection happen in one place. Callers
//! (session runner, review runner, supervisor architect) simply call
//! `factory.build(role, preamble, configure_tools)` and get a ready-to-use
//! `BuiltAgent` back.
//!
//! ## Copilot API Format Detection
//!
//! GitHub Copilot is a proxy that routes to multiple backends (OpenAI, Anthropic,
//! Mistral, etc.). Known OpenAI model families require the Responses API; all
//! other models use Chat Completions API as a safe fallback. See
//! [`copilot_requires_responses_api()`] for the hardcoded heuristic.

use crate::auth::github_copilot::{CopilotHttpClient, CopilotTokenCache, ReqwestCopilotHttpClient};
use crate::config::{BotConfig, BotSecrets, LlmRoleConfig};
use crate::session::dev_agent::streaming_chat;
use crate::session::provider::{ProviderError, copilot_headers};

use rig::agent::{Agent, AgentBuilder};
use rig::client::CompletionClient;
use rig::completion::Message;
use rig::providers::{anthropic, openai};
use std::path::PathBuf;
use std::sync::Arc;

/// Re-export [`ShutdownFlag`] for convenience.
pub use crate::session::dev_agent::ShutdownFlag;

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
}

impl std::fmt::Display for LlmRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dev => write!(f, "dev"),
            Self::Review => write!(f, "review"),
            Self::Supervisor => write!(f, "supervisor"),
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
/// - **OpenAiResponses** — Responses API (native OpenAI, or Copilot-proxied
///   OpenAI models like `gpt-4o`, `o3-pro`, `gpt-5.2-codex`).
/// - **OpenAiCompletions** — Chat Completions API (Copilot-proxied non-OpenAI
///   models like Claude, Mistral — safe fallback).
#[allow(missing_debug_implementations)]
pub enum BuiltAgent {
    /// Anthropic Messages API agent.
    Anthropic(Agent<anthropic::completion::CompletionModel>),
    /// OpenAI Responses API agent (also used for Copilot-proxied OpenAI models).
    OpenAiResponses(Agent<openai::responses_api::ResponsesCompletionModel>),
    /// OpenAI Chat Completions API agent (used for Copilot-proxied non-OpenAI models).
    OpenAiCompletions(Agent<openai::completion::CompletionModel>),
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
    ) -> Result<String, rig::completion::PromptError> {
        match self {
            Self::Anthropic(agent) => streaming_chat(agent, prompt, history, shutdown).await,
            Self::OpenAiResponses(agent) => streaming_chat(agent, prompt, history, shutdown).await,
            Self::OpenAiCompletions(agent) => {
                streaming_chat(agent, prompt, history, shutdown).await
            }
        }
    }

    /// Activate a BMAD agent by sending the agent file as the first user message.
    ///
    /// Thin wrapper around [`crate::session::dev_agent::activate_agent()`] that
    /// dispatches to the correct concrete agent type.
    ///
    /// # Arguments
    /// - `project_root` — path to the project root
    /// - `agent_relative_path` — relative path from project root to the agent file
    ///   (e.g. `"_bmad/bmm/agents/dev.md"` or `"_bmad/bmm/agents/architect.md"`)
    /// - `label` — logging label (e.g. `"dev-session"`, `"code-review"`, `"supervisor"`)
    /// - `shutdown` — optional cooperative shutdown flag
    pub async fn activate_agent(
        &self,
        project_root: &str,
        agent_relative_path: &str,
        label: &str,
        shutdown: Option<&ShutdownFlag>,
    ) -> Result<(Vec<Message>, Vec<crate::session::state::ChatMessage>), String> {
        match self {
            Self::Anthropic(agent) => {
                crate::session::dev_agent::activate_agent(
                    agent,
                    project_root,
                    agent_relative_path,
                    label,
                    shutdown,
                )
                .await
            }
            Self::OpenAiResponses(agent) => {
                crate::session::dev_agent::activate_agent(
                    agent,
                    project_root,
                    agent_relative_path,
                    label,
                    shutdown,
                )
                .await
            }
            Self::OpenAiCompletions(agent) => {
                crate::session::dev_agent::activate_agent(
                    agent,
                    project_root,
                    agent_relative_path,
                    label,
                    shutdown,
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
            Self::OpenAiResponses(_) => write!(f, "BuiltAgent::OpenAiResponses(..)"),
            Self::OpenAiCompletions(_) => write!(f, "BuiltAgent::OpenAiCompletions(..)"),
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

/// Centralized agent construction — builds [`BuiltAgent`] instances for any role.
///
/// Owns the shared configuration, secrets, and Copilot token cache. Callers
/// provide the role, preamble, and a closure to configure tools on the agent
/// builder.
///
/// # Example (conceptual)
///
/// ```ignore
/// let agent = factory.build(LlmRole::Dev, &preamble, |builder| {
///     builder.tool(git).tool(read_file).tool(edit_file)
/// }).await?;
///
/// agent.stream_chat("DS", vec![], Some(&shutdown)).await?;
/// ```
pub struct AgentFactory {
    /// Shared daemon configuration.
    config: Arc<BotConfig>,
    /// Shared secrets (API keys from `.env`).
    secrets: Arc<BotSecrets>,
    /// In-memory cache for GitHub Copilot session tokens.
    ///
    /// The `std::sync::Mutex` is used (not `tokio::sync::Mutex`) because the
    /// lock is held only briefly to check/store cached tokens — never across
    /// `.await` points.
    copilot_cache: std::sync::Mutex<CopilotTokenCache>,
}

impl std::fmt::Debug for AgentFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentFactory")
            .field("config", &"<BotConfig>")
            .field("secrets", &"<BotSecrets>")
            .field("copilot_cache", &"<CopilotTokenCache>")
            .finish()
    }
}

impl AgentFactory {
    /// Create a new `AgentFactory`.
    ///
    /// The `CopilotTokenCache` is created internally — callers do not need to
    /// manage it.
    pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>) -> Self {
        Self {
            config,
            secrets,
            copilot_cache: std::sync::Mutex::new(CopilotTokenCache::new()),
        }
    }

    /// Resolve the [`LlmRoleConfig`] for a given role.
    pub fn config_for_role(&self, role: LlmRole) -> &LlmRoleConfig {
        match role {
            LlmRole::Dev => &self.config.llm.dev,
            LlmRole::Review => &self.config.llm.review,
            LlmRole::Supervisor => &self.config.llm.supervisor,
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
                let client: anthropic::Client = anthropic::Client::builder()
                    .api_key(&api_key)
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
                    "AgentFactory built agent"
                );

                Ok(BuiltAgent::Anthropic(agent))
            }
            "openai" => {
                let client: openai::Client = openai::Client::builder()
                    .api_key(&api_key)
                    .build()
                    .map_err(|e| ProviderError::ClientCreation {
                        provider: "openai".to_string(),
                        reason: e.to_string(),
                    })?;

                let builder = client.agent(model).preamble(preamble);
                let builder =
                    apply_reasoning_effort(builder, reasoning_effort, "openai", model, role);
                let agent = configure_tools.configure_openai_responses(builder);

                tracing::info!(
                    action = "agent_built",
                    provider = "openai",
                    model = %model,
                    role = %role,
                    reasoning_effort = reasoning_effort.unwrap_or("none"),
                    "AgentFactory built agent (Responses API)"
                );

                Ok(BuiltAgent::OpenAiResponses(agent))
            }
            "github-copilot" => {
                let (session_token, base_url) = self.resolve_copilot_session(&api_key).await?;

                if copilot_requires_responses_api(model) {
                    // OpenAI model family → Responses API
                    let client: openai::Client = openai::Client::builder()
                        .api_key(&session_token)
                        .base_url(&base_url)
                        .http_headers(copilot_headers())
                        .build()
                        .map_err(|e| ProviderError::ClientCreation {
                            provider: "github-copilot".to_string(),
                            reason: e.to_string(),
                        })?;

                    let builder = client.agent(model).preamble(preamble);
                    let builder = apply_reasoning_effort(
                        builder,
                        reasoning_effort,
                        "github-copilot",
                        model,
                        role,
                    );
                    let agent = configure_tools.configure_openai_responses(builder);

                    tracing::info!(
                        action = "agent_built",
                        provider = "github-copilot",
                        api_format = "responses",
                        model = %model,
                        role = %role,
                        reasoning_effort = reasoning_effort.unwrap_or("none"),
                        "AgentFactory built Copilot agent (Responses API)"
                    );

                    Ok(BuiltAgent::OpenAiResponses(agent))
                } else {
                    // Non-OpenAI model → Completions API (safe fallback)
                    let client: openai::CompletionsClient = openai::Client::builder()
                        .api_key(&session_token)
                        .base_url(&base_url)
                        .http_headers(copilot_headers())
                        .build()
                        .map_err(|e| ProviderError::ClientCreation {
                            provider: "github-copilot".to_string(),
                            reason: e.to_string(),
                        })?
                        .completions_api();

                    let builder = client.agent(model).preamble(preamble);

                    if reasoning_effort.is_some() {
                        tracing::warn!(
                            provider = "github-copilot",
                            api_format = "completions",
                            model = %model,
                            role = %role,
                            "reasoning_effort is not supported for Completions API models — ignoring"
                        );
                    }

                    let agent = configure_tools.configure_openai_completions(builder);

                    tracing::info!(
                        action = "agent_built",
                        provider = "github-copilot",
                        api_format = "completions",
                        model = %model,
                        role = %role,
                        "AgentFactory built Copilot agent (Completions API)"
                    );

                    Ok(BuiltAgent::OpenAiCompletions(agent))
                }
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

    /// Resolve a Copilot session token and base URL from the OAuth token.
    ///
    /// Uses the internal [`CopilotTokenCache`] so that repeated calls within
    /// the same daemon run reuse a valid cached token. Returns
    /// `(session_token, base_url)` on success.
    ///
    /// The `std::sync::Mutex` guard is NOT held across the async exchange call
    /// to satisfy clippy's `await_holding_lock` lint.
    async fn resolve_copilot_session(
        &self,
        oauth_token: &str,
    ) -> Result<(String, String), ProviderError> {
        // Phase 1: check cache under lock, return immediately if valid
        {
            let cache = self
                .copilot_cache
                .lock()
                .map_err(|e| ProviderError::ClientCreation {
                    provider: "github-copilot".to_string(),
                    reason: format!("Copilot cache lock poisoned: {e}"),
                })?;
            if let Some(pair) = cache.try_get_cached() {
                return Ok(pair);
            }
        } // MutexGuard dropped here

        // Phase 2: exchange token WITHOUT holding the lock
        let http_client = ReqwestCopilotHttpClient::new();
        let resp = http_client
            .exchange_copilot_token(oauth_token)
            .await
            .map_err(|e| ProviderError::ClientCreation {
                provider: "github-copilot".to_string(),
                reason: format!("Copilot token exchange failed: {e}"),
            })?;

        let base_url = crate::auth::github_copilot::derive_base_url_from_token(&resp.token);
        let token = resp.token.clone();

        // Phase 3: store result under lock
        {
            let mut cache =
                self.copilot_cache
                    .lock()
                    .map_err(|e| ProviderError::ClientCreation {
                        provider: "github-copilot".to_string(),
                        reason: format!("Copilot cache lock poisoned: {e}"),
                    })?;
            cache.store(resp.token, base_url.clone(), resp.expires_at);
        }

        Ok((token, base_url))
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
/// same set of tools on all three builder types.
pub trait AgentConfigurator {
    /// Configure tools on an Anthropic agent builder and build the final agent.
    fn configure_anthropic(
        self,
        builder: AgentBuilder<anthropic::completion::CompletionModel>,
    ) -> Agent<anthropic::completion::CompletionModel>;

    /// Configure tools on an OpenAI Responses API agent builder and build the final agent.
    fn configure_openai_responses(
        self,
        builder: AgentBuilder<openai::responses_api::ResponsesCompletionModel>,
    ) -> Agent<openai::responses_api::ResponsesCompletionModel>;

    /// Configure tools on an OpenAI Completions API agent builder and build the final agent.
    fn configure_openai_completions(
        self,
        builder: AgentBuilder<openai::completion::CompletionModel>,
    ) -> Agent<openai::completion::CompletionModel>;
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

    fn configure_openai_responses(
        self,
        builder: AgentBuilder<openai::responses_api::ResponsesCompletionModel>,
    ) -> Agent<openai::responses_api::ResponsesCompletionModel> {
        builder.build()
    }

    fn configure_openai_completions(
        self,
        builder: AgentBuilder<openai::completion::CompletionModel>,
    ) -> Agent<openai::completion::CompletionModel> {
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
}

/// Implement [`AgentConfigurator`] for a 9-tool tuple (the standard tool set).
///
/// This covers the common case: 7 custom tools + AskSupervisor + ThinkTool.
impl<T1, T2, T3, T4, T5, T6, T7, T8, T9> AgentConfigurator
    for ToolConfigurator<(T1, T2, T3, T4, T5, T6, T7, T8, T9)>
where
    T1: rig::tool::Tool + Send + Sync + 'static,
    T2: rig::tool::Tool + Send + Sync + 'static,
    T3: rig::tool::Tool + Send + Sync + 'static,
    T4: rig::tool::Tool + Send + Sync + 'static,
    T5: rig::tool::Tool + Send + Sync + 'static,
    T6: rig::tool::Tool + Send + Sync + 'static,
    T7: rig::tool::Tool + Send + Sync + 'static,
    T8: rig::tool::Tool + Send + Sync + 'static,
    T9: rig::tool::Tool + Send + Sync + 'static,
{
    fn configure_anthropic(
        self,
        builder: AgentBuilder<anthropic::completion::CompletionModel>,
    ) -> Agent<anthropic::completion::CompletionModel> {
        let (t1, t2, t3, t4, t5, t6, t7, t8, t9) = self.tools;
        builder
            .tool(t1)
            .tool(t2)
            .tool(t3)
            .tool(t4)
            .tool(t5)
            .tool(t6)
            .tool(t7)
            .tool(t8)
            .tool(t9)
            .build()
    }

    fn configure_openai_responses(
        self,
        builder: AgentBuilder<openai::responses_api::ResponsesCompletionModel>,
    ) -> Agent<openai::responses_api::ResponsesCompletionModel> {
        let (t1, t2, t3, t4, t5, t6, t7, t8, t9) = self.tools;
        builder
            .tool(t1)
            .tool(t2)
            .tool(t3)
            .tool(t4)
            .tool(t5)
            .tool(t6)
            .tool(t7)
            .tool(t8)
            .tool(t9)
            .build()
    }

    fn configure_openai_completions(
        self,
        builder: AgentBuilder<openai::completion::CompletionModel>,
    ) -> Agent<openai::completion::CompletionModel> {
        let (t1, t2, t3, t4, t5, t6, t7, t8, t9) = self.tools;
        builder
            .tool(t1)
            .tool(t2)
            .tool(t3)
            .tool(t4)
            .tool(t5)
            .tool(t6)
            .tool(t7)
            .tool(t8)
            .tool(t9)
            .build()
    }
}

/// Implement [`AgentConfigurator`] for a 1-tool tuple (supervisor/architect use case).
impl<T1> AgentConfigurator for ToolConfigurator<(T1,)>
where
    T1: rig::tool::Tool + Send + Sync + 'static,
{
    fn configure_anthropic(
        self,
        builder: AgentBuilder<anthropic::completion::CompletionModel>,
    ) -> Agent<anthropic::completion::CompletionModel> {
        let (t1,) = self.tools;
        builder.tool(t1).build()
    }

    fn configure_openai_responses(
        self,
        builder: AgentBuilder<openai::responses_api::ResponsesCompletionModel>,
    ) -> Agent<openai::responses_api::ResponsesCompletionModel> {
        let (t1,) = self.tools;
        builder.tool(t1).build()
    }

    fn configure_openai_completions(
        self,
        builder: AgentBuilder<openai::completion::CompletionModel>,
    ) -> Agent<openai::completion::CompletionModel> {
        let (t1,) = self.tools;
        builder.tool(t1).build()
    }
}

// ---------------------------------------------------------------------------
// Copilot API format heuristic
// ---------------------------------------------------------------------------

/// Determine whether a model proxied via GitHub Copilot requires the OpenAI Responses API.
///
/// GitHub Copilot is a proxy that routes to multiple backends (OpenAI, Anthropic,
/// Mistral, etc.). OpenAI models require the Responses API — Chat Completions
/// returns 400 for newer models (e.g. `gpt-5.2-codex`).
///
/// All other models (Claude, Mistral, unknown) use Chat Completions through the
/// proxy — this is the safe fallback.
///
/// **This is hardcoded by design.** The API format is a deterministic property of
/// the provider behind the model, not a user preference. There is no `api_format`
/// config option.
///
/// # Known OpenAI model families
///
/// - `gpt-*` — GPT family (gpt-4o, gpt-4o-mini, gpt-5.2-codex, etc.)
/// - `o1-*` — O1 reasoning models (o1-mini, o1-preview, etc.)
/// - `o3-*` — O3 reasoning models (o3-pro, o3-mini, etc.)
/// - `*codex*` — Codex models (any model with "codex" in the name)
///
/// # Fallback
///
/// If the model name doesn't match any known OpenAI pattern, returns `false`
/// (Completions API). This is the safe default — it works for all non-OpenAI
/// models. The inverse (defaulting to Responses API) would break non-OpenAI models.
///
/// Adding a new OpenAI model family is a one-liner addition to this function.
/// Apply `reasoning.effort` as `additional_params` on an OpenAI Responses API builder.
///
/// If `effort` is `None`, the builder is returned unchanged. Otherwise, injects:
/// ```json
/// { "reasoning": { "effort": "<value>" } }
/// ```
///
/// Only call this for Responses API builders (OpenAI direct or Copilot Responses path).
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

pub fn copilot_requires_responses_api(model: &str) -> bool {
    let m = model.to_lowercase();
    m.starts_with("gpt-")
        || m.starts_with("o1-")
        || m.starts_with("o3-")
        || m.starts_with("o4-")
        || m.contains("codex")
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
mod tests {
    use super::*;

    // -- copilot_requires_responses_api tests --

    #[test]
    fn test_copilot_requires_responses_api_gpt_models() {
        assert!(copilot_requires_responses_api("gpt-4o"));
        assert!(copilot_requires_responses_api("gpt-4o-mini"));
        assert!(copilot_requires_responses_api("gpt-5.2-codex"));
        assert!(copilot_requires_responses_api("gpt-3.5-turbo"));
    }

    #[test]
    fn test_copilot_requires_responses_api_o1_models() {
        assert!(copilot_requires_responses_api("o1-mini"));
        assert!(copilot_requires_responses_api("o1-preview"));
    }

    #[test]
    fn test_copilot_requires_responses_api_o3_models() {
        assert!(copilot_requires_responses_api("o3-pro"));
        assert!(copilot_requires_responses_api("o3-mini"));
    }

    #[test]
    fn test_copilot_requires_responses_api_o4_models() {
        assert!(copilot_requires_responses_api("o4-mini"));
    }

    #[test]
    fn test_copilot_requires_responses_api_codex_models() {
        assert!(copilot_requires_responses_api("gpt-5.2-codex"));
        assert!(copilot_requires_responses_api("some-codex-model"));
        assert!(copilot_requires_responses_api("codex"));
    }

    #[test]
    fn test_copilot_requires_responses_api_case_insensitive() {
        assert!(copilot_requires_responses_api("GPT-4o"));
        assert!(copilot_requires_responses_api("O1-Mini"));
        assert!(copilot_requires_responses_api("O3-Pro"));
        assert!(copilot_requires_responses_api("CODEX"));
    }

    #[test]
    fn test_copilot_requires_responses_api_non_openai_models() {
        assert!(!copilot_requires_responses_api("claude-sonnet-4-20250514"));
        assert!(!copilot_requires_responses_api("claude-3.5-sonnet"));
        assert!(!copilot_requires_responses_api("mistral-large"));
        assert!(!copilot_requires_responses_api("unknown-model"));
        assert!(!copilot_requires_responses_api(""));
    }

    // -- LlmRole tests --

    #[test]
    fn test_llm_role_display() {
        assert_eq!(LlmRole::Dev.to_string(), "dev");
        assert_eq!(LlmRole::Review.to_string(), "review");
        assert_eq!(LlmRole::Supervisor.to_string(), "supervisor");
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
    }

    #[test]
    fn test_llm_role_eq() {
        assert_eq!(LlmRole::Dev, LlmRole::Dev);
        assert_ne!(LlmRole::Dev, LlmRole::Review);
        assert_ne!(LlmRole::Review, LlmRole::Supervisor);
    }

    // -- AgentFactory tests --

    fn make_test_config() -> BotConfig {
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
                },
                review: LlmRoleConfig {
                    provider: "openai".to_string(),
                    model: "gpt-4o".to_string(),
                    reasoning_effort: None,
                },
                supervisor: LlmRoleConfig {
                    provider: "github-copilot".to_string(),
                    model: "claude-sonnet-4-20250514".to_string(),
                    reasoning_effort: None,
                },
            },
            notifications: NotificationConfig {
                telegram: TelegramConfig {
                    enabled: false,
                    chat_id: String::new(),
                },
            },
            code_review_enabled: true,
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
            mcp_servers: vec![],
        }
    }

    fn make_test_secrets() -> BotSecrets {
        BotSecrets {
            anthropic_api_key: Some("sk-ant-test-key".to_string()),
            openai_api_key: Some("sk-openai-test-key".to_string()),
            github_copilot_oauth_token: Some("gh-copilot-test-key".to_string()),
            github_token: Some("ghp_test".to_string()),
            gitlab_token: None,
            telegram_bot_token: None,
        }
    }

    fn make_empty_secrets() -> BotSecrets {
        BotSecrets {
            anthropic_api_key: None,
            openai_api_key: None,
            github_copilot_oauth_token: None,
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
        assert_eq!(role_config.provider, "github-copilot");
        assert_eq!(role_config.model, "claude-sonnet-4-20250514");
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
    async fn test_agent_factory_build_openai_bare() {
        let config = Arc::new(make_test_config());
        let secrets = Arc::new(make_test_secrets());
        let factory = AgentFactory::new(config, secrets);

        let result = factory.build_bare(LlmRole::Review, "test preamble").await;
        assert!(result.is_ok());

        let agent = result.unwrap();
        assert!(matches!(agent, BuiltAgent::OpenAiResponses(_)));
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
                "github-copilot",
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
}
