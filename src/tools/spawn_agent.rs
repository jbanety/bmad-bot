//! SpawnAgentTool — LLM-initiated delegation of well-scoped subtasks to fresh sub-agents.
//!
//! Inspired by the Zed assistant's `spawn_agent` pattern, this rig tool lets the parent
//! LLM hand off a self-contained task to a brand-new sub-agent that runs on the same
//! provider and model as the parent's [`LlmRole`]. The sub-agent has no visibility into
//! the parent's conversation history — the parent must include all necessary context in
//! the `message` field of [`SpawnAgentArgs`].
//!
//! # Contrast with daemon-orchestrated consultations
//!
//! Per architecture.md Decision 10, BMAD distinguishes two delegation flavors:
//! - **Daemon-orchestrated consultations** (e.g., the Story Critic): scheduled by the
//!   pipeline, run between phases, results flow back into pipeline state.
//! - **LLM-initiated `spawn_agent`** (this tool): triggered mid-session by the LLM
//!   itself when it judges a task too narrow or context-pollutive to handle inline.
//!
//! # Shared-state design
//!
//! The tool holds an `Arc<Mutex<HashMap<String, SubAgentState>>>` so multiple parent
//! sessions (and follow-up calls within the same parent) can resume sub-agent
//! conversations by passing back a previously-returned `session_id`. Story 12.4 wires
//! the map's construction into the pipeline and registers this tool in
//! [`SessionRunner`](crate::session::runner::SessionRunner) and
//! [`ReviewRunner`](crate::review::ReviewRunner) via the
//! [`create_spawn_agent_tool`](crate::session::agent::create_spawn_agent_tool) helper.
//! [`EpicReviewRunner`](crate::review::epic::EpicReviewRunner) and
//! [`ArchitectSession`](crate::supervisor::architect::ArchitectSession) are deliberately
//! excluded — see Story 12.4 AC-7.
//!
//! # Error policy (AC-5)
//!
//! Sub-agent execution failures (timeouts, 429, context overflow) return
//! `Ok(build_error_json(...))` rather than `Err(SpawnAgentError)`. This is intentional:
//! a failure inside the sub-agent is a recoverable signal the parent LLM can read and
//! react to. Returning `Err` would poison rig's tool-call loop with an opaque error.
//! `SpawnAgentError` is reserved for infrastructure failures only (missing session,
//! agent build failure).

use crate::llm::agent_factory::{AgentFactory, BuiltAgent, LlmRole, ShutdownFlag};
use crate::session::agent::{build_sub_agent_preamble, create_base_tools};
use rig::completion::{Message, ToolDefinition};
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

/// Maximum label length in log lines. LLM-controlled input is truncated past this to
/// prevent log bloat and injection via newline-laced labels.
const LABEL_LOG_MAX_LEN: usize = 200;

/// Truncate and strip control characters from a freeform label before emitting into
/// structured logs. Keeps logs readable even when the parent LLM emits unusually long
/// or control-character-laced labels.
fn sanitize_label(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if cleaned.len() <= LABEL_LOG_MAX_LEN {
        cleaned
    } else {
        // Truncate at a char boundary at-or-before LABEL_LOG_MAX_LEN.
        let mut cut = LABEL_LOG_MAX_LEN;
        while !cleaned.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &cleaned[..cut])
    }
}

// ---------------------------------------------------------------------------
// Args / Error / State types
// ---------------------------------------------------------------------------

/// Arguments passed by the parent LLM when invoking the `spawn_agent` tool.
#[derive(Debug, Deserialize)]
pub struct SpawnAgentArgs {
    /// Short human-readable identifier for this spawn event (e.g., `"audit module X"`).
    /// Used only for structured logging — never sent to the sub-agent or returned to the parent.
    pub label: String,
    /// The full self-contained instruction to send to the sub-agent. Must include any
    /// context (file paths, snippets, prior findings) the sub-agent needs — sub-agents
    /// do NOT see the parent's conversation history.
    pub message: String,
    /// Optional session id from a previous spawn — when present, the existing sub-agent
    /// continues its conversation with `message` appended to its history.
    pub session_id: Option<String>,
}

/// Infrastructure errors returned by [`SpawnAgentTool`].
///
/// Sub-agent *execution* failures (provider timeouts, 429s, context overflow) are NOT
/// returned via this enum — they are surfaced as `Ok(build_error_json(...))` so the
/// parent LLM can inspect and react. See module-level docs.
#[derive(Debug, thiserror::Error)]
pub enum SpawnAgentError {
    /// The parent passed a `session_id` that no longer exists in the sessions map.
    #[error("No sub-agent session found for id: {session_id}")]
    SessionNotFound {
        /// The session id the parent attempted to resume.
        session_id: String,
    },

    /// Building the sub-agent via [`AgentFactory::build()`] failed (bad API key,
    /// unsupported provider, client-construction error, etc.).
    #[error("Failed to build sub-agent: {reason}")]
    AgentBuildFailed {
        /// Reason from the underlying [`crate::session::provider::ProviderError`].
        reason: String,
    },

    /// A concurrent follow-up is already in flight for the same `session_id`.
    ///
    /// The parent LLM may retry after the in-flight call completes, or spawn a fresh
    /// sub-agent. Serializing follow-ups on the same session preserves history
    /// integrity — two simultaneous `stream_chat`s would otherwise produce divergent
    /// histories under a last-writer-wins re-insert.
    #[error("Sub-agent session is busy with another in-flight follow-up: {session_id}")]
    SessionBusy {
        /// The session id whose follow-up is already executing.
        session_id: String,
    },
}

/// In-memory state for a live sub-agent session.
///
/// **Lifecycle:** dropped naturally when the owning sessions [`HashMap`] is dropped.
/// Story 12.4 owns the map's lifetime (one map per parent story pipeline). This story
/// (12.3) creates a fresh map per [`SpawnAgentTool`] under test.
///
/// **Never serialized.** No serde traits — this is in-memory-only state.
#[derive(Debug)]
pub struct SubAgentState {
    /// The built sub-agent, reusable across follow-up calls (`stream_chat` takes `&self`).
    pub agent: BuiltAgent,
    /// Full conversation history including tool calls and tool results.
    pub history: Vec<Message>,
    /// LLM role copied from the parent at construction time — kept for future logging
    /// and to preserve the provider/model pairing across follow-ups.
    // Preserved for follow-up introspection/logging; no runtime reader today.
    #[allow(dead_code)]
    pub role: LlmRole,
    /// Model id resolved at construction time — preserved across follow-ups.
    // Preserved for follow-up introspection/logging; no runtime reader today.
    #[allow(dead_code)]
    pub model: String,
}

// ---------------------------------------------------------------------------
// SpawnAgentTool
// ---------------------------------------------------------------------------

/// rig tool that lets the parent LLM spawn fresh sub-agents for delegated work.
///
/// **Derives only `Debug`** — not `Serialize`/`Deserialize`. Rig's [`Tool`] trait
/// does not require either. Adding them here would require scaffolding
/// (`Arc<AgentFactory>` has no `Default` for `#[serde(skip)]`, and `LlmRole` has no
/// serde derives) for zero runtime benefit. See Story 12.3 AC-6 for the full
/// rationale and contrast with the [`crate::supervisor::AskSupervisor`] precedent.
#[derive(Debug)]
pub struct SpawnAgentTool {
    /// Shared daemon factory used to build each sub-agent.
    agent_factory: Arc<AgentFactory>,
    /// LLM role inherited from the parent session (provider + model pairing).
    role: LlmRole,
    /// Project root passed to [`create_base_tools`] for every sub-agent.
    project_root: PathBuf,
    /// Shared map of live sub-agent sessions, keyed by UUID. Story 12.4 owns the map.
    sessions: Arc<Mutex<HashMap<String, SubAgentState>>>,
    /// Session ids whose follow-up is currently streaming. Used to reject a concurrent
    /// second follow-up on the same id with [`SpawnAgentError::SessionBusy`] instead
    /// of racing on remove/insert and corrupting history.
    in_flight: Arc<Mutex<HashSet<String>>>,
    /// Optional cooperative shutdown flag forwarded into sub-agent `stream_chat` calls
    /// so SIGINT/SIGTERM propagates from parent → sub-agent loops.
    shutdown: Option<ShutdownFlag>,
}

impl SpawnAgentTool {
    /// Construct a new `SpawnAgentTool`.
    ///
    /// # Arguments
    /// - `agent_factory` — daemon-shared factory used to build sub-agents
    /// - `role` — parent session's [`LlmRole`]; sub-agents use the same provider/model
    /// - `project_root` — used to construct the 7 base tools for each sub-agent
    /// - `sessions` — shared map for follow-up continuity (Story 12.4 owns lifecycle)
    /// - `in_flight` — shared in-flight follow-up set for concurrent-follow-up rejection
    ///   (Story 12.4 owns the set's lifetime alongside `sessions`)
    /// - `shutdown` — optional cooperative shutdown flag forwarded into `stream_chat`
    pub fn new(
        agent_factory: Arc<AgentFactory>,
        role: LlmRole,
        project_root: PathBuf,
        sessions: Arc<Mutex<HashMap<String, SubAgentState>>>,
        in_flight: Arc<Mutex<HashSet<String>>>,
        shutdown: Option<ShutdownFlag>,
    ) -> Self {
        Self {
            agent_factory,
            role,
            project_root,
            sessions,
            in_flight,
            shutdown,
        }
    }

    // Test-only accessor — private role field is otherwise inaccessible for
    // cross-module assertions (Story 12.4 AC-3).
    #[cfg(test)]
    pub(crate) fn role_for_tests(&self) -> LlmRole {
        self.role
    }

    /// Lock the shared sessions map, recovering silently from poison.
    ///
    /// On poison, logs at `error` level and returns the inner guard via
    /// [`std::sync::PoisonError::into_inner`]. Killing the daemon on transient panic
    /// is worse than proceeding with possibly-stale data — see Story 12.3 AC-6.
    fn lock_sessions(&self) -> MutexGuard<'_, HashMap<String, SubAgentState>> {
        self.sessions.lock().unwrap_or_else(|poisoned| {
            tracing::error!(
                action = "spawn_agent_mutex_poisoned",
                "Sub-agent sessions mutex was poisoned — recovering inner data"
            );
            poisoned.into_inner()
        })
    }

    /// Lock the in-flight set, recovering silently from poison (same policy as
    /// [`Self::lock_sessions`]).
    fn lock_in_flight(&self) -> MutexGuard<'_, HashSet<String>> {
        self.in_flight.lock().unwrap_or_else(|poisoned| {
            tracing::error!(
                action = "spawn_agent_in_flight_mutex_poisoned",
                "Sub-agent in-flight mutex was poisoned — recovering inner data"
            );
            poisoned.into_inner()
        })
    }
}

// ---------------------------------------------------------------------------
// RAII guards for safe in_flight + state lifecycle across .await and panics
// ---------------------------------------------------------------------------

/// RAII guard that removes `id` from the tool's in-flight set when dropped,
/// regardless of whether the follow-up succeeded, failed, or panicked.
struct InFlightGuard<'a> {
    tool: &'a SpawnAgentTool,
    id: String,
}

impl<'a> Drop for InFlightGuard<'a> {
    fn drop(&mut self) {
        let mut set = self.tool.lock_in_flight();
        set.remove(&self.id);
    }
}

/// RAII guard that re-inserts a [`SubAgentState`] into the tool's sessions map when
/// dropped, unless explicitly disarmed. Used by `continue_followup` to recover from
/// panics inside `stream_chat` (AC-3's non-destructive policy extended to the
/// panic path).
///
/// On the normal Ok/Err paths we call [`Self::disarm`] after re-inserting the updated
/// or preserved state manually. Only a panic between `remove` and re-insert triggers
/// the guard's fallback.
struct PanicReinsertGuard<'a> {
    tool: &'a SpawnAgentTool,
    id: String,
    state: Option<SubAgentState>,
    armed: bool,
}

impl<'a> PanicReinsertGuard<'a> {
    fn new(tool: &'a SpawnAgentTool, id: String, state: SubAgentState) -> Self {
        Self {
            tool,
            id,
            state: Some(state),
            armed: true,
        }
    }

    fn state_ref(&self) -> &SubAgentState {
        self.state
            .as_ref()
            .expect("PanicReinsertGuard state accessed after disarm()")
    }

    /// Take ownership of the inner state and disarm the panic-recovery fallback.
    /// The caller is responsible for re-inserting (with either new or preserved history).
    fn disarm_and_take(&mut self) -> SubAgentState {
        self.armed = false;
        self.state
            .take()
            .expect("PanicReinsertGuard::disarm_and_take called twice")
    }
}

impl<'a> Drop for PanicReinsertGuard<'a> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Some(state) = self.state.take() {
            self.tool.lock_sessions().insert(self.id.clone(), state);
            tracing::warn!(
                action = "spawn_agent_followup_panic_recovery",
                session_id = %self.id,
                "Panic during sub-agent follow-up — original state re-inserted for recovery"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Pure JSON helpers (AC-5)
// ---------------------------------------------------------------------------

/// Build the success payload returned to the parent LLM.
///
/// Pure / deterministic — independently testable without any rig or LLM machinery.
/// `serde_json::json!({&str, &str})` cannot fail, so the return type is `String`.
fn build_success_json(session_id: &str, output: &str) -> String {
    json!({ "session_id": session_id, "output": output }).to_string()
}

/// Build the execution-failure payload returned to the parent LLM.
///
/// Same shape as [`build_success_json`] but with an `"error"` key. Used when the
/// sub-agent's `stream_chat` returns `Err` (timeout, 429, context overflow). Pure
/// helper — see Story 12.3 AC-5.
fn build_error_json(session_id: &str, error: &str) -> String {
    json!({ "session_id": session_id, "error": error }).to_string()
}

/// Build the execution-failure payload used when no follow-up-capable session was
/// retained (e.g., a new-session spawn whose `stream_chat` failed — the generated
/// UUID is meaningless to the parent because nothing was inserted in the sessions
/// map). Omits `session_id` so the parent LLM does not mistakenly attempt a follow-up
/// that would return [`SpawnAgentError::SessionNotFound`].
fn build_error_json_no_session(error: &str) -> String {
    json!({ "error": error }).to_string()
}

// ---------------------------------------------------------------------------
// Tool impl
// ---------------------------------------------------------------------------

impl Tool for SpawnAgentTool {
    const NAME: &'static str = "spawn_agent";
    type Error = SpawnAgentError;
    type Args = SpawnAgentArgs;
    type Output = String;

    /// Returns the tool definition with a description that drives delegation behavior.
    ///
    /// Description quality directly drives whether the LLM uses `spawn_agent` correctly
    /// (architecture.md:802). The description below covers when to use, when NOT to
    /// use, the five Zed-style guidelines, the output schema, and the `label` purpose.
    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "spawn_agent".to_string(),
            description: SPAWN_AGENT_DESCRIPTION.to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "label": {
                        "type": "string",
                        "description": "Short human-readable identifier for this spawn event (e.g., \"audit module X\"). Used for structured logging only — never sent to the sub-agent."
                    },
                    "message": {
                        "type": "string",
                        "description": "The full self-contained instruction for the sub-agent. Include all context the sub-agent needs (file paths, snippets, goals) — sub-agents do not see your conversation history."
                    },
                    "session_id": {
                        "type": "string",
                        "description": "Optional session id from a previous spawn. When present, the existing sub-agent continues with this message; when omitted, a fresh sub-agent is spawned."
                    }
                },
                "required": ["label", "message"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Treat an explicit empty string as "no follow-up" — the parent LLM occasionally
        // passes `session_id: ""` when it means to spawn fresh. Returning SessionNotFound
        // in that case is a UX footgun.
        let follow_up_id = args
            .session_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        tracing::info!(
            action = "spawn_agent_start",
            label = %sanitize_label(&args.label),
            follow_up = follow_up_id.is_some(),
            role = ?self.role,
            "Spawning sub-agent"
        );

        match follow_up_id {
            None => self.spawn_new(args).await,
            Some(id) => self.continue_followup(id, args).await,
        }
    }
}

impl SpawnAgentTool {
    /// New-session path — build a fresh sub-agent and run the message once. (AC-2)
    async fn spawn_new(&self, args: SpawnAgentArgs) -> Result<String, SpawnAgentError> {
        let model = self
            .agent_factory
            .config_for_role(self.role)
            .model
            .clone();
        let preamble = build_sub_agent_preamble(&model);
        let (git, read_file, edit_file, grep, find_path, list_dir, terminal) =
            create_base_tools(&self.project_root);

        let agent = self
            .agent_factory
            .build(
                self.role,
                &preamble,
                crate::configure_agent_tools!(
                    git,
                    read_file,
                    edit_file,
                    grep,
                    find_path,
                    list_dir,
                    terminal,
                    rig::tools::think::ThinkTool
                ),
            )
            .await
            .map_err(|e| SpawnAgentError::AgentBuildFailed {
                reason: e.to_string(),
            })?;

        let session_id = uuid::Uuid::new_v4().to_string();

        // NOTE: effective turn cap is STREAMING_MAX_TURNS (300) from session/agent.rs.
        // Epic 12.3 AC's "default 100 turns" is intentionally deferred — refactoring
        // streaming_chat() to accept a per-call cap is out-of-scope for this story.
        let result = agent
            .stream_chat(
                Message::user(args.message.clone()),
                vec![],
                self.shutdown.as_ref(),
                None,
            )
            .await;

        match result {
            Ok((text, history)) => {
                let history_len = history.len();
                self.lock_sessions().insert(
                    session_id.clone(),
                    SubAgentState {
                        agent,
                        history,
                        role: self.role,
                        model,
                    },
                );
                tracing::info!(
                    action = "spawn_agent_complete",
                    session_id = %session_id,
                    label = %sanitize_label(&args.label),
                    output_len = text.len(),
                    history_len,
                    "Sub-agent completed"
                );
                Ok(build_success_json(&session_id, &text))
            }
            Err(prompt_error) => {
                // The sub-agent never produced any state — nothing was inserted into the
                // sessions map. Emit a session-less error so the parent LLM does not
                // attempt a follow-up on a UUID that would return SessionNotFound.
                tracing::warn!(
                    action = "spawn_agent_exec_failed",
                    label = %sanitize_label(&args.label),
                    error = %prompt_error,
                    "Sub-agent execution failed"
                );
                Ok(build_error_json_no_session(&prompt_error.to_string()))
            }
        }
    }

    /// Follow-up-session path — atomically remove existing state, re-run with a NEW
    /// user message appended to the prior history, and re-insert under the SAME id.
    ///
    /// **Concurrent-follow-up rejection:** a second follow-up on an id whose first
    /// follow-up is still streaming returns [`SpawnAgentError::SessionBusy`]. Two
    /// simultaneous `stream_chat`s on the same session would otherwise produce
    /// divergent histories under a last-writer-wins re-insert.
    ///
    /// **Non-destructive on error (AC-3):** if `stream_chat` returns `Err`, the ORIGINAL
    /// state (history untouched) is re-inserted so the parent LLM can retry after a
    /// transient 429/timeout.
    ///
    /// **Non-destructive on panic:** a panic between `remove` and re-insert would
    /// normally drop the session silently. A [`PanicReinsertGuard`] re-inserts the
    /// original state during unwind so the session survives (AC-3 extended to panics).
    async fn continue_followup(
        &self,
        id: String,
        args: SpawnAgentArgs,
    ) -> Result<String, SpawnAgentError> {
        // 1. Reserve the in-flight slot. Fail fast on a concurrent second follow-up.
        //    The guard removes the id on any exit path, including panic.
        let _in_flight_guard = {
            let mut set = self.lock_in_flight();
            if !set.insert(id.clone()) {
                return Err(SpawnAgentError::SessionBusy {
                    session_id: id,
                });
            }
            InFlightGuard {
                tool: self,
                id: id.clone(),
            }
        };

        // 2. Atomically take the session state. Guard dropped before .await.
        let state = {
            let mut guard = self.lock_sessions();
            guard
                .remove(&id)
                .ok_or_else(|| SpawnAgentError::SessionNotFound {
                    session_id: id.clone(),
                })?
        };

        // 3. Wrap the state in a panic-recovery guard. Accessors return references
        //    so we never move the state out before the await completes.
        let mut guard = PanicReinsertGuard::new(self, id.clone(), state);

        // 4. Clone history for streaming (preserves the original for error/panic recovery).
        let history_for_stream = guard.state_ref().history.clone();

        // 5. Stream via a borrow of the agent held inside the guard.
        let result = guard
            .state_ref()
            .agent
            .stream_chat(
                Message::user(args.message.clone()),
                history_for_stream,
                self.shutdown.as_ref(),
                None,
            )
            .await;

        match result {
            Ok((text, new_history)) => {
                let mut state = guard.disarm_and_take();
                state.history = new_history;
                let history_len = state.history.len();
                self.lock_sessions().insert(id.clone(), state);
                tracing::info!(
                    action = "spawn_agent_followup_complete",
                    session_id = %id,
                    label = %sanitize_label(&args.label),
                    output_len = text.len(),
                    history_len,
                    "Sub-agent follow-up completed"
                );
                Ok(build_success_json(&id, &text))
            }
            Err(prompt_error) => {
                // Preserve original state (history untouched) for parent-LLM retry.
                let state = guard.disarm_and_take();
                self.lock_sessions().insert(id.clone(), state);
                tracing::warn!(
                    action = "spawn_agent_followup_failed",
                    session_id = %id,
                    label = %sanitize_label(&args.label),
                    error = %prompt_error,
                    "Sub-agent follow-up failed — session preserved for retry"
                );
                Ok(build_error_json(&id, &prompt_error.to_string()))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Description constant (AC-4)
// ---------------------------------------------------------------------------

/// Description string for the `spawn_agent` tool — intentionally verbose because
/// description quality directly shapes the LLM's delegation decisions.
///
/// Required substrings (asserted by `test_spawn_agent_definition_meets_quality_bar`):
/// - Guideline 1: `"include all relevant context"` AND `"sub-agents do not see"`
/// - Guideline 2: `"self-contained"` AND `"concrete"`
/// - Guideline 3: `"one or two"` AND `"tool call"`
/// - Guideline 4: `"follow-up"` AND `"session_id"` AND `"short"`
/// - Guideline 5: `"parallel"` AND `"independent"`
/// - Output schemas: `"\"session_id\""`, `"\"output\""`, `"\"error\""`
/// - Label purpose: `"structured logging"`
const SPAWN_AGENT_DESCRIPTION: &str = "\
Spawn an independent sub-agent to handle a well-scoped delegated task. The sub-agent runs on the same provider and model as the current session's role, has its own fresh context, and returns a JSON result you can inspect.

**When to use**: delegate research, parallel investigation, or specialized work that would otherwise pollute your main conversation history.

**When NOT to use**: Do not spawn sub-agents for tasks accomplishable with one or two direct tool calls — the delegation overhead (preamble, cold context, JSON round-trip) is not worth it.

**Guidelines**:
1. Remember that sub-agents do not see your conversation history. You must include all relevant context inside the `message` field — file paths, snippets, prior findings, precise goals. Anything you omit is invisible to the spawn.
2. Keep subtasks concrete, self-contained, and narrowly scoped. A subtask like \"audit module X for unused imports and report the list\" works; \"figure out what to do next\" does not.
3. Do not spawn sub-agents for work you could do yourself in one or two direct tool calls — direct tool call use is faster and cheaper than delegation.
4. To continue a follow-up with an existing sub-agent, pass its `session_id` and send a short direct message that assumes the sub-agent already knows what it was doing. The sub-agent retains its full history across calls.
5. For independent tasks that can run in parallel, spawn multiple sub-agents in parallel — each with its own `label` and self-contained `message`. Do not serialize what can run concurrently.

**Output**:
- Success: `{\"session_id\": \"<uuid>\", \"output\": \"<text>\"}` — store the `session_id` if you expect to follow up.
- Execution failure: `{\"session_id\": \"<uuid>\", \"error\": \"<reason>\"}` — the sub-agent could not finish; adjust your approach and either retry the follow-up or spawn fresh.

**label**: a short human-readable identifier for this spawn event (e.g., `\"audit module X\"`) — used for structured logging so you and the operator can correlate delegated work across trace output. The label is NOT passed to the sub-agent.";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::agent_factory::tests::{make_test_config, make_test_secrets};

    fn test_tool() -> SpawnAgentTool {
        let factory = Arc::new(AgentFactory::new(
            Arc::new(make_test_config()),
            Arc::new(make_test_secrets()),
        ));
        SpawnAgentTool::new(
            factory,
            LlmRole::Dev,
            std::env::temp_dir(),
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashSet::new())),
            None,
        )
    }

    // -- AC-8.1 --------------------------------------------------------------

    #[tokio::test]
    async fn test_spawn_agent_definition_has_name() {
        assert_eq!(SpawnAgentTool::NAME, "spawn_agent");
        let def = test_tool().definition(String::new()).await;
        assert_eq!(def.name, "spawn_agent");
    }

    // -- AC-8.2 --------------------------------------------------------------

    #[tokio::test]
    async fn test_spawn_agent_definition_meets_quality_bar() {
        let def = test_tool().definition(String::new()).await;
        let desc = &def.description;

        assert!(
            desc.len() >= 400,
            "description must be >= 400 chars (got {})",
            desc.len()
        );

        // Guideline 1
        assert!(desc.contains("include all relevant context"), "missing: include all relevant context");
        assert!(desc.contains("sub-agents do not see"), "missing: sub-agents do not see");
        // Guideline 2
        assert!(desc.contains("self-contained"), "missing: self-contained");
        assert!(desc.contains("concrete"), "missing: concrete");
        // Guideline 3
        assert!(desc.contains("one or two"), "missing: one or two");
        assert!(desc.contains("tool call"), "missing: tool call");
        // Guideline 4
        assert!(desc.contains("follow-up"), "missing: follow-up");
        assert!(desc.contains("session_id"), "missing: session_id");
        assert!(desc.contains("short"), "missing: short");
        // Guideline 5
        assert!(desc.contains("parallel"), "missing: parallel");
        assert!(desc.contains("independent"), "missing: independent");
        // JSON shapes (literal quoted keys)
        assert!(desc.contains("\"session_id\""), "missing: \"session_id\"");
        assert!(desc.contains("\"output\""), "missing: \"output\"");
        assert!(desc.contains("\"error\""), "missing: \"error\"");
        // label purpose
        assert!(desc.contains("structured logging"), "missing: structured logging");
    }

    // -- AC-8.3 --------------------------------------------------------------

    #[tokio::test]
    async fn test_spawn_agent_definition_parameters_schema() {
        let def = test_tool().definition(String::new()).await;
        let params = &def.parameters;

        let props = params
            .get("properties")
            .expect("parameters must have a properties object");

        assert_eq!(
            props.get("label").and_then(|v| v.get("type")).and_then(|v| v.as_str()),
            Some("string"),
            "label must be a string"
        );
        assert_eq!(
            props.get("message").and_then(|v| v.get("type")).and_then(|v| v.as_str()),
            Some("string"),
            "message must be a string"
        );
        assert_eq!(
            props.get("session_id").and_then(|v| v.get("type")).and_then(|v| v.as_str()),
            Some("string"),
            "session_id must be a string"
        );

        let required = params
            .get("required")
            .and_then(|v| v.as_array())
            .expect("required must be a JSON array");
        let required_strs: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(required_strs.contains(&"label"), "label must be required");
        assert!(required_strs.contains(&"message"), "message must be required");
        assert!(
            !required_strs.contains(&"session_id"),
            "session_id must NOT be required"
        );
    }

    // -- AC-8.4 --------------------------------------------------------------

    #[tokio::test]
    async fn test_spawn_agent_session_not_found_returns_error() {
        let tool = test_tool();
        let err = tool
            .call(SpawnAgentArgs {
                label: "lookup".to_string(),
                message: "anything".to_string(),
                session_id: Some("bogus-id".to_string()),
            })
            .await
            .expect_err("expected SessionNotFound");

        assert!(
            matches!(&err, SpawnAgentError::SessionNotFound { session_id } if session_id == "bogus-id")
        );
        assert_eq!(
            err.to_string(),
            "No sub-agent session found for id: bogus-id"
        );
    }

    // -- AC-8.5 --------------------------------------------------------------

    #[test]
    fn test_build_success_json_shape() {
        let s = build_success_json("abc", "hello");
        let v: serde_json::Value = serde_json::from_str(&s).expect("must be valid JSON");
        assert_eq!(v.get("session_id").and_then(|v| v.as_str()), Some("abc"));
        assert_eq!(v.get("output").and_then(|v| v.as_str()), Some("hello"));
        // Exactly two keys
        assert_eq!(v.as_object().map(|o| o.len()), Some(2));
    }

    // -- AC-8.6 --------------------------------------------------------------

    #[test]
    fn test_build_error_json_shape() {
        let s = build_error_json("abc", "timeout");
        let v: serde_json::Value = serde_json::from_str(&s).expect("must be valid JSON");
        assert_eq!(v.get("session_id").and_then(|v| v.as_str()), Some("abc"));
        assert_eq!(v.get("error").and_then(|v| v.as_str()), Some("timeout"));
        assert_eq!(v.as_object().map(|o| o.len()), Some(2));
    }

    // -- AC-8.7 --------------------------------------------------------------

    #[test]
    fn test_build_success_json_escapes_special_chars() {
        let s = build_success_json("id", "line1\nline2\"quoted\"");
        let v: serde_json::Value = serde_json::from_str(&s).expect("must round-trip");
        assert_eq!(
            v.get("output").and_then(|v| v.as_str()),
            Some("line1\nline2\"quoted\"")
        );
    }

    // -- AC-8.8 / 8.9 / 8.10 — Send + Sync gates -----------------------------

    #[test]
    fn test_spawn_agent_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SpawnAgentError>();
    }

    #[test]
    fn test_spawn_agent_struct_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SpawnAgentTool>();
    }

    #[test]
    fn test_spawn_agent_state_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SubAgentState>();
    }

    // -- Review-patch coverage -----------------------------------------------

    #[tokio::test]
    async fn test_spawn_agent_empty_session_id_is_treated_as_fresh_spawn() {
        // With session_id = Some("") and no in-map state, old behavior returned
        // SessionNotFound. The patched code normalizes "" → None and falls through
        // to spawn_new. The actual stream_chat will fail with a 401 (test API key)
        // and come back as Ok(build_error_json_no_session(...)) — the key assertion
        // is that the call did NOT route to the follow-up path.
        let tool = test_tool();
        let result = tool
            .call(SpawnAgentArgs {
                label: "test".to_string(),
                message: "anything".to_string(),
                session_id: Some(String::new()),
            })
            .await;

        match result {
            Err(SpawnAgentError::SessionNotFound { .. }) => {
                panic!("empty session_id must NOT route to the follow-up path")
            }
            Err(_other) => {
                // AgentBuildFailed is acceptable — we went through spawn_new.
            }
            Ok(json_str) => {
                // spawn_new's error path — no session_id key expected.
                let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
                assert!(
                    v.get("session_id").is_none(),
                    "spawn_new error payload must not include a session_id"
                );
                assert!(v.get("error").is_some(), "expected an error key");
            }
        }
    }

    #[tokio::test]
    async fn test_spawn_agent_session_busy_when_in_flight() {
        // Pre-populate the in-flight set to simulate a concurrent follow-up.
        let tool = test_tool();
        tool.lock_in_flight().insert("id-busy".to_string());
        // Also pre-populate sessions so we know SessionBusy trips BEFORE SessionNotFound.
        // We cannot insert a real SubAgentState (BuiltAgent requires a factory build),
        // so we keep sessions empty and rely on in_flight being checked first.

        let err = tool
            .call(SpawnAgentArgs {
                label: "concurrent".to_string(),
                message: "follow-up".to_string(),
                session_id: Some("id-busy".to_string()),
            })
            .await
            .expect_err("expected SessionBusy");

        assert!(
            matches!(&err, SpawnAgentError::SessionBusy { session_id } if session_id == "id-busy"),
            "expected SessionBusy, got {err:?}"
        );
    }

    #[test]
    fn test_build_error_json_no_session_omits_session_id() {
        let s = build_error_json_no_session("timeout");
        let v: serde_json::Value = serde_json::from_str(&s).expect("must be valid JSON");
        assert_eq!(v.get("error").and_then(|v| v.as_str()), Some("timeout"));
        assert!(
            v.get("session_id").is_none(),
            "session_id must be omitted when no session was retained"
        );
        assert_eq!(v.as_object().map(|o| o.len()), Some(1));
    }

    #[test]
    fn test_sanitize_label_truncates_and_strips_control_chars() {
        let clean = sanitize_label("audit module X");
        assert_eq!(clean, "audit module X");

        // Control chars (newline, carriage return, NUL) become spaces.
        let with_ctrl = sanitize_label("a\nb\rc\0d");
        assert!(!with_ctrl.contains('\n'));
        assert!(!with_ctrl.contains('\r'));
        assert!(!with_ctrl.contains('\0'));

        // Long labels are truncated at a UTF-8 char boundary.
        let long = "x".repeat(1000);
        let truncated = sanitize_label(&long);
        assert!(
            truncated.len() <= LABEL_LOG_MAX_LEN + 4,
            "truncated label must fit within the cap plus the ellipsis ({} vs {})",
            truncated.len(),
            LABEL_LOG_MAX_LEN
        );
        assert!(truncated.ends_with('…'));
    }
}
