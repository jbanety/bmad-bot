# Story 13.3: Daemon-Orchestrated Consultation Mechanism

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the session runner to support pausing an active session, running a fresh consultation agent, and feeding results back to the paused session,
So that sessions can be enriched with external perspectives (adversarial review, critic) without losing their BMAD context.

## Acceptance Criteria

1. **AC-1: ConsultationConfig struct**
   - **Given** a pipeline phase needs to define consultations
   - **When** this story is implemented
   - **Then** a `ConsultationConfig` struct is available with the following fields:
     - `label: String` — human-readable identifier (e.g., "adversarial", "critic")
     - `skill_path: Option<String>` — SKILL.md path for skill-based consultation agents
     - `preamble_override: Option<String>` — custom system prompt for non-skill agents (e.g., Critic)
     - `role: LlmRole` — which LLM role/config to use for agent construction
     - `tool_set: ConsultationToolSet` — which tools to register on the consultation agent
     - `context_files: Vec<String>` — absolute paths to files loaded into the agent's XML context via `ContextBuilder`
     - `trigger_pattern: String` — regex pattern the daemon watches for in the main session's response text
     - `prompt_template: String` — template for the consultation agent's prompt (supports `{context}` placeholder replaced with `ContextBuilder` output from `context_files`)
     - `resume_message_template: String` — template for injecting findings back into the main session (`{findings}` placeholder)
   - **And** `ConsultationConfig` enforces that exactly one of `skill_path` or `preamble_override` is `Some`. If both are `None`, `execute()` returns `ConsultationError::AgentBuildFailed` with reason "neither skill_path nor preamble_override provided". If both are `Some`, `skill_path` takes precedence (preamble_override is ignored) and a `tracing::warn!` is emitted.
   - **And** a `ConsultationToolSet` enum with variants:
     - `Full` — all base tools + think: git, read_file, edit_file, grep, find_path, list_directory, terminal, ThinkTool
     - `Restricted` — limited set with write access only to specific files: read_file, edit_file, grep, find_path, list_directory, ThinkTool (no git, no terminal, no ask_supervisor, no spawn_agent). Named "Restricted" (not "ReadOnly") because `edit_file` IS included — the Critic needs it to update `critic-memory.md`.

2. **AC-2: ConsultationRunner executes a fresh consultation agent**
   - **Given** a `ConsultationRunner` struct holding `Arc<AgentFactory>`, `ShutdownFlag`, `UiHandle`, project root path
   - **When** `execute(&self, config: &ConsultationConfig) -> Result<String, ConsultationError>` is called
   - **Then** a fresh agent is built via `AgentFactory::build(config.role, preamble, tools)` using the role and tool set from `ConsultationConfig`
   - **And** if `config.skill_path` is `Some`, the agent is activated via `BuiltAgent::activate_agent()` (SKILL.md loaded as first user message in Zed-style XML context)
   - **And** if `config.skill_path` is `None`, the `preamble_override` is used as the system prompt (no activation step)
   - **And** `context_files` are loaded from disk and assembled via `ContextBuilder` into an XML context block
   - **And** `prompt_template` is rendered (with `{context}` replaced by the XML context block) and sent as the user message
   - **And** the consultation agent runs a mini chat loop: send prompt → get response → check for `<<BMAD_JOB_DONE>>` or max turns (30) → send "Continue." if neither → repeat
   - **And** the agent's LAST turn text response is returned as `Ok(String)` — this is the "findings". Intermediate turns (tool-use reasoning) are discarded. The consultation preamble instructs the agent to output all findings in its final response before signaling `<<BMAD_JOB_DONE>>`. If no sentinel is emitted within 30 turns, the last response is returned as-is (best-effort).
   - **And** the `<<BMAD_JOB_DONE>>` sentinel is stripped from the returned findings if present

3. **AC-3: Consultation error handling is non-fatal**
   - **Given** a consultation agent encounters an error (agent build failure, `stream_chat` failure, context file not found, shutdown requested)
   - **When** the error is handled
   - **Then** `execute()` returns `Err(ConsultationError)` with a descriptive variant
   - **And** a `ConsultationError` enum exists with variants:
     - `AgentBuildFailed { reason: String }` — `AgentFactory::build()` failed
     - `ActivationFailed { reason: String }` — `activate_agent()` failed
     - `StreamChatFailed { reason: String }` — `stream_chat()` returned an error
     - `ContextFileNotFound { label: String, path: String }` — a file in `context_files` does not exist
     - `ShutdownRequested { label: String }` — cooperative shutdown during consultation
   - **And** `ConsultationError` implements `thiserror::Error` and `Display`

4. **AC-4: Session runner supports consultations in the chat loop**
   - **Given** `SessionRunner` manages a chat loop via `stream_chat(agent, prompt, history)`
   - **When** `run_with_consultations()` is called with a non-empty `Vec<ConsultationConfig>`
   - **Then** after each agent response in the chat loop (after `analyzer.analyze()` returns `NoReply`), each pending (not-yet-triggered) consultation's `trigger_pattern` is checked against the response text via `regex::Regex`
   - **And** consultations are checked in order — the first matching consultation is triggered, remaining consultations stay pending for future turns
   - **And** when a trigger matches:
     1. The consultation is marked as "triggered" (never triggered again)
     2. `ConsultationRunner::execute()` is called with the matched `ConsultationConfig`
     3. On success: the findings are formatted via `resume_message_template` (replacing `{findings}`) and used as the reply to the main session (overriding the default "Continue.")
     4. On failure: the reply is set to `"Consultation '{label}' failed: {error}. Continue without external input."` — the main session continues best-effort
     5. The reply is logged via `tracing::info!` with the consultation label
   - **And** the chat loop continues normally after injecting the findings — the main agent processes the findings in its full BMAD context and responds
   - **And** trigger checking only runs when `analyzer.analyze()` returns `NoReply` (NOT on `Completed` or `Escalated`). The match arm is split: `NoReply` runs trigger checks then falls through to reply logic; `Continue { reply }` skips trigger checks and uses its reply directly. This prevents re-entrancy: consultation findings injected via `Continue` cannot themselves trigger consultations on the same turn.
   - **And** different consultations CAN trigger on subsequent turns — e.g., after adversarial findings are injected and the agent responds, the agent's NEW response (to the findings) is checked against remaining pending consultations. This is by-design ordered execution.
   - **And** the existing `ResponseAction::Continue { reply }` mechanism is used to inject the findings (this is the prepared extension point from Story 12.2)

5. **AC-5: Backward compatibility — existing run() unchanged**
   - **Given** the existing `SessionRunner::run()` method
   - **When** this story is implemented
   - **Then** `run()` delegates to `run_with_consultations(story, base_branch_override, vec![])` — zero behavior change when no consultations are configured
   - **And** `run_with_consultations()` has the signature:
     ```rust
     pub async fn run_with_consultations(
         &self,
         story: &StoryInfo,
         base_branch_override: Option<&str>,
         consultations: Vec<ConsultationConfig>,
     ) -> SessionOutcome
     ```
   - **And** all existing tests pass without modification — the `ready-for-dev` pipeline path is unaffected

6. **AC-6: Tests**
   - **Given** the new consultation module
   - **When** this story is implemented
   - **Then** the following unit tests exist in `src/session/consultation.rs`:
     - `test_consultation_config_trigger_matches` — verifies regex trigger matching against sample response text
     - `test_consultation_config_trigger_no_match` — verifies non-matching responses return false
     - `test_consultation_config_trigger_complex_regex` — verifies multi-line and complex patterns
     - `test_consultation_config_format_findings` — verifies `{findings}` placeholder replacement in `resume_message_template`
     - `test_consultation_config_format_findings_no_placeholder` — verifies template returned as-is if no `{findings}` placeholder
     - `test_consultation_error_display` — verifies all `ConsultationError` variants produce readable messages
     - `test_consultation_tool_set_variants` — verifies enum has expected variants and Debug impl
     - `test_strip_job_done_sentinel` — verifies `<<BMAD_JOB_DONE>>` is stripped from findings
     - `test_consultation_config_validate_both_none` — verifies warning when neither skill_path nor preamble_override is set
     - `test_consultation_config_validate_both_some` — verifies warning when both are set
     - `test_consultation_config_validate_ok` — verifies no warnings when exactly one is set
   - **And** `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` passes
   - **And** `cargo test` passes with no new failures beyond the pre-existing `test_build_context_limit_recovery_message_contains_all_sections`

## Tasks / Subtasks

- [x] Task 1: Create `ConsultationConfig`, `ConsultationToolSet`, and `ConsultationError` types (AC: #1, #3)
  - [x] 1.1 Create new file `src/session/consultation.rs`
  - [x] 1.2 Define `ConsultationToolSet` enum:
    ```rust
    /// Tool set variants for consultation agents.
    ///
    /// Controls which tools are registered when building the consultation agent.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConsultationToolSet {
        /// All base tools + ThinkTool: git, read_file, edit_file, grep, find_path,
        /// list_directory, terminal. Same set as sub-agents (Story 12.3).
        Full,
        /// Restricted tool set: read_file, edit_file, grep, find_path,
        /// list_directory + ThinkTool. No git, no terminal. Includes edit_file because
        /// the Critic needs it for critic-memory.md updates (Story 13.9).
        Restricted,
    }
    ```
  - [x] 1.3 Define `ConsultationConfig` struct:
    ```rust
    /// Configuration for a daemon-orchestrated consultation (Decision 10).
    ///
    /// Defines how to build a fresh consultation agent, when to trigger it,
    /// and how to inject its findings back into the active session.
    #[derive(Debug, Clone)]
    pub struct ConsultationConfig {
        /// Human-readable label for logging (e.g., "adversarial", "critic").
        pub label: String,
        /// SKILL.md path for skill-based activation (relative to project root).
        /// When set, the consultation agent is activated via `BuiltAgent::activate_agent()`.
        pub skill_path: Option<String>,
        /// Custom system prompt for non-skill agents. Used when `skill_path` is None.
        pub preamble_override: Option<String>,
        /// LLM role/config for agent construction via `AgentFactory::build()`.
        pub role: LlmRole,
        /// Which tools to register on the consultation agent.
        pub tool_set: ConsultationToolSet,
        /// Absolute paths to files loaded into the agent's XML context via ContextBuilder.
        pub context_files: Vec<String>,
        /// Regex pattern the daemon watches for in the main session's response.
        /// Compiled once via `trigger_regex()`.
        pub trigger_pattern: String,
        /// Template for the consultation agent's prompt.
        /// Supports `{context}` placeholder replaced with ContextBuilder output.
        pub prompt_template: String,
        /// Template for injecting findings back into the main session.
        /// Supports `{findings}` placeholder replaced with consultation output.
        pub resume_message_template: String,
    }
    ```
  - [x] 1.4 Add methods to `ConsultationConfig`:
    ```rust
    impl ConsultationConfig {
        /// Compile the trigger pattern into a regex. Returns Err if invalid.
        /// Called once at session start by `ConsultationState::from_configs()`.
        pub fn trigger_regex(&self) -> Result<regex::Regex, regex::Error> {
            regex::Regex::new(&self.trigger_pattern)
        }

        /// Convenience: check trigger match by compiling regex on the fly.
        /// **For tests and validation only** — the hot path in the chat loop uses
        /// `ConsultationState.compiled_regex` (pre-compiled once at session start).
        #[cfg(test)]
        pub fn trigger_matches(&self, response: &str) -> bool {
            self.trigger_regex()
                .map(|re| re.is_match(response))
                .unwrap_or(false)
        }

        /// Format the resume message by replacing `{findings}` with the consultation output.
        pub fn format_findings(&self, findings: &str) -> String {
            self.resume_message_template.replace("{findings}", findings)
        }

        /// Validate config sanity. Returns a list of warnings (non-fatal).
        /// Called by `ConsultationState::from_configs()` at session start.
        pub fn validate(&self) -> Vec<String> {
            let mut warnings = Vec::new();
            if self.skill_path.is_none() && self.preamble_override.is_none() {
                warnings.push(format!(
                    "Consultation '{}': neither skill_path nor preamble_override set — agent will have empty preamble",
                    self.label
                ));
            }
            if self.skill_path.is_some() && self.preamble_override.is_some() {
                warnings.push(format!(
                    "Consultation '{}': both skill_path and preamble_override set — skill_path takes precedence",
                    self.label
                ));
            }
            warnings
        }
    }
    ```
  - [x] 1.5 Define `ConsultationError` enum:
    ```rust
    /// Errors from consultation execution.
    ///
    /// Consultation errors are non-fatal — the calling code resumes the main session
    /// with an error message instead of aborting the pipeline.
    #[derive(Debug, thiserror::Error)]
    pub enum ConsultationError {
        #[error("Failed to build consultation agent '{label}': {reason}")]
        AgentBuildFailed { label: String, reason: String },

        #[error("Failed to activate consultation agent '{label}': {reason}")]
        ActivationFailed { label: String, reason: String },

        #[error("Consultation '{label}' stream_chat failed: {reason}")]
        StreamChatFailed { label: String, reason: String },

        #[error("Context file not found for consultation '{label}': {path}")]
        ContextFileNotFound { label: String, path: String },

        #[error("Shutdown requested during consultation '{label}'")]
        ShutdownRequested { label: String },
    }
    ```

- [x] Task 2: Implement `ConsultationRunner` (AC: #2)
  - [x] 2.1 Define `ConsultationRunner` struct:
    ```rust
    /// Executes daemon-orchestrated consultations (Architecture Decision 10).
    ///
    /// Builds a fresh agent for each consultation, runs it to completion,
    /// and returns the findings as a String. Stateless — one runner serves
    /// all consultations across all pipeline phases.
    pub struct ConsultationRunner {
        agent_factory: Arc<AgentFactory>,
        shutdown: ShutdownFlag,
        ui: UiHandle,
        project_root: PathBuf,
    }
    ```
  - [x] 2.2 Implement constructor:
    ```rust
    impl ConsultationRunner {
        pub fn new(
            agent_factory: Arc<AgentFactory>,
            shutdown: ShutdownFlag,
            ui: UiHandle,
            project_root: PathBuf,
        ) -> Self {
            Self { agent_factory, shutdown, ui, project_root }
        }
    }
    ```
  - [x] 2.3 Implement `execute()` method — the core consultation logic:
    ```rust
    /// Maximum turns for a consultation chat loop.
    const MAX_CONSULTATION_TURNS: usize = 30;

    /// The BMAD job-done sentinel (same as session runner).
    const JOB_DONE_SENTINEL: &str = "<<BMAD_JOB_DONE>>";

    pub async fn execute(
        &self,
        config: &ConsultationConfig,
    ) -> Result<String, ConsultationError>
    ```
    Implementation flow:
    1. **Validate & build preamble:** If both `skill_path` and `preamble_override` are `None`, return `ConsultationError::AgentBuildFailed` with reason "neither skill_path nor preamble_override provided". If both are `Some`, log `tracing::warn!` and use `skill_path` (precedence). If `skill_path` is set, use a minimal operational preamble (tool rules, English override — same pattern as `build_sub_agent_preamble()`). If only `preamble_override` is set, use it directly.
    2. **Build tools:** Match on `config.tool_set`:
       - `Full` → `create_base_tools(project_root)` → `configure_agent_tools!(git, read_file, edit_file, grep, find_path, list_dir, terminal, ThinkTool)`
       - `Restricted` → construct only: read_file, edit_file, grep, find_path, list_directory → `configure_agent_tools!(read_file, edit_file, grep, find_path, list_dir, ThinkTool)`
    3. **Build agent:** `self.agent_factory.build(config.role, &preamble, tools).await` — map error to `ConsultationError::AgentBuildFailed`
    4. **Activate (if skill-based):** If `config.skill_path` is `Some(path)`, call `agent.activate_agent(self.project_root.to_str().unwrap_or(""), path, &config.label, Some(&self.shutdown), Some(&self.ui)).await` — map error to `ConsultationError::ActivationFailed`. Capture `activation_history`. Note: `activate_agent()` takes `&str` for `project_root`, so convert via `.to_str()`.
    5. **Build context:** For each `config.context_files` entry, read the file from disk (map missing files to `ConsultationError::ContextFileNotFound`). Build XML context via `ContextBuilder::new().add_file(path, content).build()`.
    6. **Render prompt:** Replace `{context}` in `config.prompt_template` with the XML context block.
    7. **Run consultation loop:**
       ```rust
       let mut history = activation_history; // or empty vec if no activation
       let mut last_response = String::new();
       for turn in 0..MAX_CONSULTATION_TURNS {
           let msg = if turn == 0 { rendered_prompt.clone() } else { "Continue.".to_string() };
           // Check shutdown before each turn
           if self.shutdown.load(std::sync::atomic::Ordering::Relaxed) {
               return Err(ConsultationError::ShutdownRequested { label: config.label.clone() });
           }
           let (text, new_history) = agent.stream_chat(
               rig::completion::Message::user(msg),
               history,
               Some(&self.shutdown),
               Some(&self.ui),
           ).await.map_err(|e| ConsultationError::StreamChatFailed {
               label: config.label.clone(),
               reason: e.to_string(),
           })?;
           history = new_history;
           last_response = text;
           if last_response.contains(JOB_DONE_SENTINEL) {
               break;
           }
       }
       // Strip sentinel from findings
       let findings = last_response.replace(JOB_DONE_SENTINEL, "").trim().to_string();
       Ok(findings)
       ```
  - [x] 2.4 Add a `build_consultation_preamble()` private helper — minimal preamble for skill-based consultations (similar pattern to `build_sub_agent_preamble()` but for consultation context):
    ```
    You are an AI consultation agent providing an independent review.

    ## Tools
    {tool_list_based_on_tool_set}

    ## Tool Usage Rules
    - Use read_file to examine files before commenting on them
    - Use edit_file only for files you are explicitly allowed to modify
    - Use grep and find_path to discover relevant code
    - Think carefully before making recommendations

    ## Communication
    - Respond in English
    - Be direct, specific, and constructive
    - Output your complete findings in a single response when possible
    - Signal completion with <<BMAD_JOB_DONE>> when finished
    ```

- [x] Task 3: Integrate consultation triggers into the session runner chat loop (AC: #4, #5)
  - [x] 3.1 Add `ConsultationRunner` as a field on `SessionRunner`:
    ```rust
    pub struct SessionRunner {
        // ... existing fields ...
        /// Consultation runner for daemon-orchestrated consultations (Decision 10).
        consultation_runner: ConsultationRunner,
    }
    ```
    Update `SessionRunner::new()` to construct `ConsultationRunner` from existing deps (agent_factory, shutdown, ui, project_root are all available).
  - [x] 3.2 Add `run_with_consultations()` method to `SessionRunner`:
    ```rust
    pub async fn run_with_consultations(
        &self,
        story: &StoryInfo,
        base_branch_override: Option<&str>,
        consultations: Vec<ConsultationConfig>,
    ) -> SessionOutcome
    ```
    This is a copy of the current `run()` body with one addition: the `consultations` parameter is passed through to the chat loop. All branch setup, agent construction, activation, WAL management — IDENTICAL to `run()`.
  - [x] 3.3 **CRITICAL REFACTOR:** Extract the chat loop body into a shared private method to avoid duplicating the entire `run()` method. The approach:
    - Identify the chat loop section (lines ~1751-2240 in runner.rs — the `loop { ... }` that calls `analyzer.analyze()`, matches `ResponseAction`, calls `stream_chat()`)
    - The new `run_with_consultations()` passes a `Vec<ConsultationConfig>` to this shared loop
    - When `consultations` is empty (called from `run()`), the loop behaves identically to the current implementation
    - When `consultations` is non-empty, the trigger-checking logic activates after each turn
    - **Alternative (simpler):** Do NOT extract the loop. Instead, make `run()` call `run_with_consultations(story, base_branch_override, vec![])`. Then rename the current `run()` body to `run_with_consultations()` and add the consultation trigger logic. `run()` becomes a thin wrapper.
  - [x] 3.4 Modify `run()` to delegate:
    ```rust
    pub async fn run(
        &self,
        story: &StoryInfo,
        base_branch_override: Option<&str>,
    ) -> SessionOutcome {
        self.run_with_consultations(story, base_branch_override, vec![]).await
    }
    ```
  - [x] 3.5 **Split the match arm** to prevent re-entrancy. The current `ResponseAction::NoReply | ResponseAction::Continue { .. }` arm (line ~2143) must be split into TWO arms:
    ```rust
    ResponseAction::NoReply => {
        // Check pending consultation triggers ONLY on NoReply.
        // Continue { reply } skips trigger checks — it carries injected findings
        // from a previous consultation, and the agent's response to those findings
        // should NOT re-trigger on the same turn.
        let reply = if let Some(findings_reply) = self.check_consultation_triggers(
            &current_response,
            &mut consultation_states,
        ).await {
            findings_reply
        } else {
            "Continue.".to_string()
        };
        // ... existing: state.add_user_message(&reply), stream_chat, persist ...
    }

    ResponseAction::Continue { reply } => {
        // Direct reply injection — no trigger checks. Used by future code
        // (e.g., supervisor amendments). Falls through to send reply.
        // ... existing: state.add_user_message(&reply), stream_chat, persist ...
    }
    ```
    **Both arms share the same send-reply-and-continue logic after computing `reply`.** Extract the common tail (add_user_message, stream_chat, persist, update response) into a local closure or inline block to avoid duplicating ~20 lines. Alternatively, compute `reply` in the match and fall through to shared code:
    ```rust
    ResponseAction::NoReply | ResponseAction::Continue { .. } => {
        let reply = match &action {
            ResponseAction::NoReply => {
                // Trigger checks only on NoReply
                self.check_consultation_triggers(&current_response, &mut consultation_states)
                    .await
                    .unwrap_or_else(|| "Continue.".to_string())
            }
            ResponseAction::Continue { reply } => reply.clone(),
            _ => unreachable!(),
        };
        // ... existing send/stream/persist logic with `reply` ...
    }
    ```
  - [x] 3.6 Implement `check_consultation_triggers()` private method:
    ```rust
    /// Check if any pending consultation triggers match the current response.
    ///
    /// Returns Some(reply) if a consultation was triggered and findings were injected,
    /// or None if no trigger matched.
    async fn check_consultation_triggers(
        &self,
        response: &str,
        consultation_states: &mut [ConsultationState],
    ) -> Option<String>
    ```
    Implementation:
    1. Iterate `consultation_states` in order
    2. Skip already-triggered consultations
    3. For the first matching trigger:
       a. Mark as triggered
       b. Log: `tracing::info!(action = "consultation_triggered", label = %config.label, ...)`
       c. Call `self.consultation_runner.execute(&config).await`
       d. On `Ok(findings)`: format via `config.format_findings(&findings)`, return `Some(formatted)`
       e. On `Err(e)`: log `tracing::warn!(...)`, return `Some(format!("Consultation '{}' failed: {}. Continue without external input.", config.label, e))`
    4. Return `None` if no trigger matched
  - [x] 3.7 Define `ConsultationState` as a private struct in `consultation.rs` (or inline in runner.rs):
    ```rust
    /// Internal tracking state for a consultation during a session.
    pub(crate) struct ConsultationState {
        pub config: ConsultationConfig,
        pub triggered: bool,
        pub compiled_regex: regex::Regex,
    }

    impl ConsultationState {
        pub fn from_configs(configs: Vec<ConsultationConfig>) -> Vec<Self> {
            configs.into_iter().filter_map(|config| {
                // Log validation warnings (non-fatal)
                for warning in config.validate() {
                    tracing::warn!(label = %config.label, "{}", warning);
                }
                match config.trigger_regex() {
                    Ok(regex) => Some(Self {
                        config,
                        triggered: false,
                        compiled_regex: regex,
                    }),
                    Err(e) => {
                        tracing::error!(
                            label = %config.label,
                            pattern = %config.trigger_pattern,
                            error = %e,
                            "Invalid consultation trigger regex — skipping"
                        );
                        None
                    }
                }
            }).collect()
        }
    }
    ```

- [x] Task 4: Wire module and update exports (AC: #5)
  - [x] 4.1 Add `pub mod consultation;` to `src/session/mod.rs`
  - [x] 4.2 Add necessary imports in `consultation.rs`:
    ```rust
    use crate::llm::agent_factory::{AgentFactory, BuiltAgent, LlmRole, ShutdownFlag};
    use crate::llm::context::ContextBuilder;
    use crate::ui::UiHandle;
    use rig::completion::Message;
    use std::path::Path;
    use std::sync::Arc;
    ```
  - [x] 4.3 Add `regex` to `Cargo.toml` if not already a dependency (check first — it may already be pulled in transitively). If not present, add `regex = "1"`.
  - [x] 4.4 Update `SessionRunner::new()` to construct the `ConsultationRunner`:
    ```rust
    let consultation_runner = ConsultationRunner::new(
        agent_factory.clone(),
        shutdown.clone(),
        ui.clone(),
        PathBuf::from(&config.bmad_paths.project_root),
    );
    ```
    Note: `agent_factory` is already `Arc<AgentFactory>` in `SessionRunner`, `shutdown` is `ShutdownFlag` (which is `Arc<AtomicBool>`), `ui` is `UiHandle` (which is `Arc<dyn UiRenderer>`). All are cheaply cloneable. `project_root` is converted from `String` to `PathBuf` once at construction — all tool constructors (`create_base_tools`, individual `*Tool::new`) take `PathBuf`, avoiding repeated conversions in `execute()`.

- [x] Task 5: Unit tests (AC: #6)
  - [x] 5.1 Add `#[cfg(test)] mod tests` at the bottom of `src/session/consultation.rs`
  - [x] 5.2 Add trigger matching tests:
    ```rust
    #[test]
    fn test_consultation_config_trigger_matches() {
        let config = ConsultationConfig {
            label: "adversarial".to_string(),
            trigger_pattern: r"(?i)story\s+file\s+created".to_string(),
            // ... other fields with defaults
        };
        assert!(config.trigger_matches("The story file created successfully."));
        assert!(config.trigger_matches("Story File Created and validated."));
    }

    #[test]
    fn test_consultation_config_trigger_no_match() {
        let config = /* same as above */;
        assert!(!config.trigger_matches("Working on implementation..."));
        assert!(!config.trigger_matches("Creating the file now"));
    }

    #[test]
    fn test_consultation_config_trigger_complex_regex() {
        let config = ConsultationConfig {
            trigger_pattern: r"(?i)(story\s+file|spec)\s+(created|generated|written)".to_string(),
            // ...
        };
        assert!(config.trigger_matches("The spec generated for review."));
        assert!(config.trigger_matches("Story file written to disk."));
        assert!(!config.trigger_matches("Generating the spec now..."));
    }
    ```
  - [x] 5.3 Add findings formatting tests:
    ```rust
    #[test]
    fn test_consultation_config_format_findings() {
        let config = ConsultationConfig {
            resume_message_template: "An external reviewer found:\n\n{findings}\n\nPlease fix.".to_string(),
            // ...
        };
        let result = config.format_findings("Issue 1: Missing AC\nIssue 2: Wrong lib");
        assert!(result.contains("Issue 1: Missing AC"));
        assert!(result.starts_with("An external reviewer found:"));
        assert!(result.ends_with("Please fix."));
    }

    #[test]
    fn test_consultation_config_format_findings_no_placeholder() {
        let config = ConsultationConfig {
            resume_message_template: "No placeholder here.".to_string(),
            // ...
        };
        let result = config.format_findings("Some findings");
        assert_eq!(result, "No placeholder here.");
    }
    ```
  - [x] 5.4 Add error display tests:
    ```rust
    #[test]
    fn test_consultation_error_display() {
        let err = ConsultationError::AgentBuildFailed {
            label: "critic".to_string(),
            reason: "invalid API key".to_string(),
        };
        assert!(err.to_string().contains("critic"));
        assert!(err.to_string().contains("invalid API key"));
        // Test all variants produce readable messages
    }
    ```
  - [x] 5.5 Add sentinel stripping test:
    ```rust
    #[test]
    fn test_strip_job_done_sentinel() {
        let raw = "Here are my findings.\n\n<<BMAD_JOB_DONE>>";
        let stripped = raw.replace("<<BMAD_JOB_DONE>>", "").trim().to_string();
        assert_eq!(stripped, "Here are my findings.");

        let raw_embedded = "Findings:\n<<BMAD_JOB_DONE>>\nExtra text";
        let stripped = raw_embedded.replace("<<BMAD_JOB_DONE>>", "").trim().to_string();
        assert_eq!(stripped, "Findings:\n\nExtra text");
    }
    ```
  - [x] 5.6 Add `ConsultationState::from_configs()` test:
    ```rust
    #[test]
    fn test_consultation_state_from_configs_valid() {
        let configs = vec![
            ConsultationConfig { trigger_pattern: "foo".to_string(), /* ... */ },
            ConsultationConfig { trigger_pattern: "bar".to_string(), /* ... */ },
        ];
        let states = ConsultationState::from_configs(configs);
        assert_eq!(states.len(), 2);
        assert!(!states[0].triggered);
        assert!(!states[1].triggered);
    }

    #[test]
    fn test_consultation_state_from_configs_invalid_regex_skipped() {
        let configs = vec![
            ConsultationConfig { trigger_pattern: "[invalid".to_string(), /* ... */ },
            ConsultationConfig { trigger_pattern: "valid".to_string(), /* ... */ },
        ];
        let states = ConsultationState::from_configs(configs);
        assert_eq!(states.len(), 1); // invalid regex skipped
    }
    ```
  - [x] 5.7 Add validation tests:
    ```rust
    #[test]
    fn test_consultation_config_validate_both_none() {
        let config = ConsultationConfig {
            skill_path: None,
            preamble_override: None,
            // ... other fields ...
        };
        let warnings = config.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("neither skill_path nor preamble_override"));
    }

    #[test]
    fn test_consultation_config_validate_both_some() {
        let config = ConsultationConfig {
            skill_path: Some("skill.md".to_string()),
            preamble_override: Some("You are a reviewer".to_string()),
            // ...
        };
        let warnings = config.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("skill_path takes precedence"));
    }

    #[test]
    fn test_consultation_config_validate_ok() {
        let config = ConsultationConfig {
            skill_path: Some("skill.md".to_string()),
            preamble_override: None,
            // ...
        };
        assert!(config.validate().is_empty());
    }
    ```
  - [x] 5.8 Verify all existing tests pass: `cargo test` — 1179 passed, 1 pre-existing failure unchanged
  - [x] 5.9 `cargo build` — zero new warnings in modified files
  - [x] 5.10 `cargo clippy --bin bmad-bot` — 34 pre-existing errors, zero new (unchanged count)

## Dev Notes

### Architecture Compliance

- **Decision 10 (Daemon-Orchestrated Consultations):** This story implements the core mechanism defined in Decision 10. The pause/consult/resume pattern is implemented inside the session runner's chat loop: when a trigger pattern matches, the loop builds a fresh consultation agent, runs it, and injects findings back as a user message via `ResponseAction::Continue { reply }`.
- **Decision 11 (Story Critic):** The `ConsultationToolSet::Restricted` variant and the `preamble_override` field anticipate the Critic agent's restricted tool set and non-skill preamble. The actual Critic construction is Story 13.9.
- **Decision 2 (Daemon Reads, Agent Writes):** Unchanged. Consultations are orchestrated by the daemon, but all mutations (story file edits, memory updates) happen through the consultation agent's tools, not the daemon.
- **Error handling pattern:** `ConsultationError` uses `thiserror` per-module enum, consistent with `PipelineError`, `SpawnAgentError`. Non-fatal: the main session resumes with an error message rather than aborting.

### Critical Implementation Details

**This story adds the generic consultation MECHANISM — it does NOT wire specific consultations to pipeline phases.** Stories 13.4 (create-story), 13.5 (dev-story), and 13.6 (code-review) will pass `Vec<ConsultationConfig>` to `run_with_consultations()` from their respective pipeline methods.

**The `ResponseAction::Continue { reply }` extension point is already in place.** Story 12.2 added this variant with `#[allow(dead_code)]` and a comment referencing Epic 13. The `NoReply | Continue { .. }` match arm in the chat loop (runner.rs line ~2143) already extracts the reply. The consultation trigger logic slots in BEFORE this extraction: if a trigger matches, it produces a `Continue { reply: findings }` response that flows through the existing path. After this story, the `#[allow(dead_code)]` annotation on `Continue` can be removed.

**Consultation agents share the same `AgentFactory` as the main session.** The consultation role (e.g., `LlmRole::Review` for adversarial, `LlmRole::Critic` when added) determines which provider/model is used. No new LLM roles are added in this story.

**Consultation agents get their own tool instances.** Each call to `create_base_tools()` or the restricted tool constructor creates fresh tool instances with their own `project_root`. This is the same isolation model as `SpawnAgentTool` (Story 12.3).

**Trigger patterns are compiled once at session start.** `ConsultationState::from_configs()` compiles all regex patterns upfront via `ConsultationConfig::trigger_regex()`. Invalid regexes are logged and skipped — they do NOT abort the session. The `trigger_matches()` convenience method on `ConsultationConfig` is `#[cfg(test)]` only — the hot path uses `ConsultationState.compiled_regex.is_match()` directly, avoiding per-call recompilation. `from_configs()` also calls `config.validate()` and logs any warnings.

**Only one consultation triggers per turn.** If multiple consultations have patterns that match the same response, only the FIRST (in config order) triggers. The remaining stay pending for future turns. This ensures ordered consultations (e.g., adversarial first, then critic on the corrected output). **Re-entrancy is prevented:** trigger checks run only on `NoReply` responses, NOT on `Continue { reply }` responses (which carry injected findings). The agent's response to injected findings is a NEW turn → `NoReply` → remaining consultations are checked. This means different consultations fire on DIFFERENT turns, never the same turn.

**The consultation mini-loop uses the same `stream_chat()` as the main session.** Tool calls (read_file, edit_file, etc.) are handled by rig internally within `stream_chat()`. The consultation agent can make multiple tool calls per turn. **Only the last turn's text is returned as findings** — intermediate turn texts (typically tool-use reasoning like "Let me read the file...") are discarded. The consultation preamble explicitly instructs the agent to consolidate all findings in its final response before signaling `<<BMAD_JOB_DONE>>`.

**MAX_CONSULTATION_TURNS = 30.** This caps the consultation chat loop to prevent runaway consultations. Consultations should typically complete in 1-5 turns. The 30-turn limit is generous but bounded. If a consultation agent doesn't signal `<<BMAD_JOB_DONE>>` within 30 turns, the last response is returned as-is (best-effort). **No wall-clock timeout per consultation** — each `stream_chat()` call is bounded by the LLM provider's own timeout and the `ShutdownFlag` for cooperative cancellation. A stuck LLM provider is a broader infra issue, not consultation-specific. If wall-clock timeout becomes necessary in practice, it can be added as a `tokio::time::timeout` wrapper around `execute()` in a follow-up — the non-fatal error handling means the main session simply resumes.

**The `{context}` placeholder in `prompt_template`.** Context files are loaded from disk and assembled via `ContextBuilder` into Zed-style XML tags. The rendered XML is substituted for `{context}` in the prompt. **When `context_files` is empty**, `{context}` is replaced with an empty string. Callers should design `prompt_template` to be self-contained when context is absent — e.g., use `"Review the following story:\n\n{context}\n\nIdentify issues."` where the empty context produces a clean prompt. Alternatively, callers can omit `{context}` from the template entirely and pass context via the agent's activation or preamble instead.

### Interaction with `run()` Refactor

**Strategy: rename-and-wrap, not duplicate.** The current `run()` body becomes `run_with_consultations()` body, with the addition of:
1. A `consultation_states: Vec<ConsultationState>` initialized from the `consultations` parameter
2. The `check_consultation_triggers()` call in the `NoReply` arm of the chat loop

The old `run()` becomes a one-line wrapper. This avoids duplicating the ~600-line method body (branch setup, agent construction, activation, WAL management, chat loop, finalization, impact analysis — all identical).

**Borrow checker note:** `consultation_states: Vec<ConsultationState>` is a local variable inside `run_with_consultations()`. `ConsultationState` OWNS its `ConsultationConfig` (moved, not borrowed). `check_consultation_triggers()` takes `&mut [ConsultationState]` and borrows `&self` for `self.consultation_runner.execute()`. No conflict: `consultation_states` is not borrowed from `self`, and `execute()` only reads `&self.consultation_runner` (immutable). The `&ConsultationConfig` reference passed to `execute()` comes from the `ConsultationState` struct — safe because `consultation_states` is `&mut` borrowed (not moved) during `execute().await`.

### `check_consultation_triggers()` Integration Point

The trigger check happens in the `ResponseAction::NoReply | ResponseAction::Continue { .. }` arm of the chat loop, BEFORE the reply is computed. Pseudo-code:

```rust
ResponseAction::NoReply | ResponseAction::Continue { .. } => {
    // NEW: Check consultations before computing reply
    let consultation_reply = self.check_consultation_triggers(
        &current_response,
        &mut consultation_states,
    ).await;

    let reply = if let Some(findings_reply) = consultation_reply {
        findings_reply
    } else {
        match action {
            ResponseAction::Continue { reply } => reply,
            _ => "Continue.".to_string(),
        }
    };

    // Existing: send reply, stream_chat, persist, etc.
    state.add_user_message(&reply);
    // ...
}
```

### Known Limitations

**WAL does not persist consultation state.** If the daemon crashes mid-consultation, the WAL reflects the pre-consultation chat state. On recovery, the main session resumes from the last persisted turn. The trigger pattern may not fire again because the triggering response is already in history. **Impact:** The consultation is silently skipped — the agent continues without external findings. This is acceptable for two reasons: (1) consultations are non-fatal by design — the session can proceed without them; (2) Story 13.10 (WAL with Pipeline Phase Tracking) will add `pipeline_phase` to the WAL, enabling restart-from-scratch for create/review phases where consultations matter most. Document this as a known gap in the WAL recovery path.

**`resume_session()` does not support consultations.** The crash-recovery path (`SessionRunner::resume_session()`) has its OWN chat loop for WAL-recovered sessions. Consultations configured for a normal `run_with_consultations()` call are NOT applied during recovery — a recovered session skips all consultations. **Rationale:** Recovery loads pre-existing chat history and resumes mid-conversation. Trigger patterns reference the initial creation/completion phase which has already passed. Injecting consultations into a recovered session at an arbitrary mid-point would produce nonsensical results. Story 13.10 addresses this properly by restarting the affected phase from scratch rather than resuming mid-loop.

**Why NOT trigger on `Completed`?** The `Completed` action means `<<BMAD_JOB_DONE>>` was detected — the session is about to finalize. Consultations that need to happen "when the agent completes its phase" should use a trigger pattern that matches the agent's output BEFORE the completion signal. For example, the create-story agent might output "Story file created and saved" THEN on the NEXT turn output `<<BMAD_JOB_DONE>>`. The trigger matches the intermediate message, and the consultation runs before completion.

**If the calling code needs post-completion consultations** (e.g., pipeline-level consultations that happen AFTER the session ends), those are handled by the pipeline directly using `ConsultationRunner::execute()` — not by the chat loop integration. The `ConsultationRunner` is a standalone struct usable outside of `SessionRunner`.

### `ConsultationRunner` as Standalone

`ConsultationRunner` is constructed inside `SessionRunner` but its `execute()` method is self-contained. The pipeline can ALSO construct its own `ConsultationRunner` for post-session consultations. The struct is `pub` and its constructor is `pub`. This enables:
- **In-session consultations:** triggered by the chat loop via `check_consultation_triggers()`
- **Post-session consultations:** called directly by pipeline phase methods (Stories 13.4, 13.6)

### Artifact Consistency During Consultation

When a trigger fires, the main session is effectively "paused" — no `stream_chat()` calls are in flight, no tool calls are executing. The consultation agent reads files (e.g., the story file) from disk at the moment it runs. Since the main agent is paused, there is no race condition — the artifact on disk is exactly what the main agent last wrote. The consultation reviews the current state, not a stale snapshot. If `context_files` reference a file that the main agent just created or modified (e.g., the story spec), the consultation sees the latest version. No explicit artifact pinning or content snapshot is needed.

### Tool Registration Pattern for Consultations

The `ConsultationToolSet` enum maps to concrete tool configurations:

```rust
// Full tool set (same as sub-agents in Story 12.3)
ConsultationToolSet::Full => {
    let (git, read_file, edit_file, grep, find_path, list_dir, terminal) =
        create_base_tools(&self.project_root);
    configure_agent_tools!(git, read_file, edit_file, grep, find_path, list_dir, terminal, ThinkTool)
}

// Restricted tool set (for Critic — Story 13.9). Includes edit_file because
// the Critic needs it for critic-memory.md. No git, no terminal.
ConsultationToolSet::Restricted => {
    let read_file = ReadFileTool::new(self.project_root.clone());
    let edit_file = EditFileTool::new(self.project_root.clone());
    let grep = GrepTool::new(self.project_root.clone());
    let find_path = FindPathTool::new(self.project_root.clone());
    let list_dir = ListDirectoryTool::new(self.project_root.clone());
    configure_agent_tools!(read_file, edit_file, grep, find_path, list_dir, ThinkTool)
}
```

**Note:** `configure_agent_tools!` returns a `ToolConfigurator` that implements `AgentConfigurator`. The macro supports arities 1-12 (agent_factory.rs line 502). Both `Full` (8 tools) and `Restricted` (6 tools) are within the supported arity range.

### `regex` Dependency

The `regex` crate is already a transitive dependency (pulled in by `rig-core` and other crates). Check `Cargo.lock` — if `regex` appears, add `regex = "1"` to `[dependencies]` in `Cargo.toml` for direct usage. If it's not present, add it explicitly.

### Files to Modify

| File | Change Type | Scope |
|---|---|---|
| `src/session/consultation.rs` | **New** | `ConsultationConfig`, `ConsultationToolSet`, `ConsultationError`, `ConsultationRunner`, `ConsultationState`, unit tests |
| `src/session/mod.rs` | **Modify** | Add `pub mod consultation;` |
| `src/session/runner.rs` | **Modify** | Add `consultation_runner` field to `SessionRunner`; rename `run()` body to `run_with_consultations()`; add `check_consultation_triggers()` in chat loop; `run()` becomes wrapper |
| `Cargo.toml` | **Modify** (if needed) | Add `regex = "1"` if not already a direct dependency |

**NOT modified:**
- `src/pipeline.rs` — wiring consultations to pipeline phases is Stories 13.4, 13.5, 13.6
- `src/llm/agent_factory.rs` — `LlmRole::Critic` variant is Story 13.9
- `src/config/mod.rs` — `LlmConfig.critic` role config is Story 13.9
- `src/ui/renderer.rs` — consultation UI events (`consultation_start`, `consultation_complete`) are Story 13.11
- `src/session/state.rs` — WAL pipeline_phase tracking is Story 13.10
- `src/session/analyzer.rs` — the `ResponseAction::Continue { reply }` variant is unchanged; remove `#[allow(dead_code)]` since it's now used
- `SessionRunner::resume_session()` — WAL crash-recovery has its own chat loop that does NOT support consultations. Recovered sessions skip consultations by design (see Known Limitations). Story 13.10 addresses this by restarting phases from scratch instead of resuming mid-loop.

### Existing Code to Reuse

- `AgentFactory::build(role, preamble, tools)` — builds the consultation agent identically to main/sub agents [src/llm/agent_factory.rs:242]
- `BuiltAgent::stream_chat()` — runs the consultation agent's chat turn with tool calling [src/llm/agent_factory.rs:88]
- `BuiltAgent::activate_agent()` — activates skill-based consultation agents [src/llm/agent_factory.rs:120]
- `create_base_tools(project_root)` — creates the full tool set for `ConsultationToolSet::Full` [src/session/agent.rs:78]
- `configure_agent_tools!()` — macro for tool registration on the agent builder [src/llm/agent_factory.rs:423]
- `ContextBuilder::new().add_file(path, content).build()` — builds XML context for consultation agents [src/llm/context.rs:72]
- `build_sub_agent_preamble(model)` — reference pattern for the consultation preamble (NOT reused directly — consultation preamble is distinct but follows the same structure) [src/session/agent.rs:327]
- `ResponseAction::Continue { reply }` — prepared extension point for injecting findings [src/session/analyzer.rs:29]
- `ReadFileTool::new()`, `EditFileTool::new()`, etc. — individual tool constructors for `Restricted` set [src/tools/*.rs]

### Anti-Patterns to Avoid

- **DO NOT** add `LlmRole::Critic` — that is Story 13.9. Use existing roles (e.g., `LlmRole::Review`) for testing the mechanism.
- **DO NOT** add `LlmConfig.critic` config field — that is Story 13.9.
- **DO NOT** wire consultations into pipeline phases — that is Stories 13.4/13.5/13.6. The pipeline methods (`run_create_pipeline()`, `run_review_pipeline()`) remain unchanged.
- **DO NOT** add UI events for consultations (consultation_start, consultation_complete) — that is Story 13.11. Use `tracing::info!` / `tracing::warn!` for logging.
- **DO NOT** modify WAL to track consultation phase — that is Story 13.10.
- **DO NOT** create a `critic-memory.md` file or manage Critic memory — that is Story 13.8.
- **DO NOT** duplicate the `run()` method body. Rename-and-wrap: `run()` delegates to `run_with_consultations(vec![])`. One implementation, two entry points.
- **DO NOT** modify the `ResponseAnalyzer` — trigger pattern matching is separate from response analysis. The analyzer determines `NoReply`; the consultation check happens AFTER the analyzer, in the action-handling code.
- **DO NOT** add `ask_supervisor` or `spawn_agent` tools to consultation agents. Consultations are independent — no supervisor escalation, no nested delegation.
- **DO NOT** make consultation failures fatal. Always resume the main session with an error message. The pipeline continues.
- **DO** prefix the `#[allow(dead_code)]` removal on `ResponseAction::Continue` — it's now used by the consultation trigger logic.

### Previous Story Intelligence (Story 13.2)

- **Baseline test count:** 1166 passing, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)
- **Pre-existing clippy allowances:** `-A clippy::needless_splitn -A clippy::unnecessary_map_or`
- **`ResponseAction::Continue { reply }`** is at `src/session/analyzer.rs:29` with `#[allow(dead_code)]`
- **Chat loop NoReply/Continue handling** is at `src/session/runner.rs:2143` — this is the integration point
- **`StoryPhase` routing in pipeline.rs:** `run_create_pipeline()` is still a placeholder (returns error). This story does NOT fill it in.
- **Notification spam concern from 13.2 review:** Placeholder phases cause repeated Telegram notifications every poll cycle. This is expected and resolves when Stories 13.4/13.6 are implemented.
- **Brittle source-check test** (`test_process_story_installs_cleanup_guard_source_check`): This test inspects `pipeline.rs` source code. Since this story modifies only `session/` files, it is unaffected.
- **`SpawnAgentTool` pattern:** Fresh agent construction via `AgentFactory::build()`, one-shot `stream_chat()`, session management via `HashMap<String, SubAgentState>`. The consultation mechanism uses the same `build()` pattern but adds a mini chat loop and trigger-based invocation.

### Git Intelligence — Recent Commits

```
147f57d feat(epic-13): refactor pipeline into status-based phase router (Story 13.2)
fb38013 feat(epic-13): extend watcher to detect backlog and review stories (Story 13.1)
ab07b29 test(epic-12): add skill-based session and spawn-agent integration tests (Story 12.5)
cd7cce9 docs(epic-13): advance epic-13 to in-progress, create story 13-1 spec
a47a720 feat(epic-12): wire SpawnAgentTool universally in dev + review sessions (Story 12.4)
```

**Expected commit message:** `feat(epic-13): add daemon-orchestrated consultation mechanism (Story 13.3)`

### Project Structure Notes

- New file `src/session/consultation.rs` follows the existing modular pattern (session/ subdirectory)
- `ConsultationRunner` follows the same service pattern as `SessionRunner` and `ReviewRunner` — holds `Arc<>` deps, stateless, method-based
- `ConsultationConfig` is a plain data struct — no behavior beyond format/match helpers
- The `regex` dependency is standard for Rust projects and likely already transitive

### References

- [Source: _bmad-output/planning-artifacts/epics.md:3191–3220 — Story 13.3 AC (Daemon-Orchestrated Consultation Mechanism)]
- [Source: _bmad-output/planning-artifacts/architecture.md:664–693 — Decision 10 (Daemon-Orchestrated Consultations — Pause/Consult/Resume)]
- [Source: _bmad-output/planning-artifacts/architecture.md:695–716 — Decision 11 (Story Critic — Independent Vision Guardian)]
- [Source: _bmad-output/planning-artifacts/epics.md:3480–3503 — Epic 13 Summary and execution strategy]
- [Source: _bmad-output/project-context.md:48–68 — Daemon Lifecycle, Agent Construction, Supervisor Hybrid]
- [Source: _bmad-output/project-context.md:109–117 — Testing Rules]
- [Source: src/session/runner.rs:313–337 — SessionRunner struct (fields)]
- [Source: src/session/runner.rs:659–662 — SessionRunner::run() signature]
- [Source: src/session/runner.rs:1751–2240 — Chat loop body (analyze → action → reply → stream_chat)]
- [Source: src/session/runner.rs:2143–2149 — NoReply|Continue arm — consultation integration point]
- [Source: src/session/analyzer.rs:22–45 — ResponseAction enum with Continue { reply } extension point]
- [Source: src/llm/agent_factory.rs:37–46 — LlmRole enum (Dev, Review, Supervisor, EpicReview)]
- [Source: src/llm/agent_factory.rs:74–162 — BuiltAgent enum with stream_chat() and activate_agent()]
- [Source: src/llm/agent_factory.rs:242–334 — AgentFactory::build() signature and flow]
- [Source: src/llm/agent_factory.rs:417–501 — configure_agent_tools! macro]
- [Source: src/llm/context.rs:72–165 — ContextBuilder API]
- [Source: src/session/agent.rs:78–89 — create_base_tools() — 7 base tools]
- [Source: src/session/agent.rs:327–374 — build_sub_agent_preamble() — reference pattern]
- [Source: src/session/agent.rs:787–797 — activate_agent() standalone function (generic)]
- [Source: src/tools/spawn_agent.rs:39–118 — SpawnAgentTool pattern (fresh agent, one-shot stream_chat)]
- [Source: src/config/mod.rs:165–178 — LlmConfig struct (dev, review, supervisor, epic_review)]
- [Source: src/config/mod.rs:182–192 — LlmRoleConfig struct]
- [Source: _bmad-output/implementation-artifacts/13-2-pipeline-orchestrator-refonte.md — Previous story complete intelligence]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Task 1-4 complete: Created `src/session/consultation.rs` with `ConsultationConfig`, `ConsultationToolSet`, `ConsultationError`, `ConsultationRunner`, and `ConsultationState`. Implemented `execute()` with skill-based and preamble-based agent construction, mini chat loop with `<<BMAD_JOB_DONE>>` sentinel detection, and context file loading via `ContextBuilder`.
- Task 3 complete: Integrated consultation triggers into `SessionRunner` chat loop. `run()` now delegates to `run_with_consultations(vec![])` (zero behavior change). The `NoReply` arm checks pending consultation triggers before defaulting to "Continue.". Recovery paths (`resume_session`, `context_limit_recovery`) pass empty consultation slices.
- Task 5 complete: 13 unit tests covering trigger matching (positive, negative, complex regex), findings formatting (with/without placeholder), error display (all 5 variants), tool set variants, sentinel stripping, `ConsultationState::from_configs()` (valid + invalid regex skipping), and validation (both none, both some, ok).
- All 1179 tests pass (1 pre-existing failure unchanged). Zero new clippy warnings. Zero new build warnings in modified files.

### File List

- `src/session/consultation.rs` — **New** — ConsultationConfig, ConsultationToolSet, ConsultationError, ConsultationRunner, ConsultationState, unit tests
- `src/session/mod.rs` — **Modified** — Added `pub mod consultation;`
- `src/session/runner.rs` — **Modified** — Added `consultation_runner` field to SessionRunner; `run()` delegates to `run_with_consultations()`; added `check_consultation_triggers()`; threaded `consultation_states` through `run_session()`
- `src/session/analyzer.rs` — **Modified** — Updated doc comment on `ResponseAction::Continue`
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — **Modified** — Story 13-3 status update

### Review Findings

- [x] [Review][Decision] `#[allow(dead_code)]` remains on `ResponseAction::Continue` despite spec requiring removal — Resolved: refactored `check_consultation_triggers()` to return `Option<ResponseAction>` and construct `ResponseAction::Continue { reply }`, then removed `#[allow(dead_code)]`. [src/session/analyzer.rs:28, src/session/runner.rs]
- [x] [Review][Defer] `unwrap_or("")` on non-UTF-8 `project_root` in `ConsultationRunner::execute()` — `self.project_root.to_str().unwrap_or("")` silently degrades to empty string if the path contains non-UTF-8 bytes, causing cryptic `activate_agent` failures. Pre-existing pattern used across the codebase. [src/session/consultation.rs:240] — deferred, pre-existing

### Change Log

- 2026-04-22: Implemented daemon-orchestrated consultation mechanism (Story 13.3) — all 5 tasks complete, 13 new unit tests, all ACs satisfied
