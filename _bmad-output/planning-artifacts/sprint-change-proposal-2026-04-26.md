---
type: sprint-change-proposal
date: 2026-04-26
project: bmad-bot
author: JB
status: approved
triggered_by: Strategic evolution — add SDK-based providers (Claude Code, Codex) alongside existing API-based providers (rig)
scope: major
---

# Sprint Change Proposal — SDK Provider Runtime & Supervisor MCP

## 1. Issue Summary

### Problem Statement

The BMAD Bot currently manages LLM sessions exclusively through direct API calls via the `rig` crate. The daemon controls every aspect of the agent loop: tool registration, chat turn management, streaming, context window recovery, and tool execution. This works well but tightly couples the daemon to the rig framework and requires maintaining custom tool implementations (EditFile, ReadFile, Grep, FindPath, ListDirectory, Git, Terminal) that duplicate functionality already provided by modern agentic CLI tools.

Two mature agentic CLI tools now exist that can run autonomously in headless mode:

- **Claude Code** (`claude` CLI) — Anthropic's agent with built-in tools (Read, Edit, Bash, Grep, WebSearch, Agent), MCP server support, session management, and headless mode via `--bare -p "..." --output-format stream-json`
- **OpenAI Codex** (`codex` CLI) — OpenAI's agent with built-in tools (file editing, terminal, web search), MCP server support, and headless mode via `codex exec --json --full-auto`

Both support **MCP (Model Context Protocol)**, meaning the daemon's supervisor logic can be exposed as an MCP server that these tools consume natively — no custom tool registration needed.

### Proposed Change

Add a **SDK runtime mode** as a parallel option alongside the existing **API runtime mode**. Each LLM role (dev, review, supervisor, critic) can independently use either mode:

```yaml
# API mode (current) — daemon manages session via rig
dev:
  provider: anthropic
  model: claude-sonnet-4-6

# SDK mode (new) — daemon delegates to external agentic CLI
dev:
  provider: claude-code
  model: claude-sonnet-4-6

# SDK mode — OpenAI Codex
dev:
  provider: codex
  model: o4-mini
```

### Context

- All 14 epics are functionally complete
- Epic 15 is in-progress with only the pre-epic cleanup story (15-0a) done — no feature stories defined yet
- Both Claude Code and Codex CLIs are mature and widely adopted
- No Rust SDK exists for either — CLI subprocess is the integration path
- This is an additive change: existing API mode is preserved unchanged

### Evidence

- Claude Code SDK: headless mode (`--bare --output-format stream-json --allowedTools`), MCP support (`--mcp-config`), session continuity (`--resume`), permission control (`--permission-mode acceptEdits`)
- Codex CLI: headless mode (`codex exec --json --full-auto`), MCP support (stdio + HTTP servers), sandbox policies (`--sandbox workspace-write`), session management (`codex resume`)
- Both provide their own file editing, terminal, search, and navigation tools — eliminating the need for the daemon's custom rig tools in SDK mode
- MCP is the standard protocol for extending both tools with custom capabilities — the supervisor's rule engine → LLM fallback → escalation pipeline maps naturally to an MCP tool

---

## 2. Impact Analysis

### Epic Impact

| Epic | Status | Impact |
|------|--------|--------|
| **Epics 1-10** | Done | 🟢 No impact — foundation, watcher, tools, UI unchanged |
| **Epic 11** | Done (Copilot removal) | 🟢 No impact — provider simplification already done |
| **Epic 12** | Done (Skill sessions) | 🟢 No impact — skill activation works for both runtimes |
| **Epic 13** | Done (Multi-phase pipeline) | 🟡 Pipeline orchestration needs dual-runtime routing |
| **Epic 14** | Done (Epic review/deferred) | 🟢 No impact |
| **Epic 15** | In-progress (pre-epic only) | 🔴 **Defines Epic 15 content** — this proposal IS epic 15 |

### Story Impact

**No existing stories are modified.** This is entirely additive — a new epic with new stories.

The pipeline orchestration in `pipeline.rs` gains a routing layer that checks the configured provider type and delegates to the appropriate runtime. All existing API-mode code paths remain unchanged.

### Artifact Conflicts

#### PRD

**Functional Requirements to modify:**

| FR | Current | Change |
|----|---------|--------|
| FR8 | Streaming rig agent session with BMAD dev agent persona | Generalize: "agent session via configured runtime (API or SDK)" |
| FR9 | Expose surgical tools to the agent via rig tool calling | Add: "In SDK mode, tools are provided by the external CLI; daemon exposes supervisor via MCP" |
| FR42 | AgentFactory with rig BuiltAgent dispatch | Generalize to SessionFactory with API and SDK runtime dispatch |

**Functional Requirements to add:**

| New FR | Description |
|--------|------------|
| FR60 | The daemon can delegate a development session to Claude Code CLI (`claude --bare -p`) running as a subprocess, with structured JSON output parsing and real-time progress monitoring |
| FR61 | The daemon can delegate a development session to Codex CLI (`codex exec --json`) running as a subprocess, with NDJSON event parsing and real-time progress monitoring |
| FR62 | The daemon can expose the supervisor's `ask_supervisor` capability as an MCP server (stdio transport) that SDK-mode sessions consume natively. LLM fallback backend is provider-agnostic. Only `ask_supervisor` exposed via MCP — consultations (adversarial, critic) remain daemon-orchestrated (Decision 10 unchanged). |
| FR63 | Each LLM role (dev, review, supervisor, critic) can be independently configured with either an API provider (anthropic, openai-compatible) or an SDK provider (claude-code, codex), with model selection for each |
| FR64 | The `bmad-bot init` command supports interactive setup of SDK providers, including CLI availability validation and MCP server configuration |

#### Architecture

**Decisions to add:**

| Decision | Description |
|----------|------------|
| **Decision 12** | Dual Runtime Abstraction — `SessionRuntime` enum with `Api` (rig-based, current) and `Sdk` (CLI subprocess) variants |
| **Decision 13** | Supervisor MCP Server — stdio-transport MCP server exposing `ask_supervisor` with the existing 3-tier cascade |

**Decisions to amend:**

| Decision | Amendment |
|----------|-----------|
| **D1** (Supervisor Interception) | SDK mode: supervisor accessed via MCP protocol instead of rig tool. Same 3-tier cascade (rules → LLM → escalation), different transport. |
| **D5** (Agent Prompt Composition) | **API mode only.** The system preamble (`build_preamble()`, `build_create_preamble()`) with tool rules, skill activation instructions, spawn_agent rules, `ContextBuilder` XML wrapping — all exclusively for rig-based sessions. **SDK mode (both providers):** no preamble, no inlined skill content. Skills invoked natively via slash commands. Claude Code discovers `.claude/skills/` + `CLAUDE.md`; Codex discovers `.agents/skills/` + `AGENTS.md`. BMAD installer handles skill placement. Daemon only passes story context + branch info in the prompt. |
| **D8** (LLM Provider Abstraction) | `BuiltAgent` remains for API mode. New `SdkSession` struct for SDK mode. Both wrapped in `SessionRuntime` enum. |
| **D10** (Daemon-Orchestrated Consultations) | **Unchanged.** Consultations remain daemon-orchestrated for both runtimes. API mode: pause session, run consultation agent, inject findings as user message. SDK mode: session completes a phase, daemon runs consultation as separate CLI subprocess, resumes original session (`--resume`) with findings as prompt. No consultation MCP tools needed. |

#### Technical Impact

**New modules:**

| Module | Purpose |
|--------|---------|
| `src/runtime/mod.rs` | `SessionRuntime` enum, routing logic |
| `src/runtime/api.rs` | API runtime (wraps current rig-based session flow) |
| `src/runtime/sdk.rs` | SDK runtime (subprocess management, output parsing) |
| `src/runtime/sdk_claude.rs` | Claude Code-specific CLI flags, prompt construction, output format |
| `src/runtime/sdk_codex.rs` | Codex-specific CLI flags, prompt construction, NDJSON format |
| `src/mcp_server/mod.rs` | MCP server infrastructure (stdio transport) |
| `src/mcp_server/supervisor.rs` | Supervisor MCP tool — delegates to existing rule engine + LLM fallback |

**Modified modules:**

| Module | Change |
|--------|--------|
| `src/pipeline.rs` | Route to API or SDK runtime based on provider config |
| `src/config/mod.rs` | New provider types (`claude-code`, `codex`), validation |
| `src/cli/mod.rs` | Init command: SDK provider setup, CLI availability check |
| `src/session/runner.rs` | Extract API-specific logic into `runtime/api.rs` |

**Unchanged modules:** `src/tools/*`, `src/supervisor/*`, `src/review/*`, `src/watcher/*`, `src/ui/*`, `src/git_provider/*`, `src/notifier/*`

---

## 3. Recommended Approach

### Selected Path: Direct Adjustment — New Epic 15

**Rationale:**

This is a clean additive change. The existing codebase is not modified — it's wrapped in a new abstraction layer. The API runtime becomes one variant of a runtime enum, the SDK runtime is added as a parallel variant.

No rollback needed. No MVP scope change. The core value proposition is preserved (API mode works exactly as before) while adding a powerful new capability.

**Effort estimate:** High — 7-8 stories across one epic
**Risk level:** Medium — subprocess management and MCP server are new infrastructure patterns for this codebase, but both are well-documented and the daemon's existing patterns (ShutdownFlag, UI events, pipeline orchestration) apply cleanly
**Timeline impact:** One full epic (Epic 15) estimated at 2-3 weeks of automated development

### Alternatives Considered

| Option | Verdict | Why |
|--------|---------|-----|
| **Replace rig entirely** | Rejected | Breaks existing API users, removes fine-grained control. SDK CLIs have limitations (no turn-by-turn chat loop control, session isolation constraints) |
| **SDK only for dev role** | Rejected | Artificial restriction. The config-per-role model already supports mixed mode — no reason to limit it |
| **Wait for Rust SDK** | Rejected | Neither Anthropic nor OpenAI have announced Rust SDKs for their agentic tools. CLI subprocess is the stable, documented integration path |

---

## 4. Detailed Change Proposals

### Epic 15: SDK Provider Runtime & Supervisor MCP

**Theme:** Add Claude Code and Codex as SDK-based provider options alongside the existing rig-based API providers. Each LLM role independently selects its provider and model.

**Dependencies:** Epic 13 (multi-phase pipeline in place), Epic 14 (pre-epic story mechanism)

#### Story 15.1: Session Runtime Abstraction Layer

As a daemon developer,
I want the session execution logic abstracted behind a `SessionRuntime` enum with an `Api` variant wrapping the current rig-based flow,
So that a second `Sdk` variant can be added without modifying existing code.

**Acceptance Criteria:**

- New `src/runtime/mod.rs` module defines `SessionRuntime` enum with `Api(ApiRuntime)` variant
- `ApiRuntime` wraps the current session execution flow from `session/runner.rs` (build agent, run session, handle consultations)
- System preamble construction (`build_preamble()`, `build_create_preamble()`) scoped to `ApiRuntime` — these are API-mode-specific and not used by SDK sessions
- **Skill path resolution via BMAD manifest:** new `resolve_skill_path(skill_name)` reads `_bmad/_config/manifest.yaml` → `ides[]` and maps to the correct directory. Replaces all hardcoded `.claude/skills/` references in `pipeline.rs`, `review/mod.rs`, `session/runner.rs`
- `pipeline.rs` calls `SessionRuntime::run_session()` instead of directly calling `SessionRunner` methods
- All existing tests pass with zero behavioral changes
- The `Sdk` variant is defined as a stub (`todo!()`) — wired in subsequent stories

**Estimated effort:** Medium
**Risk:** 🟢 Low — pure refactoring, no new features

---

#### Story 15.2: Config Extension for SDK Providers

As a daemon operator,
I want to configure `claude-code` and `codex` as provider types in `bmad-bot.yaml` alongside `anthropic` and `openai-compatible`,
So that each LLM role can independently use API or SDK mode.

**Acceptance Criteria:**

- `BotConfig` accepts `provider: "claude-code"` and `provider: "codex"` for any LLM role (dev, review, supervisor, critic)
- SDK providers are valid for the supervisor role — the MCP server is the interface for SDK sessions, and the supervisor's LLM fallback backend uses whatever provider is configured (API spawns a rig agent, SDK spawns a Claude Code/Codex subprocess)
- Validation checks CLI availability (`claude --version` / `codex --version`) at startup for configured SDK providers
- Validation checks BMAD skills are installed in the correct directory for configured SDK providers (`.claude/skills/bmad-*` for claude-code, `.agents/skills/bmad-*` for codex) — fail fast with clear error directing user to run BMAD installer
- Config includes optional `cli_path` override for non-standard installation locations
- Model selection works identically: `model: "claude-sonnet-4-6"` or `model: "o4-mini"`
- `VALID_LLM_PROVIDERS` extended to include `"claude-code"` and `"codex"`

**Config example:**

```yaml
dev:
  provider: claude-code
  model: claude-sonnet-4-6
review:
  provider: codex
  model: o4-mini
supervisor:
  provider: claude-code      # SDK providers work too — MCP server delegates to this
  model: claude-haiku-4-5
critic:
  provider: claude-code
  model: claude-opus-4-7
```

**Estimated effort:** Small
**Risk:** 🟢 Low — config extension with validation

**Dependencies:** None

---

#### Story 15.3: SDK Runtime Subprocess Infrastructure

As a daemon developer,
I want a generic `SdkRuntime` that manages an external CLI process (spawn, environment, working directory, streaming output, shutdown),
So that Claude Code and Codex integrations share common subprocess management code.

**Acceptance Criteria:**

- `SdkRuntime` struct in `src/runtime/sdk.rs` manages:
  - Process spawning with configurable command, args, env vars, working directory
  - Streaming stdout parsing (line-by-line NDJSON)
  - Stderr capture for error reporting
  - Graceful shutdown via `ShutdownFlag` → process kill (SIGTERM, then SIGKILL after timeout)
  - Exit code interpretation
- `SdkOutputEvent` enum for parsed output events (progress, tool_call, completion, error)
- **Session ID tracking**: extract and store the CLI session ID from the output stream for each phase. Required for `--resume` (Claude Code) / `codex resume` (Codex) when injecting consultation findings or recovering from crash
- UI events emitted during subprocess execution (tool calls, progress) via `UiHandle`
- Process timeout configurable per session (default: matches story complexity estimate)
- Environment variable injection for API keys (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`)

**Estimated effort:** Medium
**Risk:** 🟡 Medium — subprocess lifecycle management is new territory for this codebase

**Dependencies:** 15.1

---

#### Story 15.4: Supervisor MCP Server

As a daemon developer,
I want the supervisor's `ask_supervisor` capability exposed as an MCP server (stdio transport),
So that SDK-mode sessions can access the supervisor via their native MCP integration.

**Acceptance Criteria:**

- New `src/mcp_server/` module implements a stdio-transport MCP server
- Exposes a single tool: `ask_supervisor` with `question: String` parameter
- Tool handler delegates to the existing 3-tier cascade:
  1. Rule engine (`supervisor/rules.rs`) — deterministic pattern matching
  2. LLM fallback (`supervisor/architect.rs`) — project docs context
  3. Human escalation — returns MCP error with escalation context
- MCP server started as a child process by the daemon when SDK sessions are active
- LLM fallback backend is provider-agnostic: uses `SessionRuntime::Api` or `SessionRuntime::Sdk` based on supervisor role config — a supervisor configured as `provider: claude-code` spawns a second Claude Code subprocess for the LLM fallback step
- **No MCP config passed to supervisor subprocess** — the supervisor IS the MCP backend, passing it its own MCP config would create an infinite recursion loop. Supervisor SDK sessions are simple question-answering calls with project docs context, no tool access beyond read-only file operations
- MCP server config JSON generated dynamically for each SDK session:
  ```json
  {
    "mcpServers": {
      "bmad-supervisor": {
        "command": "bmad-bot",
        "args": ["mcp-supervisor", "--story", "{story_id}"],
        "env": { "ANTHROPIC_API_KEY": "..." }
      }
    }
  }
  ```
- New CLI subcommand: `bmad-bot mcp-supervisor` (hidden from help, internal use only)
- Decision logging preserved — MCP tool calls logged the same way as rig tool calls
- Supervisor decisions file still committed at session end

**Estimated effort:** Large
**Risk:** 🟡 Medium — MCP server implementation is new, but the protocol is well-documented (JSON-RPC over stdio) and the supervisor logic is already battle-tested

**Dependencies:** None (can run in parallel with 15.1-15.3)

---

#### Story 15.5: Claude Code Provider Integration

As a daemon operator,
I want to use `provider: claude-code` for any LLM role so the daemon delegates sessions to the Claude Code CLI,
So that I benefit from Claude Code's built-in tools, context management, and agentic capabilities.

**Acceptance Criteria:**

- `SdkClaudeCodeProvider` in `src/runtime/sdk_claude.rs` implements Claude Code-specific logic:
  - CLI invocation: `claude -p "/bmad-dev-story {story_context}" --output-format stream-json --allowedTools "Read,Edit,Bash,Grep,WebSearch,Agent,Skill" --permission-mode acceptEdits`
  - **No `--bare`** — Claude Code discovers project skills (`.claude/skills/`), `CLAUDE.md`, and conventions natively
  - Skills invoked via native slash commands: `/bmad-dev-story`, `/bmad-create-story`, `/bmad-code-review`
  - Model override: `--model {configured_model}`
  - Working directory: `--cd {project_root}`
  - MCP config injection: `--mcp-config '{supervisor_mcp_json}'` when supervisor MCP is available
  - Session resume: `--resume {session_id}` for crash recovery
- **Native skill invocation** (no system preamble, no inlined skill content):
  - Claude Code launched **without `--bare`** so it discovers the project's `.claude/skills/`, `CLAUDE.md`, and project conventions natively
  - Skills invoked via their native slash commands: `/bmad-dev-story`, `/bmad-create-story`, `/bmad-code-review`, etc.
  - Prompt includes: skill invocation + story-specific context (story file path, branch name, language override)
  - Claude Code handles skill discovery, workflow.md reading, tool selection, and execution autonomously
  - NO inlined skill content, NO tool usage rules, NO preamble — Claude Code's native skill system handles everything
  - **Codex parity:** Codex CLI has its own native skill system (`.agents/skills/` + `AGENTS.md`) with the same SKILL.md format. BMAD's installer handles placing skills in the correct directory per CLI. Both SDK providers use native skill invocation — no inlined skill content needed for either.
- Streaming JSON output parsed into `SdkOutputEvent` for real-time UI updates
- Session ID captured and persisted in WAL for recovery
- Claude Code CLI availability validated at startup

**Estimated effort:** Large
**Risk:** 🟡 Medium — first real integration, will surface design issues in the abstraction layer

**Dependencies:** 15.2, 15.3, 15.4

---

#### Story 15.6: Codex Provider Integration

As a daemon operator,
I want to use `provider: codex` for any LLM role so the daemon delegates sessions to the Codex CLI,
So that I can use OpenAI models with autonomous agent capabilities.

**Acceptance Criteria:**

- `SdkCodexProvider` in `src/runtime/sdk_codex.rs` implements Codex-specific logic:
  - CLI invocation: `codex exec --json --full-auto --cd {project_root} "/bmad-dev-story {story_context}"`
  - **No bare mode** — Codex discovers project skills (`.agents/skills/`), `AGENTS.md`, and conventions natively
  - Skills invoked via native commands, mirrored from `.claude/skills/`
  - Model override: `--model {configured_model}`
  - Sandbox policy: `--sandbox workspace-write`
  - MCP config via `.codex/config.toml` (generated per session in project root)
  - Session resume: `codex resume {session_id}` for crash recovery
- **Native skill invocation** (same as Claude Code): Codex discovers `.agents/skills/` + `AGENTS.md` natively. Skills mirrored or symlinked from `.claude/skills/`. No inlined skill content, no system preamble.
- NDJSON output parsed into `SdkOutputEvent`
- Codex CLI availability validated at startup

**Estimated effort:** Medium (leverages infrastructure from 15.3 and patterns from 15.5)
**Risk:** 🟢 Low — follows established patterns from Claude Code integration

**Dependencies:** 15.2, 15.3, 15.4

---

#### Story 15.7: Pipeline Dual-Runtime Orchestration

As a daemon developer,
I want the multi-phase pipeline to route each phase to the appropriate runtime (API or SDK) based on the role's provider config,
So that mixed-mode configurations work seamlessly (e.g., dev via Claude Code, review via API).

**Acceptance Criteria:**

- `pipeline.rs` phase orchestration routes to `SessionRuntime::Api` or `SessionRuntime::Sdk` based on the provider configured for the phase's LLM role
- Phase mapping: `create-story` → dev role, `dev-story` → dev role, `code-review` → review role, consultations → as configured
- **Pipeline orchestration is unchanged** — same phases, same order, same Decision 10 consultation pattern:
  - Each phase (create-story, adversarial, critic, dev-story, code-review) is a session call
  - The daemon orchestrates phases identically regardless of runtime
  - API mode: session = `streaming_chat()` via rig, findings injected as user message
  - SDK mode: session = CLI subprocess, findings injected via `--resume {session_id}` with findings as prompt
  - No new MCP tools for consultations — daemon orchestrates them, not the skills
- WAL extended with `runtime_type: api | sdk` field and `sdk_session_ids: HashMap<String, String>` (phase → session_id) for correct recovery and resume routing
- Each pipeline phase's SDK session ID is persisted in WAL immediately after session start — required for consultation injection (`--resume {id}`) and crash recovery
- Recovery routing: API sessions attempt mid-session recovery (existing), SDK sessions attempt `--resume` (Claude Code) or `codex resume` (Codex) using persisted session IDs
- Shutdown propagation: SDK subprocess receives SIGTERM when ShutdownFlag is set
- UI events unified: both runtimes emit the same event types via `UiHandle`

**Estimated effort:** Large
**Risk:** 🟡 Medium — orchestration complexity, especially for mixed-mode and recovery paths

**Dependencies:** 15.1, 15.5 (or 15.6)

---

#### Story 15.8: Init Command SDK Provider Setup

As a new user,
I want `bmad-bot init` to guide me through SDK provider setup when I choose `claude-code` or `codex`,
So that configuration is correct and validated before first run.

**Acceptance Criteria:**

- Interactive init detects available CLIs (`claude --version`, `codex --version`) and suggests them as provider options
- Provider selection shows: `anthropic`, `openai-compatible`, `claude-code` (if CLI found), `codex` (if CLI found)
- For SDK providers: validates CLI version compatibility, confirms model availability
- Generates MCP supervisor config section when SDK providers are selected
- All provider types valid for all roles — no artificial restrictions
- `.env` template includes relevant API keys based on selected providers

**Estimated effort:** Small
**Risk:** 🟢 Low — extends existing interactive flow

**Dependencies:** 15.2

---

### Epic 15 Summary

| Story | Title | Dependencies | Effort | Risk |
|-------|-------|--------------|--------|------|
| 15.1 | Session Runtime Abstraction Layer | — | Medium | 🟢 |
| 15.2 | Config Extension for SDK Providers | — | Small | 🟢 |
| 15.3 | SDK Runtime Subprocess Infrastructure | 15.1 | Medium | 🟡 |
| 15.4 | Supervisor MCP Server | — | Large | 🟡 |
| 15.5 | Claude Code Provider Integration | 15.2, 15.3, 15.4 | Large | 🟡 |
| 15.6 | Codex Provider Integration | 15.2, 15.3, 15.4 | Medium | 🟢 |
| 15.7 | Pipeline Dual-Runtime Orchestration | 15.1, 15.5 or 15.6 | Large | 🟡 |
| 15.8 | Init Command SDK Provider Setup | 15.2 | Small | 🟢 |

**Skill discovery via BMAD manifest — replaces hardcoded paths:**
- The daemon reads `_bmad/_config/manifest.yaml` → `ides[]` to discover which IDEs/CLIs are installed
- IDE-to-skill-path mapping is a known convention:
  - `claude-code` → `.claude/skills/`
  - `codex` → `.agents/skills/`
- **API mode (rig):** uses the manifest to resolve skill paths dynamically instead of hardcoding `.claude/skills/`. Current hardcoded paths in `pipeline.rs`, `review/mod.rs`, `session/runner.rs` are replaced by a centralized `resolve_skill_path(skill_name)` that reads the manifest.
- **SDK mode:** the CLI discovers skills natively from its own directory, but the daemon still validates presence at startup.
- BMAD's installer places skills in the correct directory — the daemon does NOT manage skill installation.
- At startup, the daemon validates that the required skills exist at the resolved path for the configured provider. If skills are missing → fail fast: "BMAD skills not found for provider {provider}. Run the BMAD installer with {provider} support enabled."
- Multiple valid locations possible: if multiple IDEs are in the manifest, the daemon resolves for the provider's IDE. For API mode (rig), it uses the first available location from the manifest's `ides` list.

**Execution Strategy:**
- Stories 15.1, 15.2, and 15.4 can begin in parallel (no interdependencies)
- Story 15.3 depends on 15.1 (needs the runtime abstraction)
- Stories 15.5 and 15.6 can run in parallel once 15.3 and 15.4 are done
- Story 15.7 integrates everything
- Story 15.8 can start after 15.2 and be finalized after 15.5/15.6

**Critical path:** 15.1 → 15.3 → 15.5 → 15.7

---

## 5. Implementation Handoff

### Change Scope Classification: **Major**

This proposal introduces a new architectural concept (dual runtime) and two new infrastructure modules (SDK subprocess management, MCP server). It requires coordination across architecture documentation, config schema, pipeline orchestration, and CLI.

### Handoff Plan

| Role | Responsibility |
|------|---------------|
| **Architect (Winston)** | Update architecture document with Decisions 12 and 13, amend Decisions 1, 5, 8. Validate dual-runtime design. |
| **Developer (Amelia)** | Implement Epic 15 stories sequentially via the pipeline |
| **Product Manager** | Update PRD with FR60-FR64, validate MVP scope unchanged |

### Recommended Next Steps

1. **Approve this Sprint Change Proposal**
2. **Architecture update** — Run `bmad-agent-architect` to formalize Decisions 12 and 13, amend existing decisions
3. **Create Epics & Stories** — Add Epic 15 to `epics.md` with the stories defined above
4. **Sprint Planning** — Update `sprint-status.yaml` with Epic 15 stories
5. **Begin implementation** — Start with stories 15.1, 15.2, and 15.4 in parallel

### Success Criteria

- A story can be processed end-to-end using `provider: claude-code` with identical pipeline outcomes (branch, PR, notification) as API mode
- A story can be processed end-to-end using `provider: codex` with identical pipeline outcomes
- Mixed-mode configurations work (e.g., dev via Claude Code, review via API)
- Supervisor MCP server provides the same 3-tier cascade behavior as the rig tool
- Existing API mode is completely unaffected — zero regression
