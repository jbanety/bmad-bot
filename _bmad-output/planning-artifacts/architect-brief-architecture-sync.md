# Architect Brief — Architecture Document Sync

**Author:** John (PM)
**Date:** 2026-02-07
**Purpose:** The PRD and Epics have just been updated to reflect implementation reality (commits since `9d2b482`). The `architecture.md` document is now out of sync. This brief details every delta the Architect needs to address.

---

## Context

13 commits introduced significant architectural changes that were never reflected in `architecture.md`. The PRD (`prd.md`) and Epics (`epics.md`) have already been updated. The architecture document needs to catch up.

**Reference commits:** `69d1ff0..e24d39b` on `main`

---

## Delta 1: Decision 5 — Agent Prompt Composition (CRITICAL)

**Current (architecture.md L284-312):**
> "The BMAD dev agent file is loaded as-is and used as the rig agent preamble. The only addition is a language override appended at the end."

With a code example showing `preamble(&agent_prompt)` and `.tool()` registrations for 4 tools.

**Reality:**
- The agent file is **NOT** used as the system preamble
- The system preamble is now **minimal**: operational instructions only (tool usage rules, communication rules, language override to English)
- The BMAD agent file is sent as the **first user message** wrapped in **Zed-style XML context tags** (`<context><files>`) via a new `activate_agent()` method
- The agent then executes its activation steps via tools (loads `config.yaml`, shows greeting/menu) before receiving commands
- A new `llm_context` module (`src/llm_context.rs`) provides `ContextBuilder` — a helper for building Zed-style XML context with adaptive backtick fencing, absolute path resolution, multi-file support, and line ranges
- First command message remains `"DS"`

**Action required:**
- Rewrite Decision 5 title, rationale, implementation code example, and description
- Add `llm_context` module to project structure and module descriptions
- Update the "Key principle" — the daemon now has an explicit activation phase before sending commands

---

## Delta 2: Signal Handling — Cooperative Shutdown

**Current (architecture.md L89):**
> "Signal handling: tokio::signal for SIGTERM/SIGINT graceful shutdown"

**Reality:**
- Cooperative shutdown via a shared `ShutdownFlag` (`Arc<AtomicBool>`) created in `run_start()`
- Dedicated signal handler task listens for Ctrl+C/SIGTERM and flips the flag
- Flag is propagated across **pipeline → session → streaming_chat** layers
- `streaming_chat()` checks the flag between every stream chunk/tool-call round — can interrupt **mid-streaming and mid-tool-call loops**
- `run_session()` checks between chat turns, saves WAL before returning
- `run_polling_loop()` checks at the top of each cycle
- This replaces the old inline `tokio::select!` signal branches

**Action required:**
- Update Foundation Layer → Signal Handling
- Update Decision Impact Analysis step 1 (foundation)
- Consider documenting the ShutdownFlag propagation pattern in Implementation Patterns

---

## Delta 3: Streaming API — Universal

**Current (architecture.md):**
Multiple references to `agent.chat(message, history)` and non-streaming calls.

**Reality:**
- **ALL** LLM calls now use streaming via `stream_chat()` / `streaming_chat()` helper
- Applies to: session runner, review module, supervisor architect
- GitHub Copilot API **requires** `stream: true` — non-streaming requests are rejected with 400
- Each module has a `streaming_chat` helper that consumes the SSE stream and collects the final text
- Tool calls are handled automatically by rig within the stream
- Build methods return concrete `Agent<M>` types (not `impl Chat`) to satisfy `StreamingChat` trait bounds

**Action required:**
- Update Data Flow step 5 (chat loop) to mention streaming
- Update any code examples or references to `agent.chat()`
- Note streaming as a universal pattern, not just a Copilot requirement

---

## Delta 4: ThinkTool — 5th Agent Tool

**Current (architecture.md):**
- Data Flow step 4: "builds rig agent with 4 tools (git, fs, terminal, ask_supervisor)"
- Tools module only lists git, fs, terminal
- Module Communication Map shows `tools/ git/fs/term`

**Reality:**
- Agents have **5 tools**: git, filesystem, terminal, ask_supervisor, **think**
- `ThinkTool` is rig's built-in tool (derived from Anthropic's Claude Think Tool pattern)
- Gives the agent a dedicated space for structured reasoning without consuming real tool calls
- Added to all 3 agent builders (anthropic, openai, github-copilot)
- No custom implementation needed — provided by the `rig` crate

**Action required:**
- Update Data Flow step 4: 4 tools → 5 tools
- Update tools description (note think is built-in, not in `tools/` directory)
- Update Module Communication Map label
- Update any tool count references

---

## Delta 5: Project Directory Structure (MAJOR)

**Current structure in architecture.md is significantly incomplete.**

**Missing modules and files:**

```
src/
├── auth/                              # NEW — GitHub Copilot OAuth
│   ├── mod.rs
│   └── github_copilot.rs             # Device flow + token exchange
├── cli/
│   ├── mod.rs
│   ├── git_detect.rs                  # NEW — Git remote auto-detection
│   └── state.rs                       # NEW — CLI state management
├── config/
│   ├── mod.rs
│   └── discovery.rs                   # NEW — BMAD version/module discovery
├── session/
│   ├── mod.rs
│   ├── analyzer.rs                    # NEW (was parser.rs in early arch)
│   ├── branch.rs                      # NEW — Branch management
│   ├── cleanup.rs                     # NEW — Session cleanup
│   ├── escalation.rs                  # NEW — Escalation handling
│   ├── provider.rs                    # NEW — LLM provider construction + Copilot headers
│   ├── runner.rs                      # NEW — Main session runner (chat loop, activation, recovery)
│   └── state.rs
├── supervisor/
│   ├── mod.rs
│   ├── architect.rs                   # NEW — Architect LLM fallback session
│   ├── read_tool.rs                   # NEW — Read-only file tool for Architect
│   ├── rules.rs
│   └── decisions.rs
├── llm_context.rs                     # NEW — Zed-style XML ContextBuilder
├── llm_logging.rs                     # NEW — LLM request/response logging
├── pipeline.rs                        # NEW — Pipeline orchestration
├── tools/
│   ├── mod.rs
│   ├── git.rs
│   ├── fs.rs
│   └── terminal.rs
```

**Action required:**
- Update both directory structure listings (Foundation Layer + Project Structure & Boundaries)
- Add descriptions for each new file
- Note: `session/parser.rs` from original architecture is now `session/analyzer.rs`

---

## Delta 6: Requirements to Structure Mapping

**Current mapping is incomplete. Missing:**

| FRs | Domain | Module | Key Files |
|-----|--------|--------|-----------|
| FR8-11 | Development Session | `session/` | Should list `runner.rs` (chat loop, activation), `analyzer.rs` (response analysis), `provider.rs` (LLM construction), `branch.rs`, `cleanup.rs`, `escalation.rs` — not just `mod.rs` |
| FR34 | Cooperative Shutdown | `cli/`, `session/`, `pipeline.rs` | ShutdownFlag shared across layers |
| FR39 | Copilot Auth | `auth/` | `github_copilot.rs` (device flow + token exchange) |
| FR40 | LLM Logging | `llm_logging.rs` | Request/response logging |
| — | Pipeline | `pipeline.rs` | Orchestrates watcher → session → review → PR → notify |
| — | XML Context | `llm_context.rs` | ContextBuilder for agent activation |

**Action required:**
- Add rows for FR39 (`auth/`), FR40 (`llm_logging.rs`)
- Update FR8-11 row with actual session submodules
- Add FR34 updated description
- Consider adding `pipeline.rs` and `llm_context.rs` to the mapping

---

## Delta 7: Copilot Provider — Completions API Split

**Current (architecture.md External Integration Points):**
> "LLM Providers (Anthropic, OpenAI) via rig-core — API key from .env"

(Copilot listed separately with just auth details)

**Reality:**
- OpenAI uses the **Responses API** (`build_openai_agent`)
- GitHub Copilot uses the **Completions API** (`build_copilot_agent`) — separate builder function
- Copilot requires IDE-specific headers: `Editor-Version`, `Copilot-Integration-Id` ("vscode-chat"), `User-Agent`
- These headers are injected via rig's `.http_headers()` builder method
- `session/provider.rs` contains the `copilot_headers()` helper and all provider construction logic
- The Copilot token exchange in `auth/github_copilot.rs` also requires a `User-Agent` header

**Action required:**
- Update External Integration Points table — note API split and required headers
- Mention `session/provider.rs` as the provider construction module
- Update LLM Provider row to distinguish OpenAI (Responses API) vs Copilot (Completions API)

---

## Delta 8: Tracing & LLM Logging

**Current:**
Tracing Pattern section describes structured spans and basic tracing rules. No mention of LLM-specific logging.

**Reality:**
- New `llm_logging` module (`src/llm_logging.rs`, 299 lines) logs all LLM requests and responses
- Used across session runner, review, and supervisor architect modules
- Critical for debugging agent behavior and operations visibility

**Action required:**
- Add mention in Tracing Pattern section
- Add `llm_logging.rs` to project structure with description

---

## Delta 9: Watcher — Immediate Poll on Startup

**Current (implied):**
Watcher waits for the first polling interval before first poll.

**Reality:**
- Uses `tokio::time::interval` which ticks immediately on first call
- Daemon polls `sprint-status.yaml` at launch, not after `polling_interval_secs`

**Action required:**
- Minor: note in Data Flow step 3 or watcher module description

---

## Delta 10: Module Communication Map

**Current ASCII diagram shows:**
```
cli/ → watcher/ → session/ → tools/ (git/fs/term)
                            → supervisor/
                            → review/ → git_provider/ + notifier/
```

**Missing from diagram:**
- `pipeline.rs` — sits between `cli/` and the watcher→session flow, orchestrates the full pipeline
- `auth/` — used by `session/provider.rs` for Copilot token exchange
- `llm_context.rs` — used by `session/runner.rs` for agent activation
- `llm_logging.rs` — used by session, review, supervisor
- ShutdownFlag flow from `cli/` through `pipeline.rs` → `session/` → streaming_chat

**Action required:**
- Update the ASCII diagram to include pipeline, auth, llm_context, llm_logging
- Consider showing the ShutdownFlag propagation path

---

## Summary — Priority Order

| # | Delta | Severity | Effort |
|---|-------|----------|--------|
| 1 | Decision 5 rewrite (XML context activation) | 🔴 Critical | Medium |
| 5 | Project directory structure | 🔴 Critical | Medium |
| 6 | Requirements to structure mapping | 🟠 High | Small |
| 2 | Cooperative shutdown | 🟠 High | Small |
| 3 | Streaming API universal | 🟠 High | Small |
| 4 | ThinkTool 5th tool | 🟡 Medium | Small |
| 7 | Copilot provider split | 🟡 Medium | Small |
| 10 | Module communication map | 🟡 Medium | Medium |
| 8 | LLM logging | 🟢 Low | Small |
| 9 | Watcher immediate poll | 🟢 Low | Trivial |

**Total: 10 deltas, 2 critical, 2 high, 3 medium, 3 low.**