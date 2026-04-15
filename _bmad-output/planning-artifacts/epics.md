---
stepsCompleted: ['step-01-validate-prerequisites', 'step-02-design-epics', 'step-03-create-stories', 'step-04-final-validation']
inputDocuments: ['_bmad-output/planning-artifacts/prd.md', '_bmad-output/planning-artifacts/architecture.md', '_bmad-output/planning-artifacts/architect-brief-mcp-client-integration.md', '_bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md']
---

# BMAD Bot - Epic Breakdown

## Overview

This document provides the complete epic and story breakdown for BMAD Bot, decomposing the requirements from the PRD and Architecture requirements into implementable stories.

## Requirements Inventory

### Functional Requirements

**Story Management**
- FR1: The daemon can detect stories with `backlog`, `ready-for-dev`, and `review` status by polling `sprint-status.yaml` at a configurable interval, routing each to the appropriate pipeline phase
- FR2: The daemon can resolve story dependencies and determine correct execution order
- FR3: The daemon can skip stories whose dependencies are not yet completed
- FR4: The daemon can mark dependent stories as `blocked` when a prerequisite story fails

**Pre-Development Preparation**
- FR5: The agent can review previously completed stories and their implementation before starting a new story
- FR6: The agent can update the current story's specs and acceptance criteria based on actual implementation of prior stories
- FR7: The agent can create and checkout a git branch following the `story/{epic}-{story}` naming convention

**Development Session**
- FR8: The daemon can instantiate a streaming rig agent session with a BMAD skill (`SKILL.md` loaded via Zed-style XML context as first user message), replacing the former persona-based activation
- FR9: The daemon can expose surgical development tools to the agent via rig tool calling: `read_file` (partial reading & outline mode), `edit_file` (search-replace surgical editing), `grep` (regex codebase search), `find_path` (glob-based file discovery), `list_directory` (directory listing), `git` (version control operations), `terminal` (shell command execution), `ask_supervisor` (supervision escalation), and `think` (rig's built-in ThinkTool, derived from Anthropic's Claude Think Tool pattern, for structured reasoning without consuming real tool calls). Tools follow the Claude Code / Zed agent-mode pattern for optimal token efficiency and code safety
- FR10: The agent can execute the full BMAD `dev-story` workflow autonomously
- FR11: The daemon can inject a session language override (English) via a minimal system preamble without modifying repo files

**Supervision**
- FR12: The supervisor can intercept agent questions during a development session
- FR13: The supervisor can answer predictable questions via a deterministic rule engine (confirmations, step-by-step detection, story selection)
- FR14: The supervisor can answer substantive questions via LLM fallback using full project documentation as context
- FR15: The supervisor can escalate to human when neither rules nor LLM can answer confidently
- FR16: The supervisor can log every decision with the question, chosen answer, reasoning, and alternatives considered
- FR17: The supervisor can commit a decisions file at `_bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md`

**Code Review**
- FR18: The daemon can optionally launch a code review using a separate LLM after the development session (configurable: enabled/disabled)
- FR19: When enabled, the review agent can commit fixes in a separate commit (distinct from dev commits)
- FR20: When enabled, the review agent can post its review as a comment on the PR

**Pull Request Management**
- FR21: The daemon can create a Pull Request on GitHub with an agent-written description
- FR22: The PR description includes a dedicated "Supervisor Decisions" section listing all decisions made during the session
- FR23: The daemon can create a PR for blocked/failed stories with partial code and a description of the failure
- FR24: When code review is disabled, the daemon proceeds directly to PR creation after the development session

**Notifications**
- FR25: The daemon can send Telegram notifications with run summaries (stories completed, blocked, errored)
- FR26: Notifications include story ID, status, and a link to the PR

**CLI & Configuration**
- FR27: The user can run `bmad-bot init` to interactively generate a project configuration file
- FR28: The user can run `bmad-bot start` to launch the daemon
- FR29: The user can run `bmad-bot status` to view current daemon state
- FR30: The user can run `bmad-bot logs` to view structured daemon logs
- FR31: The daemon can load configuration from a YAML file with secrets separated in a gitignored file
- FR32: The daemon can auto-discover BMAD version and installed modules from the project repo
- FR39: ~~REMOVED~~ — GitHub Copilot provider support removed in Sprint Change Proposal 2026-04-15
- FR40: The daemon logs all LLM requests and responses via a dedicated `llm_logging` module for debugging and operational visibility
- FR42: The daemon centralizes all LLM provider construction via an `AgentFactory` that returns a `BuiltAgent` with unified `stream_chat()` dispatch. Two providers supported: `anthropic` (Messages API) and `openai-compatible` (Responses API, with optional `base_url` for any compatible endpoint)

**Error Handling & Resilience**
- FR33: The daemon can handle LLM provider rate limits with retry and exponential backoff
- FR34: The daemon can handle cooperative shutdown on SIGTERM/SIGINT via a shared `ShutdownFlag` (Arc<AtomicBool>) propagated across pipeline → session → streaming chat layers. The flag can interrupt mid-streaming chunks and mid-tool-call loops, not just between steps. On shutdown: saves WAL state, commits partial work, notifies
- FR35: The daemon can notify the human of any blocking error (session crash, git failure, LLM provider down)
- FR36: The daemon can validate configuration at startup and report missing or invalid settings
- FR37: The daemon can detect an interrupted session at startup (presence of WAL file) and resume the session by reloading chat history and reconstructing the agent
- FR38: The daemon can detect a context window limit error during a session, summarize the history via a separate LLM call, and bootstrap a fresh session with compressed context to continue the work

**MCP Client Integration**
- FR44: The daemon can connect to external MCP servers at startup via the Model Context Protocol (stdio transport), perform the initialize handshake, and discover available tools via `list_tools()`
- FR45: The daemon can parse an optional `mcp_servers` configuration section defining for each server: name, command, arguments, transport, and enabled/disabled flag
- FR46: MCP server connection failures are non-blocking — the daemon logs a warning and continues with native tools only
- FR47: The daemon gracefully shuts down MCP server connections during cooperative shutdown (sends MCP `close` notification before killing child processes)
- FR48: The agent can use MCP-discovered tools (e.g., Playwright browser automation) identically to native tools during development sessions, via rig's built-in `McpTool` + `.rmcp_tools()` bridge

**Story Creation & Validation (Sprint Change 2026-04-15)**
- FR50: The daemon can invoke the `bmad-create-story` skill to create a story file from a `backlog` story, transitioning it to `ready-for-dev`
- FR51: The daemon can invoke the `bmad-review-adversarial-general` skill for adversarial story critique, with findings fed back to the active create-story session for correction
- FR52: The daemon can launch a Story Critic agent with persistent cross-story memory (`critic-memory.md`), project brief as vision anchor, and extended thinking for independent product/technical review
- FR53: The pipeline executes a linear flow per story with daemon-orchestrated consultations: create-story session (with adversarial + critic consultations) → dev-story → code-review (with critic consultation for decision-needed findings)
- FR54: The epic review agent (Winston) reads `deferred-work.md` and its own code analysis findings to propose pre-epic debt/improvement stories at epic boundaries
- FR55: A `spawn_agent` tool is available to all agent sessions for LLM-initiated sub-agent delegation (Zed-style: label, message, session_id for follow-up)
- FR56: The OpenAI-compatible provider supports an optional `base_url` configuration for any OpenAI-compatible endpoint (Ollama, LM Studio, vLLM, Groq, etc.)

### NonFunctional Requirements

**Security**
- NFR-SEC1: API keys and tokens never stored in committed config — secrets loaded from gitignored `.env` or secrets file
- NFR-SEC2: Secrets never logged by `tracing` — structured logging filters sensitive fields
- NFR-SEC3: Git credentials from environment, never hardcoded

**Integration**
- NFR-INT1: LLM provider connection failures and unexpected responses handled without crashing
- NFR-INT2: GitHub API rate limiting (5000 req/hour authenticated) handled with retry
- NFR-INT3: Telegram API failures do not block the pipeline — logged but do not stop story processing

**Reliability**
- NFR-REL1: Transient LLM errors (timeouts, 500s, rate limits) recovered with exponential backoff, max 3 retries per call
- NFR-REL2: No work lost on unexpected shutdown — SIGTERM/SIGINT triggers cooperative shutdown via shared AtomicBool flag, saves WAL state, commits partial work, notifies
- NFR-REL3: Crash recovery produces clean state — no corrupted branches, no half-committed files. Watcher re-reads `sprint-status.yaml` and resumes
- NFR-REL4: All errors logged via `tracing::error!()` with full context (story_id, step, error details)

**Scalability (Future — v2/v3)**
- NFR-SCA1: MVP: single daemon per project, sequential execution. No scaling requirements.
- NFR-SCA2: Future: master daemon orchestrating workers, story parallelization, Kubernetes deployment. MVP architecture decisions must not preclude this evolution.

### Additional Requirements

**From Architecture — Starter/Foundation:**
- Project initialized via `cargo init bmad-bot` + curated dependencies (no template framework)
- Rust edition 2024, single binary target, full async tokio runtime (`features = ["full"]`)
- Single crate with modular directory structure (not a Cargo workspace for MVP)
- CLI framework: clap with derive API (init, start, status, logs subcommands)
- Config loading: serde + serde_yaml for YAML, dotenvy for `.env` secrets
- Signal handling: cooperative shutdown via shared `ShutdownFlag` (Arc<AtomicBool>) propagated across pipeline → session → streaming chat layers, can interrupt mid-streaming and mid-tool-call loops

**From Architecture — Core Decisions:**
- Decision 1 — Supervisor Interception: Hybrid Chat Loop + `ask_supervisor` rig Tool. Chat loop handles workflow-level interaction; supervisor tool handles technical/business questions. Rule engine → LLM fallback → human escalation.
- Decision 2 — Sprint-Status Mutation: Daemon is pure reader. All mutations performed by the BMAD agent via tools.
- Decision 3 — Session State Persistence: WAL file (`_bmad-output/implementation-artifacts/.bmad-bot-session.yaml`) persisted after each chat turn. Crash recovery reloads history. Context limit recovery summarizes history and bootstraps fresh session.
- Decision 4 — Error Propagation: Three-tier layered. Layer 1 (HTTP transport): reqwest-middleware auto-retry. Layer 2 (Tools): domain-specific handling + bubble-up. Layer 3 (Session/Daemon): commit partial work, create PR with failure, notify.
- Decision 5 — Agent Prompt Composition: Minimal system preamble with operational instructions and language override. BMAD dev agent file sent as first user message wrapped in Zed-style XML context tags (`<context><files>`) via `activate_agent()`. Agent executes activation steps via tools before receiving commands. First command message: `"DS"`. New `llm_context` module with `ContextBuilder` handles XML formatting (adaptive backtick fencing, absolute path resolution, multi-file support, line ranges).
- Decision 6 — Deployment Model: Foreground process via `bmad-bot start`. No self-daemonization. Logs to stdout/stderr.

**From Architecture — Implementation Patterns (mandatory for all stories):**
- Error Type Pattern: Per-module `thiserror` enums. `anyhow` only in `main.rs` / CLI layer.
- Rig Tool Pattern: Standard structure (serializable struct + dedicated args struct + dedicated error enum + Tool trait impl).
- Tracing Pattern: Structured spans with `story_id` context. Never `println!`/`eprintln!`. Dedicated `llm_logging` module logs all LLM requests and responses for debugging and operations visibility.
- Config Pattern: Validate once at startup, share via `Arc<BotConfig>`. Secrets loaded separately from `.env`.
- Git Provider Trait Pattern: Params and returns as dedicated structs. Async trait methods.
- Test Mock Pattern: Deterministic LLM responses. Arrange-Act-Assert. Naming: `test_{module}_{behavior}_{scenario}`.

**From Architecture — External Integration Points:**
- LLM Providers (Anthropic, OpenAI, GitHub Copilot) via rig-core with streaming API (`stream_chat()`) — API key from `.env`. OpenAI uses Responses API; Copilot uses Completions API with required IDE-specific headers (Editor-Version, Copilot-Integration-Id, User-Agent)
- GitHub API via octocrab — Token from `.env`
- GitLab API via reqwest — Token from `.env`
- Telegram API via reqwest — Bot token from `.env`
- Git CLI (>= 2.30) via `tokio::process::Command` / `std::process::Command` — inherits user's full git configuration (SSH agent, credential manager, osxkeychain, commit signing, `.gitconfig` identity). System dependency validated at daemon startup. Replaces former `git2` (libgit2) embedded library
- Local filesystem via std::fs / tokio::fs
- Local terminal via tokio::process

**From Architecture — Anti-Patterns (NEVER do these):**
- No `unwrap()` or `expect()` in production code
- No `anyhow::Result` in library modules
- No loose primitives as function params when 3+ params exist
- No logging of sensitive data (API keys, tokens, passwords)
- No real LLM API calls in unit tests
- No skipping doc comments on public items

### FR Coverage Map

- FR1: Epic 2 + Epic 13 — Detect backlog/ready-for-dev/review stories by polling sprint-status.yaml
- FR2: Epic 2 — Resolve story dependencies and execution order
- FR3: Epic 2 — Skip stories with unmet dependencies
- FR4: Epic 2 — Cascade blocked status to dependent stories
- FR5: Epic 4 — Review previously completed stories before starting new one
- FR6: Epic 4 — Update current story specs based on prior implementations
- FR7: Epic 4 — Create and checkout git branch (story/{epic}-{story})
- FR8: Epic 4 + Epic 12 — Instantiate streaming rig agent session with BMAD skill (SKILL.md via Zed-style XML context)
- FR9: Epic 4 (baseline) + Epic 8 (refactoring) — Expose surgical development tools via rig tool calling (read_file, edit_file, grep, find_path, list_directory, git, terminal, ask_supervisor, think)
- FR10: Epic 4 — Execute full BMAD dev-story workflow autonomously
- FR11: Epic 4 — Inject session language override (English) via minimal system preamble
- FR12: Epic 3 — Intercept agent questions during development session
- FR13: Epic 3 — Answer predictable questions via deterministic rule engine
- FR14: Epic 3 — Answer substantive questions via LLM fallback with project docs context
- FR15: Epic 3 — Escalate to human when neither rules nor LLM can answer confidently
- FR16: Epic 3 — Log every decision with question, answer, reasoning, and alternatives
- FR17: Epic 3 — Commit decisions file at implementation-artifacts path
- FR18: Epic 5 — Optionally launch code review using separate LLM (configurable)
- FR19: Epic 5 — Review agent commits fixes in separate commit
- FR20: Epic 5 — Review agent posts review as PR comment
- FR21: Epic 5 — Create Pull Request on GitHub with agent-written description
- FR22: Epic 5 — PR description includes Supervisor Decisions section
- FR23: Epic 5 — Create PR for blocked/failed stories with partial code and failure description
- FR24: Epic 5 — Proceed directly to PR creation when code review is disabled
- FR25: Epic 6 — Send Telegram notifications with run summaries
- FR26: Epic 6 — Notifications include story ID, status, and PR link
- FR27: Epic 1 — Run bmad-bot init for interactive config generation
- FR28: Epic 1 — Run bmad-bot start to launch daemon
- FR29: Epic 1 — Run bmad-bot status to view daemon state
- FR30: Epic 1 — Run bmad-bot logs to view structured logs
- FR31: Epic 1 — Load config from YAML with secrets separated in gitignored file
- FR32: Epic 1 — Auto-discover BMAD version and installed modules
- FR33: Epic 6 — Handle LLM provider rate limits with retry and exponential backoff
- FR34: Epic 1 — Handle cooperative shutdown on SIGTERM/SIGINT via shared AtomicBool flag (can interrupt mid-streaming and mid-tool-call loops)
- FR35: Epic 6 — Notify human of any blocking error
- FR36: Epic 1 — Validate configuration at startup and report issues
- FR37: Epic 6 — Detect interrupted session at startup (WAL file) and resume
- FR38: Epic 6 — Detect context window limit error and bootstrap fresh session with compressed context
- FR39: ~~Epic 1 — REMOVED~~ — Copilot OAuth removed
- FR40: Epic 1/6 — Log all LLM requests and responses via dedicated llm_logging module for debugging and operations visibility
- FR41: Epic 4 (Story 4.4) — Validate git CLI availability at startup and fail fast if missing
- FR42: Epic 4 (Story 4.5) + Epic 11 — AgentFactory with BuiltAgent enum. Simplified to Anthropic + OpenAI-compatible with optional base_url
- FR43: Epic 4 (Story 4.6) — Post-implementation impact analysis: agent analyzes downstream dependent stories after completion and updates their Previous Story Intelligence sections. Best-effort, non-blocking
- FR44: Epic 9 — Connect to external MCP servers at startup, perform handshake, discover tools
- FR45: Epic 9 — Parse optional `mcp_servers` config section
- FR46: Epic 9 — MCP connection failures are non-blocking
- FR47: Epic 9 — Graceful MCP shutdown during cooperative shutdown
- FR48: Epic 9 — Agent uses MCP-discovered tools identically to native tools via rig's `McpTool`
- FR49: Epic 10 — Display structured, user-facing terminal output in foreground mode (spinners, pipeline phases, tool calls, LLM status) via UiRenderer trait with ConsoleRenderer (indicatif + console) and NullRenderer (tests/CI). Configurable via ui_mode in bmad-bot.yaml
- FR50: Epic 13 — Invoke bmad-create-story skill to create story files from backlog stories
- FR51: Epic 13 — Adversarial review with findings fed back to active create-story session
- FR52: Epic 13 — Story Critic with persistent memory and project brief as vision anchor
- FR53: Epic 13 — Linear pipeline with daemon-orchestrated consultations (create→dev→review)
- FR54: Epic 14 — Epic review reads deferred-work.md and proposes pre-epic debt stories
- FR55: Epic 12 — spawn_agent tool for LLM-initiated sub-agent delegation in all sessions
- FR56: Epic 11 — OpenAI-compatible provider with optional base_url for any compatible endpoint

## Epic List

### Epic 1: Project Foundation & CLI
The user can install, configure, launch, and monitor the BMAD Bot daemon. This epic delivers the complete CLI interface (init, start, status, logs), configuration loading with secrets separation, BMAD auto-discovery, config validation, cooperative shutdown via shared AtomicBool flag (can interrupt mid-streaming and mid-tool-call loops), smart git auto-detection during setup, and GitHub Copilot OAuth Device Flow authentication (using the Completions API with IDE-specific headers) for zero-friction LLM provider onboarding. After this epic, the daemon runs, stops cleanly, and the user can observe its state.
**FRs covered:** FR27, FR28, FR29, FR30, FR31, FR32, FR34, FR36, FR39, FR40

### Epic 2: Story Watching & Dependency Management
The daemon automatically detects stories with ready-for-dev status by polling sprint-status.yaml, resolves dependency order, skips blocked stories, and cascades blocked status to dependents. After this epic, the daemon knows WHAT to work on and in what order.
**FRs covered:** FR1, FR2, FR3, FR4

### Epic 3: Intelligent Supervision
The supervisor can intercept agent questions and answer them via a deterministic rule engine or a dedicated BMAD Architect agent session (multi-turn chat with full project context loaded autonomously), escalate to human when unsure, and log every decision with reasoning and alternatives to a committed decisions file. After this epic, the ask_supervisor rig tool is built, tested, and ready to be registered with the agent.
**FRs covered:** FR12, FR13, FR14, FR15, FR16, FR17

### Epic 4: Autonomous Development Session
The daemon launches a streaming rig agent session with the BMAD dev agent persona (activated via Zed-style XML context, not system preamble) and registered tools (git, filesystem, terminal, ask_supervisor, think). The agent reviews prior stories, updates specs, creates a branch, and executes the full dev-story workflow autonomously with English language override via minimal system preamble. After story completion, the agent propagates implementation reality forward to downstream dependent stories via post-implementation impact analysis. The epic concludes with an autonomous review gate: upon epic completion, the daemon pauses, launches an Architect-persona LLM session to analyze the codebase, produces a structured report with functional testing guide, and blocks the next epic until the human validates. *(Note: Story 4.1's monolithic FsTool is refactored into surgical tools in Epic 8. Story 4.4 migrates all git operations from git2 to Git CLI. Story 4.6 adds the post-impl impact analysis step. Story 4.8 adds the epic gate with autonomous retrospective review.)*
**FRs covered:** FR5, FR6, FR7, FR8, FR9, FR10, FR11, FR41, FR43

### Epic 5: Code Review & Pull Request Delivery [L201-205]

*Note: Story 5.4 added post-sprint to enrich PR descriptions with agent-generated context.*
The daemon optionally launches a code review via a separate LLM after the dev session, with fixes in separate commits and review posted as a PR comment. It creates a Pull Request on GitHub with an agent-written description including a Supervisor Decisions section. PRs are also created for blocked/failed stories with partial code and failure context. After this epic, the user wakes up to PRs ready for human review.
**FRs covered:** FR18, FR19, FR20, FR21, FR22, FR23, FR24

### Epic 6: Notifications & Error Resilience
The daemon sends Telegram notifications with story status, ID, and PR links. It handles LLM rate limits with retry/backoff, notifies the human of blocking errors, detects interrupted sessions via WAL file for crash recovery, and recovers from context window limit errors by summarizing history and bootstrapping a fresh session. After this epic, the user can trust the daemon to run overnight without supervision.
**FRs covered:** FR25, FR26, FR33, FR35, FR37, FR38, FR40

### Epic 7: Integration Tests ⚠️ BLOCKED by Epic 8
All 6 functional epics have been implemented and pass 573 unit tests. This epic introduces integration tests that validate the interactions between modules at their boundaries — ensuring the daemon works as a cohesive system, not just as isolated pieces. These tests are deterministic (no real LLM calls), run in CI, and use mocked external dependencies.
**Depends on:** Epic 8 (complete — all integration tests must target the post-refactoring tool surface)

### Epic 8: Surgical Development Tooling
Replace the monolithic FsTool with focused, Claude Code-style tools to dramatically improve agent token efficiency, code safety, and codebase navigation. After this epic, the dev agent edits files surgically instead of rewriting them, searches code with grep, and navigates with outlines — matching the capability level of modern AI coding assistants. This is a refactoring of Epic 4 Story 4.1's FsTool implementation, not greenfield work.
**FRs covered:** FR9
**Depends on:** Epic 4 (Story 4.1 as baseline)

### Epic 9: MCP Client Integration — Dynamic External Tool Discovery
Connect to external MCP servers at daemon startup, discover their tools, and expose them to the rig agent alongside native tools — leveraging rig's built-in `McpTool` and `.rmcp_tools()` support. The autonomous agent gains browser automation (Playwright) and any future MCP-compatible tooling without custom tool implementations. Zero code changes to add a new MCP server — just a config entry.
**FRs covered:** FR44, FR45, FR46, FR47, FR48
**Depends on:** Epic 4 (AgentFactory + tool registration infrastructure)

### Epic 10: Terminal UI & Developer Experience
The daemon displays structured, user-facing terminal output in foreground mode (tmux, screen, interactive terminal) — replacing raw tracing logs on stdout with hierarchical progress indicators, pipeline phase tracking, agent tool call visibility, and LLM interaction status. Powered by `indicatif` (spinners, progress) and `console` (colors, styles) behind a `UiRenderer` trait that enables future migration to `iocraft` or `ratatui` without modifying business code. Debug logs remain in the JSON log file only. After this epic, the user can follow daemon progress in real-time without reading raw debug logs.
**FRs covered:** FR49
**Depends on:** Epics 1-6 (all implemented — retroactive UI event insertion)

### Epic 11: Copilot Removal & Provider Simplification
The daemon supports only Anthropic and OpenAI-compatible providers (with optional `base_url` for any compatible endpoint). All GitHub Copilot code, authentication, and the rig fork are removed. The `AgentFactory` is simplified to two provider variants. After this epic, the codebase is leaner and uses the official `rig-core` crate.
**FRs covered:** FR39 (removed), FR42 (modified), FR56

### Epic 12: Skill-Based Sessions & SpawnAgent Tool
The daemon activates agent sessions by loading BMAD skill files (`SKILL.md`) via the existing Zed-style XML context mechanism instead of persona files. The `ResponseAnalyzer` is simplified (no more menu/persona auto-response). A universal `spawn_agent` tool (Zed-inspired) is available in all sessions for LLM-initiated sub-agent delegation. After this epic, the bot speaks the BMAD v6.2+ skill language natively.
**FRs covered:** FR8 (modified), FR55

### Epic 13: Multi-Phase Pipeline & Story Critic
The pipeline orchestrates the full story lifecycle from `backlog` to `done`. For each story: a create-story session runs (with daemon-orchestrated adversarial and critic consultations fed back into the active session), then a dev-story session, then a code-review session (with critic consultation for decision-needed findings). The Story Critic is an independent vision guardian with persistent memory across stories, anchored by a project brief provided at init. After this epic, the bot autonomously creates, validates, implements, and reviews stories end-to-end.
**FRs covered:** FR1 (modified), FR50, FR51, FR52, FR53

### Epic 14: Epic Review Enhancement & Deferred Work
The epic review agent (Winston) reads `deferred-work.md` and combines it with findings from its own code analysis to propose pre-epic cleanup/improvement stories. These are injected at the head of the next epic in `sprint-status.yaml` as `backlog` stories with convention `X-0-pre-epic-X-{slug}`. Processed debt items are purged from `deferred-work.md`. After this epic, technical debt is managed rhythmically at epic boundaries.
**FRs covered:** FR54

---

## Epic 1: Project Foundation & CLI

The user can install, configure, launch, and monitor the BMAD Bot daemon. This epic delivers the complete CLI interface (init, start, status, logs), configuration loading with secrets separation, BMAD auto-discovery, config validation, and graceful shutdown.

### Story 1.1: Project Scaffolding, Configuration & Validation

As a developer,
I want to initialize the BMAD Bot project with a complete module structure and robust configuration loading,
So that I have a solid foundation to build all daemon features on.

**Acceptance Criteria:**

**Given** the project does not yet exist
**When** I run `cargo init bmad-bot` and set up the project
**Then** a Rust project is created with edition 2024, single binary target, and all required dependencies in Cargo.toml (tokio, rig-core, git2, serde, serde_yaml, dotenvy, clap, thiserror, tracing, tracing-subscriber, octocrab, reqwest, reqwest-middleware, reqwest-retry, async-trait)
**And** the complete module directory structure is scaffolded with stub mod.rs files for all modules (cli, config, watcher, session, supervisor, review, tools, git_provider, notifier)

**Given** a valid `bmad-bot.yaml` configuration file exists in the project root
**When** the config module loads the file
**Then** a `BotConfig` struct is deserialized via serde_yaml containing all configuration fields (polling_interval_secs, git_provider, llm providers/models, notification config, BMAD paths)
**And** secrets are loaded separately from `.env` via dotenvy and never stored in `BotConfig`

**Given** the project HTTP client is initialized
**When** any module needs to make external HTTP calls (LLM providers, GitHub/GitLab API, Telegram API)
**Then** a shared `reqwest` client is configured with `reqwest-middleware` and `reqwest-retry` for automatic retry with exponential backoff (max 3 retries) on transient errors (429, 500, 503, timeouts)
**And** the retry client is available to all modules from project inception — no HTTP call in any epic runs without retry resilience

**Given** a `bmad-bot.yaml` with missing or invalid fields
**When** the config module validates the configuration
**Then** a descriptive `ConfigError` (thiserror enum) is returned specifying exactly which field failed and why
**And** `ConfigError` follows the per-module thiserror pattern (no anyhow in library modules)

**Given** the project is initialized
**When** I inspect the repository
**Then** `bmad-bot.yaml.example` and `.env.example` template files exist and are committed
**And** `.env` is listed in `.gitignore`

### Story 1.2: CLI Framework & Daemon Lifecycle

As a developer,
I want to launch the daemon with `bmad-bot start` and have it run with structured logging and clean shutdown,
So that I have a controllable long-running process as the foundation for all pipeline features.

**Acceptance Criteria:**

**Given** the project has the clap dependency configured
**When** I build the CLI module
**Then** clap with derive API defines four subcommands: `init`, `start`, `status`, `logs`
**And** each subcommand has auto-generated `--help` documentation

**Given** a valid configuration file exists
**When** I run `bmad-bot start`
**Then** the daemon loads and validates the config, initializes structured tracing (JSON or pretty-print based on config) to stdout/stderr, and enters a polling loop (placeholder that sleeps for the configured interval)
**And** tracing is the only logging mechanism — no `println!` or `eprintln!` anywhere
**And** sensitive fields (API keys, tokens) are never present in log output

**Given** the daemon is running
**When** a SIGTERM or SIGINT signal is received
**Then** the daemon initiates graceful shutdown via tokio::signal, logs the shutdown event, and exits cleanly with code 0
**And** no partial state is left behind

### Story 1.3: Interactive Init Command

As a developer setting up BMAD Bot for the first time,
I want to run `bmad-bot init` and be guided through an interactive setup,
So that I can generate a valid configuration without manually writing YAML.

**Acceptance Criteria:**

**Given** I am in a project directory without existing BMAD Bot configuration
**When** I run `bmad-bot init`
**Then** interactive prompts ask for: repository path, LLM provider and model for each role (dev, review, supervisor), git provider (GitHub/GitLab), Telegram notification config, and polling interval

**Given** I have completed all interactive prompts
**When** the init command finishes
**Then** a `bmad-bot.yaml` file is generated with all user-provided settings (no secrets in this file)
**And** a `.env` file is generated with placeholder entries for all required secrets (API keys, tokens) with comments explaining each

**Given** a `bmad-bot.yaml` already exists in the directory
**When** I run `bmad-bot init`
**Then** the user is warned that existing config will be overwritten and asked to confirm before proceeding

### Story 1.4: Status, Logs & BMAD Discovery

As a developer operating BMAD Bot,
I want to check the daemon's state, review logs, and have BMAD auto-detected,
So that I can monitor operations and trust the daemon knows my project setup.

**Acceptance Criteria:**

**Given** the daemon is running or has run previously
**When** I run `bmad-bot status`
**Then** a summary is displayed showing: current state (running/stopped), stories processed count, stories in progress, stories blocked, and last activity timestamp

**Given** the daemon has been running with structured tracing
**When** I run `bmad-bot logs`
**Then** structured logs are displayed with story_id, timestamps, and action fields
**And** logs can be filtered by level (info, warn, error)

**Given** the daemon starts in a project with BMAD installed
**When** the config module initializes
**Then** the daemon auto-discovers the BMAD version and installed modules by scanning the project repo (e.g., `_bmad/` directory structure)
**And** the discovered information is logged at startup and available via `bmad-bot status`

### Story 1.5: Git Remote Auto-Detection in Init Command

As a developer setting up BMAD Bot for the first time,
I want the `bmad-bot init` command to auto-detect my git provider, repository owner, repository name, and default branch from the local `.git` configuration,
So that I can complete the setup faster with fewer manual inputs and zero risk of typos on repository information.

**Acceptance Criteria:**

**Given** I am in a directory with an initialized git repository that has an `origin` remote configured
**When** I run `bmad-bot init`
**Then** the git provider, repo owner, repo name, and target branch are auto-detected from the `origin` remote URL
**And** a summary of detected values is displayed for confirmation before proceeding

**Given** the auto-detected git settings are displayed
**When** I confirm them (default: Yes)
**Then** the init command skips the individual git provider/owner/repo/branch prompts and uses the detected values

**Given** the auto-detected git settings are displayed
**When** I decline them
**Then** the init command falls back to the standard manual prompts for git provider, repo owner, repo name, and target branch (existing Story 1.3 behavior)

**Given** I am in a directory without a `.git` directory or without any remote configured
**When** I run `bmad-bot init`
**Then** the init command silently skips auto-detection and falls back to manual prompts without any error message

**Given** the `origin` remote URL uses SSH format (`git@github.com:owner/repo.git`) or HTTPS format (`https://github.com/owner/repo.git`)
**When** auto-detection runs
**Then** the provider, owner, and repo name are correctly parsed from either format

**Given** the `origin` remote URL points to an unrecognized host (not `github.com` or `gitlab.com`)
**When** auto-detection runs
**Then** the owner and repo name are still pre-filled
**And** the git provider prompt falls back to manual selection with a note that the host was not recognized

**Given** the repository has multiple remotes but no `origin`
**When** auto-detection runs
**Then** the available remote names are listed and the user is prompted to select one

**Given** auto-detection successfully identifies git settings
**When** the final `bmad-bot.yaml` is generated
**Then** the generated config contains the correct git_provider, repo_owner, repo_name, and target_branch values identical to what was confirmed by the user

### Story 1.6: GitHub Copilot OAuth Device Flow Authentication

As a developer setting up BMAD Bot,
I want to authenticate with GitHub Copilot via an OAuth Device Flow when I choose `github-copilot` as my LLM provider,
So that I can get a token automatically without manually creating a Personal Access Token, and the daemon can transparently exchange it for short-lived Copilot session tokens at runtime.

**References:**
- OAuth Device Flow: [RFC 8628](https://datatracker.ietf.org/doc/html/rfc8628)
- Implementation reference: [openclaw github-copilot-auth.ts](https://github.com/openclaw/openclaw/blob/main/src/providers/github-copilot-auth.ts), [openclaw github-copilot-token.ts](https://github.com/openclaw/openclaw/blob/main/src/providers/github-copilot-token.ts)
- Covers: **FR39** (GitHub Copilot OAuth Device Flow authentication)

**Acceptance Criteria:**

---

**Part 1 — Rename `github-models` → `github-copilot` (do this first)**

**Given** all existing code references to `github-models` and `GITHUB_MODELS_API_KEY`
**When** this story is implemented
**Then** every occurrence of `github-models` is replaced with `github-copilot` across the following files:
- `src/cli/mod.rs` — `LLM_PROVIDERS`, `default_model_for_provider`, `generate_env_file`, and all tests referencing `github_models`
- `src/config/mod.rs` — `VALID_LLM_PROVIDERS`, `BotSecrets.github_models_api_key` → `BotSecrets.github_copilot_oauth_token`, `load()`, `validate_for_config`, and all tests
- `src/session/provider.rs` — `resolve_api_key` match arm and all tests
- `src/session/runner.rs` — `run()` match arm for `"github-models"`
- `src/supervisor/architect.rs` — `env_var_for_provider` match arm and tests
- `bmad-bot.yaml.example` — provider comments
- `README.md` — all references to `github-models` provider
- `_bmad-output/project-context.md` — multi-provider LLM config section and external integration points
**And** `GITHUB_MODELS_API_KEY` is replaced with `GITHUB_COPILOT_OAUTH_TOKEN` everywhere
**And** `default_model_for_provider("github-copilot")` returns `"gpt-4o"`
**And** all existing tests compile and pass with the renamed provider

---

**Part 2 — OAuth Device Flow in `bmad-bot init`**

**Given** the LLM provider list in `bmad-bot init`
**When** I view the available providers
**Then** the options are `anthropic`, `openai`, and `github-copilot`

**Given** I select `github-copilot` as an LLM provider for one or more roles during `bmad-bot init`
**When** all three LLM role selections (dev, review, supervisor) are complete
**Then** the GitHub Copilot OAuth Device Flow is triggered exactly once, regardless of how many roles use `github-copilot`
**And** a device code is requested from `https://github.com/login/device/code` with client ID `Iv1.b507a08c87ecfe98` and scope `read:user`
**And** the terminal displays the verification URL and user code for me to authorize in my browser
**And** the init flow polls `https://github.com/login/oauth/access_token` for the token with the interval specified by GitHub's response

**Given** no role uses `github-copilot` as its provider
**When** all three LLM role selections are complete
**Then** the Device Flow is not triggered at all

**Given** I authorize the device in my browser
**When** the polling receives a valid access token
**Then** the OAuth token is stored in memory and pre-filled as `GITHUB_COPILOT_OAUTH_TOKEN=<token>` in the generated `.env` file
**And** the init flow continues normally with remaining configuration steps (notifications, daemon settings)

**Given** the Device Flow is polling for authorization
**When** GitHub responds with `slow_down`
**Then** the polling interval is increased by 2 seconds as per the OAuth spec

**Given** the Device Flow is polling for authorization
**When** the device code expires (GitHub responds with `expired_token`)
**Then** an error message is displayed explaining the code expired
**And** `GITHUB_COPILOT_OAUTH_TOKEN=` is written empty in `.env` with a comment instructing the user to re-run init or obtain a token manually
**And** the init flow continues without aborting

**Given** the Device Flow is polling for authorization
**When** I cancel the authorization in the browser (GitHub responds with `access_denied`)
**Then** an error message is displayed explaining the authorization was denied
**And** `GITHUB_COPILOT_OAUTH_TOKEN=` is written empty in `.env`
**And** the init flow continues without aborting

**Given** the terminal is not interactive (no TTY)
**When** `github-copilot` is configured as a provider
**Then** the Device Flow is skipped with a warning message
**And** `GITHUB_COPILOT_OAUTH_TOKEN=` is written empty in `.env` with instructions to obtain the token manually

---

**Part 3 — Runtime Copilot Token Exchange and Caching**

**Given** the daemon starts with `bmad-bot start` and `github-copilot` is configured as a provider
**When** `BotSecrets` loads secrets from the environment
**Then** `GITHUB_COPILOT_OAUTH_TOKEN` is loaded
**And** validation fails with a descriptive error if the token is missing or empty

**Given** a session is about to run with the `github-copilot` provider
**When** the daemon needs an API token for the LLM client
**Then** it exchanges the long-lived OAuth token for a short-lived Copilot session token by calling `GET https://api.github.com/copilot_internal/v2/token` with `Authorization: Bearer {oauth_token}`
**And** the response `{ token: string, expires_at: number }` is parsed and cached in memory
**And** if the exchange fails (HTTP error, missing fields), the session fails with a descriptive `ProviderError`

**Given** a cached Copilot session token exists in memory
**When** a new session is about to start
**Then** the daemon checks whether the cached token is still valid (with a 5-minute safety margin before expiry)
**And** if valid, the cached token is reused without making a new exchange request
**And** if expired or within the safety margin, a fresh token is obtained via the exchange endpoint

**Given** a valid Copilot session token has been obtained
**When** the token contains a `proxy-ep=<host>` field (semicolon-delimited key-value pairs)
**Then** the base URL is derived by extracting the `proxy-ep` value, stripping the protocol, replacing `proxy.` prefix with `api.`, and prepending `https://`
**And** if no `proxy-ep` is found, the default base URL `https://api.individual.githubcopilot.com` is used

**Given** the Copilot session token and derived base URL are resolved
**When** the agent is built
**Then** it uses the OpenAI-compatible client with the dynamically derived base URL and the Copilot session token (not the OAuth token) as the API key

---

**Part 4 — Module Structure and New Files**

**Given** the new `src/auth/` module
**When** I inspect the project structure
**Then** the following files exist:
- `src/auth/mod.rs` — `pub mod github_copilot;`
- `src/auth/github_copilot.rs` — Device Flow functions (`request_device_code()`, `poll_for_access_token()`, `run_device_flow()`) and Copilot token exchange functions (`exchange_copilot_token()`, `derive_base_url_from_token()`, `CopilotTokenCache` struct with `resolve()` method)
- `src/main.rs` — `mod auth;` added

**Given** the auth module depends on HTTP calls
**When** the module is designed
**Then** HTTP calls are abstracted behind an `async` trait (e.g. `CopilotHttpClient`) to enable deterministic mocking in unit tests, consistent with the project's existing mock patterns (no external mock crate required)

---

**Part 5 — Unit Tests**

**Given** the `src/auth/github_copilot.rs` module
**When** I inspect the unit tests
**Then** the following tests exist with trait-based HTTP mocks (no real network calls):

*Device Flow tests:*
- `test_request_device_code_success` — mock HTTP 200 with valid JSON, verify parsed fields
- `test_request_device_code_http_error` — mock HTTP 500, verify error
- `test_request_device_code_missing_fields` — mock HTTP 200 with incomplete JSON, verify error
- `test_poll_authorization_pending_then_success` — mock sequential responses (`authorization_pending` × N, then `access_token`), verify final token
- `test_poll_slow_down_increases_interval` — verify interval increases by 2 seconds on `slow_down` response
- `test_poll_expired_token_returns_error` — mock `expired_token`, verify error
- `test_poll_access_denied_returns_error` — mock `access_denied`, verify error

*Token exchange tests:*
- `test_exchange_copilot_token_success` — mock valid exchange response, verify token and expiry parsed
- `test_exchange_copilot_token_http_error` — mock HTTP 401/403, verify error
- `test_exchange_copilot_token_missing_fields` — mock incomplete response, verify error
- `test_copilot_token_cache_returns_cached_when_valid` — verify no HTTP call when cache is fresh
- `test_copilot_token_cache_refreshes_when_expired` — verify HTTP call when cache is stale
- `test_derive_base_url_from_proxy_ep` — verify `proxy.example.com` → `https://api.example.com`
- `test_derive_base_url_fallback_when_no_proxy_ep` — verify default `https://api.individual.githubcopilot.com`
- `test_derive_base_url_strips_protocol_from_proxy_ep` — verify `https://proxy.foo.bar` → `https://api.foo.bar`

---

## Epic 2: Story Watching & Dependency Management

The daemon automatically detects stories with ready-for-dev status by polling sprint-status.yaml, resolves dependency order, skips blocked stories, and cascades blocked status to dependents. The daemon is a pure reader — it never writes to sprint-status.yaml.

### Story 2.1: Sprint-Status Polling & Story Detection

As a developer with stories marked ready-for-dev,
I want the daemon to automatically detect them by polling sprint-status.yaml,
So that stories are picked up for processing without manual intervention.

**Acceptance Criteria:**

**Given** a valid `sprint-status.yaml` exists at the configured output path
**When** the watcher module polls the file at the configured interval (default 5 min), polling immediately on startup (no initial wait)
**Then** all stories with status `ready-for-dev` are identified and returned as `StoryInfo` structs (id, label, branch name, specs path, dependencies, status)
**And** the polling interval is configurable via `bmad-bot.yaml`

**Given** the `sprint-status.yaml` file does not exist or is malformed
**When** the watcher attempts to read it
**Then** a descriptive `WatcherError` (thiserror enum) is returned
**And** the error is logged via `tracing::error!()` with full context
**And** the daemon continues polling on the next cycle (does not crash)

**Given** no stories have `ready-for-dev` status
**When** the watcher polls
**Then** the watcher logs an info message and sleeps until the next polling cycle

### Story 2.2: Dependency Resolution & Execution Order

As a developer with interdependent stories,
I want the daemon to resolve dependencies and determine the correct execution order,
So that stories are processed in a sequence that respects their prerequisites.

**Acceptance Criteria:**

**Given** the watcher has detected multiple `ready-for-dev` stories
**When** the dependency resolution module (`deps.rs`) processes them
**Then** a directed acyclic graph of dependencies is computed in-memory
**And** stories are returned in topological order (prerequisites first)

**Given** a story has dependencies that are not yet in `done` status
**When** the pre-gate logic evaluates it
**Then** the story is skipped for this cycle (not marked, not modified — pure read)
**And** a tracing info message logs which story was skipped and which dependency is unmet

**Given** a story has all dependencies in `done` status
**When** the pre-gate logic evaluates it
**Then** the story is marked as eligible and included in the execution queue

**Given** a circular dependency exists in sprint-status.yaml
**When** the dependency graph is computed
**Then** a `WatcherError::CyclicDependency` error is returned with the cycle path
**And** the error is logged and the affected stories are skipped

### Story 2.3: Cascade Blocking

As a developer,
I want dependent stories to be automatically identified as blocked when a prerequisite story fails,
So that the daemon doesn't waste time attempting stories that cannot succeed.

**Acceptance Criteria:**

**Given** a story has been processed and resulted in a `blocked` or `needs-clarification` status
**When** the pre-gate logic runs on the next polling cycle
**Then** all stories that depend (directly or transitively) on the failed story are identified as ineligible
**And** a tracing warn message logs each cascade-blocked story with the reason (which prerequisite failed)

**Given** the blocking prerequisite story is later resolved (status changes to `done`)
**When** the next polling cycle runs
**Then** the previously cascade-blocked dependents are re-evaluated based on current statuses
**And** stories whose dependencies are now all `done` become eligible again

**Given** the daemon has identified cascade-blocked stories
**When** the pre-gate completes
**Then** only truly eligible stories (all dependencies met, status `ready-for-dev`) are passed to the session module
**And** the daemon never writes to `sprint-status.yaml` — all blocking logic is computed in-memory per cycle

---

## Epic 3: Intelligent Supervision

The supervisor can intercept agent questions and answer them via a deterministic rule engine or a dedicated BMAD Architect agent session (multi-turn chat with full project context loaded autonomously), escalate to human when unsure, and log every decision with reasoning and alternatives to a committed decisions file. After this epic, the ask_supervisor rig tool is built, tested, and ready to be registered with the agent.

### Story 3.1: Supervisor Tool Skeleton & Rule Engine

As a daemon operator,
I want agent questions to be automatically intercepted and answered by a deterministic rule engine,
So that predictable questions are resolved instantly without LLM cost.

**Acceptance Criteria:**

**Given** the supervisor module is initialized
**When** the `ask_supervisor` rig tool is built
**Then** it follows the standard rig Tool pattern (serializable struct + `AskSupervisorArgs` + `SupervisorError` thiserror enum + Tool trait impl)
**And** the tool NAME is `ask_supervisor` (snake_case)
**And** the tool definition description is detailed enough for the LLM agent to know when and how to call it

**Given** the agent calls `ask_supervisor` with a question matching a known pattern
**When** the rule engine in `rules.rs` evaluates the question
**Then** the rule engine matches against deterministic patterns: confirmations ("Should I proceed?"), step-by-step detection, story selection prompts, and other predictable BMAD workflow interactions
**And** the matched rule returns an answer immediately without any LLM call

**Given** the agent calls `ask_supervisor` with a question that does not match any rule
**When** the rule engine evaluates the question
**Then** the rule engine returns a `NoMatch` result indicating LLM fallback is needed
**And** the question and attempted match are logged via `tracing::info!()` with `action = "rule_engine_miss"`

**Given** the rule engine is deployed
**When** new patterns are identified from decision file analysis
**Then** rules can be added to `rules.rs` without modifying the tool interface or supervisor module structure

### Story 3.2: LLM Fallback with Project Context

As a daemon operator,
I want substantive agent questions to be answered by a dedicated BMAD Architect agent session with full project context,
So that the developer agent gets expert, context-aware architectural answers when the rule engine cannot help.

**Acceptance Criteria:**

**Given** the rule engine returns `NoMatch` for a question
**When** the supervisor LLM fallback is triggered
**Then** a fresh BMAD Architect agent session is created using the supervisor provider/model configured in `bmad-bot.yaml`
**And** the Architect agent file (`_bmad/bmm/agents/architect.md`) is loaded as the full preamble (activation steps, persona, menu, rules — the complete file)
**And** a minimal read-only `ReadFile` rig tool is registered so the Architect can load project files autonomously

**Given** a fresh Architect session is created
**When** the supervisor drives the multi-turn conversation
**Then** the following messages are sent in sequence via rig's `chat()` API: (1) `"CH"` to enter free chat mode, (2) `"Load the project context"` so the Architect loads relevant project docs via the ReadFile tool, (3) `"A developer agent working on this project has the following question: {question}"` with optional context
**And** the Architect's final response is returned to the dev agent as the tool output
**And** the session is discarded after each question (no persistence between supervisor calls)
**And** the fallback is logged via `tracing::warn!()` with `action = "supervisor_fallback"` and the question

**Given** the LLM provider is unavailable or the Architect session fails
**When** the supervisor attempts the fallback
**Then** the entire session is retried with exponential backoff (max 2 retries, 3 total attempts)
**And** if all retries fail, the supervisor proceeds to human escalation

### Story 3.3: Human Escalation

As a developer,
I want the supervisor to stop and escalate to me when it cannot answer a question confidently,
So that no incorrect decision is made autonomously.

**Acceptance Criteria:**

**Given** the rule engine returns `NoMatch` and the Architect session either fails or cannot answer confidently
**When** the supervisor determines it cannot answer
**Then** the `ask_supervisor` tool returns a `SupervisorError::EscalationRequired` error
**And** this error stops the rig agent loop, returning control to the daemon session module

**Given** the supervisor has escalated
**When** the session module receives the escalation error
**Then** the story status is set to `needs-clarification` (via the agent's last actions or session cleanup)
**And** the escalation event is logged via `tracing::warn!()` with `action = "supervisor_escalation"`, the question, and the reason for escalation

**Given** the supervisor escalates
**When** the session handles the escalation
**Then** partial work is preserved (commits, branch state) so the story can be resumed after human intervention

### Story 3.4: Decision Logging & Traceability

As a developer reviewing automated work,
I want every supervisor decision logged with full reasoning and alternatives,
So that I can audit, understand, and improve the supervisor's behavior over time.

**Acceptance Criteria:**

**Given** the supervisor answers a question (via rule engine or LLM fallback)
**When** the decision is made
**Then** a `DecisionRecord` is created in `decisions.rs` containing: question, chosen answer, source (rule_engine or llm_fallback), reasoning, and alternatives considered
**And** the record is appended to an in-memory decisions list for the current session

**Given** a development session completes or is interrupted
**When** the decision logging module finalizes
**Then** a decisions file is written to `_bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md` containing all decisions from the session in a human-readable markdown format
**And** the file is committed to the git branch

**Given** decisions have been logged during a session
**When** a PR is created (Epic 5)
**Then** the decisions list is available as structured data for inclusion in the PR description's "Supervisor Decisions" section
**And** each decision entry shows: question, decision, reasoning, and alternatives

---

## Epic 4: Autonomous Development Session

The daemon launches a streaming rig agent session with the BMAD dev agent persona (activated via Zed-style XML context) and registered tools (git, filesystem, terminal, ask_supervisor, think). The agent reviews prior stories, updates specs, creates a branch, and executes the full dev-story workflow autonomously with English language override via minimal system preamble. After this epic, stories are developed end-to-end by the agent.

### Story 4.1: Rig Tools Implementation (Git, Filesystem, Terminal, Think)

As a daemon operator,
I want the agent to have access to git, filesystem, terminal, and think tools during development sessions,
So that the agent can perform all operations needed to develop a story autonomously.

**Acceptance Criteria:**

**Given** the tools module is initialized
**When** the git tool (`tools/git.rs`) is built
**Then** it follows the standard rig Tool pattern (serializable struct + `GitToolArgs` + `GitToolError` thiserror enum + Tool trait impl)
**And** it exposes git operations via git2: clone, checkout, branch create, add, commit, push, diff, status, log
**And** the tool NAME is `git` and the definition description is detailed enough for the LLM to use correctly
**And** every `call()` logs the action and result via `tracing` with story context

**Given** the tools module is initialized
**When** the filesystem tool (`tools/fs.rs`) is built
**Then** it follows the standard rig Tool pattern with `FsToolArgs` + `FsToolError`
**And** it exposes file operations via std::fs / tokio::fs: read file, write file, list directory, create directory, delete, check existence
**And** every `call()` logs the action and result via `tracing`

**Given** the tools module is initialized
**When** the terminal tool (`tools/terminal.rs`) is built
**Then** it follows the standard rig Tool pattern with `TerminalToolArgs` + `TerminalToolError`
**And** it exposes command execution via tokio::process: run command, capture stdout/stderr, return exit code
**And** every `call()` logs the command and result via `tracing`

**Given** the tools module is initialized
**When** the think tool is registered
**Then** rig's built-in `ThinkTool` (derived from Anthropic's Claude Think Tool pattern) is added to all agent builders
**And** it gives the agent a dedicated space for structured reasoning during complex tasks without consuming real tool calls
**And** no custom implementation is needed — it is provided by the `rig` crate

**Given** any tool encounters an error
**When** the error is handled
**Then** it never panics — always returns `Result` with a descriptive error
**And** errors bubble up to the rig agent loop which decides how to proceed

### Story 4.2: Agent Session Setup & Chat Loop

As a daemon operator,
I want the daemon to launch an autonomous LLM agent session with the BMAD dev persona and all registered tools,
So that stories are developed without human intervention.

**Acceptance Criteria:**

**Given** the session module is initialized with a `StoryInfo` from the watcher
**When** an agent session is created
**Then** the agent is built with a minimal system preamble containing operational instructions (tool usage, communication rules, language override to English)
**And** five tools are registered: git, filesystem, terminal, ask_supervisor, and think
**And** the agent is built using the dev LLM provider/model from `BotConfig`

**Given** an agent is built
**When** the activation flow starts
**Then** the BMAD dev agent file is loaded from the project's `_bmad/` directory and sent as the first user message wrapped in Zed-style XML context tags (`<context><files>`) with adaptive backtick fencing and absolute path resolution — NOT injected as the system preamble
**And** the `activate_agent()` method waits for the activation response (agent executes activation steps via tools: loads config.yaml, shows greeting/menu) before proceeding
**And** a new `llm_context` module with `ContextBuilder` helper handles the Zed-style XML formatting (multi-file support, line ranges, adaptive fencing)

**Given** an agent session is ready
**When** the chat loop starts
**Then** the first message sent is `"DS"` (triggers the dev-story workflow in the BMAD agent's menu system)
**And** the daemon manages the chat loop via streaming API (`stream_chat()`), analyzing each agent response for workflow interaction points (confirmations, "should I proceed?", step transitions)
**And** the daemon responds automatically to workflow-level interactions

**Given** the agent is working in a chat loop
**When** the agent completes the dev-story workflow and signals completion
**Then** the session module detects completion and exits the chat loop
**And** the session result (success, blocked, or error) is returned to the daemon for downstream processing (review, PR, notification)

**Given** a session is active
**When** the entire session lifecycle runs
**Then** a `story_session` tracing span wraps the whole session with `story_id` and `branch` fields
**And** the daemon knows nothing about BMAD workflow internals — it only loads the agent file, registers tools, and manages the chat loop

### Story 4.3: Pre-Development Preparation & Branch Management

As a developer,
I want the agent to review prior implementations and refresh the current story's specs before coding,
So that each story is developed with up-to-date context and on a clean dedicated branch.

**Acceptance Criteria:**

**Given** a story has been selected for development and the agent session is active
**When** the agent begins the dev-story workflow
**Then** the agent uses the filesystem tool to read previously completed stories and their implementations
**And** the agent updates the current story's specs and acceptance criteria based on actual implementation of prior stories (if applicable)

**Given** the agent is ready to start coding
**When** the agent prepares the development environment
**Then** the agent uses the git tool to create and checkout a new branch following the `story/{epic}-{story}` naming convention (e.g., `story/1-2-account-management`)
**And** the branch is created from the configured base branch (e.g., `main`)

**Given** the branch already exists (e.g., from a previous interrupted session)
**When** the agent attempts to create it
**Then** the agent detects the existing branch, checks it out, and continues from the current state
**And** the situation is logged via `tracing::info!()` with `action = "branch_reuse"`

### Story 4.4: Migrate All Git Operations from git2 to Git CLI

> **Triggered by:** Production incident (2026-02-10) — daemon push authentication failure. SSH agent not available in background process context. See `architect-brief-git-cli-migration.md` for full rationale.

As a daemon operator,
I want all git operations to use the Git CLI instead of the git2 (libgit2) library,
So that the daemon inherits the user's full git configuration (credential manager, SSH agent, commit signing, `.gitconfig` identity) and eliminates the dual auth path (git2 SSH vs HTTPS token workaround).

**Acceptance Criteria:**

**Given** the daemon starts up
**When** the startup validation runs
**Then** it executes `git --version` and verifies git is available
**And** it fails fast with a clear, actionable error message if git is missing
**And** this check runs in `cli/mod.rs::run_start()` alongside existing config validation

**Given** the `GitTool` in `src/tools/git.rs` currently uses `git2` for 9 actions (clone, checkout, branch_create, add, commit, push, diff, status, log)
**When** the migration is applied
**Then** each action is rewritten to use `tokio::process::Command::new("git")` with appropriate arguments
**And** the working directory is always set explicitly via `-C <path>` or `.current_dir(path)`
**And** both stdout and stderr are captured — stderr included in error messages for LLM-readable diagnostics
**And** `--porcelain` flags are used where available (status, diff) for stable, parseable output
**And** non-zero exit codes are mapped to `GitToolError::CommandFailed` with the full stderr content
**And** output is returned as-is (git CLI output is already human/LLM-readable)

**Given** the branch management in `src/session/branch.rs` currently uses `git2` for 3 functions (`determine_base_branch`, `ensure_story_branch`, `checkout_branch`)
**When** the migration is applied
**Then** each function is rewritten to use `std::process::Command::new("git")` (sync context, called from `spawn_blocking`)
**And** `determine_base_branch()` uses `git branch --list` to check branch existence
**And** `ensure_story_branch()` uses `git checkout -b` (create) or `git checkout` (reuse)
**And** `checkout_branch()` uses `git checkout`

**Given** the pipeline push in `src/pipeline.rs` currently uses a hybrid HTTPS token workaround
**When** the migration is applied
**Then** `push_branch()` is simplified to `git push origin <branch>` via `tokio::process::Command`
**And** authentication is inherited from the user's git configuration (SSH agent, credential helper, osxkeychain)
**And** the HTTPS URL construction workaround is removed

**Given** all git operations have been migrated
**When** the `git2` crate is no longer referenced anywhere
**Then** `git2` is removed from `Cargo.toml`
**And** compile time and binary size are reduced (libgit2 + libssh2 + OpenSSL transitive C dependencies eliminated)

**Given** the migration is complete
**When** existing unit tests are updated
**Then** tests mock CLI output (stdout/stderr + exit code) instead of creating in-memory git2 repositories
**And** all tests pass with the new implementation

**Given** the migration is complete
**When** documentation is updated
**Then** `project-context.md` replaces "Git Operations: git2 (embedded libgit2, no CLI dependency)" with "Git Operations: Git CLI subprocess — requires git installed on host"

**Technical Notes:**
- This story replaces the git2 implementation from Story 4.1 and the branch management from Story 4.3 with Git CLI equivalents
- Also replaces the pipeline push HTTPS workaround (hotfix commits `62929b2` and `eaafa28`)
- Estimated reduction: ~600 lines of git2 boilerplate → ~250 lines of CLI calls
- Follows the "Git CLI Subprocess Pattern" defined in the Architecture Decision Document
- Cross-cutting: touches `tools/git.rs` (Epic 4), `session/branch.rs` (Epic 4), `pipeline.rs` (Epic 5 area), `cli/mod.rs` (Epic 1 area)

---

### Story 4.5: LLM Provider Abstraction Layer (AgentFactory + BuiltAgent)

> **Triggered by:** Production incident (2026-02-12) — `gpt-5.2-codex` via GitHub Copilot proxy rejects `/chat/completions` endpoint (requires Responses API). See `architect-brief-llm-provider-abstraction.md` for full rationale.

As a daemon operator,
I want all LLM provider construction centralized behind an `AgentFactory` with a `BuiltAgent` enum,
So that provider selection, API format detection, and Copilot token exchange happen in one place, eliminating duplication and fixing the Copilot Responses API bug.

**Acceptance Criteria:**

**Given** the `llm` module exists with `context.rs` and `logging.rs`
**When** the `agent_factory.rs` module is created
**Then** it defines a `BuiltAgent` enum with variants: `Anthropic(Agent<anthropic::CompletionModel>)`, `OpenAiResponses(Agent<openai::responses_api::ResponsesCompletionModel>)`, `OpenAiCompletions(Agent<openai::completion::CompletionModel>)`
**And** `BuiltAgent` implements a `stream_chat()` method that delegates to `streaming_chat()` via match dispatch

**Given** the `AgentFactory` struct is initialized with `BotConfig`, `BotSecrets`, and `CopilotTokenCache`
**When** `AgentFactory::build(role, preamble, tools)` is called
**Then** it resolves the provider and model for the given `LlmRole` (Dev, Review, Supervisor)
**And** it resolves the API key from secrets
**And** it constructs the appropriate `BuiltAgent` variant based on provider:
  - `"anthropic"` → `BuiltAgent::Anthropic`
  - `"openai"` → `BuiltAgent::OpenAiResponses`
  - `"github-copilot"` → exchanges OAuth token for session token, then selects API format per model

**Given** the provider is `"github-copilot"`
**When** `AgentFactory::build()` determines the API format
**Then** `copilot_requires_responses_api(model)` is called — a hardcoded heuristic that matches known OpenAI model families (`gpt-*`, `o1-*`, `o3-*`, `codex`)
**And** matched models use the Responses API (`BuiltAgent::OpenAiResponses`)
**And** all other models (Claude, Mistral, unknown) **fallback to Completions API** (`BuiltAgent::OpenAiCompletions`) — the safe default
**And** this logic is not configurable — API format is a deterministic property of the provider behind the model

**Given** the `AgentFactory` is created
**When** `session/runner.rs` is refactored
**Then** the 3 `build_*_agent()` methods (`build_anthropic_agent`, `build_openai_agent`, `build_copilot_agent`) are removed
**And** all provider match arms in `run()` and `resume_session()` are replaced with a single `agent_factory.build(LlmRole::Dev, ..)` call
**And** `run_session()` accepts `&BuiltAgent` directly and uses `BuiltAgent::stream_chat()` instead of the generic `streaming_chat()`

**Given** the `AgentFactory` is created
**When** `review/mod.rs` is refactored
**Then** the provider match in `run_inner()` is replaced with `agent_factory.build(LlmRole::Review, ..)`

**Given** the `AgentFactory` is created
**When** `supervisor/architect.rs` is refactored
**Then** the provider match is replaced with `agent_factory.build(LlmRole::Supervisor, ..)`

**Given** the `AgentFactory` is created
**When** `pipeline.rs` is updated
**Then** `StoryPipeline` receives an `AgentFactory` instance instead of individual provider configs
**And** it passes the factory to `SessionRunner` and `ReviewRunner`

**Given** the refactoring is complete
**When** unit tests are written
**Then** `copilot_requires_responses_api()` is tested with known model names (gpt-4o, o1-mini, o3-pro, gpt-5.2-codex, claude-sonnet-4-20250514, mistral-large) verifying correct API format selection
**And** `AgentFactory::build()` error handling is tested (missing API key, invalid provider name)
**And** `BuiltAgent::stream_chat()` dispatch is verified for each variant

**Given** all changes are complete
**When** validation runs
**Then** `cargo build`, `cargo test`, `cargo clippy`, and `cargo fmt` all pass with zero errors and zero warnings

**Technical Notes:**
- Follows the same pattern as Story 4.4 (git CLI migration): production incident → architect brief → cross-cutting refactoring story
- `session/provider.rs` functions (`resolve_api_key`, `copilot_headers`) are absorbed into `AgentFactory` — `provider.rs` may be removed or reduced to re-exports
- `streaming_chat()` may be moved from `session/dev_agent.rs` to `llm/` or re-exported, since `BuiltAgent::stream_chat()` delegates to it
- The `BuiltAgent` enum must be updated if rig adds new provider types — acceptable trade-off (rare, compile-time concern)
- See `architect-brief-llm-provider-abstraction.md` for full technical rationale and before/after code examples
- Architecture Decision 8 in `architecture.md` documents this pattern
- Cross-cutting: touches `llm/agent_factory.rs` (new), `session/runner.rs` (Epic 4), `review/mod.rs` (Epic 5), `supervisor/architect.rs` (Epic 3), `pipeline.rs` (Epic 5 area)

---

### Story 4.6: Post-Implementation Impact Analysis on Downstream Stories

> **Triggered by:** Story 7-1 completed without updating downstream stories 7-2 through 7-10 — their Dev Notes still reference assumptions invalidated by actual implementation. See `architect-brief-post-impl-impact-analysis.md` for full rationale.

As a daemon operator,
I want the agent to analyze and update downstream dependent stories after completing a story,
So that the next agent picking up a dependent story works from accurate assumptions instead of stale specs, reducing wasted tokens, wrong patterns, and rework.

**Acceptance Criteria:**

**Given** the agent has signaled `<<BMAD_JOB_DONE>>` and the final commit (Step 7) has completed
**When** the session runner executes the impact analysis step (Step 8)
**Then** it sends an impact analysis prompt to the agent in a dedicated chat turn with full tool access

**Given** the impact analysis prompt is sent
**When** the agent processes it
**Then** it reads `sprint-status.yaml` and identifies stories whose `depends-on` references the completed story (by full key or short key `{epic}-{story}`)
**And** it checks subsequent stories in the same epic (document order) as a secondary criterion

**Given** downstream dependent stories are identified
**When** the agent reads their Dev Notes
**Then** it compares the "Previous Story Intelligence" sections against what was actually implemented
**And** it updates only "Previous Story Intelligence" sections where actual implementation deviates from planned assumptions
**And** updates include: what changed vs the original plan, new APIs/patterns/modules to use, obsolete assumptions to discard
**And** updates are idempotent — sections are replaced, not appended

**Given** the completed story introduced new modules or changed interfaces
**When** the agent checks for `architecture.md`
**Then** it verifies the file exists before attempting to read or update it (not all projects have one)
**And** it updates architecture references only if new modules or changed interfaces were introduced

**Given** downstream stories or architecture have been updated
**When** the agent commits the changes
**Then** the commit message uses the prefix `docs(stories): update downstream specs after {story_key}`

**Given** no downstream stories need updating
**When** the agent evaluates the impact
**Then** it reports that nothing needs updating and moves on without making changes — it does not invent changes

**Given** the impact analysis chat turn fails (LLM error, timeout, context window exhaustion)
**When** the session runner handles the failure
**Then** it proceeds to the PR summary step (Step 9) without error
**And** the story completion is not blocked or marked as failed
**And** the failure is logged via `tracing::warn!`

**Given** the impact analysis step completes (success or skip)
**When** the PR summary step (Step 9) executes
**Then** it is aware that an impact analysis commit may have been added to the branch
**And** the PR description reflects both the implementation work and any downstream spec updates

**Given** all changes are complete
**When** validation runs
**Then** `cargo build`, `cargo test`, `cargo clippy`, and `cargo fmt` all pass with zero errors and zero warnings

**Technical Notes:**
- Single file change: `src/session/runner.rs` — add impact analysis chat turn after Step 7 (final commit), renumber PR summary to Step 9. ~40-50 lines of new code following the same pattern as existing post-completion steps
- Agent-driven, not daemon-driven: the daemon sends the prompt, the agent uses existing tools (`read_file`, `edit_file`, `git`) to do the work. No new tools or daemon logic required beyond the prompt and chat turn
- Scope guard: the agent must only update "Previous Story Intelligence" in Dev Notes and architecture references — not rewrite tasks, ACs, or other story sections
- Estimated token cost: 2-8k tokens per story (reads 2-5 files, updates or skips)
- Symmetric counterpart to the pre-dev spec update (FR5-6): pre-dev reads prior stories' output, post-impl propagates forward to downstream stories
- See `architect-brief-post-impl-impact-analysis.md` for the full architect brief with the proposed prompt design and sequence diagrams
- Architecture Data Flow step 8 documents this feature

---

### Story 4.8: Epic Gate — Autonomous Retrospective Review

> **Triggered by:** Production incident (2026-03-06) — daemon looped 4× re-processing 14 already-completed stories (42 wasted LLM sessions, 19 duplicate MRs) due to sprint-status.yaml divergence across parallel branch chains. Root cause fixed (sequential branch chaining), but exposed a deeper problem: no human checkpoint exists between epics, allowing architectural drift, pattern inconsistencies, and infrastructure bugs to compound unchecked. See `architect-brief-epic-gate-retrospective.md` for full rationale.

As a daemon operator,
I want the daemon to automatically pause between epics, run an autonomous LLM-driven codebase review, and block the next epic until a human validates the report,
So that architectural drift, pattern inconsistencies, and technical debt are caught at epic boundaries instead of compounding silently across the entire project.

**Acceptance Criteria:**

**Given** the pipeline has just finished processing a story successfully
**When** all stories in the completed story's epic are `done`
**Then** the pipeline detects epic completion and launches the autonomous epic review process
**And** if no `epic-X-retrospective` entry exists in sprint-status.yaml, the gate is skipped entirely (backward compatibility)

**Given** epic completion is detected and the retrospective entry exists
**When** the review session launches
**Then** a `BuiltAgent` is constructed using `AgentFactory::build(LlmRole::EpicReview, ...)` with read-only tools (read_file, grep, find_path, list_directory, terminal, git, think — NO edit_file, NO ask_supervisor)
**And** the preamble uses the Architect agent persona (`_bmad/bmm/agents/architect.md`) combined with project-context.md

**Given** the review agent is analyzing the epic
**When** it produces the report
**Then** the report contains four sections: (1) Epic Recap — objectives, stories delivered, planned vs actual, (2) Functional Testing Guide — step-by-step scenarios with concrete CLI commands the human can follow to verify each capability, (3) Technical Analysis — pattern consistency, architecture adherence, tech debt inventory, codebase health via cargo check/test/clippy, (4) Recommendations — actionable items for the next epic

**Given** the review report is generated (or the review session failed)
**When** the pipeline creates retrospective artifacts
**Then** the report is saved to `{implementation_artifacts}/epic-{X}-retrospective-report.md`
**And** a branch `epic-{X}-retrospective` is created, the report committed, pushed, and an MR created via GitProvider
**And** sprint-status.yaml is updated: `epic-X-retrospective: review`
**And** the human is notified (if notifications enabled) with epic summary, MR link, and unlock instructions

**Given** `epic-X-retrospective` is set to `review`
**When** the watcher computes eligible stories
**Then** ALL stories from epic X+1 and beyond are excluded from the eligible list
**And** the gate is only cleared when the human sets `epic-X-retrospective: done`

**Given** the `LlmRole` enum exists with Dev, Review, Supervisor
**When** the `EpicReview` variant is added
**Then** `LlmConfig` gains an `epic_review: LlmRoleConfig` field with `#[serde(default)]`
**And** if absent from config, it falls back to the `review` role config at runtime (zero-config backward compatibility)

**Technical Notes:**
- New file: `src/review/epic.rs` — `EpicReviewRunner` struct mirroring `ReviewRunner` pattern
- Extends: `LlmRole` enum, `LlmConfig`, `AgentFactory::config_for_role()`, `pipeline.rs` (epic completion detection + review trigger), `watcher/deps.rs` (retrospective gate check), `Notifier` trait
- The review is pipeline-level (not session-level) — triggered in `process_eligible_stories()` after `process_story()` returns Completed
- Review failure is non-blocking for the gate activation — a failed review is MORE reason to pause
- Cross-cutting: touches `llm/agent_factory.rs`, `config/mod.rs`, `pipeline.rs`, `watcher/deps.rs`, `review/epic.rs` (new), `notifier/mod.rs`
- See `architect-brief-epic-gate-retrospective.md` for full architect brief with incident details

---

## Epic 5: Code Review & Pull Request Delivery

The daemon creates a Pull Request on GitHub or GitLab with an agent-written description immediately after the dev session, including a Supervisor Decisions section. The PR is visible for human review right away. It then optionally launches a code review via a separate LLM, with fixes in separate commits pushed to update the PR and review posted as a PR comment. PRs are also created for blocked/failed stories with partial code and failure context. After this epic, the user wakes up to PRs ready for human review.

### Story 5.1: Git Provider Trait & GitHub PR Creation

As a developer using GitHub,
I want the daemon to create Pull Requests with comprehensive descriptions after each story session,
So that I wake up to reviewable PRs with full context on what was done and why.

**Acceptance Criteria:**

**Given** the git_provider module is initialized
**When** the `GitProvider` trait is defined
**Then** it exposes async methods: `create_pr(params: CreatePrParams) -> Result<PrInfo, GitProviderError>`, `add_comment(pr_id: &str, body: &str) -> Result<(), GitProviderError>`, `get_pr_url(pr_id: &str) -> Result<String, GitProviderError>`
**And** `CreatePrParams`, `PrInfo`, and `GitProviderError` are dedicated structs/enums following the git provider trait pattern
**And** the provider is selected via `bmad-bot.yaml` config (`git_provider: github | gitlab`)

**Given** a development session has completed successfully
**When** the daemon creates a PR via the GitHub implementation (octocrab)
**Then** the PR is created with: agent-written title and description body, source branch (`story/{epic}-{story}`), target branch (configured base branch)
**And** the PR description includes a dedicated "🤖 Supervisor Decisions" section listing all decisions from the session (question, decision, reasoning, alternatives)

**Given** a development session has been blocked or failed
**When** the daemon creates a PR for the failed story
**Then** a PR is still created with partial code committed to the branch
**And** the PR description includes a clear failure/blockage description explaining what happened, where it stopped, and all decisions made before the failure

**Given** code review is disabled in configuration
**When** the development session completes
**Then** the daemon proceeds directly to PR creation without launching a review session

### Story 5.2: Automated Code Review Session

As a developer,
I want an optional automated code review by a separate LLM before the PR is finalized,
So that code quality issues are caught and fixed before I review.

**Acceptance Criteria:**

**Given** code review is enabled in `bmad-bot.yaml` configuration
**When** a development session completes successfully (`SessionOutcome::Completed`)
**Then** the daemon launches a **new rig agent session** using the review LLM provider/model from `BotConfig.llm.review`
**And** the session loads the same BMAD dev agent persona (`dev.md`) and sends `"CR"` as the initial command
**And** the `ResponseAnalyzer` auto-responds to the story selection prompt with the story specs file path

**Given** the BMAD CR workflow asks how to handle findings (fix automatically / create action items / show details)
**When** the daemon's `ResponseAnalyzer` detects this decision prompt
**Then** it auto-responds with `"1"` (fix them automatically)
**And** the review agent applies fixes to the code (but does not commit yet)

**Given** the CR workflow completes (step 5 "Review Complete")
**When** the daemon detects the review completion output
**Then** the daemon sends a post-review message asking the agent to commit all review fixes with descriptive commit messages referencing the findings, and to provide a complete markdown review report
**And** the agent commits the fixes in separate commits (distinct from dev agent commits) with full context
**And** the agent's review report response is captured in `ReviewOutcome::Completed { report }`

**Given** a PR was already created by the orchestrator before the review
**When** the review completes with `ReviewOutcome::Completed` containing a report
**Then** the orchestrator pushes any review fix commits to update the PR, then posts the review report as a comment on the PR via `GitProvider::add_comment()`
**And** the review comment includes: summary of findings, severity levels, fixes applied, and any remaining concerns

**Given** the review LLM provider is unavailable or errors out
**When** the review session fails
**Then** the daemon logs the error, returns `ReviewOutcome::Skipped` with a reason
**And** the orchestrator proceeds to PR creation without review
**And** the PR description notes that automated code review was skipped due to an error

**Given** code review is disabled in configuration (`code_review_enabled: false`)
**When** a development session completes
**Then** the daemon skips the review entirely and proceeds directly to PR creation

### Story 5.3: GitLab Merge Request Support

As a developer using GitLab,
I want the daemon to create Merge Requests with the same comprehensive descriptions as GitHub PRs,
So that I get the same experience regardless of my git provider.

**Acceptance Criteria:**

**Given** the `bmad-bot.yaml` config specifies `git_provider: gitlab`
**When** the GitLab implementation of `GitProvider` is initialized
**Then** it uses reqwest direct calls to the GitLab REST API (v4) with the token loaded from `.env`
**And** it implements all trait methods: `create_pr` (creates a Merge Request), `add_comment` (posts a note on the MR), `get_pr_url` (returns the MR web URL)

**Given** a development session has completed
**When** the daemon creates a Merge Request via the GitLab implementation
**Then** the MR is created with: agent-written title and description, source branch, target branch
**And** the MR description includes the "🤖 Supervisor Decisions" section, identical in format to the GitHub implementation

**Given** code review is enabled and completes
**When** the review is posted on GitLab
**Then** the review is posted as a note (comment) on the Merge Request via the GitLab notes API
**And** the format and content are consistent with the GitHub PR comment implementation

**Given** the GitLab API returns rate limit or transient errors
**When** the git_provider makes API calls
**Then** errors are handled by the reqwest-middleware retry layer (exponential backoff, max 3 retries)
**And** permanent failures return a descriptive `GitProviderError` with the HTTP status and response body

### Story 5.4: Enriched PR Description with Agent-Generated Context

As a developer reviewing PRs created by bmad-bot,
I want the PR description to include meaningful context, testing instructions, and additional information generated by the agent,
So that I can review the PR efficiently without having to reconstruct what was done and why.

**Acceptance Criteria:**

**Given** a development session completes successfully
**When** the daemon creates a PR
**Then** the PR description follows an enriched template with sections: `📝 Context` (agent-generated summary of what was implemented), `🤖 Supervisor Decisions`, `🧪 How to test` (agent-generated step-by-step testing instructions), and `ℹ️ Additional information` (dependencies, caveats, concerns)
**And** the footer links to the bmad-bot repository: `*Generated by [bmad-bot](https://github.com/jbanety/bmad-bot)*`

**Given** the agent reaches the end of the dev-story workflow
**When** the session completes
**Then** the agent produces a structured PR summary (using XML tags for reliable parsing) containing context, how-to-test, and additional-info sections
**And** the summary is captured via `parse_pr_summary()` and stored in `SessionOutcome::Completed { pr_summary }`

**Given** the agent does not produce a PR summary (crash, timeout, context limit)
**When** the daemon builds the PR description
**Then** fallback text is used for each section so the PR description is never empty or malformed

**Given** a development session fails or is escalated
**When** the daemon creates a failure/escalation PR
**Then** the same enriched template is used, with a `⚠️ Failure Details` section between Context and Supervisor Decisions
**And** sections use agent-generated content when available, fallback text otherwise

---

## Epic 6: Notifications & Error Resilience

The daemon sends Telegram notifications with story status, ID, and PR links. It handles LLM rate limits with retry/backoff, notifies the human of blocking errors, detects interrupted sessions via WAL file for crash recovery, and recovers from context window limit errors by summarizing history and bootstrapping a fresh session. After this epic, the user can trust the daemon to run overnight without supervision.

### Story 6.1: Telegram Notifications

As a developer running BMAD Bot overnight,
I want to receive Telegram notifications summarizing what happened,
So that I know the results without checking GitHub/GitLab manually.

**Acceptance Criteria:**

**Given** a development session completes successfully
**When** the notifier module sends a notification
**Then** a Telegram message is sent via reqwest direct HTTP call to the Telegram Bot API using the bot token from `.env`
**And** the message includes: story ID, status (✅ completed), and a direct link to the PR/MR

**Given** a story is blocked or encounters an error
**When** the notifier module sends a notification
**Then** a Telegram message is sent with: story ID, status (⚠️ blocked or ❌ error), reason for blockage/error, and a link to the PR if one was created
**And** the message provides enough context to understand the issue without opening the PR

**Given** a full daemon run completes (all eligible stories processed)
**When** the run summary is generated
**Then** a summary notification is sent with: total stories processed, count by status (completed, blocked, errored), and links to all PRs created

**Given** the Telegram API is unavailable or returns an error
**When** the notifier attempts to send a message
**Then** the failure is logged via `tracing::error!()` with full context
**And** the notification failure does NOT block the pipeline — story processing continues normally
**And** no retry is attempted for notification failures (non-critical path)

### Story 6.2: HTTP Retry & Error Resilience

As a daemon operator,
I want all external HTTP calls to be resilient to transient failures,
So that temporary provider outages don't derail overnight runs.

**Acceptance Criteria:**

**Given** all 3 retries are exhausted for an LLM provider call (retry middleware configured in Story 1.1)
**When** the final retry fails
**Then** the error bubbles up to the session/daemon layer (Layer 3 error propagation)
**And** the session commits partial work, creates a PR with failure description, and notifies the human

**Given** all 3 retries are exhausted for a GitHub/GitLab API call
**When** PR creation or comment posting fails permanently
**Then** the error is logged with full context (HTTP status, response body, story_id)
**And** the human is notified via Telegram with the failure details and the branch name so they can create the PR manually

**Given** a blocking error occurs at any point in the pipeline (session crash, git failure, all LLM providers down)
**When** the daemon's Layer 3 error handler catches it
**Then** a notification is sent to the human with: story ID, error type, error details, and recovery guidance
**And** the daemon moves on to the next eligible story (does not stop the entire run)

### Story 6.3: Crash Recovery via Session WAL

As a developer,
I want the daemon to recover from crashes by resuming interrupted sessions,
So that no work is lost if the process dies unexpectedly.

**Acceptance Criteria:**

**Given** a development session is active
**When** the chat loop completes a turn
**Then** the session state is persisted to a WAL file at `_bmad-output/implementation-artifacts/.bmad-bot-session.yaml`
**And** the WAL contains: story_id, branch name, started_at, last_activity, provider/model config, and complete chat_history (Vec<Message> serialized with role + content for each turn)

**Given** a session completes successfully (PR created)
**When** the session cleanup runs
**Then** the WAL file is deleted
**And** the daemon returns to the polling loop

**Given** the daemon starts up
**When** a WAL file exists from a previous interrupted session
**Then** the daemon detects the interrupted session and logs it via `tracing::warn!()` with `action = "crash_recovery"`
**And** the git state is verified (branch exists, dirty files confirm crash mid-session)
**And** the chat history is reloaded from the WAL file
**And** the agent is reconstructed with the same provider/model config from the WAL
**And** the chat loop resumes with the loaded history — the agent has full context and continues where it left off

**Given** the daemon starts up and no WAL file exists
**When** the initialization check completes
**Then** the daemon proceeds to normal polling (clean start)

### Story 6.4: Context Window Limit Recovery

As a developer,
I want the daemon to recover from context window limit errors without losing session progress,
So that long or complex stories can still be completed autonomously.

**Acceptance Criteria:**

**Given** the agent session is active and the chat history has grown large
**When** the LLM API returns a context limit error
**Then** the error is detected from the provider response in the chat loop
**And** the recovery process is initiated (not a crash — a controlled recovery)

**Given** a context limit error has been detected
**When** the recovery process starts
**Then** the full chat_history is read from the WAL file (already persisted after each turn)
**And** the last N exchanges are extracted verbatim from the WAL as immediate context
**And** a separate, fresh LLM call is made (new context, not the exhausted one) to summarize the full chat_history into a compact session summary

**Given** the summary has been generated
**When** the new session is bootstrapped
**Then** a fresh agent is constructed with the same provider/model config, minimal system preamble, and all five tools (git, filesystem, terminal, ask_supervisor, think)
**And** the daemon drives the BMAD activation flow via `activate_agent()`: the agent file is sent as a Zed-style XML context user message, the agent executes activation steps via tools, then the daemon sends "CH" to enter chat mode and "Load the project context" so the agent loads what it needs (same pattern as Story 3.2 Architect session)
**And** the daemon then sends a recovery message containing the session summary, last N verbatim exchanges, and instruction to continue on the current story
**And** the session enters direct chat mode (not re-entering the full dev-story workflow pipeline, since checkboxes and Dev Agent Record are already up to date on disk)

**Given** the new session is bootstrapped
**When** the chat loop resumes
**Then** the agent picks up the current task with full awareness of prior work
**And** the recovery event is logged via `tracing::info!()` with `action = "context_limit_recovery"`, original history length, and summary length
**And** the WAL file is updated with the new (compressed) session state

---

## Epic 7: Integration Tests

### Overview

All 6 functional epics have been implemented and pass 573 unit tests. This epic introduces **integration tests** that validate the interactions between modules at their boundaries — ensuring the daemon works as a cohesive system, not just as isolated pieces. These tests are deterministic (no real LLM calls), run in CI, and use mocked external dependencies.

**Why now:** Unit tests verify each module in isolation. Integration tests verify that the contracts between modules hold — that `watcher` feeds the right data to `pipeline`, that `pipeline` orchestrates `session → review → git_provider → notifier` correctly, that crash recovery actually reconstructs a valid session from a WAL file, and that the CLI lifecycle works end-to-end.

**⚠️ BLOCKED: Epic 7 is blocked until Epic 8 (Surgical Development Tooling) is fully complete.** Epic 8 replaces the monolithic `FsTool` with 5 focused tools (`ReadFileTool`, `EditFileTool`, `GrepTool`, `FindPathTool`, `ListDirectoryTool`). Writing integration tests against the old tool surface would be throwaway work. All Epic 7 stories must be written and executed against the post-Epic 8 codebase.

**Scope boundary:** This epic covers **deterministic integration tests only**. True E2E tests involving live LLM calls remain gated behind `BMAD_E2E=1` and are out of scope.

### Integration Test Strategy

#### What We're Testing

| Test Category | Modules Under Test | External Deps |
|---------------|-------------------|---------------|
| Config → Startup | `config/`, `cli/`, `config/discovery.rs` | Filesystem (temp dirs) |
| Watcher → Pipeline Dispatch | `watcher/`, `watcher/deps.rs`, `pipeline.rs` | Filesystem (temp YAML) |
| Pipeline Orchestration | `pipeline.rs`, `session/runner.rs`, `review/`, `git_provider/`, `notifier/` | All mocked via traits |
| Session WAL Recovery | `session/state.rs`, `session/runner.rs`, `pipeline.rs` | Filesystem (temp WAL) |
| Git Provider PR Flow | `git_provider/mod.rs`, `git_provider/github.rs`, `git_provider/gitlab.rs` | HTTP mocked |
| Notifier Flow | `notifier/mod.rs` | HTTP mocked |
| CLI Lifecycle | `cli/mod.rs`, `cli/state.rs` | Filesystem, process signals |
| Branch Management | `session/branch.rs`, `tools/git.rs` | Real Git CLI on temp repos |

#### Mock Strategy

All integration tests follow the architecture's **Test Mock Pattern**:
- LLM responses: static/deterministic — never call real providers
- GitHub/GitLab API: mock HTTP server (or trait mock returning canned responses)
- Telegram API: mock HTTP server (or NoopNotifier verification)
- Filesystem: `tempfile` crate for isolated temp directories
- Git repos: real Git CLI operations on temp repos (fast, deterministic)

#### Test Infrastructure Needed

- **Mock `GitProvider` implementation** — returns canned `PrInfo` responses
- **Mock `Notifier` implementation** — captures sent notifications for assertion
- **Mock `SessionRunner` wrapper** — returns configurable `SessionOutcome` without LLM calls
- **Mock `ReviewRunner` wrapper** — returns configurable `ReviewOutcome` without LLM calls
- **Test fixture helpers** — functions to create valid `BotConfig`, `BotSecrets`, `StoryInfo`, sprint-status YAML, and WAL files

---

### Story 7.1: Integration Test Infrastructure & Fixtures

As a developer,
I want a shared test infrastructure with mock implementations and fixture builders,
So that all integration tests can be written concisely and consistently.

**Acceptance Criteria:**

**Given** a new `tests/integration/` directory is created
**When** I inspect the test helpers module
**Then** the following mock implementations exist:
- `MockGitProvider` implementing `GitProvider` trait — configurable to return `Ok(PrInfo { ... })` or `Err(GitProviderError::...)` for `create_pr`, `add_comment`, and `get_pr_url`
- `MockNotifier` implementing `Notifier` trait — captures all `notify_story` and `notify_run_summary` calls into a `Vec` for later assertion
- `MockSessionRunner` — wraps `SessionRunner` or provides a standalone struct that returns a configurable `SessionOutcome` (Completed / Escalated / Failed)
- `MockReviewRunner` — returns a configurable `ReviewOutcome` (Completed / Skipped / Failed)

**Given** the fixture module exists
**When** I call fixture builder functions
**Then** the following helpers are available:
- `make_test_config()` → valid `BotConfig` with sensible defaults (polling=60, provider=github, review=enabled)
- `make_test_secrets()` → valid `BotSecrets` with dummy tokens (never real keys)
- `make_test_story(key, label, deps)` → valid `StoryInfo` with specified key, label, branch, and dependency list
- `write_sprint_status(dir, stories)` → writes a valid `sprint-status.yaml` to a temp directory with given story entries and statuses
- `write_wal_file(dir, state)` → writes a valid `.bmad-bot-session.yaml` WAL file to a temp directory
- `create_test_repo(dir)` → initializes a git repo with an initial commit in a temp directory

**Given** the test infrastructure is built
**When** I run `cargo test --test integration`
**Then** all infrastructure tests compile and pass
**And** the mock implementations satisfy the trait bounds (`Send + Sync`)

**Technical Notes:**
- Place all test code under `tests/integration/` with a `mod.rs` entry point
- Use `#[cfg(test)]` and feature gate if needed, but prefer the `tests/` directory convention
- Mock implementations must be `Send + Sync` to satisfy async trait bounds
- All fixtures use `tempfile::tempdir()` for filesystem isolation

**Story Points:** 3

---

### Story 7.2: Config → Startup Validation Integration Tests

As a developer,
I want integration tests that verify the full config loading and validation pipeline,
So that I'm confident the daemon rejects bad configs and accepts good ones end-to-end.

**Acceptance Criteria:**

**Given** a temp directory with a valid `bmad-bot.yaml` and `.env` file
**When** the integration test loads config via `BotConfig::load()` then `BotConfig::validate()` then `BotSecrets::load()` then `BotSecrets::validate_for_config()`
**Then** the full pipeline succeeds and returns a valid `Arc<BotConfig>` and `Arc<BotSecrets>`

**Given** a temp directory with a `bmad-bot.yaml` missing a required field (e.g., `polling_interval_secs: 0`)
**When** the integration test runs the full load → validate pipeline
**Then** a descriptive `ConfigError` is returned at the validation step
**And** the error message identifies the exact field that failed

**Given** a temp directory with valid config but `.env` missing a required API key for the configured LLM provider
**When** the integration test runs load → validate → secrets-validate pipeline
**Then** a `ConfigError::MissingSecret` (or equivalent) is returned
**And** the error identifies which provider key is missing

**Given** a temp directory with a valid config
**When** `BmadDiscovery::discover()` is called on a directory with a `_bmad/` structure
**Then** the discovery detects BMAD, finds installed modules, and extracts the version
**And** calling it on a directory without `_bmad/` returns `bmad_detected: false`

**Given** a valid config is loaded
**When** `build_http_client()` is called
**Then** a `ClientWithMiddleware` is returned with retry middleware configured (3 retries, exponential backoff)

**Dependencies:** Story 7.1
**Story Points:** 2

---

### Story 7.3: Watcher → Dependency Resolution → Story Selection Integration Tests

As a developer,
I want integration tests that verify the full watcher → deps → eligible story selection chain,
So that I'm confident the daemon picks the right stories in the right order.

**Acceptance Criteria:**

**Given** a temp directory with a `sprint-status.yaml` containing 5 stories:
- Story 1-1: `done`
- Story 1-2: `ready-for-dev`, depends on 1-1
- Story 1-3: `ready-for-dev`, depends on 1-2
- Story 2-1: `ready-for-dev`, no deps
- Story 2-2: `backlog`
**When** the watcher polls and deps resolution runs
**Then** eligible stories returned are `[1-2, 2-1]` (1-1 is done, 1-3's dep not met, 2-2 not ready)
**And** stories are returned in dependency-valid order

**Given** a `sprint-status.yaml` where story 1-1 has status `blocked`
**When** cascade blocking runs for stories depending on 1-1
**Then** story 1-2 (depends on 1-1) is marked as cascade-blocked
**And** story 1-3 (transitive dependency through 1-2) is also cascade-blocked

**Given** a `sprint-status.yaml` where ALL stories are `done`
**When** the watcher polls
**Then** an empty eligible list (or `NoEligibleStories` error) is returned

**Given** a `sprint-status.yaml` with circular dependencies (1-1 depends on 1-2, 1-2 depends on 1-1)
**When** the dependency resolution runs
**Then** the system handles this gracefully (no infinite loop, both stories skipped or error reported)

**Given** a missing `sprint-status.yaml` file
**When** the watcher polls
**Then** a clear error is returned (not a panic)

**Dependencies:** Story 7.1
**Story Points:** 3

---

### Story 7.4: Pipeline Orchestration Integration Tests

As a developer,
I want integration tests that verify the full `StoryPipeline.process_story()` flow with mocked dependencies,
So that I'm confident the orchestration logic correctly chains session → PR → review → notification.

**Acceptance Criteria:**

**Given** a `StoryPipeline` constructed with:
- MockDevRunner returning `SessionOutcome::Completed`
- MockCodeReviewer returning `ReviewOutcome::Completed { report: "LGTM" }`
- MockGitProvider returning `Ok(PrInfo { id: "42", url: "https://...", number: 42 })`
- MockNotifier capturing notifications
**When** `process_story()` is called with a valid `StoryInfo`
**Then** the pipeline returns `PipelineResult` with `status: Completed` and `pr_url: Some("https://...")`
**And** MockGitProvider received a `create_pr` call **before** MockCodeReviewer was called
**And** MockNotifier captured exactly one story notification with the correct story key and PR link
**And** MockGitProvider received a `create_pr` call with a title matching `feat({story_key}): ...`
**And** MockGitProvider received an `add_comment` call with the review report as body

**Given** the same setup but MockDevRunner returns `SessionOutcome::Failed { error: "LLM timeout" }`
**When** `process_story()` is called
**Then** the pipeline returns `PipelineResult` with `status: Failed` and `error_detail: Some("LLM timeout")`
**And** a PR is still created (partial work PR) with title containing `[NEEDS REVIEW]`
**And** MockNotifier captured a notification with failure status

**Given** the same setup but MockDevRunner returns `SessionOutcome::Escalated { question: "..." }`
**When** `process_story()` is called
**Then** the pipeline returns `PipelineResult` with `status: Blocked`
**And** NO PR is created (`create_pr` not called — escalation skips PR)
**And** MockNotifier captured a notification with blocked/escalated status

**Given** a `StoryPipeline` with `code_review_enabled: false` in config
**When** `process_story()` is called and session succeeds
**Then** MockCodeReviewer is NOT called (review skipped)
**And** PR is created without a review comment (`add_comment` not called)
**And** the pipeline result is still `Completed`

**Given** a `StoryPipeline` where MockGitProvider's `create_pr` returns an error
**When** `process_story()` is called and session succeeds
**Then** the pipeline returns `PipelineResult` with `pr_url: None` and an error detail about PR creation failure
**And** MockCodeReviewer is NOT called (no PR means no point running review)
**And** MockNotifier still receives a notification (notification is best-effort, never blocks)

**Dependencies:** Story 7.1
**Story Points:** 5

---

### Story 7.5: Session WAL Crash Recovery Integration Tests

As a developer,
I want integration tests that verify crash recovery from WAL files reconstructs a valid session,
So that I'm confident the daemon can survive crashes and resume work without data loss.

**Acceptance Criteria:**

**Given** a temp directory with a valid `.bmad-bot-session.yaml` WAL file containing:
- `story_key: "1-2-cli"`, `branch_name: "story/1-2-cli"`, `base_branch: "main"`
- Chat history with 4 messages (2 user, 2 assistant)
- `provider: "anthropic"`, `model: "claude-sonnet-4-20250514"`
**When** `SessionRunner::check_and_recover_wal()` is called
**Then** it returns `Some(RecoveryInfo)` with the correct story info and state
**And** `story_info_from_wal()` produces a `StoryInfo` with matching key, branch, and label

**Given** a recovered WAL state
**When** `SessionState::to_rig_messages()` is called
**Then** the returned `Vec<Message>` contains all 4 messages in the correct order with correct roles

**Given** a WAL file with corrupted/invalid YAML content
**When** `check_and_recover_wal()` is called
**Then** the WAL file is deleted (preventing infinite recovery loops)
**And** `None` is returned (clean start)

**Given** NO WAL file exists
**When** `check_and_recover_wal()` is called
**Then** `None` is returned immediately

**Given** a valid WAL file exists
**When** `recover_and_process()` runs with mocked session/review/git_provider/notifier
**Then** the full pipeline executes for the recovered story
**And** the WAL file is deleted after processing (regardless of success or failure)

**Given** a WAL file exists AND new eligible stories are found in sprint-status
**When** the daemon startup sequence runs
**Then** crash recovery is processed FIRST, before any new stories are polled

**Dependencies:** Story 7.1
**Story Points:** 3

---

### Story 7.6: Git Provider & PR Creation Integration Tests

As a developer,
I want integration tests that verify PR creation, commenting, and description building work correctly,
So that I'm confident the daemon produces well-formed PRs on both GitHub and GitLab.

**Acceptance Criteria:**

**Given** a `GitProviderConfig` with `provider: "github"`
**When** `create_provider()` is called with a valid token
**Then** a `Box<dyn GitProvider>` is returned containing a `GitHubProvider`

**Given** a `GitProviderConfig` with `provider: "gitlab"`
**When** `create_provider()` is called with a valid token
**Then** a `Box<dyn GitProvider>` is returned containing a `GitLabProvider`

**Given** a `GitProviderConfig` with `provider: "bitbucket"` (unsupported)
**When** `create_provider()` is called
**Then** a `GitProviderError::UnsupportedProvider` error is returned

**Given** a `GitLabProvider` constructed with an empty token
**When** `new()` is called
**Then** `GitProviderError::AuthenticationFailed` is returned

**Given** a successful story with supervisor decisions
**When** `build_pr_description()` is called with `PrDescriptionParams` including decisions text
**Then** the generated description contains:
- Story key and title in the header
- Outcome summary
- A "Supervisor Decisions" section with the decisions content
**And** `build_pr_title()` returns `feat({story_key}): {title}`

**Given** a failed story
**When** `build_pr_description()` is called with failure details
**Then** the description contains a "⚠️ Failure Details" section
**And** `build_pr_title()` returns `wip({story_key}): {title} [NEEDS REVIEW]`

**Dependencies:** Story 7.1
**Story Points:** 2

---

### Story 7.7: Notification Flow Integration Tests

As a developer,
I want integration tests that verify notification construction and delivery logic,
So that I'm confident the daemon sends correct, well-formatted notifications.

**Acceptance Criteria:**

**Given** a `TelegramNotifier` constructed with a valid config and bot token
**When** `notify_story()` is called with a `StoryNotification` (completed, with PR link)
**Then** the formatted message contains the story ID, "completed" status, and the PR URL

**Given** a `NotificationConfig` with `telegram.enabled: false`
**When** `create_notifier()` is called
**Then** a `NoopNotifier` is returned (not a `TelegramNotifier`)
**And** calling `notify_story()` on the noop notifier succeeds silently

**Given** a `NotificationConfig` with `telegram.enabled: true` but no bot token in secrets
**When** `create_notifier()` is called
**Then** a `NoopNotifier` is returned as graceful fallback
**And** a warning is logged (not an error — notifications are non-blocking)

**Given** a list of `PipelineResult` items (2 completed, 1 failed, 1 blocked)
**When** `build_run_summary()` constructs the `RunSummary`
**Then** the summary correctly counts: 4 total, 2 completed, 1 failed, 1 blocked
**And** `notify_run_summary()` on MockNotifier captures a message with all counts

**Dependencies:** Story 7.1
**Story Points:** 2

---

### Story 7.8: Branch Management, Git & Surgical Development Tools Integration Tests

As a developer,
I want integration tests that verify branch creation, base branch resolution, git tool operations, and the surgical development tools (read_file, edit_file, grep, find_path, list_directory) on real (temp) repositories and file trees,
So that I'm confident the daemon manages git state correctly and the agent's surgical tools work end-to-end.

**Note:** This story assumes Epic 8 (Surgical Development Tooling) is complete. Tests target the new tools (`ReadFileTool`, `EditFileTool`, `GrepTool`, `FindPathTool`, `ListDirectoryTool`), not the legacy `FsTool`.

**Acceptance Criteria:**

**Given** a temp git repo with a `main` branch and an initial commit
**When** `ensure_story_branch("story/1-2-cli", "main")` is called
**Then** a new branch `story/1-2-cli` is created from `main`
**And** the repo HEAD is on `story/1-2-cli`

**Given** a temp git repo where branch `story/1-2-cli` already exists
**When** `ensure_story_branch("story/1-2-cli", "main")` is called again
**Then** the existing branch is checked out (not duplicated)
**And** no error is returned

**Given** a `StoryInfo` with dependencies `["1-1-scaffolding"]`
**And** a temp git repo with branches `main` and `story/1-1-scaffolding`
**When** `determine_base_branch()` is called
**Then** it returns `"story/1-1-scaffolding"` (last dependency's branch)

**Given** a `StoryInfo` with no dependencies
**When** `determine_base_branch()` is called
**Then** it returns the default branch (`"main"`)

**Given** a temp git repo with uncommitted changes
**When** `preserve_partial_work()` is called
**Then** all changes are staged and committed with a descriptive message containing the story key
**And** the commit exists in the repo's log

**Given** a temp project directory with multiple files (including a file > 300 lines)
**When** `ReadFileTool` is called with a line range on a small file
**Then** it returns only the specified lines with line numbers
**And** when called on the large file without a line range, it returns an outline with symbol names and line numbers

**Given** a temp project directory with a known file
**When** `EditFileTool` is called in `edit` mode with an `old_text` → `new_text` operation
**Then** only the targeted text is replaced in the file
**And** the rest of the file content is unchanged
**And** when called with a non-existent `old_text`, a clear error is returned without modifying the file

**Given** a temp project directory with multiple `.rs` and `.md` files containing known patterns
**When** `GrepTool` is called with a regex pattern and an `include_pattern` of `"**/*.rs"`
**Then** only matches from `.rs` files are returned with file paths and line numbers
**And** `.md` files are excluded from results

**Given** a temp project directory with a nested file structure
**When** `FindPathTool` is called with a glob pattern (e.g., `"**/*.rs"`)
**Then** matching file paths are returned sorted alphabetically
**And** files matching `.gitignore` patterns are excluded

**Given** a temp project directory
**When** `ListDirectoryTool` is called on a directory
**Then** it returns entries with types (file/directory) and sizes
**And** when called on a path outside the project root, a security error is returned

**Dependencies:** Story 7.1, Epic 8 (all stories complete)
**Story Points:** 5

---

### Story 7.9: CLI Lifecycle Integration Tests

As a developer,
I want integration tests that verify the CLI commands interact correctly with daemon state,
So that I'm confident the user experience of init → start → status → logs → stop is coherent.

**Acceptance Criteria:**

**Given** a temp directory with no daemon state file
**When** `DaemonState::read()` is called
**Then** `Ok(None)` is returned

**Given** a `DaemonState::new_running()` is created and written to a temp state file
**When** `DaemonState::read()` is called on that file
**Then** the state is deserialized correctly with matching PID, started_at, and status "running"

**Given** a running state is written
**When** `touch()` is called, then `record_story_processed()` twice, then state is re-written and re-read
**Then** `stories_processed == 2` and `last_activity` is updated

**Given** a running state is written
**When** `mark_stopped()` is called and state is re-written
**Then** re-reading shows `status: "stopped"`

**Given** a state file exists
**When** `cleanup()` is called
**Then** the file is removed
**And** subsequent `read()` returns `Ok(None)`

**Given** a valid `bmad-bot.yaml` is generated via the init flow helpers
**When** `BotConfig::load()` is called on the generated file
**Then** the config loads and validates successfully (round-trip test)

**Dependencies:** Story 7.1
**Story Points:** 2

---

### Story 7.10: Response Analyzer & Supervisor Rules Integration Tests

As a developer,
I want integration tests that verify the response analyzer and supervisor rule engine work correctly together,
So that I'm confident the chat loop handles all agent response patterns.

**Acceptance Criteria:**

**Given** an agent response containing a completion signal (e.g., "Implementation complete. All acceptance criteria met.")
**When** `ResponseAnalyzer::analyze()` processes it
**Then** it returns an action indicating session completion

**Given** an agent response asking which story to work on (e.g., "Which story should I implement?")
**When** `ResponseAnalyzer::analyze()` processes it
**Then** it returns a `Continue` action with the correct story key to reply with

**Given** an agent response asking for confirmation (e.g., "Should I proceed with the implementation?")
**When** the supervisor rule engine processes it
**Then** a deterministic "Yes, proceed." response is returned without LLM fallback

**Given** an agent response with a substantive question that doesn't match any rule
**When** the supervisor rule engine processes it
**Then** it falls through to LLM fallback (verified by checking that rules returned no match)

**Given** an agent response indicating step-by-step detection (e.g., "I'll work through this step by step...")
**When** `ResponseAnalyzer::analyze()` processes it
**Then** it returns a `Continue` action (agent should keep working)

**Dependencies:** Story 7.1
**Story Points:** 3

---

### Epic Summary

| Story | Title | Points | Dependencies |
|-------|-------|--------|--------------|
| 7.1 | Integration Test Infrastructure & Fixtures | 3 | Epic 8 |
| 7.2 | Config → Startup Validation | 2 | 7.1 |
| 7.3 | Watcher → Deps → Story Selection | 3 | 7.1 |
| 7.4 | Pipeline Orchestration | 5 | 7.1 |
| 7.5 | Session WAL Crash Recovery | 3 | 7.1 |
| 7.6 | Git Provider & PR Creation | 2 | 7.1 |
| 7.7 | Notification Flow | 2 | 7.1 |
| 7.8 | Branch Management, Git & Surgical Development Tools | 5 | 7.1 |
| 7.9 | CLI Lifecycle | 2 | 7.1 |
| 7.10 | Response Analyzer & Supervisor Rules | 3 | 7.1 |

**Total Story Points:** 30

**Execution Strategy:**
- ⚠️ **Entire Epic 7 is blocked until Epic 8 is complete** — integration tests must target the final tool surface
- Story 7.1 must be completed first (all others depend on the test infrastructure)
- Stories 7.2–7.10 can then be parallelized (independent module boundaries)
- Recommended priority order: 7.4 (pipeline — highest risk) → 7.5 (crash recovery — critical path) → 7.3 (watcher — core loop) → 7.8 (git + surgical tools) → 7.10 (analyzer — chat correctness) → 7.6, 7.7, 7.9, 7.2

**CI Integration:**
- All integration tests run via `cargo test --test integration` (no special env vars needed)
- Tests must complete in < 30 seconds total (no network calls, no LLM, only temp filesystem + git2)
- E2E tests (with real LLM) remain separate, gated behind `BMAD_E2E=1`

---

## Epic 8: Surgical Development Tooling

Replace the monolithic FsTool (from Epic 4, Story 4.1) with focused, Claude Code-style tools to dramatically improve agent token efficiency, code safety, and codebase navigation. After this epic, the dev agent edits files surgically instead of rewriting them, searches code with grep, and navigates with outlines — matching the capability level of modern AI coding assistants.

**Why this epic exists:** The current `FsTool` rewrites entire files on every edit, burning ~8x more tokens than necessary, risking code loss via LLM truncation, and forcing blind navigation of the codebase. Architecture Decision 7 specifies replacing it with 5 focused tools modeled on the proven Claude Code / Zed agent-mode pattern.

**Dependency order:** Stories must be implemented sequentially — each builds on the previous.

```
8.1 ReadFileTool ──► 8.2 EditFileTool ──► 8.3 Grep + FindPath ──► 8.4 ListDir + FsTool Removal ──► 8.5 Integration
```

**Impact:**

| Metric | Before | After |
|--------|--------|-------|
| Tokens per file edit (500-line file) | ~8,000 | ~900 |
| Risk of code loss (LLM truncation) | High | Near zero |
| Tool calls to find code | 5-10 (list/read loops) | 1-2 (grep → read range) |
| Agent tools registered | 5 | 9 |

**Reference documents:**
- `_bmad-output/planning-artifacts/architecture.md` — Decision 7 (full design specs)
- `_bmad-output/planning-artifacts/architect-brief-surgical-tooling.md` — Architect brief with rationale
- `src/tools/fs.rs` — Current FsTool implementation (to be replaced)
- `src/session/runner.rs` — Current preamble and agent builder (to be updated)

---

### Story 8.1: ReadFileTool — Partial Reading & Outline Mode

As a dev agent,
I want to read files with optional line ranges and get automatic outlines for large files,
So that I can navigate the codebase efficiently without wasting tokens on irrelevant content.

**Why first:** Every other tool depends on the agent being able to read files intelligently. This is the foundation.

**Replaces:** `FsTool` `read` action

**Acceptance Criteria:**

**Given** a file exists within the project root
**When** `read_file` is called with no line range parameters
**And** the file is ≤ 300 lines
**Then** the complete file content is returned with line numbers prepended to each line (1-indexed)

**Given** a file exists within the project root
**When** `read_file` is called with `start_line` and/or `end_line` parameters (1-indexed, inclusive)
**Then** only the specified line range is returned with line numbers prepended
**And** out-of-range values are clamped to file boundaries without error

**Given** a file exists within the project root
**When** `read_file` is called with no line range parameters
**And** the file is > 300 lines
**Then** an automatic outline is returned instead of full content
**And** the outline contains symbol names (functions, structs, impls, mods, classes, etc.) with their line numbers
**And** the outline is generated via regex-based symbol extraction (not AST parsing)

**Given** a path that resolves outside the project root
**When** `read_file` is called
**Then** the tool returns a clear security error
**And** no file content is read

**Given** the tool is implemented
**When** inspecting the code structure
**Then** it follows the standard rig Tool pattern (serializable struct + `ReadFileToolArgs` + `ReadFileToolError` thiserror enum + Tool trait impl)
**And** the tool NAME and definition description are detailed enough for the LLM to use correctly
**And** `tools/mod.rs` exports `ReadFileTool`
**And** a full unit test suite covers: normal read, line range read, outline mode, security boundary, edge cases (empty file, binary file, non-existent file)

**Dependencies:** None (first story in epic)
**Story Points:** 3

---

### Story 8.2: EditFileTool — Surgical Search-Replace Editing

As a dev agent,
I want to edit files surgically via search-replace operations instead of rewriting entire files,
So that I minimize token usage, eliminate truncation risk, and make precise targeted changes.

**Why second:** This is the biggest value unlock — surgical edits instead of full rewrites.

**Replaces:** `FsTool` `write` action

**Acceptance Criteria:**

**Given** an existing file within the project root
**When** `edit_file` is called with mode `edit` and a list of `EditOperation` pairs (`old_text` → `new_text`)
**And** each `old_text` exists exactly once in the file
**Then** all replacements are applied sequentially with offset recalculation
**And** the tool returns the affected line ranges for verification

**Given** an existing file within the project root
**When** `edit_file` is called with mode `edit`
**And** an `old_text` value does not exist in the file
**Then** the tool returns a clear error message indicating the text was not found
**And** no changes are made to the file

**Given** an existing file within the project root
**When** `edit_file` is called with mode `edit`
**And** an `old_text` value matches multiple locations in the file
**Then** the tool returns a clear error message with the line numbers of all matches
**And** no changes are made to the file (ambiguity must be resolved by the caller)

**Given** a path that does not exist
**When** `edit_file` is called with mode `create` and file content
**Then** a new file is created with the provided content
**And** parent directories are automatically created if they don't exist

**Given** a path that already exists
**When** `edit_file` is called with mode `create`
**Then** the tool returns a clear error (create mode must not overwrite existing files)

**Given** an existing file within the project root
**When** `edit_file` is called with mode `overwrite` and full content
**Then** the entire file content is replaced with the provided content

**Given** a path that does not exist
**When** `edit_file` is called with mode `overwrite`
**Then** the tool returns a clear error (overwrite mode requires the file to exist)

**Given** the tool is implemented
**When** inspecting the code structure
**Then** it follows the standard rig Tool pattern (serializable struct + `EditFileToolArgs` + `EditFileToolError` thiserror enum + Tool trait impl)
**And** `tools/mod.rs` exports `EditFileTool`
**And** a full unit test suite covers: single edit, multiple sequential edits, create mode, overwrite mode, not-found error, ambiguity error, security boundary, parent directory creation

**Dependencies:** Story 8.1 (EditFileTool may use ReadFileTool internally for validation)
**Story Points:** 5

---

### Story 8.3: GrepTool & FindPathTool — Codebase Search & Navigation

As a dev agent,
I want to search file contents with regex and find files by glob pattern,
So that I can locate code and files efficiently instead of blindly listing and reading directories.

**Why third:** The agent needs to find code before it can edit it. These two tools are independent of each other but small enough to combine into one story.

**New tools** (no replacement — these capabilities didn't exist before)

**Acceptance Criteria:**

**Given** a project codebase
**When** `grep` is called with a regex pattern
**Then** it returns matching lines with file paths, line numbers, and the matched content
**And** results are paginated with a default of 20 matches per page

**Given** a project codebase
**When** `grep` is called with a regex pattern and an `include_pattern` glob filter (e.g., `"**/*.rs"`)
**Then** only files matching the glob are searched

**Given** a project codebase with a `.gitignore` file
**When** `grep` is called
**Then** files matching `.gitignore` patterns are excluded from search results

**Given** a project codebase
**When** `grep` is called with a `context_lines` parameter
**Then** the specified number of lines above and below each match are included in the output

**Given** a project codebase
**When** `find_path` is called with a glob pattern (e.g., `"**/*.rs"`, `"src/**/mod.rs"`)
**Then** it returns matching file paths sorted alphabetically
**And** results are paginated with a default of 50 matches per page

**Given** a project codebase with a `.gitignore` file
**When** `find_path` is called
**Then** files matching `.gitignore` patterns are excluded from results

**Given** the tools are implemented
**When** inspecting the code structure
**Then** `tools/grep.rs` follows the standard rig Tool pattern (serializable struct + args + error enum + Tool trait impl)
**And** `tools/find_path.rs` follows the standard rig Tool pattern
**And** both use the `regex` crate (already a dependency) for pattern matching
**And** file traversal uses `walkdir` or `glob` crate (add to `Cargo.toml` if needed)
**And** `tools/mod.rs` exports both `GrepTool` and `FindPathTool`
**And** full unit test suites cover: basic search, glob filtering, gitignore respect, pagination, no-match cases, invalid regex handling

**Dependencies:** Story 8.1 (sequential ordering for sprint coherence)
**Story Points:** 5

---

### Story 8.4: ListDirectoryTool & FsTool Removal — Complete Migration

As a dev agent,
I want a dedicated directory listing tool and the legacy FsTool fully removed,
So that the tool set is clean, focused, and each tool has a single responsibility.

**Why fourth:** Extract the last useful action from FsTool, then remove the monolith entirely.

**Replaces:** `FsTool` `list` action. Removes `FsTool` `mkdir`, `delete`, `exists` (these operations are pushed to `TerminalTool`).

**Acceptance Criteria:**

**Given** a directory within the project root
**When** `list_directory` is called with a directory path
**Then** it returns the directory contents with entry types (file/directory) and file sizes
**And** results are sorted alphabetically (directories first, then files)

**Given** a path that resolves outside the project root
**When** `list_directory` is called
**Then** the tool returns a clear security error

**Given** a non-existent directory path
**When** `list_directory` is called
**Then** the tool returns a clear error indicating the directory was not found

**Given** the `ListDirectoryTool` is implemented
**When** `tools/fs.rs` (the old FsTool) is inspected
**Then** it has been completely removed from the codebase
**And** `tools/mod.rs` no longer exports `FsTool`
**And** `tools/mod.rs` exports `ListDirectoryTool`

**Given** `supervisor/read_tool.rs` previously used `FsTool` for file reading
**When** the migration is complete
**Then** `supervisor/read_tool.rs` uses `ReadFileTool` instead of `FsTool`

**Given** all changes are complete
**When** `cargo test` is run
**Then** all tests pass with zero references to `FsTool` in the codebase
**And** all prior `FsTool` unit tests have been migrated to the new tools or deleted (each new tool already has its own test suite from prior stories)

**Dependencies:** Story 8.3
**Story Points:** 3

---

### Story 8.5: Agent Integration — Preamble, Registration & Session Update

As a dev agent,
I want all 9 surgical tools registered and properly described in my session,
So that I can use the full tool set for efficient autonomous development.

**Why last:** This wires everything together — the agent session now uses the new tools.

**Acceptance Criteria:**

**Given** `session/runner.rs` contains `build_preamble()`
**When** Story 8.5 is complete
**Then** the preamble's tool list section is updated to describe all 9 tools: `read_file`, `edit_file`, `grep`, `find_path`, `list_directory`, `git`, `terminal`, `ask_supervisor`, `think`
**And** a "Tool Usage Rules" section is added per Architecture Decision 7 (e.g., "use grep to find code before editing", "use read_file with line ranges after outline", "prefer edit mode over overwrite")

**Given** the agent builders (`build_anthropic_agent`, `build_openai_agent`, `build_copilot_agent`)
**When** Story 8.5 is complete
**Then** all 3 builders register 9 tools: `edit_file`, `read_file`, `grep`, `find_path`, `list_directory`, `git`, `terminal`, `ask_supervisor`, `think` (ThinkTool)
**And** the previous 5-tool registration (git, fs, terminal, ask_supervisor, think) is replaced

**Given** `review/mod.rs` registers tools separately for the review agent
**When** Story 8.5 is complete
**Then** the review module's tool registration is updated if it references the old `FsTool`

**Given** an agent session is started
**When** the session initializes
**Then** all 9 tools are visible in the tool definitions sent to the LLM
**And** each tool's description is optimized for maximum LLM clarity (clear parameter descriptions, usage examples in description text)

**Given** all integration is complete
**When** a smoke test is run (agent session start)
**Then** the agent can successfully call each of the 9 tools
**And** no references to the old FsTool remain in session setup code

**Dependencies:** Story 8.4
**Story Points:** 3

---

### Epic 8 Summary

| Story | Title | Points | Dependencies |
|-------|-------|--------|--------------|
| 8.1 | ReadFileTool — Partial Reading & Outline Mode | 3 | — |
| 8.2 | EditFileTool — Surgical Search-Replace Editing | 5 | 8.1 |
| 8.3 | GrepTool & FindPathTool — Codebase Search & Navigation | 5 | 8.1 |
| 8.4 | ListDirectoryTool & FsTool Removal — Complete Migration | 3 | 8.3 |
| 8.5 | Agent Integration — Preamble, Registration & Session Update | 3 | 8.4 |

**Total Story Points:** 19

**Execution Strategy:**
- Stories must be executed sequentially: 8.1 → 8.2 → 8.3 → 8.4 → 8.5
- Stories 8.1 and 8.2 are tightly coupled (EditFileTool may use ReadFileTool internally for validation)
- Story 8.3 is independent of 8.2 in code but ordered after it for sprint coherence
- Story 8.4 is the cleanup and migration gate
- Story 8.5 is the integration point — nothing works end-to-end until this is done

**Existing Epics Impacted:**
- **Epic 4, Story 4.1** ("Rig Tools Implementation") — already implemented. Epic 8 is a refactoring follow-up. Story 4.1 is the baseline.
- **Epic 4, Story 4.2** ("Agent Session Setup & Chat Loop") — preamble changes in Story 8.5. Already implemented, needs update.

---

## Epic 9: MCP Client Integration — Dynamic External Tool Discovery

Connect to external MCP servers at daemon startup, discover their tools, and expose them to the rig agent alongside native tools — leveraging rig's built-in `McpTool` and `.rmcp_tools()` support. The autonomous agent gains browser automation (Playwright) and any future MCP-compatible tooling without custom tool implementations. Zero code changes to add a new MCP server — just a config entry.

**Why this epic exists:** The autonomous agent needs to verify its own work — launch a dev server, open a browser, confirm the app runs. Rather than building and maintaining custom tool implementations for each external capability, we connect to the MCP ecosystem. An architecture spike (2026-02-18) confirmed that rig already provides native MCP client support (`McpTool`, `AgentBuilder::rmcp_tools()`, `ToolDyn`), eliminating the need for a custom bridge layer.

**Dependency order:** Sequential — each story builds on the previous.

```
9.1 McpManager + Config ──► 9.2 Agent Integration ──► 9.3 Playwright Validation & Docs
```

**Reference documents:**
- `_bmad-output/planning-artifacts/architect-brief-mcp-client-integration.md` — Full brief with spike findings
- `src/llm/agent_factory.rs` — `AgentConfigurator` trait, `ToolConfigurator` struct, `configure_agent_tools!` macro
- `src/session/runner.rs` — `build_preamble()`, `configure_agent_tools!` usage
- `rig-core/src/tool/mod.rs` — `McpTool`, `ToolDyn` (behind `rmcp` feature flag)
- `rig-core/src/agent/builder.rs` — `AgentBuilder::rmcp_tools()`, `AgentBuilderSimple::rmcp_tools()`

---

### Story 9.1: MCP Server Lifecycle Management & Config

As a daemon operator,
I want the daemon to connect to external MCP servers at startup, discover their tools, and shut them down gracefully,
So that the agent gains access to external capabilities (browser automation, etc.) without custom tool implementations.

**Acceptance Criteria:**

**Given** `Cargo.toml` is updated
**When** the project is compiled
**Then** `rmcp` is added as a dependency with features `client` + `transport-child-process`
**And** the `rmcp` feature flag is enabled on `rig-core`
**And** the `rmcp` version is compatible with rig-core's dependency (currently 0.13)

**Given** `bmad-bot.yaml` contains an `mcp_servers` section with one or more server entries
**When** the daemon parses the config at startup
**Then** `BotConfig` exposes an optional `mcp_servers: Vec<McpServerConfig>` field (defaults to empty)
**And** each entry includes: `name` (String), `command` (String), `args` (Vec<String>), `transport` (enum, default stdio), `enabled` (bool, default true)
**And** entries with `enabled: false` are skipped

**Given** no `mcp_servers` section exists in the config
**When** the daemon starts
**Then** `McpManager` is initialized with zero servers (empty Vec)
**And** the daemon operates identically to before — zero behavioral change

**Given** a valid `mcp_servers` config with an enabled server (e.g., Playwright)
**When** `McpManager::init()` is called during daemon startup
**Then** the daemon spawns the server process via rmcp's stdio transport
**And** the MCP initialize handshake completes successfully (handled by rmcp)
**And** `list_tools()` is called on the connected server
**And** the discovered `Vec<rmcp::model::Tool>` and `ServerSink` are stored in `McpServerHandle`
**And** a `tracing::info!()` log records the server name and number of tools discovered

**Given** a configured MCP server command that does not exist on the system (e.g., `npx` not installed)
**When** `McpManager::init()` attempts to spawn it
**Then** the failure is logged via `tracing::warn!()` with the server name and error details
**And** the daemon continues startup without that server's tools
**And** other configured MCP servers are still attempted

**Given** a configured MCP server that fails the handshake or times out
**When** `McpManager::init()` attempts the connection
**Then** a configurable timeout (default 30s) bounds the handshake attempt
**And** the failure is logged via `tracing::warn!()`
**And** the daemon continues without that server

**Given** multiple MCP servers are configured and connected
**When** `McpManager::tools_for_builder()` is called
**Then** it returns `Vec<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)>` — one tuple per connected server
**And** each `ServerSink` is cloneable for use across sessions

**Given** the daemon receives SIGTERM/SIGINT
**When** cooperative shutdown begins
**Then** `McpManager::shutdown()` is called
**And** MCP `close` notifications are sent to all connected servers before dropping connections
**And** child processes are cleaned up

**Given** the module is implemented
**When** inspecting the code structure
**Then** `src/mcp/mod.rs` exports `McpManager` and `McpServerConfig`
**And** `src/mcp/manager.rs` contains `McpManager`, `McpServerHandle`, and `McpServerConfig`
**And** error types follow the per-module thiserror pattern (`McpError` enum)
**And** unit tests cover: empty config, successful connection (mocked), failed spawn, handshake timeout, graceful shutdown, `tools_for_builder()` output shape

**Dependencies:** None (first story in epic)

---

### Story 9.2: Agent Integration — Register MCP Tools on Session Build

As a dev agent,
I want MCP-discovered tools registered alongside my native tools when my session is built,
So that I can use browser automation and other external tools identically to edit_file, grep, terminal, etc.

**Acceptance Criteria:**

**Given** `ToolConfigurator` in `src/llm/agent_factory.rs` currently has a single `tools` field
**When** Story 9.2 is complete
**Then** `ToolConfigurator` has an additional `mcp_servers: Vec<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)>` field
**And** the `configure_agent_tools!` macro initializes `mcp_servers` to `vec![]` by default
**And** `ToolConfigurator` exposes a `with_mcp(self, servers: Vec<(Vec<rmcp::model::Tool>, ServerSink)>) -> Self` method for injection

**Given** `McpManager` has discovered tools from one or more MCP servers
**When** a `ToolConfigurator` is created via `configure_agent_tools!` and `.with_mcp(mcp_manager.tools_for_builder())`
**Then** each `configure_*` impl (configure_anthropic, configure_openai_responses, configure_openai_completions) chains `.rmcp_tools(tools, sink)` once per MCP server after the native `.tool()` calls and before `.build()`

**Given** no MCP servers are configured (empty vec)
**When** the `configure_*` methods execute
**Then** behavior is identical to before — no `.rmcp_tools()` calls, native tools only
**And** the `AgentConfigurator` trait signature is unchanged

**Given** the `ToolConfigurator` impl for 1-tool tuple (supervisor/architect use case in `ToolConfigurator<(T1,)>`)
**When** MCP tools are injected via `.with_mcp()`
**Then** MCP tools are also chained for the 1-tool configurator
**And** the supervisor/architect agent gains MCP tools if configured

**Given** `AgentFactory::build()` in `src/llm/agent_factory.rs` is called
**When** `McpManager` is available
**Then** `McpManager` (or a reference/clone of its tool data) is accessible to the configurator
**And** the call sites in `src/session/runner.rs`, `src/review/mod.rs`, and `src/supervisor/architect.rs` pass MCP data through to the configurator

**Given** MCP tools are registered on an agent
**When** `build_preamble()` in `src/session/dev_agent.rs` generates the system prompt
**Then** the preamble's tool list section includes the names of available MCP tools (e.g., `browser_navigate`, `browser_screenshot`, etc.)
**And** if no MCP tools are configured, the preamble is unchanged

**Given** an agent session is started with MCP tools registered
**When** the agent receives its tool definitions
**Then** both native tools (edit_file, grep, etc.) and MCP tools (browser_navigate, etc.) appear in the tool list
**And** the agent can call MCP tools — calls are proxied via rig's `McpTool` to the MCP server transparently

**Given** all changes are complete
**When** `cargo test` is run
**Then** all existing tests pass unchanged (zero regression on native tool registration)
**And** new unit tests verify: ToolConfigurator with empty mcp_servers behaves identically, ToolConfigurator with mocked MCP data chains rmcp_tools correctly, with_mcp builder method works

**Dependencies:** Story 9.1

---

### Story 9.3: Playwright MCP Validation & Documentation

As a daemon operator,
I want validated Playwright MCP integration and clear documentation on adding MCP servers,
So that I can confidently enable browser automation and extend the agent with new MCP tools in the future.

**Acceptance Criteria:**

**Given** Playwright MCP server (`@playwright/mcp`) is installed on the system
**When** the daemon starts with this config:
```yaml
mcp_servers:
  - name: playwright
    command: npx
    args: ["-y", "@playwright/mcp"]
    transport: stdio
    enabled: true
```
**Then** the daemon connects to the Playwright MCP server
**And** discovers browser automation tools (navigate, screenshot, click, fill, etc.)
**And** logs the discovered tool names and count

**Given** an agent session is active with Playwright MCP tools registered
**When** the agent calls `browser_navigate` with a URL
**Then** the Playwright MCP server opens a browser and navigates to the URL
**And** the result is returned to the agent as text content

**Given** an agent session is active with Playwright MCP tools registered
**When** the agent calls `browser_screenshot`
**Then** the Playwright MCP server captures a screenshot
**And** the result (base64 image data or confirmation) is returned to the agent

**Given** the Playwright MCP server crashes or becomes unresponsive mid-session
**When** the agent calls a Playwright tool
**Then** a clear error is returned to the agent via rig's `McpTool` error handling
**And** the agent can continue using native tools (edit_file, grep, terminal, etc.)
**And** the session is not terminated

**Given** a user wants to add a new MCP server (e.g., a database tool)
**When** they read the project documentation
**Then** `docs/mcp-servers.md` (or equivalent section in README) explains:
  - The `mcp_servers` config format with all fields
  - How to add a new server (one config entry, zero code changes)
  - How to disable a server without removing config (`enabled: false`)
  - Prerequisites (e.g., `npx` for npm-based MCP servers)
  - Troubleshooting: how to verify a server connects (check daemon logs for tool discovery messages)
**And** the Playwright example config is included as a reference

**Given** the documentation is complete
**When** reviewing the `bmad-bot.yaml` example/template
**Then** it includes a commented-out `mcp_servers` section showing the Playwright example

**Dependencies:** Story 9.2

---

### Epic 9 Summary

| Story | Title | Dependencies |
|-------|-------|--------------|
| 9.1 | MCP Server Lifecycle Management & Config | — |
| 9.2 | Agent Integration — Register MCP Tools on Session Build | 9.1 |
| 9.3 | Playwright MCP Validation & Documentation | 9.2 |

**Execution Strategy:**
- Stories must be executed sequentially: 9.1 → 9.2 → 9.3
- Story 9.1 is the heaviest (new module, config, lifecycle) but all components are tightly coupled
- Story 9.2 is a targeted refactor of existing `ToolConfigurator` — no new modules, no trait changes
- Story 9.3 is validation + docs — requires a working Playwright environment for manual E2E testing

**Key architecture decisions:**
- rig's native `rmcp` feature provides `McpTool` + `AgentBuilder::rmcp_tools()` — zero custom bridge code
- `AgentConfigurator` trait is unchanged — only `ToolConfigurator` struct and its impls are modified
- MCP failures are always non-blocking — the daemon never crashes due to an MCP server issue
- **Epic 7** (Integration Tests) — not yet implemented. Tool integration test stories (especially 7.8) should be written against the new surgical tools, not the old FsTool.

---

## Epic 10: Terminal UI & Developer Experience

Replace raw `tracing` stdout output with structured, user-facing terminal rendering in foreground mode (tmux, screen, interactive terminal). The daemon displays hierarchical progress indicators, pipeline phase tracking, agent tool call visibility, and LLM interaction status — similar to GitHub Copilot CLI and Claude Code. Powered by `indicatif` (spinners, progress) and `console` (colors, styles) behind a `UiRenderer` trait that enables future migration to `iocraft` (React-like declarative TUI) or `ratatui` (full TUI framework) without modifying business code.

**Why this epic exists:** The daemon runs as a foreground process (Architecture Decision 6), and the primary usage mode is tmux/screen. The current stdout output is raw `tracing_subscriber::fmt` logs — structured debug lines with timestamps, log levels, targets, and key-value fields. This makes it impossible to follow in real-time what the daemon is doing. ~100+ `tracing::info!/warn!/error!` calls across the codebase are oriented toward debugging, not user experience. Zero TUI dependencies exist in the project today.

**Dependency order:** Sequential — each story builds on the previous.

```
10.1 Foundation (trait + renderers) ──► 10.2 Pipeline Integration ──► 10.3 Session Integration ──► 10.4 Review Integration ──► 10.5 Polish
```

**Reference documents:**
- `_bmad-output/planning-artifacts/sprint-change-proposal-2026-03-05.md` — Full analysis and rationale
- `src/cli/mod.rs` — `init_tracing()` (current stdout layer to be replaced)
- `src/pipeline.rs` — `StoryPipeline`, `process_story()`, `process_eligible_stories()`
- `src/session/runner.rs` — `SessionRunner`, `run_session()`, `drive_activation_and_recover()`
- `src/review/mod.rs` — `ReviewRunner`, `drive_review_session()`
- `src/tools/*.rs` — Tool implementations with existing `tracing::info!(action = ...)` calls

---

### Story 10.1: Module `ui/` — Foundation, Trait & Console Renderer

As a daemon developer,
I want a `ui/` module with a `UiRenderer` trait, a `ConsoleRenderer` implementation, and a `NullRenderer`,
So that user-facing terminal output is decoupled from business logic and rendering backends can be swapped without code changes.

**Acceptance Criteria:**

**Given** `Cargo.toml` is updated
**When** the project is compiled
**Then** `indicatif` (latest stable) and `console` (latest stable) are added as dependencies
**And** the project compiles without warnings

**Given** the new `src/ui/` module
**When** I inspect the project structure
**Then** the following files exist:
- `src/ui/mod.rs` — `UiHandle` struct (wraps `Arc<dyn UiRenderer>`), convenience methods that delegate to the inner trait, `pub mod renderer; pub mod console; pub mod null;`
- `src/ui/renderer.rs` — `UiRenderer` trait with all method signatures
- `src/ui/console.rs` — `ConsoleRenderer` struct implementing `UiRenderer` using `indicatif::MultiProgress` and `console::style()`
- `src/ui/null.rs` — `NullRenderer` struct implementing `UiRenderer` as no-op
- `src/main.rs` — `mod ui;` added

**Given** the `UiRenderer` trait
**When** I inspect the method signatures
**Then** the following event categories are covered:
- **Pipeline events:** `story_start(key, title)`, `story_complete(key, pr_url)`, `story_error(key, error)`, `story_escalated(key, reason)`, `batch_start(count)`, `batch_complete(summary)`
- **Phase events:** `phase_start(phase_name)`, `phase_complete(phase_name, duration)`, `phase_error(phase_name, error)`
- **Session events:** `chat_turn(turn, summary)`, `activation_start()`, `activation_complete()`, `completion_detected(story_key)`
- **Tool events:** `tool_call(tool_name, detail)`, `tool_result(tool_name, detail)`
- **LLM events:** `llm_request(label, turn)`, `llm_response(label, turn, response_len)`, `llm_error(label, turn, error)`, `llm_retry(label, turn, retry_count, delay_secs)`
- **System events:** `daemon_start(config_summary)`, `poll_cycle(cycle_num)`, `stories_found(count)`, `crash_recovery_start()`, `crash_recovery_complete(story_key)`, `shutdown_requested()`
**And** the trait is `Send + Sync` (object safe)
**And** no `indicatif` or `console` types appear in the trait signature (backend-agnostic)

**Given** the `UiHandle` struct
**When** I inspect its implementation
**Then** it wraps `Arc<dyn UiRenderer>` and implements `Clone`, `Send`, `Sync`
**And** it exposes convenience methods that delegate to the inner trait (e.g., `ui.story_start(key, title)` calls `self.0.story_start(key, title)`)

**Given** the `ConsoleRenderer`
**When** I inspect its implementation
**Then** it uses `indicatif::MultiProgress` for managing concurrent spinners
**And** it uses `console::style()` for colored and styled text output
**And** the visual vocabulary is:
- `●` (green) — completed action
- `◉` (cyan, animated) — in-progress action (spinner)
- `└` — sub-detail / child event
- `✗` (red) — error
- `⚠` (yellow) — warning / escalation
- Indentation: 2 spaces per nesting level (pipeline → phase → tool)

**Given** the `NullRenderer`
**When** any method is called
**Then** it performs no I/O and returns immediately
**And** it can be used in unit tests and CI environments

**Given** `UiHandle` is used in tests
**When** I create a `UiHandle` with `NullRenderer`
**Then** all method calls compile and succeed without side effects

**Dev Notes:**

- The trait must be object-safe (no generics, no `Self: Sized` constraints, no associated types) so it can be wrapped in `Arc<dyn UiRenderer>`
- Consider adding a `PhaseGuard` pattern where `phase_start()` returns an opaque guard that auto-completes the phase on `Drop` — but this is optional for the first iteration
- `ConsoleRenderer` should use `MultiProgress::with_draw_target(ProgressDrawTarget::stderr())` to keep stdout clean if needed, or `stdout()` — test both and pick what works best with the file tracing layer
- The `indicatif` crate's `MultiProgress` is thread-safe and can be shared across async tasks without additional synchronization

---

### Story 10.2: Pipeline Integration — UI Events in Story Lifecycle

As a developer monitoring the daemon in tmux,
I want to see the full lifecycle of each story as it progresses through the pipeline,
So that I know at a glance which story is being processed, which phase is active, and whether things are succeeding or failing.

**Acceptance Criteria:**

**Given** `StoryPipeline` struct in `src/pipeline.rs`
**When** I inspect the struct definition
**Then** it contains a `ui: UiHandle` field
**And** `StoryPipeline::new()` accepts a `UiHandle` parameter

**Given** a story is processed via `process_story()`
**When** the pipeline progresses through each phase
**Then** the following UI events are emitted in order:
1. `ui.story_start(story_key, story_title)` — at the start of `process_story()`
2. `ui.phase_start("Dev Session")` — before `session_runner.run()`
3. `ui.phase_complete("Dev Session", duration)` — after session returns (success) OR `ui.phase_error("Dev Session", error)` — on failure
4. `ui.phase_start("Push Branch")` — before `push_branch()`
5. `ui.phase_complete("Push Branch", duration)` — after push
6. `ui.phase_start("Create PR")` — before `git_provider.create_pr()`
7. `ui.phase_complete("Create PR", duration)` — after PR created
8. `ui.phase_start("Code Review")` — before `review_runner.run()` (if enabled)
9. `ui.phase_complete("Code Review", duration)` — after review
10. `ui.phase_start("Notification")` — before `notify_story_result()`
11. `ui.phase_complete("Notification", duration)` — after notification sent
12. `ui.story_complete(story_key, pr_url)` — on success OR `ui.story_error(story_key, error)` — on failure OR `ui.story_escalated(story_key, reason)` — on escalation

**Given** multiple stories are processed via `process_eligible_stories()`
**When** the batch starts and ends
**Then** `ui.batch_start(count)` is emitted at the start with the number of eligible stories
**And** `ui.batch_complete(summary)` is emitted at the end with a human-readable run summary

**Given** a crash recovery is triggered via `recover_and_process()`
**When** a WAL file is detected at startup
**Then** `ui.crash_recovery_start()` is emitted before recovery begins
**And** `ui.crash_recovery_complete(story_key)` is emitted after recovery finishes

**Given** `cli/mod.rs` `run_start()` function
**When** the daemon starts
**Then** a `UiHandle` is created:
- `ConsoleRenderer` if stdout is a TTY and `ui_mode` is `"fancy"` (default) or `"plain"`
- `NullRenderer` if stdout is not a TTY, or `ui_mode` is `"silent"`
**And** the `UiHandle` is passed to `StoryPipeline::new()`
**And** `ui.daemon_start(config_summary)` is emitted after tracing is initialized

**Given** `init_tracing()` in `cli/mod.rs`
**When** `ConsoleRenderer` is active (TTY + fancy/plain mode)
**Then** the stdout `tracing` layer is **removed** — debug logs go to the JSON file layer only
**And** `ConsoleRenderer` takes over all user-facing terminal output
**When** `NullRenderer` is active (non-TTY or silent mode)
**Then** the stdout `tracing` layer is **preserved** for backward compatibility

**Given** the polling loop in `run_polling_loop()`
**When** a poll cycle finds no eligible stories
**Then** `ui.poll_cycle(cycle_num)` is emitted (quiet — no spinner, just a timestamp or nothing)
**When** a poll cycle finds eligible stories
**Then** `ui.stories_found(count)` is emitted before processing

**Given** all existing tests
**When** they run
**Then** they pass without modification (using `NullRenderer`)
**And** `StoryPipeline::new()` in tests receives a `UiHandle::null()` or equivalent

**Dev Notes:**

- Add `UiHandle::null() -> UiHandle` convenience constructor that wraps `NullRenderer`
- The `ui_mode` config field should be added to `BotConfig` with default `"fancy"`. Acceptable values: `"fancy"`, `"plain"`, `"silent"`
- For `"plain"` mode, `ConsoleRenderer` should disable colors and spinners (use `console::set_colors_enabled(false)` and static indicators instead of animated spinners)
- Duration tracking: use `std::time::Instant` around each phase to measure elapsed time
- The `SessionRunner` and `ReviewRunner` also need the `UiHandle` — pass it from pipeline. This wiring is done in this story, but the actual session/review events are emitted in Stories 10.3 and 10.4

---

### Story 10.3: Session Integration — Tool Calls & Chat Turns Visible

As a developer monitoring the daemon in tmux,
I want to see each agent tool call, chat turn, and LLM interaction in real-time,
So that I understand what the agent is doing without reading debug logs.

**Acceptance Criteria:**

**Given** `SessionRunner` struct in `src/session/runner.rs`
**When** I inspect the struct definition
**Then** it contains a `ui: UiHandle` field
**And** `SessionRunner::new()` accepts a `UiHandle` parameter (passed from pipeline)

**Given** the agent activation sequence in `drive_activation_and_recover()` / `run_session()`
**When** activation begins
**Then** `ui.activation_start()` is emitted
**When** activation completes successfully
**Then** `ui.activation_complete()` is emitted
**When** activation fails
**Then** `ui.phase_error("Agent Activation", error)` is emitted

**Given** the chat loop in `run_session()`
**When** a chat turn completes (response received from LLM)
**Then** `ui.chat_turn(turn_number, truncated_summary)` is emitted
**And** the summary is the first 80 characters of the response, truncated with `…` if longer
**When** `ResponseAction::Completed` is detected
**Then** `ui.completion_detected(story_key)` is emitted

**Given** the post-completion sequence in `run_session()`
**When** the final commit phase starts
**Then** `ui.phase_start("Final Commit")` is emitted
**When** the impact analysis phase starts
**Then** `ui.phase_start("Impact Analysis")` is emitted
**When** the PR summary phase starts
**Then** `ui.phase_start("PR Summary")` is emitted
**And** each phase emits `phase_complete` or `phase_error` on completion

**Given** an LLM request is sent via `stream_chat()`
**When** `log_llm_request()` is called in `run_session()`
**Then** `ui.llm_request(label, turn)` is also emitted (starts a thinking spinner)
**When** `log_llm_response()` is called
**Then** `ui.llm_response(label, turn, response_len)` is also emitted (resolves spinner)
**When** `log_llm_error()` is called
**Then** `ui.llm_error(label, turn, error)` is also emitted

**Given** a transient LLM error triggers a retry
**When** the retry loop in `run_session()` backs off
**Then** `ui.llm_retry(label, turn, retry_count, delay_secs)` is emitted
**And** the terminal shows the retry count and backoff duration

**Given** a Copilot token refresh is triggered
**When** `is_token_expired_error()` returns true and agent is rebuilt
**Then** `ui.llm_retry(label, turn, refresh_count, 0)` is emitted with a note about token refresh

**Given** the agent calls tools during the chat loop
**When** a tool is invoked (detected via existing `tracing::info!(action = "edit_file", ...)` etc.)
**Then** `ui.tool_call(tool_name, detail)` is emitted for each tool call
**And** the detail includes the key argument:
- `edit_file` → file path + mode (edit/create/overwrite)
- `read_file` → file path + line range if specified
- `grep` → regex pattern
- `find_path` → glob pattern
- `list_directory` → directory path
- `git` → sub-action (commit, checkout, add, etc.) + key arg (message, branch, etc.)
- `terminal` → command (first 80 chars)
- `ask_supervisor` → question (first 80 chars)

**Given** the existing `tracing::info!` calls in tool implementations (`src/tools/*.rs`)
**When** tool events need to be emitted to the UI
**Then** the `UiHandle` is passed to the tool call sites in `run_session()` — NOT injected into the tool structs themselves
**And** tool `tracing::info!` calls remain unchanged (they continue to log to the file)
**And** UI events are emitted at the session runner level by inspecting the tool action from the tracing context or by wrapping tool call sites

**Implementation Note:** The cleanest approach is to emit `ui.tool_call()` from the session runner level. Since rig handles tool dispatch internally during `stream_chat()`, and we don't control the tool call loop directly, consider one of these approaches:
1. **Hook into the tracing layer** — create a thin subscriber that captures tool action events and forwards to UiHandle (complex but non-invasive)
2. **Emit from tool implementations** — add an optional `UiHandle` to tool constructors (invasive but explicit)
3. **Parse tool calls from the rig stream** — inspect `MultiTurnStreamItem` variants in `streaming_chat()` for tool call deltas (cleanest for rig integration)

Choose the approach that best fits the rig streaming architecture. The AC requires tool calls to be visible — the implementation path is flexible.

**Dev Notes:**

- `streaming_chat()` in `session/agent.rs` receives `MultiTurnStreamItem` variants including tool call deltas. This is the most natural interception point for tool call visibility. Consider adding an optional `UiHandle` parameter to `streaming_chat()`.
- The `ChatHistoryHook` already captures full history including tool calls — this could be extended to emit UI events.
- Keep the existing `tracing::info!` calls in tools unchanged — they serve the debug log file. UI events are a separate concern.
- For the `ConsoleRenderer`, tool calls should appear indented under the current phase spinner, using the `└` prefix.

---

### Story 10.4: Review Integration — UI Events in Code Review

As a developer monitoring the daemon in tmux,
I want to see the code review cycle as it happens,
So that I know when the review starts, what fixes are applied, and whether it succeeds.

**Acceptance Criteria:**

**Given** `ReviewRunner` struct in `src/review/mod.rs`
**When** I inspect the struct definition
**Then** it contains a `ui: UiHandle` field
**And** `ReviewRunner::new()` accepts a `UiHandle` parameter (passed from pipeline)

**Given** a code review session starts via `drive_review_session()`
**When** the review agent is activated
**Then** `ui.activation_start()` is emitted (review context)
**When** activation completes
**Then** `ui.activation_complete()` is emitted

**Given** the review chat loop
**When** a review chat turn completes
**Then** `ui.chat_turn(turn, summary)` is emitted with `[review]` prefix in the summary
**When** the review agent applies fixes
**Then** `ui.tool_call("edit_file", path)` events are emitted (same pattern as Story 10.3)
**When** the review agent commits fixes
**Then** `ui.tool_call("git", "commit \"fix: ...\"")` is emitted

**Given** the review outcome
**When** review completes successfully
**Then** `ui.phase_complete("Code Review", duration)` is emitted by the pipeline (Story 10.2)
**When** review fails
**Then** `ui.phase_error("Code Review", error)` is emitted
**When** review is skipped (disabled or not applicable)
**Then** `ui.phase_complete("Code Review", Duration::ZERO)` is emitted with a skip note

**Given** the review report is posted as a PR comment
**When** the comment is posted successfully
**Then** `ui.tool_result("pr_comment", "Review posted")` is emitted
**When** the comment fails
**Then** `ui.tool_result("pr_comment", "Failed: {error}")` is emitted

**Dev Notes:**

- Reuse all patterns from Story 10.3 — the review session has the same streaming_chat / tool call structure
- The review uses the same BMAD dev persona (`dev.md`) with a different initial command (`"CR"` instead of `"DS"`)
- Review tool calls go through the same rig streaming pipeline — the same interception approach from Story 10.3 applies
- Review events should be visually distinguishable from dev session events (e.g., different color or prefix)

---

### Story 10.5: Polish — Visual Vocabulary, Colors & Final Formatting

As a developer using BMAD Bot daily,
I want a polished, professional terminal output that is consistent across terminals and configurable,
So that the daemon feels like a production-quality tool.

**Acceptance Criteria:**

**Given** the `ConsoleRenderer` implementation
**When** I review the visual output on different terminals
**Then** the following visual vocabulary is consistently applied:
- `●` (green) — completed action
- `◉` (cyan, animated spinner) — in-progress action
- `└` (gray) — sub-detail / child event
- `✗` (red) — error
- `⚠` (yellow) — warning / escalation / retry
- `→` (dim) — LLM request sent
- `←` (dim) — LLM response received
- Indentation: 2 spaces per nesting level
- Elapsed time displayed on completed phases: `● Dev Session [47s]`

**Given** the `ui_mode` configuration in `bmad-bot.yaml`
**When** `ui_mode` is set to `"fancy"` (default)
**Then** `ConsoleRenderer` uses animated spinners and full ANSI colors
**When** `ui_mode` is set to `"plain"`
**Then** `ConsoleRenderer` disables colors (`console::set_colors_enabled(false)`) and uses static indicators instead of animated spinners (e.g., `...` instead of `◉`)
**When** `ui_mode` is set to `"silent"`
**Then** `NullRenderer` is used — no stdout output at all

**Given** stdout is not a TTY (piped output, CI environment)
**When** the daemon starts
**Then** `NullRenderer` is automatically selected regardless of `ui_mode` setting
**And** the stdout `tracing` layer is preserved for backward compatibility

**Given** the `ConsoleRenderer` is active
**When** a long-running phase completes
**Then** the elapsed time is displayed in human-readable format:
- Under 60s: `[47s]`
- 1-60 minutes: `[3m 12s]`
- Over 60 minutes: `[1h 23m]`

**Given** the daemon processes a full story pipeline
**When** I observe the terminal output
**Then** the output resembles:

```
● BMAD Bot started — polling every 30s
● Found 2 eligible stories

● Pipeline: epic-4/story-4.2 — "Agent Session Setup & Chat Loop"
  ◉ Dev Session [turn 5/50]
    ● read_file src/session/runner.rs
      └ 3567 lines (outline mode)
    ● edit_file src/session/runner.rs (edit)
    ● git commit "feat(session): add context limit recovery"
    ● Terminal: cargo test session::tests
      └ 42 tests passed
  ● Dev Session [47s]
  ● Push Branch [2s]
  ● Create PR [1s]
    └ https://github.com/jbanety/bmad-bot/pull/42
  ◉ Code Review
    ● edit_file src/session/runner.rs (edit)
    ● git commit "fix(session): handle edge case in recovery"
  ● Code Review [23s]
  ● Notification [0s]
● epic-4/story-4.2 ✓ completed — PR #42

● Pipeline: epic-4/story-4.3 — "Pre-Development Preparation"
  ◉ Dev Session [turn 2/50]
    → LLM request (turn 2)
```

**Given** the README.md
**When** I inspect the documentation
**Then** there is a section describing the terminal output format
**And** it explains the `ui_mode` configuration option with examples
**And** it mentions TTY auto-detection behavior

**Given** all existing tests
**When** they run
**Then** they all pass without modification
**And** no test output is polluted by `ConsoleRenderer` output (all tests use `NullRenderer`)

**Dev Notes:**

- Use `console::Term::stdout().is_term()` for TTY detection
- For `"plain"` mode, `indicatif` supports `ProgressDrawTarget::hidden()` to disable animations while still tracking progress
- Consider adding a `--ui-mode` CLI flag as an override (optional, not required for this story)
- Test on: tmux, iTerm2, Terminal.app, VS Code integrated terminal, GitHub Actions (CI)
- The example output above is aspirational — the exact format may vary based on implementation, but the general structure and visual vocabulary must be followed

---

### Epic 10 Summary

| Story | Title | Points | Dependencies |
|-------|-------|--------|--------------|
| 10.1 | Module `ui/` — Foundation, Trait & Console Renderer | 5 | — |
| 10.2 | Pipeline Integration — UI Events in Story Lifecycle | 5 | 10.1 |
| 10.3 | Session Integration — Tool Calls & Chat Turns Visible | 8 | 10.2 |
| 10.4 | Review Integration — UI Events in Code Review | 3 | 10.3 |
| 10.5 | Polish — Visual Vocabulary, Colors & Final Formatting | 3 | 10.4 |

**Total Story Points:** 24

**Execution Strategy:**
- Stories must be executed sequentially: 10.1 → 10.2 → 10.3 → 10.4 → 10.5
- Story 10.1 is the foundation — defines the trait contract that all subsequent stories depend on
- Story 10.2 is the integration point — wires `UiHandle` through the daemon startup and pipeline, removes stdout tracing layer
- Story 10.3 is the heaviest — requires intercepting tool calls from the rig streaming pipeline and emitting granular session events
- Story 10.4 reuses all patterns from 10.3 — small incremental work
- Story 10.5 is polish — visual consistency, TTY detection, config option, documentation

**Existing Epics Impacted:**
- **Epics 1-6** (all implemented) — retroactive `UiHandle` insertion in `StoryPipeline`, `SessionRunner`, `ReviewRunner`, and `cli/run_start()`
- **Epic 7** (Integration Tests) — tests should use `NullRenderer` via `UiHandle::null()`
- **`Cargo.toml`** — new dependencies: `indicatif`, `console`
- **`bmad-bot.yaml`** — new optional config field: `ui_mode` (default: `"fancy"`)

**Key architecture decisions:**
- `UiRenderer` trait is backend-agnostic — no `indicatif` or `console` types in the trait signature. This enables future migration to `iocraft` (React-like declarative TUI) or `ratatui` (full TUI framework) by swapping the `ConsoleRenderer` implementation without touching business code.
- `UiHandle` wraps `Arc<dyn UiRenderer>` — `Send + Sync + Clone`, safe to share across async tasks. Propagated like `ShutdownFlag`: `cli/run_start()` → `StoryPipeline` → `SessionRunner` / `ReviewRunner`.
- Stdout `tracing` layer is removed when `ConsoleRenderer` is active — debug logs go to JSON file only. This eliminates the dual-output problem (tracing + UI fighting for stdout).
- Tool call visibility requires intercepting rig's streaming pipeline — the implementation approach (tracing hook, tool constructor injection, or stream item parsing) is left to the implementer based on what fits best with rig's architecture.
- `NullRenderer` ensures zero test pollution — all existing tests continue to pass without modification.

## Epic 11: Copilot Removal & Provider Simplification

The daemon supports only Anthropic and OpenAI-compatible providers (with optional `base_url` for any compatible endpoint). All GitHub Copilot code, authentication, and the rig fork are removed. The `AgentFactory` is simplified to two provider variants. After this epic, the codebase is leaner and uses the official `rig-core` crate.

### Story 11.1: Remove Copilot Auth Module

As a maintainer,
I want all GitHub Copilot authentication code removed from the codebase,
So that the project no longer carries ~1350 lines of OAuth Device Flow, token exchange, and caching code that is no longer needed.

**Acceptance Criteria:**

**Given** the `src/auth/github_copilot.rs` module exists (~1350 lines)
**When** this story is implemented
**Then** the entire `src/auth/` directory is deleted (`mod.rs` and `github_copilot.rs`)
**And** `mod auth;` is removed from `src/main.rs`
**And** all references to `CopilotTokenCache`, `exchange_copilot_token()`, `derive_base_url_from_token()`, `run_device_flow()`, `request_device_code()`, `poll_for_access_token()` are removed from the codebase
**And** the project compiles with zero warnings

**Given** `src/cli/mod.rs` contains the `copilot-login` subcommand
**When** this story is implemented
**Then** the `copilot-login` subcommand is removed from the clap CLI definition
**And** any interactive Copilot Device Flow trigger during `bmad-bot init` is removed

### Story 11.2: Simplify AgentFactory — OpenAI-Compatible with base_url

As a developer,
I want the `AgentFactory` to support only Anthropic and OpenAI-compatible providers with an optional `base_url`,
So that I can use any OpenAI-compatible endpoint (OpenAI direct, Ollama, LM Studio, vLLM, Groq) without Copilot complexity.

**Acceptance Criteria:**

**Given** the `BuiltAgent` enum in `src/llm/agent_factory.rs`
**When** this story is implemented
**Then** the `BuiltAgent::OpenAiCompletions` variant is removed (was Copilot-only)
**And** the remaining variants are `Anthropic` and `OpenAiCompatible`
**And** the `OpenAiCompatible` variant supports an optional `base_url` field — when provided, the OpenAI client is constructed with that base URL; when absent, defaults to `https://api.openai.com/v1`

**Given** the `AgentFactory::build()` method
**When** this story is implemented
**Then** the `"github-copilot"` match arm is removed entirely
**And** the `copilot_requires_responses_api()` function is deleted
**And** the `resolve_copilot_session()` method is deleted
**And** the `CopilotTokenCache` field is removed from `AgentFactory`
**And** only two match arms remain: `"anthropic"` → `BuiltAgent::Anthropic`, `"openai-compatible"` → `BuiltAgent::OpenAiCompatible`
**And** the `"openai-compatible"` arm reads `base_url` from the LLM role config and passes it to the OpenAI client builder

**Given** the config struct `LlmRoleConfig` (or equivalent)
**When** this story is implemented
**Then** a new optional field `base_url: Option<String>` is added
**And** validation ensures `base_url`, if provided, is a valid URL

### Story 11.3: Clean Provider Routing, Config & Secrets

As a developer configuring BMAD Bot,
I want the provider list, secrets, and config to reflect only Anthropic and OpenAI-compatible,
So that there is no residual Copilot configuration anywhere.

**Acceptance Criteria:**

**Given** `src/session/provider.rs`
**When** this story is implemented
**Then** the `"github-copilot"` match arm in `resolve_api_key()` is removed
**And** `copilot_headers()` function is deleted
**And** `create_completion_model()` no longer references Copilot

**Given** `src/config/mod.rs`
**When** this story is implemented
**Then** `BotSecrets.github_copilot_oauth_token` field is removed
**And** `VALID_LLM_PROVIDERS` is updated to `["anthropic", "openai-compatible"]`
**And** config validation accepts `base_url` as an optional field per LLM role

**Given** `src/cli/mod.rs`
**When** this story is implemented
**Then** `LLM_PROVIDERS` list is updated to `["anthropic", "openai-compatible"]`
**And** `default_model_for_provider("openai-compatible")` returns `"gpt-4.1"`
**And** `generate_env_file` no longer references `GITHUB_COPILOT_OAUTH_TOKEN`
**And** the init flow prompts for `base_url` when `openai-compatible` is selected (optional, with default hint)

**Given** all Copilot-related unit tests in the above modules
**When** this story is implemented
**Then** those tests are removed and replaced with tests for the `openai-compatible` provider with `base_url` (default and custom)

### Story 11.4: Migrate rig Fork to Official Crate

As a maintainer,
I want to use the official `rig-core` crate from crates.io instead of the forked repository,
So that I no longer maintain a fork and benefit from upstream updates.

**Acceptance Criteria:**

**Given** `Cargo.toml` references `rig-core` from `git = "https://github.com/jbanety/rig.git"` branch `fix/copilot-streaming-compat`
**When** this story is implemented
**Then** the dependency is changed to `rig-core = { version = "...", features = ["rmcp"] }` from crates.io
**And** the version selected is the latest stable release that includes the `rmcp` feature

**Given** the official `rig-core` crate is used
**When** `cargo build` is run
**Then** the project compiles without errors
**And** `cargo test` passes all remaining tests (Copilot tests already removed in prior stories)
**And** `cargo clippy -- -D warnings` reports zero warnings

**Given** the fork is no longer needed
**When** this story is complete
**Then** the `Cargo.lock` reflects only crates.io dependencies for rig-core (no git sources)

### Story 11.5: Update Documentation — Remove Copilot References

As a developer reading the documentation,
I want all references to GitHub Copilot removed and OpenAI-compatible with `base_url` documented,
So that the docs accurately reflect the current provider capabilities.

**Acceptance Criteria:**

**Given** `_bmad-output/project-context.md`
**When** this story is implemented
**Then** the "Multi-Provider LLM Config" section is updated to list only `anthropic` and `openai-compatible`
**And** the `base_url` option is documented with examples (Ollama, LM Studio)
**And** all references to `github-copilot`, `CopilotTokenCache`, Copilot streaming compat, and IDE-specific headers are removed
**And** the "External Integration Points" section no longer mentions GitHub Copilot token exchange

**Given** `bmad-bot.yaml.example`
**When** this story is implemented
**Then** example config shows `openai-compatible` with `base_url` examples (commented)
**And** no `github-copilot` provider appears anywhere

**Given** `README.md`
**When** this story is implemented
**Then** all references to `github-copilot`, `github-models`, Copilot OAuth, and Device Flow are removed
**And** the provider section documents `anthropic` and `openai-compatible` (with `base_url`)

### Epic 11 Summary

| Story | Title | Dependencies |
|-------|-------|--------------|
| 11.1 | Remove Copilot Auth Module | — |
| 11.2 | Simplify AgentFactory — OpenAI-Compatible with base_url | 11.1 |
| 11.3 | Clean Provider Routing, Config & Secrets | 11.2 |
| 11.4 | Migrate rig Fork to Official Crate | 11.3 |
| 11.5 | Update Documentation — Remove Copilot References | 11.4 |

**Execution Strategy:**
- Linear chain: 11.1 → 11.2 → 11.3 → 11.4 → 11.5
- Story 11.1 is pure deletion (~1350 lines removed)
- Story 11.2 restructures the AgentFactory enum and build method
- Story 11.3 cleans up all downstream config/routing references
- Story 11.4 switches the Cargo.toml dependency — must come after code changes to avoid compilation errors with the official crate
- Story 11.5 is documentation-only polish

**Dependencies:** None — Epic 11 can start immediately
**Risk:** 🟢 Low — pure cleanup + base_url addition

## Epic 12: Skill-Based Sessions & SpawnAgent Tool

The daemon activates agent sessions by loading BMAD skill files (`SKILL.md`) via the existing Zed-style XML context mechanism instead of persona files. The `ResponseAnalyzer` is simplified (no more menu/persona auto-response). A universal `spawn_agent` tool (Zed-inspired) is available in all sessions for LLM-initiated sub-agent delegation. After this epic, the bot speaks the BMAD v6.2+ skill language natively.

### Story 12.1: Parameterize Activation by Skill

As a daemon operator,
I want agent sessions to be activated by loading a BMAD skill (`SKILL.md`) instead of a persona file (`dev.md`),
So that the bot aligns with BMAD v6.2+ skill-based workflows and no longer depends on persona/menu interaction.

**Acceptance Criteria:**

**Given** the `activate_agent()` function in `src/session/agent.rs` currently accepts an agent file path (e.g., `_bmad/bmm/agents/dev.md`)
**When** this story is implemented
**Then** the function accepts a skill path (e.g., `.github/skills/bmad-dev-story/SKILL.md`) instead
**And** the `ContextBuilder` loads the `SKILL.md` content and wraps it in Zed-style XML context (`<context><files>...</files></context>`) exactly as it does today for persona files
**And** the function sends this as the first user message — the mechanism is unchanged, only the file content differs
**And** no post-activation command is sent — no `"DS"`, no `"CR"`, no hardcoded menu commands. The LLM reads the SKILL.md, discovers `./workflow.md` via the instructions, and loads it autonomously using `read_file`

**Given** `src/session/runner.rs` currently hardcodes `"_bmad/bmm/agents/dev.md"` as the persona path and sends `"Execute [DS] for story file: {path}"` after activation
**When** this story is implemented
**Then** `run_session()` accepts a `skill_path: &str` parameter instead of hardcoding the persona
**And** the post-activation message is either empty (the skill is self-starting) or a minimal contextual hint (e.g., the story file path as context), parameterized per caller
**And** the `build_agent_for_role()` function signature is updated to accept skill path

**Given** `src/review/mod.rs` currently loads `"_bmad/bmm/agents/dev.md"` and sends `"Execute [CR] for story file: {path}"`
**When** this story is implemented
**Then** the review runner loads `.github/skills/bmad-code-review/SKILL.md` instead
**And** no `"CR"` command is sent — the skill is self-directing

**Given** `build_preamble()` in `src/session/agent.rs` contains instructions about persona activation (processing agent file, executing activation steps, displaying menu)
**When** this story is implemented
**Then** those persona-specific instructions are removed
**And** the preamble retains: tool usage rules, branch management rules, completion sentinel (`<<BMAD_JOB_DONE>>`), English language override, and general operational instructions
**And** new instruction added: "When provided a SKILL.md file in context, follow its instructions completely. Use your tools to read any referenced workflow files."

### Story 12.2: Simplify ResponseAnalyzer

As a daemon operator,
I want the `ResponseAnalyzer` to focus only on essential detection patterns,
So that it no longer carries persona/menu-specific auto-response logic that is irrelevant with skill-based sessions.

**Acceptance Criteria:**

**Given** `src/session/analyzer.rs` contains patterns for auto-responding to persona-driven prompts
**When** this story is implemented
**Then** the following pattern categories are removed:
  - Menu display detection and auto-selection (`"DS"`, `"CR"`, `"CH"` patterns)
  - "Should I proceed?" / confirmation auto-response patterns
  - Story selection prompt auto-response
  - Step-by-step progress detection that triggers automatic responses
**And** any `ResponseAction::Continue { reply }` variants that carried hardcoded menu responses are removed

**Given** the simplified `ResponseAnalyzer`
**When** an agent response is analyzed
**Then** the following essential detections remain:
  - `Completed` — detection of `<<BMAD_JOB_DONE>>` sentinel or equivalent skill completion signal
  - `Escalated` — detection of supervisor escalation / needs-clarification signals
  - `Failed` — detection of fatal error patterns
  - `Continue` — with `NoReply` as default (the LLM continues working autonomously)
**And** `Continue { reply }` is only used when the daemon needs to inject consultation results (adversarial/critic findings) — this is prepared as an extension point for Epic 13

**Given** the supervisor `rules.rs` rule engine contains patterns that overlap with the removed analyzer patterns (confirmations, step-by-step, story selection)
**When** this story is implemented
**Then** those rules remain in the rule engine — they serve a different purpose (answering agent questions via `ask_supervisor` tool, not auto-responding to workflow prompts)

### Story 12.3: SpawnAgent Tool

As an LLM agent working in a BMAD session,
I want to spawn independent sub-agents for well-scoped tasks,
So that I can delegate research, parallel investigation, or specialized work without polluting my main context.

**Acceptance Criteria:**

**Given** a new tool file `src/tools/spawn_agent.rs`
**When** this story is implemented
**Then** a `SpawnAgentTool` struct implements the rig `Tool` trait with:
  - `NAME: "spawn_agent"`
  - `Args: SpawnAgentArgs { label: String, message: String, session_id: Option<String> }`
  - `Output: String` (JSON containing `session_id` and `output` or `error`)
  - `Error: SpawnAgentError` (thiserror enum)

**Given** the `SpawnAgentTool` receives args with no `session_id` (new session)
**When** `call()` is invoked
**Then** a fresh agent is built via `AgentFactory::build()` using the same provider/model as the parent session's role
**And** the `message` is sent as the first user prompt via `stream_chat()`
**And** the agent runs to completion (up to a configurable max turns, default 100)
**And** the final assistant message is captured as `output`
**And** a unique `session_id` (UUID) is generated and the sub-agent state (agent + chat history) is stored in a shared `HashMap<String, SubAgentState>` behind an `Arc<Mutex<>>`
**And** the tool returns JSON `{ "session_id": "...", "output": "..." }`

**Given** the `SpawnAgentTool` receives args with an existing `session_id` (follow-up)
**When** `call()` is invoked
**Then** the existing sub-agent state is retrieved from the `HashMap`
**And** the `message` is appended to the existing chat history
**And** `stream_chat()` continues with the full history
**And** the new final message is returned as `output`
**And** if the `session_id` is not found, an error is returned: `"No sub-agent session found for id: {session_id}"`

**Given** the `SpawnAgentTool` is constructed
**When** the tool definition is generated
**Then** the description includes comprehensive guidelines matching the Zed pattern:
  - Sub-agents don't see parent conversation history — include all relevant context in message
  - Subtasks must be concrete, well-defined, and self-contained
  - Don't use for tasks accomplishable with one or two tool calls
  - For follow-ups with session_id, send only a short direct message
  - Parallel delegation patterns for independent tasks

**Given** the sub-agent encounters an error during execution
**When** the error is handled
**Then** the tool returns JSON `{ "session_id": "...", "error": "..." }` (session_id included if a session was created before the error)
**And** the error is logged via `tracing::warn!`

### Story 12.4: Universal SpawnAgent Registration

As a daemon operator,
I want the `spawn_agent` tool registered in all agent sessions,
So that any LLM agent (dev, review, supervisor, critic) can delegate work to sub-agents.

**Acceptance Criteria:**

**Given** `src/session/agent.rs` defines `create_base_tools()` which returns the standard tool set for all sessions
**When** this story is implemented
**Then** `SpawnAgentTool` is included in `create_base_tools()` alongside git, read_file, edit_file, grep, find_path, list_directory, terminal
**And** the `SpawnAgentTool` is constructed with a shared `AgentFactory` reference and the shared sub-agent session map
**And** the tool is available in dev sessions, review sessions, and any future session types

**Given** the supervisor's `ArchitectSession` in `src/supervisor/architect.rs` currently implements a hardcoded 4-turn scripted conversation to answer questions
**When** this story is implemented
**Then** the `ArchitectSession` is evaluated for migration to the `spawn_agent` pattern
**And** if migration is feasible (the Architect can be invoked via a single spawn_agent call with appropriate context), it replaces the hardcoded script
**And** if migration introduces complexity or regressions, the existing implementation is kept with a TODO comment documenting the future migration path

**Given** the `SpawnAgentTool` needs shared state (`AgentFactory`, session map)
**When** the tool is constructed
**Then** `AgentFactory` is passed as `Arc<AgentFactory>` (already the case in the codebase)
**And** the sub-agent session map is `Arc<Mutex<HashMap<String, SubAgentState>>>` — created once per daemon run and shared across all tool instances
**And** session cleanup: sub-agent sessions are dropped when the parent story pipeline completes (not when the parent session ends, to allow cross-phase follow-ups within the same story)

### Story 12.5: Skill-Based Session & SpawnAgent Tests

As a maintainer,
I want comprehensive tests for skill-based activation and the SpawnAgent tool,
So that I can verify the new activation model works correctly and sub-agent delegation is reliable.

**Acceptance Criteria:**

**Given** the skill-based activation changes in `session/agent.rs`
**When** tests are run
**Then** the following unit tests exist and pass:
  - `test_build_preamble_contains_skill_instructions` — verifies preamble includes skill handling instructions and does NOT contain persona activation instructions
  - `test_build_preamble_retains_operational_rules` — verifies tool rules, branch rules, completion sentinel, language override are preserved
  - `test_activate_agent_loads_skill_file` — verifies `ContextBuilder` wraps SKILL.md content in Zed-style XML tags

**Given** the `SpawnAgentTool` in `tools/spawn_agent.rs`
**When** tests are run
**Then** the following unit tests exist and pass:
  - `test_spawn_agent_new_session_returns_session_id` — mock AgentFactory, verify UUID generated and output returned
  - `test_spawn_agent_follow_up_reuses_session` — create session, then follow-up with session_id, verify history continuity
  - `test_spawn_agent_invalid_session_id_returns_error` — verify descriptive error for unknown session_id
  - `test_spawn_agent_definition_contains_guidelines` — verify tool description includes delegation best practices
  - `test_spawn_agent_session_cleanup` — verify sessions are dropped on pipeline story completion

**Given** the `ResponseAnalyzer` simplification
**When** tests are run
**Then** tests for removed patterns (menu detection, confirmation auto-response) are deleted
**And** tests for retained patterns (completion sentinel, escalation, error detection) are preserved and pass
**And** a new test `test_analyzer_default_is_continue_no_reply` verifies that unrecognized responses result in `Continue` with no auto-reply

### Epic 12 Summary

| Story | Title | Dependencies |
|-------|-------|--------------|
| 12.1 | Parameterize Activation by Skill | — |
| 12.2 | Simplify ResponseAnalyzer | 12.1 |
| 12.3 | SpawnAgent Tool | — (parallel with 12.1) |
| 12.4 | Universal SpawnAgent Registration | 12.3 |
| 12.5 | Skill-Based Session & SpawnAgent Tests | 12.4 |

**Execution Strategy:**
- Two parallel branches: skill activation (12.1 → 12.2) and spawn_agent (12.3 → 12.4), converging at 12.5
- Story 12.1 is the key change — swaps the file loaded into ContextBuilder from persona to skill. The mechanism is identical.
- Story 12.3 is a new rig tool following the established Tool pattern — the most significant new code in this epic
- Story 12.4 evaluates whether the ArchitectSession can migrate to spawn_agent — pragmatic decision, not forced

**Dependencies:** Epic 11 (rig official crate in place)
**Risk:** 🟢 Low — the activation mechanism stays the same, only the loaded content changes

## Epic 13: Multi-Phase Pipeline & Story Critic

The pipeline orchestrates the full story lifecycle from `backlog` to `done`. For each story: a create-story session runs (with daemon-orchestrated adversarial and critic consultations fed back into the active session), then a dev-story session, then a code-review session (with critic consultation for decision-needed findings). The Story Critic is an independent vision guardian with persistent memory across stories, anchored by a project brief provided at init. After this epic, the bot autonomously creates, validates, implements, and reviews stories end-to-end.

### Story 13.1: Watcher Extension — Backlog Stories Eligible

As a daemon operator,
I want the watcher to detect stories in `backlog` status in addition to `ready-for-dev` and `review`,
So that the pipeline can pick up stories at the very beginning of their lifecycle and run the full create→dev→review flow.

**Acceptance Criteria:**

**Given** `src/watcher/mod.rs` currently filters stories to only `ready-for-dev` status
**When** this story is implemented
**Then** `eligible_stories()` returns stories with status `backlog`, `ready-for-dev`, or `review`
**And** the returned `StoryInfo` includes the `status` field so the pipeline can route accordingly

**Given** the dependency resolution in `src/watcher/deps.rs`
**When** a `backlog` story's dependencies are evaluated
**Then** the same dependency rules apply: a `backlog` story is only eligible if all its dependencies are `done`
**And** cascade blocking applies identically — if a prerequisite fails, dependent `backlog` stories are excluded

**Given** the watcher returns multiple eligible stories with mixed statuses
**When** the pipeline selects the next story to process
**Then** stories are prioritized: `review` first (resume interrupted work), then `ready-for-dev` (resume after create), then `backlog` (start fresh)
**And** within each status group, document-order topo sort applies as before
**And** only one story is processed at a time — the pipeline re-polls after each story completes

### Story 13.2: Pipeline Orchestrator Refonte

As a daemon operator,
I want the pipeline to orchestrate three types of sessions per story (create-with-consultations, dev, code-review-with-consultations),
So that each story flows through the full lifecycle autonomously.

**Acceptance Criteria:**

**Given** `src/pipeline.rs` currently has `process_story()` which runs dev session → push → PR → review → notify
**When** this story is implemented
**Then** `process_story()` implements a state machine that routes based on the story's current status:
  - `backlog` → run create-story phase (Story 13.4) → on success, continue to dev phase
  - `ready-for-dev` → run dev-story phase (Story 13.5) → on success, continue to review phase
  - `review` → run code-review phase (Story 13.6) → on success, push + PR + notify

**Given** a story enters `process_story()` at any status
**When** the pipeline processes it
**Then** it runs all remaining phases sequentially to `done` (e.g., a `backlog` story goes through create → dev → review → push)
**And** between each phase, the pipeline verifies the outcome — if any phase fails or escalates, the pipeline stops and handles the error (partial PR, notification, etc.)
**And** each phase creates a fresh agent session — no session state carries between phases

**Given** the pipeline re-polls after each story
**When** a story was interrupted mid-pipeline (e.g., crash during dev phase)
**Then** the next poll picks up the story at its current status (e.g., `ready-for-dev` if create completed but dev didn't start, `review` if dev completed but review didn't)
**And** the pipeline resumes from the correct phase

### Story 13.3: Daemon-Orchestrated Consultation Mechanism

As a daemon operator,
I want the session runner to support pausing an active session, running a fresh consultation agent, and feeding results back to the paused session,
So that sessions can be enriched with external perspectives (adversarial review, critic) without losing their BMAD context.

**Acceptance Criteria:**

**Given** the session runner manages a chat loop via `stream_chat(agent, prompt, history)`
**When** a consultation is triggered (daemon detects a phase-completion pattern in the agent's response)
**Then** the session is paused: the current `chat_history` and agent state are held in memory
**And** a fresh consultation agent is built via `AgentFactory::build()` with its own preamble, tools, and context
**And** the consultation agent is run to completion via `stream_chat()` — it receives the artifact to review as its prompt
**And** the consultation agent's final output (findings, decisions) is captured as a `String`
**And** the paused session is resumed: findings are sent as a new user message to the original agent, which applies them with its full BMAD context intact

**Given** a `ConsultationConfig` struct defining a consultation
**When** the daemon sets up a pipeline phase
**Then** each consultation is configured with:
  - `skill_path: Option<String>` — SKILL.md to load for the consultation agent (if skill-based, e.g., adversarial review)
  - `preamble_override: Option<String>` — custom preamble (for non-skill agents like the critic)
  - `context_files: Vec<String>` — additional files to load into the agent's context
  - `trigger_pattern: String` — regex or keyword the daemon watches for in the main session's output to trigger the consultation
  - `resume_message_template: String` — template for the message sent to the main session with `{findings}` placeholder

**Given** a consultation agent encounters an error
**When** the error is handled
**Then** the paused session is resumed with an error message: "Consultation failed: {error}. Continue without external input."
**And** the pipeline does not abort — the main session continues best-effort
**And** the error is logged via `tracing::warn!`

### Story 13.4: Create-Story Phase with Consultations

As a daemon operator,
I want the create-story pipeline phase to run a `bmad-create-story` session enriched with adversarial review and critic consultations,
So that every story file is adversarially validated and vision-checked before development begins.

**Acceptance Criteria:**

**Given** a story with status `backlog` enters the create-story phase
**When** the phase runs
**Then** a fresh agent session is created, activated with `.github/skills/bmad-create-story/SKILL.md`
**And** the agent runs autonomously — discovers the target story from `sprint-status.yaml`, creates the story file, transitions the story to `ready-for-dev`
**And** the daemon monitors the session for the completion signal

**Given** the create-story session signals completion (story file created)
**When** the daemon detects the completion pattern
**Then** **Consultation 1 — Adversarial Review** is triggered:
  - A fresh agent is built and activated with `.github/skills/bmad-review-adversarial-general/SKILL.md`
  - The newly created story file content is provided as input
  - The adversarial agent produces findings
  - Findings are sent back to the create-story session as a message: "An external adversarial reviewer has analyzed this story and found the following issues:\n\n{findings}\n\nPlease fix all these issues and update the story file."
  - The create-story agent applies corrections with its BMAD context

**Given** the adversarial corrections are applied
**When** the create-story agent signals it has finished applying fixes
**Then** **Consultation 2 — Story Critic** is triggered:
  - A fresh Critic agent is built (see Story 13.9) with project brief + `critic-memory.md` + updated story file
  - The Critic produces observations and proposed modifications
  - The Critic updates `critic-memory.md` with its observations
  - Findings are sent back to the create-story session: "An external product/technical vision reviewer has analyzed this story:\n\n{findings}\n\nPlease apply the relevant corrections to the story file."
  - The create-story agent applies corrections

**Given** both consultations are complete and corrections applied
**When** the create-story agent finishes
**Then** a final commit is made with all story file changes
**And** the phase completes successfully with the story in `ready-for-dev` status

### Story 13.5: Dev-Story Phase

As a daemon operator,
I want the dev-story pipeline phase to run a `bmad-dev-story` session,
So that the validated story is implemented autonomously.

**Acceptance Criteria:**

**Given** a story with status `ready-for-dev` enters the dev-story phase
**When** the phase runs
**Then** a fresh agent session is created, activated with `.github/skills/bmad-dev-story/SKILL.md`
**And** the session follows the existing session runner flow: branch creation/checkout, streaming chat loop, tool calls, completion detection
**And** the `ask_supervisor` tool is registered and available (3-tier cascade: rules → architect → escalation)
**And** the `spawn_agent` tool is registered and available

**Given** the dev-story session completes successfully
**When** the agent signals `<<BMAD_JOB_DONE>>`
**Then** the story status transitions to `review`
**And** the session outcome includes: branch name, decisions log, PR context, test results
**And** any post-implementation impact analysis runs as before (Story 4.6 behavior preserved)

**Given** the dev-story session escalates or fails
**When** the session outcome is `Escalated` or `Failed`
**Then** the pipeline handles it identically to the current behavior: partial PR for failures, `needs-clarification` status for escalations, notification sent

### Story 13.6: Code-Review Phase with Critic Consultation

As a daemon operator,
I want the code-review pipeline phase to invoke the critic for `decision-needed` findings,
So that ambiguous code review findings are resolved by the vision guardian instead of blocking on human input.

**Acceptance Criteria:**

**Given** a story with status `review` enters the code-review phase
**When** the phase runs
**Then** a fresh agent session is created, activated with `.github/skills/bmad-code-review/SKILL.md`
**And** the session runs the code review workflow autonomously

**Given** the code-review session produces findings classified as `decision-needed`
**When** the daemon detects decision-needed findings in the session output
**Then** **Consultation — Critic Decision Resolution** is triggered:
  - A fresh Critic agent is built with project brief + `critic-memory.md` + the decision-needed findings + story file
  - The Critic analyzes each finding against accumulated project knowledge and vision
  - The Critic produces decisions for each finding (resolve as `patch`, `defer`, or `dismiss`) with rationale
  - The Critic updates `critic-memory.md` with the decisions made
  - Decisions are sent back to the code-review session: "An external vision reviewer has resolved the following decision-needed findings:\n\n{decisions}\n\nPlease apply accordingly."
  - The code-review agent applies the decisions

**Given** the code-review session has no `decision-needed` findings (only `patch`, `defer`, `dismiss`)
**When** the review completes
**Then** no Critic consultation is triggered — the review proceeds directly to completion

**Given** the code-review session completes
**When** all findings are resolved
**Then** the story status transitions to `done`
**And** review fixes are committed separately from dev commits
**And** the phase outcome includes the review report for the PR comment

### Story 13.7: Config Init — Project Brief

As a developer setting up BMAD Bot,
I want to provide a project brief file path during `bmad-bot init`,
So that the Story Critic has a vision anchor independent from BMAD artifacts.

**Acceptance Criteria:**

**Given** the `bmad-bot init` interactive flow in `src/cli/mod.rs`
**When** this story is implemented
**Then** a new prompt is added after the existing configuration steps: "Do you have a project brief file? (path or skip)"
**And** if a path is provided, the file existence is validated
**And** the path is stored in `bmad-bot.yaml` as `project_brief: "{path}"` (relative to project root)
**And** if skipped, no `project_brief` field is written (optional config)

**Given** the `BotConfig` struct in `src/config/mod.rs`
**When** this story is implemented
**Then** a new optional field `project_brief: Option<String>` is added
**And** if provided, the file existence is validated at startup (non-fatal warning if missing — the Critic can work without it but with degraded context)

**Given** no project brief is configured
**When** the Critic agent is constructed
**Then** it falls back to loading the PRD or any available BMAD planning artifact as its vision anchor
**And** a `tracing::info!` message notes the fallback: "No project brief configured, using PRD as Critic vision anchor"

### Story 13.8: Critic Memory System

As a daemon operator,
I want a persistent memory file that accumulates the Story Critic's observations across all stories,
So that the Critic can reference previous decisions and maintain vision continuity throughout the sprint.

**Acceptance Criteria:**

**Given** the implementation artifacts directory
**When** the first Critic invocation occurs
**Then** a `critic-memory.md` file is created at `{implementation_artifacts}/critic-memory.md` if it does not exist
**And** the file is initialized with a header: `# Story Critic Memory` and the current date

**Given** the Critic agent completes a review (story review or decision resolution)
**When** the Critic produces its output
**Then** the Critic agent appends a new section to `critic-memory.md` with:
  - Timestamp and story key
  - Type of review (story review or decision resolution)
  - Key observations and rationale
  - Decisions made and why
  - Any concerns or patterns noticed across stories
**And** the Critic manages the format of its own memory — no rigid structure is imposed by the daemon

**Given** `critic-memory.md` grows over time
**When** the file exceeds a configurable size threshold (default: 50KB)
**Then** a `tracing::warn!` is emitted suggesting manual review or summarization
**And** the pipeline does NOT auto-truncate — the Critic's memory is sacred and only the human should decide to prune it

**Given** a new sprint starts or the user wants a fresh Critic
**When** the user deletes or renames `critic-memory.md`
**Then** the next Critic invocation creates a fresh memory file
**And** no error occurs — absence of memory is a valid starting state

### Story 13.9: Critic Agent — Prompt Engineering & Construction

As a daemon operator,
I want the Story Critic to be an independent vision guardian with extended thinking and its own review perspective,
So that it provides non-BMAD, vision-anchored critique of stories and decisions.

**Acceptance Criteria:**

**Given** a Critic agent needs to be constructed for a consultation
**When** the daemon builds the Critic agent
**Then** the agent is built via `AgentFactory::build()` using a new `LlmRole::Critic` from config
**And** the `LlmRole::Critic` allows configuring a different provider/model optimized for reasoning (e.g., a model with extended thinking capabilities)
**And** the `BotConfig` and `LlmConfig` structs are extended with a `critic` role alongside `dev`, `review`, `supervisor`

**Given** the Critic agent's preamble (system prompt)
**When** the agent is constructed
**Then** the preamble establishes the Critic's identity and role:
  - "You are an independent product and technical vision guardian. You are NOT part of the BMAD methodology — you are an external advisor."
  - "Your job is to ensure that what is being built aligns with the original project vision."
  - "You have persistent memory across stories — read your memory file carefully to maintain continuity."
  - "Be direct, specific, and constructive. Flag deviations from the vision. Propose concrete corrections."
  - "When making decisions on ambiguous findings, reference your accumulated knowledge of prior stories and decisions."
**And** tool usage instructions are included (read_file, edit_file for critic-memory, think for reasoning)

**Given** the Critic is invoked for a story review
**When** the agent's context is assembled
**Then** the following are loaded via `ContextBuilder`:
  - The project brief file (from config, or PRD as fallback)
  - `critic-memory.md` (full content)
  - The story file being reviewed
**And** the prompt asks: "Review this story against the original project vision. Read your memory for context on previous stories. Identify any deviations, missing considerations, or improvements. Propose specific modifications. Then update your memory file with your observations."

**Given** the Critic is invoked for decision resolution
**When** the agent's context is assembled
**Then** the following are loaded:
  - The project brief
  - `critic-memory.md`
  - The story file
  - The `decision-needed` findings with their full detail
**And** the prompt asks: "These findings need a decision. Based on the project vision and your accumulated knowledge of prior stories and decisions, resolve each finding. Provide clear rationale referencing specific prior decisions when relevant. Then update your memory file."

**Given** the Critic agent has tools available
**When** the agent runs
**Then** the following tools are registered: `read_file`, `edit_file` (for updating critic-memory.md), `grep`, `find_path`, `list_directory`, `think`
**And** `git`, `terminal`, `ask_supervisor`, and `spawn_agent` are NOT registered — the Critic is read-only on the codebase except for its own memory file

### Story 13.10: WAL with Pipeline Phase Tracking

As a daemon operator,
I want the WAL (Write-Ahead Log) to track which pipeline phase a story is in,
So that crash recovery resumes at the correct phase instead of restarting from scratch.

**Acceptance Criteria:**

**Given** the WAL file at `{implementation_artifacts}/.bmad-bot-session.yaml`
**When** this story is implemented
**Then** a new field `pipeline_phase` is added to the WAL structure with values: `create`, `create-adversarial-consult`, `create-critic-consult`, `dev`, `review`, `review-critic-consult`
**And** the `pipeline_phase` is updated at each phase transition before the phase starts

**Given** the daemon starts and finds an existing WAL file
**When** crash recovery is attempted
**Then** the `pipeline_phase` field is read to determine where the story was in the pipeline
**And** recovery routes to the correct phase:
  - `create` / `create-*-consult` → restart create-story phase from scratch (consultations are lightweight, safe to redo)
  - `dev` → attempt dev session recovery using existing WAL chat history (existing behavior)
  - `review` / `review-critic-consult` → restart code-review phase from scratch
**And** `tracing::info!` logs the recovery: "Recovering story {key} from pipeline phase: {phase}"

**Given** a pipeline phase completes successfully
**When** the next phase starts
**Then** the WAL is updated with the new phase before the phase begins
**And** when the entire story pipeline completes (push + PR + notify), the WAL is deleted as before

### Story 13.11: UI Events for New Pipeline Phases

As a developer monitoring the daemon,
I want terminal UI events for all new pipeline phases and consultations,
So that I can follow the full create→adversarial→critic→dev→review flow in real-time.

**Acceptance Criteria:**

**Given** the `UiRenderer` trait in `src/ui/mod.rs`
**When** this story is implemented
**Then** the following new event methods are added:
  - `phase_start(&self, phase: &str)` — already exists, reused for new phases
  - `consultation_start(&self, consultation_type: &str, story_key: &str)` — new, shows "Consulting {type}..."
  - `consultation_complete(&self, consultation_type: &str, findings_count: usize)` — new, resolves with findings summary
  - `critic_memory_update(&self, story_key: &str)` — new, shows Critic memory was updated

**Given** the `ConsoleRenderer` implementation
**When** new events are emitted
**Then** the visual output follows the existing vocabulary:
  - `◉ Creating story 4-2...` (spinner for create phase)
  - `  └─ 🔍 Consulting adversarial reviewer...` (sub-spinner, indented)
  - `  └─ ● Adversarial review: 7 findings` (resolved)
  - `  └─ 🧠 Consulting story critic...` (sub-spinner)
  - `  └─ ● Story critic: 3 observations, memory updated` (resolved)
  - `◉ Developing story 4-2...` (spinner for dev phase)
  - `◉ Reviewing story 4-2...` (spinner for review phase)
  - `  └─ 🧠 Consulting critic for 2 decision-needed findings...`

**Given** the `NullRenderer` implementation
**When** new events are emitted
**Then** all new methods are no-ops (consistent with existing pattern)

### Epic 13 Summary

| Story | Title | Dependencies |
|-------|-------|--------------|
| 13.1 | Watcher Extension — Backlog Stories Eligible | — |
| 13.2 | Pipeline Orchestrator Refonte | 13.1 |
| 13.3 | Daemon-Orchestrated Consultation Mechanism | 13.2 |
| 13.4 | Create-Story Phase with Consultations | 13.3, 13.9 |
| 13.5 | Dev-Story Phase | 13.3 |
| 13.6 | Code-Review Phase with Critic Consultation | 13.3, 13.9 |
| 13.7 | Config Init — Project Brief | — (parallel with 13.1) |
| 13.8 | Critic Memory System | 13.7 |
| 13.9 | Critic Agent — Prompt Engineering & Construction | 13.8 |
| 13.10 | WAL with Pipeline Phase Tracking | 13.2 |
| 13.11 | UI Events for New Pipeline Phases | 13.2 |

**Execution Strategy:**
- Two parallel branches converge: pipeline (13.1 → 13.2 → 13.3 → 13.4/5/6) and critic (13.7 → 13.8 → 13.9)
- Stories 13.4 and 13.6 depend on BOTH branches (pipeline mechanism + critic agent)
- Stories 13.10 and 13.11 can be developed in parallel with 13.3+ (they only depend on 13.2)
- Story 13.9 is the prompt engineering challenge — the Critic's effectiveness depends on preamble quality

**Dependencies:** Epic 12 (skill-based activation + spawn_agent)
**Risk:** 🟡 Medium — the Critic is the most novel component, requiring iterative prompt engineering

## Epic 14: Epic Review Enhancement & Deferred Work

The epic review agent (Winston) reads `deferred-work.md` and combines it with findings from its own code analysis to propose pre-epic cleanup/improvement stories. These are injected at the head of the next epic in `sprint-status.yaml` as `backlog` stories with convention `X-0-pre-epic-X-{slug}`. Processed debt items are purged from `deferred-work.md`. After this epic, technical debt is managed rhythmically at epic boundaries.

### Story 14.1: Winston Reads Deferred Work

As a daemon operator,
I want the epic review agent (Winston) to read `deferred-work.md` as part of its analysis,
So that accumulated technical debt is evaluated alongside the epic's code quality.

**Acceptance Criteria:**

**Given** `src/review/epic.rs` builds an epic review prompt via `build_epic_review_prompt()`
**When** this story is implemented
**Then** the prompt includes an instruction to read `{implementation_artifacts}/deferred-work.md` via tools if the file exists
**And** Winston is instructed to categorize deferred items by severity (critical/high/medium/low) and by effort (small/medium/large)
**And** Winston integrates the deferred items into the "Technical Analysis" section of its report alongside his own code-level findings

**Given** `deferred-work.md` does not exist or is empty
**When** the epic review runs
**Then** Winston notes "No deferred work items found" in his report and continues normally
**And** no error is raised — the file is optional

**Given** `deferred-work.md` contains items from multiple past reviews (different stories, different dates)
**When** Winston reads the file
**Then** Winston considers the age and origin of each item — older items that persist across multiple stories are flagged as higher priority
**And** the report section explicitly calls out items that have been deferred for more than one epic

### Story 14.2: Pre-Epic Story Generation

As a daemon operator,
I want Winston to propose pre-epic cleanup stories from both `deferred-work.md` and his own code analysis findings,
So that technical debt and improvements are addressed before the next epic's feature work begins.

**Acceptance Criteria:**

**Given** Winston completes his epic review analysis (code analysis + deferred work review)
**When** the report is generated
**Then** a new section **"Pre-Epic Stories for Epic {N+1}"** is appended to the report
**And** each proposed story follows this format:
  - Story key: `{N+1}-0-pre-epic-{N+1}-{slug}` (e.g., `5-0-pre-epic-5-fix-error-handling`)
  - Title: descriptive, action-oriented
  - Source: `deferred-work` or `epic-review-finding` or `both`
  - Severity: critical/high/medium/low
  - Estimated effort: small/medium/large
  - Justification: why this should be addressed before epic N+1 feature work
  - Related deferred items: list of `deferred-work.md` item IDs this story would resolve (if applicable)

**Given** Winston identifies findings from his own code analysis that are not in `deferred-work.md`
**When** he generates pre-epic stories
**Then** these findings are included alongside deferred items — the two sources are merged into a unified prioritized list
**And** the report distinguishes the source of each proposed story (deferred vs epic-review vs both)

**Given** Winston evaluates the combined list of proposed stories
**When** the total exceeds a reasonable scope for pre-epic cleanup
**Then** Winston recommends a prioritized subset (top items by severity × effort ratio) as "must-do before Epic {N+1}"
**And** remaining items are listed as "can defer further" with rationale

### Story 14.3: Inject Pre-Epic Stories into Sprint Status

As a daemon operator,
I want Winston's approved pre-epic stories to be automatically added to `sprint-status.yaml`,
So that the linear pipeline processes them before the next epic's regular stories.

**Acceptance Criteria:**

**Given** Winston's epic review report contains proposed pre-epic stories
**When** the epic review phase completes (the report is generated and saved)
**Then** the daemon parses the "Pre-Epic Stories" section from Winston's report
**And** each proposed story is added to `sprint-status.yaml` under the next epic with status `backlog`
**And** pre-epic stories are inserted BEFORE the regular stories of the next epic (position `X-0` ensures document-order topo sort processes them first)

**Given** the naming convention `{N+1}-0-pre-epic-{N+1}-{slug}`
**When** multiple pre-epic stories are generated
**Then** they are numbered with sub-indices to maintain order: `5-0a-pre-epic-5-fix-error-handling`, `5-0b-pre-epic-5-missing-tests`, etc.
**And** dependencies between pre-epic stories are set sequentially (0b depends on 0a) to ensure ordered processing

**Given** pre-epic stories are inserted into `sprint-status.yaml`
**When** the daemon's next poll cycle runs
**Then** the watcher picks up the `backlog` pre-epic stories as eligible
**And** they are processed through the full pipeline (create → adversarial → critic → dev → review) like any regular story
**And** the linear pipeline naturally processes all `X-0*` stories before `X-1`, `X-2`, etc. due to document order

**Given** the `sprint-status.yaml` is updated with pre-epic stories
**When** the update is complete
**Then** the changes are committed with message `chore(sprint): add pre-epic-{N+1} debt stories from epic-{N} review`
**And** `tracing::info!` logs the number of pre-epic stories injected

### Story 14.4: Purge Processed Deferred Items

As a daemon operator,
I want resolved deferred items to be removed from `deferred-work.md` when their corresponding pre-epic stories are completed,
So that the deferred work file remains current and doesn't accumulate stale resolved items.

**Acceptance Criteria:**

**Given** a pre-epic story reaches `done` status
**When** the pipeline completes the story
**Then** the daemon checks if the story key matches the pre-epic naming convention (`X-0*-pre-epic-*`)
**And** if it matches, the daemon reads the story file to find the "Related deferred items" section (listing which `deferred-work.md` items this story resolved)
**And** the corresponding items are removed from `deferred-work.md`
**And** a `tracing::info!` logs: "Purged {count} resolved items from deferred-work.md"

**Given** `deferred-work.md` contains items under section headings (e.g., `## Deferred from: code review of story-3.3 (2026-03-18)`)
**When** all items under a section heading are removed
**Then** the section heading is also removed to keep the file clean

**Given** a pre-epic story resolves some but not all items from a deferred section
**When** the purge runs
**Then** only the resolved items (matched by description or ID) are removed
**And** remaining items in the section are preserved

**Given** `deferred-work.md` becomes empty after purging
**When** all items have been resolved
**Then** the file is NOT deleted — it is left with only the top-level heading as a placeholder for future deferred items
**And** the daemon commits the cleanup with message `chore(deferred): purge resolved items from pre-epic-{N+1} stories`

### Epic 14 Summary

| Story | Title | Dependencies |
|-------|-------|--------------|
| 14.1 | Winston Reads Deferred Work | — |
| 14.2 | Pre-Epic Story Generation | 14.1 |
| 14.3 | Inject Pre-Epic Stories into Sprint Status | 14.2 |
| 14.4 | Purge Processed Deferred Items | 14.3 |

**Execution Strategy:**
- Linear chain: 14.1 → 14.2 → 14.3 → 14.4
- Story 14.1 is a prompt extension — minimal code, mainly Winston's instructions
- Story 14.2 defines the output format Winston uses for story proposals
- Story 14.3 is the daemon-side parsing and sprint-status injection
- Story 14.4 closes the loop — purges resolved debt

**Dependencies:** Epic 13 (linear pipeline in place to process pre-epic stories)
**Risk:** 🟢 Low — extends existing epic review functionality