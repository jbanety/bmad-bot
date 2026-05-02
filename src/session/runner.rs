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

use crate::config::BotConfig;
use crate::llm::agent_factory::{AgentFactory, BuiltAgent, LlmRole};
use crate::llm::logging::{
    log_llm_error, log_llm_history, log_llm_history_summary, log_llm_request, log_llm_response,
};
use crate::session::SessionOutcome;
use crate::session::agent;
/// Re-export [`ShutdownFlag`] so existing callers (`pipeline.rs`, `cli/mod.rs`) keep working.
pub use crate::session::agent::ShutdownFlag;
use crate::session::analyzer::{ResponseAction, ResponseAnalyzer};
use crate::session::branch::{BranchAction, determine_base_branch, ensure_story_branch};
use crate::session::cleanup::{
    get_dirty_files, mark_story_needs_clarification, preserve_partial_work,
};
use crate::session::consultation::{ConsultationConfig, ConsultationRunner, ConsultationState};
use crate::session::escalation::EscalationReport;
use crate::session::provider::ProviderError;
use crate::session::state::{ChatMessage, PHASE_DEV, SessionState};
use crate::supervisor::EscalationSlot;
use crate::supervisor::decisions::{DecisionLog, write_decisions_file};
use crate::tools::SubAgentState;

use crate::watcher::StoryInfo;

use rig::completion::Message;
use rig::tools::think::ThinkTool;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

/// Maximum number of chat turns before the safety net kicks in.
///
/// Prevents infinite loops if the agent never signals completion. A future
/// improvement could make this configurable via `BotConfig`.
const MAX_CHAT_TURNS: usize = 300;

/// Number of recent exchanges (user+assistant pairs) to keep verbatim in the
/// recovery message after a context window limit error.
const RECOVERY_KEEP_LAST_EXCHANGES: usize = 10;

/// Maximum recursive recovery depth — prevents infinite context-limit loops.
const MAX_RECOVERY_DEPTH: usize = 3;

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
/// Detect transient LLM errors that should be retried with backoff.
///
/// Covers HTTP 429 (rate limit), 500 (internal server error), 503 (service
/// unavailable / high demand), and timeout patterns. These are temporary
/// failures — the same request may succeed after a short delay.
fn is_transient_llm_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("503")
        || lower.contains("service unavailable")
        || lower.contains("high demand")
        || lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("500 internal server error")
        || lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("try again later")
        || lower.contains("error decoding response body")
        || lower.contains("unexpected eof")
        || lower.contains("sse error")
        || lower.contains("broken pipe")
        || lower.contains("connection closed")
        || lower.contains("error sending request")
        || lower.contains("http client error")
        || lower.contains("token expired")
        || lower.contains("unauthorized")
}

/// Maximum number of retries for transient LLM errors during activation.
const ACTIVATION_MAX_RETRIES: usize = 3;

/// Initial backoff delay in seconds for transient error retries.
const ACTIVATION_BACKOFF_BASE_SECS: u64 = 5;

/// Initial backoff delay in seconds for chat loop transient retries.
const CHAT_LOOP_BACKOFF_BASE_SECS: u64 = 5;

/// Build the impact analysis prompt sent to the agent after story completion.
///
/// The prompt instructs the agent to read `sprint-status.yaml`, identify
/// downstream dependent stories, compare their "Previous Story Intelligence"
/// sections against actual implementation, and update only where deviations
/// exist. Architecture references are updated only when new modules or changed
/// interfaces were introduced and `architecture.md` exists.
///
/// This is a pure string builder extracted for testability — the call site
/// remains inline in the `ResponseAction::Completed` arm of `run_session()`.
pub fn build_impact_analysis_prompt(
    story_key: &str,
    impl_artifacts: &str,
    planning_artifacts: &str,
) -> String {
    format!(
        "You have just completed story {story_key}. Perform a post-implementation \
        impact analysis on downstream dependent stories.\n\n\
        GOAL: Ensure every downstream story has accurate context about what was \
        actually built, so the agent starting that story works from reality — not \
        stale assumptions.\n\n\
        INSTRUCTIONS:\n\
        1. Read `{impl_artifacts}/sprint-status.yaml` and identify stories whose \
           `depends-on` references `{story_key}` (full key or short key). \
           Also check subsequent stories in the same epic (document order) as a \
           secondary criterion.\n\
        2. For each downstream story file found in `{impl_artifacts}/`, read the \
           ENTIRE file. Compare ALL sections against what was actually implemented \
           in {story_key}.\n\
        3. Update any section in Dev Notes where actual implementation deviates \
           from planned assumptions. This includes but is not limited to:\n\
           - \"Previous Story Intelligence\" — replace with accurate post-implementation context\n\
           - \"Technical Requirements\" — fix stale API signatures, module paths, patterns\n\
           - \"Architecture Compliance\" — correct module locations, import paths\n\
           - \"Dependencies Required\" — add/remove deps based on what was actually used\n\
           - \"Testing Standards\" — update if test patterns or helpers changed\n\
           - \"File Structure\" — correct if actual file layout differs from planned\n\
           Sections must be REPLACED (idempotent), not appended.\n\
        4. Update Tasks / Subtasks where they reference specific implementation \
           details that are now stale (e.g., wrong function names, wrong module \
           paths, wrong API patterns, wrong file locations). Fix the concrete \
           details but preserve the task structure and intent.\n\
        5. Check if `{planning_artifacts}/architecture.md` exists. If it does AND \
           this story introduced new modules or changed interfaces, update the \
           relevant architecture references. If the file does not exist, skip.\n\
        6. If ANY story files or architecture were updated, commit with message: \
           `docs(stories): update downstream specs after {story_key}`\n\
        7. If nothing needs updating, report that and move on — do NOT invent changes.\n\n\
        SCOPE GUARD — DO NOT MODIFY:\n\
        - The \"Story\" section (user story text)\n\
        - The \"Acceptance Criteria\" section (owned by the PM)\n\
        - The \"Dev Agent Record\" section (filled at execution time)\n\
        - Any story that is already `done` or `review` in sprint-status.yaml\n\
        - Stories that do NOT depend on {story_key}\n\n\
        QUALITY RULES:\n\
        - Only update what genuinely needs it — do not invent changes\n\
        - When updating, include concrete details: actual function names, actual \
          module paths, actual struct fields — not vague summaries\n\
        - If a section is accurate as-is, leave it alone",
    )
}

/// Parse a structured PR summary from the agent's response.
///
/// Preferred format uses XML sub-tags inside `<pr-summary>`:
/// `<context>`, `<how-to-test>`, `<additional-info>`.
///
/// **Lenient fallback:** If `<pr-summary>` is present but the sub-tags are
/// missing (common after long contexts where the agent forgets the exact
/// format), the raw content between `<pr-summary>...</pr-summary>` is used
/// as the `context` field, with empty strings for the other two fields.
///
/// Returns `None` only if `<pr-summary>` itself is absent.
///
/// Uses regex with dotall mode (`(?s)`) so `.` matches newlines within tag content.
pub fn parse_pr_summary(response: &str) -> Option<(String, String, String)> {
    use regex::Regex;
    use std::sync::LazyLock;

    static RE_PR_SUMMARY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?si)<pr-summary>(.*?)</pr-summary>").unwrap());
    static RE_CONTEXT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?si)<context>(.*?)</context>").unwrap());
    static RE_HOW_TO_TEST: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?si)<how-to-test>(.*?)</how-to-test>").unwrap());
    static RE_ADDITIONAL_INFO: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?si)<additional-info>(.*?)</additional-info>").unwrap());

    // Outer <pr-summary> tag is mandatory
    let summary_block = RE_PR_SUMMARY.captures(response)?.get(1)?.as_str().trim();

    if summary_block.is_empty() {
        return None;
    }

    // Try to extract structured sub-tags
    let context = RE_CONTEXT
        .captures(summary_block)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();
    let how_to_test = RE_HOW_TO_TEST
        .captures(summary_block)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();
    let additional_info = RE_ADDITIONAL_INFO
        .captures(summary_block)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();

    // If all sub-tags are present, use them
    if !context.is_empty() {
        return Some((context, how_to_test, additional_info));
    }

    // Lenient fallback: no sub-tags, use raw content as context
    tracing::debug!(
        action = "pr_summary_lenient_parse",
        "No sub-tags found in <pr-summary> — using raw content as context"
    );
    Some((summary_block.to_string(), String::new(), String::new()))
}

fn is_context_limit_error(error_msg: &str) -> bool {
    let lower = error_msg.to_lowercase();
    // Anthropic patterns
    lower.contains("context_length_exceeded")
        || lower.contains("prompt is too long")
        || lower.contains("maximum context length")
        // OpenAI / Copilot patterns
        || lower.contains("max_tokens")
        || lower.contains("prompt_tokens_exceeded")
        || lower.contains("token limit")
        || lower.contains("exceeds the limit")
        || lower.contains("prompt token count")
        || lower.contains("context window")
        // Generic patterns
        || lower.contains("too many tokens")
        || lower.contains("input too long")
        || lower.contains("exceeds the model")
}

/// Truncate a string to `max_len` characters, appending `…` if truncated.
///
/// Uses `char_indices` to find a safe Unicode boundary — never slices by byte
/// index directly (e.g., `&text[..80]` panics on multi-byte chars).
pub(crate) fn truncate_summary(text: &str, max_len: usize) -> String {
    match text.char_indices().nth(max_len) {
        Some((byte_idx, _)) => {
            let mut t = text[..byte_idx].to_string();
            t.push('…');
            t
        }
        None => text.to_string(),
    }
}

/// Session runner — manages the full lifecycle of a single story development session.
///
/// Accepts an `Arc<McpManager>` at construction time so that Story 9.2 can
/// register MCP-discovered tools on the agent builder.
///
/// Constructed once per daemon run and reused across stories. Each call to
/// [`run()`](Self::run) creates a fresh agent, WAL, and chat loop for one story.
/// Also provides [`check_and_recover_wal()`](Self::check_and_recover_wal) and
/// [`resume_session()`](Self::resume_session) for crash recovery on daemon restart.
pub struct SessionRunner {
    /// Shared daemon configuration.
    config: Arc<BotConfig>,
    /// Centralized agent construction factory (owns secrets and provider credentials).
    agent_factory: Arc<AgentFactory>,
    /// Path to the WAL state file: `{implementation_artifacts}/.bmad-bot-session.yaml`.
    state_file_path: PathBuf,
    /// Stateless response analyzer (constructed once, reused).
    analyzer: ResponseAnalyzer,
    /// Cooperative shutdown flag — checked between streaming chunks and chat turns.
    shutdown: ShutdownFlag,
    /// MCP server manager — provides external tool capabilities (Story 9.2 usage).
    mcp_manager: Arc<crate::mcp::McpManager>,
    /// Shared sub-agent session map — threaded into `SpawnAgentTool` at
    /// `build_agent_for_role` time (Story 12.4).
    sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>>,
    /// Shared in-flight-follow-up set — prevents concurrent follow-ups on the same
    /// sub-agent session (Story 12.4).
    sub_agent_in_flight: Arc<Mutex<HashSet<String>>>,
    /// UI handle for rendering terminal output (fire-and-forget, Story 10.3).
    ui: crate::ui::UiHandle,
    /// Skill file path passed to `activate_agent()` for dev sessions (Story 12.1).
    /// Uses BMAD skill-based activation instead of persona/menu files.
    skill_path: String,
    /// Consultation runner for daemon-orchestrated consultations (Decision 10).
    consultation_runner: ConsultationRunner,
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
    ///
    /// `sub_agent_sessions` and `sub_agent_in_flight` are the pipeline-owned shared
    /// state that backs [`SpawnAgentTool`](crate::tools::SpawnAgentTool); the runner
    /// clones them into the tool at agent-build time (Story 12.4).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<BotConfig>,
        agent_factory: Arc<AgentFactory>,
        shutdown: ShutdownFlag,
        mcp_manager: Arc<crate::mcp::McpManager>,
        sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>>,
        sub_agent_in_flight: Arc<Mutex<HashSet<String>>>,
        ui: crate::ui::UiHandle,
        skill_path: String,
    ) -> Self {
        let state_file_path =
            Path::new(&config.bmad_paths.implementation_artifacts).join(".bmad-bot-session.yaml");
        let consultation_runner = ConsultationRunner::new(
            agent_factory.clone(),
            shutdown.clone(),
            ui.clone(),
            PathBuf::from(&config.bmad_paths.project_root),
        );
        Self {
            config,
            agent_factory,
            state_file_path,
            analyzer: ResponseAnalyzer::new(),
            shutdown,
            mcp_manager,
            sub_agent_sessions,
            sub_agent_in_flight,
            ui,
            skill_path,
            consultation_runner,
        }
    }

    pub fn config(&self) -> &BotConfig {
        &self.config
    }

    /// Get an `Arc` clone of the [`AgentFactory`].
    pub fn agent_factory_arc(&self) -> Arc<AgentFactory> {
        Arc::clone(&self.agent_factory)
    }

    /// Path to the WAL state file on disk.
    pub fn state_file_path(&self) -> PathBuf {
        self.state_file_path.clone()
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
                // Check if branch exists using git CLI
                let output = std::process::Command::new("git")
                    .arg("-C")
                    .arg(&rp)
                    .args(["branch", "--list", &bn])
                    .output()
                    .map_err(|e| format!("git branch --list failed: {e}"))?;
                if !output.status.success() || output.stdout.is_empty() {
                    return Err(format!("Recovery branch not found: {bn}"));
                }
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

        // Phase 2 — Build agent via factory (handles API key resolution)
        let provider_name = state.provider.clone();
        let model_name = state.model.clone();
        let escalation_slot: EscalationSlot = Arc::new(std::sync::Mutex::new(None));
        let decision_log = DecisionLog::new();
        let base_branch = state.base_branch.clone();

        // Resolve skill path and preamble from WAL — create-story sessions
        // use a different preamble than dev-story sessions.
        let effective_skill_path = if state.skill_path.is_empty() {
            self.skill_path.clone()
        } else {
            state.skill_path.clone()
        };
        let is_create_session = effective_skill_path.contains("bmad-create-story");

        let dev_preamble = if is_create_session {
            agent::build_create_preamble()
        } else {
            match self.build_preamble(&story_info).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(
                        action = "crash_recovery_preamble_failed",
                        error = %e,
                        "Preamble build failed during recovery"
                    );
                    let _ = SessionState::delete(&self.state_file_path).await;
                    return SessionOutcome::Failed {
                        story_key: story_info.story_key.clone(),
                        error: format!("Recovery preamble build failed: {e}"),
                        decisions: decision_log.records(),
                    };
                }
            }
        };

        let agent = match self
            .build_agent_for_role(
                LlmRole::Dev,
                &story_info,
                escalation_slot.clone(),
                decision_log.clone(),
                &dev_preamble,
            )
            .await
        {
            Ok(a) => a,
            Err(e) => {
                tracing::error!(
                    action = "crash_recovery_agent_failed",
                    error = %e,
                    "Agent build failed during recovery"
                );
                let _ = SessionState::delete(&self.state_file_path).await;
                return SessionOutcome::Failed {
                    story_key: story_info.story_key.clone(),
                    error: format!("Recovery agent build failed: {e}"),
                    decisions: decision_log.records(),
                };
            }
        };

        // Phase 3 — Activate agent properly before resuming work.
        // The WAL only contains text-only messages — no persona, no tool calls,
        // no project context. Passing raw WAL to run_session() would give the
        // fresh LLM zero activation context. Instead, activate a fresh agent
        // and pass the raw WAL history so the dev agent (with tools) can read
        // the story file and understand where to resume.
        //
        // A WAL with ≤2 messages contains only the activation exchange
        // (user=dev.md, assistant=greeting). No real work was done — the agent
        // never received DS. Treat as fresh start.
        let mut agent = agent;
        let outcome = if state.chat_history.len() <= 2 {
            tracing::info!(
                action = "crash_recovery_shallow_wal",
                history_len = %state.chat_history.len(),
                "Activation-only WAL (≤2 messages) — delegating to fresh run_session(None)"
            );
            self.run_session(
                &mut agent,
                &story_info,
                &provider_name,
                &model_name,
                &base_branch,
                escalation_slot.clone(),
                decision_log.clone(),
                None,
                &mut [],
                &effective_skill_path,
                &dev_preamble,
                LlmRole::Dev,
                &state.pipeline_phase,
            )
            .await
        } else {
            // Non-empty WAL with real work — pass raw history directly to the
            // dev agent instead of summarizing via a bare LLM (which produces
            // low-quality summaries without tool access). The dev agent has
            // tools (read_file, grep, etc.) to verify actual progress by
            // reading the story file checkboxes and inspecting the codebase.
            let formatted_history = Self::format_exchanges_for_message(&state.chat_history);

            tracing::info!(
                action = "crash_recovery_raw_history",
                history_len = %state.chat_history.len(),
                formatted_len = %formatted_history.len(),
                "Using raw WAL history for crash recovery (no LLM summarize)"
            );

            let recovery_message =
                self.build_crash_recovery_message(&story_info, &formatted_history);

            match self
                .drive_activation_and_recover(
                    &mut agent,
                    &state,
                    &story_info,
                    &provider_name,
                    &model_name,
                    &base_branch,
                    escalation_slot.clone(),
                    decision_log.clone(),
                    &recovery_message,
                    0, // recovery_depth: first attempt (not a recursive context-limit recovery)
                    &effective_skill_path,
                    &dev_preamble,
                    LlmRole::Dev,
                )
                .await
            {
                Ok(outcome) => outcome,
                Err(outcome) => outcome,
            }
        };

        // Phase 4 — Delete WAL after recovery, UNLESS this was a graceful shutdown.
        // On Ctrl+C the session saves the WAL so the daemon can resume on next start.
        // Deleting it here would leave the story stuck in `in-progress` with no WAL
        // to trigger crash recovery — the watcher only picks up `ready-for-dev`.
        let is_shutdown = self.shutdown.load(Ordering::Relaxed);
        if is_shutdown {
            tracing::info!(
                action = "crash_recovery_wal_preserved",
                "Shutdown requested — preserving WAL for next restart"
            );
        } else {
            let _ = SessionState::delete(&self.state_file_path).await;
        }

        outcome
    }

    /// Run a full development session for the given story.
    ///
    /// Delegates to [`run_with_consultations()`](Self::run_with_consultations)
    /// with an empty consultation list — zero behavior change.
    pub async fn run(
        &self,
        story: &StoryInfo,
        base_branch_override: Option<&str>,
    ) -> SessionOutcome {
        self.run_with_consultations(
            story,
            base_branch_override,
            vec![],
            None,
            None,
            LlmRole::Dev,
            PHASE_DEV,
        )
        .await
    }

    /// Run a development session with optional daemon-orchestrated consultations.
    ///
    /// Opens a `tracing::info_span!("story_session")` and drives the entire
    /// session lifecycle: build agent → create WAL → chat loop → cleanup.
    /// When `consultations` is non-empty, trigger patterns are checked after each
    /// agent response and matching consultations are executed inline.
    ///
    /// Returns a [`SessionOutcome`] indicating success, escalation, or failure.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_with_consultations(
        &self,
        story: &StoryInfo,
        base_branch_override: Option<&str>,
        consultations: Vec<ConsultationConfig>,
        skill_path_override: Option<&str>,
        preamble_override: Option<String>,
        role: LlmRole,
        initial_phase: &str,
    ) -> SessionOutcome {
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

        let mut consultation_states = ConsultationState::from_configs(consultations);

        let repo_path = PathBuf::from(&self.config.bmad_paths.project_root);
        let branch_name = story.branch_name.clone();
        let default_branch = self.config.git_provider.target_branch.clone();
        let base_branch = if let Some(override_branch) = base_branch_override {
            override_branch.to_string()
        } else {
            determine_base_branch(story, &repo_path, &default_branch)
        };

        // Create shared resources for supervisor
        let escalation_slot: EscalationSlot = Arc::new(std::sync::Mutex::new(None));
        let decision_log = DecisionLog::new();

        let role_config = match role {
            LlmRole::Dev => &self.config.llm.dev,
            LlmRole::Review => &self.config.llm.review,
            LlmRole::Supervisor => &self.config.llm.supervisor,
            LlmRole::EpicReview => {
                if self.config.llm.epic_review.provider.is_empty() {
                    &self.config.llm.review
                } else {
                    &self.config.llm.epic_review
                }
            }
            LlmRole::Critic => {
                if self.config.llm.critic.provider.is_empty() {
                    &self.config.llm.review
                } else {
                    &self.config.llm.critic
                }
            }
            LlmRole::Utility => {
                if self.config.llm.utility.provider.is_empty() {
                    &self.config.llm.review
                } else {
                    &self.config.llm.utility
                }
            }
        };
        let provider = &role_config.provider;
        let model = &role_config.model;

        // Resolve effective skill path and preamble — create-story phase
        // passes overrides; dev sessions get defaults from SessionRunner.
        let effective_skill_path = skill_path_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.skill_path.clone());
        let effective_preamble = match preamble_override {
            Some(p) => p,
            None => match self.build_preamble(story).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(
                        action = "preamble_build_failed",
                        error = %e,
                        "Failed to build agent preamble"
                    );
                    return SessionOutcome::Failed {
                        story_key: story.story_key.clone(),
                        error: format!("Preamble build failed: {e}"),
                        decisions: vec![],
                    };
                }
            },
        };

        // Build agent via AgentFactory — single call replaces 3-arm provider match
        let agent = match self
            .build_agent_for_role(
                role,
                story,
                escalation_slot.clone(),
                decision_log.clone(),
                &effective_preamble,
            )
            .await
        {
            Ok(a) => a,
            Err(e) => {
                tracing::error!(
                    action = "session_failed",
                    error = %e,
                    "Agent build failed"
                );
                return SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Agent build failed: {e}"),
                    decisions: decision_log.records(),
                };
            }
        };

        let mut agent = agent;
        let outcome = self
            .run_session(
                &mut agent,
                story,
                provider,
                model,
                &base_branch,
                escalation_slot.clone(),
                decision_log.clone(),
                None,
                &mut consultation_states,
                &effective_skill_path,
                &effective_preamble,
                role,
                initial_phase,
            )
            .await;

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

    /// Build a [`BuiltAgent`] for a given role using the [`AgentFactory`].
    ///
    /// Creates the standard 9-tool set (7 custom + AskSupervisor + ThinkTool)
    /// and delegates to [`AgentFactory::build()`] for provider-specific client
    /// construction.
    async fn build_agent_for_role(
        &self,
        role: LlmRole,
        _story: &StoryInfo,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
        preamble: &str,
    ) -> Result<BuiltAgent, ProviderError> {
        let project_root = PathBuf::from(&self.config.bmad_paths.project_root);
        let (git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor) =
            agent::create_tools_with_supervisor(
                &project_root,
                &self.config,
                &self.agent_factory,
                escalation_slot,
                decision_log,
                &self.mcp_manager,
                Arc::clone(&self.shutdown),
                self.ui.clone(),
            )
            .map_err(|e| ProviderError::ClientCreation {
                provider: "supervisor".to_string(),
                reason: format!("Failed to create AskSupervisor: {e}"),
            })?;

        let spawn_agent = agent::create_spawn_agent_tool(
            &self.agent_factory,
            LlmRole::Dev,
            &project_root,
            &self.sub_agent_sessions,
            &self.sub_agent_in_flight,
            Some(&self.shutdown),
        );

        let mcp_data = self.mcp_manager.tools_for_builder().await;
        self.agent_factory
            .build(
                role,
                preamble,
                crate::configure_agent_tools!(
                    git,
                    read_file,
                    edit_file,
                    grep,
                    find_path,
                    list_dir,
                    terminal,
                    supervisor,
                    spawn_agent,
                    ThinkTool
                )
                .with_mcp(mcp_data),
            )
            .await
    }

    /// Build the agent system prompt with operational instructions.
    ///
    /// This mirrors Zed's `system_prompt.hbs` pattern: the system prompt contains
    /// operational instructions (tool usage, formatting rules, communication style)
    /// while the agent persona (`dev.md`) is sent as a user message wrapped in
    /// XML context tags via [`agent::activate_agent()`].
    ///
    /// The system prompt provides persistent grounding across all turns.
    async fn build_preamble(&self, _story: &StoryInfo) -> Result<String, ProviderError> {
        let mcp_data = self.mcp_manager.tools_for_builder().await;
        let mcp_names = crate::mcp::extract_mcp_tool_names(&mcp_data);
        Ok(agent::build_preamble(
            &mcp_names,
            &self.config.llm.dev.model,
        ))
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
    /// Build recovery message for crash recovery — uses raw WAL history, no LLM summary.
    ///
    /// The dev agent receives the full conversation history and is instructed to
    /// read the story file to determine actual progress (checkboxes, file changes).
    fn build_crash_recovery_message(&self, story: &StoryInfo, formatted_history: &str) -> String {
        format!(
            "IMPORTANT: ALL communication MUST be in English regardless of config file settings.\n\
             \n\
             === SESSION RECOVERY — Crash Recovery ===\n\
             Your previous session was interrupted (crash recovery). Below is the raw conversation \
             history from before the crash.\n\
             \n\
             {formatted_history}\n\
             \n\
             === Current Story ===\n\
             The story file is at: {path}\n\
             Read this file NOW to see current task checkboxes and progress.\n\
             Then continue working directly on the current task. Do NOT restart the workflow from the beginning.",
            path = story.specs_path.display(),
        )
    }

    /// Build recovery message for context window limit recovery.
    ///
    /// Passes the last N verbatim exchanges so the new dev agent has immediate
    /// conversational context from just before the limit was hit. The agent is
    /// instructed to read the story file checkboxes to determine overall progress.
    fn build_context_limit_recovery_message(
        &self,
        story: &StoryInfo,
        formatted_exchanges: &str,
    ) -> String {
        format!(
            "IMPORTANT: ALL communication MUST be in English regardless of config file settings.\n\
             BRANCH REMINDER: You are already on branch `{branch}`. Do NOT create, checkout, or switch branches — the daemon manages branch lifecycle. Just commit your work on the current branch.\n\
             \n\
             === SESSION RECOVERY — Context Window Limit Reached ===\n\
             Your previous session hit the context window limit and was restarted with a fresh context.\n\
             Below are the most recent exchanges from the previous session.\n\
             \n\
             === Recent Exchanges (verbatim) ===\n\
             {formatted_exchanges}\n\
             \n\
             === Current Story ===\n\
             The story file is at: {path}\n\
             Read this file NOW to verify task checkboxes and actual progress.\n\
             Then continue working directly on the next incomplete task. Do NOT restart from the beginning.",
            branch = story.branch_name,
            path = story.specs_path.display(),
        )
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
        skill_path: &str,
        preamble: &str,
        role: LlmRole,
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

        // Step 1 — Extract last N exchanges verbatim (immediate context for the new agent).
        // The dev agent reads the story file checkboxes to determine overall progress.
        let last_exchanges =
            Self::extract_last_exchanges(&state.chat_history, RECOVERY_KEEP_LAST_EXCHANGES);
        let formatted_exchanges = Self::format_exchanges_for_message(&last_exchanges);

        tracing::info!(
            action = "context_limit_recovery_start",
            original_len = %state.chat_history.len(),
            kept_exchanges = %last_exchanges.len() / 2,
            "Context limit recovery: last exchanges ready"
        );

        // Step 2 — Build fresh agent via AgentFactory (single call, no provider match)
        let mut agent = match self
            .build_agent_for_role(
                role,
                story,
                escalation_slot.clone(),
                decision_log.clone(),
                preamble,
            )
            .await
        {
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

        let recovery_message =
            self.build_context_limit_recovery_message(story, &formatted_exchanges);

        // Step 4-7 — Drive activation and delegate to run_session()
        match self
            .drive_activation_and_recover(
                &mut agent,
                state,
                story,
                provider,
                model,
                base_branch,
                escalation_slot,
                decision_log,
                &recovery_message,
                recovery_depth,
                skill_path,
                preamble,
                role,
            )
            .await
        {
            Ok(outcome) => outcome,
            Err(outcome) => outcome,
        }
    }

    /// Drive the BMAD activation flow on a fresh agent, build compressed state,
    /// and delegate to `run_session()` for the recovered chat loop.
    ///
    /// This is factored out to avoid duplicating the activation logic across
    /// provider match arms. Accepts `&BuiltAgent` — no generics needed.
    ///
    /// Returns `Ok(SessionOutcome)` on success, `Err(SessionOutcome)` on failure.
    #[allow(clippy::too_many_arguments)]
    async fn drive_activation_and_recover(
        &self,
        agent: &mut BuiltAgent,
        original_state: &SessionState,
        story: &StoryInfo,
        provider: &str,
        model: &str,
        base_branch: &str,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
        recovery_message: &str,
        recovery_depth: usize,
        skill_path: &str,
        preamble: &str,
        role: LlmRole,
    ) -> Result<SessionOutcome, SessionOutcome> {
        // Step 4a — Activate agent: send SKILL.md as user message (Story 12.1)
        self.ui.activation_start();
        self.ui.llm_request("recovery", 0);
        self.ui
            .llm_request_content("recovery", 0, "[activate skill]");
        let (mut activation_history, mut compressed_history) = agent
            .activate_agent(
                &self.config.bmad_paths.project_root,
                skill_path,
                "recovery",
                Some(&self.shutdown),
                Some(&self.ui),
            )
            .await
            .map_err(|e| {
                tracing::error!(action = "context_limit_activation_failed", error = %e);
                self.ui.phase_error("Agent Activation", &e);
                SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Recovery activation failed: {e}"),
                    decisions: decision_log.records(),
                }
            })?;
        // Show activation response (agent menu)
        if let Some(last_assistant) = compressed_history
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
        {
            self.ui
                .llm_response("recovery", 0, last_assistant.content.len());
            self.ui
                .llm_response_content("recovery", 0, &last_assistant.content);
        }
        self.ui.activation_complete();

        // Step 4b — Send recovery continuation message with story file path.
        // Story 12.1: skill-based activation replaces the persona menu + CH command.
        // The skill is self-directing; this message provides English override and story context.
        let ch_msg = format!(
            "IMPORTANT: ALL communication MUST be in English regardless of config file settings. Continue recovery for story file: {}",
            story.specs_path.display()
        );

        let ch_turn = compressed_history.len() / 2;
        log_llm_request("recovery", ch_turn, &ch_msg, activation_history.len());
        self.ui.llm_request("recovery", ch_turn as u32);
        self.ui
            .llm_request_content("recovery", ch_turn as u32, &ch_msg);
        let (ch_response, ch_full_history) = agent
            .stream_chat(
                &ch_msg,
                activation_history,
                Some(&self.shutdown),
                Some(&self.ui),
            )
            .await
            .map_err(|e| {
                log_llm_error("recovery", 0, &e);
                self.ui.llm_error("recovery", 0, &e.to_string());
                tracing::error!(action = "context_limit_activation_ch_failed", error = %e);
                SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Recovery activation CH failed: {e}"),
                    decisions: decision_log.records(),
                }
            })?;
        log_llm_response("recovery", ch_turn, &ch_response);
        self.ui
            .llm_response("recovery", ch_turn as u32, ch_response.len());
        self.ui
            .llm_response_content("recovery", ch_turn as u32, &ch_response);
        // Use the full rig history returned by stream_chat — it includes tool calls
        // (think, read_file, etc.) that the agent made during this turn. Manually
        // pushing Message::user/assistant would lose those intermediate messages and
        // cause the agent to forget its activation state on the next turn.
        activation_history = ch_full_history;
        compressed_history.push(ChatMessage {
            role: "user".to_string(),
            content: ch_msg,
        });
        compressed_history.push(ChatMessage {
            role: "assistant".to_string(),
            content: ch_response,
        });

        // Step 4c — Load project context (existing flow unchanged)
        let ctx_turn = compressed_history.len() / 2;
        log_llm_request(
            "recovery",
            ctx_turn,
            "Load the project context",
            activation_history.len(),
        );
        self.ui.llm_request("recovery", ctx_turn as u32);
        self.ui
            .llm_request_content("recovery", ctx_turn as u32, "Load the project context");
        let (ctx_response, ctx_full_history) = agent
            .stream_chat(
                "Load the project context",
                activation_history,
                Some(&self.shutdown),
                Some(&self.ui),
            )
            .await
            .map_err(|e| {
                log_llm_error("recovery", ctx_turn, &e);
                self.ui
                    .llm_error("recovery", ctx_turn as u32, &e.to_string());
                tracing::error!(action = "context_limit_activation_ctx_failed", error = %e);
                SessionOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: format!("Recovery activation load context failed: {e}"),
                    decisions: decision_log.records(),
                }
            })?;
        log_llm_response("recovery", ctx_turn, &ctx_response);
        self.ui
            .llm_response("recovery", ctx_turn as u32, ctx_response.len());
        self.ui
            .llm_response_content("recovery", ctx_turn as u32, &ctx_response);
        // Same as step 4b — use full rig history to preserve tool call context.
        // Not read after this point, but kept for symmetry / future use.
        let _ = ctx_full_history;
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
            skill_path: original_state.skill_path.clone(),
            pipeline_phase: original_state.pipeline_phase.clone(),
            runtime_type: original_state.runtime_type.clone(),
            sdk_session_ids: original_state.sdk_session_ids.clone(),
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
            &mut [],
            skill_path,
            preamble,
            role,
            &original_state.pipeline_phase,
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
    async fn run_session(
        &self,
        agent: &mut BuiltAgent,
        story: &StoryInfo,
        provider: &str,
        model: &str,
        base_branch: &str,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
        recovered_state: Option<SessionState>,
        consultation_states: &mut [ConsultationState],
        skill_path: &str,
        preamble: &str,
        role: LlmRole,
        initial_phase: &str,
    ) -> SessionOutcome {
        let mut retries: usize = 0;
        const MAX_RETRIES: usize = 3;

        // --- Initialization: normal vs recovery path ---
        // full_history tracks the complete conversation including tool calls.
        // During normal operation it is populated from stream_chat hook captures.
        // During crash recovery it falls back to text-only WAL reconstruction.
        let (mut state, mut current_response, mut turn, mut full_history) = match recovered_state {
            None => {
                // Normal path — create new WAL, send "DS"
                let mut state = SessionState::new(story, provider, model);
                state.set_branch_info(&story.branch_name, base_branch);
                state.skill_path = skill_path.to_string();
                state.set_pipeline_phase(initial_phase);

                if let Err(e) = state.save(&self.state_file_path).await {
                    tracing::error!(action = "wal_write_failed", error = %e, "Failed to create initial WAL");
                    return SessionOutcome::Failed {
                        story_key: story.story_key.clone(),
                        error: format!("WAL creation failed: {e}"),
                        decisions: decision_log.records(),
                    };
                }

                // Activate agent: send SKILL.md as user message (Story 12.1).
                // The skill is self-contained and self-directing — no menu command needed.
                // Retries transient errors (503, 429, timeouts) with exponential backoff.
                let mut activation_retries = 0usize;
                self.ui.activation_start();
                self.ui.llm_request("dev", 0);
                self.ui.llm_request_content("dev", 0, "[activate skill]");
                let (activation_rig_history, activation_chat_history) = loop {
                    match agent
                        .activate_agent(
                            &self.config.bmad_paths.project_root,
                            skill_path,
                            "dev",
                            Some(&self.shutdown),
                            Some(&self.ui),
                        )
                        .await
                    {
                        Ok(pair) => break pair,
                        Err(e) => {
                            if is_transient_llm_error(&e)
                                && activation_retries < ACTIVATION_MAX_RETRIES
                            {
                                activation_retries += 1;
                                let delay =
                                    ACTIVATION_BACKOFF_BASE_SECS * (1 << (activation_retries - 1));
                                self.ui.llm_retry(
                                    "dev",
                                    0,
                                    activation_retries as u32,
                                    delay as f64,
                                );
                                tracing::warn!(
                                    action = "activation_transient_retry",
                                    retry = %activation_retries,
                                    max_retries = %ACTIVATION_MAX_RETRIES,
                                    delay_secs = %delay,
                                    error = %e,
                                    "Agent activation hit transient error — retrying after {delay}s"
                                );
                                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                                continue;
                            }
                            tracing::error!(action = "activation_failed", error = %e, "Agent activation failed");
                            self.ui.phase_error("Agent Activation", &e);
                            let _ = self.handle_failure(story).await;
                            return SessionOutcome::Failed {
                                story_key: story.story_key.clone(),
                                error: format!("Agent activation failed: {e}"),
                                decisions: decision_log.records(),
                            };
                        }
                    }
                };

                // Show activation response (agent menu)
                if let Some(last_assistant) = activation_chat_history
                    .iter()
                    .rev()
                    .find(|m| m.role == "assistant")
                {
                    self.ui.llm_response("dev", 0, last_assistant.content.len());
                    self.ui
                        .llm_response_content("dev", 0, &last_assistant.content);
                }
                self.ui.activation_complete();

                // Persist activation history in WAL
                for msg in &activation_chat_history {
                    if msg.role == "user" {
                        state.add_user_message(&msg.content);
                    } else {
                        state.add_assistant_message(&msg.content);
                    }
                }
                let _ = state.save(&self.state_file_path).await;

                // Now send the initial story message — the skill is activated and self-directing.
                // Story 12.1: no [DS] command needed; skill executes autonomously.
                // IMPORTANT: Override language to English. The BMAD activation loads
                // config.yaml which may set communication_language to a non-English
                // language, causing the agent to respond in that language. The response
                // analyzer only matches English patterns, so we must enforce English here.
                let initial_message = if skill_path.contains("bmad-create-story") {
                    format!(
                        "IMPORTANT: ALL communication MUST be in English regardless of config file settings.\n\
                         BRANCH REMINDER: You are already on branch `{}`. Do NOT create, checkout, or switch branches — the daemon manages branch lifecycle. Just commit your work on the current branch.\n\
                         Create story: {}",
                        story.branch_name, story.story_key
                    )
                } else if skill_path.contains("bmad-code-review") {
                    format!(
                        "IMPORTANT: ALL communication MUST be in English regardless of config file settings.\n\
                         BRANCH REMINDER: You are already on branch `{}`. Do NOT create, checkout, or switch branches — the daemon manages branch lifecycle. Just commit your work on the current branch.\n\
                         AUTONOMOUS CODE REVIEW: Review the changes on this branch.\n\
                         Diff source: branch diff against `{}`\n\
                         Story file: {}\n\
                         AUTONOMOUS MODE RULES:\n\
                         - Do NOT wait for human input at any HALT or checkpoint — proceed automatically.\n\
                         - For checkpoints: confirm and proceed without waiting.\n\
                         - For patch findings: auto-apply all fixes (batch-apply).\n\
                         - For findings tagged [Review][Decision]: present them clearly with your analysis, then HALT. An external reviewer will provide decisions.\n\
                         - For defer findings: leave as action items.\n\
                         - After all findings are resolved, commit all review fixes and signal completion.",
                        story.branch_name,
                        self.config.git_provider.target_branch,
                        story.specs_path.display()
                    )
                } else {
                    format!(
                        "IMPORTANT: ALL communication MUST be in English regardless of config file settings.\n\
                         BRANCH REMINDER: You are already on branch `{}`. Do NOT create, checkout, or switch branches — the daemon manages branch lifecycle. Just commit your work on the current branch.\n\
                         Story file: {}",
                        story.branch_name,
                        story.specs_path.display()
                    )
                };
                state.add_user_message(&initial_message);

                let activation_turn = activation_chat_history.len() / 2;

                // Send "DS" with retry on transient errors (503, 429, timeouts).
                let mut ds_retries = 0usize;
                let (response, ds_full_history) = loop {
                    log_llm_request(
                        "dev",
                        activation_turn,
                        &initial_message,
                        activation_rig_history.len(),
                    );
                    self.ui.llm_request("dev", activation_turn as u32);
                    self.ui
                        .llm_request_content("dev", activation_turn as u32, &initial_message);
                    match agent
                        .stream_chat(
                            &initial_message,
                            activation_rig_history.clone(),
                            Some(&self.shutdown),
                            Some(&self.ui),
                        )
                        .await
                    {
                        Ok((r, hist)) => {
                            log_llm_response("dev", 0, &r);
                            self.ui.llm_response("dev", 0, r.len());
                            self.ui.llm_response_content("dev", 0, &r);
                            break (r, hist);
                        }
                        Err(e) => {
                            log_llm_error("dev", 0, &e);
                            self.ui.llm_error("dev", 0, &e.to_string());
                            let error_str = e.to_string();

                            // Context limit during initial DS send — the activation history
                            // is already too large. Trigger recovery immediately; retrying
                            // with the same history would produce the same 400 error.
                            if is_context_limit_error(&error_str) {
                                tracing::warn!(
                                    action = "context_limit_detected",
                                    turn = "initial_ds",
                                    error = %error_str,
                                    "Context window limit hit on initial DS send — initiating recovery"
                                );
                                // Pop the DS user message we added to state before sending
                                state.chat_history.pop();
                                let outcome = self
                                    .context_limit_recovery(
                                        &state,
                                        story,
                                        provider,
                                        model,
                                        base_branch,
                                        escalation_slot.clone(),
                                        decision_log.clone(),
                                        0,
                                        skill_path,
                                        preamble,
                                        role,
                                    )
                                    .await;
                                self.write_decisions(story, &decision_log).await;
                                return outcome;
                            }

                            if is_transient_llm_error(&error_str)
                                && ds_retries < ACTIVATION_MAX_RETRIES
                            {
                                ds_retries += 1;
                                let delay = ACTIVATION_BACKOFF_BASE_SECS * (1 << (ds_retries - 1));
                                self.ui.llm_retry("dev", 0, ds_retries as u32, delay as f64);
                                tracing::warn!(
                                    action = "initial_chat_transient_retry",
                                    retry = %ds_retries,
                                    max_retries = %ACTIVATION_MAX_RETRIES,
                                    delay_secs = %delay,
                                    error = %error_str,
                                    "Initial DS chat hit transient error — retrying after {delay}s"
                                );
                                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                                continue;
                            }
                            tracing::error!(action = "chat_failed", turn = 0, error = %error_str, "Initial chat failed");
                            let _ = self.handle_failure(story).await;
                            return SessionOutcome::Failed {
                                story_key: story.story_key.clone(),
                                error: format!("Initial chat failed: {error_str}"),
                                decisions: decision_log.records(),
                            };
                        }
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

                (state, response, 1usize, ds_full_history)
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

                    // Activate agent: send SKILL.md as user message (Story 12.1)
                    self.ui.activation_start();
                    self.ui.llm_request("recovery", 0);
                    self.ui
                        .llm_request_content("recovery", 0, "[activate skill]");
                    let (activation_rig_history, activation_chat_history) = match agent
                        .activate_agent(
                            &self.config.bmad_paths.project_root,
                            skill_path,
                            "recovery",
                            Some(&self.shutdown),
                            Some(&self.ui),
                        )
                        .await
                    {
                        Ok(pair) => {
                            // Show activation response (agent menu)
                            if let Some(last_assistant) =
                                pair.1.iter().rev().find(|m| m.role == "assistant")
                            {
                                self.ui
                                    .llm_response("recovery", 0, last_assistant.content.len());
                                self.ui.llm_response_content(
                                    "recovery",
                                    0,
                                    &last_assistant.content,
                                );
                            }
                            self.ui.activation_complete();
                            pair
                        }
                        Err(e) => {
                            self.ui.phase_error("Agent Activation", &e);
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

                    // Now send the initial story message — skill is activated and self-directing.
                    // Story 12.1: no [DS] command needed.
                    // Override language to English (see normal path comment for rationale).
                    let initial_message = format!(
                        "IMPORTANT: ALL communication MUST be in English regardless of config file settings. Story file: {}",
                        story.specs_path.display()
                    );
                    state.add_user_message(&initial_message);

                    let activation_turn = activation_chat_history.len() / 2;
                    log_llm_request(
                        "recovery",
                        activation_turn,
                        &initial_message,
                        activation_rig_history.len(),
                    );
                    self.ui.llm_request("recovery", activation_turn as u32);
                    self.ui.llm_request_content(
                        "recovery",
                        activation_turn as u32,
                        &initial_message,
                    );
                    let (response, ds_full_history) = match agent
                        .stream_chat(
                            &initial_message,
                            activation_rig_history,
                            Some(&self.shutdown),
                            Some(&self.ui),
                        )
                        .await
                    {
                        Ok((r, hist)) => {
                            log_llm_response("recovery", 0, &r);
                            self.ui.llm_response("recovery", 0, r.len());
                            self.ui.llm_response_content("recovery", 0, &r);
                            (r, hist)
                        }
                        Err(e) => {
                            log_llm_error("recovery", 0, &e);
                            self.ui.llm_error("recovery", 0, &e.to_string());
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

                    (state, response, turn_offset + 1, ds_full_history)
                } else {
                    let last_msg = state.chat_history.last().expect("non-empty history");

                    if last_msg.role == "assistant" {
                        // Sub-case A — Last message is assistant (normal recovery)
                        let response = last_msg.content.clone();
                        tracing::info!(
                            action = "crash_recovery_last_assistant",
                            "Last message is assistant — entering analyze loop"
                        );
                        // Recovery: bootstrap full_history from text-only WAL (degraded)
                        let recovery_full_history = state.to_rig_messages();
                        (state, response, turn_offset, recovery_full_history)
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

                        log_llm_request("recovery", turn_offset, &last_user_msg, history.len());
                        self.ui.llm_request("recovery", turn_offset as u32);
                        self.ui
                            .llm_request_content("recovery", turn_offset as u32, &last_user_msg);
                        log_llm_history(
                            "recovery",
                            turn_offset,
                            &state.chat_history[..state.chat_history.len() - 1],
                        );
                        let (response, resend_full_history) = match agent
                            .stream_chat(
                                last_user_msg.as_str(),
                                history,
                                Some(&self.shutdown),
                                Some(&self.ui),
                            )
                            .await
                        {
                            Ok((r, hist)) => {
                                log_llm_response("recovery", turn_offset, &r);
                                self.ui
                                    .llm_response("recovery", turn_offset as u32, r.len());
                                self.ui
                                    .llm_response_content("recovery", turn_offset as u32, &r);
                                (r, hist)
                            }
                            Err(e) => {
                                log_llm_error("recovery", turn_offset, &e);
                                self.ui
                                    .llm_error("recovery", turn_offset as u32, &e.to_string());
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

                        (state, response, turn_offset, resend_full_history)
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
            let action = self.analyzer.analyze(&current_response, &escalation_slot);

            // Check consultation triggers even when the sentinel is detected —
            // the LLM may combine the completion report and <<BMAD_JOB_DONE>>
            // in a single turn. If a trigger matches, run the consultation
            // first; the agent will re-emit the sentinel after handling findings.
            let action = if matches!(action, ResponseAction::Completed)
                && !consultation_states.is_empty()
            {
                match self
                    .check_consultation_triggers(&current_response, consultation_states, &mut state)
                    .await
                {
                    Some(override_action) => override_action,
                    None => action,
                }
            } else {
                action
            };

            match action {
                ResponseAction::Completed => {
                    self.ui.completion_detected(&story.story_key);
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
                        pr_context: None,
                        pr_how_to_test: None,
                        pr_additional_info: None,
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
                    let reply = match &action {
                        ResponseAction::NoReply => {
                            match self
                                .check_consultation_triggers(
                                    &current_response,
                                    consultation_states,
                                    &mut state,
                                )
                                .await
                            {
                                Some(ResponseAction::Continue { reply }) => reply,
                                _ => "Continue.".to_string(),
                            }
                        }
                        ResponseAction::Continue { reply } => reply.clone(),
                        _ => unreachable!(),
                    };
                    state.add_user_message(&reply);

                    log_llm_request("dev", turn, &reply, full_history.len());
                    self.ui.llm_request("dev", turn as u32);
                    self.ui.llm_request_content("dev", turn as u32, &reply);
                    log_llm_history_summary("dev", turn, &state.chat_history);
                    match agent
                        .stream_chat(
                            reply.as_str(),
                            full_history.clone(),
                            Some(&self.shutdown),
                            Some(&self.ui),
                        )
                        .await
                    {
                        Ok((r, new_hist)) => {
                            log_llm_response("dev", turn, &r);
                            self.ui.llm_response("dev", turn as u32, r.len());
                            self.ui.llm_response_content("dev", turn as u32, &r);
                            retries = 0;
                            full_history = new_hist;
                            state.add_assistant_message(&r);
                            let _ = state.save(&self.state_file_path).await;
                            current_response = r;
                        }
                        Err(e) => {
                            log_llm_error("dev", turn, &e);
                            self.ui.llm_error("dev", turn as u32, &e.to_string());
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
                                        skill_path,
                                        preamble,
                                        role,
                                    )
                                    .await;

                                // Write decisions regardless of outcome
                                self.write_decisions(story, &decision_log).await;
                                return outcome;
                            }

                            // Non-context-limit error — retry with exponential backoff
                            retries += 1;
                            let backoff_secs =
                                CHAT_LOOP_BACKOFF_BASE_SECS * (1 << (retries - 1).min(5));
                            self.ui.llm_retry(
                                "dev",
                                turn as u32,
                                retries as u32,
                                backoff_secs as f64,
                            );
                            tracing::warn!(
                                action = "chat_error",
                                turn = %turn,
                                retries = %retries,
                                max_retries = %MAX_RETRIES,
                                backoff_secs = %backoff_secs,
                                transient = %is_transient_llm_error(&error_str),
                                error = %error_str,
                                "Chat error — retrying after {backoff_secs}s"
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
                            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                            // Retry with the same response (don't add to history)
                            // Remove the user message we just added
                            state.chat_history.pop();
                            continue;
                        }
                    }
                }
            }

            // Emit chat turn UI event (AC #3) — turn 0 is the DS response
            // emitted before the loop, so chat_turn inside the loop starts at 1+.
            let summary = truncate_summary(&current_response, 80);
            self.ui.chat_turn(turn as u32, &summary);
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

    /// Check consultation triggers and update WAL phase before executing.
    async fn check_consultation_triggers(
        &self,
        response: &str,
        consultation_states: &mut [ConsultationState],
        session_state: &mut SessionState,
    ) -> Option<ResponseAction> {
        for state in consultation_states.iter_mut() {
            if !state.can_trigger() {
                continue;
            }
            if !state.compiled_regex.is_match(response) {
                continue;
            }

            state.trigger_count += 1;

            if let Some(ref phase) = state.config.pipeline_phase {
                session_state.set_pipeline_phase(phase);
                if let Err(e) = session_state.save(&self.state_file_path).await {
                    tracing::warn!(
                        action = "wal_phase_update_failed",
                        label = %state.config.label,
                        phase = %phase,
                        error = %e,
                        "Failed to persist pipeline phase update — continuing anyway"
                    );
                }
                tracing::debug!(
                    action = "pipeline_phase_updated",
                    label = %state.config.label,
                    phase = %phase,
                    "WAL pipeline phase updated before consultation"
                );
            }

            tracing::info!(
                action = "consultation_triggered",
                label = %state.config.label,
                "Consultation trigger matched — executing"
            );

            let detail: Option<String> = if state.config.label == "review-critic" {
                let match_count = state.compiled_regex.find_iter(response).count();
                Some(format!("{match_count} decision-needed findings"))
            } else {
                None
            };
            let consultation_start_time = std::time::Instant::now();
            self.ui.consultation_start(
                &state.config.label,
                &session_state.story_key,
                detail.as_deref(),
            );

            match self.consultation_runner.execute(&state.config).await {
                Ok(findings) => {
                    let consultation_duration = consultation_start_time.elapsed();
                    let findings_count = findings.lines().filter(|l| !l.trim().is_empty()).count();
                    self.ui.consultation_complete(
                        &state.config.label,
                        findings_count,
                        consultation_duration,
                    );
                    if state.config.label.contains("critic") {
                        self.ui.critic_memory_update(&session_state.story_key);
                    }
                    let formatted = state.config.format_findings(&findings);
                    tracing::info!(
                        action = "consultation_complete",
                        label = %state.config.label,
                        findings_len = %findings.len(),
                        "Consultation completed — injecting findings"
                    );
                    return Some(ResponseAction::Continue { reply: formatted });
                }
                Err(e) => {
                    self.ui
                        .consultation_error(&state.config.label, &e.to_string());
                    tracing::warn!(
                        action = "consultation_failed",
                        label = %state.config.label,
                        error = %e,
                        "Consultation failed — continuing without external input"
                    );
                    return Some(ResponseAction::Continue {
                        reply: format!(
                            "Consultation '{}' failed: {}. Continue without external input.",
                            state.config.label, e
                        ),
                    });
                }
            }
        }
        None
    }

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
    use std::sync::atomic::AtomicBool;

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
                    reasoning_effort: None,
                    base_url: None,
                    cli_path: None,
                },
                review: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test-model".to_string(),
                    reasoning_effort: None,
                    base_url: None,
                    cli_path: None,
                },
                supervisor: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "test-model".to_string(),
                    reasoning_effort: None,
                    base_url: None,
                    cli_path: None,
                },
                epic_review: LlmRoleConfig::default(),
                critic: LlmRoleConfig::default(),
                utility: LlmRoleConfig::default(),
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
            ui_mode: "fancy".to_string(),
            ui_verbosity: "normal".to_string(),
            code_review_enabled: true,
            project_brief: None,
            critic_memory_threshold_kb: None,
            mcp_servers: vec![],
        }
    }

    /// Helper: create minimal BotSecrets for tests.
    fn make_test_secrets() -> BotSecrets {
        BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: Some("sk-test".to_string()),
            github_token: Some("ghp-test".to_string()),
            gitlab_token: None,
            telegram_bot_token: None,
        }
    }

    /// Helper: create an AgentFactory for tests.
    fn make_test_factory(config: Arc<BotConfig>) -> Arc<AgentFactory> {
        let secrets = Arc::new(make_test_secrets());
        Arc::new(AgentFactory::new(config, secrets))
    }

    fn make_test_mcp_manager() -> Arc<crate::mcp::McpManager> {
        Arc::new(crate::mcp::McpManager::empty())
    }

    type SubAgentArcs = (
        Arc<Mutex<HashMap<String, SubAgentState>>>,
        Arc<Mutex<HashSet<String>>>,
    );

    fn make_empty_sub_agent_arcs() -> SubAgentArcs {
        (
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashSet::new())),
        )
    }

    #[test]
    fn test_session_runner_new_sets_state_file_path() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let factory = make_test_factory(Arc::clone(&config));
        let shutdown = Arc::new(AtomicBool::new(false));

        let (subs, inflt) = make_empty_sub_agent_arcs();
        let runner = SessionRunner::new(
            config,
            factory,
            shutdown,
            make_test_mcp_manager(),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        );

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
        let factory = make_test_factory(Arc::clone(&config));
        let shutdown = Arc::new(AtomicBool::new(false));

        let (subs, inflt) = make_empty_sub_agent_arcs();
        let runner = SessionRunner::new(
            config,
            factory,
            shutdown,
            make_test_mcp_manager(),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        );

        let expected = dir.path().join(".bmad-bot-session.yaml");
        assert_eq!(runner.state_file_path, expected);
    }

    #[test]
    fn test_session_runner_stores_mcp_manager() {
        let dir = tempfile::tempdir().unwrap();
        let config = Arc::new(make_runner_test_config(dir.path()));
        let factory = make_test_factory(Arc::clone(&config));
        let mcp = make_test_mcp_manager();
        let (subs, inflt) = make_empty_sub_agent_arcs();
        let runner = SessionRunner::new(
            Arc::clone(&config),
            Arc::clone(&factory),
            Arc::new(AtomicBool::new(false)),
            Arc::clone(&mcp),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        );
        // Verify mcp_manager is stored (Arc strong count increased)
        assert_eq!(Arc::strong_count(&mcp), 2);
        drop(runner);
        assert_eq!(Arc::strong_count(&mcp), 1);
    }

    #[test]
    fn test_session_runner_stores_sub_agent_arcs() {
        let dir = tempfile::tempdir().unwrap();
        let config = Arc::new(make_runner_test_config(dir.path()));
        let factory = make_test_factory(Arc::clone(&config));
        let mcp = make_test_mcp_manager();
        let sessions: Arc<Mutex<HashMap<String, SubAgentState>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let in_flight: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        assert_eq!(Arc::strong_count(&sessions), 1);
        assert_eq!(Arc::strong_count(&in_flight), 1);

        let runner = SessionRunner::new(
            Arc::clone(&config),
            Arc::clone(&factory),
            Arc::new(AtomicBool::new(false)),
            Arc::clone(&mcp),
            Arc::clone(&sessions),
            Arc::clone(&in_flight),
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        );

        // >= 2, not == 2: internal cloning is an implementation detail.
        assert!(Arc::strong_count(&sessions) >= 2);
        assert!(Arc::strong_count(&in_flight) >= 2);
        drop(runner);
        assert_eq!(Arc::strong_count(&sessions), 1);
        assert_eq!(Arc::strong_count(&in_flight), 1);
    }

    #[test]
    fn test_session_runner_state_file_path_unchanged() {
        // Verify that adding branch setup in Story 4.3 did not change
        // the state_file_path derivation from Story 4.2
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let factory = make_test_factory(Arc::clone(&config));
        let shutdown = Arc::new(AtomicBool::new(false));

        let (subs, inflt) = make_empty_sub_agent_arcs();
        let runner = SessionRunner::new(
            config,
            factory,
            shutdown,
            make_test_mcp_manager(),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        );

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
            skill_path: String::new(),
            pipeline_phase: String::new(),
            runtime_type: String::new(),
            sdk_session_ids: std::collections::HashMap::new(),
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
            branch_name: String::new(),
            base_branch: String::new(),
            skill_path: String::new(),
            pipeline_phase: String::new(),
            runtime_type: String::new(),
            sdk_session_ids: std::collections::HashMap::new(),
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
        let factory = make_test_factory(Arc::clone(&config));
        let shutdown = Arc::new(AtomicBool::new(false));

        let (subs, inflt) = make_empty_sub_agent_arcs();
        let runner = SessionRunner::new(
            config,
            factory,
            shutdown,
            make_test_mcp_manager(),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        );

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
        let _secrets = Arc::new(make_test_secrets());

        // Create a valid WAL file
        let state = make_recovery_state();
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        state.save(&wal_path).await.expect("save WAL");

        let shutdown = Arc::new(AtomicBool::new(false));
        let factory = make_test_factory(Arc::clone(&config));
        let (subs, inflt) = make_empty_sub_agent_arcs();
        let runner = SessionRunner::new(
            config,
            factory,
            shutdown,
            make_test_mcp_manager(),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        );

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
        let _secrets = Arc::new(make_test_secrets());

        // Write corrupt YAML
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        tokio::fs::write(&wal_path, "not: [valid: yaml: for: session")
            .await
            .expect("write corrupt WAL");
        assert!(wal_path.exists(), "Corrupt WAL should exist before check");

        let shutdown = Arc::new(AtomicBool::new(false));
        let factory = make_test_factory(Arc::clone(&config));
        let (subs, inflt) = make_empty_sub_agent_arcs();
        let runner = SessionRunner::new(
            config,
            factory,
            shutdown,
            make_test_mcp_manager(),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        );

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
        let _secrets = Arc::new(make_test_secrets());

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
        let factory = make_test_factory(Arc::clone(&config));
        let (subs, inflt) = make_empty_sub_agent_arcs();
        let runner = SessionRunner::new(
            config,
            factory,
            shutdown,
            make_test_mcp_manager(),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        );
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
        let _secrets = Arc::new(make_test_secrets());

        let state = make_legacy_recovery_state();
        let wal_path = dir.path().join(".bmad-bot-session.yaml");
        state.save(&wal_path).await.expect("save WAL");

        let shutdown = Arc::new(AtomicBool::new(false));
        let factory = make_test_factory(Arc::clone(&config));
        let (subs, inflt) = make_empty_sub_agent_arcs();
        let runner = SessionRunner::new(
            config,
            factory,
            shutdown,
            make_test_mcp_manager(),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        );
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
    fn test_is_context_limit_error_copilot_prompt_tokens_exceeded() {
        assert!(is_context_limit_error(
            r#"CompletionError: ResponseError: CompletionError: ProviderError: Invalid status code 400 Bad Request with message: {"error":{"message":"prompt token count of 128177 exceeds the limit of 128000","code":"model_max_prompt_tokens_exceeded"}}"#
        ));
    }

    #[test]
    fn test_is_context_limit_error_exceeds_the_limit() {
        assert!(is_context_limit_error(
            "prompt token count of 128177 exceeds the limit of 128000"
        ));
    }

    #[test]
    fn test_is_context_limit_error_prompt_token_count() {
        assert!(is_context_limit_error("prompt token count exceeded"));
    }

    #[test]
    fn test_is_context_limit_error_prompt_tokens_exceeded_code() {
        assert!(is_context_limit_error("model_max_prompt_tokens_exceeded"));
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

    fn make_test_runner_with_dir(dir: &std::path::Path) -> SessionRunner {
        let config = Arc::new(make_runner_test_config(dir));
        let factory = make_test_factory(Arc::clone(&config));
        let (subs, inflt) = make_empty_sub_agent_arcs();
        SessionRunner::new(
            config,
            factory,
            Arc::new(AtomicBool::new(false)),
            make_test_mcp_manager(),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        )
    }

    fn make_test_runner() -> SessionRunner {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config = Arc::new(make_runner_test_config(dir.path()));
        let factory = make_test_factory(Arc::clone(&config));
        let shutdown = Arc::new(AtomicBool::new(false));
        // Leak the tempdir so it isn't dropped (this is fine in tests)
        std::mem::forget(dir);
        let (subs, inflt) = make_empty_sub_agent_arcs();
        SessionRunner::new(
            config,
            factory,
            shutdown,
            make_test_mcp_manager(),
            subs,
            inflt,
            crate::ui::UiHandle::null(),
            ".claude/skills/bmad-dev-story/SKILL.md".to_string(),
        )
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
    fn test_build_context_limit_recovery_message_contains_all_sections() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_context_limit_recovery_message(&story, "exchange text");
        assert!(
            msg.contains("SESSION RECOVERY"),
            "Should contain SESSION RECOVERY header"
        );
        assert!(
            msg.contains("Context Window Limit Reached"),
            "Should contain the reason in the header"
        );
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
    fn test_build_context_limit_recovery_message_includes_story_path() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_context_limit_recovery_message(&story, "e");
        assert!(
            msg.contains("6-4-context-window-limit-recovery.md"),
            "Should contain the story specs_path"
        );
    }

    #[test]
    fn test_build_context_limit_recovery_message_does_not_contain_project_context() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_context_limit_recovery_message(&story, "exchanges");
        // Project context is loaded by the agent via BMAD activation, NOT injected in message
        assert!(
            !msg.contains("Project Context"),
            "Recovery message should NOT contain a 'Project Context' section"
        );
    }

    #[test]
    fn test_build_crash_recovery_message_contains_continue_instruction() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_crash_recovery_message(&story, "e");
        assert!(
            msg.contains("continue working directly on the current task"),
            "Should instruct agent to continue"
        );
        assert!(
            msg.contains("Do NOT restart the workflow"),
            "Should instruct agent not to restart"
        );
    }

    #[test]
    fn test_build_crash_recovery_message_reason() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_crash_recovery_message(&story, "exchanges");
        assert!(
            msg.contains("=== SESSION RECOVERY — Crash Recovery ==="),
            "Should contain crash recovery reason in header"
        );
        assert!(
            msg.contains("crash recovery"),
            "Should contain lowercase reason in description"
        );
    }

    #[test]
    fn test_build_context_limit_recovery_message_reason() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_context_limit_recovery_message(&story, "exchanges");
        assert!(
            msg.contains("=== SESSION RECOVERY — Context Window Limit Reached ==="),
            "Should contain context limit reason in header"
        );
        assert!(
            msg.contains("context window limit"),
            "Should contain context limit in description"
        );
    }

    #[test]
    fn test_crash_and_context_limit_messages_share_common_sections() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let crash_msg = runner.build_crash_recovery_message(&story, "e");
        let ctx_msg = runner.build_context_limit_recovery_message(&story, "e");
        // Both contain the common sections
        for msg in [&crash_msg, &ctx_msg] {
            assert!(msg.contains("=== Current Story ==="));
            assert!(
                msg.to_lowercase().contains("continue working directly"),
                "Should contain 'continue working directly' (case-insensitive)"
            );
        }
    }

    #[test]
    fn test_build_crash_recovery_message_instructs_read_story_file() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_crash_recovery_message(&story, "e");
        assert!(
            msg.contains("Read this file NOW"),
            "Should instruct agent to read story file immediately"
        );
    }

    // -- compressed state tests --

    #[test]
    fn test_compressed_state_contains_activation_turns_context_limit() {
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

    // -----------------------------------------------------------------------
    // is_transient_llm_error tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_transient_llm_error_503() {
        assert!(is_transient_llm_error(
            "Invalid status code 503 Service Unavailable with message: Sorry, the upstream model provider is currently experiencing high demand."
        ));
    }

    #[test]
    fn test_is_transient_llm_error_429() {
        assert!(is_transient_llm_error("Rate limit exceeded (429)"));
    }

    #[test]
    fn test_is_transient_llm_error_timeout() {
        assert!(is_transient_llm_error("Request timed out after 30s"));
        assert!(is_transient_llm_error("Connection timeout"));
    }

    #[test]
    fn test_is_transient_llm_error_try_again() {
        assert!(is_transient_llm_error("Please try again later"));
    }

    #[test]
    fn test_is_transient_llm_error_false_for_permanent_auth() {
        assert!(!is_transient_llm_error("Bad credentials"));
        assert!(!is_transient_llm_error("Authentication failed"));
    }

    #[test]
    fn test_is_transient_llm_error_token_expired() {
        // Copilot "token expired" is transient — token refresh + retry should fix it
        assert!(is_transient_llm_error(
            "Invalid status code 401 Unauthorized with message: unauthorized: token expired"
        ));
        assert!(is_transient_llm_error("token expired"));
        assert!(is_transient_llm_error("unauthorized: token expired"));
    }

    #[test]
    fn test_is_transient_llm_error_decode_and_sse_errors() {
        assert!(is_transient_llm_error(
            "CompletionError: ResponseError: CompletionError: ProviderError: Http client error: error decoding response body"
        ));
        assert!(is_transient_llm_error(
            "unexpected EOF during chunk size line"
        ));
        assert!(is_transient_llm_error("SSE error: connection dropped"));
        assert!(is_transient_llm_error("Broken pipe"));
        assert!(is_transient_llm_error(
            "connection closed before message completed"
        ));
    }

    #[test]
    fn test_is_transient_llm_error_http_client_error_sending_request() {
        // Real-world Copilot connection timeout pattern
        assert!(is_transient_llm_error(
            "CompletionError: ResponseError: CompletionError: ProviderError: Http client error: error sending request for url (https://api.enterprise.githubcopilot.com/chat/completions)"
        ));
        assert!(is_transient_llm_error(
            "Http client error: error sending request for url"
        ));
    }

    #[test]
    fn test_is_transient_llm_error_false_for_context_limit() {
        assert!(!is_transient_llm_error("context_length_exceeded"));
        assert!(!is_transient_llm_error("prompt is too long"));
    }

    // -----------------------------------------------------------------------
    // parse_pr_summary tests (Task 9)
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_pr_summary_valid() {
        let input = r#"Here is the PR summary:

<pr-summary>
<context>
Implemented the integration test infrastructure including shared test fixtures,
mock providers, and helper utilities.
</context>
<how-to-test>
1. Run `cargo test --test integration` to execute all integration tests
2. Verify fixture files are created in the temp directory
</how-to-test>
<additional-info>
- Added `tempfile` as a dev dependency for test directory management
- Mock LLM provider reuses patterns from unit tests
</additional-info>
</pr-summary>"#;
        let result = parse_pr_summary(input);
        assert!(result.is_some(), "Should parse valid PR summary");
        let (ctx, test, info) = result.unwrap();
        assert!(ctx.contains("integration test infrastructure"));
        assert!(test.contains("cargo test --test integration"));
        assert!(info.contains("tempfile"));
    }

    #[test]
    fn test_parse_pr_summary_missing_sections_uses_lenient_fallback() {
        // With lenient parsing, <pr-summary> with only <context> is valid:
        // context is extracted, how_to_test and additional_info default to empty.
        let input = r#"<pr-summary>
<context>
Only context is present.
</context>
</pr-summary>"#;
        let result = parse_pr_summary(input);
        assert!(result.is_some(), "Should parse with lenient fallback");
        let (ctx, test, info) = result.unwrap();
        assert_eq!(ctx, "Only context is present.");
        assert!(test.is_empty(), "how_to_test should be empty");
        assert!(info.is_empty(), "additional_info should be empty");
    }

    #[test]
    fn test_parse_pr_summary_no_subtags_uses_raw_content() {
        // Agent forgot the sub-tags entirely — raw content becomes context.
        let input = r#"<pr-summary>
Implemented story 7-1. All tests pass. No code changes were made.
</pr-summary>"#;
        let result = parse_pr_summary(input);
        assert!(result.is_some(), "Should parse raw content as context");
        let (ctx, test, info) = result.unwrap();
        assert!(ctx.contains("Implemented story 7-1"));
        assert!(test.is_empty());
        assert!(info.is_empty());
    }

    #[test]
    fn test_parse_pr_summary_no_outer_tag_returns_none() {
        // No <pr-summary> at all → None
        let input = "<context>Some context</context>";
        let result = parse_pr_summary(input);
        assert!(result.is_none(), "Should return None without <pr-summary>");
    }

    #[test]
    fn test_parse_pr_summary_garbage_input() {
        let result = parse_pr_summary("This is just random text with no XML tags at all.");
        assert!(result.is_none(), "Should return None for garbage input");
    }

    #[test]
    fn test_parse_pr_summary_empty_input() {
        let result = parse_pr_summary("");
        assert!(result.is_none(), "Should return None for empty input");
    }

    #[test]
    fn test_parse_pr_summary_special_characters() {
        let input = r#"<pr-summary>
<context>
Added support for `HashMap<String, Vec<u8>>` and angle brackets < > in descriptions.
Also handles **bold** and _italic_ markdown.
</context>
<how-to-test>
1. Run `cargo test -- --test-threads=1`
2. Check that `<script>` tags are handled correctly
</how-to-test>
<additional-info>
Uses `regex::Regex` for parsing. The pattern `<tag>(.*?)</tag>` works with dotall mode.
</additional-info>
</pr-summary>"#;
        let result = parse_pr_summary(input);
        assert!(result.is_some(), "Should handle special characters");
        let (ctx, test, info) = result.unwrap();
        assert!(ctx.contains("HashMap<String, Vec<u8>>"));
        assert!(test.contains("<script>"));
        assert!(info.contains("regex::Regex"));
    }

    #[test]
    fn test_compressed_state_last_message_is_recovery_context_limit() {
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
    fn test_compressed_state_preserves_metadata_context_limit() {
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
            skill_path: String::new(),
            pipeline_phase: String::new(),
            runtime_type: String::new(),
            sdk_session_ids: std::collections::HashMap::new(),
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
            skill_path: original.skill_path.clone(),
            pipeline_phase: original.pipeline_phase.clone(),
            runtime_type: original.runtime_type.clone(),
            sdk_session_ids: original.sdk_session_ids.clone(),
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
    fn test_compressed_state_updates_last_activity_context_limit() {
        let original_activity = "2026-02-07T10:05:00Z";
        let new_activity = chrono::Utc::now().to_rfc3339();

        // The compressed state should have a newer last_activity
        assert_ne!(
            original_activity, &new_activity,
            "last_activity should be refreshed"
        );
    }

    #[test]
    fn test_build_context_limit_recovery_message_instructs_read_story_file() {
        let runner = make_test_runner();
        let story = make_test_story_info();
        let msg = runner.build_context_limit_recovery_message(&story, "e");
        assert!(
            msg.contains("Read this file NOW"),
            "Should instruct agent to read story file immediately"
        );
    }

    #[test]
    fn test_impact_analysis_prompt_contains_story_key() {
        let prompt = build_impact_analysis_prompt(
            "4-6-post-implementation-impact-analysis",
            "_bmad-output/implementation-artifacts",
            "_bmad-output/planning-artifacts",
        );
        assert!(
            prompt.contains("4-6-post-implementation-impact-analysis"),
            "Prompt must contain the story key"
        );
    }

    #[test]
    fn test_impact_analysis_prompt_contains_sprint_status_path() {
        let prompt = build_impact_analysis_prompt(
            "4-6-post-implementation-impact-analysis",
            "_bmad-output/implementation-artifacts",
            "_bmad-output/planning-artifacts",
        );
        assert!(
            prompt.contains("_bmad-output/implementation-artifacts/sprint-status.yaml"),
            "Prompt must reference sprint-status.yaml with full path"
        );
    }

    #[test]
    fn test_impact_analysis_prompt_contains_planning_artifacts_path() {
        let prompt = build_impact_analysis_prompt(
            "4-6-post-implementation-impact-analysis",
            "_bmad-output/implementation-artifacts",
            "_bmad-output/planning-artifacts",
        );
        assert!(
            prompt.contains("_bmad-output/planning-artifacts/architecture.md"),
            "Prompt must reference architecture.md with full planning artifacts path"
        );
    }

    #[test]
    fn test_impact_analysis_prompt_contains_scope_guard() {
        let prompt =
            build_impact_analysis_prompt("1-1-some-story", "/artifacts/impl", "/artifacts/plan");
        assert!(
            prompt.contains("SCOPE GUARD"),
            "Prompt must contain scope guard language"
        );
        assert!(
            prompt.contains("Previous Story Intelligence"),
            "Prompt must reference Previous Story Intelligence sections"
        );
        assert!(
            prompt.contains("do NOT invent changes"),
            "Prompt must include do-not-invent guard"
        );
    }

    #[test]
    fn test_impact_analysis_prompt_contains_commit_prefix() {
        let prompt = build_impact_analysis_prompt(
            "4-6-post-implementation-impact-analysis",
            "/impl",
            "/plan",
        );
        assert!(
            prompt.contains("docs(stories): update downstream specs after 4-6-post-implementation-impact-analysis"),
            "Prompt must contain the commit message prefix with story key"
        );
    }

    #[test]
    fn test_impact_analysis_prompt_idempotent_language() {
        let prompt = build_impact_analysis_prompt("x", "/i", "/p");
        assert!(
            prompt.contains("REPLACED (idempotent), not appended"),
            "Prompt must instruct idempotent replacement"
        );
    }

    #[test]
    fn test_compressed_state_total_messages_context_limit() {
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

    // ── truncate_summary tests (Task 12) ────────────────────────────

    #[test]
    fn test_truncate_summary_short_text() {
        let result = truncate_summary("hello world", 80);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_truncate_summary_exact_limit() {
        let text = "a".repeat(80);
        let result = truncate_summary(&text, 80);
        assert_eq!(
            result, text,
            "Exact-limit text should be returned unchanged"
        );
    }

    #[test]
    fn test_truncate_summary_long_text() {
        let text = "a".repeat(100);
        let result = truncate_summary(&text, 80);
        assert_eq!(result.chars().count(), 81, "80 chars + 1 ellipsis");
        assert!(result.ends_with('…'));
        assert_eq!(&result[..80], "a".repeat(80));
    }

    #[test]
    fn test_truncate_summary_empty() {
        let result = truncate_summary("", 80);
        assert_eq!(result, "");
    }

    #[test]
    fn test_truncate_summary_unicode_boundary() {
        // Each emoji is a multi-byte char. 5 emojis = 5 chars, 20 bytes.
        let text = "🎉🎊🎈🎁🎂extra";
        // Truncate at 5 chars — should cut right after the 5th emoji
        let result = truncate_summary(text, 5);
        assert!(
            result.ends_with('…'),
            "Should append ellipsis when truncated"
        );
        assert_eq!(result.chars().count(), 6, "5 emojis + 1 ellipsis = 6 chars");
        assert!(result.starts_with("🎉🎊🎈🎁🎂"));
    }

    // -----------------------------------------------------------------------
    // Pipeline phase consultation tracking tests (Story 13.10)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_consultation_trigger_updates_wal_phase() {
        use crate::session::consultation::{
            ConsultationConfig, ConsultationState, ConsultationToolSet,
        };
        use crate::session::state::PHASE_CREATE_ADVERSARIAL_CONSULT;

        let dir = tempfile::tempdir().expect("create temp dir");
        let runner = make_test_runner_with_dir(dir.path());
        let story = make_test_story_info();

        let mut state = SessionState::new(&story, "anthropic", "test-model");
        state.set_pipeline_phase("create");
        state.save(&runner.state_file_path()).await.expect("save");

        let config = ConsultationConfig {
            label: "adversarial".to_string(),
            skill_path: None,
            preamble_override: Some("You are a reviewer".to_string()),
            role: crate::llm::agent_factory::LlmRole::Review,
            tool_set: ConsultationToolSet::Full,
            context_files: vec![],
            trigger_pattern: r"story file created".to_string(),
            prompt_template: "{context}".to_string(),
            resume_message_template: "{findings}".to_string(),
            pipeline_phase: Some(PHASE_CREATE_ADVERSARIAL_CONSULT.to_string()),
        };
        let mut consultation_states = ConsultationState::from_configs(vec![config]);

        // Trigger consultation — will fail (no real LLM) but phase should be updated first
        let _result = runner
            .check_consultation_triggers(
                "story file created successfully",
                &mut consultation_states,
                &mut state,
            )
            .await;

        assert_eq!(
            state.pipeline_phase, "create-adversarial-consult",
            "pipeline_phase should be updated after consultation trigger"
        );

        // Verify WAL on disk also has the updated phase
        let loaded = SessionState::load(&runner.state_file_path())
            .await
            .expect("load WAL");
        assert_eq!(
            loaded.pipeline_phase, "create-adversarial-consult",
            "WAL on disk should have the updated phase (write-ahead guarantee)"
        );
    }

    #[tokio::test]
    async fn test_consultation_without_pipeline_phase_does_not_change_wal() {
        use crate::session::consultation::{
            ConsultationConfig, ConsultationState, ConsultationToolSet,
        };

        let dir = tempfile::tempdir().expect("create temp dir");
        let runner = make_test_runner_with_dir(dir.path());
        let story = make_test_story_info();

        let mut state = SessionState::new(&story, "anthropic", "test-model");
        state.set_pipeline_phase("create");
        state.save(&runner.state_file_path()).await.expect("save");

        let config = ConsultationConfig {
            label: "no-phase".to_string(),
            skill_path: None,
            preamble_override: Some("You are a reviewer".to_string()),
            role: crate::llm::agent_factory::LlmRole::Review,
            tool_set: ConsultationToolSet::Full,
            context_files: vec![],
            trigger_pattern: r"trigger this".to_string(),
            prompt_template: "{context}".to_string(),
            resume_message_template: "{findings}".to_string(),
            pipeline_phase: None,
        };
        let mut consultation_states = ConsultationState::from_configs(vec![config]);

        let _result = runner
            .check_consultation_triggers(
                "trigger this response",
                &mut consultation_states,
                &mut state,
            )
            .await;

        assert_eq!(
            state.pipeline_phase, "create",
            "pipeline_phase should NOT change when consultation has pipeline_phase: None"
        );
    }
}
