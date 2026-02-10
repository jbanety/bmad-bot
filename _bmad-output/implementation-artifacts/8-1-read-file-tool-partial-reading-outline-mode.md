# Story 8.1: ReadFileTool — Partial Reading & Outline Mode

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a dev agent,
I want to read files with optional line ranges and get automatic outlines for large files,
so that I can navigate the codebase efficiently without wasting tokens on irrelevant content.

## Acceptance Criteria

1. **Full read (small file):** Given a file exists within the project root, when `read_file` is called with no line range parameters and the file is ≤ 300 lines, then the complete file content is returned with line numbers prepended to each line (1-indexed).

2. **Partial read (line range):** Given a file exists within the project root, when `read_file` is called with `start_line` and/or `end_line` parameters (1-indexed, inclusive), then only the specified line range is returned with line numbers prepended. Out-of-range values are clamped to file boundaries without error. A `start_line` of `0` is clamped to `1`. If `start_line > end_line` after clamping, an empty result is returned (not an error).

3. **Outline mode (large file):** Given a file exists within the project root, when `read_file` is called with no line range parameters and the file is > 300 lines, then an automatic outline is returned instead of full content, and the outline contains symbol names (functions, structs, impls, mods, classes, etc.) with their line numbers, generated via regex-based symbol extraction (not AST parsing).

4. **Security boundary:** Given a path that resolves outside the project root, when `read_file` is called, then the tool returns a clear security error and no file content is read.

5. **Rig Tool pattern compliance:** Given the tool is implemented, when inspecting the code structure, then it follows the standard rig Tool pattern (serializable struct + `ReadFileToolArgs` + `ReadFileToolError` thiserror enum + `impl Tool`), the tool NAME and definition description are detailed enough for the LLM to use correctly, and `tools/mod.rs` exports `ReadFileTool`.

6. **Unit test coverage:** A full unit test suite covers: normal read, line range read (start only, end only, both, clamping), outline mode trigger, outline symbol extraction quality, security boundary rejection, edge cases (empty file, binary file detection, non-existent file, file with exactly 300 lines, file with 301 lines).

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [x] Verify `Cargo.toml` has all needed dependencies: `rig-core = "0.30"`, `serde`, `serde_json`, `thiserror = "2"`, `tracing`, `tokio` (full), `regex = "1"` — **all already present, no changes needed**
- [x] Verify `tempfile = "3"` exists in `[dev-dependencies]` for tests — **already present**
- [x] Read and understand `src/tools/fs.rs` for the existing `FsTool` patterns (security boundary via `validate_path`, tracing, error handling)
- [x] Read and understand `src/supervisor/read_tool.rs` for the existing `ReadFile` supervisor tool patterns (same security approach, simpler scope)

### Task 1: Create `src/tools/read_file.rs` — Struct, Args, Error Enum

- [x] Create `src/tools/read_file.rs` with module doc comment explaining purpose
- [x] Define `ReadFileTool` struct (AC: #5)
  - [x] `#[derive(Debug, Serialize, Deserialize)]`
  - [x] Single field: `project_root: PathBuf`
  - [x] Doc comment: `/// ReadFileTool — read files with optional line ranges and automatic outline mode for large files.`
- [x] Define `ReadFileToolArgs` struct (AC: #2, #3)
  - [x] `#[derive(Debug, Deserialize)]`
  - [x] `path: String` — relative path from project root
  - [x] `start_line: Option<u32>` — 1-indexed, inclusive
  - [x] `end_line: Option<u32>` — 1-indexed, inclusive
  - [x] Doc comments on each field explaining usage
- [x] Define `ReadFileToolError` enum (AC: #4, #5)
  - [x] `#[derive(Debug, thiserror::Error)]`
  - [x] `NotFound { path: String }` — file does not exist
  - [x] `PathDenied { path: String, reason: String }` — path outside project root
  - [x] `ReadFailed { path: String, reason: String }` — I/O error during read
  - [x] `IsDirectory { path: String }` — path points to a directory, not a file

### Task 2: Implement `ReadFileTool` Core Methods

- [x] Implement `ReadFileTool::new(project_root: PathBuf) -> Self`
- [x] Implement `fn validate_path(&self, requested: &str) -> Result<PathBuf, ReadFileToolError>` (AC: #4)
  - [x] Same pattern as `FsTool::validate_path`: canonicalize requested path, canonicalize project_root, check `starts_with`
  - [x] Return `NotFound` if canonicalize fails (file doesn't exist)
  - [x] Return `PathDenied` if path resolves outside project root
- [x] Implement `fn format_with_line_numbers(content: &str, start_offset: usize) -> String`
  - [x] Prepend 1-indexed line numbers to each line, right-aligned with appropriate padding
  - [x] `start_offset` is the 0-based index into the original file's lines (for partial reads)
- [x] Implement `async fn read_full_or_range(&self, path: &Path, args: &ReadFileToolArgs) -> Result<String, ReadFileToolError>` (AC: #1, #2)
  - [x] Read file content via `tokio::fs::read_to_string` — if this fails with invalid UTF-8, return `ReadFailed` with message "File appears to be binary or non-UTF-8 encoded" (do NOT let the raw IO error propagate unchecked)
  - [x] If `start_line` or `end_line` is provided → normalize inputs first: clamp `start_line` of `0` to `1`, clamp `end_line` to total line count. If `start_line > end_line` after clamping → return empty string (not an error). Then extract the specified range (1-indexed, inclusive).
  - [x] If no range and lines ≤ 300 → return full content with line numbers
  - [x] If no range and lines > 300 → delegate to outline extraction
- [x] Implement `fn extract_outline(content: &str, file_path: &str) -> String` (AC: #3)
  - [x] Use `regex` crate for language-aware symbol extraction
  - [x] Use file extension from `file_path` to select pattern set: `.rs` → Rust patterns, `.md` → Markdown patterns, everything else → generic fallback
  - [x] Declare regex patterns as `static LazyLock<Regex>` at module level (edition 2024/rustc 1.86+ — `LazyLock` is stable in `std::sync`). This avoids recompiling on every `call()` while keeping the tool struct free of regex state.
  - [x] **Rust patterns** (for `.rs` files — primary, this is a Rust project):
    - `^\s*(pub(\([^)]*\))?\s+)?(async\s+)?fn\s+\w+` — function declarations (handles `pub`, `pub(crate)`, `pub(super)`)
    - `^\s*(pub(\([^)]*\))?\s+)?struct\s+\w+` — struct declarations
    - `^\s*(pub(\([^)]*\))?\s+)?enum\s+\w+` — enum declarations
    - `^\s*impl(<[^>]*>)?\s+\w+` — impl blocks
    - `^\s*(pub(\([^)]*\))?\s+)?mod\s+\w+` — module declarations
    - `^\s*(pub(\([^)]*\))?\s+)?trait\s+\w+` — trait declarations
    - `^\s*(pub(\([^)]*\))?\s+)?type\s+\w+` — type aliases
    - `^\s*(pub(\([^)]*\))?\s+)?const\s+\w+` — constants
    - `^\s*(pub(\([^)]*\))?\s+)?static\s+\w+` — statics
    - `^\s*#\[cfg\(test\)\]` — test module markers
  - [x] **Markdown patterns** (for `.md` files):
    - `^#{1,6}\s+.+` — headings (level 1-6)
  - [x] **Generic fallback patterns** (all other file types):
    - `^\s*(pub(lic)?|private|protected)?\s*(static\s+)?(async\s+)?(fn|func|function|def|class|interface|struct|enum|mod|module|trait|type|const|let|var)\s+\w+`
  - [x] Format: `symbol_signature [L{line_number}]` — one per line
  - [x] Include a header: `File outline for {path} ({total_lines} lines):`
  - [x] If no symbols are found, return: `No structural symbols found in {path} ({total_lines} lines).\nUse start_line and end_line to read specific sections.`
  - [x] Otherwise add footer: `Use start_line and end_line to read specific sections.`

### Task 3: Implement `Tool` Trait for `ReadFileTool`

- [x] `const NAME: &'static str = "read_file";` (AC: #5)
- [x] `type Error = ReadFileToolError;`
- [x] `type Args = ReadFileToolArgs;`
- [x] `type Output = String;`
- [x] Implement `async fn definition(&self, _prompt: String) -> ToolDefinition` (AC: #5)
  - [x] Name: `"read_file"`
  - [x] Description must be **detailed and LLM-optimized**: explain full read vs partial read vs outline mode, when to use line ranges, how to interpret outline output, examples
  - [x] JSON schema with `path` (required string), `start_line` (optional integer, minimum 1), `end_line` (optional integer, minimum 1) — use `"type": "integer"` (not `"number"` or `"u32"`, JSON Schema has no unsigned types). Include clear descriptions for each parameter.
- [x] Implement `async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error>` (AC: #1, #2, #3, #4)
  - [x] Log with `tracing::info!(action = "read_file", path = %args.path, ...)` before action
  - [x] Validate path via `validate_path()`
  - [x] Check path is a file, not a directory → return `IsDirectory` error
  - [x] Delegate to `read_full_or_range()`
  - [x] Log result summary (bytes returned, mode used) via tracing

### Task 4: Update Module Registry (`src/tools/mod.rs`)

- [x] Add `pub mod read_file;` to `src/tools/mod.rs`
- [x] Add `pub use read_file::ReadFileTool;` re-export
- [x] Update module doc comment to mention ReadFileTool
- [x] **Do NOT remove FsTool yet** — that happens in Story 8.4

### Task 5: Unit Tests (`#[cfg(test)] mod tests` in `read_file.rs`)

- [x] `test_read_file_tool_definition_name` — verify `NAME == "read_file"` and definition name matches
- [x] `test_read_file_tool_definition_has_detailed_description` — verify description is non-empty and contains key usage instructions
- [x] `test_read_file_tool_definition_parameters` — verify JSON schema has `path` as required, `start_line` and `end_line` as optional
- [x] `test_read_file_tool_args_deserialize_minimal` — only `path` field
- [x] `test_read_file_tool_args_deserialize_full` — all three fields
- [x] `test_read_file_tool_error_is_send_sync` — verify `ReadFileToolError: Send + Sync`
- [x] `test_read_file_tool_error_display` — verify all error variant display strings
- [x] `test_read_file_tool_serializable` — serialize/deserialize `ReadFileTool` struct
- [x] `test_read_file_full_small_file` (AC: #1) — file ≤ 300 lines → returns full content with line numbers
- [x] `test_read_file_line_numbers_format` (AC: #1) — verify line numbers are 1-indexed and properly formatted
- [x] `test_read_file_partial_start_and_end` (AC: #2) — read lines 5-10 of a 20-line file
- [x] `test_read_file_partial_start_only` (AC: #2) — `start_line=5`, no end → reads to end of file
- [x] `test_read_file_partial_end_only` (AC: #2) — no start, `end_line=10` → reads from beginning to line 10
- [x] `test_read_file_partial_clamp_overflow` (AC: #2) — `end_line=9999` on a 20-line file → clamps to 20, no error
- [x] `test_read_file_partial_start_beyond_file` (AC: #2) — `start_line=9999` on a 20-line file → returns empty or appropriate message
- [x] `test_read_file_partial_single_line` (AC: #2) — `start_line=5, end_line=5` → returns exactly line 5
- [x] `test_read_file_outline_large_file` (AC: #3) — file > 300 lines, no range → returns outline, NOT full content
- [x] `test_read_file_outline_contains_functions` (AC: #3) — outline includes `fn` declarations with line numbers
- [x] `test_read_file_outline_contains_structs_enums` (AC: #3) — outline includes `struct` and `enum` declarations
- [x] `test_read_file_outline_contains_impl_blocks` (AC: #3) — outline includes `impl` blocks
- [x] `test_read_file_outline_contains_mods` (AC: #3) — outline includes `mod` declarations
- [x] `test_read_file_large_file_with_range_returns_content` (AC: #2, #3) — file > 300 lines BUT range specified → returns content, NOT outline
- [x] `test_read_file_exactly_300_lines` — file with exactly 300 lines → full content (≤ 300 threshold)
- [x] `test_read_file_301_lines_triggers_outline` — file with 301 lines → outline mode
- [x] `test_read_file_partial_start_zero_clamps_to_one` (AC: #2) — `start_line=0` on a 20-line file → clamps to 1, returns from line 1 (no error)
- [x] `test_read_file_partial_start_after_end` (AC: #2) — `start_line=10, end_line=5` → returns empty result (not an error)
- [x] `test_read_file_not_found` (AC: #4) — non-existent file → `NotFound` error
- [x] `test_read_file_path_denied_outside_root` (AC: #4) — `../../etc/passwd` → `PathDenied` error
- [x] `test_read_file_empty_file` — empty file → returns empty content (no error)
- [x] `test_read_file_binary_file_returns_clear_error` — file with non-UTF-8 bytes (e.g., `&[0xFF, 0xFE, 0x00]`) → `ReadFailed` error with "binary or non-UTF-8" in message
- [x] `test_read_file_is_directory` — path pointing to a directory → `IsDirectory` error
- [x] `test_read_file_nested_path` — reading from subdirectory within project root
- [x] `test_read_file_outline_markdown_headings` — `.md` file >300 lines with `# Heading` lines → outline contains headings with line numbers
- [x] `test_read_file_outline_no_symbols_fallback` — plain `.txt` file >300 lines with no recognizable symbols → returns "No structural symbols found" message with line count

### Task 6: Integration Verification

- [x] Run `cargo test` — all 34 new tests pass, zero regressions (713 total pass)
- [x] Run `cargo clippy` — only pre-existing warnings; one expected `unused import` for `ReadFileTool` re-export (consumed in Story 8.5)
- [x] Run `cargo fmt --check` — no formatting issues
- [x] Verify `ReadFileTool` is accessible from `crate::tools::ReadFileTool`

## Dev Notes

### Previous Story Intelligence & Established Patterns

**From Story 4.1 (Rig Tools Implementation — baseline for all tools):**
- The rig Tool pattern is firmly established: `Serialize + Deserialize` struct, dedicated `Args` (Deserialize), dedicated `Error` enum (thiserror), `impl Tool` with `NAME`, `Error`, `Args`, `Output` types
- `call()` always logs with `tracing::info!(action = "...")` structured fields
- All errors are typed `thiserror` enums — no `anyhow` in modules
- Tests follow `test_{tool}_{behavior}_{scenario}` naming, Arrange → Act → Assert
- Tool structs hold ONLY configuration data (`PathBuf`). Never store open resources. Resources opened fresh on each `call()`
- Tools do NOT retry internally — all errors bubble up to the rig agent
- All file I/O uses `tokio::fs` (async)
- `tempfile::TempDir` for test fixtures
- JSON schema uses clear descriptions for each parameter
- Security boundary: `canonicalize()` + `starts_with()` check against project root

**From `src/supervisor/read_tool.rs` (existing `ReadFile` supervisor tool):**
- This is a **separate tool** in the supervisor module — do NOT modify or merge with it
- It is intentionally simpler (no line ranges, no outline mode) — the supervisor only needs basic file reading
- However, the security boundary pattern (`validate_path`) is identical and should be replicated
- Story 8.4 will later update `read_tool.rs` to use `ReadFileTool` internally — but that's NOT this story's scope

**From `src/tools/fs.rs` (current FsTool — to be replaced later):**
- `FsTool::handle_read()` at lines 163-175: simple `tokio::fs::read_to_string` with path validation — this is what `ReadFileTool` replaces with enhanced capabilities
- `FsTool::validate_path()` at lines 96-121: the canonical security boundary pattern — replicate this exactly
- `FsTool::validate_path_for_new()` at lines 127-160: NOT needed for ReadFileTool (read-only tool)
- The `FsTool` will NOT be removed in this story — that happens in Story 8.4

**From git log (last 10 commits):**
- `fa26a22` — feat: add Epic 8 (Surgical Development Tooling) — PRD, epics, sprint status
- `7161267` — docs(architecture): add Decision 7 — Surgical Development Tooling
- `e24d39b` — feat(session): add rig ThinkTool to agent builders
- All recent commits are on `main`, no feature branches active for Epic 8 yet

### Architecture Decision 7 — ReadFileTool Design Spec

**From `architecture.md` Decision 7 (authoritative design):**

```
ReadFileArgs {
    path: String,                  // Relative path from project root
    start_line: Option<u32>,       // 1-indexed, inclusive
    end_line: Option<u32>,         // 1-indexed, inclusive
}
```

**Behavior rules:**
- File **≤ 300 lines** + no line range → return full content with line numbers
- File **> 300 lines** + no line range → return **outline mode** (symbol extraction with line numbers)
- Any file + line range specified → return requested range with line numbers (regardless of file size)
- Line numbers are ALWAYS included in output (both full content and outline mode)
- Out-of-range values clamped to file boundaries without error
- `start_line` of `0` clamped to `1` (the spec is 1-indexed, but LLMs may send 0)
- `start_line > end_line` after clamping → return empty result (not an error)
- Non-UTF-8 files (binary) → return clear `ReadFailed` error with "binary or non-UTF-8" message

**Outline extraction — regex heuristics, NOT AST parsing:**

For Rust files, these patterns capture 90%+ of navigable symbols. **Critical:** use `pub(\([^)]*\))?` instead of just `pub` to handle `pub(crate)`, `pub(super)`, and `pub(in path)` visibility modifiers:
- `^\s*(pub(\([^)]*\))?\s+)?(async\s+)?fn\s+` — functions
- `^\s*(pub(\([^)]*\))?\s+)?struct\s+` — structs
- `^\s*(pub(\([^)]*\))?\s+)?enum\s+` — enums
- `^\s*impl(<[^>]*>)?\s+` — impl blocks (handles generics like `impl<T>`)
- `^\s*(pub(\([^)]*\))?\s+)?mod\s+` — modules
- `^\s*(pub(\([^)]*\))?\s+)?trait\s+` — traits
- `^\s*#\[cfg\(test\)\]` — test module markers

The architecture spec says "language-aware regex heuristics" — support Rust primarily (this is a Rust project) with generic fallback for other file types. Use file extension to select the pattern set (`.rs` → Rust, `.md` → Markdown headings, other → generic multi-language fallback).

**Static regex compilation:** Use `std::sync::LazyLock<Regex>` at module level for all patterns. The project uses edition 2024 (rustc 1.86+) so `LazyLock` is stable. This compiles each regex exactly once per process lifetime — no struct state, no per-call overhead.

### Tool Definition Description — Critical for LLM Usage

The `definition()` description MUST be comprehensive enough for the LLM to understand:
1. When to use `read_file` vs other tools
2. How outline mode works and when it triggers
3. That line ranges bypass outline mode (even on large files)
4. The workflow: outline → identify section → read with line range
5. That line numbers are always present in output

**Suggested description template:**
> Read a file from the project. Returns file content with line numbers.
>
> **Modes:**
> - **Full read:** Files ≤ 300 lines return complete content with line numbers.
> - **Outline mode:** Files > 300 lines return a structural outline (function/struct/enum/impl/mod declarations with line numbers) instead of full content. Use the line numbers from the outline to read specific sections with start_line/end_line.
> - **Partial read:** Specify start_line and/or end_line (1-indexed, inclusive) to read a specific range. This works on files of any size and always returns content (never outline).
>
> **Workflow for large files:** Call without line range → get outline → identify the section you need → call again with start_line/end_line.

### Line Number Formatting

Line numbers should be right-aligned with consistent padding based on the total line count of the file:
- Files with <10 lines: 1-digit padding (e.g., `1 | content`)
- Files with <100 lines: 2-digit padding (e.g., ` 1 | content`)
- Files with <1000 lines: 3-digit padding (e.g., `  1 | content`)
- And so on

Format: `{line_number:>width} | {line_content}`

This matches common editor conventions and makes the output easy for the LLM to parse.

### Outline Mode — Symbol Extraction Details

The outline should look like this for a Rust file:

```
File outline for src/tools/fs.rs (912 lines):

pub struct FsTool [L19]
pub struct FsToolArgs [L26]
pub enum FsToolError [L39]
impl FsTool [L86]
  pub fn new [L88]
  fn validate_path [L96]
  fn validate_path_for_new [L127]
  async fn handle_read [L163]
  async fn handle_write [L178]
impl Tool for FsTool [L417]
  async fn definition [L423]
  async fn call [L460]
mod tests [L498]

Use start_line and end_line to read specific sections.
```

Key details:
- Top-level symbols are not indented
- Methods within `impl` blocks are indented with 2 spaces
- Line numbers are shown as `[L{n}]`
- Include visibility modifiers (`pub`, `pub(crate)`) and `async` qualifiers for context
- For `impl` blocks, show the full signature (e.g., `impl Tool for FsTool`)
- **Nesting algorithm (indentation-based):** Track the leading whitespace (column) of the last `impl`/`struct`/`enum`/`mod` line seen. If a subsequent `fn`/`const`/`type` match has strictly more leading whitespace than that last top-level match, indent it with 2 spaces. When a new top-level match is found (same or less indentation), reset the tracking. This is a simple, robust heuristic that uses Rust's own source indentation as a proxy for nesting depth — no brace counting or AST needed.
- If outline extraction finds zero symbols (e.g., plain text, CSV, JSON files), return: `"No structural symbols found in {path} ({N} lines).\nUse start_line and end_line to read specific sections."` — this prevents the LLM from receiving an empty response and getting stuck.

### Dependencies — No New Crates Required

All needed crates are already in `Cargo.toml`:
- `rig-core = "0.30"` — Tool trait
- `serde = { version = "1", features = ["derive"] }` — Serialize/Deserialize
- `serde_json = "1"` — JSON schema in tool definition
- `thiserror = "2"` — Error enum
- `tracing = "0.1"` — Structured logging
- `tokio = { version = "1", features = ["full"] }` — Async file I/O
- `regex = "1"` — Symbol extraction patterns
- `tempfile = "3"` (dev) — Test fixtures

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code — only in tests
- ❌ **NO** `anyhow::Result` — typed `thiserror` enums only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** panicking in `call()` — always return `Result`
- ❌ **NO** storing compiled regex in the tool struct — use `static LazyLock<Regex>` at module level instead (compiles once per process, no struct state, idiomatic for edition 2024)
- ❌ **NO** full AST parsing for outline mode — regex heuristics only (per architecture Decision 7)
- ❌ **NO** modifying `src/tools/fs.rs` — FsTool remains untouched until Story 8.4
- ❌ **NO** modifying `src/supervisor/read_tool.rs` — supervisor tool update is Story 8.4
- ❌ **NO** modifying `src/session/runner.rs` — tool registration update is Story 8.5
- ❌ **NO** removing `FsTool` from `src/tools/mod.rs` — that's Story 8.4
- ❌ **NO** blocking file I/O — use `tokio::fs` for all reads
- ❌ **NO** reading binary files as UTF-8 and crashing — detect and return a clear error message (e.g., "Binary file detected, cannot display as text")
- ❌ **NO** internal retry logic — errors bubble to the rig agent
- ❌ **NO** `action: String` multiplexer field — the tool has a single responsibility (reading)

### Scope Boundaries

**IN SCOPE for this story:**
- `src/tools/read_file.rs` — Full `ReadFileTool` implementation with outline mode
- `src/tools/mod.rs` — Add module declaration and re-export (alongside existing FsTool)
- Unit tests for all behaviors

**OUT OF SCOPE — do NOT implement:**
- EditFileTool (Story 8.2)
- GrepTool or FindPathTool (Story 8.3)
- ListDirectoryTool (Story 8.4)
- FsTool removal (Story 8.4)
- `supervisor/read_tool.rs` migration to use ReadFileTool (Story 8.4)
- `session/runner.rs` preamble or tool registration updates (Story 8.5)
- Agent builder changes (Story 8.5)

### Files Created/Modified in This Story

| File | Change |
|------|--------|
| `src/tools/read_file.rs` | **CREATE** — Full ReadFileTool implementation + unit tests |
| `src/tools/mod.rs` | **MODIFY** — Add `pub mod read_file;` + `pub use read_file::ReadFileTool;` + update doc comment |

### Testing Requirements

All tests follow established patterns: `test_{tool}_{behavior}_{scenario}`, Arrange → Act → Assert, `tempfile::TempDir` for test fixtures.

**Test helper suggestion:** Create a helper function `fn create_test_file(dir: &Path, name: &str, lines: usize) -> PathBuf` that generates a file with numbered lines like `"Line 1\nLine 2\n..."` for consistent test setup.

**For outline tests:** Create a Rust-like file with known symbols (functions, structs, enums, impls, mods) and verify the outline output contains each expected symbol with correct line numbers.

**Test coverage target:** ~33 tests covering all ACs, edge cases, and error paths.

### Project Structure Notes

After this story, the tools module gains `read_file.rs` alongside the existing tools:

```
src/tools/
├── mod.rs          # Module declarations + pub re-exports (GitTool, FsTool, TerminalTool, ReadFileTool)
├── read_file.rs    # ReadFileTool — partial reading + outline mode (NEW)
├── fs.rs           # FsTool — legacy 6-action filesystem tool (UNCHANGED — removed in Story 8.4)
├── git.rs          # GitTool — 9 git operations via git2 (UNCHANGED)
└── terminal.rs     # TerminalTool — shell execution with timeout (UNCHANGED)
```

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 8.1: ReadFileTool — Partial Reading & Outline Mode] — Acceptance criteria and user story
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 8: Surgical Development Tooling] — Epic context, dependency chain, impact metrics
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 7: Surgical Development Tooling] — Full ReadFileTool design spec (args, behavior, outline patterns, preamble rules)
- [Source: _bmad-output/planning-artifacts/architecture.md#Rig Tool Implementation Pattern — Standard Structure] — Mandatory tool structure pattern (struct + args + error + Tool impl)
- [Source: _bmad-output/planning-artifacts/architecture.md#Error Type Pattern — Per-Module thiserror Enums] — Per-module thiserror, no anyhow in library modules
- [Source: _bmad-output/planning-artifacts/architecture.md#Tracing Pattern — Structured Spans with Story Context] — Every tool action logged with `action` field
- [Source: _bmad-output/planning-artifacts/architecture.md#Project Structure & Boundaries] — tools/ module layout showing `read_file.rs`
- [Source: _bmad-output/planning-artifacts/architect-brief-surgical-tooling.md#Story 8.1] — Architect brief with scope and rationale
- [Source: _bmad-output/project-context.md#Framework-Specific Rules] — 9 tools exposed to the agent, tool design principle (focused tools over action multiplexing)
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — "Never rewrite entire files", tool design rules
- [Source: _bmad-output/project-context.md#Testing Rules] — Tests inline, `#[cfg(test)] mod tests`, every module must include unit tests
- [Source: _bmad-output/project-context.md#Code Quality & Style Rules] — rustfmt, clippy, doc comments mandatory on public items
- [Source: src/tools/fs.rs#L86-121] — `FsTool::validate_path()` — canonical security boundary pattern to replicate
- [Source: src/tools/fs.rs#L163-175] — `FsTool::handle_read()` — current read implementation being replaced
- [Source: src/tools/fs.rs#L417-495] — `impl Tool for FsTool` — reference for Tool trait implementation
- [Source: src/tools/mod.rs] — Current module registry (add ReadFileTool alongside existing exports)
- [Source: src/supervisor/read_tool.rs] — Supervisor's ReadFile tool (separate, do NOT modify — reference only)
- [Source: src/session/runner.rs#L1105-1123] — `create_tools()` — current tool creation (NOT modified in this story — Story 8.5)
- [Source: src/session/runner.rs#L1011-1026] — `build_preamble()` — current preamble (NOT modified in this story — Story 8.5)
- [Source: _bmad-output/implementation-artifacts/4-1-rig-tools-implementation-git-filesystem-terminal.md] — Story 4.1 dev notes — established patterns, anti-patterns, testing approach
- [Source: Cargo.toml] — All dependencies verified present, no additions needed

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (via Windsurf)

### Debug Log References

- `cargo test --bin bmad-bot tools::read_file` → 34 passed, 0 failed
- `cargo test` → 713 passed, 0 failed, 0 regressions
- `cargo clippy` → no new warnings (pre-existing warnings unchanged; expected `unused import: read_file::ReadFileTool` until Story 8.5 consumes the re-export)
- `cargo fmt --check` → clean

### Completion Notes List

- ✅ Created `src/tools/read_file.rs` (1097 lines) — full `ReadFileTool` implementation with 34 unit tests inline
- ✅ Three reading modes: full read (≤300 lines), outline mode (>300 lines, regex-based symbol extraction), partial read (line range)
- ✅ Security boundary: `canonicalize()` + `starts_with()` — identical pattern to `FsTool::validate_path`
- ✅ Binary/non-UTF-8 detection: reads raw bytes via `tokio::fs::read`, then `String::from_utf8` with clear error message
- ✅ Static regex compilation via `std::sync::LazyLock<Regex>` — 10 Rust patterns, 1 Markdown pattern, 1 generic fallback
- ✅ Outline nesting: indentation-based heuristic tracks last top-level symbol indent to nest inner `fn`/`const`/`type` with 2-space indent
- ✅ Line number formatting: right-aligned with dynamic width padding based on total line count
- ✅ `IsDirectory` error variant prevents directory reads
- ✅ Clamping: `start_line=0` → 1, `end_line > total` → clamped, `start > end` → empty (not error)
- ✅ Tool definition description is LLM-optimized with full mode explanations and workflow guidance
- ✅ `src/tools/mod.rs` updated: added `pub mod read_file;` + `pub use read_file::ReadFileTool;` + updated doc comment
- ✅ FsTool untouched (removal in Story 8.4), supervisor `read_tool.rs` untouched (migration in Story 8.4), `session/runner.rs` untouched (registration in Story 8.5)

### Change Log

- 2026-02-10: Story 8.1 implementation complete — ReadFileTool created with full test suite (34 tests), module registry updated

### File List

| File | Change |
|------|--------|
| `src/tools/read_file.rs` | **CREATED** — Full ReadFileTool implementation + 34 unit tests |
| `src/tools/mod.rs` | **MODIFIED** — Added `pub mod read_file;`, `pub use read_file::ReadFileTool;`, updated doc comment |