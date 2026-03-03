# Story 4.7: Agent Module Centralization

Status: ready-for-dev

## Story

As a daemon maintainer,
I want agent construction logic (tool creation, preamble building) centralized in a single shared module with a role-agnostic name,
so that adding or changing tools requires editing one place instead of three, and the misleading `dev_agent` name no longer confuses contributors.

## Acceptance Criteria

1. **Given** `src/session/dev_agent.rs` exists
   **When** the rename is applied
   **Then** the file is renamed to `src/session/agent.rs`
   **And** `src/session/mod.rs` declares `pub mod agent` instead of `pub mod dev_agent`
   **And** every file that imports from `dev_agent` is updated to import from `agent`

2. **Given** `create_tools()` is duplicated in `session/runner.rs` (L808-847) and `review/mod.rs` (L500-535)
   **When** the centralization is applied
   **Then** two public functions exist in `session/agent.rs`:
   - `create_base_tools(project_root)` — returns a 7-tuple `(GitTool, ReadFileTool, EditFileTool, GrepTool, FindPathTool, ListDirectoryTool, TerminalTool)` (infallible)
   - `create_tools_with_supervisor(project_root, config, agent_factory, escalation_slot, decision_log, mcp_manager)` — returns `Result<(GitTool, ReadFileTool, EditFileTool, GrepTool, FindPathTool, ListDirectoryTool, TerminalTool, AskSupervisor), Box<dyn Error>>`
   **And** `ThinkTool` is NOT included in the returned tuples — callers add it via `configure_agent_tools!`
   **And** `create_tools()` is removed from both `runner.rs` and `review/mod.rs`
   **And** direct tool type imports (`GitTool`, `ReadFileTool`, etc.) are removed from `runner.rs` and `review/mod.rs` since they no longer construct tools locally

3. **Given** tool construction in `supervisor/architect.rs::ask()` (L356-366) is inline
   **When** the centralization is applied
   **Then** `architect.rs::ask()` calls `create_base_tools(&self.project_root)` to get the 7 base tools
   **And** the 7 inline tool constructors are removed from `ask()`
   **And** direct tool type imports (`GitTool`, `ReadFileTool`, etc.) are removed from `architect.rs`

4. **Given** `TERMINAL_TIMEOUT_SECS` is defined identically in three files (`runner.rs` L62, `review/mod.rs` L63, `architect.rs` L34)
   **When** the centralization is applied
   **Then** a single `pub const TERMINAL_TIMEOUT_SECS: u64 = 30` exists in `session/agent.rs`
   **And** the three per-module constants are removed

5. **Given** `type ToolSet` (runner.rs L39-48) and `type ReviewToolSet` (review/mod.rs L40-49) are identical 8-tuple aliases
   **When** the centralization is applied
   **Then** both type aliases are removed
   **And** callers use the concrete tuple type returned by `create_tools_with_supervisor()` or `create_base_tools()` directly (or a single shared alias in `agent.rs` if needed)

6. **Given** `build_preamble()` is called via `dev_agent::build_preamble()` in three locations
   **When** the rename is applied
   **Then** all call sites use `agent::build_preamble()` instead
   **And** the function signature and output are byte-identical — no behavioral change

7. **Given** all changes are complete
   **When** validation runs
   **Then** `cargo build`, `cargo test`, `cargo clippy`, and `cargo fmt --check` all pass with zero errors and zero warnings

## Tasks / Subtasks

- [ ] Task 1: Rename `dev_agent.rs` → `agent.rs` (AC: #1)
  - [ ] 1.1 `git mv src/session/dev_agent.rs src/session/agent.rs`
  - [ ] 1.2 Update `src/session/mod.rs`: `pub mod dev_agent` → `pub mod agent`
  - [ ] 1.3 Update all imports across the codebase (see exhaustive list in Dev Notes)
  - [ ] 1.4 `cargo build` — verify compiles
  - [ ] 1.5 `cargo test` — verify all tests pass
  - [ ] 1.6 Commit: `refactor(session): rename dev_agent module to agent`

- [ ] Task 2: Centralize `TERMINAL_TIMEOUT_SECS` (AC: #4)
  - [ ] 2.1 Add `pub const TERMINAL_TIMEOUT_SECS: u64 = 30` to `session/agent.rs`
  - [ ] 2.2 Remove the constant from `session/runner.rs`, `review/mod.rs`, `supervisor/architect.rs`
  - [ ] 2.3 Update references to use `crate::session::agent::TERMINAL_TIMEOUT_SECS`
  - [ ] 2.4 `cargo build` + `cargo test`
  - [ ] 2.5 Commit: `refactor: centralize TERMINAL_TIMEOUT_SECS in session::agent`

- [ ] Task 3: Centralize tool creation (AC: #2, #3, #5)
  - [ ] 3.1 Add `create_base_tools()` and `create_tools_with_supervisor()` to `session/agent.rs` (see Dev Notes for signatures)
  - [ ] 3.2 Refactor `SessionRunner::create_tools()` in `runner.rs` to call `agent::create_tools_with_supervisor()`
  - [ ] 3.3 Refactor `ReviewRunner::create_tools()` in `review/mod.rs` to call `agent::create_tools_with_supervisor()`
  - [ ] 3.4 Refactor `ArchitectSession::ask()` in `supervisor/architect.rs` to call `agent::create_base_tools()`
  - [ ] 3.5 Remove `type ToolSet` from `runner.rs` and `type ReviewToolSet` from `review/mod.rs`
  - [ ] 3.6 Remove the per-module `create_tools()` methods from `runner.rs` and `review/mod.rs`
  - [ ] 3.7 Remove now-unused direct tool type imports (`GitTool`, `ReadFileTool`, `EditFileTool`, `GrepTool`, `FindPathTool`, `ListDirectoryTool`, `TerminalTool`) from `runner.rs`, `review/mod.rs`, and `supervisor/architect.rs`
  - [ ] 3.8 Add unit tests for `create_base_tools()` in `session/agent.rs` (see Dev Notes)
  - [ ] 3.9 `cargo build` + `cargo test` + `cargo clippy`
  - [ ] 3.10 Commit: `refactor: centralize tool creation in session::agent`

- [ ] Task 4: Final validation (AC: #6, #7)
  - [ ] 4.1 `cargo fmt --check` — clean
  - [ ] 4.2 `cargo clippy` — zero new warnings
  - [ ] 4.3 `cargo test` — all tests pass (947+ expected)
  - [ ] 4.4 Verify no remaining references to `dev_agent` in src/ (grep check)
  - [ ] 4.5 Update story status to complete

## Dev Notes

### Pure Refactoring — Zero Functional Changes

This story is a **pure mechanical refactoring**. No new tools, no new modules, no new error types, no new features. The tool set, preamble content, activation flow, and agent behavior are all byte-identical before and after. The only observable change is the import paths.

### Module Doc Comment Update

After renaming `dev_agent.rs` → `agent.rs`, update the module-level doc comment at the top of the file. Current text references "Shared BMAD dev agent activation" and mentions only `SessionRunner` and `ReviewRunner`. Replace with a role-agnostic description reflecting all three consumers:

```rust
//! Shared agent activation — preamble, tool construction, activation, and streaming chat.
//!
//! This module contains the common logic used by [`SessionRunner`](super::runner::SessionRunner),
//! [`ReviewRunner`](crate::review::ReviewRunner), and
//! [`ArchitectSession`](crate::supervisor::architect::ArchitectSession) to set up
//! and run BMAD agents. The activation flow is identical for all roles:
//!
//! 1. Build a generic preamble with tool usage rules and English override
//! 2. Create the standard tool set via [`create_base_tools()`] or [`create_tools_with_supervisor()`]
//! 3. Send the agent file as a user message (Zed-style XML context) to trigger BMAD activation
//! 4. The agent processes activation steps: loads `config.yaml`, greets user, shows menu
//! 5. Caller sends a menu command (`DS` for dev, `CR` for review, `CH` for supervisor)
```

### Import Naming — Avoid `agent` Keyword Ambiguity

In `review/mod.rs`, the current import is `use crate::session::dev_agent::{self, ShutdownFlag}` which allows calling `dev_agent::build_preamble(...)`. After the rename, `agent` is a common word that could shadow other names. Use an explicit alias if needed:

```rust
// Option A — direct import (preferred, cleaner)
use crate::session::agent::{self, ShutdownFlag};
// usage: agent::build_preamble(...)

// Option B — if 'agent' conflicts with a local name
use crate::session::agent::{self as session_agent, ShutdownFlag};
// usage: session_agent::build_preamble(...)
```

Check for conflicts at compile time. Option A should work in all three files since no local `agent` binding exists in any of them.

### Commit Strategy — Rename First, Then Logic

Do Task 1 (rename) as a **separate commit** before Tasks 2-3 (logic changes). This gives a clean `git mv` rename that preserves file history, and makes the logic changes easy to rebase if there are merge conflicts.

### Exhaustive Import Update List for Rename (Task 1)

Every `dev_agent` reference in the codebase:

| File | Current Reference | Change To |
|---|---|---|
| `src/session/mod.rs` | `pub mod dev_agent;` | `pub mod agent;` |
| `src/session/mod.rs` | doc comment mentioning `dev_agent` | Update to `agent` |
| `src/session/runner.rs` | `pub use crate::session::dev_agent::ShutdownFlag;` | `pub use crate::session::agent::ShutdownFlag;` |
| `src/session/runner.rs` | `use crate::session::dev_agent::{self};` | `use crate::session::agent::{self};` (rename usage from `dev_agent::` to `agent::`) |
| `src/session/runner.rs` | `dev_agent::build_preamble(...)` (L803) | `agent::build_preamble(...)` |
| `src/session/runner.rs` | doc comment `dev_agent::activate_agent()` (L797) | `agent::activate_agent()` |
| `src/session/analyzer.rs` | doc comment `dev_agent::build_preamble` (L44) | `agent::build_preamble` |
| `src/review/mod.rs` | `use crate::session::dev_agent::{self, ShutdownFlag};` | `use crate::session::agent::{self as agent, ShutdownFlag};` (or just `agent`) |
| `src/review/mod.rs` | `dev_agent::build_preamble(...)` (L448) | `agent::build_preamble(...)` |
| `src/supervisor/architect.rs` | `use crate::session::dev_agent::build_preamble;` | `use crate::session::agent::build_preamble;` |
| `src/llm/agent_factory.rs` | `use crate::session::dev_agent::streaming_chat;` | `use crate::session::agent::streaming_chat;` |
| `src/llm/agent_factory.rs` | `pub use crate::session::dev_agent::ShutdownFlag;` | `pub use crate::session::agent::ShutdownFlag;` |
| `src/llm/agent_factory.rs` | `crate::session::dev_agent::activate_agent(...)` (L138, L148, L158) — three match arms | `crate::session::agent::activate_agent(...)` |
| `src/llm/agent_factory.rs` | doc comment referencing `dev_agent::activate_agent()` (L120) | `agent::activate_agent()` |

**Verify completeness:** After the rename, run `grep -rn "dev_agent" src/` — must return zero matches.

### Tool Centralization Design (Task 3)

The architect brief proposed two approaches: different return types vs NoopSupervisor. After analysis, the **two separate functions** approach is cleaner because:

1. The `configure_agent_tools!` macro already handles any arity (1-12) thanks to `impl_agent_configurator!` from commit `3d725cf`
2. The supervisor/architect call site already uses an 8-tuple (7 tools + ThinkTool, no supervisor) — this naturally maps to `create_base_tools()`
3. No phantom "N/A" tool polluting the supervisor agent's tool list
4. `ThinkTool` is NOT included in the return tuples — callers add it via `configure_agent_tools!(git, read_file, ..., ThinkTool)` as they already do today

**Implementation pattern — two separate public functions:**

```rust
/// Standard tool set WITH ask_supervisor — used by dev session and code review.
///
/// Returns 8 custom tools (7 base + supervisor). Caller adds ThinkTool via configure_agent_tools!.
pub fn create_tools_with_supervisor(
    project_root: &Path,
    config: &BotConfig,
    agent_factory: &Arc<AgentFactory>,
    escalation_slot: EscalationSlot,
    decision_log: DecisionLog,
    mcp_manager: &Arc<McpManager>,
) -> Result<(GitTool, ReadFileTool, EditFileTool, GrepTool, FindPathTool, ListDirectoryTool, TerminalTool, AskSupervisor), Box<dyn std::error::Error>> {
    let (git, read_file, edit_file, grep, find_path, list_dir, terminal) = create_base_tools(project_root);
    let supervisor = AskSupervisor::with_architect_from_config(
        config,
        Some(Arc::clone(agent_factory)),
        escalation_slot,
        decision_log,
        Arc::clone(mcp_manager),
    )?;
    Ok((git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor))
}

/// Standard tool set WITHOUT ask_supervisor — used by the supervisor/architect agent.
///
/// Returns 7 base tools. Caller adds ThinkTool via configure_agent_tools!.
pub fn create_base_tools(
    project_root: &Path,
) -> (GitTool, ReadFileTool, EditFileTool, GrepTool, FindPathTool, ListDirectoryTool, TerminalTool) {
    let git = GitTool::new(project_root.to_path_buf());
    let read_file = ReadFileTool::new(project_root.to_path_buf());
    let edit_file = EditFileTool::new(project_root.to_path_buf());
    let grep = GrepTool::new(project_root.to_path_buf());
    let find_path = FindPathTool::new(project_root.to_path_buf());
    let list_dir = ListDirectoryTool::new(project_root.to_path_buf());
    let terminal = TerminalTool::new(project_root.to_path_buf(), TERMINAL_TIMEOUT_SECS);
    (git, read_file, edit_file, grep, find_path, list_dir, terminal)
}
```

**Why two functions instead of `Option<AskSupervisor>` with different return types:**
- Rust cannot return different tuple types from a single function without enum wrapping
- Two functions are simpler, type-safe, and self-documenting
- `create_base_tools` is infallible (no `Result`) — pure construction
- `create_tools_with_supervisor` returns `Box<dyn Error>` from `AskSupervisor::with_architect_from_config()` — callers `.map_err()` to their own error type

**CRITICAL: `ThinkTool` stays in the caller.** The current code already adds `ThinkTool` via the `configure_agent_tools!` macro at the call site, NOT in `create_tools()`. This does not change. Example:
```rust
configure_agent_tools!(git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor, ThinkTool)
```

### Caller Refactoring Patterns

**`session/runner.rs` — `build_agent_for_role()` (L764-791):**

Current:
```rust
let (git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor) =
    self.create_tools(&project_root, escalation_slot, decision_log)?;
```

After:
```rust
use crate::session::agent::create_tools_with_supervisor;

let (git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor) =
    create_tools_with_supervisor(
        &project_root,
        &self.config,
        &self.agent_factory,
        escalation_slot,
        decision_log,
        &self.mcp_manager,
    ).map_err(|e| ProviderError::ClientCreation {
        provider: "supervisor".to_string(),
        reason: format!("Failed to create AskSupervisor: {e}"),
    })?;
```

The `configure_agent_tools!` call remains unchanged — same 9-tuple (8 tools + ThinkTool).

**`review/mod.rs` — `run_inner()` (L446-498):**

Same pattern as runner. Replace `self.create_tools(...)` with `create_tools_with_supervisor(...)`, map error to `ReviewError::AgentBuildFailed`. The `ReviewRunner` fields to pass are: `&self.config`, `&self.agent_factory`, `escalation_slot`, `decision_log`, `&self.mcp_manager`. These match the runner pattern exactly — verify field names match by checking `pub struct ReviewRunner` (L317-330).

**`supervisor/architect.rs` — `ask()` (L356-400):**

Current — 7 inline tool constructors:
```rust
let git = GitTool::new(self.project_root.clone());
let read_file = ReadFileTool::new(self.project_root.clone());
// ... 5 more ...
let terminal = TerminalTool::new(self.project_root.clone(), TERMINAL_TIMEOUT_SECS);
```

After:
```rust
use crate::session::agent::create_base_tools;

let (git, read_file, edit_file, grep, find_path, list_dir, terminal) =
    create_base_tools(&self.project_root);
```

The `configure_agent_tools!` call remains unchanged — same 8-tuple (7 tools + ThinkTool).

### Error Handling for Supervisor Creation

`AskSupervisor::with_architect_from_config()` returns a `Result<AskSupervisor, Box<dyn Error>>`. Currently each caller maps this differently:

- `runner.rs` maps to `ProviderError::ClientCreation`
- `review/mod.rs` maps to `ReviewError::AgentBuildFailed`

**Decision:** `create_tools_with_supervisor()` returns `Result<..., Box<dyn Error>>` — the raw error from `AskSupervisor`. Each caller `.map_err()` to their own type, exactly as they do today. No new error types needed. Do NOT invent a `SupervisorCreationError` type.

### `build_agent_for_role()` — Keep or Remove?

`SessionRunner::build_agent_for_role()` (L764-791) combines preamble building + tool creation + `AgentFactory::build()` + MCP. The architect brief proposed centralizing this too, but that would require `agent.rs` to know about `StoryInfo`, `EscalationSlot`, `DecisionLog`, `McpManager`, and `AgentFactory` — pulling in half the session module.

**Decision: Keep `build_agent_for_role()` in `runner.rs`.** It's session-specific orchestration logic, not reusable across review/supervisor. Only the tool construction is truly duplicated and worth centralizing.

### `build_preamble()` in `runner.rs` — Keep or Remove?

`SessionRunner::build_preamble()` (L800-807) is a thin wrapper that resolves MCP tool names and delegates to `agent::build_preamble()`. The review runner has the same logic inline (L446-448). The supervisor's architect does the same (L375-379).

This wrapper is **not** duplicated enough to centralize — each caller resolves MCP names and picks a different model (`config.llm.dev.model` vs `config.llm.review.model` vs `config.llm.supervisor.model`). Leave the wrappers in place; the rename is sufficient.

### Imports to Add in `session/agent.rs` (Task 3)

The centralized functions will need these imports:

```rust
use crate::config::BotConfig;
use crate::llm::agent_factory::AgentFactory;
use crate::mcp::McpManager;
use crate::supervisor::decisions::DecisionLog;
use crate::supervisor::EscalationSlot;
use crate::tools::edit_file::EditFileTool;
use crate::tools::find_path::FindPathTool;
use crate::tools::git::GitTool;
use crate::tools::grep::GrepTool;
use crate::tools::list_directory::ListDirectoryTool;
use crate::tools::read_file::ReadFileTool;
use crate::tools::terminal::TerminalTool;
use crate::supervisor::AskSupervisor;
use std::sync::Arc;
```

Verify actual import paths by grepping existing usage in `runner.rs` and `review/mod.rs` — the tool imports may use `crate::tools::*` re-exports from `tools/mod.rs`.

### Dead Imports to Remove After Centralization (Task 3.7)

Once `create_tools()` is removed from each caller, the direct tool type imports become unused. Remove them:

**`session/runner.rs`** — remove from the `use crate::tools::{...}` block:
- `GitTool`, `ReadFileTool`, `EditFileTool`, `GrepTool`, `FindPathTool`, `ListDirectoryTool`, `TerminalTool`
- Keep `AskSupervisor` and `EscalationSlot` imports (still used as parameters to `create_tools_with_supervisor`)

**`review/mod.rs`** (L32-35) — remove from the `use crate::tools::{...}` block:
- `EditFileTool`, `FindPathTool`, `GitTool`, `GrepTool`, `ListDirectoryTool`, `ReadFileTool`, `TerminalTool`
- Keep `AskSupervisor` import (still passed as type in the returned tuple destructuring)
- Actually: after centralization, `AskSupervisor` is only in the destructured return — check if the type is still needed. If only used in `let (git, ..., supervisor) = create_tools_with_supervisor(...)`, the types are inferred and the import can go too. Let `cargo clippy` be the arbiter.

**`supervisor/architect.rs`** (L24-32) — remove direct tool imports:
- All tool types (`GitTool`, `ReadFileTool`, etc.) — they come from `create_base_tools()` now
- Keep `use crate::session::agent::{build_preamble, create_base_tools}` (the new import)

Run `cargo clippy` — it will flag any remaining unused imports as `unused_imports` warnings.

### Unit Tests for New Public Functions (Task 3.8)

Per project convention ("Every new module must include at least basic unit tests"), add tests in `session/agent.rs` inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn test_create_base_tools_returns_seven_tools() {
    let tmp = std::env::temp_dir();
    let (git, read_file, edit_file, grep, find_path, list_dir, terminal) =
        create_base_tools(&tmp);
    // Verify tools are constructed (they hold the project root path)
    // Type system guarantees correctness — this test ensures no panic on construction
    assert_eq!(git.project_root, tmp);
    assert_eq!(read_file.project_root, tmp);
    assert_eq!(edit_file.project_root, tmp);
    assert_eq!(grep.project_root, tmp);
    assert_eq!(find_path.project_root, tmp);
    assert_eq!(list_dir.project_root, tmp);
    assert_eq!(terminal.project_root, tmp);
}
```

Note: verify the actual field name for project_root on each tool struct — it may be `project_root`, `root`, or a private field. If fields are private, assert on the tool's `NAME` constant or just verify the function doesn't panic. The key is having _some_ test that exercises the new public API.

`create_tools_with_supervisor()` is harder to unit test (requires `BotConfig`, `AgentFactory`, etc.) — skip it, the existing integration-level tests in `runner.rs` and `review/mod.rs` already exercise this path indirectly.

### Previous Story Intelligence (Story 4.6 — Post-Implementation Impact Analysis)

Story 4.6 added the impact analysis step in `runner.rs` at the `ResponseAction::Completed` arm. This code is **not affected** by the refactoring — it doesn't use `create_tools()` or `dev_agent::` imports. The only touch point is if `runner.rs` has a `use crate::session::dev_agent::{self}` that needs renaming (already covered in Task 1).

Commit `3d725cf` (latest on main) introduced the `impl_agent_configurator!` macro and gave the architect all 7+ThinkTool tools. This is the **direct precursor** to this story — it revealed the duplication and proved the macro handles arbitrary arities.

### Validation Notes

These were identified during quality review and are already addressed above:
- Module doc comment must be updated to reflect role-agnostic purpose (see "Module Doc Comment Update" section)
- `review/mod.rs` import of `agent` must not conflict with local names (see "Import Naming" section)

### What NOT to Change

- ❌ Do NOT modify `AgentFactory`, `BuiltAgent`, `configure_agent_tools!`, or `impl_agent_configurator!` — they are untouched
- ❌ Do NOT modify tool implementations in `src/tools/` — only import paths change
- ❌ Do NOT modify `session/analyzer.rs` beyond doc comment updates
- ❌ Do NOT modify `session/state.rs`, `session/branch.rs`, `session/cleanup.rs`, `session/escalation.rs`
- ❌ Do NOT modify `pipeline.rs` — it doesn't create tools directly
- ❌ Do NOT modify any BMAD files under `_bmad/`
- ❌ Do NOT change preamble content, tool registration order, or activation flow
- ❌ Do NOT introduce new error types (no `SupervisorCreationError`, no custom enums) — return `Box<dyn Error>` and let callers `.map_err()`
- ❌ Do NOT add `build_agent()` centralization (out of scope — see decision above)
- ❌ Do NOT remove the `ShutdownFlag` re-export from `session/runner.rs` — `pipeline.rs` (L26) and `cli/mod.rs` (L1292, L1414) import `ShutdownFlag` via `crate::session::runner::ShutdownFlag`. The re-export must stay: `pub use crate::session::agent::ShutdownFlag;`
- ❌ Do NOT include `ThinkTool` in `create_base_tools()` or `create_tools_with_supervisor()` return tuples — callers add it via `configure_agent_tools!` as they do today

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code
- ❌ **NO** `println!` or `eprintln!` — use `tracing` only
- ❌ **NO** changing the order of tools in the tuple — `configure_agent_tools!` is order-sensitive for the macro expansion
- ❌ **NO** creating a single function that returns different tuple types via an enum — use two separate functions
- ❌ **NO** making `create_base_tools()` return `Result` — it's infallible pure construction
- ❌ **NO** circular dependencies — `session::agent` must NOT import from `session::runner` or `review`
- ❌ **NO** inventing a `SupervisorCreationError` type — return `Box<dyn Error>` from `create_tools_with_supervisor()`
- ❌ **NO** putting `ThinkTool` inside the centralized functions — it stays in the `configure_agent_tools!` call at each call site

### Project Structure Notes

```
src/
├── session/
│   ├── mod.rs              # MODIFY — rename pub mod dev_agent → pub mod agent
│   ├── agent.rs            # RENAME from dev_agent.rs + ADD create_base_tools(), create_tools_with_supervisor(), TERMINAL_TIMEOUT_SECS
│   ├── runner.rs           # MODIFY — remove create_tools(), ToolSet, TERMINAL_TIMEOUT_SECS, update imports
│   └── (other files)       # UNCHANGED (except doc comment updates in analyzer.rs)
├── review/
│   └── mod.rs              # MODIFY — remove create_tools(), ReviewToolSet, TERMINAL_TIMEOUT_SECS, update imports
├── supervisor/
│   └── architect.rs        # MODIFY — remove inline tool construction, TERMINAL_TIMEOUT_SECS, update imports
├── llm/
│   └── agent_factory.rs    # MODIFY — update dev_agent:: → agent:: imports (3 activate_agent calls + streaming_chat + ShutdownFlag)
└── (all other modules)     # UNCHANGED
```

### References

- [Source: _bmad-output/planning-artifacts/architect-brief-agent-module-centralization.md] — Full architect brief with problem statement, proposed solution, and risk analysis
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 8] — BuiltAgent + AgentFactory pattern (the centralization this story extends)
- [Source: _bmad-output/planning-artifacts/architecture.md#Rig Tool Implementation Pattern] — Standard tool structure
- [Source: _bmad-output/project-context.md#Framework-Specific Rules] — 9 tools listed, tool design principles
- [Source: src/session/dev_agent.rs] — Current shared module to rename (build_preamble, streaming_chat, activate_agent, ShutdownFlag, ChatHistoryHook)
- [Source: src/session/runner.rs#L39-48] — ToolSet type alias (to remove)
- [Source: src/session/runner.rs#L62-66] — TERMINAL_TIMEOUT_SECS (to remove)
- [Source: src/session/runner.rs#L764-791] — build_agent_for_role (to refactor)
- [Source: src/session/runner.rs#L808-847] — create_tools (to remove)
- [Source: src/review/mod.rs#L40-49] — ReviewToolSet type alias (to remove)
- [Source: src/review/mod.rs#L61-65] — TERMINAL_TIMEOUT_SECS (to remove)
- [Source: src/review/mod.rs#L500-535] — create_tools (to remove)
- [Source: src/supervisor/architect.rs#L34] — TERMINAL_TIMEOUT_SECS (to remove)
- [Source: src/supervisor/architect.rs#L356-400] — inline tool construction in ask() (to replace)
- [Source: src/llm/agent_factory.rs] — BuiltAgent with activate_agent dispatch (imports to update)
- [Source: commit 3d725cf] — impl_agent_configurator! macro, architect gets full toolset — direct precursor to this story
- [Source: _bmad-output/implementation-artifacts/4-6-post-implementation-impact-analysis.md] — Previous story context (runner.rs post-completion flow, unrelated to this refactoring)

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List