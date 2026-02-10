# Story 8.2: EditFileTool — Surgical Search-Replace Editing

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a dev agent,
I want to edit files surgically via search-replace operations instead of rewriting entire files,
so that I minimize token usage, eliminate truncation risk, and make precise targeted changes.

## Acceptance Criteria

1. **Surgical edit (search-replace):** Given an existing file within the project root, when `edit_file` is called with mode `edit` and a list of `EditOperation` pairs (`old_text` → `new_text`), and each `old_text` exists exactly once in the file, then all replacements are applied sequentially with offset recalculation, and the tool returns the affected line ranges for verification.

2. **Edit not found:** Given an existing file within the project root, when `edit_file` is called with mode `edit` and an `old_text` value does not exist in the file, then the tool returns a clear error message indicating the text was not found, and no changes are made to the file.

3. **Edit ambiguity (multiple matches):** Given an existing file within the project root, when `edit_file` is called with mode `edit` and an `old_text` value matches multiple locations in the file, then the tool returns a clear error message with the line numbers of all matches, and no changes are made to the file (ambiguity must be resolved by the caller providing more context in `old_text`).

4. **Create mode:** Given a path that does not exist, when `edit_file` is called with mode `create` and file content, then a new file is created with the provided content, and parent directories are automatically created if they don't exist. Given a path that already exists, when `edit_file` is called with mode `create`, then the tool returns a clear error (create mode must not overwrite existing files).

5. **Overwrite mode:** Given an existing file within the project root, when `edit_file` is called with mode `overwrite` and full content, then the entire file content is replaced with the provided content. Given a path that does not exist, when `edit_file` is called with mode `overwrite`, then the tool returns a clear error (overwrite mode requires the file to exist).

6. **Security boundary:** Given a path that resolves outside the project root, when `edit_file` is called in any mode, then the tool returns a clear security error and no file is created or modified.

7. **Rig Tool pattern compliance:** Given the tool is implemented, when inspecting the code structure, then it follows the standard rig Tool pattern (serializable struct + `EditFileToolArgs` + `EditFileToolError` thiserror enum + `impl Tool`), the tool NAME and definition description are detailed enough for the LLM to use correctly, and `tools/mod.rs` exports `EditFileTool`.

8. **Unit test coverage:** A full unit test suite covers: single edit, multiple sequential edits, create mode, overwrite mode, not-found error, ambiguity error with line numbers, security boundary rejection, parent directory creation, edge cases (empty old_text, empty new_text for deletion, edit at start/end of file, create over existing, overwrite non-existent).

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [ ] Verify `Cargo.toml` has all needed dependencies: `rig-core = "0.30"`, `serde`, `serde_json`, `thiserror = "2"`, `tracing`, `tokio` (full) — **all already present, no changes needed**
- [ ] Verify `tempfile = "3"` exists in `[dev-dependencies]` for tests — **already present**
- [ ] Read and understand `src/tools/fs.rs` for the existing `FsTool` write patterns (`handle_write`, `validate_path`, `validate_path_for_new`)
- [ ] Read and understand `src/tools/read_file.rs` (Story 8.1) for the established patterns in this epic (struct layout, error enum style, test conventions)

### Task 1: Create `src/tools/edit_file.rs` — Struct, Args, Error Enum, Supporting Types

- [ ] Create `src/tools/edit_file.rs` with module doc comment explaining purpose
- [ ] Define `EditFileTool` struct (AC: #7)
  - [ ] `#[derive(Debug, Serialize, Deserialize)]`
  - [ ] Single field: `project_root: PathBuf`
  - [ ] Doc comment: `/// EditFileTool — surgical search-replace edits, create new files, overwrite when justified.`
- [ ] Define `EditOperation` struct (AC: #1)
  - [ ] `#[derive(Debug, Deserialize)]`
  - [ ] `old_text: String` — exact text fragment to find in the file
  - [ ] `new_text: String` — replacement text
  - [ ] Doc comments on each field
- [ ] Define `EditFileToolArgs` struct (AC: #1, #4, #5)
  - [ ] `#[derive(Debug, Deserialize)]`
  - [ ] `path: String` — relative path from project root
  - [ ] `mode: String` — one of: `"edit"`, `"create"`, `"overwrite"`
  - [ ] `edits: Option<Vec<EditOperation>>` — for mode `"edit"`: list of search-replace operations
  - [ ] `content: Option<String>` — for mode `"create"` or `"overwrite"`: full file content
  - [ ] Doc comments on each field explaining which modes use which fields
- [ ] Define `EditFileToolError` enum (AC: #2, #3, #4, #5, #6, #7)
  - [ ] `#[derive(Debug, thiserror::Error)]`
  - [ ] `NotFound { path: String }` — file does not exist (for edit/overwrite modes)
  - [ ] `PathDenied { path: String, reason: String }` — path outside project root
  - [ ] `AlreadyExists { path: String }` — file exists (for create mode)
  - [ ] `TextNotFound { path: String, old_text_preview: String }` — old_text not found in file during edit. Display message MUST include hint: `"Use read_file to check the actual content."` (per architecture Decision 7)
  - [ ] `AmbiguousMatch { path: String, old_text_preview: String, match_lines: String }` — old_text found at multiple locations. Store `match_lines` as a **pre-formatted `String`** (e.g., `"12, 45, 78"`) instead of `Vec<usize>` — this avoids `Vec` Display formatting issues with `thiserror`. Display message MUST include guidance: `"Provide more surrounding context in old_text to uniquely identify the target."`
  - [ ] `InvalidMode { mode: String }` — unrecognized mode string
  - [ ] `MissingArgument { mode: String, argument: String }` — required field not provided for the mode
  - [ ] `WriteFailed { path: String, reason: String }` — I/O error during write
  - [ ] `ReadFailed { path: String, reason: String }` — I/O error during read (for edit mode)

### Task 2: Implement `EditFileTool` Core Methods

- [ ] Implement `EditFileTool::new(project_root: PathBuf) -> Self`
- [ ] Implement `fn validate_path_existing(&self, requested: &str) -> Result<PathBuf, EditFileToolError>` (AC: #6)
  - [ ] Same pattern as `FsTool::validate_path` / `ReadFileTool::validate_path`: canonicalize requested path, canonicalize project_root, check `starts_with`
  - [ ] Return `NotFound` if canonicalize fails (file doesn't exist)
  - [ ] Return `PathDenied` if path resolves outside project root
- [ ] Implement `fn validate_path_for_new(&self, requested: &str) -> Result<PathBuf, EditFileToolError>` (AC: #4, #6)
  - [ ] Same pattern as `FsTool::validate_path_for_new`: canonicalize **parent** directory (which must exist), verify within project root, join filename
  - [ ] Return `PathDenied` if parent resolves outside project root
  - [ ] Verify the path has a valid `file_name()` component — reject paths ending in `/` or with no filename
  - [ ] Used by create mode after parent directories are created
- [ ] Implement `fn line_number_at_offset(content: &str, byte_offset: usize) -> usize`
  - [ ] Count the number of `\n` characters in `content[..byte_offset]` and add 1 (1-indexed)
  - [ ] Used by both `AmbiguousMatch` error construction and affected line range calculation — shared helper avoids duplication
- [ ] Implement `fn truncate_preview(text: &str, max_len: usize) -> String`
  - [ ] If `text.len() <= max_len` → return text as-is
  - [ ] Otherwise → return `text[..max_len]` + `"..."` (truncate at char boundary via `text.char_indices()`)
  - [ ] Used for `old_text_preview` in `TextNotFound` errors (max 80 chars)
- [ ] Implement `async fn handle_edit(&self, path: &Path, requested: &str, edits: &[EditOperation]) -> Result<String, EditFileToolError>` (AC: #1, #2, #3)
  - [ ] Read current file content via `tokio::fs::read_to_string`
  - [ ] For each `EditOperation` in order:
    - [ ] If `old_text` is empty → return `TextNotFound` error immediately with message `"old_text is empty — provide the exact text fragment to replace"`
    - [ ] Use **`content.match_indices(&old_text)`** to find ALL occurrence byte offsets — this returns `Iterator<Item = (usize, &str)>`. Collect into a `Vec`.
    - [ ] If zero matches → return `TextNotFound` error with a preview of old_text (first 80 chars, append `"..."` if truncated). **No changes written to disk** — the file remains unmodified.
    - [ ] If multiple matches → convert each byte offset to a 1-indexed line number via `fn line_number_at_offset(content: &str, byte_offset: usize) -> usize` helper. Return `AmbiguousMatch` error with line numbers. **No changes written to disk.**
    - [ ] If exactly one match → replace `old_text` with `new_text` in the in-memory content using the byte offset. Record the affected line range (start_line..end_line after replacement) using the same `line_number_at_offset` helper.
  - [ ] **Binary/non-UTF-8 detection:** If `tokio::fs::read_to_string` fails with an invalid UTF-8 error, return `ReadFailed` with message `"File appears to be binary or non-UTF-8 encoded"` — same pattern as Story 8.1.
  - [ ] **Atomic write strategy:** All edits are validated and applied in memory first. Only after ALL edits succeed is the result written to disk via `tokio::fs::write`. If any edit fails, the file on disk is untouched.
  - [ ] Return a summary of affected line ranges, e.g., `"Applied 3 edit(s) to {path}:\n  Edit 1: lines 5-8\n  Edit 2: lines 22-22\n  Edit 3: lines 45-50"`
- [ ] Implement `async fn handle_create(&self, requested: &str, content: &str) -> Result<String, EditFileToolError>` (AC: #4)
  - [ ] Verify `requested` has a valid filename component (not ending in `/`, not empty) — return `MissingArgument` if invalid
  - [ ] Check if file already exists → return `AlreadyExists` error
  - [ ] Create parent directories via `tokio::fs::create_dir_all` if needed (with ancestor validation within project root — same pattern as `FsTool::handle_write` at L178-244)
  - [ ] Validate path for new file via `validate_path_for_new`
  - [ ] Write content via `tokio::fs::write`
  - [ ] Return `"Created {path} ({N} bytes)"`
- [ ] Implement `async fn handle_overwrite(&self, path: &Path, requested: &str, content: &str) -> Result<String, EditFileToolError>` (AC: #5)
  - [ ] File must already exist (validated by caller via `validate_path_existing`)
  - [ ] Write full content via `tokio::fs::write`
  - [ ] Return `"Overwritten {path} ({N} bytes)"`

### Task 3: Implement `Tool` Trait for `EditFileTool`

- [ ] `const NAME: &'static str = "edit_file";` (AC: #7)
- [ ] `type Error = EditFileToolError;`
- [ ] `type Args = EditFileToolArgs;`
- [ ] `type Output = String;`
- [ ] Implement `async fn definition(&self, _prompt: String) -> ToolDefinition` (AC: #7)
  - [ ] Name: `"edit_file"`
  - [ ] Description must be **detailed and LLM-optimized**: explain the three modes, when to use each, how edit operations work, error recovery workflow (ambiguity → read_file with line range → retry with more context)
  - [ ] JSON schema with:
    - `path` (required string)
    - `mode` (required string, enum: `["edit", "create", "overwrite"]`)
    - `edits` (optional array of objects with `old_text` and `new_text` strings) — for edit mode
    - `content` (optional string) — for create/overwrite modes
  - [ ] Include clear descriptions for each parameter and when each is required
- [ ] Implement `async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error>` (AC: #1-#6)
  - [ ] Log with `tracing::info!(action = "edit_file", path = %args.path, mode = %args.mode, ...)` before action
  - [ ] Match on `args.mode`:
    - `"edit"` → validate edits present (`MissingArgument` if None/empty), validate path existing, call `handle_edit`
    - `"create"` → validate content present (`MissingArgument` if None), call `handle_create`
    - `"overwrite"` → validate content present (`MissingArgument` if None), validate path existing, call `handle_overwrite`
    - other → return `InvalidMode` error
  - [ ] Log result summary via tracing

### Task 4: Update Module Registry (`src/tools/mod.rs`)

- [ ] Add `pub mod edit_file;` to `src/tools/mod.rs`
- [ ] Add `pub use edit_file::EditFileTool;` re-export
- [ ] Update module doc comment to mention EditFileTool
- [ ] **Do NOT remove FsTool yet** — that happens in Story 8.4

### Task 5: Unit Tests (`#[cfg(test)] mod tests` in `edit_file.rs`)

- [ ] `test_edit_file_tool_definition_name` — verify `NAME == "edit_file"` and definition name matches
- [ ] `test_edit_file_tool_definition_has_detailed_description` — verify description is non-empty and contains key usage instructions for all three modes
- [ ] `test_edit_file_tool_definition_parameters` — verify JSON schema has `path` and `mode` as required, `edits` and `content` as optional, mode has `"enum": ["edit", "create", "overwrite"]` constraint
- [ ] `test_edit_file_tool_args_deserialize_edit_mode` — deserialize with `edits` field
- [ ] `test_edit_file_tool_args_deserialize_create_mode` — deserialize with `content` field
- [ ] `test_edit_file_tool_error_is_send_sync` — verify `EditFileToolError: Send + Sync`
- [ ] `test_edit_file_tool_error_display` — verify all error variant display strings
- [ ] `test_edit_file_tool_serializable` — serialize/deserialize `EditFileTool` struct
- [ ] `test_edit_file_single_edit` (AC: #1) — one old_text→new_text replacement, verify file changed on disk, verify returned line range
- [ ] `test_edit_file_multiple_sequential_edits` (AC: #1) — 3 edits in one call, verify all applied in order with offset recalculation, verify returned line ranges for each
- [ ] `test_edit_file_offset_recalculation` (AC: #1) — first edit changes line count (insert/delete lines), second edit targets text after the change, verify correct application
- [ ] `test_edit_file_text_not_found` (AC: #2) — old_text doesn't exist → `TextNotFound` error, file unchanged on disk
- [ ] `test_edit_file_ambiguous_match` (AC: #3) — old_text appears 3 times → `AmbiguousMatch` error with 3 line numbers, file unchanged on disk
- [ ] `test_edit_file_ambiguous_match_line_numbers_correct` (AC: #3) — verify the reported line numbers actually correspond to the match positions
- [ ] `test_edit_file_partial_failure_no_disk_write` (AC: #1, #2) — first edit succeeds, second edit fails (not found) → file on disk is completely unchanged (atomic: all-or-nothing)
- [ ] `test_edit_file_create_new_file` (AC: #4) — create mode with non-existent path → file created with correct content
- [ ] `test_edit_file_create_with_parent_dirs` (AC: #4) — create mode with nested path `a/b/c/new.rs` → parent dirs created, file created
- [ ] `test_edit_file_create_already_exists` (AC: #4) — create mode on existing file → `AlreadyExists` error, original file unchanged
- [ ] `test_edit_file_overwrite_existing` (AC: #5) — overwrite mode on existing file → content fully replaced
- [ ] `test_edit_file_overwrite_not_found` (AC: #5) — overwrite mode on non-existent file → `NotFound` error
- [ ] `test_edit_file_path_denied_outside_root` (AC: #6) — `../../etc/passwd` → `PathDenied` error for all three modes
- [ ] `test_edit_file_invalid_mode` (AC: #7) — mode `"delete"` → `InvalidMode` error
- [ ] `test_edit_file_edit_missing_edits` — edit mode with no `edits` field → `MissingArgument` error
- [ ] `test_edit_file_create_missing_content` — create mode with no `content` field → `MissingArgument` error
- [ ] `test_edit_file_overwrite_missing_content` — overwrite mode with no `content` field → `MissingArgument` error
- [ ] `test_edit_file_empty_old_text` — edit with `old_text: ""` → should match at position 0 (or return error — specify behavior: treat as error since empty match is ambiguous everywhere)
- [ ] `test_edit_file_empty_new_text_deletes` — edit with `new_text: ""` → effectively deletes the old_text fragment from the file
- [ ] `test_edit_file_edit_at_file_start` — old_text is the very first characters of the file → correctly replaced
- [ ] `test_edit_file_edit_at_file_end` — old_text is the very last characters of the file → correctly replaced
- [ ] `test_edit_file_edit_entire_line` — old_text is a complete line including newline → replaced correctly
- [ ] `test_edit_file_multiline_old_text` — old_text spans multiple lines → correctly found and replaced
- [ ] `test_edit_file_create_path_denied` (AC: #6) — create mode with path outside root → `PathDenied` error
- [ ] `test_edit_file_nested_path_edit` — editing a file in a subdirectory within project root
- [ ] `test_edit_file_binary_file_read_fails_clearly` — edit mode on a file with non-UTF-8 bytes → `ReadFailed` error with "binary or non-UTF-8" in message
- [ ] `test_edit_file_overwrite_with_empty_content` — overwrite with `content: ""` → file becomes 0 bytes (valid, not an error)
- [ ] `test_edit_file_create_empty_content` — create with `content: ""` → empty file created (valid)
- [ ] `test_edit_file_edit_empty_edits_vec` — `edits: Some(vec![])` → `MissingArgument` error (empty vec treated same as None)
- [ ] `test_edit_file_create_trailing_slash_path` — create with `path: "src/tools/"` (trailing slash) → error (no valid filename)

### Task 6: Integration Verification

- [ ] Run `cargo test` — all new tests pass, zero regressions on existing tests
- [ ] Run `cargo clippy` — zero warnings
- [ ] Run `cargo fmt --check` — no formatting issues
- [ ] Verify `EditFileTool` is accessible from `crate::tools::EditFileTool`

## Dev Notes

### Previous Story Intelligence — Story 8.1 (ReadFileTool)

**Patterns established in Story 8.1 that MUST be followed:**
- Tool struct: `#[derive(Debug, Serialize, Deserialize)]` with single `project_root: PathBuf` field
- Args struct: `#[derive(Debug, Deserialize)]` with doc comments on each field
- Error enum: `#[derive(Debug, thiserror::Error)]` with descriptive variants, all fields named
- `validate_path`: `canonicalize()` + `starts_with()` security check — replicate exactly
- `call()` logs with `tracing::info!(action = "edit_file", ...)` before and after
- JSON schema in `definition()` uses `"type": "integer"` for integers, `"enum": [...]` for constrained strings
- Tests follow `test_{tool}_{behavior}_{scenario}` naming, Arrange → Act → Assert
- All async file I/O via `tokio::fs`
- `static LazyLock<Regex>` at module level for any regex patterns (edition 2024 stable)
- `tempfile::TempDir` for all test fixtures
- Tools do NOT retry internally — all errors bubble to the rig agent

**From Story 8.1 anti-patterns (carry forward):**
- ❌ NO `unwrap()`/`expect()` in production
- ❌ NO `anyhow` — thiserror only
- ❌ NO `println!` — tracing only
- ❌ NO panic in `call()`
- ❌ NO blocking I/O — tokio::fs only
- ❌ NO modifying `fs.rs`, `read_tool.rs`, or `runner.rs`

### Architecture Decision 7 — EditFileTool Design Spec

**From `architecture.md` Decision 7 (authoritative design):**

```
EditFileArgs {
    path: String,           // Relative path from project root
    mode: String,           // "edit", "create", "overwrite"
    edits: Option<Vec<EditOperation>>,  // For mode="edit"
    content: Option<String>,            // For mode="create" or "overwrite"
}

EditOperation {
    old_text: String,       // Exact text fragment to find in the file
    new_text: String,       // Replacement text
}
```

**Validation rules (from architecture spec):**
- `old_text` must exist in the file and be **unique** (exactly one match)
- Zero matches → error with "not found" **+ hint to use `read_file`** (the `TextNotFound` Display string must include this guidance)
- Multiple matches → error with **line numbers of all occurrences** + guidance to "provide more surrounding context in old_text"
- `create` mode fails if the file already exists (forces the agent to use `edit` for existing files)
- `overwrite` mode requires the file to already exist
- Multiple `EditOperation` items are applied sequentially within a single call — **offsets are recalculated after each edit**
- Return value includes the **line range affected by each edit** for verification

### Offset Recalculation — Critical Implementation Detail

When multiple edits are applied sequentially, earlier edits change the content length and shift positions of subsequent text. The algorithm:

1. Read the full file content into a `String` (in-memory working copy)
2. For each `EditOperation` in order:
   a. Search for `old_text` in the **current** working copy (not the original)
   b. Validate uniqueness (0 matches → error, 2+ matches → error with line numbers)
   c. Record the byte offset of the match
   d. Replace `old_text` with `new_text` in the working copy
   e. Compute the affected line range in the **post-replacement** content
3. After ALL edits succeed in memory, write the final working copy to disk
4. If ANY edit fails, return the error — the file on disk is untouched

This means:
- Edit 2 searches in the content that already has Edit 1 applied
- Edit 3 searches in the content that has Edits 1+2 applied
- This is a simple sequential string replacement — no complex offset tracking data structures needed
- The "offset recalculation" happens naturally because each edit operates on the already-modified string

**Key API:** Use `content.match_indices(&old_text).collect::<Vec<_>>()` to find all occurrences and their byte offsets. Then `line_number_at_offset(content, byte_offset)` converts byte offsets to 1-indexed line numbers. This same helper is used for both `AmbiguousMatch` error line numbers and for computing the affected line ranges in successful edit return values.

### Atomic Write Strategy — All-or-Nothing

**CRITICAL:** The file on disk must NEVER be in a partially-edited state. The implementation must:
1. Read the original file content into memory
2. Apply all edits to the in-memory copy
3. Only if ALL edits succeed → write the final result to disk
4. If any edit fails → return error, file on disk is unchanged

This is NOT a filesystem-level atomic write (no temp file + rename). It's a logical atomicity: the write only happens after all validations pass. This is sufficient because:
- The tool operates on a single file at a time
- The daemon runs one story session at a time (sequential execution)
- The agent can retry the entire `edit_file` call if it fails

### Tool Definition Description — Critical for LLM Usage

The `definition()` description MUST teach the LLM:
1. **When to use each mode**: edit (modify existing), create (new file), overwrite (rare, full rewrite)
2. **How edit operations work**: exact text match → replace, must be unique
3. **Error recovery workflow**: `TextNotFound` → use `read_file` to check content, retry with correct text. `AmbiguousMatch` → use `read_file` with reported line range, retry with more surrounding context in `old_text`
4. **Batching**: multiple edits in one call are applied sequentially — more efficient than multiple calls
5. **Key constraint**: `old_text` must match EXACTLY (whitespace, indentation, newlines all matter)

**Suggested description template:**

> Edit a file in the project. Three modes available:
>
> **edit mode** (preferred for existing files): Provide a list of search-replace operations. Each operation specifies `old_text` (exact text to find) and `new_text` (replacement). The old_text must match exactly once in the file — if not found or ambiguous, an error with line numbers is returned. Multiple operations are applied sequentially in one call. Use `read_file` first to see the exact content you want to change.
>
> **create mode** (new files only): Provide the full file content. Fails if the file already exists. Parent directories are created automatically.
>
> **overwrite mode** (use sparingly): Replaces the entire file content. The file must already exist. Only use when a complete rewrite is truly necessary.
>
> **Error recovery:** If edit fails with "not found", use `read_file` to check the actual file content. If edit fails with "ambiguous", the error includes line numbers — use `read_file` with those line ranges to get more context, then retry with a larger `old_text` that uniquely identifies the location.

### Parent Directory Creation — Security-Aware Pattern

For `create` mode, parent directories may not exist. The implementation must replicate the `FsTool::handle_write` pattern (lines 178-244 of `fs.rs`):

1. Compute `full_path = project_root.join(requested)`
2. If parent directory doesn't exist:
   a. Walk up the directory tree to find the first existing ancestor
   b. Canonicalize that ancestor and verify it's within the project root
   c. If validation passes → `tokio::fs::create_dir_all(parent)`
   d. If validation fails → `PathDenied` error
3. Then validate the new file path via `validate_path_for_new`
4. Write the content

This prevents creating directories outside the project root via path traversal.

### Line Number Computation for Error Messages and Return Values

**For `AmbiguousMatch` errors:** Count newlines before each match position to determine line numbers (1-indexed). The error message should look like:
> `"Text found at multiple locations in {path}: lines 12, 45, 78. Provide more surrounding context in old_text to uniquely identify the target."`

**For successful edit return values:** After each replacement, compute the affected line range in the post-replacement content. The return message should look like:
> `"Applied 2 edit(s) to src/main.rs:\n  Edit 1: lines 5-8\n  Edit 2: lines 22-22"`

Line number computation: count newlines from the start of the content to the match position → that's the start line. Count newlines within the `new_text` → end line = start line + newline count in new_text.

### Empty `old_text` Edge Case

If the agent sends `old_text: ""`, an empty string matches at every position in the file — this is always ambiguous. Treat it as an error: check for empty `old_text` BEFORE calling `match_indices` and return `TextNotFound` with a message like `"old_text is empty — provide the exact text fragment to replace"`. Do NOT call `str::match_indices("")` — it returns a match at every byte position.

### `old_text_preview` Truncation Rule

The `TextNotFound` and `AmbiguousMatch` errors store `old_text_preview: String`. If `old_text.len() > 80`, truncate to 80 characters at the nearest char boundary and append `"..."`. Use a `truncate_preview(text, 80)` helper function. This keeps error messages readable when the LLM provides large text fragments.

### `AmbiguousMatch` — Pre-formatted `match_lines` String

Store `match_lines` as a `String` (e.g., `"12, 45, 78"`) NOT `Vec<usize>`. This avoids `Vec` Display formatting issues with `thiserror`'s `#[error("...")]` macro. Format the line numbers during error construction: `lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(", ")`.

### Dependencies — No New Crates Required

All needed crates are already in `Cargo.toml`:
- `rig-core = "0.30"` — Tool trait
- `serde = { version = "1", features = ["derive"] }` — Serialize/Deserialize
- `serde_json = "1"` — JSON schema in tool definition
- `thiserror = "2"` — Error enum
- `tracing = "0.1"` — Structured logging
- `tokio = { version = "1", features = ["full"] }` — Async file I/O
- `tempfile = "3"` (dev) — Test fixtures

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code — only in tests
- ❌ **NO** `anyhow::Result` — typed `thiserror` enums only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** panicking in `call()` — always return `Result`
- ❌ **NO** modifying `src/tools/fs.rs` — FsTool remains untouched until Story 8.4
- ❌ **NO** modifying `src/tools/read_file.rs` — ReadFileTool is complete, do not change it
- ❌ **NO** modifying `src/supervisor/read_tool.rs` — supervisor tool update is Story 8.4
- ❌ **NO** modifying `src/session/runner.rs` — tool registration update is Story 8.5
- ❌ **NO** removing `FsTool` from `src/tools/mod.rs` — that's Story 8.4
- ❌ **NO** blocking file I/O — use `tokio::fs` for all reads and writes
- ❌ **NO** partial writes to disk — all edits must succeed in memory before any disk write
- ❌ **NO** internal retry logic — errors bubble to the rig agent
- ❌ **NO** `action: String` multiplexer field — the tool has three modes via the `mode` field, not an action multiplexer. Each mode has distinct behavior, but it's still a single tool concern: file editing
- ❌ **NO** using `ReadFileTool` internally — EditFileTool reads files directly via `tokio::fs::read_to_string`. While the architecture notes "EditFileTool may use ReadFileTool internally for validation", this is unnecessary for MVP: direct async read is simpler, avoids a cross-tool dependency, and `ReadFileTool` adds outline/line-number formatting overhead that EditFileTool doesn't need
- ❌ **NO** regex-based matching for `old_text` — use exact string matching via `str::match_indices()`. The LLM provides exact text fragments, not patterns. Do NOT use `str::find` (only finds first match) or `str::matches` (no positions) — use `str::match_indices` to get all positions
- ❌ **NO** attempting to match empty `old_text` — check for empty before calling `match_indices`, return `TextNotFound` error immediately
- ❌ **NO** `Vec<usize>` in thiserror Display — pre-format line numbers as `String` before storing in error variants

### Scope Boundaries

**IN SCOPE for this story:**
- `src/tools/edit_file.rs` — Full `EditFileTool` implementation with all three modes
- `src/tools/mod.rs` — Add module declaration and re-export (alongside existing FsTool, ReadFileTool)
- Unit tests for all behaviors

**OUT OF SCOPE — do NOT implement:**
- GrepTool or FindPathTool (Story 8.3)
- ListDirectoryTool (Story 8.4)
- FsTool removal (Story 8.4)
- `supervisor/read_tool.rs` migration (Story 8.4)
- `session/runner.rs` preamble or tool registration updates (Story 8.5)
- Agent builder changes (Story 8.5)
- Any changes to `read_file.rs` (Story 8.1 — complete)

### Files Created/Modified in This Story

| File | Change |
|------|--------|
| `src/tools/edit_file.rs` | **CREATE** — Full EditFileTool implementation + unit tests |
| `src/tools/mod.rs` | **MODIFY** — Add `pub mod edit_file;` + `pub use edit_file::EditFileTool;` + update doc comment |

### Testing Requirements

All tests follow established patterns: `test_{tool}_{behavior}_{scenario}`, Arrange → Act → Assert, `tempfile::TempDir` for test fixtures.

**Test helper suggestion:** Create helpers:
- `fn create_test_file_with_content(dir: &Path, name: &str, content: &str) -> PathBuf` — creates a file with specific content
- `fn read_test_file(path: &Path) -> String` — reads file back for assertion (use `std::fs::read_to_string` in tests, not tokio)

**For edit mode tests:** Create files with known content, apply edits, then read the file back and assert the content is correctly modified. Always verify both the return message (line ranges) and the actual file content on disk.

**For atomic write tests:** Apply a batch where the second edit fails — then verify the file on disk still has the ORIGINAL content (not partially edited).

**Test coverage target:** ~40 tests covering all ACs, edge cases, and error paths.

**Performance note (informational):** `str::match_indices` is O(n*m) worst-case where n = content length, m = old_text length. For MVP this is fine — typical files are <10k lines and old_text is <100 chars. If performance becomes an issue in the future, the `memchr` crate offers faster substring search via SIMD-accelerated Boyer-Moore. Not needed now.

### Project Structure Notes

After this story, the tools module gains `edit_file.rs`:

```
src/tools/
├── mod.rs          # Module declarations + pub re-exports (GitTool, FsTool, TerminalTool, ReadFileTool, EditFileTool)
├── edit_file.rs    # EditFileTool — surgical search-replace, create, overwrite (NEW)
├── read_file.rs    # ReadFileTool — partial reading + outline mode (Story 8.1)
├── fs.rs           # FsTool — legacy 6-action filesystem tool (UNCHANGED — removed in Story 8.4)
├── git.rs          # GitTool — 9 git operations via git2 (UNCHANGED)
└── terminal.rs     # TerminalTool — shell execution with timeout (UNCHANGED)
```

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 8.2: EditFileTool — Surgical Search-Replace Editing] — Acceptance criteria and user story
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 8: Surgical Development Tooling] — Epic context, dependency chain, impact metrics
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 7: Surgical Development Tooling] — Full EditFileTool design spec (args, modes, validation rules, error behavior)
- [Source: _bmad-output/planning-artifacts/architecture.md#Rig Tool Implementation Pattern — Standard Structure] — Mandatory tool structure pattern (struct + args + error + Tool impl)
- [Source: _bmad-output/planning-artifacts/architecture.md#Error Type Pattern — Per-Module thiserror Enums] — Per-module thiserror, no anyhow in library modules
- [Source: _bmad-output/planning-artifacts/architecture.md#Tracing Pattern — Structured Spans with Story Context] — Every tool action logged with `action` field
- [Source: _bmad-output/planning-artifacts/architecture.md#Project Structure & Boundaries] — tools/ module layout showing `edit_file.rs`
- [Source: _bmad-output/planning-artifacts/architect-brief-surgical-tooling.md#Story 8.2] — Architect brief with scope and rationale
- [Source: _bmad-output/project-context.md#Framework-Specific Rules] — 9 tools exposed to the agent, tool design principle (focused tools over action multiplexing)
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — "Never rewrite entire files", tool design rules
- [Source: _bmad-output/project-context.md#Testing Rules] — Tests inline, `#[cfg(test)] mod tests`, every module must include unit tests
- [Source: _bmad-output/project-context.md#Code Quality & Style Rules] — rustfmt, clippy, doc comments mandatory on public items
- [Source: src/tools/fs.rs#L86-121] — `FsTool::validate_path()` — canonical security boundary pattern to replicate
- [Source: src/tools/fs.rs#L127-160] — `FsTool::validate_path_for_new()` — new-file path validation pattern to replicate
- [Source: src/tools/fs.rs#L178-244] — `FsTool::handle_write()` — parent directory creation with ancestor validation pattern to replicate
- [Source: src/tools/fs.rs#L417-495] — `impl Tool for FsTool` — reference for Tool trait implementation (mode dispatch, JSON schema with enum)
- [Source: src/tools/mod.rs] — Current module registry (add EditFileTool alongside existing exports)
- [Source: _bmad-output/implementation-artifacts/8-1-read-file-tool-partial-reading-outline-mode.md] — Story 8.1 dev notes — established patterns for this epic, anti-patterns, testing approach
- [Source: _bmad-output/implementation-artifacts/4-1-rig-tools-implementation-git-filesystem-terminal.md] — Story 4.1 dev notes — original tool patterns, FsTool design decisions
- [Source: Cargo.toml] — All dependencies verified present, no additions needed

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List