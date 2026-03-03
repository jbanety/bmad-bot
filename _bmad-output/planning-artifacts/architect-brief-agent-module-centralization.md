# Architect Brief — Agent Module Centralization

## Status: Draft
## Date: 2026-03-03
## Author: Amelia (Dev Agent), reviewed by JB

---

## Problem Statement

Agent construction logic (tool creation, preamble building, agent factory calls) is duplicated across three modules with minor variations:

| Module | Tools | AskSupervisor | Preamble | activate_agent |
|---|---|---|---|---|
| `session/runner.rs` (`create_tools`, `build_agent_for_role`) | 8 custom + ThinkTool | ✅ Yes | via `dev_agent::build_preamble` | via `dev_agent::activate_agent` |
| `review/mod.rs` (`create_tools`, `run_inner`) | 8 custom + ThinkTool | ✅ Yes | via `dev_agent::build_preamble` | via `dev_agent::activate_agent` |
| `supervisor/architect.rs` (`ask`) | 8 custom + ThinkTool | ❌ No (recursion) | via `dev_agent::build_preamble` | via `dev_agent::activate_agent` |

The `create_tools()` function is copy-pasted 3 times (7 identical tool constructors + supervisor wiring). The supervisor/architect had a broken tool set until the preceding bugfix (only had `read_file`, now has all 8 tools minus `ask_supervisor`).

The shared module is called `dev_agent.rs` despite being used by all three agent roles — misleading name.

## Proposed Solution

### 1. Rename `session/dev_agent.rs` → `session/agent.rs`

Pure rename + update imports. The module already contains role-agnostic code:
- `build_preamble()` — used by dev, review, and supervisor
- `activate_agent()` — generic over agent file path (`dev.md`, `architect.md`)
- `streaming_chat()` — generic streaming wrapper
- `ChatHistoryHook` — history capture hook
- `ShutdownFlag` type alias

### 2. Centralize `create_tools()` in `session/agent.rs`

Extract the duplicated tool construction into a single function:

```rust
/// Standard tool set: git, read_file, edit_file, grep, find_path, list_directory, terminal.
/// Optionally includes AskSupervisor (excluded for supervisor role to prevent recursion).
pub fn create_standard_tools(
    project_root: &Path,
    supervisor: Option<AskSupervisor>,
) -> StandardToolSet { ... }
```

The 7 base tools (`GitTool`, `ReadFileTool`, `EditFileTool`, `GrepTool`, `FindPathTool`, `ListDirectoryTool`, `TerminalTool`) are always created identically. The only variable is `AskSupervisor`:
- **Dev session / Review**: `Some(AskSupervisor::with_architect_from_config(...))`
- **Supervisor**: `None`

When `supervisor` is `Some`, the function returns a 9-tuple (8 + ThinkTool). When `None`, it returns an 8-tuple (7 + ThinkTool). Both arities are already covered by `impl_agent_configurator!`.

**Alternative (simpler):** Always return a 9-tuple. When `supervisor` is `None`, substitute a `NoopSupervisor` tool that always returns `"N/A — supervisor not available in this context"`. This avoids two return types and keeps `configure_agent_tools!` calls uniform everywhere. The LLM preamble already lists `ask_supervisor` for all roles, so the tool would exist but gracefully decline.

### 3. Centralize `build_agent()` helper

Move the `build_agent_for_role` pattern (preamble + tools + factory.build + MCP) into `session/agent.rs`:

```rust
pub async fn build_agent(
    factory: &AgentFactory,
    config: &BotConfig,
    role: LlmRole,
    mcp_manager: &McpManager,
    supervisor: Option<AskSupervisor>,
) -> Result<BuiltAgent, ProviderError> { ... }
```

Callers simplify from ~30 lines to a single call.

### 4. Remove per-module `create_tools()`

- `session/runner.rs`: remove `create_tools()`, `build_agent_for_role()`, `build_preamble()` — call `agent::build_agent()` instead
- `review/mod.rs`: remove `create_tools()`, inline toolset type alias — call `agent::build_agent()` instead
- `supervisor/architect.rs`: remove tool construction in `ask()` — call `agent::build_agent()` with `supervisor: None`

## Files Changed

| File | Change |
|---|---|
| `src/session/dev_agent.rs` | Renamed to `src/session/agent.rs`, add `create_standard_tools()` and `build_agent()` |
| `src/session/mod.rs` | Update `pub mod dev_agent` → `pub mod agent` |
| `src/session/runner.rs` | Remove `create_tools()`, `build_agent_for_role()`, `build_preamble()`. Import from `agent::`. Remove `ToolSet` type alias, `TERMINAL_TIMEOUT_SECS` (moved to agent.rs) |
| `src/review/mod.rs` | Remove `create_tools()`, `ReviewToolSet` type alias, `TERMINAL_TIMEOUT_SECS`. Import from `session::agent::` |
| `src/supervisor/architect.rs` | Remove tool construction in `ask()`, `TERMINAL_TIMEOUT_SECS`. Import from `session::agent::` |
| All files importing `dev_agent::` | Update import paths to `agent::` |

## Scope Guard

- **No functional changes.** This is a pure refactoring — same tools, same preamble, same activation flow.
- **No new tools.** Tool list stays identical.
- **No architecture changes.** `AgentFactory`, `BuiltAgent`, `ToolConfigurator`, `impl_agent_configurator!` macro are untouched.
- **Preamble content unchanged.** `build_preamble()` moves files but its output is byte-identical.

## Risks

| Risk | Mitigation |
|---|---|
| Import breakage across many files | `cargo check` after each step; commit rename and logic changes separately |
| `configure_agent_tools!` arity mismatch | Already covered by `impl_agent_configurator!` macro (arities 1–12) |
| Supervisor recursion if tool set changes | `AskSupervisor` inclusion is explicit via `Option` parameter — can't accidentally add it |
| Merge conflicts with in-flight stories | Pure refactoring commit — easy to rebase |

## Estimated Effort

Small-medium. ~200 lines removed (duplication), ~50 lines added (centralized helpers). 5 files touched. No new tests needed — existing tests cover all paths. Rename step can be a separate commit for clean git history.

## Decision Needed

**NoopSupervisor vs two return types:** Should `create_standard_tools` always return a uniform 9-tuple with a `NoopSupervisor` placeholder, or return different tuple arities? The NoopSupervisor approach is simpler (one code path everywhere) but means the LLM could call a tool that always returns "N/A". The different-arity approach is more correct but adds a branching point.

Recommendation: **NoopSupervisor**. The LLM already sees `ask_supervisor` in the preamble for all roles. A graceful "N/A" response is better than a `ToolNotFoundError`.