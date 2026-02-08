# Story 4.2: Agent Session Setup & Chat Loop

Status: review
Dependencies: 4-1-rig-tools-implementation-git-filesystem-terminal (hard — tools must be implemented and exported before session can register them)

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the daemon to launch an autonomous LLM agent session with the BMAD dev persona and all registered tools,
So that stories are developed without human intervention.

## Acceptance Criteria

1. **Given** the session module is initialized with a `StoryInfo` from the watcher **When** an agent session is created **Then** the BMAD dev agent file is loaded from the project's `_bmad/` directory and used as-is as the rig agent preamble **And** a language override (`communication_language = English`) is appended to the preamble **And** four tools are registered: git, filesystem, terminal, and ask_supervisor **And** the agent is built using the dev LLM provider/model from `BotConfig`

2. **Given** an agent session is ready **When** the chat loop starts **Then** the first message sent is `"DS"` (triggers the dev-story workflow in the BMAD agent's menu system) **And** the daemon manages the chat loop via `agent.chat(message, history)`, analyzing each agent response for workflow interaction points (confirmations, "should I proceed?", step transitions) **And** the daemon responds automatically to workflow-level interactions

3. **Given** the agent is working in a chat loop **When** the agent completes the dev-story workflow and signals completion **Then** the session module detects completion and exits the chat loop **And** the session result (success, blocked, or error) is returned to the daemon for downstream processing (review, PR, notification)

4. **Given** a session is active **When** the entire session lifecycle runs **Then** a `story_session` tracing span wraps the whole session with `story_id` and `branch` fields **And** the daemon knows nothing about BMAD workflow internals — it only loads the agent file, registers tools, and manages the chat loop

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [x] **BLOCKING DEPENDENCY:** Verify Story 4.1 is `done` — tools must be fully implemented before this story can start
- [x] Verify Story 4.1 tools are implemented: `src/tools/git.rs`, `src/tools/fs.rs`, `src/tools/terminal.rs` export `GitTool`, `FsTool`, `TerminalTool`
- [x] Verify `src/tools/mod.rs` has `pub mod` declarations and `pub use` re-exports so that `use crate::tools::{GitTool, FsTool, TerminalTool}` compiles — if not, Story 4.1 is incomplete
- [x] Verify `src/session/mod.rs` already defines `SessionError`, `SessionOutcome`, submodules `cleanup`, `escalation`, `state`
- [x] Verify `src/session/cleanup.rs` already implements `preserve_partial_work()` and `mark_story_needs_clarification()`
- [x] Verify `src/session/escalation.rs` already defines `EscalationInfo`, `EscalationReport`
- [x] Verify `src/session/state.rs` is a stub (`TODO: Implemented in Story 2.1`) — this story replaces it with WAL implementation
- [x] Verify `src/supervisor/mod.rs` exports `AskSupervisor` with `with_all()`, `with_architect_from_config()`, `escalation_slot()`, `decision_log()`
- [x] Verify `src/config/mod.rs` exports `BotConfig`, `LlmConfig`, `LlmRoleConfig`, `BmadPathsConfig`, `BotSecrets`
- [x] Verify `src/watcher/mod.rs` exports `StoryInfo` with fields: `story_id`, `story_key`, `branch_name`, `specs_path`, etc.
- [x] Verify rig-core v0.30 dependencies: `rig::prelude::*`, `rig::message::Message`, `rig::completion::Chat`, `rig::tool::ToolDyn`
- [x] **Verify rig-core v0.30 API surface** — confirm the following assumptions against actual crate docs/source before coding:
  - [x] `agent.chat(&str, Vec<Message>)` returns `Result<String, _>` (not a stream or structured response)
  - [x] `Message::user(content)` and `Message::assistant(content)` are the correct constructors for building chat history
  - [x] `CompletionModel` trait is object-safe (can be used as `Box<dyn CompletionModel>`) — if NOT, flag immediately and adapt Task 3 to use an enum wrapper instead
  - [x] Provider client constructors: `anthropic::Client::new(&key)`, `openai::Client::new(&key)`, `openai::Client::from_url(&key, &base_url)` exist as expected
- [x] Run `cargo check` — clean baseline

### Task 1: Implement WAL State File (`src/session/state.rs`)

- [x] **1.1** Define `SessionState` struct
  - [x] `#[derive(Debug, Serialize, Deserialize)]`
  - [x] Field: `story_id: String`
  - [x] Field: `story_key: String`
  - [x] Field: `branch: String`
  - [x] Field: `started_at: String` — ISO 8601 timestamp
  - [x] Field: `last_activity: String` — ISO 8601 timestamp, updated each turn
  - [x] Field: `provider: String` — LLM provider name for reconstruction
  - [x] Field: `model: String` — LLM model name for reconstruction
  - [x] Field: `chat_history: Vec<ChatMessage>` — complete serialized history

- [x] **1.2** Define `ChatMessage` struct
  - [x] `#[derive(Debug, Clone, Serialize, Deserialize)]`
  - [x] Field: `role: String` — `"user"` or `"assistant"`
  - [x] Field: `content: String` — message content

- [x] **1.3** Define `StateError` thiserror enum
  - [x] `WriteFailed { path: String, reason: String }`
  - [x] `ReadFailed { path: String, reason: String }`
  - [x] `ParseFailed { path: String, reason: String }`
  - [x] `DeleteFailed { path: String, reason: String }`

- [x] **1.4** Implement `SessionState` methods
  - [x] `pub fn new(story: &StoryInfo, provider: &str, model: &str) -> Self` — creates initial state with empty history
  - [x] `pub fn add_user_message(&mut self, content: &str)` — appends user message, updates `last_activity`
  - [x] `pub fn add_assistant_message(&mut self, content: &str)` — appends assistant message, updates `last_activity`
  - [x] `pub fn to_rig_messages(&self) -> Vec<Message>` — converts `chat_history` to `rig::message::Message` vector for `agent.chat()`
  - [x] `pub async fn save(&self, path: &Path) -> Result<(), StateError>` — serialize to YAML, atomic write (write to `.tmp` then rename)
  - [x] `pub async fn load(path: &Path) -> Result<Self, StateError>` — read and deserialize from YAML file
  - [x] `pub async fn delete(path: &Path) -> Result<(), StateError>` — remove state file, ignore if already gone
  - [x] `pub fn exists(path: &Path) -> bool` — check if state file exists (for crash recovery detection)

- [x] **1.5** Write unit tests
  - [x] `test_session_state_new_has_empty_history`
  - [x] `test_session_state_add_messages_preserves_order`
  - [x] `test_session_state_to_rig_messages_converts_correctly`
  - [x] `test_session_state_save_load_roundtrip`
  - [x] `test_session_state_save_creates_file`
  - [x] `test_session_state_load_missing_file_returns_error`
  - [x] `test_session_state_delete_removes_file`
  - [x] `test_session_state_delete_missing_file_no_error`
  - [x] `test_session_state_exists_true_when_present`
  - [x] `test_session_state_exists_false_when_absent`
  - [x] `test_session_state_serializable_yaml_roundtrip`
  - [x] `test_state_error_is_send_sync`

### Task 2: Implement Response Analyzer (`src/session/analyzer.rs`)

- [x] **2.1** Define `ResponseAction` enum
  - [x] `Continue { reply: String }` — send this reply and continue the loop
  - [x] `Completed` — agent signaled workflow completion, exit loop
  - [x] `Escalated` — escalation detected via slot, exit loop
  - [x] `NoReply` — reserved for future streaming/async response support where the agent may still be processing tool calls; currently treated as `Continue("Continue.")` but kept as a distinct variant for forward-compatibility with rig streaming APIs

- [x] **2.2** Define `ResponseAnalyzer` struct
  - [x] Stateless analyzer — no fields, constructed once
  - [x] `pub fn new() -> Self`

- [x] **2.3** Implement `pub fn analyze(&self, response: &str, escalation_slot: &EscalationSlot, story_key: &str) -> ResponseAction`
  - [x] **Priority 1 — Escalation check:** If escalation slot contains `Some(EscalationInfo)`, return `ResponseAction::Escalated`
  - [x] **Priority 2 — Completion detection:** If response contains strong completion signals (e.g., "all tasks completed", "story implementation complete", "dev-story workflow complete", "Story marked as done"), return `ResponseAction::Completed`
  - [x] **Priority 3 — Confirmation/proceed patterns:** If response asks "Should I proceed?", "Continue?", "Ready to move on?", "Shall I continue?", reply `"Yes, proceed."` → return `Continue { reply }`
  - [x] **Priority 4 — Step-by-step detection:** If response indicates working step-by-step or asking for per-step approval, reply `"Continue with all steps. Do not ask for confirmation between steps."` → return `Continue { reply }`
  - [x] **Priority 5 — YOLO/mode questions:** If response asks about YOLO mode or batch vs interactive, reply `"Use YOLO mode. Complete all remaining work without asking for confirmation."` → return `Continue { reply }`
  - [x] **Priority 6 — Story selection:** If response asks which story to work on or needs story context, reply with `story_key` parameter value → return `Continue { reply: story_key.to_string() }`
  - [x] **Priority 7 — Default:** If none of the above match, reply `"Continue."` → return `Continue { reply: "Continue.".to_string() }`
  - [x] All pattern matching is case-insensitive substring search (use `.to_lowercase().contains()`)
  - [x] Log the chosen action via `tracing::debug!(action = "response_analysis", ...)`

- [x] **2.4** Write unit tests
  - [x] `test_analyzer_detects_completion_signal`
  - [x] `test_analyzer_detects_proceed_question`
  - [x] `test_analyzer_detects_step_by_step`
  - [x] `test_analyzer_detects_yolo_question`
  - [x] `test_analyzer_detects_escalation_from_slot`
  - [x] `test_analyzer_escalation_takes_priority_over_completion`
  - [x] `test_analyzer_default_continues`
  - [x] `test_analyzer_case_insensitive`
  - [x] `test_analyzer_completion_various_phrases` — test multiple completion signal variants
  - [x] `test_analyzer_proceed_various_phrases` — test multiple proceed patterns
  - [x] `test_analyzer_story_selection_replies_with_story_key` — verify Priority 6 returns the provided `story_key`

### Task 3: Implement LLM Provider Factory (`src/session/provider.rs`)

- [x] **3.1** Define `ProviderError` thiserror enum
  - [x] `UnsupportedProvider { provider: String }`
  - [x] `MissingApiKey { provider: String, env_var: String }`
  - [x] `ClientCreation { provider: String, reason: String }`

- [x] **3.2** Implement provider factory function
  - [x] **Pre-check:** Confirm `CompletionModel` trait is object-safe (verified in Task 0). If NOT object-safe, replace `Box<dyn CompletionModel>` with a `ProviderModel` enum wrapping each concrete provider model type and implement `CompletionModel` on the enum via delegation.
  - [x] `pub fn create_completion_model(role_config: &LlmRoleConfig, secrets: &BotSecrets) -> Result<Box<dyn CompletionModel>, ProviderError>`
  - [x] Match on `role_config.provider`:
    - `"anthropic"` → `rig::providers::anthropic::Client::new(&api_key).completion_model(&role_config.model)`
    - `"openai"` → `rig::providers::openai::Client::new(&api_key).completion_model(&role_config.model)`
    - `"github-models"` → OpenAI-compatible with base URL override: `rig::providers::openai::Client::from_url(&api_key, "https://models.inference.ai.azure.com").completion_model(&role_config.model)`
  - [x] Extract API key from `BotSecrets` based on provider name
  - [x] Return boxed `CompletionModel` for use with rig agent builder

- [x] **3.3** Write unit tests
  - [x] `test_provider_error_is_send_sync`
  - [x] `test_provider_error_display`
  - [x] `test_unsupported_provider_returns_error`
  - [x] Note: Cannot test actual provider creation without API keys — test error paths only, E2E tests for real provider

### Task 4: Implement Session Runner (`src/session/runner.rs`)

- [x] **4.1** Define `SessionRunner` struct
  - [x] Field: `config: Arc<BotConfig>`
  - [x] Field: `secrets: Arc<BotSecrets>`
  - [x] Field: `state_file_path: PathBuf` — WAL file location
  - [x] Field: `analyzer: ResponseAnalyzer`
  - [x] Constructor: `pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>) -> Self`
  - [x] The `state_file_path` is derived from config: `{implementation_artifacts}/.bmad-bot-session.yaml`

- [x] **4.2** Implement `pub async fn run(&self, story: &StoryInfo) -> SessionOutcome`
  - [x] Open a `tracing::info_span!("story_session", story_id = %story.story_id, branch = %story.branch_name)` span for the entire session
  - [x] Log session start: `tracing::info!(action = "session_start", story_key = %story.story_key, "Starting dev session")`
  - [x] **Step 1: Build agent** — call `self.build_agent(story)?`
  - [x] **Step 2: Create WAL** — `SessionState::new(story, provider, model)`, save to disk
  - [x] **Step 3: Create shared resources** — `EscalationSlot`, `DecisionLog` for supervisor
  - [x] **Step 4: Run chat loop** — call `self.chat_loop(agent, state, escalation_slot, decision_log, story)?`
  - [x] **Step 5: Handle result** — map chat loop result to `SessionOutcome`
  - [x] **Step 6: Cleanup** — delete WAL on success, preserve partial work on failure/escalation
  - [x] Wrap entire body in a top-level `match`/`if let Err` that converts all `SessionError` variants to `SessionOutcome::Failed`. Do NOT use `std::panic::catch_unwind` — async code is not `UnwindSafe` and catching panics across `await` points is unsound. Instead, rely on typed `Result` propagation for all recoverable errors; let genuine panics (logic bugs) propagate and crash the daemon for visibility.
  - [x] Log session end: `tracing::info!(action = "session_end", outcome = %outcome_type, "Dev session ended")`

- [x] **4.3** Implement `async fn build_agent(&self, story: &StoryInfo) -> Result<Agent, SessionError>`
  - [x] Resolve BMAD dev agent file path: `{project_root}/_bmad/bmm/agents/dev.md` (or discover via config)
  - [x] Load agent file content: `tokio::fs::read_to_string(agent_path).await?`
  - [x] Append language override: `format!("{agent_content}\n\nOVERRIDE: communication_language = English")`
  - [x] Create LLM provider via `create_completion_model(&config.llm.dev, &secrets)?`
  - [x] Create tools: `GitTool::new(project_root)`, `FsTool::new(project_root)`, `TerminalTool::new(project_root, 30)`
  - [x] Create supervisor: `AskSupervisor::with_architect_from_config(&config, escalation_slot, decision_log)?`
  - [x] Build agent: `provider.agent(model).preamble(&preamble).tool(git).tool(fs).tool(terminal).tool(supervisor).build()`
  - [x] Log agent creation: `tracing::info!(action = "agent_built", tools = 4, model = %model, "Rig agent built")`

- [x] **4.4** Implement `async fn chat_loop(...) -> Result<ChatLoopResult, SessionError>`
  - [x] Send initial message `"DS"` via `agent.chat("DS", history)`
  - [x] Record in WAL: `state.add_user_message("DS")`, then `state.add_assistant_message(&response)`, then `state.save()`
  - [x] Enter loop:
    1. Analyze response via `self.analyzer.analyze(&response, &escalation_slot, &story.story_key)`
    2. Match on `ResponseAction`:
       - `Completed` → break loop, return success
       - `Escalated` → extract EscalationInfo from slot, call `preserve_partial_work()`, call `mark_story_needs_clarification()`, build `EscalationReport`, break loop, return escalated
       - `Continue { reply }` → record reply in WAL, send via `agent.chat(&reply, history)`, record response in WAL, save WAL, continue loop
       - `NoReply` → not expected in current chat-based flow (reserved for future streaming support), treat as `Continue { reply: "Continue.".to_string() }`
    3. On any `agent.chat()` error:
       - Check if it's a context limit error → initiate context recovery (future Story 6.4 — for now, return `Failed`)
       - Check if it's a transient error → retry up to 3 times with exponential backoff
       - Otherwise → call `preserve_partial_work()`, return `Failed`
  - [x] Maximum turn limit: define `const MAX_CHAT_TURNS: usize = 200` at module top (safety net to prevent infinite loops). If exceeded, return `Failed` with "Maximum turn limit exceeded". Note: a future improvement could make this configurable via `BotConfig` — for now a const is sufficient.
  - [x] Log each turn: `tracing::debug!(action = "chat_turn", turn = %n, response_len = %len, "Chat turn completed")`

- [x] **4.5** Write unit tests
  - [x] `test_session_runner_new_sets_state_file_path`
  - [x] `test_state_file_path_derived_from_config`
  - [x] Note: Full session tests require LLM mocking — see Task 6 for integration approach

### Task 5: Update Session Module (`src/session/mod.rs`)

- [x] **5.1** Add new submodules
  - [x] `pub mod analyzer;`
  - [x] `pub mod provider;`
  - [x] `pub mod runner;`
  - [x] Update module-level doc comment to reflect new capabilities

- [x] **5.2** Re-export key types
  - [x] `pub use runner::SessionRunner;`
  - [x] `pub use analyzer::ResponseAnalyzer;`
  - [x] `pub use provider::create_completion_model;`
  - [x] `pub use state::SessionState;`

- [x] **5.3** Ensure `SessionOutcome` and `SessionError` remain unchanged — Story 3.4 already defined them correctly

### Task 6: Integration Verification

- [x] **6.1** Run `cargo check` — zero errors
- [x] **6.2** Run `cargo test` — all new tests pass, all existing ~372 tests still pass (zero regressions)
- [x] **6.3** Run `cargo clippy` — zero new warnings
- [x] **6.4** Run `cargo fmt` — all code formatted
- [x] **6.5** Verify the session runner can be instantiated with a test `BotConfig` (use `make_test_bot_config()` from watcher tests)

## Dev Notes

### Previous Story Intelligence & Established Patterns

**Story 4.1** (Rig Tools — immediate predecessor) established:
- `GitTool::new(repo_path: PathBuf)`, `FsTool::new(project_root: PathBuf)`, `TerminalTool::new(working_dir: PathBuf, timeout_secs: u64)` — constructor signatures for tool registration
- Tools re-exported from `tools` module: `use crate::tools::{GitTool, FsTool, TerminalTool};`
- All tools implement rig `Tool` trait, are `Send + Sync`, `Serialize + Deserialize`

**Stories 3.1–3.4** (Epic 3 — Supervisor) established:
- `AskSupervisor::with_all(provider, escalation_slot, decision_log)` — full production constructor
- `AskSupervisor::with_architect_from_config(config, escalation_slot, decision_log)` — builds from BotConfig
- `EscalationSlot = Arc<Mutex<Option<EscalationInfo>>>` — check after each chat turn
- `DecisionLog` — clone for shared access, `records()` to extract at session end, `write_decisions_file()` at session end
- `preserve_partial_work(repo_path, branch)` in `src/session/cleanup.rs` — stages all files, creates WIP commit
- `mark_story_needs_clarification(sprint_status_path, story_key)` in `src/session/cleanup.rs` — updates sprint-status YAML

**Stories 1.1–1.4** (Config & CLI):
- `BotConfig` loaded from `bmad-bot.yaml`, shared as `Arc<BotConfig>`
- `BotSecrets` loaded from `.env` via dotenvy
- `BmadPathsConfig` has `project_root`, `implementation_artifacts` fields (String paths)
- `LlmRoleConfig` has `provider` and `model` fields
- Test helper: `watcher::tests::make_test_bot_config()` creates a minimal valid BotConfig

**Stories 2.1–2.3** (Watcher):
- `StoryInfo` fully defined with `story_id`, `story_key`, `branch_name`, `specs_path`, `epic_num`, `story_num`, `label`, `dependencies`, `status`
- `Watcher::poll()` returns `Vec<StoryInfo>` of eligible stories

### Core Design — Session Runner Architecture

The session runner is the daemon's execution engine for a single story. It follows architecture Decision 1 (Hybrid Chat Loop + Supervisor Tool) and Decision 5 (Load BMAD Agent File Directly).

```
┌─────────────────────────────────────────────────────────────────┐
│  SessionRunner::run(story)                                       │
│                                                                  │
│  1. Build agent                                                  │
│     ├── Load BMAD dev agent file → preamble                     │
│     ├── Append "OVERRIDE: communication_language = English"     │
│     ├── Create LLM provider from config (anthropic/openai/etc)  │
│     ├── Create 4 tools (git, fs, terminal, ask_supervisor)      │
│     └── provider.agent(model).preamble().tool()×4.build()       │
│                                                                  │
│  2. Create WAL file (SessionState)                               │
│                                                                  │
│  3. Chat loop                                                    │
│     ├── Send "DS" → agent starts dev-story workflow             │
│     └── Loop:                                                    │
│         ├── agent.chat(message, history) → response             │
│         ├── WAL save (after each turn)                          │
│         ├── ResponseAnalyzer::analyze(response, slot)           │
│         ├── Completed → break, return SessionOutcome::Completed │
│         ├── Escalated → cleanup, return SessionOutcome::Escalated│
│         └── Continue{reply} → send reply, next turn             │
│                                                                  │
│  4. Cleanup                                                      │
│     ├── Success → delete WAL, collect decisions                 │
│     ├── Escalated → preserve partial work, mark needs-clarify   │
│     └── Failed → preserve partial work, log error               │
└─────────────────────────────────────────────────────────────────┘
```

**Key principle:** The daemon knows NOTHING about BMAD workflow internals. It loads the agent file, registers tools, sends "DS", and auto-responds to confirmations. The agent handles everything else via its tools.

### rig-core Chat API (v0.30)

The chat loop uses rig's `Chat` trait:

```rust
use rig::message::Message;
use rig::completion::Chat;

// Build history from WAL state
let history: Vec<Message> = state.to_rig_messages();

// Send message with history — rig handles tool calls internally
let response: String = agent.chat("DS", history).await?;

// For subsequent turns
let response = agent.chat(&reply, updated_history).await?;
```

**Critical rig behavior:** When the agent calls tools (git, fs, terminal, ask_supervisor), rig handles the entire tool-calling loop internally within a single `agent.chat()` call. The daemon only sees the final text response after all tool calls complete. This is why the supervisor is a tool (internal interception) and the chat loop handles workflow interaction (external interception).

**Message types:** `Message::user(content)` and `Message::assistant(content)` are the two constructors for building history. The rig Chat trait expects `Vec<Message>` as the history parameter.

### WAL File Strategy

**Location:** `{implementation_artifacts}/.bmad-bot-session.yaml` — dot-prefixed for convention (transient file).

**Atomic write pattern:** Write to `{path}.tmp` then `tokio::fs::rename()` to final path. This prevents corruption if the daemon crashes mid-write.

```rust
pub async fn save(&self, path: &Path) -> Result<(), StateError> {
    let yaml = serde_yml::to_string(self).map_err(|e| StateError::WriteFailed {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let tmp_path = path.with_extension("yaml.tmp");
    tokio::fs::write(&tmp_path, &yaml).await.map_err(|e| StateError::WriteFailed {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    tokio::fs::rename(&tmp_path, path).await.map_err(|e| StateError::WriteFailed {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    Ok(())
}
```

**Crash recovery** (daemon restart with existing WAL):
- `SessionState::exists(path)` checks for WAL at startup
- If found → `SessionState::load(path)` recovers full chat history
- Rebuild agent with same provider/model from WAL metadata
- Resume `agent.chat()` with recovered history
- If WAL is corrupted → delete it, treat as clean start, log warning

### Response Analyzer — Pattern Matching Strategy

The analyzer uses simple substring matching. This is deliberately simple — the rule engine in the supervisor handles complex pattern matching. The analyzer only needs to handle workflow-level interactions.

**Method signature:** `pub fn analyze(&self, response: &str, escalation_slot: &EscalationSlot, story_key: &str) -> ResponseAction`

The `story_key` parameter is required so that Priority 6 (story selection) can reply with the actual story key when the agent asks which story to work on. The caller (`SessionRunner::chat_loop`) passes `&story.story_key` from the `StoryInfo` received at session start.

**Pattern priority order prevents conflicts:**
1. Escalation (slot check) — highest priority, always checked first
2. Completion signals — "all tasks completed", "story implementation complete", etc.
3. Confirmation requests — "should I proceed", "continue?", etc.
4. Step-by-step — "step by step", "one at a time", etc.
5. YOLO/batch mode — "yolo", "batch mode", etc.
6. Story selection — "which story", "story to work on", etc. → replies with `story_key` value
7. Default — "Continue."

**Completion signals must be specific** to avoid false positives. The agent might say "I'll complete the task" (not a completion signal) vs "All tasks completed successfully" (real completion). Use multi-word phrases, not single words.

**Completion detection phrases (case-insensitive):**
- "all tasks completed"
- "story implementation complete"
- "dev-story workflow complete"
- "story marked as done"
- "implementation is complete"
- "all acceptance criteria met"
- "story is ready for review"

### LLM Provider Factory — Multi-Provider Support

The daemon supports three LLM providers per architecture:

| Provider | rig Module | API Key Env Var | Notes |
|----------|-----------|----------------|-------|
| `anthropic` | `rig::providers::anthropic` | `ANTHROPIC_API_KEY` | Native rig support |
| `openai` | `rig::providers::openai` | `OPENAI_API_KEY` | Native rig support |
| `github-models` | `rig::providers::openai` (base URL override) | `GITHUB_MODELS_API_KEY` | OpenAI-compatible, Azure endpoint |

**GitHub Models uses OpenAI-compatible API** — the rig openai client with a custom base URL pointing to `https://models.inference.ai.azure.com`. This is a thin adapter, not a separate provider.

**Note on rig's agent builder type:** The `.agent()` method returns an `AgentBuilder` that is generic over the model type. Since we need to support multiple providers dynamically, we need to use a trait object or enum dispatch. Check rig's `CompletionModel` trait — if it's object-safe, use `Box<dyn CompletionModel>`. Otherwise, use an enum wrapper. The exact approach depends on rig v0.30's API surface — the dev should check and adapt.

### Escalation Flow Within Chat Loop

When the supervisor tool escalates (returns `EscalationRequired` error), rig stops the tool-calling loop and returns control to the daemon's chat loop. The escalation flow:

```
agent.chat(msg, history)
  └── agent internally calls ask_supervisor tool
       └── rule engine miss → architect fail → EscalationRequired
            └── escalation_slot.lock() → write Some(EscalationInfo)
            └── return Err(SupervisorError::EscalationRequired)
       └── rig catches tool error → returns error text to daemon
  └── daemon receives response (may contain error text)

ResponseAnalyzer checks escalation_slot → Some(EscalationInfo) → Escalated

Chat loop handles escalation:
  1. Extract EscalationInfo from slot
  2. preserve_partial_work(repo_path, branch)
  3. mark_story_needs_clarification(sprint_status_path, story_key)
  4. Build EscalationReport
  5. Collect decisions from DecisionLog
  6. Return SessionOutcome::Escalated { report, decisions }
```

### Session End — Decision File & Cleanup

At session end (any outcome), the decisions file must be written:

```rust
use crate::supervisor::decisions::write_decisions_file;

// Collect decisions from the shared log
let decisions: Vec<DecisionRecord> = decision_log.records();

// Write decisions file if any decisions were made
if !decisions.is_empty() {
    let decisions_path = Path::new(&config.bmad_paths.implementation_artifacts)
        .join(format!("{}-DECISIONS.md", story.story_key));
    // Note: signature is write_decisions_file(decisions, output_path, story_key)
    write_decisions_file(&decisions, &decisions_path, &story.story_key).await.ok();
    // Best-effort — don't fail the session for a logging issue
}
```

### Integration with Future Stories

**Story 4.3** (Pre-Development Preparation & Branch Management) will:
- Be handled by the BMAD agent itself via its dev-story workflow — the daemon does NOT implement pre-dev preparation
- The agent uses git tool to create branches, filesystem tool to read prior stories
- This story's session runner launches the agent; the agent does the rest

**Epic 5** (Code Review & PR) will:
- Receive `SessionOutcome` from the runner
- On `Completed` → launch review session, then create PR
- On `Escalated` → create PR with partial work + escalation info
- On `Failed` → create PR with partial work + error description

**Epic 6** (Notifications & Resilience) will:
- Story 6.3: Implement full crash recovery using the WAL file created here
- Story 6.4: Implement context window limit recovery (currently returns `Failed`)
- This story creates the WAL infrastructure that 6.3 and 6.4 will use

### Files Created/Modified in This Story

| File | Change |
|------|--------|
| `src/session/state.rs` | **MODIFY** — Replace stub with full `SessionState`, `ChatMessage`, `StateError`, WAL read/write/delete |
| `src/session/analyzer.rs` | **CREATE** — `ResponseAnalyzer`, `ResponseAction`, pattern matching for workflow interactions |
| `src/session/provider.rs` | **CREATE** — `create_completion_model()` factory, `ProviderError`, multi-provider support |
| `src/session/runner.rs` | **CREATE** — `SessionRunner`, `run()`, `build_agent()`, `chat_loop()`, full session lifecycle |
| `src/session/mod.rs` | **MODIFY** — Add `pub mod analyzer`, `pub mod provider`, `pub mod runner`, re-exports |

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code — only in tests
- ❌ **NO** `anyhow::Result` in session module — typed `thiserror` enums only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** knowing BMAD workflow internals — the daemon loads the agent file and sends "DS", nothing more
- ❌ **NO** modifying `sprint-status.yaml` from the session runner — the BMAD agent handles all status mutations (Decision 2)
- ❌ **NO** modifying any file under `_bmad/` — the daemon is a read-only consumer of BMAD config (Critical Rule)
- ❌ **NO** storing API keys in config structs — keys come from `BotSecrets` (loaded from `.env`)
- ❌ **NO** parallel sessions — one session at a time, sequential execution only
- ❌ **NO** implementing pre-dev preparation or branch creation — that's the agent's job via tools (Story 4.3)
- ❌ **NO** implementing code review or PR creation — that's Epic 5
- ❌ **NO** implementing crash recovery logic — that's Story 6.3 (but create the WAL infrastructure here)
- ❌ **NO** implementing context limit recovery — that's Story 6.4 (return `Failed` for now)
- ❌ **NO** modifying supervisor, tools, watcher, or cleanup modules — they are already complete
- ❌ **NO** calling real LLM APIs in unit tests — mock all external dependencies
- ❌ **NO** infinite chat loops — enforce 200-turn maximum safety limit

### Scope Boundaries

**IN SCOPE for this story:**
- `src/session/state.rs` — Full WAL state file implementation
- `src/session/analyzer.rs` — Response analysis and action dispatch
- `src/session/provider.rs` — LLM provider factory for multi-provider support
- `src/session/runner.rs` — Session lifecycle management (build agent, chat loop, cleanup)
- `src/session/mod.rs` — Module wiring and re-exports

**OUT OF SCOPE — do NOT implement:**
- Pre-development preparation or branch management (Story 4.3 — handled by agent)
- Code review session (Epic 5)
- PR creation (Epic 5)
- Notifications (Epic 6)
- Full crash recovery from WAL (Story 6.3 — only create WAL infrastructure)
- Context window limit recovery (Story 6.4 — return Failed for now)
- Graceful SIGTERM handling during session (handled by cli/main, not session)
- Any modifications to supervisor, tools, watcher, or cleanup modules

### Testing Requirements

All tests follow established patterns: `test_{module}_{behavior}_{scenario}`, Arrange → Act → Assert, `tempfile::TempDir` for fixtures.

**Test coverage targets:**
- **state.rs**: ~12 tests — SessionState CRUD, WAL save/load/delete roundtrip, YAML serialization, error paths
- **analyzer.rs**: ~11 tests — all ResponseAction variants, pattern priority, case insensitivity, escalation slot check, story_key reply
- **provider.rs**: ~3 tests — error types, unsupported provider, display messages (real provider creation requires API keys)
- **runner.rs**: ~2 tests — construction, state file path derivation (full session tests need LLM mocks → E2E only)
- **Total**: ~28 new tests, 0 regressions on existing ~372 tests

**E2E testing note:** Full session integration tests (real LLM, real tools, real chat loop) are gated behind `BMAD_E2E=1` environment variable and placed in `tests/e2e/`. They are expensive (token cost) and manual-launch only. This story may add a placeholder E2E test structure but does not require passing E2E tests.

### Dev Dependencies Required

No new crate dependencies needed. All required crates are present:
- `rig-core = "0.30"` — Agent, Chat, Message, CompletionModel, Tool, ToolDyn, providers
- `serde_yml = "0.0.12"` — WAL file serialization (project uses serde_yml, NOT serde_yaml)
- `tokio` with `full` features — async fs, process, spawn, time
- `chrono = "0.4"` — ISO 8601 timestamps for WAL
- `serde` + `serde_json` — serialization
- `thiserror = "2"` — error enums
- `tracing = "0.1"` — structured logging
- `regex = "1"` — optional, for response analysis patterns (simple `.contains()` may suffice)
- `tempfile = "3"` (dev-dependency) — test fixtures

### Project Structure Notes

After this story, the session module is the execution engine:

```
src/session/
├── mod.rs          # Module declarations, SessionError, SessionOutcome, re-exports
├── state.rs        # SessionState WAL — create/save/load/delete, ChatMessage
├── analyzer.rs     # ResponseAnalyzer — pattern match agent responses to actions
├── provider.rs     # LLM provider factory — anthropic/openai/github-models
├── runner.rs       # SessionRunner — build agent, chat loop, lifecycle management
├── cleanup.rs      # preserve_partial_work(), mark_story_needs_clarification() (unchanged)
└── escalation.rs   # EscalationInfo, EscalationReport (unchanged)
```

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 4.2: Agent Session Setup & Chat Loop] — Acceptance criteria and user story
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 4: Autonomous Development Session] — "launches rig agent session with BMAD dev persona and registered tools"
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 1: Supervisor Interception Model] — Hybrid chat loop + supervisor tool, `agent.chat(message, history)` pattern
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 3: Session State Persistence] — WAL file location, contents, lifecycle, crash recovery, context limit recovery
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 5: Agent Prompt Composition] — Load BMAD agent file directly, append language override, register 4 tools, send "DS"
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 2: Sprint-Status Mutation] — Daemon reads only, agent writes — session runner NEVER modifies sprint-status
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 4: Error Propagation] — Layer 3: session errors → commit partial work, create PR, notify, next story
- [Source: _bmad-output/planning-artifacts/architecture.md#Data Flow] — Steps 4-6: session init, chat loop, supervisor calls
- [Source: _bmad-output/planning-artifacts/architecture.md#Architectural Boundaries] — session → tools via .tool(), session → supervisor as rig tool, session → review passes ReviewContext
- [Source: _bmad-output/planning-artifacts/prd.md#Functional Requirements] — FR8-FR11: agent session, tool exposure, autonomous execution, language override
- [Source: _bmad-output/project-context.md#Framework-Specific Rules] — Daemon lifecycle steps 3-5: session setup, chat loop, supervisor
- [Source: _bmad-output/project-context.md#Session Language Override] — Inject English override in preamble, never modify BMAD config files
- [Source: _bmad-output/project-context.md#Sequential Execution] — One story at a time, no parallelism
- [Source: _bmad-output/project-context.md#Pre-Development Spec Update] — Agent reviews prior stories (handled by agent, not daemon)
- [Source: _bmad-output/project-context.md#Multi-Provider LLM Config] — Three independent LLM roles: dev, review, supervisor
- [Source: _bmad-output/project-context.md#Supervisor Hybrid Pattern] — Rule engine → LLM fallback → escalation
- [Source: _bmad-output/implementation-artifacts/4-1-rig-tools-implementation-git-filesystem-terminal.md#Integration with Future Stories] — Tool constructors and registration pattern
- [Source: _bmad-output/implementation-artifacts/3-4-decision-logging-traceability.md#Integration with Future Stories] — "Epic 4 will: Create DecisionLog::new(), pass to AskSupervisor, call write_decisions_file() at session end"
- [Source: src/session/mod.rs] — SessionError, SessionOutcome already defined
- [Source: src/session/cleanup.rs] — preserve_partial_work(), mark_story_needs_clarification() already implemented
- [Source: src/session/escalation.rs] — EscalationInfo, EscalationReport already defined
- [Source: src/supervisor/mod.rs] — AskSupervisor constructors, escalation_slot(), decision_log() accessors
- [Source: src/config/mod.rs] — BotConfig, LlmConfig, LlmRoleConfig, BotSecrets, BmadPathsConfig
- [Source: src/watcher/mod.rs] — StoryInfo struct with all fields, make_test_bot_config() test helper

## Dev Agent Record

### Agent Model Used

Claude Opus 4 (claude-opus-4-20250514)

### Debug Log References

- `cargo check`: zero errors (warnings are dead_code from unreferenced modules — expected until main.rs wires session runner)
- `cargo test`: 421 passed, 0 failed, 0 ignored (49 new tests added, 372 existing tests unchanged)
- `cargo clippy`: zero new warnings (renamed `StateError` variants from `WriteFailed`/`ReadFailed`/etc to `Write`/`Read`/etc per clippy `enum_variant_names` lint)
- `cargo fmt --check`: all formatted

### Completion Notes List

- **Task 0**: All prerequisites verified — tools module, session module stubs, supervisor module, config module, watcher module all present and functional. `cargo check` clean baseline.
- **Task 1**: `src/session/state.rs` — Full WAL implementation with `SessionState`, `ChatMessage`, `StateError`. Atomic write via `.tmp` then rename. 12 unit tests all pass. Note: `StateError` variant names use `Write`/`Read`/`Parse`/`Delete` (not `WriteFailed` etc.) per clippy `enum_variant_names` lint.
- **Task 2**: `src/session/analyzer.rs` — `ResponseAnalyzer` with `ResponseAction` enum (Continue/Completed/Escalated/NoReply). 7-priority pattern matching with case-insensitive substring search. 14 unit tests all pass (including false positive prevention test).
- **Task 3**: `src/session/provider.rs` — `ProviderError` enum + `resolve_api_key()` + `create_completion_model()`. **Key adaptation**: rig's `Chat` trait is NOT object-safe (confirmed by `supervisor/architect.rs` pattern), so `create_completion_model` returns the resolved API key string rather than `Box<dyn CompletionModel>`. Agent construction uses per-provider match arms in the runner (established pattern). 13 unit tests all pass.
- **Task 4**: `src/session/runner.rs` — `SessionRunner` with full lifecycle: `run()` → `build_anthropic_agent()`/`build_openai_agent()` → `run_session()` (generic over `A: Chat`). Chat loop sends "DS", analyzes responses, handles completion/escalation/failure with WAL persistence. `MAX_CHAT_TURNS = 200` safety net. 3-retry on transient errors. `preserve_partial_work()` on failure. `write_decisions_file()` at session end. 2 unit tests for constructor/path derivation (full session tests need LLM mocking → E2E only).
- **Task 5**: `src/session/mod.rs` — Added `pub mod analyzer`, `pub mod provider`, `pub mod runner`. Re-exports: `SessionRunner`, `ResponseAnalyzer`, `create_completion_model`, `SessionState`. Doc comments updated. `SessionError` and `SessionOutcome` unchanged.
- **Task 6**: All integration checks pass — `cargo check` (0 errors), `cargo test` (421 pass), `cargo clippy` (0 new warnings), `cargo fmt` (formatted). Runner instantiation verified via `test_session_runner_new_sets_state_file_path`.

### Change Log

- 2026-02-08: Story 4.2 implemented — session state WAL, response analyzer, LLM provider factory, session runner with chat loop. 49 new tests, 0 regressions on 372 existing tests. Status → review.

### File List

- `src/session/state.rs` — **MODIFIED** — Replaced stub with full `SessionState`, `ChatMessage`, `StateError`, WAL CRUD + atomic write, 12 tests
- `src/session/analyzer.rs` — **CREATED** — `ResponseAnalyzer`, `ResponseAction`, 7-priority pattern matching, 14 tests
- `src/session/provider.rs` — **CREATED** — `ProviderError`, `resolve_api_key()`, `create_completion_model()`, 13 tests
- `src/session/runner.rs` — **CREATED** — `SessionRunner`, `run()`, `build_anthropic_agent()`, `build_openai_agent()`, `run_session()`, chat loop, escalation/failure handling, 2 tests
- `src/session/mod.rs` — **MODIFIED** — Added `pub mod analyzer/provider/runner`, re-exports for `SessionRunner`, `ResponseAnalyzer`, `create_completion_model`, `SessionState`
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — **MODIFIED** — `4-2-agent-session-setup-chat-loop: ready-for-dev → review`