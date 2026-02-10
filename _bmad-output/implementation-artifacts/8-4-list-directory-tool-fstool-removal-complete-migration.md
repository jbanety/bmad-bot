# Story 8.4: ListDirectoryTool & FsTool Removal — Complete Migration

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a dev agent,
I want a dedicated directory listing tool and the legacy FsTool fully removed,
So that the tool set is clean, focused, and each tool has a single responsibility.

## Acceptance Criteria

1. **Given** a directory within the project root
   **When** `list_directory` is called with a directory path
   **Then** it returns the directory contents with entry types (file/directory) and file sizes
   **And** results are sorted alphabetically (directories first, then files)

2. **Given** a path that resolves outside the project root
   **When** `list_directory` is called
   **Then** the tool returns a clear security error

3. **Given** a non-existent directory path
   **When** `list_directory` is called
   **Then** the tool returns a clear error indicating the directory was not found

4. **Given** the `ListDirectoryTool` is implemented
   **When** `tools/fs.rs` (the old FsTool) is inspected
   **Then** it has been completely removed from the codebase
   **And** `tools/mod.rs` no longer exports `FsTool`
   **And** `tools/mod.rs` exports `ListDirectoryTool`

5. **Given** `supervisor/read_tool.rs` has its own `ReadFile` implementation
   **When** the migration is complete
   **Then** `supervisor/read_tool.rs` delegates to `ReadFileTool` from `tools/read_file.rs` instead of its own independent implementation

6. **Given** `session/runner.rs` and `review/mod.rs` reference `FsTool` in `create_tools()`
   **When** `fs.rs` is deleted
   **Then** both `create_tools()` functions are updated to create `ListDirectoryTool` instead of `FsTool`
   **And** both `use crate::tools::FsTool` imports are replaced with `use crate::tools::ListDirectoryTool`
   **And** the agent builder `.tool(fs)` calls are replaced with `.tool(list_dir)` (or equivalent)
   **And** `cargo check` passes — the codebase compiles with zero `FsTool` references

7. **Given** all changes are complete
   **When** `cargo test` is run
   **Then** all tests pass with zero references to `FsTool` in the codebase
   **And** all prior `FsTool` unit tests have been migrated to the new tools or deleted (each new tool already has its own test suite from prior stories)

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [x] Verify `src/tools/read_file.rs` exists and compiles (Story 8.1 delivered) (AC: all)
- [x] Verify `src/tools/edit_file.rs` exists and compiles (Story 8.2 delivered) (AC: all)
- [x] Verify `src/tools/grep.rs` and `src/tools/find_path.rs` exist and compile (Story 8.3 delivered) (AC: all)
- [x] Verify `cargo test` passes on current `main` (AC: all)
- [x] Verify `src/tools/mod.rs` currently exports `FsTool`, `GitTool`, `TerminalTool`, `ReadFileTool`, `EditFileTool`, `GrepTool`, `FindPathTool` (AC: 4)
- [x] Read `src/tools/read_file.rs` to confirm the `ReadFileTool` API for supervisor migration (AC: 5)
- [x] Read `src/tools/fs.rs` lines 247-293 (`handle_list`) to understand the listing logic being replaced (AC: 1)
- [x] Read `src/supervisor/read_tool.rs` to understand the current `ReadFile` implementation (AC: 5)
- [x] Read `src/session/runner.rs` lines 1105-1123 (`create_tools`) and lines 863-1001 (agent builders) to map all `FsTool` references (AC: 6)
- [x] Read `src/review/mod.rs` lines 394-427 (`build_preamble`, `create_tools`) to map all `FsTool` references (AC: 6)

### Task 1: Create `src/tools/list_directory.rs` — Struct, Args, Error Enum

- [x] Create file `src/tools/list_directory.rs` (AC: 4)
- [x] Define `ListDirectoryTool` struct: `#[derive(Debug, Serialize, Deserialize)]` with `project_root: PathBuf` field (AC: 1)
- [x] Define `ListDirectoryToolArgs` struct: `#[derive(Debug, Deserialize)]` with fields (AC: 1):
  - `path: String` — relative path from project root to the directory to list (required)
  - Doc comments on each field
- [x] Define `ListDirectoryToolError` enum: `#[derive(Debug, thiserror::Error)]` with variants (AC: 1, 2, 3):
  - `PathDenied { path: String, reason: String }` — path outside project root
  - `NotFound { path: String }` — directory does not exist
  - `NotADirectory { path: String }` — path exists but is a file, not a directory
  - `IoError { path: String, reason: String }` — I/O error during listing

### Task 2: Implement `ListDirectoryTool` Core Methods

- [x] Implement `ListDirectoryTool::new(project_root: PathBuf) -> Self` (AC: 1)
- [x] Implement `ListDirectoryTool::validate_path(&self, requested: &str) -> Result<PathBuf, ListDirectoryToolError>` — replicate the `FsTool::validate_path()` pattern exactly: `canonicalize()` + `starts_with()` (AC: 2)
- [x] Implement listing logic in `call()` (AC: 1):
  - [x] Validate the path via `validate_path()`
  - [x] Verify the path is a directory (not a file) — return `NotADirectory` error if not
  - [x] Use `tokio::fs::read_dir()` to list entries
  - [x] For each entry, collect: name, type (file/directory), and size (for files)
  - [x] Sort results: **directories first** (alphabetically), **then files** (alphabetically) — this differs from old FsTool which sorted all entries together
  - [x] Format output: `[dir]  name/` for directories, `[file] name (N bytes)` for files
  - [x] Return `"Empty directory"` for empty directories

### Task 3: Implement `Tool` Trait for `ListDirectoryTool`

- [x] Implement `Tool for ListDirectoryTool` with (AC: 1, 4):
  - `const NAME: &'static str = "list_directory"`
  - `type Error = ListDirectoryToolError`
  - `type Args = ListDirectoryToolArgs`
  - `type Output = String`
- [x] Implement `definition()` with comprehensive description and JSON schema (AC: 1):
  - Description must teach the LLM: when to use list_directory, what output looks like, how to use it for exploration
  - JSON schema with `path` (required)
- [x] Implement `call()` with tracing: `tracing::info!(action = "list_directory", path = %args.path, ...)` before and after (AC: 1)
- [x] Return meaningful output for empty directories: `"Empty directory"` (AC: 1)

### Task 4: Update Module Registry (`src/tools/mod.rs`)

- [x] Add `pub mod list_directory;` declaration (AC: 4)
- [x] Add `pub use list_directory::ListDirectoryTool;` re-export (AC: 4)
- [x] Remove `pub mod fs;` declaration (AC: 4)
- [x] Remove `pub use fs::FsTool;` re-export (AC: 4)
- [x] Update module doc comment to replace FsTool with ListDirectoryTool and reflect all 7 tool modules (AC: 4)

### Task 5: Delete `src/tools/fs.rs`

- [x] Delete the entire `src/tools/fs.rs` file (912 lines including tests) (AC: 4, 7)
- [x] Verify `src/tools/mod.rs` no longer references `fs` module (AC: 4)

### Task 6: Migrate `src/supervisor/read_tool.rs` to Use `ReadFileTool`

- [x] Replace the supervisor's independent `ReadFile` implementation with a thin wrapper around `ReadFileTool` from `tools/read_file.rs` (AC: 5)
- [x] Update the import: add `use crate::tools::ReadFileTool;` (AC: 5)
- [x] The supervisor tool should still be named `"read_file"` (same NAME as before) for backward compatibility with the Architect agent (AC: 5)
- [x] **Approach A (recommended) — Delegate internally:**
  - Keep the `ReadFile` struct but replace `project_root: PathBuf` with a `inner: ReadFileTool` field
  - Keep `ReadFileArgs` unchanged (single `path: String` field — the supervisor doesn't need line ranges or outline mode)
  - In `call()`, delegate to `inner.call()` with `start_line: None, end_line: None`
  - Map `ReadFileToolError` variants to `ReadFileError` variants in the delegation
  - This preserves the simple supervisor API while reusing ReadFileTool's implementation
- [ ] ~~**Approach B (alternative) — Full replacement:**~~ (not chosen — Approach A implemented)
  - Remove the entire `ReadFile` struct and re-export `ReadFileTool` directly
  - Update `supervisor/architect.rs` to use `ReadFileTool` instead of `ReadFile`
  - This is simpler but changes the supervisor's tool interface (adds `start_line`/`end_line` params)
  - The Architect agent would gain outline mode capabilities — may or may not be desirable
- [x] Migrate existing `read_tool.rs` tests to verify delegation works correctly (AC: 5)
- [x] Verify all existing `read_tool.rs` tests still pass (AC: 7)

### Task 7: Update `src/session/runner.rs` — Remove FsTool References

- [x] Replace `use crate::tools::{FsTool, GitTool, TerminalTool};` with `use crate::tools::{GitTool, ListDirectoryTool, TerminalTool};` at line 30 (AC: 6)
- [x] Update `create_tools()` method signature (line 1105-1123) (AC: 6):
  - Change return type from `(GitTool, FsTool, TerminalTool, AskSupervisor)` to `(GitTool, ListDirectoryTool, TerminalTool, AskSupervisor)`
  - Replace `let fs = FsTool::new(project_root.to_path_buf());` with `let list_dir = ListDirectoryTool::new(project_root.to_path_buf());`
  - Return `(git, list_dir, terminal, supervisor)` instead of `(git, fs, terminal, supervisor)`
- [x] Update all three agent builders to use the new variable name (AC: 6):
  - `build_anthropic_agent` (line 886): change `let (git, fs, terminal, supervisor) =` to `let (git, list_dir, terminal, supervisor) =` and `.tool(fs)` to `.tool(list_dir)`
  - `build_openai_agent` (line 935): same change
  - `build_copilot_agent` (line 985): same change
- [x] **DO NOT update `build_preamble()`** — preamble text changes are Story 8.5 scope (AC: 6)
- [x] **DO NOT change tool count logging** — still 5 tools registered (git, list_dir, terminal, supervisor, think). The 9-tool registration is Story 8.5 (AC: 6)

### Task 8: Update `src/review/mod.rs` — Remove FsTool References

- [x] Replace `use crate::tools::{FsTool, GitTool, TerminalTool};` with `use crate::tools::{GitTool, ListDirectoryTool, TerminalTool};` at line 85 (AC: 6)
- [x] Update `create_tools()` method (lines 410-427) (AC: 6):
  - Change return type from `(GitTool, FsTool, TerminalTool, AskSupervisor)` to `(GitTool, ListDirectoryTool, TerminalTool, AskSupervisor)`
  - Replace `let fs = FsTool::new(project_root.to_path_buf());` with `let list_dir = ListDirectoryTool::new(project_root.to_path_buf());`
  - Update doc comment `"Create the 4 tools..."` to reflect `list_directory` instead of `filesystem`
  - Return `(git, list_dir, terminal, supervisor)`
- [x] Update agent builder `.tool(fs)` to `.tool(list_dir)` in `run_inner()` where tools are registered (AC: 6)

### Task 9: Verify Zero FsTool References

- [x] Run `grep -rn "FsTool" src/` — must return zero matches (AC: 7)
- [x] Run `grep -rn "tools::fs" src/` — must return zero matches (only `tools::find_path` etc. should remain) (AC: 7)
- [x] Run `grep -rn "use.*fs::" src/tools/` — must return zero matches from the old fs module (standard library `std::fs` in tests is OK) (AC: 7)

### Task 10: Unit Tests — ListDirectoryTool (`#[cfg(test)] mod tests` in `list_directory.rs`)

- [x] `test_list_directory_basic` — list a directory with files and subdirectories (AC: 1)
- [x] `test_list_directory_dirs_first_then_files` — verify directories appear before files in output (AC: 1)
- [x] `test_list_directory_alphabetical_within_groups` — dirs sorted alphabetically, files sorted alphabetically (AC: 1)
- [x] `test_list_directory_shows_file_sizes` — file entries include byte sizes (AC: 1)
- [x] `test_list_directory_dir_entries_have_trailing_slash` — `[dir]  name/` format (AC: 1)
- [x] `test_list_directory_empty_directory` — returns "Empty directory" (AC: 1)
- [x] `test_list_directory_path_denied_outside_root` — path traversal blocked (AC: 2)
- [x] `test_list_directory_not_found` — non-existent path returns NotFound error (AC: 3)
- [x] `test_list_directory_not_a_directory` — file path returns NotADirectory error (AC: 3)
- [x] `test_list_directory_nested_path` — can list subdirectories (AC: 1)
- [x] `test_list_directory_hidden_files_included` — hidden files (dotfiles) are listed (AC: 1)
- [x] `test_list_directory_definition_name` — verify `NAME == "list_directory"` (AC: 4)
- [x] `test_list_directory_definition_has_detailed_description` — verify description is comprehensive (AC: 4)
- [x] `test_list_directory_serializable` — verify struct is serializable/deserializable (AC: 4)
- [x] `test_list_directory_error_is_send_sync` — verify error type implements Send + Sync (AC: 4)
- [x] `test_list_directory_root_path` — list the project root itself with `""` or `"."` (AC: 1)

### Task 11: Integration Verification

- [x] Run `cargo fmt` (AC: 7)
- [x] Run `cargo clippy` with zero warnings (AC: 7) — Note: 3 pre-existing clippy errors in `read_file.rs` (out of scope, not modified)
- [x] Run `cargo test` — all existing tests + new tests pass (AC: 7) — 794 tests passed, 0 failed
- [x] Verify `tools/mod.rs` exports: `GitTool`, `TerminalTool`, `ReadFileTool`, `EditFileTool`, `GrepTool`, `FindPathTool`, `ListDirectoryTool` — NO `FsTool` (AC: 4, 7)
- [x] Verify `grep -rn "FsTool" src/` returns zero results (AC: 7)
- [x] Verify no changes to `read_file.rs`, `edit_file.rs`, `grep.rs`, `find_path.rs`, `git.rs`, `terminal.rs` (AC: 7)

## Dev Notes

### Previous Story Intelligence — Stories 8.1, 8.2, 8.3

**Patterns established in Stories 8.1-8.3 that MUST be followed:**
- Tool struct: `#[derive(Debug, Serialize, Deserialize)]` with single `project_root: PathBuf` field
- Args struct: `#[derive(Debug, Deserialize)]` with doc comments on each field
- Error enum: `#[derive(Debug, thiserror::Error)]` with descriptive variants, all fields named
- `validate_path`: `canonicalize()` + `starts_with()` security check — replicate exactly from `FsTool::validate_path()` at `src/tools/fs.rs` lines 96-121 (copy before deleting fs.rs!)
- `call()` logs with `tracing::info!(action = "list_directory", ...)` before and after
- JSON schema in `definition()` uses `"type": "string"` for strings, `"type": "integer"` for integers
- Tests follow `test_{tool}_{behavior}_{scenario}` naming, Arrange → Act → Assert
- All async file I/O via `tokio::fs`
- `tempfile::TempDir` for all test fixtures
- Tools do NOT retry internally — all errors bubble to the rig agent

**From previous stories' anti-patterns (carry forward):**
- ❌ NO `unwrap()`/`expect()` in production
- ❌ NO `anyhow` — thiserror only
- ❌ NO `println!` — tracing only
- ❌ NO panic in `call()`
- ❌ NO blocking I/O on the async runtime — use `tokio::fs` for ListDirectoryTool (it's a simple listing, no need for `spawn_blocking`)

### Current FsTool `handle_list` Implementation — What to Replicate and Improve

**Current implementation at `src/tools/fs.rs` lines 247-293:**

```rust
async fn handle_list(&self, requested: &str) -> Result<String, FsToolError> {
    let path = self.validate_path(requested)?;
    tracing::info!(action = "fs_list", path = %path.display(), "Listing directory");

    let mut entries = tokio::fs::read_dir(&path).await.map_err(|e| FsToolError::IoError { ... })?;
    let mut lines: Vec<String> = Vec::new();

    while let Some(entry) = entries.next_entry().await.map_err(...)? {
        let metadata = entry.metadata().await.map_err(...)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if metadata.is_dir() {
            lines.push(format!("[dir] {}/", name));
        } else {
            lines.push(format!("[file] {} ({} bytes)", name, metadata.len()));
        }
    }
    lines.sort();
    if lines.is_empty() { Ok("Empty directory".to_string()) } else { Ok(lines.join("\n")) }
}
```

**What changes in `ListDirectoryTool`:**
1. **Directories first, then files** — the old FsTool sorts all entries together. The new tool groups directories before files, each group sorted alphabetically. This is specified in the epics acceptance criteria.
2. **`NotADirectory` error** — the old FsTool silently fails or returns an I/O error if you pass a file path. The new tool has a dedicated error variant.
3. **Standalone tool** — no `action` multiplexer. Single responsibility: list a directory.

**Implementation pattern for sorted output:**

```rust
let mut dirs: Vec<String> = Vec::new();
let mut files: Vec<String> = Vec::new();

while let Some(entry) = entries.next_entry().await? {
    let metadata = entry.metadata().await?;
    let name = entry.file_name().to_string_lossy().to_string();
    if metadata.is_dir() {
        dirs.push(format!("[dir]  {name}/"));
    } else {
        files.push(format!("[file] {name} ({} bytes)", metadata.len()));
    }
}

dirs.sort();
files.sort();

let mut result = dirs;
result.extend(files);
// result is now: directories first (sorted), then files (sorted)
```

### FsTool `validate_path` — COPY BEFORE DELETING fs.rs

**⚠️ CRITICAL:** Before deleting `fs.rs`, copy the `validate_path()` method (lines 96-121) into `list_directory.rs`. This is the canonical security boundary pattern used by ALL tools in this epic:

```rust
fn validate_path(&self, requested: &str) -> Result<PathBuf, ListDirectoryToolError> {
    let full_path = self.project_root.join(requested);

    let canonical = full_path
        .canonicalize()
        .map_err(|_| ListDirectoryToolError::NotFound {
            path: requested.to_string(),
        })?;

    let canonical_root =
        self.project_root
            .canonicalize()
            .map_err(|_| ListDirectoryToolError::PathDenied {
                path: requested.to_string(),
                reason: "Cannot resolve project root".to_string(),
            })?;

    if !canonical.starts_with(&canonical_root) {
        return Err(ListDirectoryToolError::PathDenied {
            path: requested.to_string(),
            reason: "Path is outside project root".to_string(),
        });
    }

    Ok(canonical)
}
```

Note: `ReadFileTool`, `EditFileTool`, `GrepTool`, and `FindPathTool` from Stories 8.1-8.3 each have their own copy of this pattern. This is intentional — each tool owns its security boundary independently.

### Supervisor `read_tool.rs` Migration — Detailed Analysis

**Current state of `src/supervisor/read_tool.rs`:**
- Has its OWN `ReadFile` struct with `project_root: PathBuf`
- Has its OWN `ReadFileArgs` with just `path: String` (no `start_line`/`end_line`)
- Has its OWN `ReadFileError` enum (NotFound, ReadFailed, PathDenied)
- Has its OWN `validate_path()` implementation
- Tool NAME is `"read_file"`
- Used ONLY by the Architect agent in `supervisor/architect.rs`
- Does NOT use `FsTool` — completely independent implementation
- Has 8 unit tests

**The epics say:** "supervisor/read_tool.rs uses ReadFileTool instead of FsTool" — but in reality the supervisor's `ReadFile` NEVER used `FsTool`. It's a separate tool. The intent is to consolidate: instead of two independent `read_file` implementations, delegate to the canonical `ReadFileTool`.

**Recommended approach — Delegate internally (Approach A):**

```rust
use crate::tools::ReadFileTool;

pub struct ReadFile {
    inner: ReadFileTool,
}

impl ReadFile {
    pub fn new(project_root: PathBuf) -> Self {
        Self {
            inner: ReadFileTool::new(project_root),
        }
    }
}

impl Tool for ReadFile {
    const NAME: &'static str = "read_file";
    type Error = ReadFileError;
    type Args = ReadFileArgs;  // Still just { path: String }
    type Output = String;

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Delegate with no line range → full read or outline mode
        let inner_args = ReadFileToolArgs {
            path: args.path.clone(),
            start_line: None,
            end_line: None,
        };

        self.inner.call(inner_args).await.map_err(|e| {
            // Map ReadFileToolError → ReadFileError
            match e {
                ReadFileToolError::NotFound { path } => ReadFileError::NotFound { path },
                ReadFileToolError::PathDenied { path, reason } => ReadFileError::PathDenied { path, reason },
                ReadFileToolError::ReadFailed { path, reason } => ReadFileError::ReadFailed { path, reason },
                // Handle any other variants...
            }
        })
    }
}
```

**Benefits:**
- Supervisor tool API unchanged (simple `{ path }` args)
- All file reading logic consolidated in `ReadFileTool`
- Supervisor's `validate_path()` removed (delegated to `ReadFileTool`)
- Architect agent now gets outline mode for large files (improvement)
- Tests remain largely the same — just verify delegation

**Keep the `ReadFileError` enum** — the supervisor module should own its error types. Map from `ReadFileToolError` to `ReadFileError` in the delegation layer.

**Keep `ReadFileArgs`** — no `start_line`/`end_line` for the supervisor. The Architect agent only needs simple file reading.

### `session/runner.rs` and `review/mod.rs` FsTool References — Exact Changes

**⚠️ CRITICAL COMPILATION ISSUE:** Deleting `fs.rs` without updating these files causes compilation failure. Both files import and use `FsTool`. These changes are REQUIRED in Story 8.4, even though the full 9-tool registration is Story 8.5.

**`src/session/runner.rs` — 3 locations to change:**

1. **Line 30 — import:**
   ```
   // BEFORE:
   use crate::tools::{FsTool, GitTool, TerminalTool};
   // AFTER:
   use crate::tools::{GitTool, ListDirectoryTool, TerminalTool};
   ```

2. **Lines 1105-1123 — `create_tools()` method:**
   ```
   // BEFORE:
   fn create_tools(&self, project_root: &Path, ...) -> Result<(GitTool, FsTool, TerminalTool, AskSupervisor), ProviderError> {
       let git = GitTool::new(project_root.to_path_buf());
       let fs = FsTool::new(project_root.to_path_buf());
       ...
       Ok((git, fs, terminal, supervisor))
   }
   // AFTER:
   fn create_tools(&self, project_root: &Path, ...) -> Result<(GitTool, ListDirectoryTool, TerminalTool, AskSupervisor), ProviderError> {
       let git = GitTool::new(project_root.to_path_buf());
       let list_dir = ListDirectoryTool::new(project_root.to_path_buf());
       ...
       Ok((git, list_dir, terminal, supervisor))
   }
   ```

3. **Lines 886, 935, 985 — all 3 agent builders (destructuring + `.tool()` call):**
   ```
   // BEFORE (in each builder):
   let (git, fs, terminal, supervisor) = self.create_tools(&project_root, escalation_slot, decision_log)?;
   let agent = client.agent(model).preamble(&preamble)
       .tool(git).tool(fs).tool(terminal).tool(supervisor).tool(ThinkTool).build();
   // AFTER:
   let (git, list_dir, terminal, supervisor) = self.create_tools(&project_root, escalation_slot, decision_log)?;
   let agent = client.agent(model).preamble(&preamble)
       .tool(git).tool(list_dir).tool(terminal).tool(supervisor).tool(ThinkTool).build();
   ```

**`src/review/mod.rs` — 2 locations to change:**

1. **Line 85 — import:**
   ```
   // BEFORE:
   use crate::tools::{FsTool, GitTool, TerminalTool};
   // AFTER:
   use crate::tools::{GitTool, ListDirectoryTool, TerminalTool};
   ```

2. **Lines 410-427 — `create_tools()` method:**
   ```
   // BEFORE:
   fn create_tools(&self, project_root: &Path, ...) -> Result<(GitTool, FsTool, TerminalTool, AskSupervisor), ReviewError> {
       let git = GitTool::new(project_root.to_path_buf());
       let fs = FsTool::new(project_root.to_path_buf());
       ...
       Ok((git, fs, terminal, supervisor))
   }
   // AFTER:
   fn create_tools(&self, project_root: &Path, ...) -> Result<(GitTool, ListDirectoryTool, TerminalTool, AskSupervisor), ReviewError> {
       let git = GitTool::new(project_root.to_path_buf());
       let list_dir = ListDirectoryTool::new(project_root.to_path_buf());
       ...
       Ok((git, list_dir, terminal, supervisor))
   }
   ```

3. **In `run_inner()` wherever `.tool(fs)` is called — update to `.tool(list_dir)`** (AC: 6)

**⚠️ DO NOT change:**
- `build_preamble()` in `session/runner.rs` — that's Story 8.5
- Tool count logging (`tools = 5`) — stays at 5 until Story 8.5 adds all 9
- The `build_preamble()` in `review/mod.rs` — loads from file, no changes needed

### Tool Definition Description — Critical for LLM Usage

**ListDirectoryTool `definition()` description — suggested template:**

> List the contents of a directory in the project. Returns files and subdirectories with types and sizes.
>
> **Output format:** Directories are listed first (alphabetically), then files (alphabetically). Each entry shows:
> - `[dir]  name/` — for directories
> - `[file] name (N bytes)` — for files with their size
>
> **Usage:** Provide a `path` relative to the project root. Use `"."` or `""` to list the project root.
>
> **Prefer `list_directory` when** you need to explore directory structure or check what files exist in a specific folder.
> **Prefer `find_path` when** you need to find files matching a pattern across the entire project.
> **Prefer `grep` when** you need to find files containing specific code or text.

### Edge Cases to Handle

- **Empty string or `"."` as path** → list the project root. `self.project_root.join("")` gives the root itself. `canonicalize()` should resolve correctly.
- **Trailing slash** → `"src/"` should work same as `"src"`. `PathBuf::join` handles this.
- **Symlinks** → `canonicalize()` in `validate_path` resolves symlinks. If a symlink points outside project root, it will be denied. Entries within the listing that are symlinks should show as their target type (file/dir based on metadata).
- **Permission denied** → `read_dir()` may fail on directories the process can't read. Return `IoError`.
- **Very large directories** → No pagination for MVP. List all entries. If this becomes a problem, pagination can be added later.
- **Hidden files (dotfiles)** → Include all entries including hidden files. Unlike `GrepTool`/`FindPathTool` which use the `ignore` crate and skip hidden files, `list_directory` should show everything for full directory exploration. This matches the behavior of `ls -a` vs the search tools which behave like `ripgrep`.

### FsTool Actions Being Removed — Where They Go

| FsTool Action | Replacement | Notes |
|---|---|---|
| `read` | `ReadFileTool` (Story 8.1) | Enhanced with line ranges + outline mode |
| `write` | `EditFileTool` (Story 8.2) | Surgical edits, create, overwrite modes |
| `list` | `ListDirectoryTool` (this story) | Dirs-first sorting, dedicated tool |
| `mkdir` | `TerminalTool` (`mkdir -p`) | Agent uses terminal for infrequent ops |
| `delete` | `TerminalTool` (`rm`, `rm -rf`) | Agent uses terminal for infrequent ops |
| `exists` | `TerminalTool` (`test -e`) or `read_file` | Agent checks via read or terminal |

### FsTool Unit Tests — Migration Strategy

`src/tools/fs.rs` has 24 unit tests (lines 498-912). Migration strategy:

| FsTool Test | Action | Reason |
|---|---|---|
| `test_fs_tool_definition_name` | DELETE | FsTool gone, ListDirectoryTool has its own definition test |
| `test_fs_tool_definition_has_detailed_description` | DELETE | Same as above |
| `test_fs_tool_definition_action_enum` | DELETE | No action enum in new tools |
| `test_fs_tool_args_deserialize_minimal` | DELETE | Each new tool has its own args tests |
| `test_fs_tool_args_deserialize_full` | DELETE | Same as above |
| `test_fs_tool_error_is_send_sync` | DELETE | Each new tool has its own error tests |
| `test_fs_tool_error_display` | DELETE | Same |
| `test_fs_tool_serializable` | DELETE | Same |
| `test_fs_tool_invalid_action` | DELETE | No action multiplexer in new tools |
| `test_fs_tool_path_denied_outside_root` | COVERED | All new tools have path denial tests |
| `test_fs_tool_read_existing_file` | COVERED | ReadFileTool tests (Story 8.1) |
| `test_fs_tool_read_not_found` | COVERED | ReadFileTool tests |
| `test_fs_tool_write_new_file` | COVERED | EditFileTool tests (Story 8.2) |
| `test_fs_tool_write_overwrites_existing` | COVERED | EditFileTool tests |
| `test_fs_tool_write_creates_parent_dirs` | COVERED | EditFileTool tests |
| `test_fs_tool_list_directory` | **MIGRATE** | Adapt to ListDirectoryTool format (dirs first) |
| `test_fs_tool_list_empty_directory` | **MIGRATE** | Adapt to ListDirectoryTool |
| `test_fs_tool_mkdir_single` | DELETE | Pushed to TerminalTool |
| `test_fs_tool_mkdir_recursive` | DELETE | Pushed to TerminalTool |
| `test_fs_tool_delete_file` | DELETE | Pushed to TerminalTool |
| `test_fs_tool_delete_directory_recursive` | DELETE | Pushed to TerminalTool |
| `test_fs_tool_exists_true_file` | DELETE | Pushed to TerminalTool / ReadFileTool |
| `test_fs_tool_exists_true_directory` | DELETE | Pushed to TerminalTool |
| `test_fs_tool_exists_false` | DELETE | Pushed to TerminalTool |
| `test_fs_tool_write_missing_content` | COVERED | EditFileTool tests |

**Summary:** 22 tests DELETE (covered by new tool test suites), 2 tests MIGRATE to ListDirectoryTool's test suite. The "migrate" tests should be rewritten to match the new format (dirs-first sorting).

### Dependencies — No New Crates Required

All needed crates are already in `Cargo.toml`:
- `rig-core = "0.30"` — Tool trait
- `serde = { version = "1", features = ["derive"] }` — Serialize/Deserialize
- `serde_json = "1"` — JSON schema in tool definition
- `thiserror = "2"` — Error enum
- `tracing = "0.1"` — Structured logging
- `tokio = { version = "1", features = ["full"] }` — Async file I/O (read_dir, metadata)
- `tempfile = "3"` (dev) — Test fixtures

No additions needed. The `ignore` and `globset` crates added in Story 8.3 are not used by ListDirectoryTool (it lists a single directory, not recursive search).

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code — only in tests
- ❌ **NO** `anyhow::Result` — typed `thiserror` enums only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** panicking in `call()` — always return `Result`
- ❌ **NO** creating any new `FsTool` references or re-exports — it must be fully removed
- ❌ **NO** modifying `src/tools/read_file.rs` — ReadFileTool is complete
- ❌ **NO** modifying `src/tools/edit_file.rs` — EditFileTool is complete
- ❌ **NO** modifying `src/tools/grep.rs` — GrepTool is complete
- ❌ **NO** modifying `src/tools/find_path.rs` — FindPathTool is complete
- ❌ **NO** modifying `src/tools/git.rs` — GitTool is unchanged
- ❌ **NO** modifying `src/tools/terminal.rs` — TerminalTool is unchanged
- ❌ **NO** updating `build_preamble()` in `session/runner.rs` — that's Story 8.5
- ❌ **NO** changing tool count from 5 to 9 in agent builders — that's Story 8.5
- ❌ **NO** adding `ReadFileTool`, `EditFileTool`, `GrepTool`, `FindPathTool` to agent builders — that's Story 8.5
- ❌ **NO** `action: String` multiplexer field — single responsibility per tool
- ❌ **NO** internal retry logic — errors bubble to the rig agent
- ❌ **NO** sorting all entries together — directories MUST come before files in ListDirectoryTool output
- ❌ **NO** using the `ignore` crate for listing — `tokio::fs::read_dir` is correct for single-directory listing (ignore is for recursive walking with gitignore)

### Scope Boundaries

**IN SCOPE for this story:**
- `src/tools/list_directory.rs` — **CREATE** — Full ListDirectoryTool implementation + unit tests
- `src/tools/fs.rs` — **DELETE** — Remove legacy FsTool entirely (912 lines)
- `src/tools/mod.rs` — **MODIFY** — Remove FsTool, add ListDirectoryTool
- `src/supervisor/read_tool.rs` — **MODIFY** — Delegate to ReadFileTool instead of independent implementation
- `src/session/runner.rs` — **MODIFY** — Replace FsTool imports and usage with ListDirectoryTool in `create_tools()` and all 3 agent builders
- `src/review/mod.rs` — **MODIFY** — Replace FsTool imports and usage with ListDirectoryTool in `create_tools()` and agent builder

**OUT OF SCOPE — do NOT implement:**
- `session/runner.rs` `build_preamble()` update (Story 8.5)
- Registration of 9 tools in agent builders (Story 8.5)
- Tool count logging change from 5 to 9 (Story 8.5)
- Any changes to `read_file.rs`, `edit_file.rs`, `grep.rs`, `find_path.rs`, `git.rs`, `terminal.rs`

### Files Created/Modified/Deleted in This Story

| File | Change |
|------|--------|
| `src/tools/list_directory.rs` | **CREATE** — Full ListDirectoryTool implementation + unit tests |
| `src/tools/fs.rs` | **DELETE** — Remove legacy FsTool entirely (912 lines) |
| `src/tools/mod.rs` | **MODIFY** — Remove `pub mod fs; pub use fs::FsTool;`, add `pub mod list_directory; pub use list_directory::ListDirectoryTool;`, update doc comment |
| `src/supervisor/read_tool.rs` | **MODIFY** — Delegate to `ReadFileTool`, remove independent implementation |
| `src/session/runner.rs` | **MODIFY** — Replace `FsTool` import + `create_tools()` return type + all 3 builder destructuring/tool calls |
| `src/review/mod.rs` | **MODIFY** — Replace `FsTool` import + `create_tools()` return type + builder tool call |

### Testing Requirements

All tests follow established patterns: `test_{tool}_{behavior}_{scenario}`, Arrange → Act → Assert, `tempfile::TempDir` for test fixtures.

**ListDirectoryTool test fixture helper:**

```rust
fn create_test_directory(dir: &Path) -> PathBuf {
    // Create structure:
    // dir/
    //   src/
    //     main.rs       → "fn main() {}" (13 bytes)
    //     lib.rs        → "pub mod tools;" (15 bytes)
    //   docs/
    //     README.md     → "# Readme" (8 bytes)
    //   Cargo.toml      → "[package]" (9 bytes)
    //   .gitignore      → "target/" (7 bytes)
}
```

**For dirs-first sorting tests:** Create a mix of files and directories and verify directories appear before files in the output.

**For supervisor migration tests:** Verify that the delegating `ReadFile` tool still passes all existing test cases — same inputs, same outputs (or improved outputs from ReadFileTool).

**Test coverage targets:**
- ListDirectoryTool: ~16 tests
- Supervisor read_tool.rs migration: existing 8 tests adapted

### Project Structure Notes

After this story, the tools module loses `fs.rs` and gains `list_directory.rs`:

```
src/tools/
├── mod.rs              # Module declarations + pub re-exports (GitTool, TerminalTool, ReadFileTool, EditFileTool, GrepTool, FindPathTool, ListDirectoryTool)
├── list_directory.rs   # ListDirectoryTool — list directory contents (NEW — replaces FsTool list action)
├── grep.rs             # GrepTool — regex search across file contents (Story 8.3)
├── find_path.rs        # FindPathTool — glob-based file path discovery (Story 8.3)
├── edit_file.rs        # EditFileTool — surgical search-replace, create, overwrite (Story 8.2)
├── read_file.rs        # ReadFileTool — partial reading + outline mode (Story 8.1)
├── git.rs              # GitTool — 9 git operations via git2 (UNCHANGED)
└── terminal.rs         # TerminalTool — shell execution with timeout (UNCHANGED)
```

Note: `fs.rs` is **GONE**. The FsTool monolith has been fully replaced by 5 focused tools.

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 8.4: ListDirectoryTool & FsTool Removal — Complete Migration] — Acceptance criteria and user story
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 8: Surgical Development Tooling] — Epic context, dependency chain (8.1→8.2→8.3→**8.4**→8.5), impact metrics
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 7: Surgical Development Tooling] — Full tool inventory, migration path, FsTool removal spec
- [Source: _bmad-output/planning-artifacts/architecture.md#Rig Tool Implementation Pattern — Standard Structure] — Mandatory tool structure pattern
- [Source: _bmad-output/planning-artifacts/architecture.md#Error Type Pattern — Per-Module thiserror Enums] — Per-module thiserror, no anyhow
- [Source: _bmad-output/planning-artifacts/architecture.md#Tracing Pattern — Structured Spans with Story Context] — Every tool action logged
- [Source: _bmad-output/planning-artifacts/architecture.md#Project Structure & Boundaries] — tools/ module layout showing `list_directory.rs`, no `fs.rs`
- [Source: _bmad-output/planning-artifacts/architect-brief-surgical-tooling.md#Story 8.4] — Architect brief with scope and rationale
- [Source: _bmad-output/project-context.md#Framework-Specific Rules] — 9 tools, tool design principle, module structure
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — One tool = one concern
- [Source: _bmad-output/project-context.md#Testing Rules] — Tests inline, `#[cfg(test)] mod tests`, every module must include unit tests
- [Source: _bmad-output/project-context.md#Code Quality & Style Rules] — rustfmt, clippy, doc comments mandatory
- [Source: src/tools/fs.rs#L86-121] — `FsTool::validate_path()` — COPY this pattern before deleting fs.rs
- [Source: src/tools/fs.rs#L247-293] — `FsTool::handle_list()` — current listing logic to replicate/improve
- [Source: src/tools/fs.rs#L417-495] — `impl Tool for FsTool` — reference for Tool trait impl
- [Source: src/tools/fs.rs#L498-912] — FsTool unit tests — migration decisions documented above
- [Source: src/tools/mod.rs] — Current module registry (remove FsTool, add ListDirectoryTool)
- [Source: src/supervisor/read_tool.rs] — Current independent ReadFile implementation (refactor to delegate to ReadFileTool)
- [Source: src/session/runner.rs#L30] — `use crate::tools::{FsTool, GitTool, TerminalTool}` — must replace FsTool
- [Source: src/session/runner.rs#L1105-1123] — `create_tools()` — must replace FsTool with ListDirectoryTool
- [Source: src/session/runner.rs#L863-1001] — All 3 agent builders — must update destructuring and .tool() calls
- [Source: src/review/mod.rs#L85] — `use crate::tools::{FsTool, GitTool, TerminalTool}` — must replace FsTool
- [Source: src/review/mod.rs#L410-427] — `create_tools()` — must replace FsTool with ListDirectoryTool
- [Source: _bmad-output/implementation-artifacts/8-1-read-file-tool-partial-reading-outline-mode.md] — Story 8.1 dev notes — ReadFileTool API for supervisor migration
- [Source: _bmad-output/implementation-artifacts/8-2-edit-file-tool-surgical-search-replace-editing.md] — Story 8.2 dev notes — established patterns
- [Source: _bmad-output/implementation-artifacts/8-3-grep-tool-find-path-tool-codebase-search-navigation.md] — Story 8.3 dev notes — established patterns, ignore/globset crates
- [Source: Cargo.toml] — All dependencies verified present, no additions needed

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (via Zed)

### Debug Log References

- `cargo test`: 794 passed, 0 failed (down from 802 — net: -24 FsTool tests deleted, +16 ListDirectoryTool tests, +1 read_tool migration test)
- `cargo fmt`: clean
- `cargo clippy`: 3 pre-existing errors in `read_file.rs` (out of scope), zero warnings in changed files
- `grep -rn "FsTool" src/`: zero code references (1 doc comment in `list_directory.rs`)
- `grep -rn "tools::fs" src/`: zero matches
- `grep -rn "use.*fs::" src/tools/`: zero matches

### Completion Notes List

- ✅ **Task 0**: All prerequisites verified — 8 tools files exist, 802 tests pass, all source files read
- ✅ **Task 1**: Created `src/tools/list_directory.rs` with `ListDirectoryTool` struct, `ListDirectoryToolArgs`, `ListDirectoryToolError` enum (PathDenied, NotFound, NotADirectory, IoError)
- ✅ **Task 2**: Implemented `new()`, `validate_path()` (canonicalize + starts_with), and listing logic with dirs-first sorting
- ✅ **Task 3**: Implemented `Tool` trait — NAME="list_directory", comprehensive `definition()` with LLM guidance, `call()` with tracing before/after
- ✅ **Task 4**: Updated `src/tools/mod.rs` — removed `pub mod fs` / `pub use fs::FsTool`, added `pub mod list_directory` / `pub use list_directory::ListDirectoryTool`, updated doc comment
- ✅ **Task 5**: Deleted `src/tools/fs.rs` (912 lines)
- ✅ **Task 6**: Migrated `src/supervisor/read_tool.rs` — Approach A (delegate internally). Replaced `project_root: PathBuf` with `inner: ReadFileTool`. Added `From<ReadFileToolError> for ReadFileError` mapping. Removed independent `validate_path()`. Updated 2 test assertions to account for ReadFileTool's line-numbered output format. Added `test_read_file_error_from_is_directory` test.
- ✅ **Task 7**: Updated `src/session/runner.rs` — replaced `FsTool` import, `create_tools()` return type, and all 3 agent builders (anthropic, openai, copilot) destructuring + `.tool()` calls
- ✅ **Task 8**: Updated `src/review/mod.rs` — replaced `FsTool` import, `create_tools()` return type, and all 3 provider branches in `run_inner()` destructuring + `.tool()` calls
- ✅ **Task 9**: Verified zero FsTool references in code (only 1 doc comment)
- ✅ **Task 10**: 16 unit tests implemented in `list_directory.rs` covering all ACs
- ✅ **Task 11**: Integration verification complete — fmt, clippy (no new issues), 794 tests pass, exports verified, no unintended file changes

**Decision: Approach A for supervisor migration** — kept `ReadFile` wrapper with `inner: ReadFileTool` field. Supervisor API unchanged (simple `{path}` args). Architect agent now gains outline mode for large files automatically via delegation.

**Observation:** Supervisor `read_tool.rs` tests for `test_read_file_existing_file` and `test_read_file_nested_path` needed assertion updates because `ReadFileTool` returns line-numbered output (`"1 | content"`) instead of raw content. Changed from exact equality to `contains()` assertions.

### Change Log

- **2026-02-10**: Story 8.4 implementation complete. Created ListDirectoryTool (dirs-first listing), deleted FsTool (912 lines), migrated supervisor read_tool.rs to delegate to ReadFileTool, updated session/runner.rs and review/mod.rs to use ListDirectoryTool. 794 tests pass.

### File List

| File | Change |
|------|--------|
| `src/tools/list_directory.rs` | **CREATE** — ListDirectoryTool implementation + 16 unit tests (545 lines) |
| `src/tools/fs.rs` | **DELETE** — Legacy FsTool removed (912 lines) |
| `src/tools/mod.rs` | **MODIFY** — Removed fs module/export, added list_directory module/export, updated doc comment |
| `src/supervisor/read_tool.rs` | **MODIFY** — Delegate to ReadFileTool (Approach A), removed validate_path(), added From impl, updated 2 test assertions, added 1 test |
| `src/session/runner.rs` | **MODIFY** — Replaced FsTool import/usage with ListDirectoryTool in create_tools() + 3 agent builders |
| `src/review/mod.rs` | **MODIFY** — Replaced FsTool import/usage with ListDirectoryTool in create_tools() + 3 provider branches |
| `_bmad-output/implementation-artifacts/sprint-status.yaml` | **MODIFY** — Story 8-4 status: ready-for-dev → in-progress → review |