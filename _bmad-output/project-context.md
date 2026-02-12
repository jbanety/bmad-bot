---
project_name: 'bmad-bot'
user_name: 'JB'
date: '2026-02-07'
sections_completed: ['technology_stack', 'language_rules', 'framework_rules', 'testing_rules', 'code_quality', 'workflow_rules', 'critical_rules']
status: 'complete'
rule_count: 45
optimized_for_llm: true
---

# Project Context for AI Agents

_This file contains critical rules and patterns that AI agents must follow when implementing code in this project. Focus on unobvious details that agents might otherwise miss._

---

## Technology Stack & Versions

- **Language:** Rust (edition 2024, rustc 1.86.0+)
- **Async Runtime:** tokio (latest stable)
- **AI Agent Framework:** rig-core (latest stable)
- **Serialization:** serde + serde_yaml
- **Git Operations:** Git CLI (>= 2.30) via subprocess (`tokio::process::Command` / `std::process::Command`). Requires `git` installed on host. Inherits user's full git configuration (credential managers, commit signing, SSH agent, `.gitconfig` identity). Validated at daemon startup.
- **HTTP Client:** reqwest (Telegram API, GitHub Copilot adapter)
- **Logging:** tracing (structured logging for daemon)
- **All crates:** latest stable versions, no pinned versions

## Critical Implementation Rules

### Language-Specific Rules (Rust)

- **Edition 2024** — All crates use `edition = "2024"` in Cargo.toml
- **Error Handling:** `thiserror` for custom error types, `anyhow` for propagation in binary. No `unwrap()` or `expect()` in production code — only allowed in tests
- **Async:** Full async tokio runtime. No `block_on()` inside async context, no `std::thread::spawn` unless explicitly justified
- **Logging:** Use `tracing` exclusively — no `println!` or `eprintln!` in production. Use structured fields: `tracing::info!(story_id = %id, "Processing story")`
- **Linting:** `#![deny(clippy::all)]` at crate root. Zero warnings policy
- **No `unsafe`** unless explicitly justified and documented

### Framework-Specific Rules (rig + Daemon Architecture)

#### Daemon Role — Minimal Orchestrator with Pre-Gate
- The daemon is a **launcher**, not an executor. It watches, pre-filters, launches, and notifies.
- It does NOT manage story statuses, create branches, or modify BMAD files — the BMAD agent handles all of that via the `dev-story` workflow
- **Pre-gate responsibility:** The daemon performs a lightweight, deterministic dependency check on `sprint-status.yaml` BEFORE launching any LLM session. This avoids burning tokens on stories that can't proceed. The BMAD agent also verifies dependencies within its own workflow as a second layer.
- Think of it as a headless Claude Code specialized for BMAD workflows running autonomously

#### rig Agent + Tool Calling
- The LLM agent is instantiated via `rig-core` with the BMAD dev agent persona (Amelia)
- **Agent construction is centralized in `llm/agent_factory.rs`** — `AgentFactory::build(role, preamble, tools)` returns a `BuiltAgent` enum with unified `stream_chat()` dispatch. No provider-specific logic in session, review, or supervisor code.
- **9 tools exposed to the agent via rig:**
  - `edit_file` — surgical search_replace edits, create new files, overwrite when justified
  - `read_file` — partial reading (line ranges) + automatic outline mode for large files (>300 lines)
  - `grep` — regex search across project file contents with glob filtering
  - `find_path` — glob-based file path discovery
  - `list_directory` — list directory contents with types and sizes
  - `git` — branch, checkout, commit, push, diff, status, log via Git CLI subprocess
  - `terminal` — shell command execution with timeout (also used for mkdir, rm, build, test)
  - `ask_supervisor` — supervisor question tool (rule engine → LLM fallback → escalation)
  - `ThinkTool` — rig built-in reasoning tool (no custom implementation)
- The agent uses tools autonomously — the daemon does not perform operations on behalf of the agent
- All custom tools must be implemented as rig-compatible tool traits
- **Tool design principle — focused tools over action multiplexing:** Each tool owns a single concern with a compact JSON schema. Do NOT add an `action: String` field that multiplexes multiple operations into one tool. The LLM reasons better with many small, clearly-described tools than with one mega-tool with a large branching schema.

#### Daemon Lifecycle
1. **Watcher** — Polls `sprint-status.yaml` every 5 minutes for `ready-for-dev` stories
2. **Pre-gate** — Deterministic dependency check: skip stories with unmet dependencies, cascade `blocked` status to dependents. No LLM involved — pure graph resolution on sprint data. If no eligible story, wait for next poll cycle.
3. **Session** — Spins up a rig agent with persona + tools + context, sends `DS` to start the `dev-story` workflow
4. **Supervisor** — Intercepts agent questions: rule engine (deterministic patterns) → LLM fallback (project docs context) → escalate `needs-clarification` + notify human. Every decision logged with question, answer, reasoning, and alternatives.
5. **Code Review** — Optional (configurable: enabled/disabled). After agent session completes, launches a separate LLM for adversarial code review. Fixes committed in separate commits for visibility. Review posted as PR comment.
6. **Notification** — Sends result to human (Telegram, extensible)

#### Session Language Override
- BMAD project config may set `communication_language` to a non-English language
- The daemon injects an English override in the system prompt: `"OVERRIDE: communication_language = English"`
- **Never modify BMAD config files on disk** — the daemon is a read-only consumer of the repo
- Notifications to the human remain in the user's configured language

#### Supervisor Hybrid Pattern
- **Rule engine first** (fast, free, deterministic): match known patterns — confirmations ("Should I proceed?" → "Yes"), step-by-step detection → "Yolo", story selection → provide story content
- **LLM fallback** (context-aware): loads full project docs (`_bmad/_memory/`, PRD, architecture, conventions) to answer substantive questions
- **Escalade humaine**: if neither rules nor LLM can answer → mark story `needs-clarification`, notify human, move to next story
- **Decision logging:** Every supervisor decision (rule engine or LLM) is logged to a decisions file at `_bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md`. Each entry includes: question, chosen answer, reasoning, alternatives considered. A summary "🤖 Supervisor Decisions" section is included in the PR description.

#### Sequential Execution
- One story at a time, in sprint order. No parallelism.
- If story B depends on story A, the agent is aware (via BMAD context) and handles branching from the correct parent
- **Two-layer dependency model:** The daemon pre-gate performs cheap deterministic dependency checks (skip/block). The BMAD agent performs full story selection and verification within its workflow. Both layers must agree before work proceeds.

#### Pre-Development Spec Update
- Before starting development, the agent reviews previously completed stories and their actual implementation
- The agent updates the current story's specs and acceptance criteria based on what was actually built in prior stories
- This ensures specs stay current and reflect reality, not just the original plan

#### PR Management
- After code review passes (or directly after dev session if review is disabled), the daemon creates a Pull Request
- **PR for blocked/failed stories:** Even when a story fails or is escalated, the daemon creates a PR with partial code and a description of the failure point — nothing is lost silently
- PR description is agent-written and includes a dedicated "🤖 Supervisor Decisions" section

#### Multi-Provider LLM Config — AgentFactory + BuiltAgent
- Three independent LLM roles: **dev** (Amelia session), **review** (code review), **supervisor** (question answering)
- Supported providers: Anthropic, OpenAI, GitHub Copilot
- **All provider construction is centralized in `AgentFactory`** (`src/llm/agent_factory.rs`). Since rig's `Chat` trait is not object-safe, `BuiltAgent` uses enum dispatch to wrap concrete agent types with a unified `stream_chat()` method.
- **API format is hardcoded per provider/model — not configurable:**
  - **Anthropic** → Messages API (always)
  - **OpenAI direct** → Responses API (always, rig default)
  - **GitHub Copilot** → proxy to multiple backends. Known OpenAI model families (`gpt-*`, `o1-*`, `o3-*`, `codex`) → Responses API. **Everything else → Completions API** (safe fallback for non-OpenAI models like Claude, Mistral, etc.)
- `copilot_requires_responses_api()` is the hardcoded heuristic — new OpenAI model families are a one-liner addition. No `api_format` config — the API format is a deterministic property of the provider behind the model, not a user preference.
- API keys stored in environment variables, never in config files
- `AgentFactory` owns the `CopilotTokenCache` — Copilot token exchange is handled internally, not passed around

### Testing Rules

- **Framework:** Rust native only — `#[cfg(test)]` + `cargo test`. No external test runner
- **Structure:** Tests inline in the same file, inside `#[cfg(test)] mod tests { ... }` at the bottom of each module
- **Unit tests with mocked LLM responses:** All supervisor rule engine logic, response parser, config parsing, git operations — tested with deterministic mocked data. Mock the LLM provider responses, never call real APIs in unit tests
- **E2E tests:** Separate `tests/` directory for integration/E2E tests that run actual LLM sessions. These are expensive (token cost) — **manual launch only**, never in CI or automated runs. Gate behind a feature flag or env var (e.g., `BMAD_E2E=1`)
- **Test naming:** Descriptive snake_case — `test_supervisor_handles_confirmation_pattern`, `test_parser_detects_step_by_step`
- **Every new module must include at least basic unit tests** before being considered complete

### Code Quality & Style Rules

- **Formatting:** `rustfmt` with default configuration. No custom `rustfmt.toml`. Run `cargo fmt` before every commit
- **Modular directory structure:** Organize by domain with subdirectories, not flat files. Example:
  ```
  src/
  ├── main.rs
  ├── cli/
  │   └── mod.rs          # init, start, status, logs commands
  ├── config/
  │   └── mod.rs           # YAML config + .env secrets loading + validation
  ├── llm/
  │   ├── mod.rs           # LLM module root (re-exports context, logging, agent_factory)
  │   ├── agent_factory.rs # AgentFactory + BuiltAgent enum dispatch — centralized provider construction, Copilot API format detection
  │   ├── context.rs       # Zed-style XML ContextBuilder
  │   └── logging.rs       # LLM request/response debug logging
  ├── watcher/
  │   ├── mod.rs
  │   └── deps.rs          # Dependency graph resolution + cascade blocking
  ├── session/
  │   ├── mod.rs
  │   └── parser.rs
  ├── supervisor/
  │   ├── mod.rs
  │   ├── rules.rs
  │   └── decisions.rs     # Decision logging to file + PR section
  ├── review/
  │   └── mod.rs
  ├── tools/
  │   ├── mod.rs
  │   ├── edit_file.rs     # Surgical search_replace edits, create, overwrite
  │   ├── read_file.rs     # Partial reading (line ranges) + outline mode
  │   ├── grep.rs          # Regex search across project file contents
  │   ├── find_path.rs     # Glob-based file path discovery
  │   ├── list_directory.rs # List directory contents
  │   ├── git.rs
  │   └── terminal.rs
  └── notifier/
      └── mod.rs
  ```
- **Documentation:** `///` doc comments mandatory on all public structs, traits, enums, and functions. This serves double duty: Rust docs + LLM context when reading the codebase
- **No dead code:** `#![deny(dead_code)]` — remove unused code, don't comment it out

### CLI Rules

- **CLI Commands:** `bmad-bot init` (interactive setup → generates `bmad-bot.yaml` + `.env`), `bmad-bot start` (launches daemon), `bmad-bot status` (current state summary), `bmad-bot logs` (structured tracing logs)
- **Config validation:** Both `init` and `start` validate configuration before proceeding — missing keys, unreachable repos, invalid YAML all reported clearly
- **Git validation:** `bmad-bot start` verifies `git --version` >= 2.30 before entering the polling loop. Fails fast with a clear error if git is missing or too old
- **BMAD auto-discovery:** On startup, detect BMAD version and installed modules from the project repo
- **Graceful shutdown:** SIGTERM/SIGINT handled — finish current step if possible, commit partial work, create PR with progress description, notify human, then exit cleanly. No corrupted branches, no half-committed files.

### Resilience Rules

- **Retry with backoff:** Transient LLM errors (timeouts, 429, 500s, 503s) retried with exponential backoff, max 3 retries per call
- **Notification failures are non-blocking:** Telegram API failures are logged but do not stop story processing
- **Crash recovery:** On restart, daemon re-reads `sprint-status.yaml` and resumes from clean state. No corrupted branches or half-committed files.

### Development Workflow Rules

- **Tool usage in system preamble:** The daemon's `build_preamble()` includes explicit tool usage rules that instruct the agent to always use `edit_file` with search_replace for existing files, use `read_file` with line ranges for large files, use `grep` before editing to find code, and never rewrite entire files
- **Branch naming:** Follow sprint-status.yaml key convention → `story/{epic}-{story}` (e.g., `story/1-2-account-management`). The BMAD dev agent creates and manages branches via exposed git tools
- **Commit messages:** Conventional Commits enforced — `feat:`, `fix:`, `refactor:`, `test:`, `chore:`, `docs:`. Scope optional but encouraged (e.g., `feat(supervisor): add rule engine pattern matching`)
- **PR creation:** After code review passes (or directly after dev session if review disabled), the daemon opens a Pull Request automatically. GitHub supported in MVP. Provider configured in `bmad-bot.yaml`
- **No auto-merge:** PRs are created for human review. Never merge automatically into `main`
- **No CI for now:** No GitHub Actions or CI pipeline. May be added later

### Critical Don't-Miss Rules

- **Never rewrite entire files:** The agent MUST use `edit_file` with search_replace mode for existing files. Full file rewrites waste tokens, risk truncation, and can silently lose code. The only exceptions are `create` mode (new files) and `overwrite` mode (rare, justified complete rewrites)
- **BMAD files are sacred:** Never modify anything under `_bmad/` — the daemon is a read-only consumer of BMAD config, agents, and workflows
- **Never merge on `main`:** Even if code review passes, only a human merges. No exceptions
- **Never call real LLM APIs in unit tests:** Mock responses only. E2E tests with real APIs are manual-launch-only
- **Secrets in env vars only:** API keys, bot tokens — always referenced via `_env` suffix in config (e.g., `api_key_env: ANTHROPIC_API_KEY`). Never hardcoded, never committed
- **Supervisor must never invent answers:** If the rule engine doesn't match AND the LLM supervisor can't answer confidently from project docs → mark story `needs-clarification`, notify human, move to next story. Hallucinated answers lead to hallucinated code
- **No silent failures:** Every error must be logged via `tracing::error!()`. Blocking errors (session crash, git failure, LLM provider down) must also trigger a notification to the human
- **One session at a time:** Never run multiple story sessions in parallel. Sequential execution only — avoids git conflicts, codebase race conditions, and context confusion
- **One tool = one concern:** Each rig tool has a single responsibility with a focused JSON schema. Never add an `action` multiplexer field to a tool — split into separate tools instead

---

## Usage Guidelines

**For AI Agents:**

- Read this file before implementing any code
- Follow ALL rules exactly as documented
- When in doubt, prefer the more restrictive option
- Update this file if new patterns emerge during implementation

**For Humans:**

- Keep this file lean and focused on agent needs
- Update when technology stack or architecture changes
- Review quarterly for outdated rules
- Remove rules that become obvious over time

Last Updated: 2026-02-12 — LLM provider construction centralized in `llm/agent_factory.rs` (`AgentFactory` + `BuiltAgent` enum dispatch). Fixes Copilot gpt-5.2-codex Responses API bug. API format hardcoded per provider/model: Anthropic → Messages API, OpenAI → Responses API, GitHub Copilot → explicit match on OpenAI model families for Responses API, fallback to Completions API for all other models. ~610 lines of duplicated provider match arms eliminated. See architect-brief-llm-provider-abstraction.md for full rationale.