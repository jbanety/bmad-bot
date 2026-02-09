---
stepsCompleted: [1, 2, 3, 4, 5, 6, 7, 8]
inputDocuments: ['_bmad-output/planning-artifacts/prd.md', '_bmad-output/project-context.md']
workflowType: 'architecture'
project_name: 'bmad-bot'
user_name: 'JB'
date: '2026-02-07'
lastStep: 8
status: 'complete'
completedAt: '2026-02-07'
---

# Architecture Decision Document

_This document builds collaboratively through step-by-step discovery. Sections are appended as we work through each architectural decision together._

## Project Context Analysis

### Requirements Overview

**Functional Requirements:**
36 FRs across 9 domains. The system is a pipeline daemon: watcher → pre-gate → session → supervision → review → PR → notification. Each domain maps cleanly to an architectural module. The supervisor (FR12-17) is the most complex component, combining a deterministic rule engine, LLM fallback, and full decision traceability.

**Non-Functional Requirements:**
- Security: Secrets never in committed config or logs. Environment-variable-only secrets management.
- Reliability: Exponential backoff (max 3 retries) for transient LLM errors. Graceful shutdown with partial commit on SIGTERM/SIGINT. Crash recovery produces clean state.
- Integration: GitHub API with rate limit handling, Telegram notifications (non-blocking), multi-provider LLM support.
- Scalability: MVP is single-daemon sequential execution. Architecture must not preclude future parallelization (multi-worker, story-level concurrency).

**Scale & Complexity:**
- Primary domain: Backend CLI daemon / Developer tool
- Complexity level: Medium
- Estimated architectural components: 8 core modules (cli, config, watcher, session, supervisor, review, tools, notifier)

### Technical Constraints & Dependencies

- **rig-core maturity:** Core dependency for agent orchestration. Evaluate early — fallback is direct LLM provider API calls.
- **git2 (libgit2):** Embedded, no external git CLI dependency. All git operations through library bindings.
- **LLM provider variability:** Three providers may behave differently (rate limits, response formats, error codes). Abstraction layer required.
- **BMAD files are read-only:** The daemon never modifies anything under `_bmad/`. All output goes to `_bmad-output/`.
- **Sequential execution in MVP:** Simplifies architecture significantly — no concurrency primitives needed for story processing.

### Cross-Cutting Concerns Identified

1. **Error handling & resilience** — Every component must handle failures gracefully, log with full context, and propagate to notification when blocking.
2. **Structured logging with story context** — `tracing` spans with `story_id` across the entire pipeline for debuggability.
3. **LLM provider abstraction** — Three independent roles (dev, review, supervisor) each configurable with different providers. Shared retry/backoff logic.
4. **Decision traceability** — Supervisor decisions flow from rule engine/LLM through to decisions file and PR description. End-to-end audit trail.
5. **Secret management** — Filtering in logs, separation in config, environment-variable-only loading. Applies to all components that touch credentials.

## Starter Template Evaluation

### Primary Technology Domain

Rust CLI daemon / Developer tool — long-running autonomous process with CLI interface for setup and monitoring.

### Starter Options Considered

No traditional starter template applies. Rust daemon projects start from `cargo init` with deliberate dependency selection. The technology stack is fully defined in the Project Context.

### Selected Approach: cargo init + curated dependencies

**Rationale:**
Rust ecosystem does not have opinionated starter templates like web frameworks. The Project Context already locks the core stack (tokio, rig-core, git2, serde, tracing). The remaining foundation decisions are CLI framework, config loading, Git provider abstraction, and signal handling.

**Initialization Command:**

```bash
cargo init bmad-bot
```

### Architectural Decisions — Foundation Layer

**Language & Runtime:**
- Rust edition 2024, single binary target
- Full async tokio runtime (`features = ["full"]`)
- Single crate with modular directory structure (not a Cargo workspace for MVP)

**CLI Framework:**
- clap with derive API — handles `init`, `start`, `status`, `logs` subcommands
- Auto-generated `--help`, shell completion support

**Configuration Loading:**
- serde + serde_yaml for `bmad-bot.yaml`
- dotenvy for `.env` secrets loading
- Custom validation with thiserror error types

**Signal Handling:**
- tokio::signal for SIGTERM/SIGINT graceful shutdown

**Git Provider Abstraction:**
- Trait `GitProvider` with async methods: `create_pr()`, `add_pr_comment()`, `get_pr_url()`
- **GitHub implementation:** octocrab — mature Rust client, built-in rate limit handling and pagination
- **GitLab implementation:** reqwest direct calls to GitLab REST API (v4) — lightweight, no heavy crate needed for the limited surface area (create MR, post comment)
- Provider selected via `bmad-bot.yaml` config (`git_provider: github | gitlab`)

**Telegram API:**
- reqwest direct HTTP calls (simple send message endpoint, no crate needed)

**Project Structure:**

```
bmad-bot/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── cli/
│   │   └── mod.rs
│   ├── config/
│   │   └── mod.rs
│   ├── watcher/
│   │   ├── mod.rs
│   │   └── deps.rs
│   ├── session/
│   │   ├── mod.rs
│   │   └── parser.rs
│   ├── supervisor/
│   │   ├── mod.rs
│   │   ├── rules.rs
│   │   └── decisions.rs
│   ├── review/
│   │   └── mod.rs
│   ├── tools/
│   │   ├── mod.rs
│   │   ├── git.rs
│   │   ├── fs.rs
│   │   └── terminal.rs
│   ├── git_provider/
│   │   ├── mod.rs
│   │   ├── github.rs
│   │   └── gitlab.rs
│   └── notifier/
│       └── mod.rs
└── tests/
    └── e2e/
```

**Note:** Project initialization (`cargo init` + dependency setup + module scaffolding) should be the first implementation story.

## Core Architectural Decisions

### Decision Priority Analysis

**Critical Decisions (Block Implementation):**
1. Supervisor interception model — How the supervisor integrates with rig's agent loop
2. Sprint-status.yaml mutation strategy — Who writes, who reads
3. Session state persistence — Crash recovery mechanism

**Important Decisions (Shape Architecture):**
4. Error propagation strategy — Layered error handling with bubble-up
5. Agent prompt composition — How the BMAD agent is loaded and configured
6. Deployment model — How the daemon runs as a process

**Deferred Decisions (Post-MVP):**
- Multi-worker orchestration (v2/v3)
- CI/CD pipeline setup
- Web dashboard architecture
- Plugin system design

### Decision 1: Supervisor Interception Model — Hybrid Chat Loop + Supervisor Tool

**Decision:** Combine an external chat loop with an internal `ask_supervisor` rig tool.

**Rationale:**
rig v0.29.0 exposes two main APIs: `agent.prompt()` and `agent.chat()`. Both handle tool-calling internally in an opaque loop — there is no hook or callback to intercept individual turns. This rules out proxy-based or hook-based interception.

The hybrid approach uses both rig interaction patterns for their natural strengths:

**Chat loop (external)** — The daemon controls the session via `agent.chat(message, history)` in a loop. This replaces the human sitting at the terminal. When the agent returns text (end of a turn), the daemon analyzes it for workflow interaction points (confirmations, "should I proceed?", step transitions) and responds automatically. This handles the BMAD workflow's natural conversation flow.

**`ask_supervisor` tool (internal)** — Registered as a standard rig tool alongside git/fs/terminal. When the agent has a substantive question or doubt *during* tool-calling work, it calls `ask_supervisor`. Inside the tool's `call()` method:
1. Rule engine (deterministic, free) — matches known patterns
2. LLM fallback (context-aware) — loads project docs to answer
3. Human escalation — returns error, which stops the rig loop and gives the daemon control back

**Why this works:**
- Chat loop handles workflow-level interaction (the "human replacement" layer)
- Supervisor tool handles technical/business questions (the "decision-making" layer)
- Both are natural rig patterns — no fighting the framework
- Tool calls are natively logged by rig — built-in traceability
- The supervisor tool is unit-testable in isolation
- Minimal exposure to rig breaking changes (tool API is the most stable surface)

**Affects:** session module, supervisor module, tools module

### Decision 2: Sprint-Status Mutation — Daemon Reads, Agent Writes

**Decision:** The daemon is a pure reader of `sprint-status.yaml`. All mutations are performed by the BMAD agent.

**Rationale:**
`sprint-status.yaml` lives in `_bmad-output/` (path configured in BMAD config). The BMAD agent has full workflow context and knows how to properly update story statuses, mark dependencies as blocked, and cascade state changes. The daemon lacks this context.

**Flow:**
1. **Daemon polls** — reads `sprint-status.yaml` from the configured output path
2. **Pre-gate** — computes dependency satisfaction in-memory (pure read, no writes). Stories with unmet dependencies are skipped, not marked.
3. **Agent session** — the agent reads and writes `sprint-status.yaml` as part of its BMAD workflow (mark story in-progress, completed, blocked, needs-clarification)
4. **Next poll cycle** — daemon re-reads the file, now updated by the agent

**Cascade blocking:** Either handled explicitly by the agent (marks dependents as blocked) or implicitly by the pre-gate (skips stories whose parents aren't `done`). No daemon writes needed.

**Consequence:** The daemon has zero write operations on any BMAD artifact. Its only outputs are: PR creation (via Git provider API), notifications (via Telegram API), session state file, and logs.

**Affects:** watcher module, session module

### Decision 3: Session State Persistence — WAL File for Crash & Context Limit Recovery

**Decision:** Persist session state (including full chat history) to a YAML file after each chat turn. Delete on successful session completion. On context limit, use the WAL to bootstrap a fresh session with summarized history.

**State file location:** `_bmad-output/implementation-artifacts/.bmad-bot-session.yaml`

**State file contents:**
- `story_id` — current story identifier
- `branch` — git branch name
- `started_at` — session start timestamp
- `last_activity` — last update timestamp
- `provider` / `model` — LLM config for session reconstruction
- `chat_history` — complete `Vec<Message>` serialized (role + content for each turn)

**Lifecycle:**
- **Session start** → create state file
- **After each chat loop turn** → overwrite state file with updated history
- **Session complete (PR created)** → delete state file
- **Polling** → only when no active session (no state file exists)

**Recovery Case A — Crash recovery on daemon startup:**
1. Check for existing state file
2. If found → interrupted session detected
3. Check git state (branch `story/xxx`, dirty files confirm crash)
4. Reload chat history from state file
5. Reconstruct agent with same provider/model config
6. Resume `chat()` with loaded history — agent has full context and continues
7. If not found → clean start, begin polling

**Recovery Case B — Context limit recovery (mid-session):**

The LLM API returns a context limit error during the chat loop. This is detectable from the provider response. The WAL file already contains the full `chat_history` on disk, so no proactive save is needed.

Recovery flow:
1. Detect context limit error from LLM API response
2. Read full `chat_history` from WAL file (already persisted after each turn)
3. Extract last N exchanges (complete, verbatim) from the WAL — these are the most relevant immediate context
4. Make a **separate, fresh LLM call** (new context) to summarize the full `chat_history` from the WAL into a compact session summary
5. Reconstruct agent with same provider/model config (same persona, same tools)
6. **Do not re-enter the full dev-story workflow pipeline** — instead, start a direct chat session (equivalent to CH mode)
7. Inject into the new session preamble:
   - Agent persona + tool registrations (standard)
   - Project context (`project-context.md`) — so the agent knows the project, conventions, patterns
   - The generated session summary — compressed history of everything that happened
   - The last N verbatim exchanges — immediate context for continuity
   - Current story file reference — checkboxes and Dev Agent Record are already up to date on disk
8. Resume `chat()` — the agent picks up the current task with full awareness of prior work

**Key design points:**
- The summarization call uses a **fresh context** (not the exhausted one), so it can process the full WAL history
- The story file on disk already tracks task progress (`[x]`/`[ ]`), Dev Agent Record, File List — no duplication needed
- The resumed session is lean: summary + last exchanges + project context fits well within budget
- From the agent's perspective, it's like being briefed by a colleague and continuing the work

**Affects:** session module, config module

### Decision 4: Error Propagation — Layered with Bubble-Up

**Decision:** Three-tier error handling where each layer manages its own errors and escalates what it cannot resolve.

**Layer 1 — HTTP Transport (reqwest-middleware):**
- Automatic retry with exponential backoff for transient HTTP errors (429, 500, 503, timeouts)
- Max 3 retries per request
- Applies globally: LLM providers, GitHub/GitLab API, Telegram API
- Transparent to higher layers — they only see the final result

**Layer 2 — Tools & Components (git, fs, terminal, supervisor):**
- Handle domain-specific errors (git conflicts, file permissions, parse failures)
- Recoverable errors → retry or fallback within the tool
- Unrecoverable errors → bubble up as tool errors to rig, which stops the agent loop
- All errors logged via `tracing::error!()` with story context

**Layer 3 — Session & Daemon (chat loop, main loop):**
- Session errors (agent escalation, tool crash, unrecoverable git state) → commit partial work, create PR with failure description, notify human, move to next story
- Fatal errors (config invalid, all providers down after retries, SIGTERM) → graceful shutdown: commit if possible, notify, exit
- Notification failures are non-blocking at all layers — logged but never stop story processing

**Affects:** all modules (cross-cutting)

### Decision 5: Agent Prompt Composition — Load BMAD Agent File Directly

**Decision:** The BMAD dev agent file is loaded as-is and used as the rig agent preamble. The only addition is a language override appended at the end.

**Rationale:**
The BMAD agent file (`dev.md` or equivalent) already contains the complete agent definition: persona, activation instructions, workflow references, menu system. The agent's workflows instruct it to read whatever context it needs (story specs, project docs, prior implementations) via filesystem tools. No pre-loading or context injection by the daemon is necessary.

**Implementation:**
```
let agent_prompt = fs::read_to_string(&bmad_dev_agent_path).await?;
let preamble = format!("{agent_prompt}\n\nOVERRIDE: communication_language = English");

let agent = provider
    .agent(model)
    .preamble(&preamble)
    .tool(git_tool)
    .tool(fs_tool)
    .tool(terminal_tool)
    .tool(ask_supervisor)
    .build();
```

**First message:** `"DS"` (triggers the dev-story workflow as defined in the BMAD agent's menu system).

**Key principle:** The daemon knows nothing about BMAD workflow internals. It loads the agent file, registers tools, and sends the start command. Everything else is the agent's responsibility.

**Affects:** session module

### Decision 6: Deployment Model — Foreground Process

**Decision:** `bmad-bot start` runs as a simple foreground process. No self-daemonization.

**Rationale:**
This is a developer tool, not infrastructure software. Users can background it with standard OS tools (`tmux`, `screen`, `nohup`, `systemd`, `launchd`). Adding daemonization (fork, PID files, log rotation) is unnecessary complexity for the MVP.

**Behavior:**
- Logs to stdout/stderr via `tracing` (structured JSON or pretty-print based on config)
- SIGTERM/SIGINT triggers graceful shutdown
- No PID file, no log file management, no auto-restart
- Future (v2): could add `--daemon` flag or provide example systemd/launchd service files

**Affects:** cli module, main

### Decision Impact Analysis

**Implementation Sequence:**
1. Foundation: cargo init, CLI (clap), config loading, signal handling
2. Tools: git (git2), filesystem, terminal as rig Tool traits
3. Watcher: sprint-status.yaml parser, dependency graph, pre-gate logic
4. Session: rig agent setup, chat loop, state file persistence
5. Supervisor: ask_supervisor tool, rule engine, LLM fallback, decision logging
6. Git Provider: GitHub + GitLab PR creation trait + implementations
7. Review: separate LLM session for code review (optional, configurable)
8. Notifier: Telegram integration

**Cross-Component Dependencies:**
- Session depends on: tools, supervisor, config, git_provider
- Supervisor depends on: config (LLM provider for fallback), decisions logging
- Watcher depends on: config (paths, polling interval)
- Git Provider depends on: config (provider selection, credentials)
- All components depend on: error handling strategy (Layer 1-3), tracing setup

## Implementation Patterns & Consistency Rules

### Pattern Categories Defined

**Critical Conflict Points Identified:**
6 areas where AI agents could make different implementation choices. The Project Context already covers Rust conventions (snake_case, rustfmt, clippy, doc comments, module structure, testing placement). The patterns below address bmad-bot-specific concerns not covered there.

### Error Type Pattern — Per-Module thiserror Enums

Each module defines its own error enum using `thiserror`. `anyhow` is used only in `main.rs` / CLI layer for composition. Never use `anyhow` inside library modules.

```
// Pattern: each module owns its error type
#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
    #[error("Failed to read sprint-status: {0}")]
    SprintStatusRead(#[from] std::io::Error),
    #[error("Invalid sprint-status format: {0}")]
    SprintStatusParse(#[from] serde_yaml::Error),
    #[error("No eligible stories found")]
    NoEligibleStories,
}

// Modules expose typed errors via thiserror
// main.rs/cli uses anyhow to compose module errors
// Never anyhow in library modules — only in binary entry points
```

**Why this matters:** Without this rule, one agent might use `anyhow::Result` everywhere while another creates typed errors. Typed errors enable pattern matching in the session/daemon layers for error-level decisions.

### Rig Tool Implementation Pattern — Standard Structure

Every rig tool (git, fs, terminal, ask_supervisor) follows the same structural pattern:

```
// 1. Serializable struct with shared state
#[derive(Deserialize, Serialize)]
pub struct MyTool {
    // Shared config/state needed by the tool
}

// 2. Dedicated args struct
#[derive(Deserialize)]
pub struct MyToolArgs {
    pub action: String,
    // Action-specific parameters
}

// 3. Dedicated error enum (thiserror)
#[derive(Debug, thiserror::Error)]
pub enum MyToolError {
    #[error("...")]
    SpecificError(/* ... */),
}

// 4. Tool trait implementation
impl Tool for MyTool {
    const NAME: &'static str = "my_tool";  // snake_case, descriptive
    type Error = MyToolError;               // Dedicated thiserror enum
    type Args = MyToolArgs;                 // Dedicated Deserialize struct
    type Output = String;                   // String for rig compatibility

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        // Detailed description so the LLM knows when/how to use the tool
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Log via tracing before action
        // Execute
        // Log result or error
        // Return
    }
}
```

**Mandatory rules:**
- Tool NAME is always snake_case and descriptive
- Tool definition description must be detailed enough for the LLM to use correctly
- Every `call()` logs the action and result via `tracing`
- Never panic in a tool — always return `Result`

### Tracing Pattern — Structured Spans with Story Context

```
// Session level: open a span with story_id for the entire session
let span = tracing::info_span!("story_session", story_id = %story.id, branch = %branch);
let _guard = span.enter();

// Tool/action level: always log action + context
tracing::info!(action = "git_commit", message = %msg, "Committing changes");
tracing::warn!(action = "supervisor_fallback", question = %q, "Rule engine miss, using LLM");
tracing::error!(action = "git_push", error = %e, "Push failed");

// NEVER log sensitive data (API keys, tokens, secrets)
// NEVER use println! or eprintln! — tracing only
```

**Mandatory rules:**
- Every session wrapped in a `story_session` span with `story_id`
- Every tool action logged with `action` field
- Errors always include `error` field with the error value
- Sensitive fields filtered — never log API keys, tokens, or credentials

### Config Pattern — Validate Once, Share via Arc

```
// Config structs with serde + custom validation
#[derive(Debug, Deserialize)]
pub struct BotConfig {
    pub polling_interval_secs: u64,
    pub git_provider: GitProviderConfig,
    pub llm: LlmConfig,
    pub notifications: NotificationConfig,
}

impl BotConfig {
    /// Validates all config fields. Called once at startup.
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Business validation: interval > 0, provider recognized, etc.
    }
}

// Config is loaded, validated, then wrapped in Arc<BotConfig>
// Passed to modules as Arc<BotConfig> — never cloned in full
// Secrets loaded separately from .env via dotenvy, never in this struct
```

**Mandatory rules:**
- Config loaded and validated exactly once at startup
- Shared as `Arc<BotConfig>` — never cloned, never mutated after load
- Secrets (API keys, tokens) loaded from `.env` via dotenvy, stored separately
- Validation errors are descriptive — report exactly which field failed and why

### Git Provider Trait Pattern — Params as Structs

```
#[async_trait]
pub trait GitProvider: Send + Sync {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError>;
    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError>;
    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError>;
}

// Input params and return values always as dedicated structs
pub struct CreatePrParams {
    pub title: String,
    pub body: String,
    pub source_branch: String,
    pub target_branch: String,
}

pub struct PrInfo {
    pub id: String,
    pub url: String,
    pub number: u64,
}
```

**Mandatory rules:**
- Trait methods use dedicated param/return structs — never loose primitives for complex inputs
- All trait methods are async
- Error type is a dedicated `GitProviderError` enum (thiserror)
- Implementations (GitHub via octocrab, GitLab via reqwest) are in separate files

### Test Mock Pattern — Deterministic LLM Responses

```
#[cfg(test)]
mod tests {
    use super::*;

    // Mock responses as constants or fixtures — never random, never live API
    const MOCK_CONFIRMATION_QUESTION: &str = "Should I proceed with the implementation?";
    const MOCK_SUPERVISOR_ANSWER: &str = "Yes, proceed.";

    #[tokio::test]
    async fn test_rule_engine_matches_confirmation_pattern() {
        // Arrange: set up rule engine with known rules
        // Act: pass the mock question
        // Assert: verify the expected answer
    }

    // Test naming: test_{module}_{behavior}_{scenario}
    // Always Arrange → Act → Assert order
    // Mock all external dependencies (LLM, GitHub, Telegram)
    // Never call real APIs in unit tests
}
```

**Mandatory rules:**
- Test naming: `test_{module}_{behavior}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert, always in that order
- LLM responses mocked with static data — never call real providers
- Each new module must include at least basic unit tests before being considered complete
- E2E tests in `tests/` directory, gated behind `BMAD_E2E=1` env var

### Enforcement Guidelines

**All AI Agents MUST:**
- Follow the error type pattern: thiserror per module, anyhow only in binary
- Implement rig tools using the standard structure above
- Use tracing with structured fields — never println/eprintln
- Pass config as `Arc<BotConfig>` — never clone, never mutate
- Use dedicated structs for trait method params and returns
- Write unit tests with mocked dependencies for every new module
- Check the Project Context file for additional rules before implementing any code

**Anti-Patterns (NEVER do these):**
- `unwrap()` or `expect()` in production code
- `anyhow::Result` in library modules
- Loose primitives as function params when 3+ params exist
- Logging sensitive data (API keys, tokens, passwords)
- Calling real LLM APIs in unit tests
- Skipping doc comments on public items

## Project Structure & Boundaries

### Complete Project Directory Structure

```
bmad-bot/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── .gitignore
├── .env.example                      # Template secrets (API keys)
├── bmad-bot.yaml.example             # Template config (committed)
├── src/
│   ├── main.rs                       # Entry point, CLI dispatch, signal handling
│   ├── cli/
│   │   └── mod.rs                    # clap: init, start, status, logs
│   ├── config/
│   │   └── mod.rs                    # BotConfig, YAML + .env loading, validation
│   ├── watcher/
│   │   ├── mod.rs                    # Polling loop, sprint-status.yaml reader
│   │   └── deps.rs                   # Dependency graph, pre-gate logic
│   ├── session/
│   │   ├── mod.rs                    # rig agent setup, chat loop, lifecycle
│   │   └── state.rs                  # Session WAL file persistence
│   ├── supervisor/
│   │   ├── mod.rs                    # ask_supervisor Tool implementation
│   │   ├── rules.rs                  # Rule engine (deterministic patterns)
│   │   └── decisions.rs              # Decision logging (file + PR section)
│   ├── review/
│   │   └── mod.rs                    # Code review session (separate LLM, optional)
│   ├── tools/
│   │   ├── mod.rs                    # Tool registration helpers
│   │   ├── git.rs                    # Git Tool (git2 operations)
│   │   ├── fs.rs                     # Filesystem Tool (read/write)
│   │   └── terminal.rs              # Terminal Tool (run commands)
│   ├── git_provider/
│   │   ├── mod.rs                    # GitProvider trait + factory
│   │   ├── github.rs                # GitHub impl (octocrab)
│   │   └── gitlab.rs                # GitLab impl (reqwest)
│   └── notifier/
│       └── mod.rs                    # Notifier trait + Telegram impl
└── tests/
    └── e2e/
        └── mod.rs                    # E2E tests (gated behind BMAD_E2E=1)
```

### Requirements to Structure Mapping

| FRs | Domain | Module | Key Files |
|-----|--------|--------|-----------|
| FR1-4 | Story Management | `watcher/` | `mod.rs` (polling), `deps.rs` (pre-gate) |
| FR5-7 | Pre-Dev Preparation | *BMAD Agent* | Handled by agent via tools — no daemon code |
| FR8-11 | Development Session | `session/` | `mod.rs` (chat loop), `state.rs` (WAL) |
| FR12-17 | Supervision | `supervisor/` | `mod.rs` (tool), `rules.rs`, `decisions.rs` |
| FR18-20 | Code Review | `review/` | `mod.rs` |
| FR21-24 | PR Management | `git_provider/` | `mod.rs` (trait), `github.rs`, `gitlab.rs` |
| FR25-26 | Notifications | `notifier/` | `mod.rs` |
| FR27-32 | CLI & Config | `cli/`, `config/` | `mod.rs` each |
| FR33-36 | Resilience | *Cross-cutting* | reqwest-middleware + per-module error handling |

### Architectural Boundaries

**Module Communication Map:**

```
                ┌──────────┐
                │  config/  │ Arc<BotConfig> shared to all modules
                └────┬─────┘
                     │
┌────────┐    ┌──────┴──────┐    ┌────────────┐
│  cli/  │───▶│   watcher/  │───▶│  session/   │
└────────┘    │  deps.rs    │    │  state.rs   │
              └─────────────┘    └──┬───┬───┬──┘
                                    │   │   │
                    ┌───────────────┘   │   └──────────────┐
                    ▼                   ▼                   ▼
             ┌────────────┐    ┌──────────────┐    ┌──────────────┐
             │   tools/    │    │  supervisor/  │    │   review/    │
             │ git/fs/term │    │ rules/decisions│   │ (optional)   │
             └────────────┘    └──────────────┘    └──────┬───────┘
                                                          │
                                    ┌─────────────────────┤
                                    ▼                     ▼
                             ┌──────────────┐    ┌──────────────┐
                             │ git_provider/ │    │  notifier/   │
                             │ github/gitlab │    │  telegram    │
                             └──────────────┘    └──────────────┘
```

**Interface contracts between modules:**

- **watcher → session:** Passes `StoryInfo` struct (eligible story with metadata: id, label, branch name, specs path, dependencies)
- **session → tools:** Tools registered at agent build time via `.tool()` — no direct calls from session to tools
- **session → supervisor:** Supervisor is a rig tool called by the agent autonomously, not by the daemon
- **session → review:** Passes `StoryInfo` (story_key, branch_name, specs_path). `ReviewRunner` loads the same BMAD dev persona (`dev.md`), sends `"CR"` as initial command, `ResponseAnalyzer` handles interaction patterns (story selection, fix decisions, completion detection), post-review phase captures agent commit + markdown report in `ReviewOutcome::Completed { report }`, orchestrator posts report as PR comment via `GitProvider::add_comment()`
- **session → git_provider:** Passes `CreatePrParams` after session/review complete
- **session → notifier:** Passes `NotificationData` (status, story info, PR link if available, error details if any)
- **config → all:** `Arc<BotConfig>` injected at startup — read-only, never mutated

### Data Flow

1. **Startup:** `config/` loads and validates `bmad-bot.yaml` + `.env` → `Arc<BotConfig>`
2. **Crash check:** `session/state.rs` checks for existing WAL file → if found, resume interrupted session (skip to step 4)
3. **Poll:** `watcher/` reads `sprint-status.yaml` from configured output path → `deps.rs` computes pre-gate → eligible story or sleep until next cycle
4. **Session init:** `session/` loads BMAD dev agent file, appends language override, builds rig agent with 4 tools (git, fs, terminal, ask_supervisor)
5. **Chat loop:** Sends `"DS"` → agent works autonomously via tools → `state.rs` persists chat history after each turn
6. **During session:** Agent calls `ask_supervisor` tool as needed → rule engine → LLM fallback → or escalation (stops session)
7. **Session end:** If `code_review_enabled`, `review/ReviewRunner` launches a new rig agent session with the review LLM config, loads the same BMAD dev persona (`dev.md`), and sends `"CR"` as the initial command. The agent drives the full CR workflow autonomously (diff analysis, adversarial review, fix application). `ResponseAnalyzer` handles all interaction patterns (story selection replies, fix decisions, completion detection). On CR completion, the daemon sends a post-review message asking the agent to commit fixes with descriptive messages and produce a markdown review report. The report is captured in `ReviewOutcome::Completed { report }` and later posted as a PR comment by the orchestrator via `GitProvider::add_comment()`. Review failures are non-blocking — `ReviewOutcome::Failed` proceeds to PR creation with a note in the description
8. **PR creation:** `git_provider/` creates PR (GitHub or GitLab) with agent-written description + Supervisor Decisions section
9. **Notification:** `notifier/` sends Telegram message with story status + PR link
10. **Cleanup:** `session/state.rs` deletes WAL file → return to step 3

### External Integration Points

| Integration | Module | Protocol | Auth |
|-------------|--------|----------|------|
| LLM Providers (Anthropic, OpenAI) | `session/`, `supervisor/`, `review/` | HTTPS via rig-core | API key from `.env` |
| LLM Provider (GitHub Copilot) | `session/`, `supervisor/`, `review/`, `auth/` | HTTPS via rig-core + reqwest | OAuth token from `.env` → exchanged at runtime for short-lived Copilot session token via `GET https://api.github.com/copilot_internal/v2/token`; base URL derived dynamically from token `proxy-ep` field; default: `https://api.individual.githubcopilot.com` |
| GitHub API | `git_provider/github.rs` | HTTPS via octocrab | Token from `.env` |
| GitLab API | `git_provider/gitlab.rs` | HTTPS via reqwest | Token from `.env` |
| Telegram API | `notifier/mod.rs` | HTTPS via reqwest | Bot token from `.env` |
| Local git repo | `tools/git.rs` | libgit2 via git2 | SSH key or credential helper |
| Local filesystem | `tools/fs.rs` | std::fs / tokio::fs | OS permissions |
| Local terminal | `tools/terminal.rs` | tokio::process | OS permissions |
| BMAD config | `config/mod.rs` | File read (YAML) | Filesystem access |
| sprint-status.yaml | `watcher/mod.rs` | File read (YAML) | Filesystem access |

### Configuration Files

| File | Committed | Purpose |
|------|-----------|---------|
| `bmad-bot.yaml` | ✅ Yes | Project config: polling interval, LLM providers/models, git provider, notification config, BMAD paths |
| `.env` | ❌ No (gitignored) | Secrets: API keys, tokens, credentials |
| `bmad-bot.yaml.example` | ✅ Yes | Template for new users |
| `.env.example` | ✅ Yes | Template listing required env vars (no values) |
| `_bmad-output/implementation-artifacts/.bmad-bot-session.yaml` | ❌ No (transient) | Session WAL file — exists only during active session |

## Architecture Validation Results

### Coherence Validation ✅

**Decision Compatibility:**
All architectural decisions work together without conflicts:
- rig-core v0.29.0 + tokio + git2 + reqwest + tracing — no dependency conflicts, all async-compatible
- Hybrid supervisor model (chat loop + ask_supervisor tool) aligns naturally with rig's `chat()` API and `Tool` trait
- Daemon-reads/agent-writes model is consistent with "BMAD files are sacred" principle and "daemon as minimal orchestrator"
- Session WAL file + crash recovery is consistent with graceful shutdown requirements
- Three-tier error propagation (middleware → tool → session) aligns with module boundaries
- Agent prompt composition (load file as-is + append override) is consistent with "daemon knows nothing about BMAD internals"

**Pattern Consistency:**
- All patterns use thiserror per module, anyhow only in binary — consistent error handling across all modules
- All rig tools follow the same structural template (struct + args + error + Tool impl)
- Tracing patterns use story_id spans consistently across the pipeline
- Config shared as Arc<BotConfig> everywhere — single pattern, no exceptions

**Structure Alignment:**
- Project structure directly maps to architectural decisions: one module per FR domain
- Module boundaries match the communication diagram — no circular dependencies
- Integration points are clearly defined at module interfaces with dedicated structs

### Requirements Coverage Validation ✅

**Functional Requirements: 36/36 covered**

| FR Range | Domain | Architectural Support |
|----------|--------|----------------------|
| FR1-4 | Story Management | `watcher/` (polling + pre-gate dependency check). Agent handles status mutations via tools. |
| FR5-7 | Pre-Dev Preparation | BMAD agent autonomously reads prior stories and updates specs via filesystem tool. No daemon code needed. |
| FR8-11 | Development Session | `session/` builds rig agent from BMAD agent file, registers 4 tools, manages chat loop. Language override appended to preamble. |
| FR12-17 | Supervision | `supervisor/` implements ask_supervisor as rig Tool. Rule engine in `rules.rs`, LLM fallback in `mod.rs`, decision logging in `decisions.rs`. Escalation returns tool error → stops rig loop. |
| FR18-20 | Code Review | `review/` launches separate LLM session. Configurable (enabled/disabled). Fixes in separate commits, review posted as PR comment. |
| FR21-24 | PR Management | `git_provider/` trait with GitHub (octocrab) and GitLab (reqwest) implementations. PR created even for failed/blocked stories with failure description. |
| FR25-26 | Notifications | `notifier/` sends Telegram messages with story ID, status, and PR link. Non-blocking — failures logged but don't stop pipeline. |
| FR27-32 | CLI & Config | `cli/` implements 4 clap subcommands. `config/` loads YAML + .env, validates at startup, auto-discovers BMAD version. |
| FR33-36 | Resilience | reqwest-middleware for HTTP retry/backoff (max 3). tokio::signal for graceful shutdown. Session WAL for crash recovery. Notifier for blocking error alerts. |

**Non-Functional Requirements: All covered**

| NFR | Coverage |
|-----|----------|
| Security | Secrets in `.env` only (dotenvy), never in committed config, never logged. Tracing filters sensitive fields. Git credentials from environment. |
| Integration | LLM providers via rig-core (20+ providers supported). GitHub via octocrab. GitLab via reqwest. Telegram via reqwest. All with retry middleware. |
| Reliability | Exponential backoff (max 3 retries) via reqwest-middleware. Graceful shutdown via tokio::signal. Crash recovery via session WAL file. All errors logged with full context. |
| Scalability | MVP: single daemon, sequential execution. Architecture does not preclude future parallelization — modules are independent, config is Arc-shared, no global mutable state. |

### Implementation Readiness Validation ✅

**Decision Completeness:**
- All 6 critical/important decisions documented with rationale and implementation guidance
- Technology versions verified (rig-core 0.29.0, Rust edition 2024)
- Implementation patterns include code examples for all 6 pattern categories
- Anti-patterns explicitly listed to prevent common mistakes

**Structure Completeness:**
- Complete directory tree with every file and its purpose
- All 36 FRs mapped to specific modules and files
- Module communication diagram with interface contracts
- Full data flow documented (10 steps from startup to cleanup)

**Pattern Completeness:**
- Error handling: per-module thiserror + anyhow in binary only
- Tool implementation: standard struct/args/error/trait template
- Tracing: structured spans with story_id context
- Config: validate once, share via Arc
- Git provider: trait with dedicated param/return structs
- Testing: mocked LLM responses, Arrange-Act-Assert, naming convention

### Gap Analysis Results

| # | Priority | Gap | Resolution |
|---|----------|-----|------------|
| 1 | Minor | `review/` module needs diff access — `ReviewContext` struct should include branch name for git2 diff computation | Implementation detail — resolved when coding `review/mod.rs` |
| 2 | Minor | Supervisor LLM fallback needs project docs as context — source paths come from `BotConfig` (planning_artifacts, project_knowledge) | Implementation detail — supervisor reads paths from config |
| 3 | Minor | Exact `bmad-bot.yaml` field schema not specified | Normal for architecture stage — defined during `config/` implementation |

**No critical or blocking gaps found.**

### Architecture Completeness Checklist

**✅ Requirements Analysis**
- [x] Project context thoroughly analyzed (36 FRs, 4 NFR categories)
- [x] Scale and complexity assessed (medium, CLI daemon)
- [x] Technical constraints identified (rig maturity, git2, BMAD read-only)
- [x] Cross-cutting concerns mapped (errors, logging, secrets, LLM abstraction, traceability)

**✅ Architectural Decisions**
- [x] 6 critical/important decisions documented with rationale
- [x] Technology stack fully specified with versions
- [x] Integration patterns defined (chat loop + supervisor tool)
- [x] Crash recovery and resilience addressed

**✅ Implementation Patterns**
- [x] Error type pattern established (thiserror per module)
- [x] Rig tool pattern standardized
- [x] Tracing pattern with structured spans
- [x] Config, Git provider, and test mock patterns defined
- [x] Anti-patterns explicitly documented

**✅ Project Structure**
- [x] Complete directory structure with all files
- [x] Module boundaries and communication map
- [x] FR-to-module mapping (36/36)
- [x] Data flow documented (10 steps)
- [x] External integration points catalogued

### Architecture Readiness Assessment

**Overall Status:** ✅ READY FOR IMPLEMENTATION

**Confidence Level:** High — all requirements covered, no blocking gaps, decisions are coherent and well-documented.

**Key Strengths:**
- Simple, modular architecture that aligns with rig's design philosophy
- Daemon stays minimal — the BMAD agent does the heavy lifting
- Crash recovery built-in from day one (session WAL)
- Full decision traceability (supervisor decisions → file + PR)
- Two-layer dependency model (daemon pre-gate + agent verification) saves tokens
- GitProvider trait enables GitHub + GitLab from MVP

**Areas for Future Enhancement:**
- Multi-worker orchestration with story parallelization (v2)
- Web dashboard for monitoring runs and history (v3)
- Plugin system for custom tools and notification providers (v3)
- Self-improving supervisor rules based on recurring patterns (v3)
- Code integrity safeguards: static analysis, sandboxed execution (v3)

### Implementation Handoff

**AI Agent Guidelines:**
- Read the Project Context file (`_bmad-output/project-context.md`) before implementing any code
- Follow all architectural decisions exactly as documented in this file
- Use implementation patterns consistently — especially error types, tool structure, and tracing
- Respect module boundaries — each module owns its error types and exposes clean interfaces
- Check the anti-patterns list before submitting any code

**First Implementation Priority:**
`cargo init bmad-bot` + dependency setup + module scaffolding. This should be the first implementation story, establishing the project skeleton that all subsequent stories build on.