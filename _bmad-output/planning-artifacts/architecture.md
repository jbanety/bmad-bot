---
stepsCompleted: [1, 2, 3, 4, 5, 6, 7, 8]
inputDocuments: ['_bmad-output/planning-artifacts/prd.md', '_bmad-output/project-context.md']
workflowType: 'architecture'
project_name: 'bmad-bot'
user_name: 'JB'
date: '2026-02-10'
lastStep: 8
status: 'complete'
completedAt: '2026-02-07'
revisedAt: '2026-02-15'
revisionNote: 'Post-Implementation Impact Analysis step added to session runner post-completion sequence. New Step 8 between final commit and PR summary — agent evaluates downstream dependent stories and updates their Previous Story Intelligence sections with actual implementation details. Best-effort, non-blocking, agent-driven. Single file change in runner.rs (~50 lines). Triggered by story 7-1 completing without propagating implementation reality to dependent stories 7-2 through 7-10.'
---

# Architecture Decision Document

_This document builds collaboratively through step-by-step discovery. Sections are appended as we work through each architectural decision together._

## Project Context Analysis

### Requirements Overview

**Functional Requirements:**
38 FRs across 9 domains. The system is a pipeline daemon: watcher → pre-gate → session → supervision → review → PR → notification. Each domain maps cleanly to an architectural module. The supervisor (FR12-17) is the most complex component, combining a deterministic rule engine, LLM fallback, and full decision traceability.

**Non-Functional Requirements:**
- Security: Secrets never in committed config or logs. Environment-variable-only secrets management.
- Reliability: Exponential backoff (max 3 retries) for transient LLM errors. Cooperative shutdown with partial commit on SIGTERM/SIGINT via shared ShutdownFlag. Crash recovery produces clean state.
- Integration: GitHub API with rate limit handling, Telegram notifications (non-blocking), multi-provider LLM support.
- Scalability: MVP is single-daemon sequential execution. Architecture must not preclude future parallelization (multi-worker, story-level concurrency).

**Scale & Complexity:**
- Primary domain: Backend CLI daemon / Developer tool
- Complexity level: Medium
- Estimated architectural components: 8 core modules (cli, config, watcher, session, supervisor, review, tools, notifier) + 4 support modules (auth, pipeline, llm_context, llm_logging)

### Technical Constraints & Dependencies

- **rig-core maturity:** Core dependency for agent orchestration. Evaluate early — fallback is direct LLM provider API calls.
- **Git CLI (>= 2.30):** All git operations via subprocess (`tokio::process::Command` / `std::process::Command`). Requires `git` installed on host — acceptable for a developer tool targeting machines that already have git. Inherits user's full git configuration (credential managers, commit signing, SSH agent, `.gitconfig` identity). Replaces the previous `git2` (libgit2) embedded approach, which could not access the user's SSH agent in daemon context and ignored user git config.
- **LLM provider variability:** Three providers may behave differently (rate limits, response formats, error codes). Abstraction layer required. GitHub Copilot requires streaming (`stream: true`) and IDE-specific headers.
- **BMAD files are read-only:** The daemon never modifies anything under `_bmad/`. All output goes to `_bmad-output/`.
- **Sequential execution in MVP:** Simplifies architecture significantly — no concurrency primitives needed for story processing.

### Cross-Cutting Concerns Identified

1. **Error handling & resilience** — Every component must handle failures gracefully, log with full context, and propagate to notification when blocking.
2. **Structured logging with story context** — `tracing` spans with `story_id` across the entire pipeline for debuggability. Dedicated `llm_logging` module for LLM request/response payloads.
3. **LLM provider abstraction** — Three independent roles (dev, review, supervisor) each configurable with different providers. Shared retry/backoff logic. Provider-specific API differences (Responses API vs Completions API) handled in `session/provider.rs`.
4. **Decision traceability** — Supervisor decisions flow from rule engine/LLM through to decisions file and PR description. End-to-end audit trail.
5. **Secret management** — Filtering in logs, separation in config, environment-variable-only loading. Applies to all components that touch credentials.
6. **Cooperative shutdown** — Shared `ShutdownFlag` (`Arc<AtomicBool>`) propagated across pipeline → session → streaming chat layers for clean interruption at any depth.

## Starter Template Evaluation

### Primary Technology Domain

Rust CLI daemon / Developer tool — long-running autonomous process with CLI interface for setup and monitoring.

### Starter Options Considered

No traditional starter template applies. Rust daemon projects start from `cargo init` with deliberate dependency selection. The technology stack is fully defined in the Project Context.

### Selected Approach: cargo init + curated dependencies

**Rationale:**
Rust ecosystem does not have opinionated starter templates like web frameworks. The Project Context already locks the core stack (tokio, rig-core, serde, tracing, Git CLI). The remaining foundation decisions are CLI framework, config loading, Git provider abstraction, and signal handling.

**Initialization Command:**

```
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

**Signal Handling — Cooperative Shutdown:**
- Cooperative shutdown via a shared `ShutdownFlag` (`Arc<AtomicBool>`) created in `run_start()`
- A dedicated signal handler task (spawned via `tokio::spawn`) listens for Ctrl+C (`SIGINT`) and `SIGTERM`, then flips the flag
- The flag is propagated across **pipeline → session → streaming_chat** layers
- `streaming_chat()` checks the flag between every stream chunk and tool-call round — enabling interruption **mid-streaming and mid-tool-call loops**
- `run_session()` checks between chat turns and saves WAL before returning
- `run_polling_loop()` checks at the top of each poll cycle
- This replaces the earlier inline `tokio::select!` signal branches with a more composable, testable pattern

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
│   ├── auth/
│   │   ├── mod.rs
│   │   └── github_copilot.rs         # Device flow + token exchange + cache
│   ├── cli/
│   │   ├── mod.rs
│   │   ├── git_detect.rs              # Git remote auto-detection for init
│   │   └── state.rs                   # Daemon state file (.bmad-bot-state.json)
│   ├── config/
│   │   ├── mod.rs
│   │   └── discovery.rs               # BMAD version/module auto-discovery
│   ├── watcher/
│   │   ├── mod.rs
│   │   └── deps.rs
│   ├── session/
│   │   ├── mod.rs
│   │   ├── analyzer.rs                # Response analysis (workflow interactions)
│   │   ├── branch.rs                  # Branch management (create/checkout)
│   │   ├── cleanup.rs                 # Session cleanup (partial work, needs-clarification)
│   │   ├── escalation.rs              # Escalation report handling
│   │   ├── provider.rs                # LLM provider construction + Copilot headers
│   │   ├── runner.rs                  # Main session runner (chat loop, activation, recovery)
│   │   └── state.rs                   # Session WAL file persistence
│   ├── supervisor/
│   │   ├── mod.rs
│   │   ├── architect.rs               # Architect LLM fallback session
│   │   ├── read_tool.rs               # Read-only file tool for Architect
│   │   ├── rules.rs
│   │   └── decisions.rs
│   ├── review/
│   │   └── mod.rs
│   ├── tools/
│   │   ├── mod.rs
│   │   ├── git.rs                     # GitTool — git operations via Git CLI subprocess
│   │   ├── fs.rs
│   │   └── terminal.rs
│   ├── git_provider/
│   │   ├── mod.rs
│   │   ├── github.rs
│   │   └── gitlab.rs
│   ├── notifier/
│   │   └── mod.rs
│   ├── llm_context.rs                 # Zed-style XML ContextBuilder
│   ├── llm_logging.rs                 # LLM request/response debug logging
│   └── pipeline.rs                    # Pipeline orchestration (watcher → session → review → PR → notify)
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
5. Agent prompt composition — How the BMAD agent is loaded and activated
6. Deployment model — How the daemon runs as a process

**Deferred Decisions (Post-MVP):**
- Multi-worker orchestration (v2/v3)
- CI/CD pipeline setup
- Web dashboard architecture
- Plugin system design

### Decision 1: Supervisor Interception Model — Hybrid Chat Loop + Supervisor Tool

**Decision:** Combine an external chat loop with an internal `ask_supervisor` rig tool.

**Rationale:**
rig exposes streaming and non-streaming APIs. Both handle tool-calling internally — there is no hook or callback to intercept individual turns. This rules out proxy-based or hook-based interception.

The hybrid approach uses both rig interaction patterns for their natural strengths:

**Chat loop (external)** — The daemon controls the session via `streaming_chat(agent, prompt, history)` in a loop. This replaces the human sitting at the terminal. When the agent returns text (end of a turn), the daemon analyzes it for workflow interaction points (confirmations, "should I proceed?", step transitions) and responds automatically. This handles the BMAD workflow's natural conversation flow.

**`ask_supervisor` tool (internal)** — Registered as a standard rig tool alongside git/fs/terminal/think. When the agent has a substantive question or doubt *during* tool-calling work, it calls `ask_supervisor`. Inside the tool's `call()` method:
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
6. Resume `streaming_chat()` with loaded history — agent has full context and continues
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
8. Resume `streaming_chat()` — the agent picks up the current task with full awareness of prior work

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
- Fatal errors (config invalid, all providers down after retries, SIGTERM) → cooperative shutdown: save WAL, commit if possible, notify, exit
- Notification failures are non-blocking at all layers — logged but never stop story processing

**Affects:** all modules (cross-cutting)

### Decision 5: Agent Prompt Composition — XML Context Activation

**Decision:** The BMAD dev agent file is **not** used as the system preamble. Instead, a minimal system preamble provides operational instructions, and the agent file is sent as the **first user message** wrapped in Zed-style XML context tags. The agent then executes its own activation steps via tools before receiving commands.

**Rationale:**
The Zed-style XML context format (`<context><files>...</files></context>`) is the standard way LLMs trained on Zed-context (Claude, GPT, etc.) interpret attached files as actionable context. This approach:

1. Keeps the system preamble lean and stable (operational rules only) — it persists across all turns as grounding
2. Lets the full BMAD agent definition flow through the normal conversation, where the LLM naturally processes activation instructions
3. Leverages the `llm_context` module's `ContextBuilder` for adaptive backtick fencing, absolute path resolution, and multi-file support
4. Creates a clean separation: **system prompt = how to behave as a tool-using agent** vs **user message = which persona to embody and what workflow to follow**

**Implementation:**

```
// 1. System preamble — minimal operational instructions
fn build_preamble() -> String {
    r#"You are an AI agent operating autonomously in a BMAD workflow environment.
## Tools
You have access to these tools: git, filesystem, terminal, ask_supervisor.
## Communication
OVERRIDE: communication_language = English
## Rules
- When the user provides an agent file in <context><files> tags, you MUST fully
  embody that agent's persona and follow ALL activation instructions.
- Execute activation steps in order — load configuration files via tools, then
  greet and display the menu.
- Wait for user input after displaying the menu."#
}

// 2. Agent activation — send dev.md as first user message in XML context
async fn activate_agent(agent: &Agent, label: &str) -> (Vec<Message>, Vec<ChatMessage>) {
    let activation_msg = ContextBuilder::new()
        .add_file_from_disk(&agent_path)?  // resolves to absolute path
        .build();                           // wraps in <context><files>...</files></context>

    let response = streaming_chat(agent, &activation_msg, vec![], Some(&shutdown)).await?;

    // Agent processes activation: loads config.yaml via tools, shows greeting/menu
    // Returns (rig_history, chat_history) for subsequent turns
}

// 3. Build agent with 5 tools (4 custom + 1 built-in)
let agent = client
    .agent(model)
    .preamble(&preamble)
    .tool(git_tool)
    .tool(fs_tool)
    .tool(terminal_tool)
    .tool(ask_supervisor)
    .tool(ThinkTool)        // rig's built-in think tool
    .build();

// 4. After activation, send "DS" to trigger dev-story workflow
let response = streaming_chat(agent, "DS", rig_history, Some(&shutdown)).await?;
```

**Activation phase:** After `activate_agent()` returns, the agent has:
- Read `config.yaml` via filesystem tool
- Stored session variables ({user_name}, {communication_language}, etc.)
- Displayed its greeting and menu
- Is ready to accept commands

**First command:** `"DS"` (triggers the dev-story workflow as defined in the BMAD agent's menu system).

**Key principle:** The daemon has an explicit activation phase before sending commands. The system preamble provides persistent behavioral grounding, while the agent persona flows through user-message context — keeping the two concerns cleanly separated.

**Affects:** session module, llm_context module

### Decision 6: Deployment Model — Foreground Process

**Decision:** `bmad-bot start` runs as a simple foreground process. No self-daemonization.

**Rationale:**
This is a developer tool, not infrastructure software. Users can background it with standard OS tools (`tmux`, `screen`, `nohup`, `systemd`, `launchd`). Adding daemonization (fork, PID files, log rotation) is unnecessary complexity for the MVP.

**Behavior:**
- Logs to stdout/stderr via `tracing` (structured JSON or pretty-print based on config)
- SIGTERM/SIGINT triggers cooperative shutdown via ShutdownFlag
- No PID file, no log file management, no auto-restart
- Future (v2): could add `--daemon` flag or provide example systemd/launchd service files

**Affects:** cli module, main

### Decision 7: Surgical Development Tooling — Focused Tools Modeled on Claude Code/Zed

> **Amendment (2026-02-11) — GitTool: git2 → Git CLI migration**
>
> The `GitTool` (row 6 in the table below) now uses **Git CLI subprocess calls** instead of `git2` (libgit2). A production incident (2026-02-10) revealed that the daemon cannot access the user's SSH agent when running as a background process, causing `git2` push operations to fail. Additionally, `git2` ignores user git configuration (commit signing, credential managers, `.gitconfig` identity). The migration applies to all three git-using components: `tools/git.rs` (agent-facing), `session/branch.rs` (daemon branch management), and `pipeline.rs` (push). The `git2` crate is fully removed from `Cargo.toml`. See architect brief `architect-brief-git-cli-migration.md` for full rationale and scope.

**Decision:** Replace the monolithic `FsTool` (read/write/list/mkdir/delete/exists) with **5 focused tools** — `EditFileTool`, `ReadFileTool`, `GrepTool`, `FindPathTool`, `ListDirectoryTool` — bringing the total agent toolset from 5 to 9 tools. The system preamble is expanded with detailed tool usage rules.

**Problem Statement:**
The current `FsTool` with its `write` action requires the agent to regenerate the **entire file content** for every edit. On a 500-line file, this burns ~8000 tokens per edit (read + full rewrite), risks truncation/code loss by the LLM, and is fundamentally incompatible with surgical development. Claude Code, Zed agent mode, and Cursor all use targeted edit primitives — search-and-replace on exact text fragments — which is the proven pattern for LLM-driven code editing.

**Rationale:**
- **Token efficiency:** A search_replace edit on a 500-line file costs ~900 tokens (outline + targeted read + delta) vs ~8000 tokens for full rewrite — **~8x reduction**
- **Code safety:** Only the changed fragment is touched; the rest of the file is never at risk of truncation or accidental modification
- **Navigation efficiency:** `GrepTool` and `FindPathTool` eliminate blind `list` → `read` loops; the agent finds code in 1-2 calls instead of 5-10
- **Proven pattern:** This is exactly how Claude Code, Zed, Aider, and Cursor implement file editing — it's the industry-standard approach for LLM agents
- **Separate tools > action multiplexing:** Each tool gets a focused JSON schema and description. LLMs reason better with small, clear tool interfaces than with one mega-tool that has 10+ actions

**New Tool Inventory (9 tools total):**

| # | Tool | Replaces | Purpose |
|---|------|----------|---------|
| 1 | **EditFileTool** | FsTool `write` | Surgical edits via search_replace, create new files, overwrite when justified |
| 2 | **ReadFileTool** | FsTool `read` | Read with optional line range (`start_line`/`end_line`) + automatic outline mode for large files |
| 3 | **GrepTool** | _(new)_ | Regex search across project file contents with glob filtering and pagination |
| 4 | **FindPathTool** | _(new)_ | Glob-based file path discovery with pagination |
| 5 | **ListDirectoryTool** | FsTool `list` | List directory contents with types and sizes |
| 6 | **GitTool** | _(unchanged)_ | Git operations via Git CLI subprocess (`tokio::process::Command`) |
| 7 | **TerminalTool** | _(unchanged)_ | Shell command execution with timeout |
| 8 | **AskSupervisor** | _(unchanged)_ | Supervisor question tool |
| 9 | **ThinkTool** | _(unchanged)_ | rig built-in reasoning tool |

**Removed actions:** FsTool `mkdir`, `delete`, `exists` — the agent uses `TerminalTool` for these infrequent operations (`mkdir -p`, `rm`, test with `ls`). `EditFileTool` in `create` mode auto-creates parent directories.

**EditFileTool Design:**

```
EditFileArgs {
    path: String,           // Relative path from project root
    mode: String,           // "edit", "create", "overwrite"
    // For mode="edit": list of search-replace operations
    edits: Option<Vec<EditOperation>>,
    // For mode="create" or "overwrite": full file content
    content: Option<String>,
}

EditOperation {
    old_text: String,       // Exact text fragment to find in the file
    new_text: String,       // Replacement text
}
```

Validation rules:
- `old_text` must exist in the file and be **unique** (exactly one match). If zero matches → error with "not found" + nearby candidates. If multiple matches → error with line numbers of all occurrences so the agent can provide more context.
- `create` mode fails if the file already exists (forces the agent to use `edit` for existing files)
- `overwrite` mode requires the file to already exist
- Multiple `EditOperation` items are applied sequentially within a single call — offsets are recalculated after each edit
- Return value includes the line range affected by each edit for verification

**ReadFileTool Design:**

```
ReadFileArgs {
    path: String,                  // Relative path from project root
    start_line: Option<u32>,       // 1-indexed, inclusive
    end_line: Option<u32>,         // 1-indexed, inclusive
}
```

Behavior:
- If file is **≤ 300 lines** and no line range specified → return full content with line numbers
- If file is **> 300 lines** and no line range specified → return **outline mode**: extract structural symbols (Rust: `fn`, `struct`, `enum`, `impl`, `mod`, `trait`, `pub`, `///` doc comments) with their line numbers. The agent then uses line ranges to read specific sections.
- Outline extraction uses language-aware regex heuristics, not a full AST parser. For Rust, patterns like `^\s*(pub\s+)?(async\s+)?fn\s+`, `^\s*(pub\s+)?struct\s+`, `^\s*(pub\s+)?enum\s+`, `^\s*impl\s+`, `^\s*mod\s+`, `^\s*#\[cfg\(test\)\]` are sufficient to capture 90%+ of navigable symbols.
- Line numbers are always included in output (both full content and outline mode) to enable precise `EditFileTool` usage.

**GrepTool Design:**

```
GrepToolArgs {
    regex: String,                    // Regex pattern (Rust `regex` crate syntax)
    include_pattern: Option<String>,  // Glob filter (e.g., "src/**/*.rs")
    context_lines: Option<u32>,       // Lines of context before/after each match (default: 2)
    max_results: Option<u32>,         // Pagination limit (default: 20)
}
```

Implementation: Uses `grep -rn --include` via `TerminalTool` internally (or the `grep` crate for pure Rust), with structured output parsing. Returns matches as `{path, line_number, content, context_before, context_after}`.

**FindPathTool Design:**

```
FindPathToolArgs {
    glob: String,                     // Glob pattern (e.g., "**/*.rs", "src/**/mod.rs")
    max_results: Option<u32>,         // Pagination limit (default: 50)
}
```

Implementation: Uses the `glob` crate or `walkdir` + pattern matching. Respects `.gitignore` patterns. Returns sorted list of matching paths relative to project root.

**Expanded System Preamble — Tool Usage Rules:**

The `build_preamble()` method in `session/runner.rs` is updated to include explicit tool usage guidance. This is critical — the tools alone are not enough; the agent must be **instructed** on when and how to use each one.

```
## Tools
You have access to these tools: edit_file, read_file, grep, find_path, list_directory,
git, terminal, ask_supervisor, plus a built-in think tool for reasoning.

## Tool Usage Rules
- **ALWAYS use `edit_file` with mode="edit"** to modify existing files. NEVER rewrite
  entire files unless creating a new file (mode="create") or a complete rewrite is
  truly necessary (mode="overwrite").
- **Use `read_file` with line ranges** for large files. Read the outline first, then
  target specific sections with start_line/end_line.
- **Use `grep` to find symbols** before editing — never assume file paths or line numbers.
- **Use `find_path`** to discover files by name pattern when you don't know the full path.
- **Use `list_directory`** to explore directory structure.
- **Use `terminal`** for build commands, tests, mkdir, rm, and other shell operations.
- When `edit_file` fails (ambiguous match), use `read_file` with a line range to get
  more context, then retry with a larger `old_text` fragment.
- When making multiple related changes in one file, batch them in a single `edit_file`
  call with multiple edit operations.
```

**Migration Path:**
- The existing `FsTool` is removed entirely
- All 5 new tools follow the established Rig Tool Implementation Pattern (struct + args + error + Tool impl)
- The `supervisor/read_tool.rs` (read-only fs tool for the Architect agent) is updated to use `ReadFileTool` instead of `FsTool`
- Session builder (`runner.rs`) registers the 5 new tools instead of `FsTool`
- Unit tests for `FsTool` are replaced with per-tool test suites

**Affects:** tools module (major restructure), session/runner.rs (preamble + tool registration), supervisor/read_tool.rs, review/mod.rs, project-context.md

### Decision 8: LLM Provider Abstraction — BuiltAgent Enum + AgentFactory

> **Added (2026-02-12) — Production incident: gpt-5.2-codex via GitHub Copilot**
>
> A production bug revealed that the Copilot proxy branch unconditionally used the Chat Completions API (`/chat/completions`), but newer OpenAI models like `gpt-5.2-codex` only support the Responses API (`/responses`). This decision centralizes all LLM provider construction and eliminates ~610 lines of duplicated provider match arms across 5 call sites. See architect brief `architect-brief-llm-provider-abstraction.md` for full rationale and scope.

**Decision:** Centralize all LLM provider construction behind a `BuiltAgent` enum with `stream_chat()` dispatch and an `AgentFactory` struct. API format selection is hardcoded per provider/model — not configurable.

**Problem Statement:**
rig-core's `Chat` trait is not object-safe (associated types, `Self: Sized`), so `dyn Chat` is impossible. The codebase used 3-arm match statements (anthropic / openai / copilot) duplicated across `session/runner.rs` (run + resume + 3 build methods), `review/mod.rs`, and `supervisor/architect.rs` — ~610 lines of near-identical provider-specific code. Adding a provider or fixing a provider quirk required changes in every match site.

**Rationale:**
- **Enum dispatch** is the idiomatic Rust pattern when trait objects are unavailable — one match per `stream_chat()` call, negligible overhead vs seconds-long LLM calls
- **Single construction site** — provider selection, API key resolution, Copilot token exchange, and API format detection happen once in `AgentFactory::build()`
- **Hardcoded API format** — the API format is a deterministic property of the provider behind the model, not a user preference:
  - **Anthropic** → Messages API (always)
  - **OpenAI direct** → Responses API (always, rig default)
  - **GitHub Copilot** → proxy to multiple backends. Explicit match on known OpenAI model families (`gpt-*`, `o1-*`, `o3-*`, `codex`) → Responses API. **Everything else falls back to Completions API** — safe default for non-OpenAI models (Claude, Mistral, etc.). The inverse default would break non-OpenAI models.
- **No config override** — `api_format` in user config would expose an internal implementation detail. When OpenAI introduces new model name patterns, update `copilot_requires_responses_api()` — it's a one-liner

**Core Design:**

```
pub enum BuiltAgent {
    Anthropic(Agent<anthropic::CompletionModel>),
    OpenAiResponses(Agent<openai::responses_api::ResponsesCompletionModel>),
    OpenAiCompletions(Agent<openai::completion::CompletionModel>),
}

impl BuiltAgent {
    pub async fn stream_chat(&self, prompt, history, shutdown) -> Result<String, PromptError> {
        match self { /* delegates to streaming_chat() for each variant */ }
    }
}

pub struct AgentFactory { config, secrets, copilot_cache }

impl AgentFactory {
    pub async fn build(&self, role: LlmRole, preamble: &str, tools: ToolSet)
        -> Result<BuiltAgent, ProviderError>
    {
        match provider {
            "anthropic" => BuiltAgent::Anthropic(..),
            "openai" => BuiltAgent::OpenAiResponses(..),
            "github-copilot" => {
                if copilot_requires_responses_api(model) {
                    BuiltAgent::OpenAiResponses(..)  // OpenAI models via proxy
                } else {
                    BuiltAgent::OpenAiCompletions(..) // fallback: safe for non-OpenAI
                }
            }
        }
    }
}

fn copilot_requires_responses_api(model: &str) -> bool {
    let m = model.to_lowercase();
    m.starts_with("gpt-") || m.starts_with("o1-") || m.starts_with("o3-") || m.contains("codex")
}
```

**Impact:**
- `session/runner.rs` — removes `build_anthropic_agent`, `build_openai_agent`, `build_copilot_agent` and all provider match arms; replaced by single `agent_factory.build(LlmRole::Dev, ..)` call
- `review/mod.rs` — same pattern, `agent_factory.build(LlmRole::Review, ..)`
- `supervisor/architect.rs` — same pattern, `agent_factory.build(LlmRole::Supervisor, ..)`
- `pipeline.rs` — passes `AgentFactory` to `StoryPipeline` instead of individual provider configs
- `session/provider.rs` — absorbed into `AgentFactory` (resolve_api_key, copilot_headers)

**Affects:** llm module (new agent_factory.rs), session module, review module, supervisor module, pipeline

### Decision Impact Analysis

**Implementation Sequence:**
1. Foundation: cargo init, CLI (clap), config loading, cooperative shutdown (ShutdownFlag in `run_start()`, signal handler task, propagation to pipeline/session), git version validation (>= 2.30)
2. Tools: git (Git CLI), edit_file, read_file, grep, find_path, list_directory, terminal as rig Tool traits
3. Watcher: sprint-status.yaml parser, dependency graph, pre-gate logic
4. Session: rig agent setup with XML context activation, streaming chat loop, state file persistence, `llm/context` module for ContextBuilder, `llm/agent_factory` for centralized provider construction
5. Supervisor: ask_supervisor tool, rule engine, LLM fallback (architect session), decision logging
6. Git Provider: GitHub + GitLab PR creation trait + implementations
7. Review: separate LLM session for code review (optional, configurable)
8. Notifier: Telegram integration

**Cross-Component Dependencies:**
- Session depends on: tools (edit_file, read_file, grep, find_path, list_directory, git, terminal), supervisor, config, git_provider, llm/context, llm/logging, llm/agent_factory
- AgentFactory depends on: config (provider/model per role), auth (CopilotTokenCache for Copilot token exchange)
- Pipeline depends on: session, review, git_provider, notifier, config, llm/agent_factory
- Supervisor depends on: config (LLM provider for fallback), decisions logging
- Watcher depends on: config (paths, polling interval)
- Git Provider depends on: config (provider selection, credentials)
- All components depend on: error handling strategy (Layer 1-3), tracing setup, ShutdownFlag (pipeline/session/streaming layers)

## Implementation Patterns & Consistency Rules

### Pattern Categories Defined

**Critical Conflict Points Identified:**
7 areas where AI agents could make different implementation choices. The Project Context already covers Rust conventions (snake_case, rustfmt, clippy, doc comments, module structure, testing placement). The patterns below address bmad-bot-specific concerns not covered there.

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

Every rig tool (edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor) follows the same structural pattern. Note: `GitTool` uses `tokio::process::Command` to invoke the `git` CLI — each action maps to a subprocess call with structured output parsing.

```
// 1. Serializable struct with shared state
#[derive(Deserialize, Serialize)]
pub struct MyTool {
    // Shared config/state needed by the tool (e.g., project_root: PathBuf)
}

// 2. Dedicated args struct — focused on ONE concern
#[derive(Deserialize)]
pub struct MyToolArgs {
    // Tool-specific parameters only — no "action" multiplexer
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
        // CRITICAL: description quality directly impacts agent behavior
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Log via tracing before action
        // Execute
        // Log result or error
        // Return
    }
}
```

**Tool design principle — Focused tools over action multiplexing:**
Each tool owns a single concern with a compact JSON schema. Do NOT add an `action: String` field that multiplexes multiple operations into one tool (this was the anti-pattern of the original `FsTool`). The LLM reasons better with many small, clearly-described tools than with one mega-tool that has a large branching schema.

**Note on ThinkTool:** The 9th agent tool is rig's built-in `ThinkTool` (derived from Anthropic's Claude Think Tool pattern). It gives the agent a dedicated space for structured reasoning without consuming real tool calls. No custom implementation needed — it is imported from the `rig` crate and added via `.tool(ThinkTool)` on the agent builder. It does **not** live in the `tools/` directory.

**Mandatory rules:**
- Tool NAME is always snake_case and descriptive
- Tool definition description must be detailed enough for the LLM to use correctly
- Every `call()` logs the action and result via `tracing`
- Never panic in a tool — always return `Result`
- One tool = one concern — no action multiplexing

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

**LLM Payload Logging (`llm_logging` module):**
All LLM requests and responses are logged via a dedicated `llm_logging` module (`src/llm_logging.rs`) using the `bmad_bot::llm` tracing target. This allows independent filtering:

```
# Enable LLM payload logging
RUST_LOG=bmad_bot::llm=debug cargo run -- start

# Enable full history tracing
RUST_LOG=bmad_bot::llm=trace cargo run -- start
```

Functions: `log_llm_request()`, `log_llm_response()`, `log_llm_error()`, `log_llm_history()`, `log_llm_history_summary()`. Each has an early `tracing::enabled!` guard — zero cost when disabled (~1ns atomic load). Used across session runner, review, and supervisor architect modules.

**Mandatory rules:**
- Every session wrapped in a `story_session` span with `story_id`
- Every tool action logged with `action` field
- Errors always include `error` field with the error value
- Sensitive fields filtered — never log API keys, tokens, or credentials
- All LLM interactions logged via `llm_logging` module, not ad-hoc tracing calls

### Cooperative Shutdown Pattern — ShutdownFlag Propagation

```
// Type alias — shared across modules
pub type ShutdownFlag = Arc<AtomicBool>;

// Created once in run_start()
let shutdown: ShutdownFlag = Arc::new(AtomicBool::new(false));

// Signal handler task flips the flag
{
    let flag = Arc::clone(&shutdown);
    tokio::spawn(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
        flag.store(true, Ordering::Relaxed);
    });
}

// Propagation chain:
// run_start() → StoryPipeline::new(shutdown) → SessionRunner::new(shutdown)
//                                             → streaming_chat(agent, prompt, history, Some(&shutdown))

// Check points:
// 1. run_polling_loop() — top of each poll cycle
// 2. run_session() — between chat turns, saves WAL before returning
// 3. streaming_chat() — between every stream chunk AND between tool-call rounds
```

**Why this matters:** The daemon runs long-lived streaming LLM calls with multi-turn tool-calling loops. A simple `tokio::select!` on the top-level loop would only catch shutdown between stories, not mid-stream. The ShutdownFlag pattern enables interruption at the **finest granularity** — between individual SSE chunks — so Ctrl+C always responds within milliseconds, not minutes.

**Mandatory rules:**
- `ShutdownFlag` is always `Arc<AtomicBool>` — never a channel, never a mutex
- Use `Ordering::Relaxed` for loads and stores — no cross-thread ordering needed, just visibility
- Every function that runs an LLM call must accept `Option<&ShutdownFlag>` and check between chunks
- On shutdown detection: save WAL state, commit partial work if possible, then return cleanly

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

### Git CLI Subprocess Pattern — Working Directory & Error Handling

All git CLI calls follow a consistent pattern for subprocess invocation:

```
// Async context (GitTool, pipeline.rs)
let output = tokio::process::Command::new("git")
    .arg("-C").arg(&self.project_root)   // Always set working directory explicitly
    .args(&["status", "--porcelain"])
    .output()
    .await?;

if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(GitToolError::CommandFailed { cmd: "status", stderr: stderr.into() });
}
let stdout = String::from_utf8_lossy(&output.stdout);

// Sync context (session/branch.rs — called from spawn_blocking or sync init)
let output = std::process::Command::new("git")
    .arg("-C").arg(&project_root)
    .args(&["checkout", "-b", &branch_name])
    .output()?;
```

**Mandatory rules:**
- Always use `-C <path>` or `.current_dir(path)` — never rely on process-level `cwd`
- Capture both stdout and stderr — include stderr in error messages for LLM-readable diagnostics
- Use `--porcelain` flags where available (`status`, `diff`, `log`) for stable, parseable output
- Use `tokio::process::Command` in async contexts (tools, pipeline), `std::process::Command` in sync contexts (branch.rs via `spawn_blocking`)
- Check `output.status.success()` — map non-zero exit codes to the module's thiserror enum

**Startup validation:** The daemon's `run_start()` verifies git availability and minimum version before proceeding:

```
// In cli/mod.rs::run_start(), before entering the polling loop
let output = std::process::Command::new("git").arg("--version").output()?;
// Parse "git version X.Y.Z" → require >= 2.30
// Fail fast with clear error if git missing or too old
```

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
- Log all LLM interactions via the `llm_logging` module
- Pass config as `Arc<BotConfig>` — never clone, never mutate
- Propagate `ShutdownFlag` to any function that runs LLM calls
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
- Using `agent.chat()` (non-streaming) — always use `streaming_chat()` / `stream_chat()`

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
│   ├── main.rs                       # Entry point, CLI dispatch, rustls init
│   ├── lib.rs                        # Library crate — pub mod for all modules (enables integration test imports via bmad_bot::*)
│   ├── auth/
│   │   ├── mod.rs                    # Auth module root
│   │   └── github_copilot.rs         # OAuth Device Flow, token exchange, CopilotTokenCache
│   ├── cli/
│   │   ├── mod.rs                    # clap: init, start, status, logs + run_start/run_polling_loop
│   │   ├── git_detect.rs             # Git remote auto-detection for interactive init
│   │   └── state.rs                  # DaemonState file (.bmad-bot-state.json) for `status` command
│   ├── config/
│   │   ├── mod.rs                    # BotConfig, BotSecrets, YAML + .env loading, validation
│   │   └── discovery.rs              # BMAD version/module auto-discovery from _bmad/ directory
│   ├── watcher/
│   │   ├── mod.rs                    # Polling loop, sprint-status.yaml reader
│   │   └── deps.rs                   # Dependency graph, topological sort, pre-gate logic
│   ├── session/
│   │   ├── mod.rs                    # Session module root, SessionOutcome enum
│   │   ├── analyzer.rs               # ResponseAnalyzer — workflow interaction detection (confirmations, step transitions, completion)
│   │   ├── branch.rs                 # Branch management — create/checkout story branches, determine base branch
│   │   ├── cleanup.rs                # Session cleanup — partial work preservation, needs-clarification marking
│   │   ├── escalation.rs             # EscalationReport — structured escalation handling
│   │   ├── provider.rs               # LLM provider construction, resolve_api_key(), copilot_headers()
│   │   ├── runner.rs                 # SessionRunner — agent build, XML context activation, streaming chat loop, post-impl impact analysis, crash/context-limit recovery
│   │   └── state.rs                  # Session WAL file persistence (ChatMessage, SessionState)
│   ├── supervisor/
│   │   ├── mod.rs                    # ask_supervisor Tool implementation, EscalationSlot
│   │   ├── architect.rs              # Architect LLM fallback session (separate agent for substantive questions)
│   │   ├── read_tool.rs              # Read-only filesystem tool for the Architect agent (uses ReadFileTool)
│   │   ├── rules.rs                  # Rule engine (deterministic pattern matching)
│   │   └── decisions.rs              # Decision logging (DecisionLog, write_decisions_file)
│   ├── review/
│   │   └── mod.rs                    # Code review session (separate LLM, optional, configurable)
│   ├── tools/
│   │   ├── mod.rs                    # Tool re-exports
│   │   ├── edit_file.rs              # EditFileTool — surgical search_replace edits, create, overwrite
│   │   ├── read_file.rs              # ReadFileTool — partial reading (line ranges) + outline mode for large files
│   │   ├── grep.rs                   # GrepTool — regex search across project file contents
│   │   ├── find_path.rs             # FindPathTool — glob-based file path discovery
│   │   ├── list_directory.rs        # ListDirectoryTool — list directory contents with types/sizes
│   │   ├── git.rs                    # GitTool — git operations via Git CLI subprocess (tokio::process::Command)
│   │   └── terminal.rs              # TerminalTool — shell command execution with timeout (unchanged)
│   ├── git_provider/
│   │   ├── mod.rs                    # GitProvider trait + factory
│   │   ├── github.rs                # GitHub impl (octocrab)
│   │   └── gitlab.rs                # GitLab impl (reqwest)
│   ├── notifier/
│   │   └── mod.rs                    # Notifier trait + Telegram impl + NoopNotifier
│   ├── llm/
│   │   ├── mod.rs                    # LLM module root (re-exports context, logging, agent_factory)
│   │   ├── agent_factory.rs          # AgentFactory + BuiltAgent enum dispatch — centralized LLM provider construction, Copilot API format detection
│   │   ├── context.rs                # Zed-style XML ContextBuilder — adaptive backtick fencing, absolute path resolution, multi-file support
│   │   └── logging.rs                # LLM request/response debug logging — dedicated bmad_bot::llm tracing target
│   └── pipeline.rs                   # StoryPipeline — orchestrates watcher → session → review → PR → notify per story; DevRunner + CodeReviewer traits for DI
└── tests/
    ├── integration.rs                 # Integration test binary entry point (cargo test --test integration)
    ├── integration/
    │   ├── helpers/
    │   │   ├── mod.rs                 # Re-exports: pub mod mocks; pub mod fixtures;
    │   │   ├── mocks.rs              # MockGitProvider, MockNotifier, MockSessionRunner, MockReviewRunner, MockDevRunner, MockCodeReviewer
    │   │   └── fixtures.rs           # make_test_config, make_test_secrets, make_test_story, write_sprint_status, write_wal_file, create_test_repo, create_test_repo_with_remote, PipelineTestBuilder
    │   ├── test_mocks.rs             # Self-verification tests for mock implementations
    │   ├── test_fixtures.rs          # Self-verification tests for fixture builders
    │   ├── test_config.rs            # Config validation integration tests
    │   ├── test_watcher.rs           # Watcher/dependency resolution integration tests
    │   └── test_pipeline.rs          # Pipeline orchestration integration tests (Story 7.4)
    └── e2e/
        └── mod.rs                    # E2E tests (gated behind BMAD_E2E=1)
```

### Requirements to Structure Mapping

| FRs | Domain | Module | Key Files |
|-----|--------|--------|-----------|
| FR1-4 | Story Management | `watcher/` | `mod.rs` (polling), `deps.rs` (pre-gate, topological sort) |
| FR5-7 | Pre-Dev Preparation | *BMAD Agent* | Handled by agent via tools — no daemon code. Symmetric post-implementation impact analysis handled by session runner Step 8 (see Data Flow) |
| FR8-11 | Development Session | `session/`, `llm/` | `runner.rs` (streaming chat loop, XML context activation, post-implementation impact analysis, context-limit recovery), `analyzer.rs` (response analysis), `llm/agent_factory.rs` (AgentFactory + BuiltAgent — centralized provider construction), `branch.rs` (branch management), `cleanup.rs` (partial work), `escalation.rs` (escalation handling), `state.rs` (WAL persistence) |
| FR12-17 | Supervision | `supervisor/`, `llm/` | `mod.rs` (ask_supervisor tool), `rules.rs` (rule engine), `architect.rs` (LLM fallback via AgentFactory), `read_tool.rs` (read-only fs for architect — uses ReadFileTool), `decisions.rs` (decision logging) |
| FR18-20 | Code Review | `review/` | `mod.rs` |
| FR21-24 | PR Management | `git_provider/` | `mod.rs` (trait), `github.rs`, `gitlab.rs` |
| FR25-26 | Notifications | `notifier/` | `mod.rs` |
| FR27-32 | CLI & Config | `cli/`, `config/` | `cli/mod.rs` (clap subcommands, run_start, run_polling_loop), `cli/git_detect.rs` (remote auto-detection), `cli/state.rs` (daemon state), `config/mod.rs` (BotConfig, BotSecrets), `config/discovery.rs` (BMAD discovery) |
| FR33-34 | Resilience & Shutdown | `cli/`, `session/`, `pipeline.rs` | ShutdownFlag created in `run_start()`, propagated through `pipeline.rs` → `session/runner.rs` → `streaming_chat()`. WAL save on interrupt |
| FR35-36 | Error Alerts & Validation | *Cross-cutting* | reqwest-middleware + per-module error handling + notifier |
| FR39 | Copilot Auth | `auth/` | `github_copilot.rs` (OAuth Device Flow, token exchange, CopilotTokenCache) |
| FR40 | LLM Logging | `llm_logging.rs` | Request/response payload logging, `bmad_bot::llm` tracing target |
| — | Pipeline Orchestration | `pipeline.rs` | `StoryPipeline` — orchestrates full story pipeline (session → review → PR → notify) |
| — | LLM Provider Abstraction | `llm/agent_factory.rs` | `AgentFactory` + `BuiltAgent` enum — centralized provider construction, Copilot API format detection, `stream_chat()` dispatch |
| — | XML Context | `llm/context.rs` | `ContextBuilder` for agent activation (Zed-style XML formatting) |
| — | Agent Dev Tools | `tools/` | `edit_file.rs`, `read_file.rs`, `grep.rs`, `find_path.rs`, `list_directory.rs`, `git.rs`, `terminal.rs` — 7 custom tools + ThinkTool (rig built-in) + ask_supervisor |

### Architectural Boundaries

**Module Communication Map:**

```
                ┌──────────┐
                │  config/  │ Arc<BotConfig> + Arc<BotSecrets> shared to all
                └────┬─────┘
                     │
┌────────┐    ┌──────┴──────┐    ┌───────────────┐
│  cli/  │───▶│ pipeline.rs │───▶│   watcher/    │
│        │    │ (orchestr.) │    │   deps.rs     │
└────────┘    └──────┬──────┘    └───────────────┘
  │ creates          │
  │ ShutdownFlag     │ propagates ShutdownFlag
  │                  │
  │           ┌──────┴──────┐
  └──────────▶│  session/   │
              │  runner.rs  │◀─── llm/context.rs (XML context for activation)
              │  analyzer.rs│◀─── llm/logging.rs (request/response logging)
              │             │◀─── llm/agent_factory.rs (BuiltAgent + provider construction)
              │             │◀─── auth/github_copilot.rs (Copilot token)
              │  branch.rs  │
              │  cleanup.rs │
              │  state.rs   │
              └──┬───┬───┬──┘
                 │   │   │
  ┌──────────────┘   │   └──────────────┐
  ▼                  ▼                  ▼
┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│   tools/     │  │  supervisor/ │  │   review/    │
│ git/fs/term  │  │ rules/arch/  │  │ (optional)   │
│ + ThinkTool  │  │ decisions    │  │              │
│ (rig builtin)│  └──────────────┘  └──────┬───────┘
└──────────────┘                           │
                          ┌────────────────┤
                          ▼                ▼
                   ┌──────────────┐ ┌──────────────┐
                   │ git_provider/ │ │  notifier/   │
                   │ github/gitlab │ │  telegram    │
                   └──────────────┘ └──────────────┘
```

**ShutdownFlag propagation path:**
```
cli/run_start() ──creates──▶ ShutdownFlag (Arc<AtomicBool>)
       │
       ├──▶ signal handler task (sets flag on SIGINT/SIGTERM)
       │
       ├──▶ run_polling_loop() ── checks at top of each cycle
       │
       └──▶ StoryPipeline::new(shutdown)
                   │
                   └──▶ SessionRunner::new(shutdown)
                               │
                               ├──▶ run_session() ── checks between chat turns
                               │
                               └──▶ streaming_chat() ── checks between every chunk/tool-call
```

**Interface contracts between modules:**

- **pipeline → watcher:** Pipeline delegates story discovery to watcher. Watcher returns `Vec<StoryInfo>` (eligible stories sorted by dependency order).
- **pipeline → session:** Passes `StoryInfo` struct (eligible story with metadata: id, label, branch name, specs path, dependencies). SessionRunner returns `SessionOutcome`.
- **session → tools:** 8 tools registered at agent build time via `.tool()` (edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor) + ThinkTool — no direct calls from session to tools
- **session → supervisor:** Supervisor is a rig tool called by the agent autonomously, not by the daemon
- **session → llm_context:** SessionRunner uses `ContextBuilder` to format `dev.md` as Zed-style XML for agent activation
- **session → llm/agent_factory:** SessionRunner delegates all LLM provider construction to `AgentFactory::build()`, which returns a `BuiltAgent` with unified `stream_chat()`. No provider-specific logic in session code.
- **llm/agent_factory → auth:** `AgentFactory` uses `CopilotTokenCache` internally to resolve short-lived Copilot session tokens at runtime
- **pipeline → review:** Passes `StoryInfo` (story_key, branch_name, specs_path). `ReviewRunner` loads the same BMAD dev persona (`dev.md`), sends `"CR"` as initial command, `ResponseAnalyzer` handles interaction patterns (story selection, fix decisions, completion detection), post-review phase captures agent commit + markdown report in `ReviewOutcome::Completed { report }`, orchestrator posts report as PR comment via `GitProvider::add_comment()`
- **pipeline → git_provider:** Passes `CreatePrParams` after session/review complete
- **pipeline → notifier:** Passes `NotificationData` (status, story info, PR link if available, error details if any)
- **config → all:** `Arc<BotConfig>` + `Arc<BotSecrets>` injected at startup — read-only, never mutated

### Data Flow

1. **Startup:** `config/` loads and validates `bmad-bot.yaml` + `.env` → `Arc<BotConfig>` + `Arc<BotSecrets>`. `cli/run_start()` validates git availability (`git --version` → require >= 2.30), creates the `ShutdownFlag`, and spawns the signal handler task.
2. **Crash check:** `SessionRunner::check_and_recover_wal()` checks for existing WAL file → if found, `pipeline.recover_and_process()` resumes the interrupted session (skip to step 5 with loaded history)
3. **Poll:** `watcher/` reads `sprint-status.yaml` from configured output path → `deps.rs` computes topological sort and pre-gate → eligible stories or sleep until next cycle. Uses `tokio::time::interval` which **ticks immediately on first call** — daemon polls at launch, not after `polling_interval_secs`.
4. **Session init:** `session/runner.rs` builds the system preamble (operational instructions + tool usage rules + language override), then uses `AgentFactory::build()` to construct the rig agent with **9 tools** (edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor, ThinkTool). `GitTool` invokes the `git` CLI via `tokio::process::Command`, inheriting the user's full git configuration. `AgentFactory` centralizes all provider construction behind a `BuiltAgent` enum with `stream_chat()` dispatch. Provider resolution is hardcoded: Anthropic → Messages API, OpenAI → Responses API, GitHub Copilot → explicit match on known OpenAI model families (`gpt-*`, `o1-*`, `o3-*`, `codex`) for Responses API, fallback to Completions API for all other models (safe default for non-OpenAI backends).
5. **Agent activation:** `activate_agent()` sends the BMAD dev agent file (`dev.md`) as the first user message wrapped in Zed-style XML context tags (via `ContextBuilder`). The agent processes activation steps via tools (loads `config.yaml`, displays greeting/menu). Returns `(rig_history, chat_history)` for subsequent turns.
6. **Chat loop:** Sends `"DS"` via `streaming_chat()` → agent works autonomously via tools → `state.rs` persists chat history (WAL) after each turn. **All LLM calls use streaming** — `streaming_chat()` consumes SSE stream, collects text, handles tool calls via rig's multi-turn stream. ShutdownFlag checked between every chunk. `llm_logging` records request/response payloads.
7. **During session:** Agent calls `ask_supervisor` tool as needed → rule engine → LLM fallback (architect session) → or escalation (stops session)
8. **Post-completion — Impact analysis:** After the agent signals `<<BMAD_JOB_DONE>>`, the session runner executes a three-step post-completion sequence: **(a) Final commit** — commit any uncommitted changes (tool access: yes). **(b) Impact analysis** — a dedicated chat turn where the agent retains full tool access and evaluates downstream impact (tool access: yes). The agent reads `sprint-status.yaml`, identifies stories whose `depends-on` references the completed story, reads their Dev Notes ("Previous Story Intelligence" sections), compares actual implementation against assumptions, and updates stale sections with what was actually built. Optionally updates `architecture.md` if new modules or changed interfaces were introduced (checks existence first). Commits changes with `docs(stories): update downstream specs after {story_key}` prefix. **Design constraints:** best-effort and non-blocking — if this turn fails (LLM error, timeout, context exhaustion), the session proceeds to PR summary without error. Agent-driven — the daemon sends the prompt, the agent uses existing tools (`read_file`, `edit_file`, `git`). Scope-guarded — only "Previous Story Intelligence" in Dev Notes and architecture references are updated, never tasks, ACs, or other story sections. Updates are idempotent — sections are replaced, not appended. Dependency resolution uses `depends-on` as primary criterion, same-epic document order as secondary. **(c) PR summary** — agent generates `<pr-summary>` for PR description (tool access: no, text only). The PR summary prompt is aware that an impact analysis commit may have been added.
9. **Push & PR creation:** `pipeline.rs` pushes the story branch to remote via `git push`, then `git_provider/` creates PR (GitHub or GitLab) with agent-written description + Supervisor Decisions section. The PR is immediately visible for human review even if automated code review is disabled.
10. **Code review (optional):** If `code_review_enabled`, `review/ReviewRunner` launches a new rig agent session with the review LLM config, loads the same BMAD dev persona (`dev.md`), and sends `"CR"` as the initial command. The agent drives the full CR workflow autonomously (diff analysis, adversarial review, fix application). `ResponseAnalyzer` handles all interaction patterns (story selection replies, fix decisions, completion detection). On CR completion, the daemon sends a post-review message asking the agent to commit fixes with descriptive messages and produce a markdown review report. The report is captured in `ReviewOutcome::Completed { report }`. The pipeline then pushes any review fix commits to update the PR, and posts the review report as a comment via `GitProvider::add_comment()`. Review failures are non-blocking — the PR already exists regardless of review outcome.
11. **Notification:** `notifier/` sends Telegram message with story status + PR link (run summary for batch)
12. **Cleanup:** `session/state.rs` deletes WAL file → return to step 3

### External Integration Points

| Integration | Module | Protocol | Auth | Notes |
|-------------|--------|----------|------|-------|
| LLM Provider (Anthropic) | `llm/agent_factory.rs` | HTTPS via rig-core | API key from `.env` | Anthropic Messages API. Constructed via `AgentFactory::build()` → `BuiltAgent::Anthropic` |
| LLM Provider (OpenAI) | `llm/agent_factory.rs` | HTTPS via rig-core | API key from `.env` | Uses **Responses API**. Constructed via `AgentFactory::build()` → `BuiltAgent::OpenAiResponses` |
| LLM Provider (GitHub Copilot) | `llm/agent_factory.rs`, `auth/` | HTTPS via rig-core + reqwest | OAuth token from `.env` → exchanged at runtime for short-lived Copilot session token via `GET https://api.github.com/copilot_internal/v2/token` | Proxy to multiple backends — API format is **hardcoded per model**: known OpenAI model families (`gpt-*`, `o1-*`, `o3-*`, `codex`) use **Responses API** (`BuiltAgent::OpenAiResponses`), all other models (Claude, Mistral, etc.) **fallback to Completions API** (`BuiltAgent::OpenAiCompletions`) — safe default for non-OpenAI backends. Requires IDE headers: `Editor-Version`, `Editor-Plugin-Version`, `Copilot-Integration-Id` ("vscode-chat"). Base URL derived from token `proxy-ep` field; default: `https://api.individual.githubcopilot.com`. Headers injected via rig's `.http_headers()` builder. Provider construction centralized in `AgentFactory` with `copilot_requires_responses_api()` heuristic |
| GitHub API | `git_provider/github.rs` | HTTPS via octocrab | Token from `.env` | |
| GitLab API | `git_provider/gitlab.rs` | HTTPS via reqwest | Token from `.env` | |
| Telegram API | `notifier/mod.rs` | HTTPS via reqwest | Bot token from `.env` | |
| Git CLI (>= 2.30) | `tools/git.rs`, `session/branch.rs`, `pipeline.rs` | Subprocess via `tokio::process::Command` / `std::process::Command` | Inherits user's git config (SSH agent, credential manager, osxkeychain) | System dependency — validated at daemon startup. Replaces former `git2` (libgit2) embedded library. Enables commit signing, user identity, and unified auth path |
| Local filesystem | `tools/edit_file.rs`, `read_file.rs`, `grep.rs`, `find_path.rs`, `list_directory.rs` | std::fs / tokio::fs | OS permissions | 5 focused tools replacing former monolithic FsTool |
| Local terminal | `tools/terminal.rs` | tokio::process | OS permissions | Configurable timeout |
| BMAD config | `config/mod.rs` | File read (YAML) | Filesystem access | |
| sprint-status.yaml | `watcher/mod.rs` | File read (YAML) | Filesystem access | |

### Configuration Files

| File | Committed | Purpose |
|------|-----------|---------|
| `bmad-bot.yaml` | ✅ Yes | Project config: polling interval, LLM providers/models, git provider, notification config, BMAD paths |
| `.env` | ❌ No (gitignored) | Secrets: API keys, tokens, credentials |
| `bmad-bot.yaml.example` | ✅ Yes | Template for new users |
| `.env.example` | ✅ Yes | Template listing required env vars (no values) |
| `_bmad-output/implementation-artifacts/.bmad-bot-session.yaml` | ❌ No (transient) | Session WAL file — exists only during active session |
| `.bmad-bot-state.json` | ❌ No (transient) | Daemon state file — exists only while daemon is running, used by `bmad-bot status` |

## Architecture Validation Results

### Coherence Validation ✅

**Decision Compatibility:**
All architectural decisions work together without conflicts:
- rig-core + tokio + reqwest + tracing — no dependency conflicts, all async-compatible. Git operations via CLI subprocess (no native library dependency)
- Hybrid supervisor model (streaming chat loop + ask_supervisor tool) aligns naturally with rig's `StreamingChat` trait and `Tool` trait
- Daemon-reads/agent-writes model is consistent with "BMAD files are sacred" principle and "daemon as minimal orchestrator"
- Session WAL file + crash recovery is consistent with cooperative shutdown requirements
- Three-tier error propagation (middleware → tool → session) aligns with module boundaries
- Agent prompt composition (minimal preamble + XML context activation) is consistent with "daemon has an explicit activation phase" and clean separation of concerns
- Cooperative ShutdownFlag propagation is consistent with streaming-first architecture — every LLM call can be interrupted
- Pipeline module (`pipeline.rs`) cleanly separates orchestration from execution (session/review)
- Post-implementation impact analysis (session runner Step 8b) is the symmetric counterpart to the pre-dev spec update (FR5-7) — together they form a closed loop: pre-dev reads prior stories' output, post-impl propagates forward to downstream stories. Both are agent-driven with existing tools, no new daemon logic required.
- Impact analysis step follows the same best-effort, non-blocking pattern as the enriched PR summary (commit `6450450`) — a dedicated post-completion chat turn with grounded context, graceful degradation on failure

**Pattern Consistency:**
- All patterns use thiserror per module, anyhow only in binary — consistent error handling across all modules
- All rig tools follow the same structural template (struct + args + error + Tool impl)
- Tracing patterns use story_id spans consistently across the pipeline
- Config shared as Arc<BotConfig> everywhere — single pattern, no exceptions
- All LLM calls use streaming (`streaming_chat()`) — no exceptions, Copilot requires it
- ShutdownFlag propagated to every layer that runs LLM calls

**Structure Alignment:**
- Project structure directly maps to architectural decisions: one module per FR domain plus support modules
- Module boundaries match the communication diagram — no circular dependencies
- Integration points are clearly defined at module interfaces with dedicated structs

### Requirements Coverage Validation ✅

**Functional Requirements: 38/38 covered**

| FR Range | Domain | Architectural Support |
|----------|--------|----------------------|
| FR1-4 | Story Management | `watcher/` (polling + pre-gate dependency check with topological sort). Agent handles status mutations via tools. |
| FR5-7 | Pre-Dev Preparation | BMAD agent autonomously reads prior stories and updates specs via filesystem tool. No daemon code needed. Symmetric post-implementation impact analysis in session runner Step 8b propagates implementation reality forward to downstream stories. |
| FR8-11 | Development Session | `session/runner.rs` builds rig agent with expanded preamble (tool usage rules + language override), activates BMAD agent via XML context (`llm_context.rs`), registers 9 tools (edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor, ThinkTool), manages streaming chat loop, executes post-completion sequence (final commit → impact analysis → PR summary). Agent file sent as user message, not system prompt. |
| FR12-17 | Supervision | `supervisor/` implements ask_supervisor as rig Tool. Rule engine in `rules.rs`, LLM fallback via architect session in `architect.rs` (with read-only `read_tool.rs`), decision logging in `decisions.rs`. Escalation returns tool error → stops rig loop. |
| FR18-20 | Code Review | `review/` launches separate LLM session. Configurable (enabled/disabled). Fixes in separate commits, review posted as PR comment. |
| FR21-24 | PR Management | `git_provider/` trait with GitHub (octocrab) and GitLab (reqwest) implementations. PR created even for failed/blocked stories with failure description. |
| FR25-26 | Notifications | `notifier/` sends Telegram messages with story ID, status, and PR link. Non-blocking — failures logged but don't stop pipeline. Run summaries for batch processing. |
| FR27-32 | CLI & Config | `cli/` implements 4 clap subcommands. `config/` loads YAML + .env, validates at startup, auto-discovers BMAD version (`config/discovery.rs`). `cli/git_detect.rs` for interactive init. `cli/state.rs` for daemon state tracking. |
| FR33-34 | Resilience & Shutdown | Cooperative shutdown via `ShutdownFlag` (Arc<AtomicBool>) created in `cli/run_start()`, propagated through `pipeline.rs` → `session/runner.rs` → `streaming_chat()`. Interrupts mid-streaming and mid-tool-call. Saves WAL, commits partial work, notifies on shutdown. |
| FR35-36 | Error Alerts & Validation | reqwest-middleware for HTTP retry/backoff (max 3). Notifier for blocking error alerts. Config validation at startup with descriptive errors. |
| FR39 | Copilot Auth | `auth/github_copilot.rs` — OAuth Device Flow for user authentication during `init`, `CopilotTokenCache` for transparent runtime token exchange. Short-lived session tokens refreshed automatically. |
| FR40 | LLM Logging | `llm_logging.rs` — dedicated `bmad_bot::llm` tracing target. Logs requests, responses, errors, and full history. Zero-cost when disabled. Used across session, review, and supervisor. |

**Non-Functional Requirements: All covered**

| NFR | Coverage |
|-----|----------|
| Security | Secrets in `.env` only (dotenvy), never in committed config, never logged. Tracing filters sensitive fields. Git credentials from environment. Copilot OAuth tokens exchanged for short-lived session tokens. |
| Integration | LLM providers via rig-core (Anthropic, OpenAI Responses API, Copilot Completions API). GitHub via octocrab. GitLab via reqwest. Telegram via reqwest. All with retry middleware. |
| Reliability | Exponential backoff (max 3 retries) via reqwest-middleware. Cooperative shutdown via ShutdownFlag (mid-stream interruption). Crash recovery via session WAL file. Context-limit recovery with history summarization. All errors logged with full context. |
| Scalability | MVP: single daemon, sequential execution. Architecture does not preclude future parallelization — modules are independent, config is Arc-shared, no global mutable state. Pipeline processes stories sequentially but structure supports future parallelization. |

### Implementation Readiness Validation ✅

**Decision Completeness:**
- All 6 critical/important decisions documented with rationale and implementation guidance
- Technology versions verified (rig-core, Rust edition 2024)
- Implementation patterns include code examples for all 7 pattern categories
- Anti-patterns explicitly listed to prevent common mistakes

**Structure Completeness:**
- Complete directory tree with every file and its purpose (35 source files)
- All 38 FRs mapped to specific modules and files
- Module communication diagram with interface contracts
- Full data flow documented (11 steps from startup to cleanup)
- ShutdownFlag propagation path documented

**Pattern Completeness:**
- Error handling: per-module thiserror + anyhow in binary only
- Tool implementation: standard struct/args/error/trait template + ThinkTool note
- Tracing: structured spans with story_id context + LLM payload logging
- Cooperative shutdown: ShutdownFlag propagation pattern
- Config: validate once, share via Arc
- Git provider: trait with dedicated param/return structs
- Testing: mocked LLM responses, Arrange-Act-Assert, naming convention

### Gap Analysis Results

| # | Priority | Gap | Resolution |
|---|----------|-----|------------|
| 1 | Minor | `review/` module needs diff access — `ReviewContext` struct should include branch name for `git diff` computation via CLI | Implementation detail — resolved when coding `review/mod.rs` |
| 2 | Minor | Supervisor LLM fallback needs project docs as context — source paths come from `BotConfig` (planning_artifacts, project_knowledge) | Implementation detail — supervisor reads paths from config |
| 3 | Minor | Exact `bmad-bot.yaml` field schema not specified | Normal for architecture stage — defined during `config/` implementation |
| 4 | Minor | ReadFileTool outline mode uses regex heuristics, not AST parsing — may miss some symbols in complex Rust code | Acceptable trade-off: 90%+ coverage with zero dependencies. Can be enhanced incrementally with tree-sitter if needed |
| 5 | Minor | GrepTool may need the `glob` or `walkdir` crate as new dependencies | Lightweight, well-maintained crates — add to Cargo.toml during implementation |

**No critical or blocking gaps found.**

### Architecture Completeness Checklist

**✅ Requirements Analysis**
- [x] Project context thoroughly analyzed (38 FRs, 4 NFR categories)
- [x] Scale and complexity assessed (medium, CLI daemon)
- [x] Technical constraints identified (rig maturity, Git CLI >= 2.30, BMAD read-only, Copilot streaming requirement)
- [x] Cross-cutting concerns mapped (errors, logging, secrets, LLM abstraction, traceability, cooperative shutdown)

**✅ Architectural Decisions**
- [x] 6 critical/important decisions documented with rationale
- [x] Technology stack fully specified with versions
- [x] Integration patterns defined (streaming chat loop + supervisor tool + XML context activation)
- [x] Crash recovery and resilience addressed
- [x] Cooperative shutdown pattern documented with propagation chain

**✅ Implementation Patterns**
- [x] Error type pattern established (thiserror per module)
- [x] Rig tool pattern standardized — focused tools, no action multiplexing (including ThinkTool note)
- [x] Tracing pattern with structured spans + LLM payload logging
- [x] Cooperative shutdown pattern with ShutdownFlag propagation
- [x] Config, Git provider, and test mock patterns defined
- [x] Anti-patterns explicitly documented (including non-streaming anti-pattern, full-file-rewrite anti-pattern)

**✅ Project Structure**
- [x] Complete directory structure with all source files (tools/ expanded: edit_file, read_file, grep, find_path, list_directory, git, terminal)
- [x] Module boundaries and communication map (including pipeline, auth, llm_context, llm_logging)
- [x] FR-to-module mapping (38/38)
- [x] Data flow documented (11 steps)
- [x] External integration points catalogued (with Copilot API split documented)
- [x] ShutdownFlag propagation path documented

### Architecture Readiness Assessment

**Overall Status:** ✅ READY FOR IMPLEMENTATION

**Confidence Level:** High — all requirements covered, no blocking gaps, decisions are coherent and well-documented.

**Key Strengths:**
- Simple, modular architecture that aligns with rig's design philosophy
- Daemon stays minimal — the BMAD agent does the heavy lifting
- **Surgical development tooling** — focused tools (edit_file, read_file, grep, find_path) modeled on Claude Code/Zed patterns for token-efficient, safe code editing
- Crash recovery built-in from day one (session WAL)
- Full decision traceability (supervisor decisions → file + PR)
- Two-layer dependency model (daemon pre-gate + agent verification) saves tokens
- GitProvider trait enables GitHub + GitLab from MVP
- Cooperative shutdown interrupts at finest granularity (mid-stream)
- Clean agent activation pattern (system preamble with tool usage rules vs XML context persona)
- Universal streaming ensures consistency across all providers

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
- Use implementation patterns consistently — especially error types, tool structure (focused tools, no action multiplexing), tracing, cooperative shutdown, and streaming
- Respect module boundaries — each module owns its error types and exposes clean interfaces
- Check the anti-patterns list before submitting any code
- Always use `streaming_chat()` — never `agent.chat()`
- Always propagate ShutdownFlag to functions that make LLM calls
- Agent activation uses XML context (first user message), not system preamble
- Tools follow the "one tool = one concern" principle — never add an `action` multiplexer field

**First Implementation Priority:**
`cargo init bmad-bot` + dependency setup + module scaffolding. This should be the first implementation story, establishing the project skeleton that all subsequent stories build on.