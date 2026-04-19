# Story 12.3: SpawnAgent Tool

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As an LLM agent working in a BMAD session,
I want to spawn independent sub-agents for well-scoped tasks,
So that I can delegate research, parallel investigation, or specialized work without polluting my main context.

## Acceptance Criteria

1. **AC-1: New tool file with rig `Tool` trait implementation**
   - **Given** a new tool file at `src/tools/spawn_agent.rs`
   - **When** this story is implemented
   - **Then** a `SpawnAgentTool` struct implements the rig `Tool` trait with:
     - `const NAME: &'static str = "spawn_agent"`
     - `type Args = SpawnAgentArgs` — a `#[derive(Debug, Deserialize)]` struct with fields `label: String`, `message: String`, `session_id: Option<String>`
     - `type Output = String` — a JSON string produced by a private helper (see AC-5), either `{"session_id": "...", "output": "..."}` on success or `{"session_id": "...", "error": "..."}` on sub-agent execution failure
     - `type Error = SpawnAgentError` — a `#[derive(Debug, thiserror::Error)]` enum with variants `SessionNotFound { session_id }` and `AgentBuildFailed { reason }`. No `LockPoisoned` variant — poisoning is recovered silently via the `lock_sessions()` helper (see AC-6); no `SerializationFailed` variant — the JSON helpers (AC-5) return `String` directly because `serde_json::json!({&str, &str}).to_string()` cannot fail
   - **And** the tool is re-exported from `src/tools/mod.rs` via `pub mod spawn_agent;` + `pub use spawn_agent::SpawnAgentTool;` so it becomes importable as `crate::tools::SpawnAgentTool`
   - **And** the tool is NOT yet registered in `create_base_tools()` or any session — registration, pipeline wiring, and sessions-map ownership are Story 12.4 scope

2. **AC-2: New-session path (no `session_id`) creates fresh sub-agent**
   - **Given** `SpawnAgentTool::call()` receives `SpawnAgentArgs { session_id: None, label, message }`
   - **When** `call()` runs
   - **Then** a fresh `BuiltAgent` is built via the tool's stored `Arc<AgentFactory>` using the parent role captured at construction time:
     - `model = self.agent_factory.config_for_role(self.role).model.clone()`
     - `preamble = build_sub_agent_preamble(&model)` — a NEW helper added to `src/session/agent.rs` alongside `build_preamble()`. It produces a preamble identical in structure to `build_preamble(&[], &model)` BUT the tool inventory line lists only the tools actually registered on sub-agents: `edit_file, read_file, grep, find_path, list_directory, git, terminal, think`. It explicitly omits `ask_supervisor` and `spawn_agent` so the sub-agent is not told about tools it does not have.
     - `configurator = configure_agent_tools!(git, read_file, edit_file, grep, find_path, list_dir, terminal, ThinkTool)` — exactly 8 entries: the 7 base tools from `create_base_tools(&self.project_root)` + rig's `ThinkTool`. Explicitly **no** `AskSupervisor` (sub-agents must not use the supervisor/escalation flow) and **no** `SpawnAgentTool` (prevents nested delegation recursion).
   - **And** a unique `session_id` is generated via `uuid::Uuid::new_v4().to_string()`
   - **And** the user message is constructed as `rig::completion::Message::user(args.message.clone())` and passed to `agent.stream_chat(prompt, vec![], self.shutdown.as_ref(), None).await` with empty history, the tool's optional parent `ShutdownFlag` (see AC-6), and `ui: None` (sub-agent tool calls are not rendered on the parent UI — they emit structured trace logs only)
   - **And** the streaming loop is bounded by rig's existing `STREAMING_MAX_TURNS = 300` constant in `session/agent.rs:121`. **This story does not introduce a separate `MAX_SUB_AGENT_TURNS` constant** and does not refactor `streaming_chat()`. The epic's "default 100 turns" language is explicitly deferred — documented in a `// NOTE:` comment in `call()` and listed as a deferral in Dev Notes.
   - **And** on success `(accumulated_text, full_history)`, the completed agent is stored in the shared session map under `session_id` via the `lock_sessions()` helper (AC-6): `guard.insert(session_id.clone(), SubAgentState { agent, history: full_history, role: self.role, model })`
   - **And** the tool returns `Ok(build_success_json(&session_id, &accumulated_text))` (pure helper, AC-5)

3. **AC-3: Follow-up-session path (`session_id` present) reuses existing sub-agent**
   - **Given** `SpawnAgentTool::call()` receives `SpawnAgentArgs { session_id: Some(id), label, message }`
   - **When** `call()` runs
   - **Then** the existing `SubAgentState` is removed from the map under a briefly-held lock (not retained across `.await`): the removed `state` is moved onto the stack; if `remove` returned `None`, the tool returns `Err(SpawnAgentError::SessionNotFound { session_id: id })` whose `Display` message is exactly `"No sub-agent session found for id: {session_id}"` — the lookup is atomic
   - **And** with the lock guard dropped, `state.agent.stream_chat(Message::user(args.message.clone()), state.history, self.shutdown.as_ref(), None).await` continues the conversation — rig's multi-turn loop appends the new prompt and prior tool calls/results
   - **And** on `Ok((accumulated_text, new_history))`: re-acquire the lock and re-insert `SubAgentState { agent: state.agent, history: new_history, role: state.role, model: state.model }` under the SAME `id` — preserving the provider, model, and extended history
   - **And** on `Err(prompt_error)` — **critical non-destructive policy**: re-insert the ORIGINAL state (with the ORIGINAL `history`, not a mutated one) under the SAME `id` so the parent LLM can retry the follow-up after a transient 429/timeout. The agent instance is reused from `state.agent` (which was not consumed — `stream_chat` takes `&self`). This is tested by AC-8.4.
   - **And** the tool returns `Ok(build_success_json(&id, &accumulated_text))` on success or `Ok(build_error_json(&id, &error_string))` on execution failure — same `session_id` either way (AC-5)

4. **AC-4: Tool `definition()` description matches the Zed delegation guideline pattern**
   - **Given** `SpawnAgentTool::definition()` is called
   - **When** the tool description is generated
   - **Then** the description string is at least **400 characters** (asserted in tests; prevents vacuous descriptions that pass keyword checks while being useless) AND contains each of the five guidelines as a full sentence (not just keyword hits):
     1. A sentence that says sub-agents have no visibility into parent conversation history and the `message` must include all relevant context — assertable via the substring `"include all relevant context"` or `"sub-agents do not see"`
     2. A sentence that says subtasks must be concrete, well-defined, and self-contained — assertable via `"self-contained"` AND `"concrete"`
     3. A sentence that discourages use for tasks accomplishable with 1–2 direct tool calls — assertable via `"1-2"` or `"one or two"` AND `"tool call"`
     4. A sentence that says for follow-ups, pass the existing `session_id` and send a short direct message — assertable via `"follow-up"` AND `"session_id"` AND `"short"`
     5. A sentence that says for independent parallel tasks, spawn multiple sub-agents in parallel — assertable via `"parallel"` AND `"independent"`
   - **And** the description documents the output schema: success `{"session_id": "...", "output": "..."}` vs execution failure `{"session_id": "...", "error": "..."}` — both shapes literally appear in the description text
   - **And** the description documents the purpose of `label`: *"a short human-readable identifier for this spawn event (e.g., `\"audit module X\"`) — used for structured logging so you and the operator can correlate delegated work across trace output"*
   - **And** the `parameters` JSON schema declares `label` (string, required), `message` (string, required), `session_id` (string, optional) with short per-field descriptions aligned with the above

5. **AC-5: Sub-agent execution errors return structured JSON via pure helpers**
   - **Given** the sub-agent's `stream_chat()` returns `Err(PromptError)` (e.g., provider timeout, 429, context overflow)
   - **When** the error is handled
   - **Then** `call()` returns `Ok(build_error_json(&session_id, &prompt_error.to_string()))` — NOT `Err`
     - Rationale: a sub-agent execution failure is a recoverable signal for the parent LLM ("the sub-agent couldn't finish, adjust your approach"); returning `Err` would poison the parent's tool-call loop with a rig-level error the LLM cannot inspect
   - **And** two private helper functions exist in `src/tools/spawn_agent.rs`, both pure/deterministic and independently unit-testable:
     - `fn build_success_json(session_id: &str, output: &str) -> String` — returns `serde_json::json!({"session_id": session_id, "output": output}).to_string()`. Must NOT return `Result` — `json!` on `{&str, &str}` cannot fail, and the return type as `String` makes the caller site trivial.
     - `fn build_error_json(session_id: &str, error: &str) -> String` — same shape with `"error"` key
   - **And** `session_id` is included in the JSON whenever a session was created before the error (new-session: UUID already generated; follow-up: the provided `id`)
   - **And** the error is logged via `tracing::warn!(action = "spawn_agent_exec_failed", session_id = %session_id, label = %args.label, error = %prompt_error, "Sub-agent execution failed")`
   - **And** `SpawnAgentError` is reserved exclusively for infrastructure failures: missing session (AC-3), agent build failure (provider/secret errors from `AgentFactory::build()`), and lock poisoning (AC-6)

6. **AC-6: Tool holds shared state with poison-safe access and optional cooperative shutdown**
   - **Given** the `SpawnAgentTool` struct
   - **When** the struct is defined
   - **Then** the fields are:
     - `agent_factory: Arc<AgentFactory>` — shared daemon factory for building sub-agents
     - `role: LlmRole` — captured at construction time; copies parent session's provider/model
     - `project_root: PathBuf` — used to construct the 7 base tools for each sub-agent
     - `sessions: Arc<Mutex<HashMap<String, SubAgentState>>>` — shared map for follow-up continuity
     - `shutdown: Option<ShutdownFlag>` — optional cooperative shutdown flag, forwarded to `stream_chat` as `self.shutdown.as_ref()` so SIGINT/SIGTERM propagates into sub-agent loops
   - **And** `SubAgentState { agent: BuiltAgent, history: Vec<rig::completion::Message>, role: LlmRole, model: String }` is defined in the same file. The struct implements `Debug` via a manual impl (since `BuiltAgent: Debug` already exists, a `#[derive(Debug)]` compiles). `SubAgentState` does NOT derive or implement `Serialize`/`Deserialize` — it is in-memory-only state.
   - **And** **SpawnAgentTool derives only `Debug` — NOT `Serialize`, NOT `Deserialize`.** Rig's `Tool` trait does not require either derive on the tool struct. Deriving them here is not possible without scaffolding: `Arc<AgentFactory>` has no `Default` (required for `#[serde(skip)]`), and `LlmRole` has no serde derives today. Adding both would be out-of-scope scaffolding with no runtime use (tools are never serialized at runtime). The `AskSupervisor` precedent's derives work only because all its skipped fields implement `Default` and all its un-skipped fields are serde-compatible — preconditions that do NOT hold here. This deviation from precedent is explicit and intentional.
   - **And** a private helper `fn lock_sessions(&self) -> Result<MutexGuard<'_, HashMap<String, SubAgentState>>, SpawnAgentError>` wraps `self.sessions.lock()` with `.map_err(|poisoned| SpawnAgentError::LockPoisoned { recovered: poisoned.into_inner() })` → actually, simpler policy: **on poison, recover the inner guard and log a warning** because killing the daemon on transient panic is worse than proceeding with possibly-stale data. Final shape: `fn lock_sessions(&self) -> MutexGuard<'_, HashMap<String, SubAgentState>> { self.sessions.lock().unwrap_or_else(|poisoned| { tracing::error!(action = "spawn_agent_mutex_poisoned", "Sub-agent sessions mutex was poisoned — recovering inner data"); poisoned.into_inner() }) }`. All `call()` paths use this helper — NO direct `.lock().expect(...)`. Because the helper cannot fail, `SpawnAgentError::LockPoisoned` is redundant and is NOT added to the enum.
   - **And** the constructor signature is: `pub fn new(agent_factory: Arc<AgentFactory>, role: LlmRole, project_root: PathBuf, sessions: Arc<Mutex<HashMap<String, SubAgentState>>>, shutdown: Option<ShutdownFlag>) -> Self`

7. **AC-7: `uuid` crate added to `Cargo.toml`**
   - **Given** `Cargo.toml` has no `uuid` dependency today (verified at `grep uuid Cargo.toml` → no match)
   - **When** this story is implemented
   - **Then** `uuid = { version = "1", features = ["v4"] }` is added to the `[dependencies]` section — placed alphabetically (after `tracing-subscriber`, before the `[dev-dependencies]` section header)
   - **And** `Cargo.lock` is regenerated by `cargo build` and committed

8. **AC-8: Unit tests in `src/tools/spawn_agent.rs`**
   - **Given** the `SpawnAgentTool` implementation
   - **When** `cargo test` is run
   - **Then** the following unit tests exist and pass. Total: **10 new tests** (net +10 test count). Tests marked *(no LLM traffic)* are pure unit tests; the remaining tests use factored-out helpers and mocked state — none of these tests call real provider APIs.
     - **8.1** `test_spawn_agent_definition_has_name` *(no LLM traffic)* — verifies `SpawnAgentTool::NAME == "spawn_agent"` and `definition("".to_string()).await.name == "spawn_agent"`
     - **8.2** `test_spawn_agent_definition_meets_quality_bar` *(no LLM traffic)* — asserts `def.description.len() >= 400` AND the description contains each of the 11 required substrings defined in AC-4 (5 guideline phrases × 1–3 substrings each, plus both JSON shapes `"\"session_id\""` and `"\"output\""` and `"\"error\""`, plus the `label` purpose phrase `"structured logging"`). Use one `assert!(desc.contains(s), "missing: {s}")` per required substring.
     - **8.3** `test_spawn_agent_definition_parameters_schema` *(no LLM traffic)* — parse `def.parameters` as `serde_json::Value`, assert `.properties.label.type == "string"` and similar for `message` and `session_id`; assert `.required` is a JSON array containing `"label"` and `"message"` but NOT `"session_id"`
     - **8.4** `test_spawn_agent_session_not_found_returns_error` *(no LLM traffic)* — construct tool with empty sessions map, call with `session_id: Some("bogus-id".to_string())`, assert `matches!(err, SpawnAgentError::SessionNotFound { session_id }) where session_id == "bogus-id"` and that `err.to_string() == "No sub-agent session found for id: bogus-id"`
     - **8.5** `test_build_success_json_shape` *(no LLM traffic, pure helper)* — `build_success_json("abc", "hello")` produces JSON parseable to `{"session_id": "abc", "output": "hello"}` with two keys and no others; verifies AC-5
     - **8.6** `test_build_error_json_shape` *(no LLM traffic, pure helper)* — `build_error_json("abc", "timeout")` produces JSON parseable to `{"session_id": "abc", "error": "timeout"}`; verifies AC-5 error path without an LLM call
     - **8.7** `test_build_success_json_escapes_special_chars` *(no LLM traffic)* — `build_success_json("id", "line1\nline2\"quoted\"")` produces valid JSON (round-trip-parseable); verifies the helpers delegate to `serde_json` rather than string-concatenate
     - **8.8** `test_spawn_agent_error_is_send_sync`, **8.9** `test_spawn_agent_struct_is_send_sync`, **8.10** `test_spawn_agent_state_is_send_sync` — three one-line `fn assert_send_sync<T: Send + Sync>() {} assert_send_sync::<T>();` gates. `Arc<AgentFactory>` + `Arc<Mutex<HashMap>>` + `Option<ShutdownFlag>` are all `Send + Sync`; `BuiltAgent` already passes a compile-time `Send + Sync` assertion (agent_factory.rs:165–173).
   - **And** three happy-path tests deferred to Story 12.5 (documented for continuity): `test_spawn_agent_new_session_returns_session_id`, `test_spawn_agent_follow_up_reuses_session`, `test_spawn_agent_session_cleanup`. These require either a live provider call or a mock-agent factory that Story 12.5 will set up alongside pipeline wiring.
   - **And** a follow-up-error non-destructive behavior test (`test_spawn_agent_follow_up_error_preserves_session`) is also deferred to 12.5 because it requires a mock `AgentFactory` that returns `Err` from `stream_chat`. AC-3's non-destructive policy is documented in the story and code comment until 12.5's mock lands. This is the one known coverage gap.

9. **AC-9: Compilation, clippy, and test counts**
   - **Given** the baseline from Story 12.2 is **1124 passing, 1 pre-existing failure** (`test_build_context_limit_recovery_message_contains_all_sections`)
   - **When** `cargo build`, `cargo clippy`, and `cargo test` are run
   - **Then** `cargo build` produces zero new warnings (the 2 pre-existing `cargo clippy` errors in `src/session/branch.rs` remain — do not touch)
   - **And** `cargo clippy` introduces zero new warnings in `src/tools/spawn_agent.rs`, `src/tools/mod.rs`, and `src/session/agent.rs` (the additive `build_sub_agent_preamble` helper must not trigger lints)
   - **And** the expected test count is **1124 + 10 = 1134 passing**, 1 pre-existing failure (the 10 new tests are all AC-8.1 through AC-8.10; no existing tests are touched)
   - **And** `cargo doc --no-deps` produces no broken intra-doc links for the new public items (`SpawnAgentTool`, `SpawnAgentArgs`, `SpawnAgentError`, `SubAgentState`, `build_sub_agent_preamble`)

## Tasks / Subtasks

- [x] Task 1: Add `uuid` dependency (AC: #7)
  - [x] 1.1 Edit `Cargo.toml` — insert `uuid = { version = "1", features = ["v4"] }` alphabetically between `tracing-subscriber` and the `[dev-dependencies]` header
  - [x] 1.2 Run `cargo build` once so `Cargo.lock` is regenerated with `uuid`
  - [x] 1.3 Verify via `cargo tree | grep uuid` that `uuid v1.x` appears as a direct dependency

- [x] Task 2: Add `build_sub_agent_preamble()` to `src/session/agent.rs` (AC: #2)
  - [x] 2.1 Add a new `pub fn build_sub_agent_preamble(model: &str) -> String` after `build_preamble()` (~L268). Reuse the same `format!` template but with the tool inventory line changed to `"You have access to these tools: edit_file, read_file, grep, find_path, list_directory, git, terminal, plus a built-in think tool for reasoning."` — no `ask_supervisor`, no `spawn_agent`, no MCP line (sub-agents receive no MCP tools per AC-2)
  - [x] 2.2 Omit the `ask_supervisor` reference from the "Tool Usage Rules" section too — remove the bullet `"Use `ask_supervisor` when you need clarification..."` from the sub-agent template
  - [x] 2.3 Retain: tool usage rules, branch management, completion sentinel, English override, sequential-tool workaround for preview models, session completion protocol, and communication section
  - [x] 2.4 Retain the persona activation rules at the bottom of the preamble (same as `build_preamble` — sub-agents may receive persona files in context; this is a low-risk dormant rule for new-session first-user-message style)
  - [x] 2.5 Add a doc comment explaining the divergence from `build_preamble()`: sub-agents get a different tool inventory line because their registered tool set differs
  - [x] 2.6 Add three unit tests: `test_build_sub_agent_preamble_excludes_ask_supervisor` (asserts `!preamble.contains("ask_supervisor")`), `test_build_sub_agent_preamble_excludes_spawn_agent` (asserts `!preamble.contains("spawn_agent")`), `test_build_sub_agent_preamble_retains_completion_sentinel` (asserts `preamble.contains("<<BMAD_JOB_DONE>>")`). These three tests are in `src/session/agent.rs` and are counted separately from the 10 tests in AC-8 — adjust test count claim if needed (see Task 8.3)

- [x] Task 3: Create `src/tools/spawn_agent.rs` with core types (AC: #1, #6)
  - [x] 3.1 Module-level doc comment: purpose, Zed inspiration, contrast with daemon-orchestrated consultations (architecture.md Decision 10), shared-state design with `Arc<Mutex<HashMap>>`, note that ownership of the sessions map is Story 12.4 scope
  - [x] 3.2 Define `SpawnAgentArgs { label: String, message: String, session_id: Option<String> }` with `#[derive(Debug, Deserialize)]`
  - [x] 3.3 Define `SpawnAgentError` enum with `#[derive(Debug, thiserror::Error)]`: variants `SessionNotFound { session_id: String }` (Display: `"No sub-agent session found for id: {session_id}"`) and `AgentBuildFailed { reason: String }` (Display: `"Failed to build sub-agent: {reason}"`). Do NOT add `LockPoisoned` — the `lock_sessions()` helper recovers from poisoning and never errors
  - [x] 3.4 Define `SubAgentState { agent: BuiltAgent, history: Vec<rig::completion::Message>, role: LlmRole, model: String }` with `#[derive(Debug)]`. Does NOT derive serde traits. Marker comment: "In-memory-only state — never serialized, dropped when the parent pipeline drops the sessions map (Story 12.4 owns the lifecycle)"
  - [x] 3.5 Define `SpawnAgentTool` struct with **only `#[derive(Debug)]`** (see AC-6). Fields: `agent_factory: Arc<AgentFactory>`, `role: LlmRole`, `project_root: PathBuf`, `sessions: Arc<Mutex<HashMap<String, SubAgentState>>>`, `shutdown: Option<ShutdownFlag>`
  - [x] 3.6 Implement `SpawnAgentTool::new(agent_factory, role, project_root, sessions, shutdown) -> Self` with a doc comment listing each field's purpose
  - [x] 3.7 Implement private helper `fn lock_sessions(&self) -> MutexGuard<'_, HashMap<String, SubAgentState>>` per AC-6 — `unwrap_or_else(|p| { tracing::error!(...); p.into_inner() })`
  - [x] 3.8 Implement private helpers `fn build_success_json(session_id: &str, output: &str) -> String` and `fn build_error_json(session_id: &str, error: &str) -> String` per AC-5. Both use `serde_json::json!({...}).to_string()`

- [x] Task 4: Implement `Tool::definition()` (AC: #4)
  - [x] 4.1 Populate `ToolDefinition::name` as `"spawn_agent"`
  - [x] 4.2 Write the description string (target length 500–800 chars — well above the 400-char floor in AC-8.2). Structure as:
    - One-sentence purpose
    - "**When to use**" paragraph — delegation scenarios
    - "**When NOT to use**" paragraph — "Do not spawn sub-agents for tasks accomplishable with one or two direct tool calls — the delegation overhead is not worth it."
    - "**Guidelines**" numbered list with 5 items matching AC-4 exactly
    - "**Output**" section showing both JSON shapes literally (`{"session_id": "...", "output": "..."}` and `{"session_id": "...", "error": "..."}`)
    - "**label**" sentence explaining the field is for structured logging correlation (per AC-4)
  - [x] 4.3 Populate `parameters` JSON schema with three properties; mark `label` and `message` as required; each property gets a `description` field
  - [x] 4.4 Add a doc comment above `definition()` noting that description quality directly drives LLM delegation behavior (per architecture.md:802)

- [x] Task 5: Implement `Tool::call()` — new-session path (AC: #2, #5, #6)
  - [x] 5.1 Log entry: `tracing::info!(action = "spawn_agent_start", label = %args.label, follow_up = args.session_id.is_some(), role = ?self.role, "Spawning sub-agent")`
  - [x] 5.2 Branch on `args.session_id.is_none()`. Resolve model + preamble:
    ```rust
    let model = self.agent_factory.config_for_role(self.role).model.clone();
    let preamble = crate::session::agent::build_sub_agent_preamble(&model);
    let (git, read_file, edit_file, grep, find_path, list_dir, terminal) =
        crate::session::agent::create_base_tools(&self.project_root);
    ```
  - [x] 5.3 Build the agent:
    ```rust
    let agent = self
        .agent_factory
        .build(
            self.role,
            &preamble,
            crate::configure_agent_tools!(git, read_file, edit_file, grep, find_path, list_dir, terminal, rig::tools::think::ThinkTool),
        )
        .await
        .map_err(|e| SpawnAgentError::AgentBuildFailed { reason: e.to_string() })?;
    ```
  - [x] 5.4 Generate UUID: `let session_id = uuid::Uuid::new_v4().to_string();`
  - [x] 5.5 Run the stream:
    ```rust
    // NOTE: effective turn cap is STREAMING_MAX_TURNS (300) from session/agent.rs.
    // Epic 12.3 AC's "default 100" is deferred to a future tuning pass.
    let result = agent
        .stream_chat(
            rig::completion::Message::user(args.message.clone()),
            vec![],
            self.shutdown.as_ref(),
            None,
        )
        .await;
    ```
  - [x] 5.6 On `Ok((text, history))`: insert `SubAgentState { agent, history, role: self.role, model }` via `self.lock_sessions()` under `session_id.clone()`; log `tracing::info!(action = "spawn_agent_complete", session_id = %session_id, label = %args.label, output_len = text.len(), history_len = self.lock_sessions().get(&session_id).map(|s| s.history.len()).unwrap_or(0), "Sub-agent completed")` (NO `output = %text` — see Anti-Patterns); return `Ok(build_success_json(&session_id, &text))`
  - [x] 5.7 On `Err(prompt_error)`: `tracing::warn!(action = "spawn_agent_exec_failed", session_id = %session_id, label = %args.label, error = %prompt_error, "Sub-agent execution failed")`; DO NOT insert into the map; return `Ok(build_error_json(&session_id, &prompt_error.to_string()))`

- [x] Task 6: Implement `Tool::call()` — follow-up-session path (AC: #3, #5, #6)
  - [x] 6.1 On `Some(id)`: atomic remove under a short-lived lock:
    ```rust
    let state = {
        let mut guard = self.lock_sessions();
        guard.remove(&id).ok_or_else(|| SpawnAgentError::SessionNotFound { session_id: id.clone() })?
    }; // guard dropped here
    ```
  - [x] 6.2 Run the stream with ownership-preserving borrow (`state.agent.stream_chat` uses `&self`, so `state.agent` is NOT consumed):
    ```rust
    let original_history = state.history.clone();  // kept for non-destructive error path
    let result = state
        .agent
        .stream_chat(
            rig::completion::Message::user(args.message.clone()),
            state.history,
            self.shutdown.as_ref(),
            None,
        )
        .await;
    ```
  - [x] 6.3 On `Ok((text, new_history))`: re-insert updated state under SAME id:
    ```rust
    self.lock_sessions().insert(
        id.clone(),
        SubAgentState { agent: state.agent, history: new_history, role: state.role, model: state.model.clone() },
    );
    tracing::info!(action = "spawn_agent_followup_complete", session_id = %id, label = %args.label, output_len = text.len(), "Sub-agent follow-up completed");
    Ok(build_success_json(&id, &text))
    ```
  - [x] 6.4 On `Err(prompt_error)` — **non-destructive**: re-insert with ORIGINAL history (the session survives a transient failure):
    ```rust
    self.lock_sessions().insert(
        id.clone(),
        SubAgentState { agent: state.agent, history: original_history, role: state.role, model: state.model.clone() },
    );
    tracing::warn!(action = "spawn_agent_followup_failed", session_id = %id, label = %args.label, error = %prompt_error, "Sub-agent follow-up failed — session preserved for retry");
    Ok(build_error_json(&id, &prompt_error.to_string()))
    ```
  - [x] 6.5 Add a doc comment on the follow-up branch explaining the non-destructive policy and referencing this story by number

- [x] Task 7: Export from `src/tools/mod.rs` (AC: #1)
  - [x] 7.1 Add `pub mod spawn_agent;` alphabetically (after `read_file` line 16)
  - [x] 7.2 Add `pub use spawn_agent::SpawnAgentTool;` alphabetically (after `pub use read_file::ReadFileTool;` line 24)
  - [x] 7.3 Update the module-level doc comment to list `SpawnAgentTool` — describe as "daemon-provided; fresh sub-agents for delegated tasks with follow-up via `session_id`"
  - [x] 7.4 Do NOT add `SpawnAgentTool` to `create_base_tools()` or any `configure_agent_tools!` call — Story 12.4 scope

- [x] Task 8: Unit tests in `src/tools/spawn_agent.rs` (AC: #8)
  - [x] 8.1 Test fixture: expose `make_test_config()` and `make_test_secrets()` as `#[cfg(test)] pub(crate)` at the bottom of `src/llm/agent_factory.rs` — minimal refactor (change two `fn`s to `pub(crate) fn`). This is a 2-line change to one test-only block. Add a helper inside `spawn_agent.rs`:
    ```rust
    #[cfg(test)]
    fn test_tool() -> SpawnAgentTool {
        use crate::llm::agent_factory::{make_test_config, make_test_secrets};
        let factory = Arc::new(AgentFactory::new(Arc::new(make_test_config()), Arc::new(make_test_secrets())));
        SpawnAgentTool::new(
            factory,
            LlmRole::Dev,
            std::env::temp_dir(),
            Arc::new(Mutex::new(HashMap::new())),
            None,
        )
    }
    ```
  - [x] 8.2 Write the 10 tests listed in AC-8.1 through AC-8.10 exactly as specified
  - [x] 8.3 Task 2.6 adds 3 tests to `src/session/agent.rs` — update expected test count in AC-9 (1124 + 10 + 3 = 1137) during implementation if the count changes from this pre-dev estimate. Story uses **1134** as the target (AC-9 count matches Task 8 only); the 3 preamble tests are separately additive and push the real total to 1137. If the implementer bundles the helper and tests as described, cite 1137 in Dev Agent Record's Completion Notes.

- [x] Task 9: Verify (AC: #9)
  - [x] 9.1 `cargo build` — zero new warnings. Confirm `uuid` compiles.
  - [x] 9.2 `cargo clippy -- -D warnings` — zero new errors in the new/modified files (pre-existing errors in `src/session/branch.rs` are out of scope; running with `-D warnings` may still fail on those pre-existing errors — in that case run `cargo clippy` without `-D warnings` and manually review output for any NEW items in the touched files)
  - [x] 9.3 `cargo test` — expect **1134 passing** (Story 12.2 baseline 1124 + 10 new tests from Task 8) OR **1137 passing** if Task 2.6 preamble tests land in the same commit. Either target is acceptable; record the actual count in the Dev Agent Record.
  - [x] 9.4 `cargo doc --no-deps` — no broken intra-doc links on new public items

## Dev Notes

### Epic 12 Context — Story 12.3 is on the Parallel Branch

Epic 12 has two parallel branches converging at Story 12.5:
- **Skill activation branch:** 12.1 → 12.2 (done)
- **SpawnAgent branch:** 12.3 (this story) → 12.4 → 12.5

Story 12.3 is **parallel** with Story 12.1 per the epic summary — it has **no dependency on 12.1 or 12.2**. The skill-based activation changes in Stories 12.1/12.2 do not touch `src/tools/`. The `build_preamble()` changes from 12.1 are inherited for any code reusing `build_preamble()`, but this story introduces a separate `build_sub_agent_preamble()` helper because the tool inventory differs (see Task 2).

Story 12.4 is the integration story — it adds `SpawnAgentTool` to `create_base_tools()`, wires the shared sessions map through the pipeline, and evaluates migrating `ArchitectSession`. **Do NOT attempt any of that here.**

### Rig Tool Implementation Pattern — Adapted for Shared Runtime State

The project's standard rig tool pattern (architecture.md:769–812) prescribes `#[derive(Debug, Serialize, Deserialize)]` on the tool struct. `SpawnAgentTool` **deliberately diverges** — it derives `Debug` only. See AC-6 for the full rationale. The divergence is:

- `Arc<AgentFactory>` has no `Default` — `#[serde(skip)]` would fail to compile without scaffolding
- `LlmRole` has no `Serialize` / `Deserialize` derives — adding them is out-of-scope cross-module work
- Rig never deserializes tools at runtime — the `Serialize + Deserialize` bound on other tools is convention, not requirement

The `AskSupervisor` precedent (supervisor/mod.rs:103) works only because all its fields either implement `Default` (when `#[serde(skip)]`) or are fully serde-compatible — preconditions that do not hold for `SpawnAgentTool`. Do NOT "follow the AskSupervisor precedent" blindly here.

### `LlmRole` — Parent Role Captured at Construction

The sub-agent inherits the parent session's provider/model. `LlmRole` is captured at construction time. Current roles: `Dev`, `Review`, `Supervisor`, `EpicReview` (agent_factory.rs:37).

**Edge case — Supervisor role recursion:** If a future caller wires `SpawnAgentTool` into an `ArchitectSession` (Story 12.4 explicitly evaluates this), the parent role will be `LlmRole::Supervisor`. Sub-agents will then also run on the supervisor provider/model. This is consistent with the epic AC ("same provider/model as the parent session's role") but produces compounding supervisor costs. Story 12.3 inherits the role unconditionally — Story 12.4 is responsible for deciding whether to register `SpawnAgentTool` in Architect sessions at all.

### Sub-Agent Preamble — Why a New Helper

`build_preamble()` hardcodes the tool inventory string as: `"You have access to these tools: edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor, plus a built-in think tool for reasoning."` (session/agent.rs:226). Reusing this preamble for sub-agents would tell the sub-agent it has `ask_supervisor` when it does not — the sub-agent would then call `ask_supervisor`, and rig would return an "unknown tool" error.

`build_sub_agent_preamble()` (added in Task 2) produces an identical preamble structure but with an accurate tool inventory line: `"edit_file, read_file, grep, find_path, list_directory, git, terminal, plus a built-in think tool for reasoning."` — no `ask_supervisor`, no `spawn_agent`, no MCP line. The "Tool Usage Rules" section also drops the `ask_supervisor` bullet.

### Turn Cap Decision — Accept 300, Defer 100

The epic AC says "up to a configurable max turns, default 100" but Story 12.3 intentionally uses rig's existing `STREAMING_MAX_TURNS = 300`:

- `BuiltAgent::stream_chat()` calls `streaming_chat()` which hardcodes `.multi_turn(STREAMING_MAX_TURNS)`
- Introducing a 100-turn cap requires refactoring `streaming_chat()` to accept `max_turns: Option<usize>` — out-of-scope for this story
- Inlining the streaming loop in `spawn_agent.rs` duplicates ~100 lines of tool-call/UI-event dispatch — rejected as DRY violation
- 300 is a safe upper bound; real sub-agent workloads are expected to finish in <50 turns

A `// NOTE:` comment in `call()` flags this deferral. A future tuning story can parameterize it.

### `std::sync::Mutex` and Async — Critical Pattern

`self.sessions` uses `std::sync::Mutex`, NOT `tokio::sync::Mutex`. The standard Mutex is not async-safe — holding its guard across `.await` can deadlock under tokio. The pattern used in Tasks 5 and 6:

```
LOCK → remove OR insert (no await) → DROP GUARD → AWAIT stream_chat → LOCK → insert
```

The `{ let mut guard = ...; ... }` block with explicit scope guarantees the guard drops before `.await`. DO NOT refactor to `tokio::sync::Mutex` — the contention is microseconds per map op; adding async mutex is unnecessary complexity.

### Ownership in Follow-Up — Non-Destructive Error Path

`BuiltAgent::stream_chat(&self, ...)` takes `&self` — so `state.agent` is NOT consumed by the `.await`. After the stream completes (success OR failure), `state.agent` is still owned by the outer scope and can be re-inserted into the map.

**Non-destructive policy (AC-3):** On follow-up error, re-insert the original state with `original_history` (cloned before the `.await`) under the SAME id. A transient 429 or timeout must not kill the session — the parent LLM should be able to retry. Previously this story said "session is lost on error" — that was wrong; fixed per adversarial review.

`state.history` is `Vec<Message>` which is `Clone` — cloning before the stream is cheap enough (bounded by rig's history cap).

### Error Semantics — `Err` vs `Ok(json)`

| Failure type | Return path | Rationale |
|---|---|---|
| Follow-up with unknown `session_id` | `Err(SpawnAgentError::SessionNotFound)` | Caller bug — LLM passed a stale ID; rig surfaces the error to the LLM, which can retry with `session_id: None` |
| `AgentFactory::build()` fails (bad API key, unsupported provider) | `Err(SpawnAgentError::AgentBuildFailed)` | Infrastructure error — no session was ever created, nothing to report in JSON; daemon should escalate |
| Sub-agent `stream_chat()` fails (timeout, 429, context limit) — **new session** | `Ok(build_error_json(session_id, error))` | Recoverable — LLM inspects the error and decides next action; session is NOT inserted |
| Sub-agent `stream_chat()` fails — **follow-up** | `Ok(build_error_json(id, error))` + re-insert original state | Recoverable + session preserved for retry (AC-3) |

### Cooperative Shutdown

The tool stores `shutdown: Option<ShutdownFlag>` and forwards it to both `stream_chat` calls. Story 12.4's pipeline wiring passes `Some(Arc::clone(&self.shutdown))` from the parent runner. Sub-agents then honor SIGINT/SIGTERM at the same cadence as the parent session — `streaming_chat` checks the flag between chunks (session/agent.rs:319–332).

For standalone tests (without a parent runner), pass `None` — stream runs without shutdown coordination, which is fine in unit tests.

### Session ID Strategy — UUID v4

- `uuid::Uuid::new_v4().to_string()` → 36-char hyphenated form (e.g., `550e8400-e29b-41d4-a716-446655440000`)
- Good enough for in-process uniqueness — the map is never persisted
- DO NOT use `chrono::Utc::now().timestamp_nanos()` — parallel spawns can collide
- DO NOT shorten — readable UUIDs are easier for the LLM to copy-paste between tool calls

### `label` Field — Purpose

`label` is a short human-readable identifier (e.g., `"audit module X"` or `"parse config file"`). Its only use is structured logging: `tracing::info!(label = %args.label, ...)` correlates spawn events across trace output so operators can trace delegated work back to the spawning decision. It is NOT passed to the sub-agent, NOT returned in the JSON output, and NOT displayed in UI.

### `SubAgentState` Lifecycle Contract

- `SubAgentState` is in-memory only — never serialized, never persisted
- The sessions `HashMap` is owned by Story 12.4's pipeline — when the pipeline drops the map at story completion, all `SubAgentState` entries drop naturally (`BuiltAgent` has no explicit cleanup — the rig `Agent` has no drop ceremony)
- **For Story 12.3 standalone:** the sessions map is created fresh per test (each `test_tool()` call makes its own `Arc::new(Mutex::new(HashMap::new()))`) — no retention
- **For Story 12.4 integration:** the map's lifetime will span the parent story pipeline, per epic 12.4 AC #3: "created once per daemon run and shared across all tool instances... sub-agent sessions are dropped when the parent story pipeline completes"

### File Impact Summary

| File | Change type | Scope |
|---|---|---|
| `Cargo.toml` | **Minor** — add `uuid` dependency | 1 line added |
| `src/session/agent.rs` | **Additive** — new `build_sub_agent_preamble()` helper + 3 unit tests | ~80 lines added; `build_preamble()` untouched |
| `src/llm/agent_factory.rs` | **Trivial** — upgrade `make_test_config()` and `make_test_secrets()` from `fn` to `pub(crate) fn` inside the existing `#[cfg(test)]` block | 2 lines changed |
| `src/tools/spawn_agent.rs` | **New** — full module: struct, args, error enum, helpers, `Tool` impl, 10 unit tests | ~500 lines added |
| `src/tools/mod.rs` | **Trivial** — add `pub mod spawn_agent;`, `pub use spawn_agent::SpawnAgentTool;`, update module doc | 3 lines added |
| `Cargo.lock` | Auto-regenerated | — |

**NOT modified in this story (Story 12.4 scope):**
- `src/session/agent.rs::create_base_tools()` — unchanged (only the new helper function is added)
- `src/session/runner.rs` — NOT changed
- `src/review/mod.rs` — NOT changed
- `src/supervisor/architect.rs` — NOT changed (potential migration is 12.4 scope)
- `src/pipeline.rs` — NOT changed (sessions map wiring is 12.4 scope)
- `src/supervisor/mod.rs` — NOT changed
- `src/llm/agent_factory.rs::LlmRole` — NOT changed (no serde derives added)

### Anti-Patterns to Avoid

- **DO NOT** register `SpawnAgentTool` in `create_base_tools()` — Story 12.4 scope
- **DO NOT** thread the sessions map through `SessionRunner` / `ReviewRunner` — Story 12.4 scope
- **DO NOT** modify `ArchitectSession` — potential migration to `spawn_agent` is Story 12.4 scope
- **DO NOT** modify `LlmRole` to add `Serialize`/`Deserialize` — not needed because `SpawnAgentTool` doesn't derive them (AC-6)
- **DO NOT** give sub-agents the `AskSupervisor` tool — sub-agents must not trigger escalation on the parent's slot
- **DO NOT** give sub-agents the `SpawnAgentTool` itself — prevents unbounded nested delegation
- **DO NOT** reuse `build_preamble()` for sub-agents — it claims `ask_supervisor` exists; use `build_sub_agent_preamble()` instead (Task 2)
- **DO NOT** hold `std::sync::Mutex` guards across `.await` — use the remove/drop-guard/await/re-insert pattern
- **DO NOT** use `.lock().expect(...)` or `.lock().unwrap()` — use the `lock_sessions()` helper which recovers from poisoning via `PoisonError::into_inner()` and logs the recovery
- **DO NOT** add `tokio::sync::Mutex` — standard Mutex is correct; contention is brief map ops
- **DO NOT** destroy sub-agent state on follow-up `stream_chat` error — re-insert with `original_history` so the parent LLM can retry (AC-3)
- **DO NOT** persist `SubAgentState` — in-memory only
- **DO NOT** introduce new config fields — `max_turns`, `preamble_override`, `tool_subset` are hardcoded for this story
- **DO NOT** refactor `streaming_chat()` to add `max_turns` — keep scope tight
- **DO NOT** return `Err` on sub-agent execution failure — return `Ok(build_error_json(...))` so the parent LLM can read and react
- **DO NOT** log the full `args.message` at `tracing::info!` — log `label` + `session_id` + `follow_up` flag only
- **DO NOT** log the full `accumulated_text` output at `tracing::info!` — log `output_len` only; full output goes into the JSON return value and is consumed by the parent LLM
- **DO NOT** inherit `LlmRole::Supervisor` blindly when registering the tool in Architect sessions — Story 12.4 must make an explicit decision about whether Architect sessions should spawn sub-agents at all
- **DO NOT** use `args.message` by reference (`&args.message`) when constructing `Message::user` — use `args.message.clone()` to avoid subtle borrow entanglements with later uses of `args`

### Previous Story Intelligence (Story 12.1 — 12-1-parameterize-activation-by-skill.md)

- **Baseline test count (pre-12.2):** 1131 passing, 1 pre-existing failure
- `build_preamble()` is skill-aware (persona rules retained, skill instruction added). Sub-agents get `build_sub_agent_preamble()` instead (Task 2) — different tool inventory
- `SessionRunner` has a `skill_path: String` field; `SpawnAgentTool` does NOT need one — sub-agents receive a raw user message
- Agent model used for 12.1: anthropic/claude-sonnet-4-6

### Previous Story Intelligence (Story 12.2 — 12-2-simplify-response-analyzer.md)

- **Baseline test count (post-12.2):** 1124 passing, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)
- `ResponseAnalyzer` changes do NOT affect `SpawnAgentTool` — sub-agent responses bypass the analyzer entirely
- Pre-existing: 2 clippy errors in `src/session/branch.rs` — untouched, out of scope
- Agent model used for 12.2: anthropic/claude-opus-4-7 (1M context)

### Git Intelligence — Recent Commits

Last 5 commits:
- `ec72cc2` `feat(epic-12): simplify ResponseAnalyzer (Story 12.2)`
- `e62467d` `docs(epic-9): complete code review story 9.3 — fix findings, mark done`
- `95723d0` `claude code` (amendment — ignore)
- `d9c7103` `docs(epic-12): complete code review story 12.1 — mark done, log deferred items`
- `c9e7c34` `feat(epic-12): parameterize activation by skill (Story 12.1)`

**Expected commit message:** `feat(epic-12): add SpawnAgentTool (Story 12.3)`

### Project Structure Notes

- `src/tools/spawn_agent.rs` fits the `tools/` convention (one tool per file)
- `SpawnAgentTool` is the 8th custom rig tool in the `tools/` directory. Counting the dev-session tool stack post-12.4: 7 base + `AskSupervisor` + `SpawnAgentTool` + `ThinkTool` = 10 tools via `configure_agent_tools!` (well under the macro's arity-12 ceiling at agent_factory.rs:526–528)
- `uuid` added alphabetically in `Cargo.toml`
- No new modules, no new directories

### References

- [Source: _bmad-output/planning-artifacts/epics.md:3015–3060 — Epic 12, Story 12.3 AC]
- [Source: _bmad-output/planning-artifacts/epics.md:3062–3086 — Story 12.4 scope, clarifies what NOT to do in 12.3]
- [Source: _bmad-output/planning-artifacts/epics.md:3118–3135 — Epic 12 Summary and Execution Strategy]
- [Source: _bmad-output/planning-artifacts/architecture.md:769–819 — Rig Tool Implementation Pattern + SpawnAgentTool note]
- [Source: _bmad-output/planning-artifacts/architecture.md:664–694 — Decision 10 (contrasts LLM-initiated `spawn_agent` with daemon-orchestrated consultations)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md:197–215 — Epic 12 overview]
- [Source: _bmad-output/project-context.md:46–62 — rig Agent + Tool Calling rules; "One tool = one concern"]
- [Source: _bmad-output/project-context.md:172 — Graceful shutdown contract]
- [Source: _bmad-output/project-context.md:202–213 — Critical Don't-Miss Rules]
- [Source: src/tools/grep.rs — simple shared-state rig tool template]
- [Source: src/tools/terminal.rs — rig tool with more complex state]
- [Source: src/supervisor/mod.rs:103–230 — `AskSupervisor` pattern (compare with AC-6 rationale for divergence)]
- [Source: src/llm/agent_factory.rs:37–57 — `LlmRole` enum (no serde derives; verified)]
- [Source: src/llm/agent_factory.rs:93–153 — `BuiltAgent::stream_chat()` / `activate_agent()` API signatures]
- [Source: src/llm/agent_factory.rs:165–173 — compile-time `Send + Sync` assertion for `BuiltAgent`]
- [Source: src/llm/agent_factory.rs:242–345 — `AgentFactory::build()` API]
- [Source: src/llm/agent_factory.rs:422–430, 502–528 — `configure_agent_tools!` macro and arity-12 ceiling]
- [Source: src/session/agent.rs:42–112 — `create_base_tools()` / `create_tools_with_supervisor()` / `TERMINAL_TIMEOUT_SECS`]
- [Source: src/session/agent.rs:114–121 — `ShutdownFlag` type and `STREAMING_MAX_TURNS` constant]
- [Source: src/session/agent.rs:204–268 — `build_preamble()` — skill-aware preamble; parent of new `build_sub_agent_preamble()`]
- [Source: src/session/agent.rs:291–394 — `streaming_chat()` — effective 300-turn cap]
- [Source: src/tools/mod.rs — module export patterns]
- [Source: Cargo.toml — dependency insertion point; currently no `uuid`]

## Dev Agent Record

### Agent Model Used

anthropic/claude-opus-4-7 (1M context)

### Debug Log References

- Initial `cargo build` after adding `uuid` succeeded — `uuid v1.23.1` confirmed via `cargo tree` as a direct dependency.
- First test run failed `test_spawn_agent_definition_meets_quality_bar` with "missing: sub-agents do not see" — the description had `Sub-agents` (capital S). Rephrased the guideline-1 sentence to "Remember that sub-agents do not see your conversation history." so the lowercase substring is present without sacrificing readability.
- First `cargo build` produced 11 new dead-code/unused-import warnings because `SpawnAgentTool` is not yet wired into the bin entry chain (Story 12.4 scope). Resolved by adding `#![allow(dead_code)]` at module scope in `src/tools/spawn_agent.rs`, `#[allow(dead_code)]` on `build_sub_agent_preamble` in `src/session/agent.rs`, and `#[allow(unused_imports)]` on the `pub use spawn_agent::SpawnAgentTool;` re-export in `src/tools/mod.rs` — each annotated with a comment pointing to Story 12.4.
- The shared test fixtures `make_test_config` / `make_test_secrets` in `agent_factory::tests` are referenced by `spawn_agent::tests`. Required two tweaks: marking the fns `pub(crate) fn` AND elevating the surrounding `mod tests` to `pub(crate) mod tests` so the cross-module path `crate::llm::agent_factory::tests::*` resolves under `cfg(test)`.

### Completion Notes List

- All 9 ACs implemented. Story 12.4 will register `SpawnAgentTool` in `create_base_tools` and own the sessions-map lifecycle.
- **Test count: 1137 passing, 1 pre-existing failure** (`session::runner::tests::test_build_context_limit_recovery_message_contains_all_sections`). Net delta vs Story 12.2 baseline (1124): +13 (10 from `tools::spawn_agent::tests` per AC-8 + 3 from `session::agent::tests` per Task 2.6). Matches the 1137 target in Task 9.3.
- `cargo clippy` reports 2 errors — both pre-existing in `src/session/branch.rs` (`needless_splitn`, `unnecessary_map_or`); neither touched per story constraints.
- `cargo doc --no-deps` reports 1 warning — pre-existing broken intra-doc link in `src/config/mod.rs:174` (`AgentFactory::config_for_role`); unrelated to this story.
- Three happy-path tests deferred to Story 12.5 per AC-8 (require a mock `AgentFactory`): `test_spawn_agent_new_session_returns_session_id`, `test_spawn_agent_follow_up_reuses_session`, `test_spawn_agent_session_cleanup`, plus the non-destructive follow-up-error test `test_spawn_agent_follow_up_error_preserves_session`. Non-destructive policy (AC-3) is documented in the `continue_followup` doc comment until 12.5's mock lands.
- `STREAMING_MAX_TURNS = 300` accepted per Dev Notes — refactoring `streaming_chat()` to add a per-call cap is explicitly deferred to a future tuning story. Marked with a `// NOTE:` comment in `spawn_new`.

### File List

- `Cargo.toml` — added `uuid = { version = "1", features = ["v4"] }`
- `Cargo.lock` — auto-regenerated by cargo build
- `src/session/agent.rs` — added `build_sub_agent_preamble()` helper + 3 unit tests
- `src/llm/agent_factory.rs` — `mod tests` → `pub(crate) mod tests`; `make_test_config` and `make_test_secrets` → `pub(crate) fn`
- `src/tools/spawn_agent.rs` — NEW module: `SpawnAgentArgs`, `SpawnAgentError`, `SubAgentState`, `SpawnAgentTool`, JSON helpers, `Tool` impl, 10 unit tests
- `src/tools/mod.rs` — added `pub mod spawn_agent;`, `pub use spawn_agent::SpawnAgentTool;` (with `#[allow(unused_imports)]` until 12.4 wires it), updated module-level doc

### Change Log

| Date | Author | Change |
|---|---|---|
| 2026-04-19 | Amelia (claude-opus-4-7) | Story 12.3 implementation complete — SpawnAgentTool with 10 unit tests + 3 sub-agent preamble tests; module wired for export but registration deferred to Story 12.4. |
| 2026-04-19 | Code Review (claude-opus-4-7) | Adversarial review complete — Blind Hunter + Edge Case Hunter + Acceptance Auditor. 5 `decision-needed`, 3 `patch`, 7 `defer`, ~20 dismissed. |

### Review Findings

- [x] [Review][Decision→Patch] **Concurrent follow-up race on same `session_id`** — Fixed by adding an `in_flight: Arc<Mutex<HashSet<String>>>` field on `SpawnAgentTool` + a new `SpawnAgentError::SessionBusy { session_id }` variant. A second concurrent follow-up on an id already streaming returns `SessionBusy` immediately (enforced by `InFlightGuard` RAII cleanup). **AC-6 constructor signature change:** `new()` now also takes `in_flight: Arc<Mutex<HashSet<String>>>` — Story 12.4 must construct and wire this alongside `sessions`. Covered by `test_spawn_agent_session_busy_when_in_flight`.
- [x] [Review][Decision→Patch] **Panic between `remove` and re-insert silently loses session** — Fixed by introducing `PanicReinsertGuard`: the guard holds the removed `SubAgentState` across the `.await` and re-inserts the original state during unwind. The normal Ok/Err paths call `disarm_and_take()` to cancel the fallback. AC-3 non-destructive invariant now extends to panics.
- [x] [Review][Decision→Patch] **`spawn_new` error path returns a `session_id` that is NOT in the map** — Fixed by adding `build_error_json_no_session(error)` helper and returning it from the `spawn_new` `Err` branch. **Minor AC-5 deviation documented:** `session_id` is now emitted only for error payloads where the session remains follow-up-capable. Covered by `test_build_error_json_no_session_omits_session_id`.
- [x] [Review][Decision→Patch] **Sub-agent preamble retains "Wait for user input after displaying the menu"** — Fixed by deleting the line from `build_sub_agent_preamble`. Doc comment updated to explain the divergence from `build_preamble`. **Minor Task 2.4 deviation documented:** persona rules are retained *except* the interactive wait-for-input rule (inapplicable to non-interactive sub-agents). Covered by `test_build_sub_agent_preamble_excludes_wait_for_user_input`.
- [x] [Review][Decision→Patch] **Non-destructive doc comment overpromises** — `continue_followup` doc now explicitly documents three guarantees: concurrent-follow-up rejection (`SessionBusy`), non-destructive on `Err` (AC-3), and non-destructive on panic (via `PanicReinsertGuard`).

- [x] [Review][Patch] **Clamp/sanitize `label` in tracing logs** [src/tools/spawn_agent.rs: `sanitize_label` + call sites] — Added `sanitize_label()` helper (control chars → space, truncation at `LABEL_LOG_MAX_LEN = 200` with UTF-8 char-boundary handling). All `%args.label` sites now route through it. Covered by `test_sanitize_label_truncates_and_strips_control_chars`.
- [x] [Review][Patch] **Empty `session_id = Some("")` treated as unknown follow-up** [src/tools/spawn_agent.rs: `call()`] — Normalized via `.filter(|s| !s.is_empty())` before routing to `spawn_new` / `continue_followup`. Covered by `test_spawn_agent_empty_session_id_is_treated_as_fresh_spawn`.
- [x] [Review][Patch] **Asymmetric `history_len` logging** [src/tools/spawn_agent.rs: `continue_followup` Ok branch] — Added `history_len = state.history.len()` to the follow-up success log for parity with `spawn_new`.

### Review Test Count Update

- Pre-review baseline: 1137 passing, 1 pre-existing failure.
- Post-review: **1142 passing, 1 pre-existing failure.** +5 new tests:
  - `test_build_sub_agent_preamble_excludes_wait_for_user_input`
  - `test_spawn_agent_session_busy_when_in_flight`
  - `test_spawn_agent_empty_session_id_is_treated_as_fresh_spawn`
  - `test_build_error_json_no_session_omits_session_id`
  - `test_sanitize_label_truncates_and_strips_control_chars`

### Review Deviations From Spec (Documented)

- **AC-6 constructor signature** — `SpawnAgentTool::new` now takes one additional arg: `in_flight: Arc<Mutex<HashSet<String>>>`. Rationale: atomic check-and-reserve of a per-session in-flight slot is not expressible with the single `sessions` map alone. Story 12.4 must supply this shared set alongside the sessions map.
- **AC-5 (`spawn_new` error payload)** — `session_id` omitted when the spawn did not produce a follow-up-capable session. Rationale: returning a UUID the parent cannot resume creates confusion (`SessionNotFound` on retry).
- **Task 2.4 (persona activation rules)** — Dropped the "Wait for user input after displaying the menu" rule from `build_sub_agent_preamble`. Rationale: sub-agents are non-interactive; the rule can stall persona execution.

- [x] [Review][Defer] **Unbounded `sessions` HashMap growth (no eviction)** [src/tools/spawn_agent.rs: map field] — deferred to Story 12.4 (owns map lifecycle per spec).
- [x] [Review][Defer] **No upfront empty-`message` validation** [src/tools/spawn_agent.rs: `call()`] — deferred; provider returns 400 and flows through `build_error_json` — acceptable for v1.
- [x] [Review][Defer] **`#![allow(dead_code)]` + public re-export create a permanent silence blanket** [src/tools/mod.rs + spawn_agent.rs:1] — deferred to Story 12.4 which registers the tool and lets us drop the blanket allow.
- [x] [Review][Defer] **Shutdown flag is a stored snapshot (possible staleness across parent-session rotation)** [src/tools/spawn_agent.rs:135] — deferred to Story 12.4 (ownership/lifecycle design).
- [x] [Review][Defer] **Shutdown race: sub-agent Err re-insert while daemon drops the `sessions Arc`** [src/tools/spawn_agent.rs: `continue_followup` Err branch] — deferred to Story 12.4 (lifecycle).
- [x] [Review][Defer] **Preamble negative-substring tests are brittle** [src/session/agent.rs:1150–1157] — deferred; polish, parse the tool-inventory line instead.
- [x] [Review][Defer] **No cross-check between preamble tool names and rig Tool `NAME` constants** [src/session/agent.rs:297] — deferred; add a small integration test that walks the registered tools and asserts each appears in the preamble string.
