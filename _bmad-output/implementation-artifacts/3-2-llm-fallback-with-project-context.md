# Story 3.2: LLM Fallback with Project Context

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want substantive agent questions to be answered by a dedicated BMAD Architect agent session with full project context,
So that the developer agent gets expert, context-aware architectural answers when the rule engine cannot help.

## Acceptance Criteria

1. **Given** the rule engine returns `NoMatch` for a question **When** the supervisor LLM fallback is triggered **Then** a fresh BMAD Architect agent session is created using the supervisor provider/model configured in `bmad-bot.yaml` **And** the Architect agent file (`_bmad/bmm/agents/architect.md`) is loaded as the full preamble **And** a minimal `ReadFile` rig tool is registered so the Architect can load project files autonomously

2. **Given** a fresh Architect session is created **When** the supervisor drives the multi-turn conversation **Then** the following messages are sent in sequence: (1) "CH" to enter free chat mode, (2) "Load the project context" so the Architect loads relevant project docs via the ReadFile tool, (3) "A developer agent working on this project has the following question: {question}" with optional context **And** the Architect's final response is returned to the dev agent as the tool output

3. **Given** the Architect session completes or fails **When** the response is returned or an error occurs **Then** the session is discarded (no persistence between supervisor calls) **And** each supervisor invocation creates an entirely fresh Architect session

4. **Given** the LLM provider is unavailable or the Architect session fails **When** the supervisor attempts the fallback **Then** the error is caught and the supervisor proceeds to human escalation (returns `SupervisorError::EscalationRequired`) **And** the failure is logged via `tracing::error!()` with `action = "supervisor_fallback_failed"`

5. **Given** the supervisor LLM fallback is invoked **When** any step of the Architect session occurs **Then** all interactions are logged via `tracing::warn!()` with `action = "supervisor_fallback"` including the original question and turn count

## Tasks / Subtasks

- [ ] Task 0: Verify prerequisites from Story 3.1 and Epic 1 (AC: #1–#5)
  - [ ] 0.1 Verify `src/supervisor/mod.rs` contains full `AskSupervisor` tool implementation with `SupervisorError`, `AskSupervisorArgs`, Tool trait impl
  - [ ] 0.2 Verify `AskSupervisor::call()` currently returns `Err(SupervisorError::LlmFallbackNotImplemented)` on `NoMatch`
  - [ ] 0.3 Verify `BotConfig` (from Story 1.1) has `llm: LlmConfig` with `supervisor: LlmRoleConfig { provider, model, api_key_env }` fields
  - [ ] 0.4 Verify `BotConfig` has path fields: `bmad_agent_path` (or resolvable path to `_bmad/bmm/agents/`), `project_root`
  - [ ] 0.5 Run `cargo check` to confirm clean baseline

- [ ] Task 1: Create minimal `ReadFile` rig tool in `src/supervisor/read_tool.rs` (AC: #1)
  - [ ] 1.1 Create new file `src/supervisor/read_tool.rs`
  - [ ] 1.2 Add `pub mod read_tool;` to `src/supervisor/mod.rs`
  - [ ] 1.3 Define `#[derive(Debug, Serialize, Deserialize)] pub struct ReadFile` — empty struct (no state needed)
  - [ ] 1.4 Define `#[derive(Debug, Deserialize)] pub struct ReadFileArgs { pub path: String }`
  - [ ] 1.5 Define `#[derive(Debug, thiserror::Error)] pub enum ReadFileError` with variants: `NotFound { path: String }`, `ReadFailed { path: String, reason: String }`, `PathDenied { path: String, reason: String }`
  - [ ] 1.6 Implement `Tool for ReadFile` with `NAME = "read_file"`, `Output = String`:
    - Read the file at the given path via `tokio::fs::read_to_string()`
    - Return the file content as a string
    - Return `ReadFileError::NotFound` if the file does not exist
    - Return `ReadFileError::ReadFailed` on I/O errors
  - [ ] 1.7 **Security:** Implement a path allowlist or project-root boundary check — the ReadFile tool must ONLY allow reading files under `{project_root}` (prevent directory traversal or reading system files). Return `ReadFileError::PathDenied` for paths outside the boundary
  - [ ] 1.8 The tool definition description must explain to the LLM: "Read a file from the project. Provide the relative path from the project root."
  - [ ] 1.9 Log every read via `tracing::debug!(action = "supervisor_read_file", path = %args.path, "Architect reading file")`
  - [ ] 1.10 Add `/// doc comments` on all public items
  - [ ] 1.11 **Note:** This is a minimal read-only tool for the supervisor Architect session. The full filesystem tool (read + write) is in Epic 4, Story 4.1. This tool is intentionally separate and limited.

- [ ] Task 2: Create `ArchitectSession` in `src/supervisor/architect.rs` (AC: #1, #2, #3, #4, #5)
  - [ ] 2.1 Create new file `src/supervisor/architect.rs`
  - [ ] 2.2 Add `pub mod architect;` to `src/supervisor/mod.rs`
  - [ ] 2.3 Define `#[derive(Debug, thiserror::Error)] pub enum ArchitectSessionError` with variants:
    - `AgentFileNotFound { path: String }` — architect.md not found
    - `AgentFileReadFailed { path: String, reason: String }` — I/O error reading architect.md
    - `ProviderInit { reason: String }` — failed to create rig provider client
    - `ApiKeyMissing { env_var: String }` — API key env var not set
    - `UnsupportedProvider { provider: String }` — provider string not recognized
    - `ChatFailed { turn: u32, reason: String }` — a chat turn failed
    - `NoResponse` — architect returned empty response
  - [ ] 2.4 Define `pub struct ArchitectSession` — holds the configuration needed to create sessions on demand:
    - `agent_file_content: String` — pre-loaded content of `architect.md`
    - `provider: String` — provider name from config
    - `model: String` — model name from config
    - `api_key: String` — resolved API key value (read from env at construction)
    - `project_root: PathBuf` — for the ReadFile tool boundary
  - [ ] 2.5 Implement `ArchitectSession::new(config: &BotConfig) -> Result<Self, ArchitectSessionError>`:
    - Resolve the path to `architect.md` from `{project_root}/_bmad/bmm/agents/architect.md`
    - Read the full file content
    - Read the supervisor LLM config from `config.llm.supervisor` (provider, model, api_key_env)
    - Read the API key from the environment variable named in `api_key_env`
    - Store everything for later session creation
    - **Do NOT create a rig agent yet** — that happens per question in `ask()`
  - [ ] 2.6 Implement `ArchitectSession::ask(&self, question: &str, context: Option<&str>) -> Result<String, ArchitectSessionError>`:
    - **Step 1:** Create the rig provider client (Anthropic, OpenAI, or GitHub Models) using stored config
    - **Step 2:** Build a rig agent with: preamble = `self.agent_file_content`, tool = `ReadFile` (with project_root boundary)
    - **Step 3:** Drive the multi-turn chat conversation:
      - Turn 1: Send `"CH"` → receive greeting/acknowledgment (discard response)
      - Turn 2: Send `"Load the project context"` → Architect uses ReadFile to load docs (discard response)
      - Turn 3: Send the question message (see 2.7 below) → **capture and return this response**
    - **Step 4:** Return the Architect's answer from Turn 3
    - On any turn failure, return `ArchitectSessionError::ChatFailed` with the turn number and error
  - [ ] 2.7 The question message format for Turn 3:
    - Without context: `"A developer agent working on this project has the following question: {question}"`
    - With context: `"A developer agent working on this project has the following question: {question}\n\nAdditional context from the developer: {context}"`
  - [ ] 2.8 Log each turn: `tracing::warn!(action = "supervisor_fallback", turn = turn_num, "Architect session turn")` and log the final answer: `tracing::info!(action = "supervisor_fallback_response", response_len = response.len(), "Architect answered")`
  - [ ] 2.9 **Provider selection logic:**
    - `"anthropic"` → `anthropic::Client::new(&self.api_key)` + `client.agent(model)`
    - `"openai"` → `openai::Client::new(&self.api_key)` + `client.agent(model)` (rig reads OPENAI_API_KEY but we use explicit key)
    - `"github-models"` → `openai::Client::new("https://models.inference.ai.azure.com", &self.api_key)` + `client.agent(model)` (OpenAI-compatible API)
    - Any other provider → return `ArchitectSessionError::UnsupportedProvider`
  - [ ] 2.10 **rig chat API usage:** Use `agent.chat(message, chat_history)` in a loop, accumulating `chat_history` across turns. Each call returns the agent response and updated history. The agent may invoke the ReadFile tool autonomously during any turn (rig handles tool calls internally within `chat()`).
  - [ ] 2.11 Add `/// doc comments` on all public items

- [ ] Task 3: Modify `AskSupervisor` struct to hold optional Architect session (AC: #1, #2, #3)
  - [ ] 3.1 Add field `#[serde(skip)] architect_session: Option<ArchitectSession>` to `AskSupervisor` — must be `#[serde(skip)]` because `ArchitectSession` holds an API key and is not serializable. `AskSupervisor` derives `Serialize + Deserialize` for the rig Tool trait. The field is `Option` to support construction without LLM (for testing and backward compatibility)
  - [ ] 3.2 Update `AskSupervisor::new()` to remain a simple constructor with just `RuleEngine` (no Architect session) — used in tests and when LLM is not configured
  - [ ] 3.3 Add `AskSupervisor::with_architect(session: ArchitectSession) -> Self` constructor that initializes with both rule engine and Architect session
  - [ ] 3.4 Add `AskSupervisor::with_architect_from_config(config: &BotConfig) -> Result<Self, ArchitectSessionError>` convenience constructor that reads config and builds the `ArchitectSession`
  - [ ] 3.5 `Default` impl remains unchanged (no Architect session, no LLM fallback)

- [ ] Task 4: Update `AskSupervisor::call()` with Architect session fallback logic (AC: #1, #2, #3, #4, #5)
  - [ ] 4.1 In the `RuleResult::NoMatch` branch, replace `Err(SupervisorError::LlmFallbackNotImplemented)` with:
    - Check if `self.architect_session` is `Some`
    - If `Some`: call `session.ask(&args.question, args.context.as_deref()).await`
    - On success: log `tracing::warn!(action = "supervisor_fallback", question = %args.question, "Architect session answered")` and return `Ok(response)`
    - On error: log `tracing::error!(action = "supervisor_fallback_failed", question = %args.question, error = %e, "Architect session failed — escalating")` and return `Err(SupervisorError::EscalationRequired { question, reason })`
    - If `None` (no Architect configured): return `Err(SupervisorError::LlmFallbackNotImplemented)` (preserves existing behavior for tests)
  - [ ] 4.2 Ensure the tracing log for rule engine miss still fires before attempting Architect fallback
  - [ ] 4.3 The full call() pipeline is now: rule engine → Architect session → escalation error

- [ ] Task 5: Add or update `SupervisorError` variants in `src/supervisor/mod.rs` (AC: #4)
  - [ ] 5.1 Optionally add `ArchitectSessionFailed { question: String, reason: String }` variant — or reuse `EscalationRequired` when the Architect session fails (choose the simpler approach)
  - [ ] 5.2 Keep the existing `LlmFallbackNotImplemented` variant for when no Architect session is configured
  - [ ] 5.3 Ensure `SupervisorError` still implements `std::error::Error + Send + Sync`

- [ ] Task 6: Write unit tests (AC: #1–#5)
  - [ ] 6.1 **ReadFile tool tests** in `src/supervisor/read_tool.rs`:
    - Test reading an existing file returns its content
    - Test reading a non-existent file returns `ReadFileError::NotFound`
    - Test reading a file outside project root returns `ReadFileError::PathDenied`
    - Test tool definition has correct name (`read_file`) and non-empty description
    - Use `tempfile::TempDir` for test fixtures
  - [ ] 6.2 **ArchitectSession construction tests** in `src/supervisor/architect.rs`:
    - Test `ArchitectSession::new()` with missing architect.md returns `AgentFileNotFound`
    - Test `ArchitectSession::new()` with missing API key env var returns `ApiKeyMissing`
    - Test `ArchitectSession::new()` with unsupported provider returns `UnsupportedProvider`
    - Test `ArchitectSessionError` variants display correctly and implement `Send + Sync`
  - [ ] 6.3 **AskSupervisor integration tests** in `src/supervisor/mod.rs`:
    - Test `AskSupervisor::new()` (no Architect) still returns `LlmFallbackNotImplemented` on `NoMatch` (backward compat)
    - Test `AskSupervisor::call()` with matching rule still returns rule engine answer (Architect not invoked)
    - Test `AskSupervisor` serialization/deserialization still works (`architect_session` skipped via `serde(skip)`)
    - All existing Story 3.1 tests must still pass (no regressions)
  - [ ] 6.4 **Mock-based Architect test strategy:**
    - The `ArchitectSession::ask()` method calls real LLM APIs and cannot be unit-tested without mocking
    - **Recommended approach:** Extract an `AnswerProvider` trait with `async fn ask(question, context) -> Result<String, _>` and have `ArchitectSession` implement it. In tests, create a `MockAnswerProvider` that returns deterministic responses. `AskSupervisor` holds `Option<Box<dyn AnswerProvider>>` instead of `Option<ArchitectSession>` directly
    - This enables testing the full `call()` pipeline (rule engine miss → fallback → answer returned) without real API calls
    - If the trait approach adds too much complexity, test the `ask()` integration path in E2E tests only (`tests/e2e/`, gated behind `BMAD_E2E=1`)
  - [ ] 6.5 Verify all existing Story 3.1 tests still pass (no regressions)

- [ ] Task 7: Final quality checks
  - [ ] 7.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [ ] 7.2 Run `cargo clippy` and fix any warnings
  - [ ] 7.3 Run `cargo test` and verify all tests pass (including Epic 1, Epic 2, and Story 3.1 tests)
  - [ ] 7.4 Verify all public items have `///` doc comments
  - [ ] 7.5 Verify `SupervisorError` still implements `std::error::Error + Send + Sync`
  - [ ] 7.6 Verify no `unwrap()` or `expect()` in production code
  - [ ] 7.7 Verify no `println!` or `eprintln!` — tracing only
  - [ ] 7.8 Verify no API keys or secrets are logged by any tracing statement

## Dev Notes

### Previous Story Intelligence

**Story 3.1** established the complete supervisor tool skeleton:
- `AskSupervisor` struct with `rule_engine: RuleEngine` field, derives `Serialize + Deserialize`
- `AskSupervisorArgs` with `question: String` and `context: Option<String>`
- `SupervisorError` thiserror enum with `RuleEngineError`, `EscalationRequired`, `LlmFallbackNotImplemented`
- Full `Tool` trait impl: `NAME = "ask_supervisor"`, `Error = SupervisorError`, `Args = AskSupervisorArgs`, `Output = String`
- `call()` pipeline: rule engine match → return answer, no match → `Err(LlmFallbackNotImplemented)`
- `RuleEngine` with 6 built-in rule categories (confirmations, permissions, step-by-step, story selection, progress, stuck)
- `DecisionRecord` and `DecisionSource` stubs in `decisions.rs`
- Comprehensive unit tests for all rule patterns and tool behavior

**Story 3.1 forward-compatibility notes for THIS story:**
- Replace `Err(SupervisorError::LlmFallbackNotImplemented)` with actual LLM fallback call
- Add LLM client field to `AskSupervisor` struct
- **⚠️ Serde note:** The LLM client is NOT serializable. The new field must be marked `#[serde(skip)]` since `AskSupervisor` derives `Serialize + Deserialize` for the rig Tool trait

**Story 1.1** established:
- `BotConfig` with `llm: LlmConfig` containing `supervisor: LlmRoleConfig { provider, model, api_key_env }` — the supervisor's LLM provider/model config used in this story
- `Arc<BotConfig>` sharing pattern across modules
- Config paths: `project_root` for resolving file locations, plus BMAD paths

**Stories 1.2–2.3** established:
- Per-module thiserror enum pattern — apply same to `ArchitectSessionError` and `ReadFileError`
- Tracing structured fields with `action` field pattern
- Test patterns: `make_test_*` helpers, inline `#[cfg(test)] mod tests`

### Core Design — Simulated Human Interaction with BMAD Architect

The supervisor LLM fallback is NOT a generic "LLM + docs dump". It is a **full BMAD Architect agent session** where the daemon acts as a simulated human, driving the conversation exactly as a person would in an IDE.

**Why this approach:**
- BMAD agents are designed for expert interaction — the Architect (Winston) has a persona, principles, and deep architectural reasoning built in
- The BMAD activation flow (load config, set variables, load context) ensures the Architect operates with full project awareness
- "Treat it like a human" is the principle — the same pattern used for context window recovery (Architecture Decision 3)
- The Architect knows WHAT project files to load and HOW to interpret them — no need for the daemon to decide which docs are relevant

**The conversation flow:**

```
┌──────────────────────────────────────────────────────┐
│  Preamble: Full architect.md content                 │
│  (activation steps, persona, menu, rules — ALL of it)│
│  Tool registered: ReadFile (read-only, project root) │
├──────────────────────────────────────────────────────┤
│  Turn 1 (daemon → Architect):                        │
│    "CH"                                              │
│  Turn 1 (Architect → daemon):                        │
│    Greeting + enters free chat mode [DISCARD]        │
├──────────────────────────────────────────────────────┤
│  Turn 2 (daemon → Architect):                        │
│    "Load the project context"                        │
│  Turn 2 (Architect → daemon):                        │
│    [Architect calls ReadFile tool to load            │
│     config.yaml, architecture.md, prd.md,            │
│     project-context.md, etc.]                        │
│    Acknowledgment [DISCARD]                          │
├──────────────────────────────────────────────────────┤
│  Turn 3 (daemon → Architect):                        │
│    "A developer agent working on this project has    │
│     the following question: {question}"              │
│  Turn 3 (Architect → daemon):                        │
│    Expert answer ← THIS IS RETURNED                  │
└──────────────────────────────────────────────────────┘
```

**Token cost trade-off:** Each supervisor call involves ~5-6 LLM turns (activation + CH + load context + question). This uses more tokens than a one-shot prompt, but produces significantly better answers because the Architect has its full BMAD persona active and loads exactly the context it needs.

### Architecture Decision 3 Parallel — Context Window Recovery Pattern

This approach directly mirrors Architecture Decision 3's context limit recovery:

> "Do not re-enter the full dev-story workflow pipeline — instead, start a direct chat session (equivalent to CH mode)"

Both patterns:
1. Create a fresh rig agent with a BMAD agent persona as preamble
2. Enter CH (free chat) mode to bypass workflow menus
3. Provide context (injected or loaded by the agent)
4. Ask the question / resume work
5. Discard the session when done

The supervisor Architect session IS the same pattern, just applied to answering questions instead of resuming work.

### rig-core Chat API — Multi-Turn Conversation Pattern

The Architect session uses rig's `chat()` API for multi-turn conversation with tool use:

```rust
use rig::completion::{Chat, Message};

// Build agent with preamble + tools
let agent = provider_client
    .agent(model)
    .preamble(&architect_file_content)
    .tool(ReadFile::new(project_root))
    .build();

// Drive multi-turn conversation
let mut chat_history: Vec<Message> = vec![];

// Turn 1: Enter free chat
let response = agent.chat("CH", chat_history.clone()).await?;
chat_history.push(Message::user("CH"));
chat_history.push(Message::assistant(&response));

// Turn 2: Load project context
let response = agent.chat("Load the project context", chat_history.clone()).await?;
chat_history.push(Message::user("Load the project context"));
chat_history.push(Message::assistant(&response));

// Turn 3: Ask the question
let response = agent.chat(&question_message, chat_history.clone()).await?;
// response is the Architect's answer — return this
```

**Key notes:**
- `chat()` handles tool calls internally — if the Architect calls `ReadFile` during a turn, rig executes the tool and feeds the result back to the LLM automatically within that turn
- `chat_history` accumulates the full conversation for context continuity
- The agent has no memory between separate `ask()` calls — each call creates a fresh agent and history

### `ReadFile` Tool — Minimal Supervisor-Only Implementation

This is intentionally separate from Epic 4's full filesystem tool (`src/tools/fs.rs`) which will include read, write, and directory operations. The supervisor's `ReadFile`:

- **Read-only** — no write, no delete, no directory listing
- **Project-root bounded** — rejects paths outside `{project_root}` (security)
- **Located in supervisor module** — `src/supervisor/read_tool.rs`, not `src/tools/`
- **Simple implementation** — `tokio::fs::read_to_string()` with path validation

```rust
use rig::tool::Tool;
use rig::completion::ToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadFile {
    project_root: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct ReadFileArgs {
    pub path: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ReadFileError {
    #[error("File not found: {path}")]
    NotFound { path: String },
    #[error("Read failed for '{path}': {reason}")]
    ReadFailed { path: String, reason: String },
    #[error("Access denied for '{path}': {reason}")]
    PathDenied { path: String, reason: String },
}

impl ReadFile {
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    /// Validate the requested path is within the project root.
    fn validate_path(&self, requested: &str) -> Result<PathBuf, ReadFileError> {
        let full_path = self.project_root.join(requested);
        let canonical = full_path.canonicalize().map_err(|_| ReadFileError::NotFound {
            path: requested.to_string(),
        })?;
        if !canonical.starts_with(&self.project_root) {
            return Err(ReadFileError::PathDenied {
                path: requested.to_string(),
                reason: "Path is outside project root".to_string(),
            });
        }
        Ok(canonical)
    }
}

impl Tool for ReadFile {
    const NAME: &'static str = "read_file";
    type Error = ReadFileError;
    type Args = ReadFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file from the project. Provide the path relative \
                to the project root. Use this to load configuration files, \
                documentation, and source code as needed."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path from the project root to the file to read"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        tracing::debug!(
            action = "supervisor_read_file",
            path = %args.path,
            "Architect reading file"
        );

        let validated_path = self.validate_path(&args.path)?;

        tokio::fs::read_to_string(&validated_path)
            .await
            .map_err(|e| ReadFileError::ReadFailed {
                path: args.path,
                reason: e.to_string(),
            })
    }
}
```

### Provider Selection Logic

| Provider string | rig module | Client construction | Notes |
|----------------|-----------|-------------------|-------|
| `"anthropic"` | `rig::providers::anthropic` | `anthropic::Client::new(&api_key)` | Direct Anthropic API |
| `"openai"` | `rig::providers::openai` | `openai::Client::new(&api_key)` | Direct OpenAI API |
| `"github-models"` | `rig::providers::openai` | `openai::Client::new("https://models.inference.ai.azure.com", &api_key)` | OpenAI-compatible API at Azure inference endpoint |

### `AskSupervisor` Struct Changes — Serde Compatibility

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct AskSupervisor {
    rule_engine: RuleEngine,
    #[serde(skip)]
    architect_session: Option<ArchitectSession>,
}
```

- `#[serde(skip)]` excludes the field from serialization AND deserialization
- When deserialized, `architect_session` will be `None` (default for `Option`)
- Acceptable because `AskSupervisor` is always constructed via `::new()` or `::with_architect()` in production — never via deserialization
- The `Default` impl and `::new()` both set `architect_session: None`

### `call()` Pipeline — Updated Flow

```rust
async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
    tracing::info!(
        action = "ask_supervisor",
        question = %args.question,
        has_context = args.context.is_some(),
        "Supervisor tool invoked"
    );

    // Step 1: Try rule engine (deterministic, free, fast)
    let result = self.rule_engine.evaluate(&args.question);

    match result {
        RuleResult::Matched { ref rule_name, ref answer } => {
            tracing::info!(
                action = "rule_engine_match",
                rule = %rule_name,
                question = %args.question,
                "Rule engine matched — returning deterministic answer"
            );
            Ok(answer.clone())
        }
        RuleResult::NoMatch => {
            tracing::info!(
                action = "rule_engine_miss",
                question = %args.question,
                "Rule engine miss — no matching pattern found"
            );

            // Step 2: Try Architect session (BMAD agent, costs tokens)
            match &self.architect_session {
                Some(session) => {
                    tracing::warn!(
                        action = "supervisor_fallback",
                        question = %args.question,
                        "Launching Architect session for question"
                    );
                    match session.ask(&args.question, args.context.as_deref()).await {
                        Ok(response) => {
                            tracing::info!(
                                action = "supervisor_fallback_response",
                                question = %args.question,
                                response_len = response.len(),
                                "Architect session answered successfully"
                            );
                            Ok(response)
                        }
                        Err(e) => {
                            tracing::error!(
                                action = "supervisor_fallback_failed",
                                question = %args.question,
                                error = %e,
                                "Architect session failed — escalating"
                            );
                            // Step 3: Escalate to human (Story 3.3 refines)
                            Err(SupervisorError::EscalationRequired {
                                question: args.question,
                                reason: format!("Architect session failed: {e}"),
                            })
                        }
                    }
                }
                None => {
                    // No Architect session configured — old behavior
                    Err(SupervisorError::LlmFallbackNotImplemented)
                }
            }
        }
    }
}
```

### rig `Prompt` vs `Chat` Trait — Implementation Guidance

The Architect session uses `chat()` (multi-turn) not `prompt()` (one-shot). Key differences:

- `prompt(message)` — single user message, returns single response. No history. Suitable for one-shot questions.
- `chat(message, history)` — user message + conversation history, returns response. History enables multi-turn context.

Since rig agents from different providers have different concrete types, and `chat()` involves generic type parameters, use an **enum dispatch** pattern for provider selection:

```rust
enum ProviderAgent {
    Anthropic(/* anthropic agent type */),
    OpenAi(/* openai agent type */),
}
```

Each variant wraps the provider-specific agent. The `ask()` method matches on the variant and calls `chat()` on the concrete type. This avoids trait object issues with async methods and provider-specific generics.

If rig's `Chat` trait IS object-safe (check at implementation time), `Box<dyn Chat>` is simpler. Attempt it first; fall back to enum dispatch if it doesn't compile.

### Retry Strategy — Not Handled by Middleware

**Critical note:** rig-core creates its own HTTP clients internally. The `build_http_client()` with reqwest-retry middleware from Story 1.1 is NOT used by rig providers. This means:

- LLM API errors (rate limits, timeouts, 5xx) from the Architect session are NOT automatically retried
- The `ArchitectSession::ask()` method should implement explicit retry logic around the full chat sequence:
  - On transient error (timeout, rate limit), retry the entire session (fresh agent, fresh history)
  - Max 2 retries (3 total attempts) with exponential backoff (1s, 4s)
  - On persistent failure after retries, return `ArchitectSessionError::ChatFailed`
- Alternatively, implement retry only around individual `chat()` calls that fail, preserving history for successful turns

**Recommended approach:** Retry the entire session on failure (simpler, avoids partial state issues). The overhead is acceptable since supervisor calls are infrequent.

### Integration with Future Stories

**Story 3.3 (Human Escalation)** will refine the escalation path:
- When `ArchitectSession::ask()` fails, `call()` returns `Err(SupervisorError::EscalationRequired)`
- Story 3.3 adds: session module catches this → marks story `needs-clarification` → notifies human
- No changes to `ArchitectSession` needed

**Story 3.4 (Decision Logging)** will:
- Record a `DecisionRecord` for every Architect fallback call (question, answer, `DecisionSource::LlmFallback`)
- No changes to `ArchitectSession` needed — logging happens at the `call()` level

**Epic 4 (Session)** will:
- Construct `AskSupervisor::with_architect_from_config(&config)` at session startup
- Register via `.tool(ask_supervisor)` on the dev agent builder
- The `ArchitectSession` is created once at daemon startup and shared (it's reusable — each `ask()` creates a fresh internal agent)

**Epic 4, Story 4.1 (Tools)** will:
- Create the full `src/tools/fs.rs` filesystem tool with read + write
- The supervisor's `ReadFile` in `src/supervisor/read_tool.rs` remains separate — it is intentionally limited and supervisor-specific

### Imports Required in `src/supervisor/mod.rs` (Updated)

```rust
use rig::tool::Tool;
use rig::completion::ToolDefinition;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub mod rules;
pub mod decisions;
pub mod read_tool;    // NEW in Story 3.2
pub mod architect;    // NEW in Story 3.2

use rules::{RuleEngine, RuleResult};
use architect::ArchitectSession;  // NEW in Story 3.2
```

### Files Modified/Created in This Story

| File | Change |
|------|--------|
| `src/supervisor/mod.rs` | **MODIFY** — Add `architect_session: Option<ArchitectSession>` field with `#[serde(skip)]`, add `with_architect()` and `with_architect_from_config()` constructors, update `call()` with Architect fallback logic, add `pub mod read_tool;` and `pub mod architect;`, update unit tests |
| `src/supervisor/read_tool.rs` | **CREATE** — `ReadFile` tool (rig Tool trait), `ReadFileArgs`, `ReadFileError`, project-root path boundary |
| `src/supervisor/architect.rs` | **CREATE** — `ArchitectSession` struct, `ArchitectSessionError` enum, provider factory, multi-turn `ask()` method |
| `src/supervisor/rules.rs` | **NO CHANGE** |
| `src/supervisor/decisions.rs` | **NO CHANGE** |

### Anti-Patterns to Avoid

- ❌ **NO** generic "LLM + docs dump" approach — the supervisor IS a BMAD Architect session, not a raw completion call
- ❌ **NO** skipping the BMAD activation flow — send the full `architect.md` as preamble, drive CH mode properly, let the Architect load context himself
- ❌ **NO** pre-loading docs and injecting them — the Architect uses the `ReadFile` tool to load what IT decides is needed
- ❌ **NO** persistent state between supervisor calls — each `ask()` creates a fresh agent and history
- ❌ **NO** real LLM API calls in unit tests — mock everything, E2E tests in `tests/e2e/` gated behind `BMAD_E2E=1`
- ❌ **NO** hardcoded API keys — read from environment variables via `std::env::var(api_key_env)`
- ❌ **NO** logging API keys or secrets via tracing — `NFR-SEC2` applies
- ❌ **NO** `unwrap()` or `expect()` in production code
- ❌ **NO** `anyhow::Result` in supervisor module — typed errors only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** modifying `rules.rs` or `decisions.rs`
- ❌ **NO** decision logging in `call()` — that's Story 3.4
- ❌ **NO** human escalation refinement — that's Story 3.3
- ❌ **NO** tool registration with dev agent — that's Epic 4
- ❌ **NO** write access in the `ReadFile` tool — read-only, project-root bounded
- ❌ **NO** allowing `ReadFile` to access paths outside `{project_root}`

### Scope Boundaries

**IN SCOPE for this story:**
- `src/supervisor/read_tool.rs` — Minimal `ReadFile` rig tool (read-only, project-root bounded)
- `src/supervisor/architect.rs` — `ArchitectSession`, `ArchitectSessionError`, provider factory, multi-turn `ask()` with chat API
- `src/supervisor/mod.rs` — Updated `AskSupervisor` with `architect_session` field, new constructors, updated `call()` pipeline

**OUT OF SCOPE — do NOT implement:**
- Full filesystem tool with write (Epic 4, Story 4.1 — separate `src/tools/fs.rs`)
- Human escalation refinement beyond returning `EscalationRequired` error (Story 3.3)
- Decision logging for Architect fallback calls (Story 3.4)
- Tool registration with rig dev agent (Epic 4, Story 4.2)
- Notification of escalation to human (Epic 6, Story 6.1)
- Routing to different BMAD agents based on question type (future enhancement — currently always Architect)
- Caching of Architect responses for repeated questions (future enhancement)
- Confidence scoring of Architect answers (future enhancement)

### Testing Requirements

**In `src/supervisor/read_tool.rs` — `#[cfg(test)] mod tests`:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_read_file_existing_file() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("test.md");
        fs::write(&file_path, "# Test Content\nHello").unwrap();

        let tool = ReadFile::new(dir.path().to_path_buf());
        let args = ReadFileArgs { path: "test.md".to_string() };
        let result = tool.call(args).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "# Test Content\nHello");
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let dir = TempDir::new().unwrap();
        let tool = ReadFile::new(dir.path().to_path_buf());
        let args = ReadFileArgs { path: "nonexistent.md".to_string() };
        let result = tool.call(args).await;
        assert!(matches!(result.unwrap_err(), ReadFileError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_read_file_path_denied_outside_root() {
        let dir = TempDir::new().unwrap();
        // Create a file outside the project root
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("secret.txt");
        fs::write(&outside_file, "secret").unwrap();

        let tool = ReadFile::new(dir.path().to_path_buf());
        let args = ReadFileArgs { path: format!("../../{}", outside_file.display()) };
        let result = tool.call(args).await;
        // Should be denied or not found (path traversal blocked)
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file_nested_path() {
        let dir = TempDir::new().unwrap();
        let sub_dir = dir.path().join("_bmad/bmm/agents");
        fs::create_dir_all(&sub_dir).unwrap();
        fs::write(sub_dir.join("architect.md"), "# Architect").unwrap();

        let tool = ReadFile::new(dir.path().to_path_buf());
        let args = ReadFileArgs { path: "_bmad/bmm/agents/architect.md".to_string() };
        let result = tool.call(args).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "# Architect");
    }

    #[tokio::test]
    async fn test_read_file_tool_definition() {
        let tool = ReadFile::new(PathBuf::from("/tmp"));
        let def = tool.definition("test".to_string()).await;
        assert_eq!(def.name, "read_file");
        assert!(!def.description.is_empty());
    }

    #[test]
    fn test_read_file_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ReadFileError>();
    }
}
```

**In `src/supervisor/architect.rs` — `#[cfg(test)] mod tests`:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_test_config(
        dir: &TempDir,
        provider: &str,
        api_key_env: &str,
        write_agent: bool,
    ) -> BotConfig {
        if write_agent {
            let agents_dir = dir.path().join("_bmad/bmm/agents");
            std::fs::create_dir_all(&agents_dir).unwrap();
            std::fs::write(
                agents_dir.join("architect.md"),
                "# Architect Agent\nTest persona content",
            ).unwrap();
        }

        // Construct a minimal BotConfig with supervisor LLM config
        // pointing to the test directory as project_root
        // ... (adjust to actual BotConfig struct from Story 1.1)
        todo!("Construct BotConfig matching Story 1.1 struct")
    }

    #[test]
    fn test_architect_session_missing_agent_file() {
        let dir = TempDir::new().unwrap();
        let config = make_test_config(&dir, "openai", "FAKE_KEY_ENV", false);
        let result = ArchitectSession::new(&config);
        assert!(matches!(
            result.unwrap_err(),
            ArchitectSessionError::AgentFileNotFound { .. }
        ));
    }

    #[test]
    fn test_architect_session_missing_api_key() {
        let dir = TempDir::new().unwrap();
        let config = make_test_config(&dir, "openai", "NONEXISTENT_ENV_VAR_12345", true);
        let result = ArchitectSession::new(&config);
        assert!(matches!(
            result.unwrap_err(),
            ArchitectSessionError::ApiKeyMissing { .. }
        ));
    }

    #[test]
    fn test_architect_session_unsupported_provider() {
        let dir = TempDir::new().unwrap();
        std::env::set_var("TEST_SUPERVISOR_KEY_3_2", "fake-key");
        let config = make_test_config(&dir, "unsupported-provider", "TEST_SUPERVISOR_KEY_3_2", true);
        let result = ArchitectSession::new(&config);
        assert!(matches!(
            result.unwrap_err(),
            ArchitectSessionError::UnsupportedProvider { .. }
        ));
        std::env::remove_var("TEST_SUPERVISOR_KEY_3_2");
    }

    #[test]
    fn test_architect_session_error_display() {
        let err = ArchitectSessionError::AgentFileNotFound {
            path: "/some/path".to_string(),
        };
        assert!(err.to_string().contains("/some/path"));

        let err = ArchitectSessionError::ChatFailed {
            turn: 2,
            reason: "timeout".to_string(),
        };
        assert!(err.to_string().contains("2"));
        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn test_architect_session_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ArchitectSessionError>();
    }
}
```

**In `src/supervisor/mod.rs` — updated `#[cfg(test)] mod tests`:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // === Existing Story 3.1 tests (MUST all still pass) ===

    #[tokio::test]
    async fn test_ask_supervisor_returns_answer_for_matching_question() {
        let supervisor = AskSupervisor::new();
        let args = AskSupervisorArgs {
            question: "Should I proceed with the implementation?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Yes, proceed.");
    }

    #[tokio::test]
    async fn test_ask_supervisor_returns_error_for_no_match_without_architect() {
        let supervisor = AskSupervisor::new(); // No Architect configured
        let args = AskSupervisorArgs {
            question: "What database schema should I use?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            SupervisorError::LlmFallbackNotImplemented => {} // expected
            other => panic!("Expected LlmFallbackNotImplemented, got: {other}"),
        }
    }

    #[tokio::test]
    async fn test_ask_supervisor_with_context_matching_rule() {
        let supervisor = AskSupervisor::new();
        let args = AskSupervisorArgs {
            question: "Should I proceed?".to_string(),
            context: Some("Working on task 3".to_string()),
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ask_supervisor_tool_definition_correct_name() {
        let supervisor = AskSupervisor::new();
        let def = supervisor.definition("test".to_string()).await;
        assert_eq!(def.name, "ask_supervisor");
        assert!(!def.description.is_empty());
        let params = &def.parameters;
        assert!(params["required"].as_array().unwrap()
            .iter().any(|v| v.as_str() == Some("question")));
    }

    #[test]
    fn test_decision_record_serializable() {
        let record = decisions::DecisionRecord {
            question: "Should I proceed?".to_string(),
            answer: "Yes, proceed.".to_string(),
            source: decisions::DecisionSource::RuleEngine {
                rule_name: "confirmation_proceed".to_string(),
            },
            reasoning: "Matched confirmation pattern".to_string(),
            alternatives: vec!["Wait for explicit approval".to_string()],
            timestamp: "2026-02-07T12:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&record).expect("Should serialize");
        let deserialized: decisions::DecisionRecord =
            serde_json::from_str(&json).expect("Should deserialize");
        assert_eq!(deserialized.question, "Should I proceed?");
    }

    #[test]
    fn test_supervisor_error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SupervisorError>();
    }

    // === NEW Story 3.2 tests ===

    #[test]
    fn test_ask_supervisor_serialization_skips_architect() {
        let supervisor = AskSupervisor::new();
        let json = serde_json::to_string(&supervisor).expect("Should serialize");
        assert!(!json.contains("architect_session"));
        let deserialized: AskSupervisor =
            serde_json::from_str(&json).expect("Should deserialize");
        // Deserialized supervisor has no architect session — NoMatch returns old error
    }

    #[tokio::test]
    async fn test_ask_supervisor_rule_match_bypasses_architect() {
        // Even if an Architect session were configured, rule engine match
        // should return immediately without launching a session.
        let supervisor = AskSupervisor::new();
        let args = AskSupervisorArgs {
            question: "Should I proceed?".to_string(),
            context: None,
        };
        let result = supervisor.call(args).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Yes, proceed.");
    }
}
```

### Project Structure Notes

After this story, the supervisor module structure is:

```
src/supervisor/
├── mod.rs          # AskSupervisor tool (updated with architect_session), SupervisorError
├── rules.rs        # RuleEngine, RulePattern, Rule, RuleResult (unchanged)
├── decisions.rs    # DecisionRecord, DecisionSource stubs (unchanged)
├── read_tool.rs    # NEW: ReadFile rig tool (read-only, project-root bounded)
└── architect.rs    # NEW: ArchitectSession, ArchitectSessionError, multi-turn ask()
```

Epic 3 progress after this story:
- **Story 3.1:** Supervisor Tool Skeleton & Rule Engine ✅
- **Story 3.2:** LLM Fallback with Project Context ✅ (this story)
- **Story 3.3:** Human Escalation (next)
- **Story 3.4:** Decision Logging & Traceability

### Dev Dependencies Required

Add to `Cargo.toml` under `[dev-dependencies]` (if not already present):
- `tempfile` — for creating temporary directories in tests

### References

- [Source: epics.md § Story 3.2: LLM Fallback with Project Context] — User story, acceptance criteria
- [Source: epics.md § Epic 3: Intelligent Supervision] — Epic context, FR12–FR17
- [Source: prd.md § FR14] — Answer substantive questions via LLM fallback with project docs context
- [Source: prd.md § FR15] — Escalate to human when neither rules nor LLM can answer confidently
- [Source: architecture.md § Decision 1: Supervisor Interception Model] — Hybrid Chat Loop + Supervisor Tool
- [Source: architecture.md § Decision 3: Session State Persistence] — Context limit recovery pattern: fresh session, CH mode, context injection — the same pattern used for the Architect supervisor session
- [Source: architecture.md § Decision 4: Error Propagation] — Three-tier error handling, retry not handled by rig internally
- [Source: architecture.md § Rig Tool Implementation Pattern] — Standard structure for all rig tools
- [Source: architecture.md § Error Type Pattern] — Per-module thiserror enums
- [Source: architecture.md § Test Mock Pattern] — Deterministic mocked responses
- [Source: architecture.md § Project Structure & Boundaries] — supervisor/ module files, external integration points
- [Source: project-context.md § Supervisor Hybrid Pattern] — Rule engine → LLM fallback → human escalation
- [Source: project-context.md § Critical Don't-Miss Rules] — "Supervisor must never invent answers"
- [Source: project-context.md § Multi-Provider LLM Config] — Three LLM roles: dev, review, supervisor
- [Source: project-context.md § Testing Rules] — Mock responses only, E2E gated behind BMAD_E2E=1
- [Source: project-context.md § Framework-Specific Rules (rig)] — rig agent + tool calling patterns
- [Source: Story 3.1 § Integration with Future Stories] — Serde skip, LLM client field, constructor change
- [Source: Story 3.1 § AskSupervisor Tool Implementation] — Current call() pipeline to be modified
- [Source: Story 1.1] — BotConfig with LlmConfig.supervisor, project_root, Arc<BotConfig>
- [Source: architect.md] — Full BMAD Architect agent file used as preamble
- [Source: agent-manifest.csv] — Agent registry with paths and descriptions
- [Source: rig-core docs] — Chat trait, agent builder, tool registration, multi-turn conversation API

## Dev Agent Record

<!-- This section is filled automatically by the dev agent post-implementation. Do not edit manually. -->

### Agent Model Used

_(filled post-implementation)_

### Debug Log References

### Completion Notes List

### File List