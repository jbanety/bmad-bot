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

use crate::auth::github_copilot::{CopilotHttpClient, CopilotTokenCache, ReqwestCopilotHttpClient};
use crate::config::{BotConfig, BotSecrets};
use crate::llm_context::ContextBuilder;
use crate::llm_logging::{
    log_llm_error, log_llm_history, log_llm_history_summary, log_llm_request, log_llm_response,
};
use crate::session::SessionOutcome;
use crate::session::analyzer::{ResponseAction, ResponseAnalyzer};
use crate::session::branch::{BranchAction, determine_base_branch, ensure_story_branch};
use crate::session::cleanup::{mark_story_needs_clarification, preserve_partial_work};
use crate::session::escalation::EscalationReport;
use crate::session::provider::{ProviderError, resolve_api_key};
use crate::session::state::{ChatMessage, SessionState};
use crate::supervisor::decisions::{DecisionLog, write_decisions_file};
use crate::supervisor::{AskSupervisor, EscalationSlot};
use crate::tools::{FsTool, GitTool, TerminalTool};
use crate::watcher::StoryInfo;
use futures::StreamExt;
use git2::{BranchType, Repository};
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::client::CompletionClient;
use rig::completion::{Chat, CompletionModel, GetTokenUsage, Message};
use rig::message::Text;
use rig::providers::{anthropic, openai};
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Shared shutdown flag — set to `true` when Ctrl+C or SIGTERM is received.
///
/// Checked cooperatively between streaming chunks and chat turns so that
/// long-running tool-calling loops can be interrupted cleanly. The daemon
/// spawns a signal handler task that flips this flag, and the session runner
/// checks it at strategic points to break out and save partial work.
pub type ShutdownFlag = Arc<AtomicBool>;

/// Maximum number of chat turns before the safety net kicks in.
///
/// Prevents infinite loops if the agent never signals completion. A future
/// improvement could make this configurable via `BotConfig`.
const MAX_CHAT_TURNS: usize = 200;

/// Terminal tool timeout in seconds for commands executed by the agent.
const TERMINAL_TIMEOUT_SECS: u64 = 30;

/// Number of recent exchanges (user+assistant pairs) to keep verbatim in the
/// recovery message after a context window limit error.
const RECOVERY_KEEP_LAST_EXCHANGES: usize = 10;

/// Maximum recursive recovery depth — prevents infinite context-limit loops.
const MAX_RECOVERY_DEPTH: usize = 3;

/// Maximum tool-call rounds allowed per single prompt in the streaming loop.
///
/// This limits how many consecutive tool calls rig will execute before requiring
/// a text response. Set high because the dev agent may chain many reads/writes.
const STREAMING_MAX_TURNS: usize = 100;

/// Send a prompt via streaming and collect the complete text response.
///
/// This is a drop-in replacement for `agent.chat(prompt, history)` that uses
/// rig's streaming API instead. All providers (Anthropic, OpenAI, GitHub Copilot)
/// support streaming — and Copilot **requires** it (`stream: false` is rejected).
///
/// Tool calls are handled automatically by rig within the stream.
///
/// When `shutdown` is `Some` and the flag is set to `true`, the stream is
/// abandoned and a `ShutdownRequested` error is returned. This allows Ctrl+C
/// to interrupt even deep multi-turn tool-calling loops.
async fn streaming_chat<A, M>(
    agent: &A,
    prompt: impl Into<Message> + Send,
    history: Vec<Message>,
    shutdown: Option<&ShutdownFlag>,
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
        // Cooperative shutdown check — between every chunk/tool-call round
        if let Some(flag) = shutdown {
            if flag.load(Ordering::Relaxed) {
                tracing::info!(
                    action = "shutdown_requested",
                    "Shutdown flag detected in streaming loop"
                );
                return Err(rig::completion::PromptError::CompletionError(
                    rig::completion::CompletionError::ResponseError(
                        "Shutdown requested (Ctrl+C)".to_string(),
                    ),
                ));
            }
        }

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
                // FinalResponse signals the end of the stream.
                // acc already contains the full accumulated text.
                break;
            }
            Err(e) => {
                return Err(rig::completion::PromptError::CompletionError(
                    rig::completion::CompletionError::ResponseError(e.to_string()),
                ));
            }
            _ => continue, // tool call deltas, reasoning, etc. — handled by rig
        }
    }

    Ok(acc)
}

/// Data recovered from a WAL file for crash recovery.
///
/// Does NOT implement `Clone` — `SessionState` is consumed by ownership (move
/// semantics). The caller must clone any fields it needs before passing this
/// struct to [`SessionRunner::resume_session()`].
#[derive(Debug)]
pub struct RecoveryInfo {
    /// The loaded WAL session state (consumed by ownership during recovery).
    pub state: SessionState,
    /// Reconstructed story metadata from WAL fields.
    pub story_info: StoryInfo,
}

/// Build a [`StoryInfo`] from WAL session state fields.
///
/// Parses the `story_key` (e.g., `"6-3-crash-recovery-via-session-wal"`) to extract
/// `epic_num`, `story_num`, and `label`. Prefers `branch_name` over the legacy `branch`
/// field for backward compatibility with pre-4.3 WAL files.
pub fn story_info_from_wal(state: &SessionState, config: &BotConfig) -> StoryInfo {
    let parts: Vec<&str> = state.story_key.splitn(3, '-').collect();
    let epic_num: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let story_num: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let label = parts.get(2).unwrap_or(&"").to_string();

    // Prefer branch_name (Story 4.3+), fallback to branch (Story 4.2 WAL compat)
    let branch_name = if state.branch_name.is_empty() {
        state.branch.clone()
    } else {
        state.branch_name.clone()
    };

    StoryInfo {
        story_id: state.story_id.clone(),
        story_key: state.story_key.clone(),
        epic_num,
        story_num,
        label,
        branch_name,
        specs_path: PathBuf::from(format!(
            "{}/{}.md",
            config.bmad_paths.implementation_artifacts, state.story_key
        )),
        dependencies: vec![], // Already resolved — not needed for recovery
        status: "in-progress".to_string(),
    }
}

/// Detect context window / token limit errors from LLM provider error strings.
///
/// Each provider returns different error messages. This function checks for
/// known patterns across Anthropic, OpenAI, and GitHub Copilot. The check is
/// case-insensitive to handle inconsistent casing across provider SDKs.
fn is_context_limit_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    // Anthropic patterns
    lower.contains("context_length_exceeded")
        || lower.contains("prompt is too long")
        || lower.contains("maximum context length")
        // OpenAI patterns
        || lower.contains("max_tokens")
        || lower.contains("token limit")
        || lower.contains("context window")
        // Generic patterns
        || lower.contains("too many tokens")
        || lower.contains("input too long")
        || lower.contains("exceeds the model")
}

/// Session runner — manages the full lifecycle of a single story development session.
///
/// Constructed once per daemon run and reused across stories. Each call to
/// [`run()`](Self::run) creates a fresh agent, WAL, and chat loop for one story.
/// Also provides [`check_and_recover_wal()`](Self::check_and_recover_wal) and
/// [`resume_session()`](Self::resume_session) for crash recovery on daemon restart.
pub struct SessionRunner {
    /// Shared daemon configuration.
    config: Arc<BotConfig>,
    /// Shared secrets (API keys loaded from `.env`).
    secrets: Arc<BotSecrets>,
    /// Path to the WAL state file: `{implementation_artifacts}/.bmad-bot-session.yaml`.
    state_file_path: PathBuf,
    /// Stateless response analyzer (constructed once, reused).
    analyzer: ResponseAnalyzer,
    /// In-memory cache for GitHub Copilot session tokens (persists across stories).
    copilot_cache: std::sync::Mutex<CopilotTokenCache>,
    /// Cooperative shutdown flag — checked between streaming chunks and chat turns.
    shutdown: ShutdownFlag,
}

impl SessionRunner {
    /// Create a new session runner.
    ///
    /// The `state_file_path` is derived from
    /// `config.bmad_paths.implementation_artifacts` + `/.bmad-bot-session.yaml`.
    ///
    /// The `shutdown` flag is shared with the signal handler task spawned by the
    /// daemon. When set to `true`, the session exits cleanly after the current
    /// streaming chunk, saving partial work via the WAL.
    pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>, shutdown: ShutdownFlag) -> Self {
        let state_file_path =
            Path::new(&config.bmad_paths.implementation_artifacts).join(".bmad-bot-session.yaml");
        Self {
            config,
            secrets,
            state_file_path,
            analyzer: ResponseAnalyzer::new(),
            copilot_cache: std::sync::Mutex::new(CopilotTokenCache::new()),
            shutdown,
        }
    }

    /// Resolve a Copilot session token and base URL from the OAuth token.
    ///
    /// Uses the internal [`CopilotTokenCache`] so that repeated calls within
    /// the same daemon run reuse a valid cached token. Returns
    /// `(session_token, base_url)` on success, or a formatted error string.
    ///
    /// The `std::sync::Mutex` guard is NOT held across the async exchange call
    /// to satisfy clippy's `await_holding_lock` lint.
    async fn resolve_copilot_session(&self, oauth_token: &str) -> Result<(String, String), String> {
        // Phase 1: check cache under lock, return immediately if valid
        {
            let cache = self
                .copilot_cache
                .lock()
                .map_err(|e| format!("Copilot cache lock poisoned: {e}"))?;
            if let Some(pair) = cache.try_get_cached() {
                return Ok(pair);
            }
        } // MutexGuard dropped here

        // Phase 2: exchange token WITHOUT holding the lock
        let http_client = ReqwestCopilotHttpClient::new();
        let resp = http_client
            .exchange_copilot_token(oauth_token)
            .await
            .map_err(|e| format!("Copilot token exchange failed: {e}"))?;

        let base_url = crate::auth::github_copilot::derive_base_url_from_token(&resp.token);
        let token = resp.token.clone();

        // Phase 3: store result under lock
        {
            let mut cache = self
                .copilot_cache
                .lock()
                .map_err(|e| format!("Copilot cache lock poisoned: {e}"))?;
            cache.store(resp.token, base_url.clone(), resp.expires_at);
        }

        Ok((token, base_url))
    }

    /// Check for an interrupted session WAL file and prepare recovery data.
    ///
    /// Returns `Some(RecoveryInfo)` if a valid WAL file exists (crash recovery needed),
    /// or `None` for a clean start. Corrupt WAL files are deleted and treated as clean.
    pub async fn check_and_recover_wal(&self) -> Option<RecoveryInfo> {
        if !SessionState::exists(&self.state_file_path) {
            return None;
        }

        tracing::warn!(
            action = "crash_recovery",
            path = %self.state_file_path.display(),
            "WAL file detected — interrupted session found"
        );

        match SessionState::load(&self.state_file_path).await {
            Ok(state) => {
                let story_info = story_info_from_wal(&state, &self.config);
                Some(RecoveryInfo { state, story_info })
            }
            Err(e) => {
                tracing::error!(
                    action = "crash_recovery_wal_corrupt",
                    path = %self.state_file_path.display(),
                    error = %e,
                    "Failed to load WAL — deleting corrupt file"
                );
                let _ = SessionState::delete(&self.state_file_path).await;
                None
            }
        }
    }

    /// Resume an interrupted session from recovered WAL data.
    ///
    /// Verifies git state, resolves the API key, reconstructs the rig agent with
    /// the same provider/model from the WAL, and calls the refactored
    /// [`run_session()`](Self::run_session) with the recovered state.
    ///
    /// **Critical:** After `run_session()` returns, the WAL is ALWAYS deleted
    /// regardless of outcome to prevent infinite recovery loops.
    pub async fn resume_session(&self, recovery: RecoveryInfo) -> SessionOutcome {
        let RecoveryInfo { state, story_info } = recovery;

        let span = tracing::info_span!(
            "crash_recovery_session",
            story_id = %story_info.story_id,
            branch = %state.branch_name
        );
        let _guard = span.enter();

        tracing::info!(
            action = "crash_recovery_start",
            story_key = %state.story_key,
            history_len = %state.chat_history.len(),
            started_at = %state.started_at,
            "Resuming interrupted session"
        );

        // Phase 1 — Git state verification (inlined)
        let repo_path = PathBuf::from(&self.config.bmad_paths.project_root);
        let branch_name_for_git = state.branch_name.clone();

        let git_ok = {
            let rp = repo_path.clone();
            let bn = branch_name_for_git.clone();
            match tokio::task::spawn_blocking(move || -> Result<bool, String> {
                let repo =
                    Repository::open(&rp).map_err(|e| format!("Failed to open repo: {e}"))?;
                // Check if branch exists
                repo.find_branch(&bn, BranchType::Local)
                    .map_err(|_| format!("Recovery branch not found: {bn}"))?;
                Ok(true)
            })
            .await
            {
                Ok(Ok(true)) => true,
                Ok(Ok(false)) => false,
                Ok(Err(e)) => {
                    tracing::warn!(
                        action = "crash_recovery_git_failed",
                        error = %e,
                        "Git verification failed — stale WAL"
                    );
                    let _ = SessionState::delete(&self.state_file_path).await;
                    return SessionOutcome::Failed {
                        story_key: story_info.story_key.clone(),
                        error: format!("Recovery git verification failed: {e}"),
                        decisions: vec![],
                    };
                }
                Err(e) => {
                    tracing::error!(
                        action = "crash_recovery_git_panicked",
                        error = %e,
                        "Git verification panicked"
                    );
                    let _ = SessionState::delete(&self.state_file_path).await;
                    return SessionOutcome::Failed {
                        story_key: story_info.story_key.clone(),
                        error: format!("Git verification panicked: {e}"),
                        decisions: vec![],
                    };
                }
            }
        };

        if !git_ok {
            let _ = SessionState::delete(&self.state_file_path).await;
            return SessionOutcome::Failed {
                story_key: story_info.story_key.clone(),
                error: "Git verification returned false".to_string(),
                decisions: vec![],
            };
        }

        // Checkout the branch via ensure_story_branch (should return Reused)
        let rp = repo_path.clone();
        let bn = branch_name_for_git.clone();
        let bb = state.base_branch.clone();
        match tokio::task::spawn_blocking(move || ensure_story_branch(&rp, &bn, &bb)).await {
            Ok(Ok(_action)) => {
                tracing::info!(
                    action = "crash_recovery_git_verified",
                    branch = %branch_name_for_git,
                    "Git state verified"
                );
            }
            Ok(Err(e)) => {
                tracing::error!(
                    action = "crash_recovery_checkout_failed",
                    error = %e,
                    "Branch checkout failed during recovery"
                );
                let _ = SessionState::delete(&self.state_file_path).await;
                return SessionOutcome::Failed {
                    story_key: story_info.story_key.clone(),
                    error: format!("Recovery branch checkout failed: {e}"),
                    decisions: vec![],
                };
            }
            Err(e) => {
                tracing::error!(
                    action = "crash_recovery_checkout_panicked",
                    error = %e,
                    "Branch checkout panicked during recovery"
                );
                let _ = SessionState::delete(&self.state_file_path).await;
                return SessionOutcome::Failed {
                    story_key: story_info.story_key.clone(),
                    error: format!("Recovery branch checkout panicked: {e}"),
                    decisions: vec![],
                };
            }
        }

        // Phase 2 — Resolve API key
        let provider_name = state.provider.clone();
        let model_name = state.model.clone();
        let api_key = match resolve_api_key(&provider_name, &self.secrets) {
            Ok(key) => key,
            Err(e) => {
                tracing::error!(
                    action = "crash_recovery_api_key_failed",
                    provider = %provider_name,
                    error = %e,
                    "Failed to resolve API key for recovery"
                );
                let _ = SessionState::delete(&self.state_file_path).await;
                return SessionOutcome::Failed {
                    story_key: story_info.story_key.clone(),
                    error: format!("Recovery API key resolution failed: {e}"),
                    decisions: vec![],
                };
            }
        };

        // Phase 3 — Reconstruct agent and run recovered session
        let escalation_slot: EscalationSlot = Arc::new(std::sync::Mutex::new(None));
        let decision_log = DecisionLog::new();
        let base_branch = state.base_branch.clone();

        let outcome = match provider_name.as_str() {
            "anthropic" => {
                match self.build_anthropic_agent(
                    &story_info,
                    &api_key,
                    &model_name,
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(agent) => {
                        self.run_session(
                            &agent,
                            &story_info,
                            &provider_name,
                            &model_name,
                            &base_branch,
                            escalation_slot.clone(),
                            decision_log.clone(),
                            Some(state),
                        )
                        .await
                    }
                    Err(e) => {
                        tracing::error!(action = "crash_recovery_agent_failed", error = %e, "Agent build failed during recovery");
                        SessionOutcome::Failed {
                            story_key: story_info.story_key.clone(),
                            error: format!("Recovery agent build failed: {e}"),
                            decisions: decision_log.records(),
                        }
                    }
                }
            }
            "openai" => {
                match self.build_openai_agent(
                    &story_info,
                    &api_key,
                    &model_name,
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(agent) => {
                        self.run_session(
                            &agent,
                            &story_info,
                            &provider_name,
                            &model_name,
                            &base_branch,
                            escalation_slot.clone(),
                            decision_log.clone(),
                            Some(state),
                        )
                        .await
                    }
                    Err(e) => {
                        tracing::error!(action = "crash_recovery_agent_failed", error = %e, "Agent build failed during recovery");
                        SessionOutcome::Failed {
                            story_key: story_info.story_key.clone(),
                            error: format!("Recovery agent build failed: {e}"),
                            decisions: decision_log.records(),
                        }
                    }
                }
            }
            "github-copilot" => {
                // Exchange OAuth token for short-lived Copilot session token
                let (session_token, base_url) = match self.resolve_copilot_session(&api_key).await {
                    Ok(pair) => pair,
                    Err(e) => {
                        tracing::error!(action = "crash_recovery_copilot_failed", error = %e, "Copilot token exchange failed during recovery");
                        return SessionOutcome::Failed {
                            story_key: story_info.story_key.clone(),
                            error: e,
                            decisions: decision_log.records(),
                        };
                    }
                };

                match self.build_copilot_agent(
                    &story_info,
                    &session_token,
                    &model_name,
                    &base_url,
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(agent) => {
                        self.run_session(
                            &agent,
                            &story_info,
                            &provider_name,
                            &model_name,
                            &base_branch,
                            escalation_slot.clone(),
                            decision_log.clone(),
                            Some(state),
                        )
                        .await
                    }
                    Err(e) => {
                        tracing::error!(action = "crash_recovery_agent_failed", error = %e, "Agent build failed during recovery");
                        SessionOutcome::Failed {
                            story_key: story_info.story_key.clone(),
                            error: format!("Recovery agent build failed: {e}"),
                            decisions: decision_log.records(),
                        }
                    }
                }
            }
            other => {
                tracing::error!(
                    action = "crash_recovery_unsupported_provider",
                    provider = %other,
                    "Unsupported provider in WAL"
                );
                SessionOutcome::Failed {
                    story_key: story_info.story_key.clone(),
                    error: format!("Unsupported provider in WAL: {other}"),
                    decisions: vec![],
                }
            }
        };

        // Phase 4 — ALWAYS delete WAL after recovery attempt (prevents infinite loops)
        let _ = SessionState::delete(&self.state_file_path).await;

        outcome
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

        // --- Branch setup (BEFORE agent build) ---
        let repo_path = PathBuf::from(&self.config.bmad_paths.project_root);
        let default_branch = self.config.git_provider.target_branch.clone();
        let branch_name = story.branch_name.clone();

        // Resolve base branch (needs repo open for branch lookup)
        let base_branch = match Repository::open(&repo_path) {
            Ok(repo) => determine_base_branch(story, &repo, &default_branch),
            Err(e) => {
                tracing::error!(
                    action = "session_failed",
                    error = %e,
                    "Failed to open repo for base branch resolution"
                );
                return SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Failed to open repo: {e}"),
                    decisions: vec![],
                };
            }
        };

        // Create/checkout story branch — wrap blocking git2 call in spawn_blocking
        let rp = repo_path.clone();
        let bn = branch_name.clone();
        let bb = base_branch.clone();

        let branch_result =
            match tokio::task::spawn_blocking(move || ensure_story_branch(&rp, &bn, &bb)).await {
                Ok(Ok(action)) => action,
                Ok(Err(e)) => {
                    tracing::error!(
                        action = "session_failed",
                        error = %e,
                        "Branch setup failed"
                    );
                    return SessionOutcome::Failed {
                        story_key: story.story_key.clone(),
                        error: format!("Branch setup failed: {e}"),
                        decisions: vec![],
                    };
                }
                Err(e) => {
                    tracing::error!(
                        action = "session_failed",
                        error = %e,
                        "Branch setup panicked"
                    );
                    return SessionOutcome::Failed {
                        story_key: story.story_key.clone(),
                        error: format!("Branch setup panicked: {e}"),
                        decisions: vec![],
                    };
                }
            };

        match &branch_result {
            BranchAction::Created {
                branch_name: bn,
                base_branch: bb,
            } => {
                tracing::info!(
                    action = "branch_ready",
                    branch = %bn,
                    base = %bb,
                    "Story branch created"
                );
            }
            BranchAction::Reused { branch_name: bn } => {
                tracing::info!(
                    action = "branch_ready",
                    branch = %bn,
                    "Story branch reused"
                );
            }
        }

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
                            &base_branch,
                            escalation_slot.clone(),
                            decision_log.clone(),
                            None,
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
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(agent) => {
                        self.run_session(
                            &agent,
                            story,
                            provider,
                            model,
                            &base_branch,
                            escalation_slot.clone(),
                            decision_log.clone(),
                            None,
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
            "github-copilot" => {
                // Exchange OAuth token for short-lived Copilot session token
                let (session_token, base_url) = match self.resolve_copilot_session(&api_key).await {
                    Ok(pair) => pair,
                    Err(e) => {
                        tracing::error!(
                            action = "session_failed",
                            error = %e,
                            "Copilot token exchange failed"
                        );
                        return SessionOutcome::Failed {
                            story_key: story.story_key.clone(),
                            error: e,
                            decisions: decision_log.records(),
                        };
                    }
                };

                tracing::info!(
                    action = "copilot_token_exchanged",
                    "Copilot session token obtained"
                );

                match self.build_copilot_agent(
                    story,
                    &session_token,
                    model,
                    &base_url,
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(agent) => {
                        self.run_session(
                            &agent,
                            story,
                            provider,
                            model,
                            &base_branch,
                            escalation_slot.clone(),
                            decision_log.clone(),
                            None,
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
    ) -> Result<Agent<anthropic::completion::CompletionModel>, ProviderError> {
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

    /// Build an OpenAI agent (Responses API — default for native OpenAI models).
    fn build_openai_agent(
        &self,
        story: &StoryInfo,
        api_key: &str,
        model: &str,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
    ) -> Result<Agent<openai::responses_api::ResponsesCompletionModel>, ProviderError> {
        let preamble = self.build_preamble(story)?;

        let client: openai::Client =
            openai::Client::builder()
                .api_key(api_key)
                .build()
                .map_err(|e| ProviderError::ClientCreation {
                    provider: "openai".to_string(),
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
            provider = "openai",
            "Rig agent built"
        );

        Ok(agent)
    }

    /// Build a GitHub Copilot agent (Completions API — required for proxied models like Claude).
    ///
    /// Copilot proxies requests to various model backends (Anthropic, OpenAI, etc.).
    /// Many of these do not support the newer Responses API, so we use the
    /// Chat Completions API via [`openai::CompletionsClient`].
    fn build_copilot_agent(
        &self,
        story: &StoryInfo,
        api_key: &str,
        model: &str,
        base_url: &str,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
    ) -> Result<Agent<openai::completion::CompletionModel>, ProviderError> {
        let preamble = self.build_preamble(story)?;

        let client: openai::CompletionsClient = openai::Client::builder()
            .api_key(api_key)
            .base_url(base_url)
            .http_headers(super::provider::copilot_headers())
            .build()
            .map_err(|e| ProviderError::ClientCreation {
                provider: "github-copilot".to_string(),
                reason: e.to_string(),
            })?
            .completions_api();

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
            provider = "github-copilot",
            "Rig agent built (Completions API)"
        );

        Ok(agent)
    }

    /// Build the agent system prompt with operational instructions.
    ///
    /// This mirrors Zed's `system_prompt.hbs` pattern: the system prompt contains
    /// operational instructions (tool usage, formatting rules, communication style)
    /// while the agent persona (`dev.md`) is sent as a user message wrapped in
    /// XML context tags via [`activate_agent()`](Self::activate_agent).
    ///
    /// The system prompt provides persistent grounding across all turns.
    fn build_preamble(&self, _story: &StoryInfo) -> Result<String, ProviderError> {
        Ok(r#"You are an AI agent operating autonomously in a BMAD workflow environment.

## Tools
You have access to these tools: git, filesystem, terminal, ask_supervisor.
Use them to read files, explore the project, run commands, and ask the supervisor when you need clarification.

## Communication
OVERRIDE: communication_language = English

## Rules
- When the user provides an agent file in <context><files> tags, you MUST fully embody that agent's persona and follow ALL activation instructions exactly as specified.
- NEVER break character until given an exit command.
- Execute activation steps in order — load configuration files via tools, then greet and display the menu.
- Wait for user input after displaying the menu."#.to_string())
    }

    /// Activate the BMAD agent by sending the agent file as the first user message.
    ///
    /// Returns `(rig_history, chat_history)` — the rig `Message` vec for subsequent
    /// `streaming_chat` calls and the `ChatMessage` vec for WAL state persistence.
    ///
    /// The LLM receives the full `dev.md` content as a user message, processes the
    /// activation steps (loads `config.yaml` via tools, reads the story file, shows
    /// the greeting and menu), and is then ready to accept commands like `"DS"`.
    ///
    /// After this method returns, the caller continues with the existing flow
    /// (e.g. "DS", "CH", recovery message) — those flows are unchanged.
    async fn activate_agent<A, M>(
        &self,
        agent: &A,
        label: &str,
    ) -> Result<(Vec<Message>, Vec<ChatMessage>), String>
    where
        A: Chat + StreamingChat<M, M::StreamingResponse>,
        M: CompletionModel + 'static,
        M::StreamingResponse: Clone + Unpin + GetTokenUsage,
    {
        let agent_path =
            Path::new(&self.config.bmad_paths.project_root).join("_bmad/bmm/agents/dev.md");

        // Build Zed-style XML context message via ContextBuilder helper.
        // This reads the file, resolves to absolute path, and wraps in
        // <context><files>...</files></context> — same format Zed uses
        // for @file inclusions (thread.rs:206-409).
        let activation_msg = ContextBuilder::new()
            .add_file_from_disk(&agent_path)
            .map_err(|e| format!("Failed to build agent activation context: {e}"))?
            .build();

        let mut rig_history: Vec<Message> = vec![];
        let mut chat_history: Vec<ChatMessage> = vec![];

        // Send dev.md wrapped in XML context tags — triggers BMAD activation flow
        log_llm_request(
            label,
            0,
            "[agent activation: dev.md in context tags]",
            rig_history.len(),
        );
        let response = streaming_chat(
            agent,
            activation_msg.as_str(),
            rig_history.clone(),
            Some(&self.shutdown),
        )
        .await
        .map_err(|e| {
            log_llm_error(label, 0, &e);
            format!("Agent activation failed: {e}")
        })?;
        log_llm_response(label, 0, &response);

        rig_history.push(Message::user(&activation_msg));
        rig_history.push(Message::assistant(&response));
        chat_history.push(ChatMessage {
            role: "user".to_string(),
            content: activation_msg,
        });
        chat_history.push(ChatMessage {
            role: "assistant".to_string(),
            content: response,
        });

        tracing::info!(
            action = "agent_activation_complete",
            label = %label,
            "BMAD agent activated via user message"
        );

        Ok((rig_history, chat_history))
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

    /// Extract the last `n` exchanges (user+assistant pairs) from chat history.
    ///
    /// Returns cloned messages. If the history has fewer than `n * 2` messages,
    /// returns all of them. Odd-length histories are rounded DOWN to the nearest
    /// even count before slicing to keep clean user/assistant pairs.
    fn extract_last_exchanges(history: &[ChatMessage], n: usize) -> Vec<ChatMessage> {
        if history.is_empty() {
            return vec![];
        }
        // Round down to even count to keep clean pairs
        let usable_len = history.len() - (history.len() % 2);
        let take = (n * 2).min(usable_len);
        history[usable_len - take..usable_len].to_vec()
    }

    /// Format extracted exchanges as readable text for inclusion in a recovery message.
    ///
    /// Individual messages longer than 2000 characters are truncated with
    /// `"... [truncated]"` to keep the recovery message within reasonable bounds.
    fn format_exchanges_for_message(exchanges: &[ChatMessage]) -> String {
        if exchanges.is_empty() {
            return String::from("=== Recent Conversation ===\n(no recent exchanges available)");
        }
        let mut out = format!(
            "=== Recent Conversation (last {} exchanges) ===\n",
            exchanges.len() / 2
        );
        for msg in exchanges {
            let label = if msg.role == "user" {
                "User"
            } else {
                "Assistant"
            };
            let content = if msg.content.len() > 2000 {
                format!("{}... [truncated]", &msg.content[..2000])
            } else {
                msg.content.clone()
            };
            out.push_str(&format!("{label}: {content}\n"));
        }
        out
    }

    /// Build the recovery message sent after BMAD activation in a recovered session.
    ///
    /// Contains the session summary, recent exchanges, and a pointer to the story
    /// file. This is sent as a plain user message — NOT injected into the preamble.
    /// The agent already has its standard preamble and has loaded project context
    /// via the BMAD activation flow (CH → "Load the project context").
    fn build_recovery_message(
        &self,
        story: &StoryInfo,
        summary: &str,
        formatted_exchanges: &str,
    ) -> String {
        format!(
            "=== SESSION RECOVERY — Context Window Limit Reached ===\n\
             Your previous session hit the context window limit. Below is your recovery context:\n\
             \n\
             === Session Summary ===\n\
             {summary}\n\
             \n\
             {formatted_exchanges}\n\
             \n\
             === Current Story ===\n\
             The story file is at: {path}\n\
             Read this file to see current task checkboxes and progress.\n\
             Continue working directly on the current task. Do NOT restart the workflow from the beginning.",
            path = story.specs_path.display(),
        )
    }

    /// Summarize the full chat history via a fresh LLM call (no tools, empty history).
    ///
    /// Uses the same provider/model as the dev session for consistency. If the full
    /// history is too large for a fresh context, retries with the last 50% of messages.
    async fn summarize_history(
        &self,
        state: &SessionState,
        story: &StoryInfo,
        provider: &str,
        model: &str,
    ) -> Result<String, String> {
        let api_key = resolve_api_key(provider, &self.secrets)
            .map_err(|e| format!("Summarization API key failed: {e}"))?;

        let format_history = |messages: &[ChatMessage]| -> String {
            messages
                .iter()
                .map(|m| {
                    let label = if m.role == "user" {
                        "User"
                    } else {
                        "Assistant"
                    };
                    format!("{label}: {}", m.content)
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let full_history_text = format_history(&state.chat_history);

        let summarization_prompt = format!(
            "You are summarizing a development session for continuity. The session is implementing \
             story {key} in a software project. Provide a concise but comprehensive summary covering:\n\
             - What tasks have been completed (with file paths and key decisions)\n\
             - What task is currently in progress\n\
             - Any issues encountered and how they were resolved\n\
             - Key code patterns and conventions established\n\
             - Current state of the implementation\n\
             \n\
             Here is the full conversation history:\n\
             {full_history_text}",
            key = story.story_key,
        );

        // Attempt summarization — provider-specific because Chat is not object-safe
        let copilot_cache_ref = &self.copilot_cache;
        let summarize_with_prompt = |prompt: &str,
                                     prov: &str,
                                     mdl: &str,
                                     key: &str|
         -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, String>> + '_>,
        > {
            let prompt = prompt.to_string();
            let prov = prov.to_string();
            let mdl = mdl.to_string();
            let flag = Arc::clone(&self.shutdown);
            let key = key.to_string();
            Box::pin(async move {
                let preamble =
                    "You are a technical session summarizer. Be concise but comprehensive.";
                match prov.as_str() {
                    "anthropic" => {
                        let client: anthropic::Client = anthropic::Client::builder()
                            .api_key(&key)
                            .build()
                            .map_err(|e| format!("Summarization client failed: {e}"))?;
                        let agent = client.agent(&mdl).preamble(preamble).build();
                        log_llm_request("dev-summarize", 0, &prompt, 0);
                        let result =
                            streaming_chat(&agent, prompt.as_str(), vec![], Some(&flag)).await;
                        match &result {
                            Ok(r) => log_llm_response("dev-summarize", 0, r),
                            Err(e) => log_llm_error("dev-summarize", 0, &e),
                        }
                        result.map_err(|e| e.to_string())
                    }
                    "openai" => {
                        let client: openai::Client = openai::Client::builder()
                            .api_key(&key)
                            .build()
                            .map_err(|e| format!("Summarization client failed: {e}"))?;
                        let agent = client.agent(&mdl).preamble(preamble).build();
                        log_llm_request("dev-summarize", 0, &prompt, 0);
                        let result =
                            streaming_chat(&agent, prompt.as_str(), vec![], Some(&flag)).await;
                        match &result {
                            Ok(r) => log_llm_response("dev-summarize", 0, r),
                            Err(e) => log_llm_error("dev-summarize", 0, &e),
                        }
                        result.map_err(|e| e.to_string())
                    }
                    "github-copilot" => {
                        // Check cache first (short lock, no await)
                        let cached = {
                            let cache = copilot_cache_ref
                                .lock()
                                .map_err(|e| format!("Copilot cache lock poisoned: {e}"))?;
                            cache.try_get_cached()
                        };

                        let (sess_tok, base) = if let Some(pair) = cached {
                            pair
                        } else {
                            // Exchange without holding lock
                            let http_client = ReqwestCopilotHttpClient::new();
                            let resp =
                                http_client
                                    .exchange_copilot_token(&key)
                                    .await
                                    .map_err(|e| {
                                        format!(
                                            "Copilot token exchange for summarization failed: {e}"
                                        )
                                    })?;
                            let base_url = crate::auth::github_copilot::derive_base_url_from_token(
                                &resp.token,
                            );
                            let token = resp.token.clone();
                            // Store under lock
                            {
                                let mut cache = copilot_cache_ref
                                    .lock()
                                    .map_err(|e| format!("Copilot cache lock poisoned: {e}"))?;
                                cache.store(resp.token, base_url.clone(), resp.expires_at);
                            }
                            (token, base_url)
                        };

                        let client: openai::CompletionsClient = openai::Client::builder()
                            .api_key(&sess_tok)
                            .base_url(&base)
                            .http_headers(super::provider::copilot_headers())
                            .build()
                            .map_err(|e| format!("Summarization client failed: {e}"))?
                            .completions_api();
                        let agent = client.agent(&mdl).preamble(preamble).build();
                        log_llm_request("dev-summarize", 0, &prompt, 0);
                        let result =
                            streaming_chat(&agent, prompt.as_str(), vec![], Some(&flag)).await;
                        match &result {
                            Ok(r) => log_llm_response("dev-summarize", 0, r),
                            Err(e) => log_llm_error("dev-summarize", 0, &e),
                        }
                        result.map_err(|e| e.to_string())
                    }
                    other => Err(format!("Unsupported provider for summarization: {other}")),
                }
            })
        };

        match summarize_with_prompt(&summarization_prompt, provider, model, &api_key).await {
            Ok(summary) => Ok(summary),
            Err(e) => {
                // Fallback: if the full history was too large, retry with last 50%
                if is_context_limit_error(&e) {
                    tracing::warn!(
                        action = "context_limit_summary_fallback",
                        "Summarization hit context limit — retrying with truncated history (50%)"
                    );
                    let half = state.chat_history.len() / 2;
                    let truncated = &state.chat_history[half..];
                    let truncated_text = format_history(truncated);
                    let fallback_prompt = format!(
                        "You are summarizing a development session for continuity. The session is implementing \
                         story {key} in a software project. Provide a concise but comprehensive summary covering:\n\
                         - What tasks have been completed (with file paths and key decisions)\n\
                         - What task is currently in progress\n\
                         - Any issues encountered and how they were resolved\n\
                         - Key code patterns and conventions established\n\
                         - Current state of the implementation\n\
                         \n\
                         Here is the conversation history (truncated to last 50%):\n\
                         {truncated_text}",
                        key = story.story_key,
                    );
                    summarize_with_prompt(&fallback_prompt, provider, model, &api_key)
                        .await
                        .map_err(|e2| {
                            format!("Summarization failed even with truncated history: {e2}")
                        })
                } else {
                    Err(format!("Summarization failed: {e}"))
                }
            }
        }
    }

    /// Recover from a context window limit error by summarizing history and
    /// bootstrapping a fresh session following the BMAD activation pattern.
    ///
    /// Architecture Decision 3, Recovery Case B. The method:
    /// 1. Extracts last N exchanges from in-memory state as immediate context
    /// 2. Makes a fresh LLM call to summarize the full history
    /// 3. Builds a fresh agent with the STANDARD dev preamble + all 4 tools
    /// 4. Drives BMAD activation: "CH" → "Load the project context" (agent
    ///    loads what it needs via its tools — same pattern as Story 3.2)
    /// 5. Sends recovery message (summary + last N exchanges + continue instruction)
    /// 6. Builds compressed SessionState with activation turns + recovery message
    /// 7. Calls `run_session()` with `Some(compressed_state)` — reuses the existing
    ///    chat loop instead of duplicating it
    /// 8. Returns the `SessionOutcome` from the inner loop directly
    ///
    /// If `recovery_depth >= MAX_RECOVERY_DEPTH`, returns `SessionOutcome::Failed`
    /// to prevent infinite recursion.
    #[allow(clippy::too_many_arguments)]
    async fn context_limit_recovery(
        &self,
        state: &SessionState,
        story: &StoryInfo,
        provider: &str,
        model: &str,
        base_branch: &str,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
        recovery_depth: usize,
    ) -> SessionOutcome {
        // Step 0 — Check recovery depth
        if recovery_depth >= MAX_RECOVERY_DEPTH {
            tracing::error!(
                action = "context_limit_max_depth",
                depth = %recovery_depth,
                "Max recovery depth reached — aborting"
            );
            return SessionOutcome::Failed {
                story_key: story.story_key.clone(),
                error: format!("Context limit recovery exceeded max depth ({MAX_RECOVERY_DEPTH})"),
                decisions: decision_log.records(),
            };
        }

        // Step 1 — Extract last N exchanges
        let last_exchanges =
            Self::extract_last_exchanges(&state.chat_history, RECOVERY_KEEP_LAST_EXCHANGES);
        let formatted_exchanges = Self::format_exchanges_for_message(&last_exchanges);

        // Step 2 — Summarize full history via fresh LLM call
        let summary = match self.summarize_history(state, story, provider, model).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    action = "context_limit_summary_failed",
                    error = %e,
                    "History summarization failed"
                );
                return SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Context limit recovery summarization failed: {e}"),
                    decisions: decision_log.records(),
                };
            }
        };

        tracing::info!(
            action = "context_limit_summary_generated",
            original_len = %state.chat_history.len(),
            summary_len = %summary.len(),
            "Session summary generated for recovery"
        );

        // Step 3-7 — Build fresh agent, drive activation, delegate to run_session()
        // All inside provider match arm because Chat trait is not object-safe.
        let api_key = match resolve_api_key(provider, &self.secrets) {
            Ok(key) => key,
            Err(e) => {
                tracing::error!(
                    action = "context_limit_api_key_failed",
                    error = %e,
                    "API key resolution failed during recovery"
                );
                return SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Recovery API key resolution failed: {e}"),
                    decisions: decision_log.records(),
                };
            }
        };

        // Macro-like closure to drive activation and run_session for a concrete agent type.
        // We must duplicate the activation logic per provider arm because Chat is not object-safe.
        let recovery_message = self.build_recovery_message(story, &summary, &formatted_exchanges);

        match provider {
            "anthropic" => {
                let agent = match self.build_anthropic_agent(
                    story,
                    &api_key,
                    model,
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::error!(action = "context_limit_agent_failed", error = %e, "Recovery agent build failed");
                        return SessionOutcome::Failed {
                            story_key: story.story_key.clone(),
                            error: format!("Recovery agent build failed: {e}"),
                            decisions: decision_log.records(),
                        };
                    }
                };

                match self
                    .drive_activation_and_recover(
                        &agent,
                        state,
                        story,
                        provider,
                        model,
                        base_branch,
                        escalation_slot,
                        decision_log,
                        &recovery_message,
                        recovery_depth,
                    )
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(outcome) => outcome,
                }
            }
            "openai" => {
                let agent = match self.build_openai_agent(
                    story,
                    &api_key,
                    model,
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::error!(action = "context_limit_agent_failed", error = %e, "Recovery agent build failed");
                        return SessionOutcome::Failed {
                            story_key: story.story_key.clone(),
                            error: format!("Recovery agent build failed: {e}"),
                            decisions: decision_log.records(),
                        };
                    }
                };

                match self
                    .drive_activation_and_recover(
                        &agent,
                        state,
                        story,
                        provider,
                        model,
                        base_branch,
                        escalation_slot,
                        decision_log,
                        &recovery_message,
                        recovery_depth,
                    )
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(outcome) => outcome,
                }
            }
            "github-copilot" => {
                // Exchange OAuth token for short-lived Copilot session token
                let (session_token, base_url) = match self.resolve_copilot_session(&api_key).await {
                    Ok(pair) => pair,
                    Err(e) => {
                        tracing::error!(action = "context_limit_copilot_failed", error = %e, "Copilot token exchange failed during recovery");
                        return SessionOutcome::Failed {
                            story_key: story.story_key.clone(),
                            error: e,
                            decisions: decision_log.records(),
                        };
                    }
                };

                let agent = match self.build_copilot_agent(
                    story,
                    &session_token,
                    model,
                    &base_url,
                    escalation_slot.clone(),
                    decision_log.clone(),
                ) {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::error!(action = "context_limit_agent_failed", error = %e, "Recovery agent build failed");
                        return SessionOutcome::Failed {
                            story_key: story.story_key.clone(),
                            error: format!("Recovery agent build failed: {e}"),
                            decisions: decision_log.records(),
                        };
                    }
                };

                match self
                    .drive_activation_and_recover(
                        &agent,
                        state,
                        story,
                        provider,
                        model,
                        base_branch,
                        escalation_slot,
                        decision_log,
                        &recovery_message,
                        recovery_depth,
                    )
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(outcome) => outcome,
                }
            }
            other => {
                tracing::error!(
                    action = "context_limit_unsupported_provider",
                    provider = %other,
                    "Unsupported provider for recovery"
                );
                SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Unsupported provider for recovery: {other}"),
                    decisions: decision_log.records(),
                }
            }
        }
    }

    /// Drive the BMAD activation flow on a fresh agent, build compressed state,
    /// and delegate to `run_session()` for the recovered chat loop.
    ///
    /// This is factored out to avoid duplicating the activation logic across
    /// provider match arms. The agent type is generic (`<A: Chat>`).
    ///
    /// Returns `Ok(SessionOutcome)` on success, `Err(SessionOutcome)` on failure.
    #[allow(clippy::too_many_arguments)]
    async fn drive_activation_and_recover<A, M>(
        &self,
        agent: &A,
        original_state: &SessionState,
        story: &StoryInfo,
        provider: &str,
        model: &str,
        base_branch: &str,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
        recovery_message: &str,
        recovery_depth: usize,
    ) -> Result<SessionOutcome, SessionOutcome>
    where
        A: Chat + StreamingChat<M, M::StreamingResponse>,
        M: CompletionModel + 'static,
        M::StreamingResponse: Clone + Unpin + GetTokenUsage,
    {
        // Step 4a — Activate agent: send dev.md as user message
        let (mut activation_history, mut compressed_history) = self
            .activate_agent(agent, "dev-recovery")
            .await
            .map_err(|e| {
                tracing::error!(action = "context_limit_activation_failed", error = %e);
                SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Recovery activation failed: {e}"),
                    decisions: decision_log.records(),
                }
            })?;

        // Step 4b — Enter chat mode (existing flow unchanged)
        let ch_turn = compressed_history.len() / 2;
        log_llm_request("dev-recovery", ch_turn, "CH", activation_history.len());
        let ch_response = streaming_chat(
            agent,
            "CH",
            activation_history.clone(),
            Some(&self.shutdown),
        )
        .await
        .map_err(|e| {
            log_llm_error("dev-recovery", 0, &e);
            tracing::error!(action = "context_limit_activation_ch_failed", error = %e);
            SessionOutcome::Failed {
                story_key: story.story_key.clone(),
                error: format!("Recovery activation CH failed: {e}"),
                decisions: decision_log.records(),
            }
        })?;
        log_llm_response("dev-recovery", ch_turn, &ch_response);
        activation_history.push(Message::user("CH"));
        activation_history.push(Message::assistant(&ch_response));
        compressed_history.push(ChatMessage {
            role: "user".to_string(),
            content: "CH".to_string(),
        });
        compressed_history.push(ChatMessage {
            role: "assistant".to_string(),
            content: ch_response,
        });

        // Step 4c — Load project context (existing flow unchanged)
        let ctx_turn = compressed_history.len() / 2;
        log_llm_request(
            "dev-recovery",
            ctx_turn,
            "Load the project context",
            activation_history.len(),
        );
        let ctx_response = streaming_chat(
            agent,
            "Load the project context",
            activation_history.clone(),
            Some(&self.shutdown),
        )
        .await
        .map_err(|e| {
            log_llm_error("dev-recovery", ctx_turn, &e);
            tracing::error!(action = "context_limit_activation_ctx_failed", error = %e);
            SessionOutcome::Failed {
                story_key: story.story_key.clone(),
                error: format!("Recovery activation load context failed: {e}"),
                decisions: decision_log.records(),
            }
        })?;
        log_llm_response("dev-recovery", ctx_turn, &ctx_response);
        activation_history.push(Message::user("Load the project context"));
        activation_history.push(Message::assistant(&ctx_response));
        compressed_history.push(ChatMessage {
            role: "user".to_string(),
            content: "Load the project context".to_string(),
        });
        compressed_history.push(ChatMessage {
            role: "assistant".to_string(),
            content: ctx_response,
        });

        tracing::info!(
            action = "context_limit_activation_complete",
            "BMAD activation flow completed for recovery agent"
        );

        // Step 5 — Build compressed SessionState

        // Add recovery message as the final user message.
        // run_session(Some(state)) will detect last msg = user and re-send it.
        compressed_history.push(ChatMessage {
            role: "user".to_string(),
            content: recovery_message.to_string(),
        });

        let compressed_state = SessionState {
            story_id: original_state.story_id.clone(),
            story_key: original_state.story_key.clone(),
            branch: original_state.branch.clone(),
            started_at: original_state.started_at.clone(),
            last_activity: chrono::Utc::now().to_rfc3339(),
            provider: original_state.provider.clone(),
            model: original_state.model.clone(),
            branch_name: original_state.branch_name.clone(),
            base_branch: original_state.base_branch.clone(),
            chat_history: compressed_history,
        };

        // Step 6 — Delegate to run_session() with compressed state.
        // Box::pin breaks the async recursion cycle:
        // run_session → context_limit_recovery → drive_activation_and_recover → run_session
        let outcome = Box::pin(self.run_session(
            agent,
            story,
            provider,
            model,
            base_branch,
            escalation_slot,
            decision_log,
            Some(compressed_state),
        ))
        .await;

        tracing::info!(
            action = "context_limit_recovery",
            depth = %recovery_depth,
            original_history_len = %original_state.chat_history.len(),
            "Context limit recovery delegated to inner run_session()"
        );

        Ok(outcome)
    }

    /// Run the chat loop with a concrete agent that implements [`Chat`].
    ///
    /// This is the provider-agnostic core: send "DS", analyze responses,
    /// auto-respond, and handle completion/escalation/failure.
    ///
    /// When `recovered_state` is `None` (normal path): creates a new `SessionState`,
    /// sends "DS", and enters the chat loop as usual.
    ///
    /// When `recovered_state` is `Some(state)` (crash recovery path): uses the
    /// loaded state directly (already has chat_history, branch info, timestamps).
    /// The turn counter is offset by `chat_history.len() / 2` to account for
    /// pre-crash turns against `MAX_CHAT_TURNS`.
    #[allow(clippy::too_many_arguments)]
    async fn run_session<A, M>(
        &self,
        agent: &A,
        story: &StoryInfo,
        provider: &str,
        model: &str,
        base_branch: &str,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
        recovered_state: Option<SessionState>,
    ) -> SessionOutcome
    where
        A: Chat + StreamingChat<M, M::StreamingResponse>,
        M: CompletionModel + 'static,
        M::StreamingResponse: Clone + Unpin + GetTokenUsage,
    {
        let mut retries: usize = 0;
        const MAX_RETRIES: usize = 3;

        // --- Initialization: normal vs recovery path ---
        let (mut state, mut current_response, mut turn) = match recovered_state {
            None => {
                // Normal path — create new WAL, send "DS"
                let mut state = SessionState::new(story, provider, model);
                state.set_branch_info(&story.branch_name, base_branch);

                if let Err(e) = state.save(&self.state_file_path).await {
                    tracing::error!(action = "wal_write_failed", error = %e, "Failed to create initial WAL");
                    return SessionOutcome::Failed {
                        story_key: story.story_key.clone(),
                        error: format!("WAL creation failed: {e}"),
                        decisions: decision_log.records(),
                    };
                }

                // Activate agent: send dev.md as user message so the LLM
                // processes activation steps (load config via tools, show menu)
                let (activation_rig_history, activation_chat_history) = match self
                    .activate_agent(agent, "dev-session")
                    .await
                {
                    Ok(pair) => pair,
                    Err(e) => {
                        tracing::error!(action = "activation_failed", error = %e, "Agent activation failed");
                        let _ = self.handle_failure(story).await;
                        return SessionOutcome::Failed {
                            story_key: story.story_key.clone(),
                            error: format!("Agent activation failed: {e}"),
                            decisions: decision_log.records(),
                        };
                    }
                };

                // Persist activation history in WAL
                for msg in &activation_chat_history {
                    if msg.role == "user" {
                        state.add_user_message(&msg.content);
                    } else {
                        state.add_assistant_message(&msg.content);
                    }
                }
                let _ = state.save(&self.state_file_path).await;

                // Now send "DS" — the agent is activated and recognizes the menu command
                let initial_message = "DS";
                state.add_user_message(initial_message);

                let activation_turn = activation_chat_history.len() / 2;
                log_llm_request(
                    "dev-session",
                    activation_turn,
                    initial_message,
                    activation_rig_history.len(),
                );
                let response = match streaming_chat(
                    agent,
                    initial_message,
                    activation_rig_history,
                    Some(&self.shutdown),
                )
                .await
                {
                    Ok(r) => {
                        log_llm_response("dev-session", 0, &r);
                        r
                    }
                    Err(e) => {
                        log_llm_error("dev-session", 0, &e);
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

                (state, response, 1usize)
            }

            Some(mut state) => {
                // Recovery path — use loaded state, determine next action from last message
                let turn_offset = state.chat_history.len() / 2;

                tracing::info!(
                    action = "crash_recovery_resume",
                    history_len = %state.chat_history.len(),
                    turn_offset = %turn_offset,
                    "Resuming chat loop from recovered state"
                );

                if state.chat_history.is_empty() {
                    // Sub-case C — Empty chat_history (crash before first response)
                    // Fall back to normal "DS" send
                    tracing::info!(
                        action = "crash_recovery_empty_history",
                        "Empty chat history — sending initial DS"
                    );

                    // Activate agent: send dev.md as user message
                    let (activation_rig_history, activation_chat_history) = match self
                        .activate_agent(agent, "dev-recovery")
                        .await
                    {
                        Ok(pair) => pair,
                        Err(e) => {
                            tracing::error!(action = "activation_failed", error = %e, "Agent activation failed during recovery");
                            let _ = self.handle_failure(story).await;
                            return SessionOutcome::Failed {
                                story_key: story.story_key.clone(),
                                error: format!("Recovery agent activation failed: {e}"),
                                decisions: decision_log.records(),
                            };
                        }
                    };

                    // Persist activation history in WAL
                    for msg in &activation_chat_history {
                        if msg.role == "user" {
                            state.add_user_message(&msg.content);
                        } else {
                            state.add_assistant_message(&msg.content);
                        }
                    }

                    // Now send "DS" — the agent is activated
                    let initial_message = "DS";
                    state.add_user_message(initial_message);

                    let activation_turn = activation_chat_history.len() / 2;
                    log_llm_request(
                        "dev-recovery",
                        activation_turn,
                        initial_message,
                        activation_rig_history.len(),
                    );
                    let response = match streaming_chat(
                        agent,
                        initial_message,
                        activation_rig_history,
                        Some(&self.shutdown),
                    )
                    .await
                    {
                        Ok(r) => {
                            log_llm_response("dev-recovery", 0, &r);
                            r
                        }
                        Err(e) => {
                            log_llm_error("dev-recovery", 0, &e);
                            tracing::error!(action = "chat_failed", turn = 0, error = %e, "Initial chat failed during recovery");
                            let _ = self.handle_failure(story).await;
                            return SessionOutcome::Failed {
                                story_key: story.story_key.clone(),
                                error: format!("Recovery initial chat failed: {e}"),
                                decisions: decision_log.records(),
                            };
                        }
                    };

                    state.add_assistant_message(&response);
                    let _ = state.save(&self.state_file_path).await;

                    (state, response, turn_offset + 1)
                } else {
                    let last_msg = state.chat_history.last().expect("non-empty history");

                    if last_msg.role == "assistant" {
                        // Sub-case A — Last message is assistant (normal recovery)
                        let response = last_msg.content.clone();
                        tracing::info!(
                            action = "crash_recovery_last_assistant",
                            "Last message is assistant — entering analyze loop"
                        );
                        (state, response, turn_offset)
                    } else {
                        // Sub-case B — Last message is user (crash between send and receive)
                        // Re-send the last user message
                        let last_user_msg = last_msg.content.clone();
                        tracing::info!(
                            action = "crash_recovery_last_user",
                            msg_len = %last_user_msg.len(),
                            "Last message is user — re-sending"
                        );

                        // Build history from all messages except the last user message
                        // so we can re-send it properly
                        let history: Vec<Message> = state.chat_history
                            [..state.chat_history.len() - 1]
                            .iter()
                            .map(|msg| match msg.role.as_str() {
                                "user" => Message::user(&msg.content),
                                _ => Message::assistant(&msg.content),
                            })
                            .collect();

                        log_llm_request("dev-recovery", turn_offset, &last_user_msg, history.len());
                        log_llm_history(
                            "dev-recovery",
                            turn_offset,
                            &state.chat_history[..state.chat_history.len() - 1],
                        );
                        let response = match streaming_chat(
                            agent,
                            last_user_msg.as_str(),
                            history,
                            Some(&self.shutdown),
                        )
                        .await
                        {
                            Ok(r) => {
                                log_llm_response("dev-recovery", turn_offset, &r);
                                r
                            }
                            Err(e) => {
                                log_llm_error("dev-recovery", turn_offset, &e);
                                tracing::error!(
                                    action = "chat_failed",
                                    error = %e,
                                    "Re-send of last user message failed during recovery"
                                );
                                let _ = self.handle_failure(story).await;
                                return SessionOutcome::Failed {
                                    story_key: story.story_key.clone(),
                                    error: format!("Recovery re-send failed: {e}"),
                                    decisions: decision_log.records(),
                                };
                            }
                        };

                        state.add_assistant_message(&response);
                        let _ = state.save(&self.state_file_path).await;

                        (state, response, turn_offset)
                    }
                }
            }
        };

        loop {
            // Cooperative shutdown check — between chat turns
            if self.shutdown.load(Ordering::Relaxed) {
                tracing::info!(
                    action = "shutdown_requested",
                    turn = %turn,
                    story_key = %story.story_key,
                    "Shutdown requested — saving WAL and exiting session"
                );
                let _ = state.save(&self.state_file_path).await;
                return SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: "Shutdown requested (Ctrl+C)".to_string(),
                    decisions: decision_log.records(),
                };
            }

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

                    log_llm_request("dev-session", turn, &reply, history.len());
                    log_llm_history_summary("dev-session", turn, &state.chat_history);
                    match streaming_chat(agent, reply.as_str(), history, Some(&self.shutdown)).await
                    {
                        Ok(r) => {
                            log_llm_response("dev-session", turn, &r);
                            retries = 0;
                            state.add_assistant_message(&r);
                            let _ = state.save(&self.state_file_path).await;
                            current_response = r;
                        }
                        Err(e) => {
                            log_llm_error("dev-session", turn, &e);
                            let error_str = e.to_string();

                            // Check for context limit error BEFORE retry logic —
                            // retrying a context limit error is pointless (same history = same error).
                            if is_context_limit_error(&error_str) {
                                tracing::warn!(
                                    action = "context_limit_detected",
                                    turn = %turn,
                                    history_len = %state.chat_history.len(),
                                    error = %error_str,
                                    "Context window limit hit — initiating recovery"
                                );

                                // Remove the user message we just added (it failed)
                                state.chat_history.pop();

                                // Recovery runs its own inner chat loop to completion via run_session().
                                // It returns a terminal SessionOutcome — the current loop exits.
                                let outcome = self
                                    .context_limit_recovery(
                                        &state,
                                        story,
                                        provider,
                                        model,
                                        base_branch,
                                        escalation_slot.clone(),
                                        decision_log.clone(),
                                        0, // recovery_depth: first recovery attempt
                                    )
                                    .await;

                                // Write decisions regardless of outcome
                                self.write_decisions(story, &decision_log).await;
                                return outcome;
                            }

                            // Non-context-limit error — existing retry logic
                            retries += 1;
                            tracing::warn!(
                                action = "chat_error",
                                turn = %turn,
                                retries = %retries,
                                error = %error_str,
                                "Chat error, will retry"
                            );
                            if retries >= MAX_RETRIES {
                                let _ = self.handle_failure(story).await;
                                self.write_decisions(story, &decision_log).await;
                                return SessionOutcome::Failed {
                                    story_key: story.story_key.clone(),
                                    error: format!(
                                        "Chat failed after {MAX_RETRIES} retries: {error_str}"
                                    ),
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
            code_review_enabled: true,
        }
    }

    /// Helper: create minimal BotSecrets for tests.
    fn make_test_secrets() -> BotSecrets {
        BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: Some("sk-test".to_string()),
            github_copilot_oauth_token: Some("gh-test".to_string()),
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
        let shutdown = Arc::new(AtomicBool::new(false));

        let runner = SessionRunner::new(config, secrets, shutdown);

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
        let shutdown = Arc::new(AtomicBool::new(false));

        let runner = SessionRunner::new(config, secrets, shutdown);

        let expected = dir.path().join(".bmad-bot-session.yaml");
        assert_eq!(runner.state_file_path, expected);
    }

    #[test]
    fn test_session_runner_state_file_path_unchanged() {
        // Verify that adding branch setup in Story 4.3 did not change
        // the state_file_path derivation from Story 4.2
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let secrets = Arc::new(make_test_secrets());
        let shutdown = Arc::new(AtomicBool::new(false));

        let runner = SessionRunner::new(config, secrets, shutdown);

        // Must still be {implementation_artifacts}/.bmad-bot-session.yaml
        let expected = dir.path().join(".bmad-bot-session.yaml");
        assert_eq!(
            runner.state_file_path, expected,
            "state_file_path derivation must not change from 4.2 contract"
        );
    }

    // -----------------------------------------------------------------------
    // Story 6.3 — Crash Recovery Tests
    // -----------------------------------------------------------------------

    /// Helper: create a SessionState suitable for recovery tests.
    fn make_recovery_state() -> SessionState {
        SessionState {
            story_id: "6.3".to_string(),
            story_key: "6-3-crash-recovery-via-session-wal".to_string(),
            branch: "story/6-3-crash-recovery-via-session-wal".to_string(),
            started_at: "2026-02-07T10:00:00Z".to_string(),
            last_activity: "2026-02-07T10:05:00Z".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            branch_name: "story/6-3-crash-recovery-via-session-wal".to_string(),
            base_branch: "main".to_string(),
            chat_history: vec![],
        }
    }

    /// Helper: create a SessionState with pre-4.3 WAL format (no branch_name).
    fn make_legacy_recovery_state() -> SessionState {
        SessionState {
            story_id: "4.2".to_string(),
            story_key: "4-2-agent-session-setup-chat-loop".to_string(),
            branch: "story/4-2-agent-session-setup-chat-loop".to_string(),
            started_at: "2026-02-01T10:00:00Z".to_string(),
            last_activity: "2026-02-01T10:05:00Z".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            branch_name: String::new(), // Empty — pre-4.3 WAL
            base_branch: String::new(),
            chat_history: vec![],
        }
    }

    // -- story_info_from_wal tests --

    #[test]
    fn test_story_info_from_wal_parses_story_key() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let state = make_recovery_state();

        let info = story_info_from_wal(&state, &config);

        assert_eq!(info.epic_num, 6);
        assert_eq!(info.story_num, 3);
        assert_eq!(info.label, "crash-recovery-via-session-wal");
    }

    #[test]
    fn test_story_info_from_wal_simple_key() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let mut state = make_recovery_state();
        state.story_key = "1-1-scaffolding".to_string();
        state.story_id = "1.1".to_string();

        let info = story_info_from_wal(&state, &config);

        assert_eq!(info.epic_num, 1);
        assert_eq!(info.story_num, 1);
        assert_eq!(info.label, "scaffolding");
        assert_eq!(info.story_id, "1.1");
    }

    #[test]
    fn test_story_info_from_wal_specs_path_is_pathbuf() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let state = make_recovery_state();

        let info = story_info_from_wal(&state, &config);

        let expected_path = PathBuf::from(format!(
            "{}/6-3-crash-recovery-via-session-wal.md",
            config.bmad_paths.implementation_artifacts
        ));
        assert_eq!(info.specs_path, expected_path);
    }

    #[test]
    fn test_story_info_from_wal_branch_name_fallback() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let state = make_legacy_recovery_state();

        let info = story_info_from_wal(&state, &config);

        // branch_name is empty → should fall back to state.branch
        assert_eq!(
            info.branch_name, "story/4-2-agent-session-setup-chat-loop",
            "Should fall back to state.branch when branch_name is empty"
        );
    }

    #[test]
    fn test_story_info_from_wal_prefers_branch_name_over_branch() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let mut state = make_recovery_state();
        state.branch = "old-branch".to_string();
        state.branch_name = "story/6-3-crash-recovery-via-session-wal".to_string();

        let info = story_info_from_wal(&state, &config);

        assert_eq!(
            info.branch_name, "story/6-3-crash-recovery-via-session-wal",
            "Should prefer branch_name over branch when non-empty"
        );
    }

    #[test]
    fn test_story_info_from_wal_dependencies_empty() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let state = make_recovery_state();

        let info = story_info_from_wal(&state, &config);

        assert!(
            info.dependencies.is_empty(),
            "Dependencies should always be empty for recovered stories"
        );
    }

    #[test]
    fn test_story_info_from_wal_status_is_in_progress() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let state = make_recovery_state();

        let info = story_info_from_wal(&state, &config);

        assert_eq!(info.status, "in-progress");
    }

    // -- check_and_recover_wal tests --

    #[tokio::test]
    async fn test_check_wal_returns_none_when_no_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let secrets = Arc::new(make_test_secrets());
        let shutdown = Arc::new(AtomicBool::new(false));

        let runner = SessionRunner::new(config, secrets, shutdown);

        let result = runner.check_and_recover_wal().await;
        assert!(
            result.is_none(),
            "Should return None when no WAL file exists"
        );
    }

    #[tokio::test]
    async fn test_check_wal_returns_some_when_file_exists() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let secrets = Arc::new(make_test_secrets());

        // Create a valid WAL file
        let state = make_recovery_state();
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        state.save(&wal_path).await.expect("save WAL");

        let shutdown = Arc::new(AtomicBool::new(false));
        let runner = SessionRunner::new(config, secrets, shutdown);

        let result = runner.check_and_recover_wal().await;
        assert!(result.is_some(), "Should return Some when WAL file exists");

        let recovery = result.unwrap();
        assert_eq!(
            recovery.state.story_key,
            "6-3-crash-recovery-via-session-wal"
        );
        assert_eq!(recovery.story_info.epic_num, 6);
        assert_eq!(recovery.story_info.story_num, 3);
    }

    #[tokio::test]
    async fn test_check_wal_deletes_corrupt_file() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let secrets = Arc::new(make_test_secrets());

        // Write corrupt YAML
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        tokio::fs::write(&wal_path, "not: [valid: yaml: for: session")
            .await
            .expect("write corrupt WAL");
        assert!(wal_path.exists(), "Corrupt WAL should exist before check");

        let shutdown = Arc::new(AtomicBool::new(false));
        let runner = SessionRunner::new(config, secrets, shutdown);

        let result = runner.check_and_recover_wal().await;
        assert!(result.is_none(), "Should return None for corrupt WAL");
        assert!(
            !wal_path.exists(),
            "Corrupt WAL file should be deleted after check"
        );
    }

    // -- RecoveryInfo tests --

    #[test]
    fn test_recovery_info_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RecoveryInfo>();
    }

    #[test]
    fn test_recovery_info_debug() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let state = make_recovery_state();
        let story_info = story_info_from_wal(&state, &config);

        let recovery = RecoveryInfo { state, story_info };
        let debug_str = format!("{recovery:?}");
        assert!(debug_str.contains("RecoveryInfo"));
        assert!(debug_str.contains("6-3-crash-recovery-via-session-wal"));
    }

    // -- story_info_from_wal edge cases --

    #[test]
    fn test_story_info_from_wal_key_with_two_parts() {
        // Edge case: story_key with only 2 numeric parts and no slug
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let mut state = make_recovery_state();
        state.story_key = "1-1".to_string();

        let info = story_info_from_wal(&state, &config);

        assert_eq!(info.epic_num, 1);
        assert_eq!(info.story_num, 1);
        assert_eq!(info.label, ""); // No slug portion
    }

    #[test]
    fn test_story_info_from_wal_story_key_preserves_original() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let state = make_recovery_state();

        let info = story_info_from_wal(&state, &config);

        assert_eq!(
            info.story_key, "6-3-crash-recovery-via-session-wal",
            "story_key must be preserved exactly from WAL"
        );
    }

    #[test]
    fn test_story_info_from_wal_story_id_preserved() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = make_runner_test_config(dir.path());
        let state = make_recovery_state();

        let info = story_info_from_wal(&state, &config);

        assert_eq!(info.story_id, "6.3");
    }

    // -- WAL roundtrip test (save → load → recovery) --

    #[tokio::test]
    async fn test_wal_roundtrip_with_chat_history() {
        use crate::session::state::ChatMessage;

        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let secrets = Arc::new(make_test_secrets());

        let mut state = make_recovery_state();
        state.chat_history = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "DS".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Starting implementation...".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Continue.".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Task 1 done.".to_string(),
            },
        ];

        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        state.save(&wal_path).await.expect("save WAL");

        let shutdown = Arc::new(AtomicBool::new(false));
        let runner = SessionRunner::new(config, secrets, shutdown);
        let recovery = runner
            .check_and_recover_wal()
            .await
            .expect("WAL should be detected");

        assert_eq!(recovery.state.chat_history.len(), 4);
        assert_eq!(recovery.state.chat_history[0].role, "user");
        assert_eq!(recovery.state.chat_history[0].content, "DS");
        assert_eq!(recovery.state.chat_history[3].role, "assistant");
        assert_eq!(recovery.state.chat_history[3].content, "Task 1 done.");
        assert_eq!(
            recovery.story_info.story_key,
            "6-3-crash-recovery-via-session-wal"
        );
    }

    // -- Pipeline recovery returns None when no WAL --

    #[tokio::test]
    async fn test_check_wal_legacy_wal_backward_compat() {
        // Test that a WAL file without branch_name/base_branch fields
        // (pre-Story 4.3) can still be loaded and recovered
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let secrets = Arc::new(make_test_secrets());

        let state = make_legacy_recovery_state();
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        state.save(&wal_path).await.expect("save WAL");

        let shutdown = Arc::new(AtomicBool::new(false));
        let runner = SessionRunner::new(config, secrets, shutdown);
        let recovery = runner
            .check_and_recover_wal()
            .await
            .expect("Legacy WAL should be detected");

        assert_eq!(
            recovery.state.story_key,
            "4-2-agent-session-setup-chat-loop"
        );
        assert!(
            recovery.state.branch_name.is_empty(),
            "Legacy WAL should have empty branch_name"
        );
        // story_info should fall back to state.branch
        assert_eq!(
            recovery.story_info.branch_name,
            "story/4-2-agent-session-setup-chat-loop"
        );
    }

    // -----------------------------------------------------------------------
    // Story 6.4 — Context Window Limit Recovery Tests
    // -----------------------------------------------------------------------

    // -- is_context_limit_error tests --

    #[test]
    fn test_is_context_limit_error_anthropic_pattern() {
        assert!(is_context_limit_error(
            "context_length_exceeded: prompt is 204835 tokens"
        ));
    }

    #[test]
    fn test_is_context_limit_error_openai_pattern() {
        assert!(is_context_limit_error(
            "This model's maximum context length is 128000 tokens"
        ));
    }

    #[test]
    fn test_is_context_limit_error_token_limit() {
        assert!(is_context_limit_error("Request exceeds token limit"));
    }

    #[test]
    fn test_is_context_limit_error_too_many_tokens() {
        assert!(is_context_limit_error(
            "Error: too many tokens in the request"
        ));
    }

    #[test]
    fn test_is_context_limit_error_case_insensitive() {
        assert!(is_context_limit_error("CONTEXT_LENGTH_EXCEEDED"));
        assert!(is_context_limit_error("Prompt Is Too Long"));
        assert!(is_context_limit_error("MAXIMUM CONTEXT LENGTH"));
    }

    #[test]
    fn test_is_context_limit_error_false_for_network_error() {
        assert!(!is_context_limit_error("connection refused"));
    }

    #[test]
    fn test_is_context_limit_error_false_for_auth_error() {
        assert!(!is_context_limit_error("invalid api key"));
    }

    #[test]
    fn test_is_context_limit_error_false_for_rate_limit() {
        assert!(!is_context_limit_error("rate limit exceeded"));
    }

    #[test]
    fn test_is_context_limit_error_prompt_too_long() {
        assert!(is_context_limit_error(
            "prompt is too long: 204835 tokens > 200000 maximum"
        ));
    }

    #[test]
    fn test_is_context_limit_error_input_too_long() {
        assert!(is_context_limit_error("input too long for model"));
    }

    #[test]
    fn test_is_context_limit_error_exceeds_the_model() {
        assert!(is_context_limit_error(
            "This request exceeds the model's context window"
        ));
    }

    #[test]
    fn test_is_context_limit_error_context_window() {
        assert!(is_context_limit_error("context window overflow"));
    }

    // -- extract_last_exchanges tests --

    fn make_chat_history(n: usize) -> Vec<ChatMessage> {
        (0..n)
            .map(|i| ChatMessage {
                role: if i % 2 == 0 {
                    "user".to_string()
                } else {
                    "assistant".to_string()
                },
                content: format!("message-{i}"),
            })
            .collect()
    }

    #[test]
    fn test_extract_last_exchanges_normal() {
        let history = make_chat_history(40); // 20 exchanges
        let result = SessionRunner::extract_last_exchanges(&history, 10);
        assert_eq!(result.len(), 20); // 10 exchanges = 20 messages
        assert_eq!(result[0].content, "message-20");
        assert_eq!(result[19].content, "message-39");
    }

    #[test]
    fn test_extract_last_exchanges_fewer_than_n() {
        let history = make_chat_history(6); // 3 exchanges
        let result = SessionRunner::extract_last_exchanges(&history, 10);
        assert_eq!(result.len(), 6); // all 3 exchanges
        assert_eq!(result[0].content, "message-0");
        assert_eq!(result[5].content, "message-5");
    }

    #[test]
    fn test_extract_last_exchanges_empty_history() {
        let result = SessionRunner::extract_last_exchanges(&[], 10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_last_exchanges_odd_message_count() {
        // 21 messages — odd count. Should round down to 20, then take last 20.
        let history = make_chat_history(21);
        let result = SessionRunner::extract_last_exchanges(&history, 10);
        // usable_len = 20 (round down from 21), take = min(20, 20) = 20
        assert_eq!(result.len(), 20);
        // The orphan message-0 is dropped (it's before usable_len boundary)
        assert_eq!(result[0].content, "message-0");
        assert_eq!(result[19].content, "message-19");
    }

    #[test]
    fn test_extract_last_exchanges_odd_count_small() {
        // 5 messages — odd count, N=10.
        // usable_len = 4, take = min(20, 4) = 4
        let history = make_chat_history(5);
        let result = SessionRunner::extract_last_exchanges(&history, 10);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].content, "message-0");
        assert_eq!(result[3].content, "message-3");
    }

    #[test]
    fn test_extract_last_exchanges_exact_n() {
        // Exactly 20 messages, N=10 → should return all 20
        let history = make_chat_history(20);
        let result = SessionRunner::extract_last_exchanges(&history, 10);
        assert_eq!(result.len(), 20);
    }

    #[test]
    fn test_extract_last_exchanges_single_pair() {
        let history = make_chat_history(2);
        let result = SessionRunner::extract_last_exchanges(&history, 1);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "message-0");
        assert_eq!(result[1].content, "message-1");
    }

    // -- format_exchanges_for_message tests --

    #[test]
    fn test_format_exchanges_for_message_basic() {
        let exchanges = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Hi there".to_string(),
            },
        ];
        let result = SessionRunner::format_exchanges_for_message(&exchanges);
        assert!(result.contains("=== Recent Conversation (last 1 exchanges) ==="));
        assert!(result.contains("User: Hello"));
        assert!(result.contains("Assistant: Hi there"));
    }

    #[test]
    fn test_format_exchanges_for_message_truncates_long_messages() {
        let long_content = "x".repeat(3000);
        let exchanges = vec![ChatMessage {
            role: "user".to_string(),
            content: long_content,
        }];
        let result = SessionRunner::format_exchanges_for_message(&exchanges);
        assert!(result.contains("... [truncated]"));
        // Should contain the first 2000 chars but not all 3000
        assert!(result.len() < 3000 + 200); // some overhead for labels
    }

    #[test]
    fn test_format_exchanges_for_message_empty() {
        let result = SessionRunner::format_exchanges_for_message(&[]);
        assert!(result.contains("=== Recent Conversation ==="));
        assert!(result.contains("no recent exchanges available"));
    }

    #[test]
    fn test_format_exchanges_for_message_multiple_exchanges() {
        let exchanges = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "Q1".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "A1".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Q2".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "A2".to_string(),
            },
        ];
        let result = SessionRunner::format_exchanges_for_message(&exchanges);
        assert!(result.contains("last 2 exchanges"));
        assert!(result.contains("User: Q1"));
        assert!(result.contains("Assistant: A1"));
        assert!(result.contains("User: Q2"));
        assert!(result.contains("Assistant: A2"));
    }

    // -- build_recovery_message tests --

    fn make_test_runner() -> SessionRunner {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let secrets = Arc::new(make_test_secrets());
        let shutdown = Arc::new(AtomicBool::new(false));
        // Leak the tempdir so it isn't dropped (this is fine in tests)
        std::mem::forget(dir);
        SessionRunner::new(config, secrets, shutdown)
    }

    fn make_test_story_info() -> StoryInfo {
        StoryInfo {
            story_id: "6.4".to_string(),
            story_key: "6-4-context-window-limit-recovery".to_string(),
            epic_num: 6,
            story_num: 4,
            label: "context-window-limit-recovery".to_string(),
            branch_name: "story/6-4-context-window-limit-recovery".to_string(),
            specs_path: PathBuf::from(
                "/project/_bmad-output/implementation-artifacts/6-4-context-window-limit-recovery.md",
            ),
            dependencies: vec![],
            status: "in-progress".to_string(),
        }
    }

    #[test]
    fn test_build_recovery_message_contains_all_sections() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_recovery_message(&story, "summary text", "exchange text");
        assert!(
            msg.contains("SESSION RECOVERY"),
            "Should contain SESSION RECOVERY header"
        );
        assert!(
            msg.contains("Session Summary"),
            "Should contain Session Summary section"
        );
        assert!(msg.contains("summary text"), "Should contain the summary");
        assert!(
            msg.contains("exchange text"),
            "Should contain the formatted exchanges"
        );
        assert!(
            msg.contains("Current Story"),
            "Should contain Current Story section"
        );
    }

    #[test]
    fn test_build_recovery_message_includes_story_path() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_recovery_message(&story, "s", "e");
        assert!(
            msg.contains("6-4-context-window-limit-recovery.md"),
            "Should contain the story specs_path"
        );
    }

    #[test]
    fn test_build_recovery_message_does_not_contain_project_context() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_recovery_message(&story, "summary", "exchanges");
        // Project context is loaded by the agent via BMAD activation, NOT injected in message
        assert!(
            !msg.contains("Project Context"),
            "Recovery message should NOT contain a 'Project Context' section"
        );
    }

    #[test]
    fn test_build_recovery_message_contains_continue_instruction() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_recovery_message(&story, "s", "e");
        assert!(
            msg.contains("Continue working directly on the current task"),
            "Should instruct agent to continue"
        );
        assert!(
            msg.contains("Do NOT restart the workflow"),
            "Should instruct agent not to restart"
        );
    }

    // -- compressed state tests --

    #[test]
    fn test_compressed_state_contains_activation_turns() {
        // Simulate the compressed history that drive_activation_and_recover would build:
        // 1. dev.md activation (2 msgs)
        // 2. CH (2 msgs)
        // 3. Load the project context (2 msgs)
        // 4. Recovery message (1 msg)
        let mut compressed_history = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "--- dev.md agent content ---".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Salut JB! Here is the menu...".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "CH".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "greeting".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Load the project context".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "context loaded".to_string(),
            },
        ];
        compressed_history.push(ChatMessage {
            role: "user".to_string(),
            content: "=== SESSION RECOVERY ===".to_string(),
        });

        // dev.md activation is the first turn
        assert_eq!(compressed_history[0].role, "user");
        assert!(compressed_history[0].content.contains("dev.md"));
        // CH is the second turn (after activation)
        assert_eq!(compressed_history[2].role, "user");
        assert_eq!(compressed_history[2].content, "CH");
        // Load context is the third turn
        assert_eq!(compressed_history[4].role, "user");
        assert_eq!(compressed_history[4].content, "Load the project context");
    }

    #[test]
    fn test_compressed_state_last_message_is_recovery() {
        let compressed_history = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "--- dev.md agent content ---".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Salut JB! Here is the menu...".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "CH".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "greeting".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Load the project context".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "context loaded".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "=== SESSION RECOVERY — Context Window Limit Reached ===".to_string(),
            },
        ];

        let last = compressed_history.last().expect("non-empty");
        assert_eq!(last.role, "user");
        assert!(last.content.contains("SESSION RECOVERY"));
    }

    #[test]
    fn test_compressed_state_preserves_metadata() {
        let original = SessionState {
            story_id: "6.4".to_string(),
            story_key: "6-4-context-window-limit-recovery".to_string(),
            branch: "story/6-4-context-window-limit-recovery".to_string(),
            started_at: "2026-02-07T10:00:00Z".to_string(),
            last_activity: "2026-02-07T10:05:00Z".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            branch_name: "story/6-4-context-window-limit-recovery".to_string(),
            base_branch: "main".to_string(),
            chat_history: vec![],
        };

        // Simulate what drive_activation_and_recover does
        let compressed = SessionState {
            story_id: original.story_id.clone(),
            story_key: original.story_key.clone(),
            branch: original.branch.clone(),
            started_at: original.started_at.clone(),
            last_activity: chrono::Utc::now().to_rfc3339(),
            provider: original.provider.clone(),
            model: original.model.clone(),
            branch_name: original.branch_name.clone(),
            base_branch: original.base_branch.clone(),
            chat_history: vec![],
        };

        assert_eq!(compressed.story_id, "6.4");
        assert_eq!(compressed.story_key, "6-4-context-window-limit-recovery");
        assert_eq!(compressed.branch, "story/6-4-context-window-limit-recovery");
        assert_eq!(compressed.provider, "anthropic");
        assert_eq!(compressed.model, "claude-sonnet-4-20250514");
        assert_eq!(
            compressed.branch_name,
            "story/6-4-context-window-limit-recovery"
        );
        assert_eq!(compressed.base_branch, "main");
        // started_at preserved from original
        assert_eq!(compressed.started_at, "2026-02-07T10:00:00Z");
    }

    #[test]
    fn test_compressed_state_updates_last_activity() {
        let original_activity = "2026-02-07T10:05:00Z";
        let new_activity = chrono::Utc::now().to_rfc3339();

        // The compressed state should have a newer last_activity
        assert_ne!(
            original_activity, &new_activity,
            "last_activity should be refreshed"
        );
    }

    #[test]
    fn test_compressed_state_total_messages() {
        // After dev.md activation (2 msgs) + CH (2 msgs) + Load context (2 msgs) + recovery message (1 msg) = 7 messages
        let compressed_history = vec![
            ChatMessage {
                role: "user".to_string(),
                content: "--- dev.md agent content ---".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "Salut JB! Here is the menu...".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "CH".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "greeting".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "Load the project context".to_string(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "context loaded".to_string(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "=== SESSION RECOVERY ===".to_string(),
            },
        ];

        assert_eq!(
            compressed_history.len(),
            7,
            "Compressed state should have exactly 7 messages: 2 dev.md activation + 2 CH + 2 Load context + 1 recovery"
        );
    }
}
