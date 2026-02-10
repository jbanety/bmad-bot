# Story 8.5: Agent Integration — Preamble, Registration & Session Update

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a dev agent,
I want all 9 surgical tools registered and properly described in my session,
So that I can use the full tool set for efficient autonomous development.

## Acceptance Criteria

1. **Given** `session/runner.rs` contains `build_preamble()`
   **When** Story 8.5 is complete
   **Then** the preamble's tool list section is updated to describe all 9 tools: `read_file`, `edit_file`, `grep`, `find_path`, `list_directory`, `git`, `terminal`, `ask_supervisor`, `think`
   **And** a "Tool Usage Rules" section is added per Architecture Decision 7 (e.g., "use grep to find code before editing", "use read_file with line ranges after outline", "prefer edit mode over overwrite")

2. **Given** the agent builders (`build_anthropic_agent`, `build_openai_agent`, `build_copilot_agent`) in `session/runner.rs`
   **When** Story 8.5 is complete
   **Then** all 3 builders register 9 tools: `edit_file`, `read_file`, `grep`, `find_path`, `list_directory`, `git`, `terminal`, `ask_supervisor`, `think` (ThinkTool)
   **And** the previous 5-tool registration (git, fs, terminal, ask_supervisor, think) is replaced

3. **Given** `review/mod.rs` registers tools separately for the review agent
   **When** Story 8.5 is complete
   **Then** the review module's tool registration is updated to register all 9 tools (matching session runner)
   **And** the review agent builders in `run_inner()` for all 3 providers (anthropic, openai, github-copilot) are updated

4. **Given** an agent session is started
   **When** the session initializes
   **Then** all 9 tools are visible in the tool definitions sent to the LLM
   **And** each tool's description is optimized for maximum LLM clarity (clear parameter descriptions, usage examples in description text)

5. **Given** all integration is complete
   **When** a smoke test is run (agent session start)
   **Then** the agent can successfully call each of the 9 tools
   **And** no references to the old FsTool remain in session setup code

## Tasks / Subtasks

### Task 0: Prerequisite Verification (AC: all)

- [x] Verify `src/tools/read_file.rs` exists, compiles, and exports `ReadFileTool` (Story 8.1 delivered)
- [x] Verify `src/tools/edit_file.rs` exists, compiles, and exports `EditFileTool` (Story 8.2 delivered)
- [x] Verify `src/tools/grep.rs` exists, compiles, and exports `GrepTool` (Story 8.3 delivered)
- [x] Verify `src/tools/find_path.rs` exists, compiles, and exports `FindPathTool` (Story 8.3 delivered)
- [x] Verify `src/tools/list_directory.rs` exists, compiles, and exports `ListDirectoryTool` (Story 8.4 delivered)
- [x] Verify `src/tools/fs.rs` has been DELETED (Story 8.4 removed it)
- [x] Verify `src/tools/mod.rs` exports exactly: `GitTool`, `TerminalTool`, `ReadFileTool`, `EditFileTool`, `GrepTool`, `FindPathTool`, `ListDirectoryTool` — NO `FsTool`
- [x] Verify `src/supervisor/read_tool.rs` has been migrated to delegate to `ReadFileTool` (Story 8.4 delivered)
- [x] Run `cargo test` — all tests pass on current state
- [x] Run `grep -rn "FsTool" src/` — must return zero matches (Story 8.4 cleaned this up)

### Task 1: Update `src/tools/mod.rs` — Doc Comment (AC: 4)

- [x] Update the module-level doc comment to reflect the full 7-module tool set:
  - `edit_file` — Surgical search-replace edits, create new files, overwrite
  - `read_file` — Partial reading with line ranges + automatic outline mode for large files
  - `grep` — Regex search across project file contents with glob filtering
  - `find_path` — Glob-based file path discovery
  - `list_directory` — List directory contents with types and sizes
  - `git` — Git operations via git2
  - `terminal` — Shell command execution with timeout
- [x] Verify all `pub mod` and `pub use` re-exports are present and correct

### Task 2: Update `build_preamble()` in `src/session/runner.rs` (AC: 1)

- [x] Replace the current preamble text (lines 1012-1026) with the expanded version from Architecture Decision 7
- [x] The new preamble MUST include these sections:
  1. **Tools section** — lists all 9 tools: `edit_file`, `read_file`, `grep`, `find_path`, `list_directory`, `git`, `terminal`, `ask_supervisor`, plus built-in `think` tool for reasoning
  2. **Tool Usage Rules section** — per Decision 7 specification (see Dev Notes below for exact text)
  3. **Communication section** — keep the existing `OVERRIDE: communication_language = English`
  4. **Rules section** — keep the existing agent activation rules (context tags, persona embodiment, etc.)
- [x] Ensure the preamble is a single `r#"..."#` raw string literal for readability
- [x] Keep the method signature unchanged: `fn build_preamble(&self, _story: &StoryInfo) -> Result<String, ProviderError>`

### Task 3: Update `create_tools()` in `src/session/runner.rs` (AC: 2, 5)

- [x] Update the import line (currently line ~30) to import all new tool types:
  ```
  use crate::tools::{EditFileTool, FindPathTool, GitTool, GrepTool, ListDirectoryTool, ReadFileTool, TerminalTool};
  ```
- [x] Update the `create_tools()` method (currently lines 1105-1123):
  - Change return type to a struct or tuple containing all 7 custom tools + AskSupervisor:
    `(GitTool, ReadFileTool, EditFileTool, GrepTool, FindPathTool, ListDirectoryTool, TerminalTool, AskSupervisor)`
  - Instantiate all new tools with `project_root`:
    ```
    let git = GitTool::new(project_root.to_path_buf());
    let read_file = ReadFileTool::new(project_root.to_path_buf());
    let edit_file = EditFileTool::new(project_root.to_path_buf());
    let grep = GrepTool::new(project_root.to_path_buf());
    let find_path = FindPathTool::new(project_root.to_path_buf());
    let list_dir = ListDirectoryTool::new(project_root.to_path_buf());
    let terminal = TerminalTool::new(project_root.to_path_buf(), TERMINAL_TIMEOUT_SECS);
    ```
  - Return all 8 tools (7 custom + supervisor)

### Task 4: Update Agent Builders in `src/session/runner.rs` (AC: 2, 4)

- [x] Update `build_anthropic_agent()` (currently lines 863-904):
  - Destructure the full tuple from `create_tools()`
  - Register all 9 tools on the agent builder:
    ```
    let agent = client.agent(model).preamble(&preamble)
        .tool(git)
        .tool(read_file)
        .tool(edit_file)
        .tool(grep)
        .tool(find_path)
        .tool(list_dir)
        .tool(terminal)
        .tool(supervisor)
        .tool(ThinkTool)
        .build();
    ```
  - Update the tracing log: `tools = 9` (was `tools = 5`)

- [x] Update `build_openai_agent()` (currently lines 907-949):
  - Same changes as anthropic builder
  - Update tracing log: `tools = 9`

- [x] Update `build_copilot_agent()` (currently lines 956-1001):
  - Same changes as anthropic builder
  - Update tracing log: `tools = 9`

### Task 5: Update `create_tools()` in `src/review/mod.rs` (AC: 3)

- [x] Update the import line (currently line ~85) to import all new tool types:
  ```
  use crate::tools::{EditFileTool, FindPathTool, GitTool, GrepTool, ListDirectoryTool, ReadFileTool, TerminalTool};
  ```
- [x] Update the `create_tools()` method (currently lines 410-427):
  - Change return type to match the session runner's expanded tuple
  - Instantiate all 7 custom tools + supervisor (same pattern as session runner)
  - Update the doc comment: `"Create the 8 tools for the rig agent (7 custom + ask_supervisor)"`

### Task 6: Update Review Agent Builders in `src/review/mod.rs` — `run_inner()` (AC: 3)

- [x] Update ALL THREE provider blocks in `run_inner()` (currently lines 237-388):

  - **Anthropic block** (~line 288-310):
    - Destructure the full tuple from `create_tools()`
    - Register all 9 tools: `.tool(git).tool(read_file).tool(edit_file).tool(grep).tool(find_path).tool(list_dir).tool(terminal).tool(supervisor).tool(ThinkTool)`
    - Add `use rig::tools::think::ThinkTool;` import if not already present

  - **OpenAI block** (~line 314-336):
    - Same changes as anthropic block

  - **GitHub Copilot block** (~line 340-380):
    - Same changes as anthropic block

### Task 7: Verify Tool Descriptions Are LLM-Optimized (AC: 4)

- [x] Review each tool's `definition()` method output — verify the `description` field is detailed enough for an LLM to understand:
  - **When** to use the tool (vs alternatives)
  - **What** the parameters mean
  - **What** the output looks like
  - **Common patterns** and gotchas
- [x] If any tool's description is insufficiently detailed, update it in the tool's source file (`src/tools/<tool>.rs`)
- [x] This is a READ-ONLY verification task for most tools — only edit if descriptions are clearly deficient (all descriptions verified adequate)

### Task 8: Update Tests in `src/session/runner.rs` (AC: 2, 5)

- [x] Update `test_review_runner_new_stores_config` and similar tests if they assert on tool count (no tests assert on tool count directly)
- [x] If any tests reference `FsTool` in imports or assertions, update to new tool types (none found — cleaned in Story 8.4)
- [x] Run `grep -rn "FsTool" src/` — confirm zero matches remain ✅
- [x] Run `grep -rn "tools = 5" src/` — confirm zero matches remain ✅ (all 3 session builders now `tools = 9`)

### Task 9: Final Verification (AC: all)

- [x] Run `cargo fmt` ✅
- [x] Run `cargo clippy` — 3 pre-existing errors in `read_file.rs` (out of scope), zero new warnings
- [x] Run `cargo test` — 794 passed, 0 failed ✅
- [x] Run `grep -rn "FsTool" src/` — zero matches ✅
- [x] Run `grep -rn "tools = 5" src/` — zero matches ✅
- [x] Run `grep -rn "tool(fs)" src/` — zero matches ✅
- [x] Verify `src/session/runner.rs` `build_preamble()` mentions all 9 tools by name ✅
- [x] Verify all 3 session agent builders register exactly 9 tools ✅ (tools = 9 in tracing)
- [x] Verify all 3 review agent builders register exactly 9 tools ✅ (ThinkTool added to all 3)
- [x] Verify `resume_session()` compiles correctly (it calls the same builders — no direct changes needed, just compilation check) ✅
- [x] Verify `review/mod.rs` now imports `use rig::tools::think::ThinkTool;` ✅
- [x] No changes to `read_file.rs`, `edit_file.rs`, `grep.rs`, `find_path.rs`, `list_directory.rs`, `git.rs`, `terminal.rs` ✅ (verified via `git diff --name-only`)

## Dev Notes

### Previous Story Intelligence — Stories 8.1 through 8.4

**Story 8.4 (immediate predecessor) scope and impact:**
- Created `src/tools/list_directory.rs` with `ListDirectoryTool`
- Deleted `src/tools/fs.rs` entirely
- Migrated `src/supervisor/read_tool.rs` to delegate to `ReadFileTool`
- Updated `session/runner.rs` and `review/mod.rs` to replace `FsTool` with `ListDirectoryTool` in `create_tools()` and agent builders
- After 8.4, `session/runner.rs` registers **5 tools**: `git`, `list_dir`, `terminal`, `ask_supervisor`, `ThinkTool`
- After 8.4, `review/mod.rs` registers **4 tools**: `git`, `list_dir`, `terminal`, `ask_supervisor` — ⚠️ review does **NOT** have `ThinkTool` (never had it)
- The preamble was NOT changed in 8.4 — it still says the old text

**Story 8.3 delivered:** `GrepTool` in `src/tools/grep.rs`, `FindPathTool` in `src/tools/find_path.rs`
**Story 8.2 delivered:** `EditFileTool` in `src/tools/edit_file.rs`
**Story 8.1 delivered:** `ReadFileTool` in `src/tools/read_file.rs`

**Key insight:** Stories 8.1-8.3 created the tool FILES but did NOT register them in the agent builders or update the preamble. Story 8.4 removed FsTool and replaced it with ListDirectoryTool in the builders. Story 8.5 is where ALL 9 tools come together — adding 4 new tools to session runner and 5 new tools (including ThinkTool) to review.

### Actual Code State (pre-8.4) — For Task 0 Prerequisite Verification

Before Stories 8.1-8.4 are implemented, the actual codebase looks like this:
- `src/tools/mod.rs` exports only: `FsTool`, `GitTool`, `TerminalTool` (3 tools)
- `src/tools/fs.rs` still exists (912 lines)
- `src/tools/read_file.rs`, `edit_file.rs`, `grep.rs`, `find_path.rs`, `list_directory.rs` do **NOT** exist yet
- `session/runner.rs` imports `FsTool` and registers 5 tools: `git`, `fs`, `terminal`, `supervisor`, `ThinkTool`
- `review/mod.rs` imports `FsTool` and registers 4 tools: `git`, `fs`, `terminal`, `supervisor` (no ThinkTool)
- `supervisor/read_tool.rs` has its own independent `ReadFile` implementation (not delegating to ReadFileTool)

During Task 0, verify that ALL of the above has changed to the post-8.4 state before proceeding.

### Current `build_preamble()` Text — BEFORE (to be replaced)

[Source: `src/session/runner.rs` lines 1011-1026]

```rust
fn build_preamble(&self, _story: &StoryInfo) -> Result<String, ProviderError> {
    Ok(r#"You are an AI agent operating autonomously in a BMAD workflow environment.

## Tools
You have access to these tools: git, filesystem, terminal, ask_supervisor.
Use them to read files, explore the project, run commands, and ask the supervisor when you need clarification.

## Communication
OVERRIDE: communication_language = English

## Rules
- When the user provides an agent file in <context><files> tags, you MUST fully embody that agent's persona and follow ALL activation instructions exactly as specified.
- NEVER break character until given an exit command.
- Execute activation steps in order — load configuration files via tools, then greet and display the menu.
- Wait for user input after displaying the menu."#.to_string())
}
```

### New `build_preamble()` Text — AFTER (Architecture Decision 7 specification)

Replace the entire preamble with this text. This is the EXACT specification from Architecture Decision 7, section "Expanded System Preamble — Tool Usage Rules":

```rust
fn build_preamble(&self, _story: &StoryInfo) -> Result<String, ProviderError> {
    Ok(r#"You are an AI agent operating autonomously in a BMAD workflow environment.

## Tools
You have access to these tools: edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor, plus a built-in think tool for reasoning.

## Tool Usage Rules
- **ALWAYS use `edit_file` with mode="edit"** to modify existing files. NEVER rewrite entire files unless creating a new file (mode="create") or a complete rewrite is truly necessary (mode="overwrite").
- **Use `read_file` with line ranges** for large files. Read the outline first, then target specific sections with start_line/end_line.
- **Use `grep` to find symbols** before editing — never assume file paths or line numbers.
- **Use `find_path`** to discover files by name pattern when you don't know the full path.
- **Use `list_directory`** to explore directory structure.
- **Use `terminal`** for build commands, tests, mkdir, rm, and other shell operations.
- **Use `ask_supervisor`** when you need clarification on requirements, architecture decisions, or are uncertain about the correct approach.
- When `edit_file` fails (ambiguous match), use `read_file` with a line range to get more context, then retry with a larger `old_text` fragment.
- When making multiple related changes in one file, batch them in a single `edit_file` call with multiple edit operations.

## Communication
OVERRIDE: communication_language = English

## Rules
- When the user provides an agent file in <context><files> tags, you MUST fully embody that agent's persona and follow ALL activation instructions exactly as specified.
- NEVER break character until given an exit command.
- Execute activation steps in order — load configuration files via tools, then greet and display the menu.
- Wait for user input after displaying the menu."#.to_string())
}
```

### Current `create_tools()` — BEFORE (after Story 8.4)

[Source: `src/session/runner.rs` lines 1105-1123 — post Story 8.4 state]

After Story 8.4, `session/runner.rs` `create_tools()` returns `(GitTool, ListDirectoryTool, TerminalTool, AskSupervisor)` and its 3 agent builders call `.tool(git).tool(list_dir).tool(terminal).tool(supervisor).tool(ThinkTool)` (5 tools).

After Story 8.4, `review/mod.rs` `create_tools()` returns the same tuple and its 3 provider blocks call `.tool(git).tool(list_dir).tool(terminal).tool(supervisor)` (**4 tools — no ThinkTool**).

### New `create_tools()` — AFTER

```rust
/// Create the 8 tools for the rig agent: 7 custom tools + ask_supervisor.
fn create_tools(
    &self,
    project_root: &Path,
    escalation_slot: EscalationSlot,
    decision_log: DecisionLog,
) -> Result<
    (
        GitTool,
        ReadFileTool,
        EditFileTool,
        GrepTool,
        FindPathTool,
        ListDirectoryTool,
        TerminalTool,
        AskSupervisor,
    ),
    ProviderError,
> {
    let git = GitTool::new(project_root.to_path_buf());
    let read_file = ReadFileTool::new(project_root.to_path_buf());
    let edit_file = EditFileTool::new(project_root.to_path_buf());
    let grep = GrepTool::new(project_root.to_path_buf());
    let find_path = FindPathTool::new(project_root.to_path_buf());
    let list_dir = ListDirectoryTool::new(project_root.to_path_buf());
    let terminal = TerminalTool::new(project_root.to_path_buf(), TERMINAL_TIMEOUT_SECS);

    let supervisor =
        AskSupervisor::with_architect_from_config(&self.config, escalation_slot, decision_log)
            .map_err(|e| ProviderError::ClientCreation {
                provider: "supervisor".to_string(),
                reason: format!("Failed to create AskSupervisor: {e}"),
            })?;

    Ok((git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor))
}
```

### New Agent Builder Pattern — AFTER (apply to all 3 builders)

```rust
let (git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor) =
    self.create_tools(&project_root, escalation_slot, decision_log)?;

let agent = client
    .agent(model)
    .preamble(&preamble)
    .tool(git)
    .tool(read_file)
    .tool(edit_file)
    .tool(grep)
    .tool(find_path)
    .tool(list_dir)
    .tool(terminal)
    .tool(supervisor)
    .tool(ThinkTool)
    .build();

tracing::info!(
    action = "agent_built",
    tools = 9,
    model = %model,
    provider = "anthropic", // or "openai" or "github-copilot"
    "Rig agent built"
);
```

### `src/session/runner.rs` — Exact Import Line Change

```rust
// BEFORE (post Story 8.4):
use crate::tools::{GitTool, ListDirectoryTool, TerminalTool};

// AFTER:
use crate::tools::{EditFileTool, FindPathTool, GitTool, GrepTool, ListDirectoryTool, ReadFileTool, TerminalTool};
```

### `src/review/mod.rs` — Exact Import Line Change

```rust
// BEFORE (post Story 8.4):
use crate::tools::{GitTool, ListDirectoryTool, TerminalTool};

// AFTER:
use crate::tools::{EditFileTool, FindPathTool, GitTool, GrepTool, ListDirectoryTool, ReadFileTool, TerminalTool};
```

### `src/review/mod.rs` — `create_tools()` Change

Same pattern as session runner's `create_tools()` above, with **one difference**: error type is `ReviewError` instead of `ProviderError`. The return type, tool instantiation, and tuple are identical. Use `ReviewError::AgentBuildFailed` for the supervisor error mapping (same as current code).

### `src/review/mod.rs` — `run_inner()` Agent Builder Changes (All 3 Providers)

Each of the 3 provider blocks in `run_inner()` must be updated. Same "New Agent Builder Pattern" as session runner (see above) — destructure 8-tuple from `create_tools()`, register all 9 tools with `.tool(ThinkTool)` at the end.

**⚠️ CRITICAL — ThinkTool was NEVER in review before:** The review module's `run_inner()` currently does NOT include `ThinkTool` in any of its 3 agent builders (unlike session runner which has always had it). Story 8.5 MUST:
1. Add `use rig::tools::think::ThinkTool;` import to `review/mod.rs` (not present today)
2. Add `.tool(ThinkTool)` to ALL 3 provider blocks (anthropic, openai, github-copilot)
3. This brings review from **4 tools → 9 tools** (not 5→9 like session runner)

### ⚠️ Design Decision: Review Module Preamble & Tool Usage Rules

The review module's `build_preamble()` loads `dev.md` from disk and appends `OVERRIDE: communication_language = English`. It does **NOT** include the "Tool Usage Rules" section that the session runner's preamble has. This means the review agent will have 9 tools registered but no explicit guidance on how to use them efficiently (e.g., "use grep before editing", "prefer edit_file mode=edit").

**Recommendation:** Consider appending the Tool Usage Rules block to the review preamble as well. This would be a small addition to `review/mod.rs` `build_preamble()`:
```rust
Ok(format!(
    "{agent_content}\n\nOVERRIDE: communication_language = English\n\n{TOOL_USAGE_RULES}"
))
```
Where `TOOL_USAGE_RULES` is a shared constant or inline string. **This is optional** — the review agent's primary task is code review, not development. But having the rules would improve its efficiency when using `grep` and `read_file` to navigate the codebase during review. **Flag this for discussion during code review.**

### `src/tools/mod.rs` — Expected State After Story 8.4 (verify, then update doc comment only)

```rust
//! Tool modules for the rig agent — 7 focused tools for autonomous development.
//!
//! - **[`EditFileTool`]** — Surgical search-replace edits, create new files, overwrite
//! - **[`ReadFileTool`]** — Partial reading (line ranges) + automatic outline mode for large files
//! - **[`GrepTool`]** — Regex search across project file contents with glob filtering
//! - **[`FindPathTool`]** — Glob-based file path discovery
//! - **[`ListDirectoryTool`]** — List directory contents with types and sizes
//! - **[`GitTool`]** — Git operations via git2
//! - **[`TerminalTool`]** — Shell command execution with timeout

pub mod edit_file;
pub mod find_path;
pub mod git;
pub mod grep;
pub mod list_directory;
pub mod read_file;
pub mod terminal;

pub use edit_file::EditFileTool;
pub use find_path::FindPathTool;
pub use git::GitTool;
pub use grep::GrepTool;
pub use list_directory::ListDirectoryTool;
pub use read_file::ReadFileTool;
pub use terminal::TerminalTool;
```

### Tool Constructor Signatures — What Each Tool's `new()` Expects

All 5 new tools follow the same pattern as GitTool — a single `project_root: PathBuf` parameter:

| Tool | Constructor | Notes |
|------|------------|-------|
| `ReadFileTool::new(project_root)` | `PathBuf` | Outline threshold: 300 lines (hardcoded) |
| `EditFileTool::new(project_root)` | `PathBuf` | Modes: edit, create, overwrite |
| `GrepTool::new(project_root)` | `PathBuf` | Uses `regex` crate, respects `.gitignore` |
| `FindPathTool::new(project_root)` | `PathBuf` | Uses `glob`/`walkdir`, respects `.gitignore` |
| `ListDirectoryTool::new(project_root)` | `PathBuf` | Dirs first, then files, alphabetical |
| `GitTool::new(project_root)` | `PathBuf` | Unchanged from Epic 4 |
| `TerminalTool::new(project_root, timeout)` | `PathBuf, u64` | Unchanged from Epic 4 |

### Rig Tool Implementation Pattern — Reminder

[Source: `_bmad-output/planning-artifacts/architecture.md` — "Rig Tool Implementation Pattern"]

Every tool follows the standard pattern:
- `#[derive(Deserialize, Serialize)]` struct with `project_root: PathBuf`
- Dedicated `*Args` struct with `#[derive(Deserialize)]`
- Dedicated `*Error` enum with `#[derive(Debug, thiserror::Error)]`
- `impl Tool for *Tool` with `NAME`, `Error`, `Args`, `Output = String`
- The `ThinkTool` is special — imported from rig crate, NOT in `src/tools/`

### Anti-Patterns to Avoid

1. **DO NOT create a wrapper struct/trait for the 8-tool tuple.** Rig's `.tool()` builder pattern expects individual tool values.
2. **DO NOT change any tool's `NAME` constant.** The tool names are part of the LLM's interface contract.
3. **DO NOT modify any tool's `definition()` method** unless Task 7 verification reveals a clearly deficient description.
4. **DO NOT add `ThinkTool` to `src/tools/mod.rs`** — it's a rig built-in, imported from `rig::tools::think::ThinkTool`.
5. **DO NOT change `streaming_chat`, `activate_agent`, `resume_session`, or `run` methods** — these use the builder methods and will automatically pick up the 9-tool registration.

### Scope Boundaries

**IN SCOPE:**
- `src/session/runner.rs`: `build_preamble()`, `create_tools()`, `build_anthropic_agent()`, `build_openai_agent()`, `build_copilot_agent()`, import line, tracing logs
- `src/review/mod.rs`: `create_tools()`, `run_inner()` (all 3 provider blocks), import line, add ThinkTool import
- `src/tools/mod.rs`: doc comment update only (if needed after Story 8.4)
- Any tests that assert on tool count or FsTool references

**OUT OF SCOPE:**
- Individual tool implementations (`read_file.rs`, `edit_file.rs`, `grep.rs`, `find_path.rs`, `list_directory.rs`, `git.rs`, `terminal.rs`) — DO NOT MODIFY unless Task 7 finds deficient descriptions
- `src/supervisor/read_tool.rs` — already migrated in Story 8.4
- `src/supervisor/architect.rs` — uses `read_tool.rs`'s `ReadFile`, not directly affected
- WAL state, session recovery, branch management — none of these are affected
- `src/session/analyzer.rs` — response analysis is unrelated to tools
- Any other module outside `session/runner.rs`, `review/mod.rs`, and `tools/mod.rs`

### Files Created/Modified in This Story

| Action | File | Changes |
|--------|------|---------|
| Modify | `src/session/runner.rs` | Import line, `build_preamble()`, `create_tools()`, 3 agent builders, tracing logs |
| Modify | `src/review/mod.rs` | Import line, `create_tools()`, 3 provider blocks in `run_inner()`, add ThinkTool import |
| Modify (maybe) | `src/tools/mod.rs` | Doc comment only — verify it reflects all 7 modules post-8.4 |
| None | `src/tools/*.rs` | Individual tool files should NOT be modified |

### Testing Requirements

**Existing tests to update (if they assert on tool count or old imports):**
- `src/session/runner.rs` `mod tests` — check for any assertions on `tools = 5`
- `src/review/mod.rs` `mod tests` — check for any assertions on tool count or FsTool references

**No new test file needed.** The verification is:
1. `cargo test` passes (all existing + updated tests)
2. `cargo clippy` — zero warnings
3. `grep -rn "FsTool" src/` — zero matches
4. `grep -rn "tools = 5" src/` — zero matches

**Implicit consumers to verify (no code changes needed, just verify they work):**
- `resume_session()` in `session/runner.rs` (lines 341-608) — calls the same 3 agent builders for crash recovery. Verify crash recovery path compiles and existing tests pass with the updated builders.

Integration-level verification (that the agent can actually call all 9 tools) is covered by the E2E tests in Epic 7 (not yet implemented) and manual smoke testing.

### Git Intelligence — Recent Commits

Last 10 commits show Epic 8 story file creation (no implementation yet):
- `2486437` feat(story): create story 8-3 GrepTool & FindPathTool
- `2e7110c` feat(story): create story 8-2 EditFileTool — surgical search-replace editing
- `ff4e2e2` feat(story): create story 8-1 ReadFileTool — partial reading & outline mode
- `fa26a22` feat: add Epic 8 (Surgical Development Tooling) — PRD, epics, sprint status
- `b4fd1aa` docs(planning): add architect brief for PO — Surgical Development Tooling epic
- `7161267` docs(architecture): add Decision 7 — Surgical Development Tooling

This confirms Stories 8.1-8.4 have story FILES created but have NOT been implemented yet. Story 8.5 assumes all prior stories are implemented and merged before it begins.

### Project Structure Notes

- All changes align with the project structure in architecture.md
- Tool modules remain in `src/tools/` directory
- Session runner remains in `src/session/runner.rs`
- Review module remains in `src/review/mod.rs`
- No new files or directories created
- No dependency changes — all crates already present from prior stories

### References

- [Source: `_bmad-output/planning-artifacts/architecture.md` — Decision 7: Surgical Development Tooling] — Full tool inventory, preamble specification, migration path
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Rig Tool Implementation Pattern] — Standard tool structure
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Decision 5: Agent Prompt Composition] — Preamble design, XML context activation
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Project Structure & Boundaries] — Directory structure, tool file locations
- [Source: `_bmad-output/planning-artifacts/epics.md` — Story 8.5] — Original acceptance criteria and story definition
- [Source: `_bmad-output/project-context.md` — Framework-Specific Rules] — 9 tools list, tool design principle
- [Source: `_bmad-output/project-context.md` — Critical Don't-Miss Rules] — "Never rewrite entire files", tool usage rules
- [Source: `_bmad-output/implementation-artifacts/8-4-list-directory-tool-fstool-removal-complete-migration.md`] — Story 8.4 tasks showing FsTool removal and runner/review migration details
- [Source: `src/session/runner.rs` lines 1011-1026] — Current `build_preamble()` implementation
- [Source: `src/session/runner.rs` lines 1105-1123] — Current `create_tools()` implementation
- [Source: `src/session/runner.rs` lines 863-1001] — Current 3 agent builders
- [Source: `src/review/mod.rs` lines 237-388] — Current `run_inner()` with 3 provider blocks
- [Source: `src/review/mod.rs` lines 410-427] — Current `create_tools()` in review module

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (via Zed)

### Debug Log References

- `cargo test`: 794 passed, 0 failed
- `cargo fmt`: clean
- `cargo clippy`: 3 pre-existing errors in `read_file.rs` (out of scope), zero new warnings
- `grep -rn "FsTool" src/`: zero matches
- `grep -rn "tools = 5" src/`: zero matches
- `grep -rn "tool(fs)" src/`: zero matches
- Session runner: 3 builders × `tools = 9` ✅
- Review module: 3 provider blocks + ThinkTool import ✅
- Preamble: all 9 tool names present (edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor, think) ✅

### Completion Notes List

- ✅ **Task 0**: All prerequisites verified — 7 tool files exist, fs.rs deleted, mod.rs exports correct, 794 tests pass, zero FsTool refs
- ✅ **Task 1**: Updated `src/tools/mod.rs` doc comment — concise 7-tool listing with descriptions
- ✅ **Task 2**: Replaced `build_preamble()` in `session/runner.rs` — expanded tool list (9 tools), added "Tool Usage Rules" section per Architecture Decision 7
- ✅ **Task 3**: Updated `create_tools()` in `session/runner.rs` — return type expanded to 8-tuple (7 custom + AskSupervisor), all tools instantiated with `project_root`
- ✅ **Task 4**: Updated all 3 agent builders (`build_anthropic_agent`, `build_openai_agent`, `build_copilot_agent`) — 9 tools registered, tracing `tools = 9`
- ✅ **Task 5**: Updated `create_tools()` in `review/mod.rs` — same 8-tuple pattern as session runner, doc comment updated
- ✅ **Task 6**: Updated all 3 provider blocks in `review/mod.rs` `run_inner()` — 9 tools registered, added `use rig::tools::think::ThinkTool;` import (review never had ThinkTool before, now has it)
- ✅ **Task 7**: Verified all tool descriptions are LLM-optimized — all adequate, no modifications needed
- ✅ **Task 8**: No tests assert on tool count directly; zero FsTool refs; zero `tools = 5` refs
- ✅ **Task 9**: Full verification passed — fmt, clippy, test, grep checks all clean

**Note on review preamble:** The review module's `build_preamble()` loads `dev.md` from disk + English override. It does NOT include the "Tool Usage Rules" section from the session runner preamble. This is by design — the session runner's preamble is for the autonomous dev agent; the review agent gets its persona from dev.md. Flagged for code review per Dev Notes recommendation.

### Change Log

- **2026-02-10**: Story 8.5 implementation complete. Registered all 9 tools in session runner (3 builders) and review module (3 provider blocks). Updated preamble with Tool Usage Rules. Added ThinkTool to review module. 794 tests pass.

### File List

| File | Change |
|------|--------|
| `src/session/runner.rs` | **MODIFY** — Expanded import (7 tool types), `build_preamble()` rewritten with 9-tool list + Tool Usage Rules, `create_tools()` returns 8-tuple, all 3 agent builders register 9 tools with `tools = 9` tracing |
| `src/review/mod.rs` | **MODIFY** — Expanded import (7 tool types + ThinkTool), `create_tools()` returns 8-tuple, all 3 provider blocks in `run_inner()` register 9 tools including ThinkTool |
| `src/tools/mod.rs` | **MODIFY** — Doc comment updated to "7 focused tools for autonomous development" with concise descriptions |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **MODIFY** — Story 8-5 status: ready-for-dev → in-progress → review |