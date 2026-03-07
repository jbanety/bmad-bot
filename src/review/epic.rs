//! Epic review session — autonomous post-epic retrospective analysis.
//!
//! The [`EpicReviewRunner`] launches a new rig agent session using the
//! `EpicReview` LLM role to conduct an autonomous codebase review after
//! all stories in an epic have been completed. The agent adopts the
//! Architect persona (Winston) and explores the codebase using read-only
//! tools to produce a structured retrospective report.
//!
//! **Design:** Mirrors the [`ReviewRunner`](super::ReviewRunner) pattern —
//! struct with config/secrets/factory/shutdown fields, `new()` constructor,
//! `run()` public method with retry loop, typed outcome enum.
//!
//! Key differences from `ReviewRunner`:
//! - Uses `LlmRole::EpicReview` (falls back to `Review` config if unconfigured)
//! - Preamble is Architect persona (read-only constraints, no BMAD workflow)
//! - Tool set: 6 tools + ThinkTool (no `edit_file`, no `ask_supervisor`) — read-only by prompt
//! - Single prompt → multi-turn autonomous exploration
//! - Completion via `<<EPIC_REVIEW_REPORT_END>>` delimiter or no-tool-call heuristic
//! - Output is a markdown report, not a review verdict

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use rig::completion::Message;
use rig::tools::think::ThinkTool;

use crate::config::{BotConfig, BotSecrets};
use crate::llm::agent_factory::{AgentFactory, BuiltAgent, LlmRole};
use crate::llm::logging::{log_llm_request, log_llm_response};
use crate::session::agent::{self, ShutdownFlag};
use crate::session::provider::ProviderError;
use crate::tools::{
    FindPathTool, GitTool, GrepTool, ListDirectoryTool, ReadFileTool, TerminalTool,
};
use crate::ui::UiHandle;

use super::{
    MAX_SESSION_RETRIES, MAX_TOKEN_REFRESHES, is_retryable_review_error, is_token_expired_error,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum chat turns for an epic review session (safety net).
///
/// Higher than `ReviewRunner`'s 100 — epic review explores more files and
/// runs cargo commands across the entire epic's codebase.
const MAX_EPIC_REVIEW_TURNS: usize = 200;

/// Maximum number of per-turn retries for transient SSE/network errors within
/// a single epic review session (e.g. unexpected EOF, connection reset).
///
/// These are retried inline with exponential backoff *before* escalating to a
/// full session retry via [`super::MAX_SESSION_RETRIES`].  Keeping them separate
/// avoids burning a full-session retry budget on a one-second blip.
const MAX_TURN_RETRIES: usize = 3;

/// Base delay (ms) for exponential backoff on transient turn-level errors.
const TURN_RETRY_BASE_MS: u64 = 1_000;

/// Maximum number of full session retries for transient errors.

/// Number of consecutive no-tool-call turns before treating the session as complete.
const NO_TOOL_CALL_THRESHOLD: usize = 3;

/// Delimiter that marks the start of the review report in agent output.
const REPORT_START_DELIMITER: &str = "<<EPIC_REVIEW_REPORT_START>>";

/// Delimiter that marks the end of the review report in agent output.
const REPORT_END_DELIMITER: &str = "<<EPIC_REVIEW_REPORT_END>>";

// ---------------------------------------------------------------------------
// EpicReviewOutcome
// ---------------------------------------------------------------------------

/// Result of an epic review session.
#[derive(Debug, Clone)]
pub enum EpicReviewOutcome {
    /// Review completed — report extracted (full or partial).
    Completed {
        /// The markdown report content.
        report: String,
        /// The epic number that was reviewed.
        epic_num: u32,
    },
    /// Review failed — error captured, minimal failure report generated.
    Failed {
        /// Human-readable reason for the failure.
        reason: String,
        /// The epic number that was being reviewed.
        epic_num: u32,
    },
}

// ---------------------------------------------------------------------------
// EpicReviewRunner
// ---------------------------------------------------------------------------

/// Manages the lifecycle of an autonomous epic review session.
///
/// Mirrors the [`ReviewRunner`](super::ReviewRunner) pattern: construct once
/// via [`new()`](Self::new), then call [`run()`](Self::run) for each epic.
#[derive(Debug)]
pub struct EpicReviewRunner {
    /// Shared bot configuration.
    config: Arc<BotConfig>,
    /// Shared secrets (API keys loaded from `.env`).
    #[allow(dead_code)]
    secrets: Arc<BotSecrets>,
    /// Centralized agent construction factory.
    agent_factory: Arc<AgentFactory>,
    /// Cooperative shutdown flag — checked between chat turns.
    shutdown: ShutdownFlag,
    /// MCP server manager — provides external tool capabilities.
    mcp_manager: Arc<crate::mcp::McpManager>,
    /// UI handle for rendering terminal output.
    ui: UiHandle,
}

impl EpicReviewRunner {
    /// Create a new epic review runner.
    pub fn new(
        config: Arc<BotConfig>,
        secrets: Arc<BotSecrets>,
        agent_factory: Arc<AgentFactory>,
        shutdown: ShutdownFlag,
        mcp_manager: Arc<crate::mcp::McpManager>,
        ui: UiHandle,
    ) -> Self {
        Self {
            config,
            secrets,
            agent_factory,
            shutdown,
            mcp_manager,
            ui,
        }
    }

    /// Run an autonomous epic review session.
    ///
    /// This method NEVER panics or returns an unhandled error. All failures
    /// are caught and returned as [`EpicReviewOutcome::Failed`].
    ///
    /// **Retry logic:** If the session fails due to a transient error
    /// (network, malformed tool call, etc.), the session is retried from
    /// scratch with a fresh agent and history.
    pub async fn run(&self, epic_num: u32) -> EpicReviewOutcome {
        for attempt in 0..=MAX_SESSION_RETRIES {
            match self.run_inner(epic_num).await {
                Ok(outcome) => return outcome,
                Err(e) => {
                    let error_str = e.to_string();
                    let is_retryable = is_retryable_review_error(&error_str);

                    if is_retryable && attempt < MAX_SESSION_RETRIES {
                        tracing::warn!(
                            action = "epic_review_retry",
                            attempt = attempt + 1,
                            max_retries = MAX_SESSION_RETRIES,
                            error = %error_str,
                            epic_num = epic_num,
                            "Transient error — retrying epic review with fresh session"
                        );
                        self.ui
                            .llm_retry("epic_review", 0, (attempt + 1) as u32, 0.0);
                        continue;
                    }

                    tracing::error!(
                        action = "epic_review_failed",
                        error = %e,
                        epic_num = epic_num,
                        attempt = attempt + 1,
                        "Epic review failed — generating failure report"
                    );
                    return EpicReviewOutcome::Failed {
                        reason: e.to_string(),
                        epic_num,
                    };
                }
            }
        }

        // Unreachable — the loop always returns on the last attempt
        EpicReviewOutcome::Failed {
            reason: "Epic review exhausted all retries".to_string(),
            epic_num: 0,
        }
    }

    /// Build a fresh [`BuiltAgent`] for epic review.
    ///
    /// Uses 6 read-safe tools + ThinkTool (no `edit_file`, no `ask_supervisor`).
    /// The "read-only" constraint for `git` and `terminal` is enforced by the preamble prompt.
    async fn build_epic_review_agent(&self) -> Result<BuiltAgent, EpicReviewError> {
        let mcp_data = self.mcp_manager.tools_for_builder().await;
        let mcp_tool_names = crate::mcp::extract_mcp_tool_names(&mcp_data);

        let preamble = build_epic_review_preamble(&self.config, &mcp_tool_names);

        let project_root = PathBuf::from(&self.config.bmad_paths.project_root);

        // Build 6 tools: read_file, grep, find_path, list_directory, terminal, git
        // Deliberately NO edit_file and NO ask_supervisor
        let read_file = ReadFileTool::new(project_root.clone());
        let grep = GrepTool::new(project_root.clone());
        let find_path = FindPathTool::new(project_root.clone());
        let list_dir = ListDirectoryTool::new(project_root.clone());
        let terminal = TerminalTool::new(project_root.clone(), agent::TERMINAL_TIMEOUT_SECS);
        let git = GitTool::new(project_root);

        self.agent_factory
            .build(
                LlmRole::EpicReview,
                &preamble,
                crate::configure_agent_tools!(
                    git, read_file, grep, find_path, list_dir, terminal, ThinkTool
                )
                .with_mcp(mcp_data),
            )
            .await
            .map_err(|e| match e {
                ProviderError::MissingApiKey { env_var, .. } => EpicReviewError::ApiKeyMissing {
                    provider: self
                        .agent_factory
                        .config_for_role(LlmRole::EpicReview)
                        .provider
                        .clone(),
                    env_var,
                },
                other => EpicReviewError::ProviderInit {
                    reason: other.to_string(),
                },
            })
    }

    /// Inner implementation that can fail with [`EpicReviewError`].
    async fn run_inner(&self, epic_num: u32) -> Result<EpicReviewOutcome, EpicReviewError> {
        let span = tracing::info_span!("epic_review_session", epic_num = epic_num);
        let _guard = span.enter();

        tracing::info!(
            action = "epic_review_start",
            epic_num = epic_num,
            "Starting autonomous epic review session"
        );

        self.ui
            .phase_start(&format!("Epic {epic_num} Autonomous Review"));

        let agent = self.build_epic_review_agent().await?;

        let prompt = build_epic_review_prompt(epic_num, &self.config);

        let outcome = self.drive_epic_review(agent, epic_num, &prompt).await?;

        let outcome_type = match &outcome {
            EpicReviewOutcome::Completed { .. } => "completed",
            EpicReviewOutcome::Failed { .. } => "failed",
        };

        self.ui.phase_complete(
            &format!("Epic {epic_num} Review: {outcome_type}"),
            std::time::Duration::ZERO,
        );

        tracing::info!(
            action = "epic_review_end",
            outcome = %outcome_type,
            epic_num = epic_num,
            "Epic review session ended"
        );

        Ok(outcome)
    }

    /// Drive the epic review chat loop.
    ///
    /// Sends the initial prompt and then loops, checking for:
    /// 1. Report delimiter detection (primary completion signal)
    /// 2. No-tool-call heuristic (fallback completion signal)
    /// 3. Turn limit safety net
    /// 4. Shutdown flag
    ///
    /// Transient per-turn errors (SSE EOF, connection reset, 429, 503) are
    /// retried inline up to [`MAX_TURN_RETRIES`] times with exponential
    /// backoff.  If all per-turn retries are exhausted the error is returned
    /// as [`EpicReviewError::ChatFailed`] so the outer `run()` loop can
    /// trigger a full-session retry via [`MAX_SESSION_RETRIES`].
    async fn drive_epic_review(
        &self,
        mut agent: BuiltAgent,
        epic_num: u32,
        prompt: &str,
    ) -> Result<EpicReviewOutcome, EpicReviewError> {
        let start = Instant::now();
        let mut history: Vec<Message> = Vec::new();
        let mut all_agent_output: Vec<String> = Vec::new();
        let mut consecutive_no_tool_turns: usize = 0;
        let mut token_refreshes: usize = 0;

        // Send initial prompt
        let mut current_prompt = prompt.to_string();

        for turn in 0..MAX_EPIC_REVIEW_TURNS {
            // Check shutdown
            if self.shutdown.load(Ordering::Relaxed) {
                tracing::info!(
                    action = "epic_review_shutdown",
                    turn = turn,
                    epic_num = epic_num,
                    "Shutdown requested — capturing partial output"
                );
                break;
            }

            log_llm_request("epic_review", turn, &current_prompt, history.len());

            self.ui.llm_request("epic_review", turn as u32);

            // ---------------------------------------------------------------
            // Per-turn retry loop — handles transient network/SSE errors
            // ---------------------------------------------------------------
            let result = {
                let mut last_err: Option<String> = None;
                let mut outcome = None;
                for attempt in 0..=MAX_TURN_RETRIES {
                    match agent
                        .stream_chat(
                            &current_prompt,
                            history.clone(),
                            Some(&self.shutdown),
                            Some(&self.ui),
                        )
                        .await
                    {
                        Ok(val) => {
                            outcome = Some(Ok(val));
                            break;
                        }
                        Err(e) => {
                            let err_str = e.to_string();

                            // ── Token expiry handling ──────────────────────
                            // Copilot session tokens are short-lived (~30 min).
                            // Epic reviews easily exceed that. Rebuild the agent
                            // with a fresh token and retry the same turn — the
                            // existing history is reusable, only the HTTP client
                            // (with the baked-in bearer token) needs replacing.
                            if is_token_expired_error(&err_str)
                                && token_refreshes < MAX_TOKEN_REFRESHES
                            {
                                token_refreshes += 1;
                                tracing::warn!(
                                    action = "epic_review_token_expired_rebuild",
                                    turn = turn,
                                    refresh = token_refreshes,
                                    max_refreshes = MAX_TOKEN_REFRESHES,
                                    epic_num = epic_num,
                                    "Copilot token expired in epic review — rebuilding agent with fresh token"
                                );
                                match self.build_epic_review_agent().await {
                                    Ok(new_agent) => {
                                        agent = new_agent;
                                        self.ui.llm_retry(
                                            "epic_review",
                                            turn as u32,
                                            token_refreshes as u32,
                                            0.0,
                                        );
                                        tracing::info!(
                                            action = "epic_review_token_refreshed",
                                            refresh = token_refreshes,
                                            "Epic review agent rebuilt with fresh token — retrying turn"
                                        );
                                        // Retry this attempt without consuming a turn-retry slot
                                        continue;
                                    }
                                    Err(rebuild_err) => {
                                        tracing::error!(
                                            action = "epic_review_token_refresh_failed",
                                            error = %rebuild_err,
                                            epic_num = epic_num,
                                            "Failed to rebuild epic review agent after token expiry"
                                        );
                                        // Fall through to normal transient retry logic
                                    }
                                }
                            }

                            let transient = is_retryable_review_error(&err_str);
                            if transient && attempt < MAX_TURN_RETRIES {
                                let delay_ms = TURN_RETRY_BASE_MS * (1u64 << attempt); // 1s, 2s, 4s
                                tracing::warn!(
                                    action = "epic_review_turn_retry",
                                    turn = turn,
                                    attempt = attempt + 1,
                                    max_retries = MAX_TURN_RETRIES,
                                    delay_ms = delay_ms,
                                    error = %err_str,
                                    epic_num = epic_num,
                                    "Transient turn error — retrying with backoff"
                                );
                                self.ui.llm_retry(
                                    "epic_review",
                                    turn as u32,
                                    (attempt + 1) as u32,
                                    delay_ms as f64 / 1_000.0,
                                );
                                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                            } else {
                                last_err = Some(err_str);
                                break;
                            }
                        }
                    }
                }
                match outcome {
                    Some(r) => r,
                    None => Err(last_err.unwrap_or_else(|| "unknown error".to_string())),
                }
            };

            match result {
                Ok((response_text, updated_history)) => {
                    log_llm_response("epic_review", turn, &response_text);

                    let has_tool_calls = response_text.contains("Tool call:")
                        || response_text.contains("tool_use")
                        || updated_history.len() > history.len() + 2;

                    history = updated_history;
                    all_agent_output.push(response_text.clone());

                    self.ui.chat_turn(
                        turn as u32,
                        if has_tool_calls { "tool calls" } else { "text" },
                    );

                    // Check for report end delimiter (primary completion signal)
                    if response_text.contains(REPORT_END_DELIMITER) {
                        tracing::info!(
                            action = "epic_review_complete_delimiter",
                            turn = turn,
                            elapsed_secs = start.elapsed().as_secs(),
                            "Epic review completed — report delimiter found"
                        );
                        let report = extract_report(&all_agent_output, epic_num);
                        return Ok(EpicReviewOutcome::Completed { report, epic_num });
                    }

                    // No-tool-call heuristic (fallback completion signal)
                    if has_tool_calls {
                        consecutive_no_tool_turns = 0;
                    } else {
                        consecutive_no_tool_turns += 1;
                        if consecutive_no_tool_turns >= NO_TOOL_CALL_THRESHOLD {
                            tracing::info!(
                                action = "epic_review_complete_heuristic",
                                turn = turn,
                                consecutive_no_tool = consecutive_no_tool_turns,
                                "Epic review completed — no-tool-call heuristic triggered"
                            );
                            let report = extract_report(&all_agent_output, epic_num);
                            return Ok(EpicReviewOutcome::Completed { report, epic_num });
                        }
                    }

                    // Continue conversation — nudge toward completion
                    current_prompt = "Continue your analysis. When you have finished, output the complete report between the <<EPIC_REVIEW_REPORT_START>> and <<EPIC_REVIEW_REPORT_END>> delimiters.".to_string();
                }
                Err(error_str) => {
                    tracing::error!(
                        action = "epic_review_chat_error",
                        turn = turn,
                        error = %error_str,
                        epic_num = epic_num,
                        "Chat error during epic review — all per-turn retries exhausted"
                    );

                    // If we already have meaningful partial output, salvage it
                    // rather than discarding everything and retrying the session.
                    if !all_agent_output.is_empty() {
                        tracing::warn!(
                            action = "epic_review_partial_salvage",
                            turn = turn,
                            chunks = all_agent_output.len(),
                            epic_num = epic_num,
                            "Salvaging partial output after unrecoverable turn error"
                        );
                        let report = extract_report(&all_agent_output, epic_num);
                        return Ok(EpicReviewOutcome::Completed { report, epic_num });
                    }

                    // No output at all — propagate as Err so run() can retry
                    // with a fresh session (full-session retry budget).
                    return Err(EpicReviewError::ChatFailed {
                        turn,
                        reason: error_str,
                    });
                }
            }
        }

        // Turn limit reached — use whatever output we have
        tracing::warn!(
            action = "epic_review_turn_limit",
            max_turns = MAX_EPIC_REVIEW_TURNS,
            epic_num = epic_num,
            "Epic review hit turn limit — using partial output"
        );

        if all_agent_output.is_empty() {
            Ok(EpicReviewOutcome::Failed {
                reason: format!(
                    "Epic review hit turn limit ({MAX_EPIC_REVIEW_TURNS}) with no output"
                ),
                epic_num,
            })
        } else {
            let report = extract_report(&all_agent_output, epic_num);
            Ok(EpicReviewOutcome::Completed { report, epic_num })
        }
    }
}

// ---------------------------------------------------------------------------
// EpicReviewError (internal)
// ---------------------------------------------------------------------------

/// Internal error type for epic review — not exposed publicly.
/// Failures are wrapped into [`EpicReviewOutcome::Failed`] at the boundary.
#[derive(Debug, thiserror::Error)]
enum EpicReviewError {
    #[error("Failed to initialize LLM provider: {reason}")]
    ProviderInit { reason: String },

    #[error("API key missing for {provider} (env var: {env_var})")]
    ApiKeyMissing { provider: String, env_var: String },

    /// Transient chat error that exhausted all per-turn retries.
    /// Propagated to `run()` so the full-session retry loop can trigger.
    #[error("Chat failed on turn {turn} after retries: {reason}")]
    ChatFailed { turn: usize, reason: String },
}

// ---------------------------------------------------------------------------
// Preamble Construction
// ---------------------------------------------------------------------------

/// Build the system preamble for the epic review agent.
///
/// Combines:
/// 1. Minimal operational instructions (read-only constraints)
/// 2. Architect persona identity (role + communication style)
/// 3. Reference to project-context.md (loaded by agent via tools)
///
/// The preamble does NOT include BMAD workflow activation, menus, or
/// interactive agent setup — this is a single-purpose autonomous review.
pub fn build_epic_review_preamble(config: &BotConfig, mcp_tool_names: &[String]) -> String {
    let mcp_line = if mcp_tool_names.is_empty() {
        String::new()
    } else {
        format!(
            "\nYou also have access to MCP tools: {}. Use them like any other tool.",
            mcp_tool_names.join(", ")
        )
    };

    let project_root = &config.bmad_paths.project_root;

    format!(
        "You are Winston, a Senior Architect conducting an autonomous post-epic codebase review.\n\
         \n\
         ## Your Identity\n\
         - **Role:** Senior Architect\n\
         - **Identity:** Designs robust, scalable systems with deep distributed-systems expertise. \
         Reviews code for architectural consistency, pattern adherence, and technical debt.\n\
         - **Communication Style:** Precise, structured, evidence-based. Every claim is backed by \
         file paths and code references.\n\
         - **Principles:** Architecture decisions must be justified. Patterns must be consistent. \
         Debt must be catalogued, not hidden.\n\
         \n\
         ## Tools\n\
         You have access to these tools: read_file, grep, find_path, list_directory, git, terminal, \
         plus a built-in think tool for reasoning.{mcp_line}\n\
         \n\
         ## Tool Usage Rules\n\
         - **Use `read_file` with line ranges** for large files. Read the outline first, then target \
         specific sections.\n\
         - **Use `grep` to find patterns** across the codebase — naming conventions, error handling, \
         logging styles.\n\
         - **Use `find_path`** to discover files by name pattern.\n\
         - **Use `list_directory`** to explore project structure.\n\
         - **Use `terminal`** ONLY for read-only commands: `cargo check`, `cargo test`, `cargo clippy`, \
         `cargo fmt --check`, `wc`, `find`, `cat`.\n\
         - **Use `git`** ONLY for read-only operations: `git log`, `git diff`, `git status`, `git show`.\n\
         - **Use `think`** for structured reasoning during analysis.\n\
         \n\
         ## CRITICAL CONSTRAINTS\n\
         - You are conducting a **READ-ONLY** review. Do NOT modify any files.\n\
         - Do NOT use the git tool for write operations (commit, checkout, branch, reset, stash, push). \
         Only use: git log, git diff, git status, git show.\n\
         - Do NOT use the terminal tool to modify files. Only use it for: cargo check, cargo test, \
         cargo clippy, cargo fmt --check, wc, find, cat.\n\
         - Your job is to **ANALYZE and REPORT**, not to fix.\n\
         \n\
         ## Project Context\n\
         Load the project rules from `{project_root}/_bmad-output/project-context.md` using \
         `read_file` at the start of your analysis.\n\
         \n\
         ## Communication\n\
         All output MUST be in English.",
        mcp_line = mcp_line,
        project_root = project_root
    )
}

// ---------------------------------------------------------------------------
// Review Prompt Construction
// ---------------------------------------------------------------------------

/// Build the initial user message for the epic review session.
///
/// Instructs the agent to use tools to load epic definition, story files,
/// and architecture doc. Provides the four-section report structure and
/// delimiter instructions.
///
/// **Why tool-based loading:** Large epics can have 10+ story files at 200-400
/// lines each. Inlining everything would consume the context window before the
/// agent starts analyzing code. Tool-based loading lets the agent load what it
/// needs, when it needs it.
pub fn build_epic_review_prompt(epic_num: u32, config: &BotConfig) -> String {
    let planning = &config.bmad_paths.planning_artifacts;
    let implementation = &config.bmad_paths.implementation_artifacts;
    let project_root = &config.bmad_paths.project_root;
    let start_delim = REPORT_START_DELIMITER;
    let end_delim = REPORT_END_DELIMITER;

    format!(
        "## Epic {epic_num} Autonomous Review\n\
         \n\
         You are reviewing Epic {epic_num} after all its stories have been completed.\n\
         \n\
         ### Files to Load (use your tools)\n\
         \n\
         1. **Project context**: Use `read_file` to load `{project_root}/_bmad-output/project-context.md` \
         — these are the implementation rules you must evaluate against\n\
         2. **Epic definition**: Use `grep` to find \"## Epic {epic_num}:\" in `{planning}/epics.md`, \
         then `read_file` to load that section\n\
         3. **Story files**: Use `find_path` with glob `{implementation}/{epic_num}-*` to discover all \
         story files for this epic, then `read_file` each one — focus on Acceptance Criteria, Dev Notes, \
         and Completion Notes\n\
         4. **Architecture document**: Use `find_path` to locate architecture doc in `{planning}/`, \
         then `read_file` — focus on patterns, decisions, and project structure sections\n\
         5. **Source code**: Use `grep`, `find_path`, `list_directory`, and `read_file` to explore \
         the actual implementation\n\
         \n\
         ### Your Mission\n\
         \n\
         Produce a comprehensive review report. Structure it with these four sections.\n\
         Output the COMPLETE report between delimiters:\n\
         \n\
         {start_delim}\n\
         (your full markdown report here)\n\
         {end_delim}\n\
         \n\
         #### 1. Epic Recap\n\
         - What were the epic's objectives?\n\
         - What stories were delivered?\n\
         - What was planned vs what was actually built?\n\
         - Any scope changes or deviations?\n\
         \n\
         #### 2. Functional Testing Guide\n\
         - For each major capability delivered in this epic, provide:\n\
           - A step-by-step testing scenario the human can follow\n\
           - Concrete CLI commands to run (with expected output)\n\
           - Edge cases worth testing manually\n\
           - What \"working correctly\" looks like\n\
         - Base this on ACTUAL implementation (read the code), not just acceptance criteria\n\
         - Be specific: file paths, config values, command flags\n\
         \n\
         #### 3. Technical Analysis\n\
         - **Pattern Consistency**: Are the same problems solved the same way across all stories? \
         Error handling, logging, naming conventions — uniform?\n\
         - **Architecture Adherence**: Does the implementation match architecture.md? Any drift?\n\
         - **Technical Debt Inventory**: TODOs, shortcuts, known issues — list them with file locations\n\
         - **Cross-Cutting Concerns**: Test coverage gaps, security surface, dependency hygiene\n\
         - **Codebase Health**: Run `cargo check`, `cargo test`, `cargo clippy` via terminal tool \
         and report results verbatim\n\
         \n\
         #### 4. Recommendations\n\
         - Actionable items for the next epic\n\
         - Risks to watch\n\
         - Debt to address before it compounds\n\
         - Patterns to reinforce or abandon\n\
         \n\
         Use your tools to explore the codebase thoroughly. Read source files, grep for patterns, \
         run build commands. Do not guess — verify.\n\
         \n\
         CRITICAL CONSTRAINTS:\n\
         - You are conducting a READ-ONLY review. Do NOT modify any files.\n\
         - Do NOT use git for write operations. Only: git log, git diff, git status, git show.\n\
         - Do NOT use terminal to modify files. Only: cargo check, cargo test, cargo clippy, \
         cargo fmt --check, wc, find, cat.",
        epic_num = epic_num,
        project_root = project_root,
        planning = planning,
        implementation = implementation,
        start_delim = start_delim,
        end_delim = end_delim,
    )
}

// ---------------------------------------------------------------------------
// Report Extraction
// ---------------------------------------------------------------------------

/// Extract the review report from the agent's accumulated output.
///
/// Strategy (in order):
/// 1. Find content between `<<EPIC_REVIEW_REPORT_START>>` and `<<EPIC_REVIEW_REPORT_END>>`
/// 2. If delimiters not found, concatenate all agent messages as fallback
/// 3. If empty, generate a minimal placeholder report
pub fn extract_report(all_output: &[String], epic_num: u32) -> String {
    let combined = all_output.join("\n");

    // Strategy 1: Delimiter extraction
    if let Some(start_idx) = combined.find(REPORT_START_DELIMITER) {
        let content_start = start_idx + REPORT_START_DELIMITER.len();
        if let Some(end_idx) = combined[content_start..].find(REPORT_END_DELIMITER) {
            let report = combined[content_start..content_start + end_idx].trim();
            if !report.is_empty() {
                return report.to_string();
            }
        }
        // Start delimiter found but no end — take everything after start
        let report = combined[content_start..].trim();
        if !report.is_empty() {
            return format!(
                "<!-- WARNING: End delimiter not found — report may be incomplete -->\n\n{report}"
            );
        }
    }

    // Strategy 2: Fallback — use all output
    let fallback = combined.trim();
    if !fallback.is_empty() {
        return format!(
            "<!-- WARNING: Report delimiters not found — this is the raw agent output -->\n\n{fallback}"
        );
    }

    // Strategy 3: Empty report
    format!(
        "# Epic {epic_num} Review — No Report Generated\n\n\
         The review session completed but produced no extractable report. Manual review recommended."
    )
}

/// Extract the "Epic Recap" section from a report for use as MR description.
///
/// Looks for a heading containing "Epic Recap" and takes content until the next
/// heading of the same or higher level. Falls back to the first 50 lines.
pub fn extract_epic_recap(report: &str) -> String {
    let lines: Vec<&str> = report.lines().collect();
    let mut recap_start: Option<usize> = None;
    let mut recap_heading_level: usize = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.contains("Epic Recap") && trimmed.starts_with('#') {
            recap_start = Some(i);
            recap_heading_level = trimmed.chars().take_while(|c| *c == '#').count();
            continue;
        }
        if let Some(start) = recap_start
            && i > start
            && trimmed.starts_with('#')
        {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            if level <= recap_heading_level {
                // Found next section — take everything between
                return lines[start..i].join("\n").trim().to_string();
            }
        }
    }

    // If we found the start but no end, take the rest
    if let Some(start) = recap_start {
        return lines[start..].join("\n").trim().to_string();
    }

    // Fallback: first 50 lines
    let max_lines = lines.len().min(50);
    lines[..max_lines].join("\n").trim().to_string()
}

/// Generate a minimal failure report when the review session fails.
pub fn generate_failure_report(epic_num: u32, error: &str) -> String {
    format!(
        "# Epic {epic_num} Review — Session Failed\n\
         \n\
         ## Error Details\n\
         \n\
         The autonomous review session failed with the following error:\n\
         \n\
         ```\n\
         {error}\n\
         ```\n\
         \n\
         ## Recommended Actions\n\
         \n\
         1. Check the daemon logs for detailed error information\n\
         2. Verify LLM provider connectivity and API key validity\n\
         3. Consider running the review manually\n\
         4. Set `epic-{epic_num}-retrospective: done` in sprint-status.yaml after manual review\n\
         \n\
         ## Codebase Health (not verified)\n\
         \n\
         The automated review could not complete. Manual verification of the following is recommended:\n\
         - `cargo check` — compile errors\n\
         - `cargo test` — test failures\n\
         - `cargo clippy` — lint warnings\n\
         - `cargo fmt --check` — formatting issues\n",
        epic_num = epic_num,
        error = error,
    )
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Report extraction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_report_with_delimiters() {
        let output = vec![
            "Analyzing codebase...".to_string(),
            format!(
                "Here is my report:\n{}\n# Epic 1 Recap\nGreat work\n## Technical\nClean code\n{}",
                REPORT_START_DELIMITER, REPORT_END_DELIMITER
            ),
        ];
        let report = extract_report(&output, 1);
        assert!(report.contains("Epic 1 Recap"));
        assert!(report.contains("Clean code"));
        assert!(!report.contains(REPORT_START_DELIMITER));
        assert!(!report.contains(REPORT_END_DELIMITER));
    }

    #[test]
    fn test_extract_report_without_delimiters_uses_fallback() {
        let output = vec![
            "I analyzed the code.".to_string(),
            "Here are my findings: everything looks good.".to_string(),
        ];
        let report = extract_report(&output, 2);
        assert!(report.contains("WARNING: Report delimiters not found"));
        assert!(report.contains("everything looks good"));
    }

    #[test]
    fn test_extract_report_empty_output_generates_placeholder() {
        let output: Vec<String> = vec![];
        let report = extract_report(&output, 3);
        assert!(report.contains("No Report Generated"));
        assert!(report.contains("Epic 3"));
    }

    #[test]
    fn test_extract_report_start_delimiter_without_end() {
        let output = vec![format!(
            "{}\n# Partial Report\nSome analysis here",
            REPORT_START_DELIMITER
        )];
        let report = extract_report(&output, 1);
        assert!(report.contains("WARNING: End delimiter not found"));
        assert!(report.contains("Partial Report"));
    }

    #[test]
    fn test_extract_report_delimiters_across_messages() {
        let output = vec![
            format!(
                "Analysis:\n{}\n# Epic Recap\nStories delivered",
                REPORT_START_DELIMITER
            ),
            "## Technical Analysis\nCode is clean".to_string(),
            format!("## Recommendations\nDo X\n{}", REPORT_END_DELIMITER),
        ];
        let report = extract_report(&output, 4);
        assert!(report.contains("Epic Recap"));
        assert!(report.contains("Code is clean"));
        assert!(report.contains("Do X"));
    }

    // -----------------------------------------------------------------------
    // Epic recap extraction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_epic_recap_with_heading() {
        let report = "# Full Report\n\
                       \n\
                       ## Epic Recap\n\
                       Stories 1-3 delivered.\n\
                       Scope was as planned.\n\
                       \n\
                       ## Technical Analysis\n\
                       Code is clean.\n\
                       \n\
                       ## Recommendations\n\
                       None.\n";
        let recap = extract_epic_recap(report);
        assert!(recap.contains("## Epic Recap"));
        assert!(recap.contains("Stories 1-3 delivered"));
        assert!(!recap.contains("Technical Analysis"));
    }

    #[test]
    fn test_extract_epic_recap_with_h4_heading() {
        let report = "#### 1. Epic Recap\n\
                       Objectives met.\n\
                       \n\
                       #### 2. Technical Analysis\n\
                       Patterns consistent.\n";
        let recap = extract_epic_recap(report);
        assert!(recap.contains("Epic Recap"));
        assert!(recap.contains("Objectives met"));
        assert!(!recap.contains("Technical Analysis"));
    }

    #[test]
    fn test_extract_epic_recap_no_heading_falls_back_to_first_50_lines() {
        let report = "Line 1\nLine 2\nLine 3\n";
        let recap = extract_epic_recap(report);
        assert!(recap.contains("Line 1"));
        assert!(recap.contains("Line 3"));
    }

    #[test]
    fn test_extract_epic_recap_recap_at_end_of_report() {
        let report = "## Epic Recap\n\
                       This was the last section.\n\
                       No more headings after this.\n";
        let recap = extract_epic_recap(report);
        assert!(recap.contains("## Epic Recap"));
        assert!(recap.contains("No more headings after this"));
    }

    // -----------------------------------------------------------------------
    // Failure report tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_failure_report_contains_error() {
        let report = generate_failure_report(5, "connection timeout");
        assert!(report.contains("Epic 5"));
        assert!(report.contains("connection timeout"));
        assert!(report.contains("Session Failed"));
        assert!(report.contains("epic-5-retrospective: done"));
    }

    // -----------------------------------------------------------------------
    // Preamble tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_preamble_contains_read_only_constraints() {
        let config = make_test_config();
        let preamble = build_epic_review_preamble(&config, &[]);
        assert!(preamble.contains("READ-ONLY"));
        assert!(preamble.contains("Do NOT modify any files"));
        assert!(preamble.contains("Winston"));
        assert!(preamble.contains("Architect"));
    }

    #[test]
    fn test_build_preamble_contains_english_override() {
        let config = make_test_config();
        let preamble = build_epic_review_preamble(&config, &[]);
        assert!(preamble.contains("English"));
    }

    #[test]
    fn test_build_preamble_includes_project_root_reference() {
        let config = make_test_config();
        let preamble = build_epic_review_preamble(&config, &[]);
        assert!(preamble.contains(&config.bmad_paths.project_root));
    }

    #[test]
    fn test_build_preamble_does_not_include_edit_file() {
        let config = make_test_config();
        let preamble = build_epic_review_preamble(&config, &[]);
        assert!(!preamble.contains("edit_file"));
        assert!(!preamble.contains("ask_supervisor"));
    }

    #[test]
    fn test_build_preamble_with_mcp_tools() {
        let config = make_test_config();
        let mcp = vec!["playwright_navigate".to_string()];
        let preamble = build_epic_review_preamble(&config, &mcp);
        assert!(preamble.contains("playwright_navigate"));
    }

    // -----------------------------------------------------------------------
    // Prompt tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_prompt_contains_epic_num() {
        let config = make_test_config();
        let prompt = build_epic_review_prompt(4, &config);
        assert!(prompt.contains("Epic 4"));
        assert!(prompt.contains(REPORT_START_DELIMITER));
        assert!(prompt.contains(REPORT_END_DELIMITER));
    }

    #[test]
    fn test_build_prompt_references_artifacts_paths() {
        let config = make_test_config();
        let prompt = build_epic_review_prompt(2, &config);
        assert!(prompt.contains(&config.bmad_paths.planning_artifacts));
        assert!(prompt.contains(&config.bmad_paths.implementation_artifacts));
    }

    #[test]
    fn test_build_prompt_contains_four_sections() {
        let config = make_test_config();
        let prompt = build_epic_review_prompt(1, &config);
        assert!(prompt.contains("Epic Recap"));
        assert!(prompt.contains("Functional Testing Guide"));
        assert!(prompt.contains("Technical Analysis"));
        assert!(prompt.contains("Recommendations"));
    }

    #[test]
    fn test_build_prompt_contains_read_only_constraints() {
        let config = make_test_config();
        let prompt = build_epic_review_prompt(1, &config);
        assert!(prompt.contains("READ-ONLY"));
    }

    // -----------------------------------------------------------------------
    // Outcome tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_epic_review_outcome_completed_fields() {
        let outcome = EpicReviewOutcome::Completed {
            report: "test report".to_string(),
            epic_num: 3,
        };
        match outcome {
            EpicReviewOutcome::Completed { report, epic_num } => {
                assert_eq!(report, "test report");
                assert_eq!(epic_num, 3);
            }
            _ => panic!("Expected Completed"),
        }
    }

    #[test]
    fn test_epic_review_outcome_failed_fields() {
        let outcome = EpicReviewOutcome::Failed {
            reason: "timeout".to_string(),
            epic_num: 5,
        };
        match outcome {
            EpicReviewOutcome::Failed { reason, epic_num } => {
                assert_eq!(reason, "timeout");
                assert_eq!(epic_num, 5);
            }
            _ => panic!("Expected Failed"),
        }
    }

    #[test]
    fn test_epic_review_outcome_is_debug() {
        let outcome = EpicReviewOutcome::Completed {
            report: "test".to_string(),
            epic_num: 1,
        };
        let debug = format!("{outcome:?}");
        assert!(debug.contains("Completed"));
    }

    // -----------------------------------------------------------------------
    // Constants tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_max_epic_review_turns_is_reasonable() {
        assert!(MAX_EPIC_REVIEW_TURNS >= 100);
        assert!(MAX_EPIC_REVIEW_TURNS <= 500);
    }

    #[test]
    fn test_no_tool_call_threshold_is_reasonable() {
        assert!(NO_TOOL_CALL_THRESHOLD >= 2);
        assert!(NO_TOOL_CALL_THRESHOLD <= 5);
    }

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

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
                epic_review: LlmRoleConfig::default(),
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
            ui_mode: "fancy".to_string(),
            ui_verbosity: "normal".to_string(),
            mcp_servers: vec![],
        }
    }
}
