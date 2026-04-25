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
//!
//! ## Epic Review
//!
//! The [`epic`] submodule provides [`EpicReviewRunner`](epic::EpicReviewRunner)
//! for autonomous post-epic retrospective analysis.

pub mod epic;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::session::cleanup::get_dirty_files;

use rig::tools::think::ThinkTool;

use crate::config::{BotConfig, BotSecrets};
use crate::llm::agent_factory::{AgentFactory, BuiltAgent, LlmRole};
use crate::llm::logging::{log_llm_error, log_llm_request, log_llm_response};
use crate::session::agent::{self, ShutdownFlag};
use crate::session::analyzer::{ResponseAction, ResponseAnalyzer};
use crate::session::provider::ProviderError;
use crate::session::runner::truncate_summary;
use crate::supervisor::EscalationSlot;
use crate::supervisor::decisions::DecisionLog;
use crate::tools::SubAgentState;
use crate::watcher::StoryInfo;

/// Maximum chat turns for a review session (safety net).
#[allow(dead_code)]
const MAX_REVIEW_TURNS: usize = 100;

/// Maximum number of full session retries for transient errors (e.g. malformed tool calls).
///
/// When the LLM sends malformed tool call arguments (e.g. two JSON objects concatenated),
/// rig includes the broken tool call in conversation history, causing the API to reject
/// all subsequent turns with 400 "invalid_tool_call_format". The only recovery is to
/// retry with a fresh session (clean history). This constant limits how many times we retry.
///
/// Shared with [`epic::EpicReviewRunner`] to ensure both runners use the same retry budget.
pub(super) const MAX_SESSION_RETRIES: usize = 2;

/// Build the post-review message sent after the CR workflow completes.
///
/// Asks the agent to commit review fixes with descriptive messages and produce
/// a structured review report wrapped in XML tags. The XML format ensures we
/// can parse the report cleanly and discard any LLM narrative/thoughts that
/// surround it (e.g. "Now let me verify...", "All tests pass...").
///
/// Includes the project name and story key to anchor the agent's context and
/// prevent hallucinated project names after long review sessions.
#[allow(dead_code)]
fn build_post_review_message(project_name: &str, story_key: &str) -> String {
    format!(
        "Commit all your review fixes with descriptive commit messages \
        that reference the findings.\n\n\
        Then provide your review report for project '{project_name}', story '{story_key}', \
        using EXACTLY this XML structure:\n\n\
        <review-report>\n\
        <findings>\n\
        (numbered list with **severity** — High/Medium/Low — and description for each finding, or 'No findings.' if clean)\n\
        </findings>\n\
        <fixes-applied>\n\
        (list each fix with the commit reference, or 'No fixes needed.' if clean)\n\
        </fixes-applied>\n\
        <remaining-concerns>\n\
        (list any unresolved issues, or 'None.')\n\
        </remaining-concerns>\n\
        </review-report>\n\n\
        CRITICAL RULES:\n\
        - Put your ENTIRE report inside the <review-report> tags\n\
        - Do NOT add any commentary, narrative, or meta-text OUTSIDE the tags\n\
        - Do NOT add any preamble like 'Here is the report:' before the tags\n\
        - Start directly with <review-report>"
    )
}

/// Parsed review report — extracted from the agent's XML-structured response.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct ParsedReviewReport {
    /// Numbered list of findings with severity.
    pub findings: String,
    /// List of fixes applied with commit references.
    pub fixes_applied: String,
    /// Remaining unresolved concerns.
    pub remaining_concerns: String,
}

/// Parse a structured review report from the agent's response.
///
/// Extracts content from `<review-report>` with sub-tags `<findings>`,
/// `<fixes-applied>`, `<remaining-concerns>`. This discards all LLM
/// narrative/thoughts outside the XML tags (e.g. "Now let me implement...",
/// "All tests pass...", etc.).
///
/// **Lenient fallback:** If `<review-report>` is present but sub-tags are
/// missing, the raw content is used as `findings` with defaults for the rest.
///
/// Returns `None` only if `<review-report>` itself is absent.
#[allow(dead_code)]
pub fn parse_review_report(response: &str) -> Option<ParsedReviewReport> {
    use regex::Regex;
    use std::sync::LazyLock;

    static RE_REPORT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?si)<review-report>(.*?)</review-report>").unwrap());
    static RE_FINDINGS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?si)<findings>(.*?)</findings>").unwrap());
    static RE_FIXES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?si)<fixes-applied>(.*?)</fixes-applied>").unwrap());
    static RE_CONCERNS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?si)<remaining-concerns>(.*?)</remaining-concerns>").unwrap()
    });

    let report_block = RE_REPORT.captures(response)?.get(1)?.as_str().trim();

    if report_block.is_empty() {
        return None;
    }

    let findings = RE_FINDINGS
        .captures(report_block)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();
    let fixes_applied = RE_FIXES
        .captures(report_block)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();
    let remaining_concerns = RE_CONCERNS
        .captures(report_block)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_default();

    // If at least one sub-tag was found, use structured data
    if !findings.is_empty() {
        return Some(ParsedReviewReport {
            findings,
            fixes_applied,
            remaining_concerns,
        });
    }

    // Lenient fallback: no sub-tags, use raw content as findings
    tracing::debug!(
        action = "review_report_lenient_parse",
        "No sub-tags found in <review-report> — using raw content as findings"
    );
    Some(ParsedReviewReport {
        findings: report_block.to_string(),
        fixes_applied: String::new(),
        remaining_concerns: String::new(),
    })
}

/// Build a formatted markdown review comment from a parsed report.
///
/// Produces a clean, structured PR comment with consistent formatting.
/// This is the review equivalent of `build_pr_description()` for dev sessions.
#[allow(dead_code)]
pub fn build_review_comment(story_key: &str, report: &ParsedReviewReport) -> String {
    let mut body = String::new();

    body.push_str(&format!("## 🔍 Code Review Report — {story_key}\n\n"));

    // Findings section
    let findings = if report.findings.is_empty() {
        "No findings."
    } else {
        &report.findings
    };
    body.push_str(&format!("### Findings\n\n{findings}\n\n"));

    // Fixes Applied section
    let fixes = if report.fixes_applied.is_empty() {
        "No fixes needed."
    } else {
        &report.fixes_applied
    };
    body.push_str(&format!("### Fixes Applied\n\n{fixes}\n\n"));

    // Remaining Concerns section
    let concerns = if report.remaining_concerns.is_empty() {
        "None."
    } else {
        &report.remaining_concerns
    };
    body.push_str(&format!("### Remaining Concerns\n\n{concerns}\n\n"));

    // Footer
    body.push_str("---\n*Generated by [bmad-bot](https://github.com/jbanety/bmad-bot)*\n");

    body
}

/// Fallback review comment when the agent's response could not be parsed.
///
/// Uses `strip_agent_artifacts()` to clean the raw response as best-effort.
#[allow(dead_code)]
fn build_fallback_review_comment(story_key: &str, raw_response: &str) -> String {
    use crate::session::analyzer::strip_agent_artifacts;
    let cleaned = strip_agent_artifacts(raw_response);
    format!(
        "## 🔍 Code Review Report — {story_key}\n\n\
        {cleaned}\n\n\
        ---\n*Generated by [bmad-bot](https://github.com/jbanety/bmad-bot)*\n"
    )
}

/// Errors originating from the review module.
///
/// Each variant carries structured context for logging and error handling.
/// Uses `thiserror` for `Display` and `Error` derive — no `anyhow` in this module.
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
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
}

/// Result of a code review session.
///
/// Returned by [`ReviewRunner::run`]. The orchestrator handles each variant:
/// - [`Completed`](ReviewOutcome::Completed) → post report as PR comment
/// - [`Failed`](ReviewOutcome::Failed) → log error, proceed to PR creation anyway
/// - [`Skipped`](ReviewOutcome::Skipped) → proceed to PR creation, note skip in description
#[derive(Debug)]
#[allow(dead_code)]
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
/// Manages code review lifecycle: build agent, run CR workflow, capture report.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ReviewRunner {
    /// Shared bot configuration.
    config: Arc<BotConfig>,
    /// Shared secrets (API keys loaded from `.env`).
    secrets: Arc<BotSecrets>,
    /// Centralized agent construction factory.
    agent_factory: Arc<AgentFactory>,
    /// Stateless response analyzer (constructed once, reused).
    analyzer: ResponseAnalyzer,
    /// Cooperative shutdown flag — checked between streaming chunks and chat turns.
    shutdown: ShutdownFlag,
    /// MCP server manager — provides external tool capabilities (Story 9.2 usage).
    mcp_manager: Arc<crate::mcp::McpManager>,
    /// Shared sub-agent session map — threaded into `SpawnAgentTool` at
    /// `build_review_agent` time (Story 12.4).
    sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>>,
    /// Shared in-flight-follow-up set — prevents concurrent follow-ups on the same
    /// sub-agent session (Story 12.4).
    sub_agent_in_flight: Arc<Mutex<HashSet<String>>>,
    /// UI handle for rendering terminal output (fire-and-forget, Story 10.2).
    ui: crate::ui::UiHandle,
}

impl ReviewRunner {
    /// Create a new review runner.
    // 8 args: construction aggregates daemon-wide shared state; splitting into a
    // builder would add a layer of indirection without hiding any one parameter.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<BotConfig>,
        secrets: Arc<BotSecrets>,
        agent_factory: Arc<AgentFactory>,
        shutdown: ShutdownFlag,
        mcp_manager: Arc<crate::mcp::McpManager>,
        sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>>,
        sub_agent_in_flight: Arc<Mutex<HashSet<String>>>,
        ui: crate::ui::UiHandle,
    ) -> Self {
        Self {
            config,
            secrets,
            agent_factory,
            analyzer: ResponseAnalyzer::new(),
            shutdown,
            mcp_manager,
            sub_agent_sessions,
            sub_agent_in_flight,
            ui,
        }
    }

    /// Run a code review session for the given story.
    ///
    /// This method NEVER panics or returns an unhandled error. All failures
    /// are caught and returned as [`ReviewOutcome::Skipped`] or [`ReviewOutcome::Failed`].
    ///
    /// **Retry logic:** If the session fails due to a malformed tool call (rig/API bug
    /// where concatenated JSON args poison the conversation history), the entire session
    /// is retried from scratch with a clean agent and history.
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        for attempt in 0..=MAX_SESSION_RETRIES {
            match self.run_inner(story).await {
                Ok(outcome) => return outcome,
                Err(e) => {
                    let error_str = e.to_string();
                    let is_retryable = is_retryable_review_error(&error_str);

                    if is_retryable && attempt < MAX_SESSION_RETRIES {
                        tracing::warn!(
                            action = "review_retry",
                            attempt = attempt + 1,
                            max_retries = MAX_SESSION_RETRIES,
                            error = %error_str,
                            story_key = %story.story_key,
                            "Transient error — retrying with fresh session"
                        );
                        self.ui.llm_retry("review", 0, (attempt + 1) as u32, 0.0);
                        continue;
                    }

                    tracing::error!(
                        action = "review_failed",
                        error = %e,
                        story_key = %story.story_key,
                        attempt = attempt + 1,
                        "Code review failed — skipping"
                    );
                    return ReviewOutcome::Skipped {
                        reason: e.to_string(),
                    };
                }
            }
        }

        // Unreachable — the loop always returns on the last attempt
        ReviewOutcome::Skipped {
            reason: "Review exhausted all retries".to_string(),
        }
    }
}

/// Check if a review error is transient and worth retrying with a fresh session.
///
/// Covers two categories:
/// 1. **Poisoned history** — malformed tool call args that make the entire
///    conversation history invalid (rig/API bug workaround)
/// 2. **Network errors** — transient HTTP failures (connection reset, timeout,
///    incomplete message) that are likely to succeed on retry
pub fn is_retryable_review_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    // Poisoned history (rig sends malformed tool call args in conversation history)
    lower.contains("invalid_tool_call_format")
        || lower.contains("not in a valid json format")
        // Transient network errors
        || lower.contains("incompletemessage")
        || lower.contains("connection reset")
        || lower.contains("connection refused")
        || lower.contains("broken pipe")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("error sending request")
        || lower.contains("eof while parsing")
        // Transient API errors
        || lower.contains("status code 429")
        || lower.contains("status code 500")
        || lower.contains("status code 502")
        || lower.contains("status code 503")
        || lower.contains("status code 504")
}

impl ReviewRunner {
    /// Build a fresh [`BuiltAgent`] for review, reusing existing shared resources.
    ///
    /// Used both for initial agent construction and for token refresh rebuilds.
    /// The `escalation_slot` and `decision_log` are passed through so the new
    /// agent's supervisor tool shares state with the existing session.
    async fn build_review_agent(
        &self,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
    ) -> Result<BuiltAgent, ReviewError> {
        let mcp_data = self.mcp_manager.tools_for_builder().await;
        let mcp_tool_names = crate::mcp::extract_mcp_tool_names(&mcp_data);
        let preamble = agent::build_preamble(&mcp_tool_names, &self.config.llm.review.model);

        let project_root = PathBuf::from(&self.config.bmad_paths.project_root);
        let (git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor) =
            agent::create_tools_with_supervisor(
                &project_root,
                &self.config,
                &self.agent_factory,
                escalation_slot,
                decision_log,
                &self.mcp_manager,
            )
            .map_err(|e| ReviewError::AgentBuildFailed {
                reason: format!("Failed to create AskSupervisor: {e}"),
            })?;

        let spawn_agent = agent::create_spawn_agent_tool(
            &self.agent_factory,
            LlmRole::Review,
            &project_root,
            &self.sub_agent_sessions,
            &self.sub_agent_in_flight,
            Some(&self.shutdown),
        );

        self.agent_factory
            .build(
                LlmRole::Review,
                &preamble,
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
            .map_err(|e| match e {
                ProviderError::MissingApiKey { env_var, .. } => ReviewError::ApiKeyMissing {
                    provider: self.config.llm.review.provider.clone(),
                    env_var,
                },
                other => ReviewError::ProviderInit {
                    reason: other.to_string(),
                },
            })
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

        // Create shared resources
        let escalation_slot: EscalationSlot = Arc::new(std::sync::Mutex::new(None));
        let decision_log = DecisionLog::new();

        // Build agent via centralized helper
        let agent = self
            .build_review_agent(escalation_slot.clone(), decision_log.clone())
            .await?;

        // Drive the review session (agent is mutable for token refresh rebuilds)
        let mut agent = agent;
        let outcome = self
            .drive_review_session(&mut agent, story, escalation_slot, decision_log)
            .await?;

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

    /// Drive the review session chat loop.
    ///
    /// Two-phase approach:
    /// 1. **Normal phase:** Send `"CR"`, analyze responses with `ResponseAnalyzer`
    ///    for completion/escalation detection. On `Completed` → enter post-review phase.
    /// 2. **Post-review phase:** Send `POST_REVIEW_MESSAGE`, capture the agent's
    ///    next response as the review report. Return `ReviewOutcome::Completed`.
    async fn drive_review_session(
        &self,
        agent: &mut BuiltAgent,
        story: &StoryInfo,
        escalation_slot: EscalationSlot,
        _decision_log: DecisionLog,
    ) -> Result<ReviewOutcome, ReviewError> {
        // Activate agent: send code review SKILL.md as user message (Story 12.1)
        self.ui.activation_start();
        self.ui.llm_request("review", 0);
        self.ui.llm_request_content("review", 0, "[activate skill]");
        let (activation_rig_history, _activation_chat_history) = agent
            .activate_agent(
                &self.config.bmad_paths.project_root,
                ".claude/skills/bmad-code-review/SKILL.md",
                "review",
                Some(&self.shutdown),
                Some(&self.ui),
            )
            .await
            .map_err(|e| {
                tracing::error!(action = "review_activation_failed", error = %e);
                self.ui
                    .phase_error("Agent Activation", &format!("Agent activation failed: {e}"));
                ReviewError::AgentBuildFailed {
                    reason: format!("Agent activation failed: {e}"),
                }
            })?;
        // Show activation response (agent menu)
        if let Some(last_assistant) = _activation_chat_history
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
        {
            self.ui
                .llm_response("review", 0, last_assistant.content.len());
            self.ui
                .llm_response_content("review", 0, &last_assistant.content);
        }
        self.ui.activation_complete();

        // Send initial story message with English language override (Story 12.1).
        // Skill is self-directing — no [CR] command needed. No branch reminder for review sessions.
        let initial_message = format!(
            "IMPORTANT: ALL communication MUST be in English regardless of config file settings. Story file: {}",
            story.specs_path.display()
        );
        log_llm_request("review", 1, &initial_message, activation_rig_history.len());
        self.ui.llm_request("review", 1);
        self.ui.llm_request_content("review", 1, &initial_message);
        let (response, mut full_history) = agent
            .stream_chat(
                &initial_message,
                activation_rig_history,
                Some(&self.shutdown),
                Some(&self.ui),
            )
            .await
            .map_err(|e| {
                log_llm_error("review", 1, &e);
                self.ui.llm_error("review", 1, &e.to_string());
                ReviewError::ChatFailed {
                    turn: 1,
                    reason: e.to_string(),
                }
            })?;
        log_llm_response("review", 1, &response);
        self.ui.llm_response("review", 1, response.len());
        self.ui.llm_response_content("review", 1, &response);
        self.ui
            .chat_turn(1, &format!("[review] {}", truncate_summary(&response, 80)));

        tracing::debug!(
            action = "review_chat_turn",
            turn = 1,
            response_len = %response.len(),
            "Initial review chat turn completed (post-activation)"
        );

        let mut current_response = response;
        let mut turn: usize = 2;
        let mut retries: usize = 0;
        let mut post_review_phase = false;
        let mut post_review_start: Option<Instant> = None;

        // Safety: check shutdown before entering the loop
        if self.shutdown.load(Ordering::Relaxed) {
            return Ok(ReviewOutcome::Failed {
                story_key: story.story_key.clone(),
                error: "Shutdown requested (Ctrl+C)".to_string(),
            });
        }
        const MAX_RETRIES: usize = 3;

        loop {
            // Cooperative shutdown check — between chat turns
            if self.shutdown.load(Ordering::Relaxed) {
                tracing::info!(
                    action = "shutdown_requested",
                    turn = %turn,
                    story_key = %story.story_key,
                    "Shutdown requested — exiting review session"
                );
                return Ok(ReviewOutcome::Failed {
                    story_key: story.story_key.clone(),
                    error: "Shutdown requested (Ctrl+C)".to_string(),
                });
            }

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

            // Post-review phase: parse the agent's response into a structured report
            if post_review_phase {
                let elapsed = post_review_start.map(|s| s.elapsed()).unwrap_or_default();
                let formatted_report = match parse_review_report(&current_response) {
                    Some(parsed) => {
                        tracing::info!(
                            action = "review_report_parsed",
                            story_key = %story.story_key,
                            findings_len = %parsed.findings.len(),
                            fixes_len = %parsed.fixes_applied.len(),
                            concerns_len = %parsed.remaining_concerns.len(),
                            "Review report parsed successfully — LLM narrative stripped"
                        );
                        build_review_comment(&story.story_key, &parsed)
                    }
                    None => {
                        tracing::warn!(
                            action = "review_report_parse_failed",
                            story_key = %story.story_key,
                            response_len = %current_response.len(),
                            "Could not parse <review-report> — using fallback"
                        );
                        build_fallback_review_comment(&story.story_key, &current_response)
                    }
                };

                self.ui.phase_complete("Post-Review Report", elapsed);
                tracing::info!(
                    action = "review_report_captured",
                    story_key = %story.story_key,
                    report_len = %formatted_report.len(),
                    "Review report captured from agent"
                );

                // ── Dirty worktree safety net ─────────────────────────
                // The review agent may have left uncommitted files (e.g.,
                // touched downstream story files during analysis, or tool
                // calls that modified files after the last commit). If the
                // worktree is dirty, send one more turn so the agent commits
                // and pushes — prevents checkout failures when the pipeline
                // switches branches for the next story.
                let repo_path = PathBuf::from(&self.config.bmad_paths.project_root);
                let dirty_files = get_dirty_files(&repo_path).await;
                if !dirty_files.is_empty() {
                    let file_list = dirty_files.join("\n  - ");
                    let dirty_msg = format!(
                        "WARNING: There are still uncommitted files in the working tree:\n  \
                         - {file_list}\n\n\
                         You MUST commit ALL of these files NOW. Use `git add -A` then \
                         `git commit` with a descriptive conventional commit message \
                         that explains what these files contain. Then push with `git push`."
                    );
                    tracing::warn!(
                        action = "dirty_worktree_detected",
                        file_count = dirty_files.len(),
                        story_key = %story.story_key,
                        step = "post_review",
                        "Uncommitted files remain after review — sending cleanup turn"
                    );
                    let dirty_turn = turn + 1;
                    log_llm_request(
                        "review",
                        dirty_turn,
                        "[dirty-worktree-cleanup]",
                        full_history.len(),
                    );
                    self.ui.llm_request("review", dirty_turn as u32);
                    self.ui
                        .llm_request_content("review", dirty_turn as u32, &dirty_msg);
                    match agent
                        .stream_chat(
                            &dirty_msg,
                            full_history.clone(),
                            Some(&self.shutdown),
                            Some(&self.ui),
                        )
                        .await
                    {
                        Ok((r, _new_hist)) => {
                            log_llm_response("review", dirty_turn, &r);
                            self.ui.llm_response("review", dirty_turn as u32, r.len());
                            self.ui
                                .llm_response_content("review", dirty_turn as u32, &r);
                            tracing::info!(
                                action = "dirty_worktree_cleanup_done",
                                story_key = %story.story_key,
                                "Review dirty worktree cleanup turn completed"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                action = "dirty_worktree_cleanup_failed",
                                error = %e,
                                story_key = %story.story_key,
                                "Review dirty worktree cleanup turn failed — proceeding anyway"
                            );
                        }
                    }
                }

                return Ok(ReviewOutcome::Completed {
                    story_key: story.story_key.clone(),
                    branch: story.branch_name.clone(),
                    report: formatted_report,
                });
            }

            // Normal phase: analyze response
            let action = self.analyzer.analyze(&current_response, &escalation_slot);

            let reply = match action {
                ResponseAction::Completed => {
                    tracing::info!(
                        action = "review_cr_complete",
                        turn = %turn,
                        "CR workflow completion detected — entering post-review phase"
                    );
                    post_review_phase = true;
                    self.ui.phase_start("Post-Review Report");
                    post_review_start = Some(Instant::now());
                    build_post_review_message(&self.config.git_provider.repo_name, &story.story_key)
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

            log_llm_request("review", turn, &reply, full_history.len());
            self.ui.llm_request("review", turn as u32);
            self.ui.llm_request_content("review", turn as u32, &reply);
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
                    log_llm_response("review", turn, &r);
                    self.ui.llm_response("review", turn as u32, r.len());
                    self.ui.llm_response_content("review", turn as u32, &r);
                    retries = 0;
                    full_history = new_hist;
                    current_response = r;
                }
                Err(e) => {
                    log_llm_error("review", turn, &e);
                    self.ui.llm_error("review", turn as u32, &e.to_string());
                    let error_str = e.to_string();

                    retries += 1;
                    tracing::warn!(
                        action = "review_chat_error",
                        turn = %turn,
                        retries = %retries,
                        error = %error_str,
                        "Review chat error, will retry"
                    );
                    self.ui
                        .llm_retry("review", turn as u32, retries as u32, 0.0);
                    if retries >= MAX_RETRIES {
                        if post_review_phase {
                            self.ui.phase_error(
                                "Post-Review Report",
                                &format!("Chat failed after {MAX_RETRIES} retries: {error_str}"),
                            );
                        }
                        return Ok(ReviewOutcome::Failed {
                            story_key: story.story_key.clone(),
                            error: format!(
                                "Review chat failed after {MAX_RETRIES} retries: {error_str}"
                            ),
                        });
                    }
                    continue;
                }
            }

            self.ui.chat_turn(
                turn as u32,
                &format!("[review] {}", truncate_summary(&current_response, 80)),
            );

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
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });

        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let agent_factory = Arc::new(AgentFactory::new(config.clone(), secrets.clone()));
        let mcp_manager = Arc::new(crate::mcp::McpManager::empty());
        let sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let sub_agent_in_flight: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        let runner = ReviewRunner::new(
            config.clone(),
            secrets,
            agent_factory,
            shutdown,
            mcp_manager,
            sub_agent_sessions,
            sub_agent_in_flight,
            crate::ui::UiHandle::null(),
        );
        // Verify config is stored by checking a known field
        assert_eq!(runner.config.llm.review.provider, "anthropic");
    }

    #[test]
    fn test_review_runner_stores_sub_agent_arcs() {
        let config = Arc::new(BotConfig::_test_minimal("pretty", "info"));
        let secrets = Arc::new(BotSecrets {
            anthropic_api_key: Some("sk-test".to_string()),
            openai_api_key: None,
            github_token: None,
            gitlab_token: None,
            telegram_bot_token: None,
        });
        let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let agent_factory = Arc::new(AgentFactory::new(config.clone(), secrets.clone()));
        let mcp_manager = Arc::new(crate::mcp::McpManager::empty());
        let sessions: Arc<Mutex<HashMap<String, SubAgentState>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let in_flight: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        assert_eq!(Arc::strong_count(&sessions), 1);
        assert_eq!(Arc::strong_count(&in_flight), 1);

        let runner = ReviewRunner::new(
            config,
            secrets,
            agent_factory,
            shutdown,
            mcp_manager,
            Arc::clone(&sessions),
            Arc::clone(&in_flight),
            crate::ui::UiHandle::null(),
        );

        // >= 2, not == 2: internal cloning is an implementation detail.
        assert!(Arc::strong_count(&sessions) >= 2);
        assert!(Arc::strong_count(&in_flight) >= 2);
        drop(runner);
        assert_eq!(Arc::strong_count(&sessions), 1);
        assert_eq!(Arc::strong_count(&in_flight), 1);
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
        let msg = build_post_review_message("bmad-bot", "7-1-integration-tests");
        assert!(msg.contains("Commit all your review fixes"));
        assert!(msg.contains("commit messages"));
        assert!(msg.contains("<findings>"));
        assert!(msg.contains("<fixes-applied>"));
        assert!(msg.contains("<remaining-concerns>"));
        assert!(msg.contains("<review-report>"));
        assert!(msg.contains("Do NOT add any commentary"));
    }

    #[test]
    fn test_post_review_message_includes_project_and_story() {
        let msg = build_post_review_message("bmad-bot", "7-1-integration-tests");
        assert!(msg.contains("bmad-bot"), "Should contain project name");
        assert!(
            msg.contains("7-1-integration-tests"),
            "Should contain story key"
        );
    }

    // -----------------------------------------------------------------------
    // parse_review_report tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_review_report_valid() {
        let input = r#"Now let me verify all tests pass. All 1035 tests pass.

<review-report>
<findings>
1. **Low** — `test_config_load` used hardcoded path instead of tempdir
2. **Medium** — Missing error handling in `parse_config`
</findings>
<fixes-applied>
1. Replaced hardcoded path with `tempfile::tempdir()` (commit abc123)
2. Added `?` propagation in `parse_config` (commit def456)
</fixes-applied>
<remaining-concerns>
None.
</remaining-concerns>
</review-report>"#;
        let result = parse_review_report(input);
        assert!(result.is_some(), "Should parse valid review report");
        let report = result.unwrap();
        assert!(report.findings.contains("hardcoded path"));
        assert!(report.findings.contains("Missing error handling"));
        assert!(report.fixes_applied.contains("commit abc123"));
        assert!(report.fixes_applied.contains("commit def456"));
        assert_eq!(report.remaining_concerns, "None.");
    }

    #[test]
    fn test_parse_review_report_strips_llm_narrative() {
        let input = "Now let me implement the fixes:\n\
            Now let me verify all tests pass:\n\
            All 990 tests pass. Let me commit.\n\n\
            <review-report>\n\
            <findings>\n\
            1. **Low** — unused import\n\
            </findings>\n\
            <fixes-applied>\n\
            1. Removed import (commit abc)\n\
            </fixes-applied>\n\
            <remaining-concerns>\n\
            None.\n\
            </remaining-concerns>\n\
            </review-report>\n\n\
            That concludes the review.";
        let result = parse_review_report(input);
        assert!(result.is_some());
        let report = result.unwrap();
        // LLM narrative must NOT appear in the parsed report
        assert!(!report.findings.contains("Now let me"));
        assert!(!report.findings.contains("All 990 tests"));
        assert!(!report.fixes_applied.contains("That concludes"));
        assert!(report.findings.contains("unused import"));
    }

    #[test]
    fn test_parse_review_report_lenient_fallback() {
        let input = "<review-report>\n\
            Clean code, no issues found. All tests pass.\n\
            </review-report>";
        let result = parse_review_report(input);
        assert!(result.is_some(), "Should parse with lenient fallback");
        let report = result.unwrap();
        assert!(report.findings.contains("Clean code"));
        assert!(report.fixes_applied.is_empty());
        assert!(report.remaining_concerns.is_empty());
    }

    #[test]
    fn test_parse_review_report_no_outer_tag_returns_none() {
        let input = "## Code Review Report\n\nFindings: none.";
        assert!(
            parse_review_report(input).is_none(),
            "Should return None without <review-report> tag"
        );
    }

    #[test]
    fn test_parse_review_report_empty_tag_returns_none() {
        let input = "<review-report></review-report>";
        assert!(
            parse_review_report(input).is_none(),
            "Should return None for empty <review-report>"
        );
    }

    #[test]
    fn test_parse_review_report_garbage_input() {
        assert!(parse_review_report("random text").is_none());
    }

    #[test]
    fn test_parse_review_report_empty_input() {
        assert!(parse_review_report("").is_none());
    }

    #[test]
    fn test_parse_review_report_partial_subtags() {
        let input = "<review-report>\n\
            <findings>\n\
            1. **High** — SQL injection vulnerability\n\
            </findings>\n\
            </review-report>";
        let result = parse_review_report(input);
        assert!(result.is_some());
        let report = result.unwrap();
        assert!(report.findings.contains("SQL injection"));
        assert!(report.fixes_applied.is_empty());
        assert!(report.remaining_concerns.is_empty());
    }

    // -----------------------------------------------------------------------
    // build_review_comment tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_review_comment_full_report() {
        let report = ParsedReviewReport {
            findings: "1. **Low** — unused import".to_string(),
            fixes_applied: "1. Removed import (commit abc)".to_string(),
            remaining_concerns: "None.".to_string(),
        };
        let comment = build_review_comment("7-1-test", &report);
        assert!(comment.contains("## 🔍 Code Review Report — 7-1-test"));
        assert!(comment.contains("### Findings"));
        assert!(comment.contains("unused import"));
        assert!(comment.contains("### Fixes Applied"));
        assert!(comment.contains("Removed import"));
        assert!(comment.contains("### Remaining Concerns"));
        assert!(comment.contains("None."));
        assert!(comment.contains("bmad-bot"));
    }

    #[test]
    fn test_build_review_comment_empty_sections_use_defaults() {
        let report = ParsedReviewReport {
            findings: String::new(),
            fixes_applied: String::new(),
            remaining_concerns: String::new(),
        };
        let comment = build_review_comment("7-1-test", &report);
        assert!(comment.contains("No findings."));
        assert!(comment.contains("No fixes needed."));
        // Remaining Concerns empty → "None."
        assert!(comment.contains("None."));
    }

    #[test]
    fn test_build_review_comment_has_footer() {
        let report = ParsedReviewReport {
            findings: "x".to_string(),
            fixes_applied: String::new(),
            remaining_concerns: String::new(),
        };
        let comment = build_review_comment("1-1-test", &report);
        assert!(comment.contains("Generated by [bmad-bot]"));
    }

    // -----------------------------------------------------------------------
    // build_fallback_review_comment tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_fallback_review_comment_contains_cleaned_content() {
        let raw = "Some review.\n\n<<BMAD_JOB_DONE>>";
        let comment = build_fallback_review_comment("7-1-test", raw);
        assert!(comment.contains("## 🔍 Code Review Report — 7-1-test"));
        assert!(comment.contains("Some review."));
        assert!(!comment.contains("BMAD_JOB_DONE"));
        assert!(comment.contains("bmad-bot"));
    }

    // -----------------------------------------------------------------------
    // Constants tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_max_review_turns_is_reasonable() {
        assert!(MAX_REVIEW_TURNS >= 50, "Max turns should be at least 50");
        assert!(MAX_REVIEW_TURNS <= 200, "Max turns should be at most 200");
    }
}
