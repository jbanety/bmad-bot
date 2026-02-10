# Story 8.3: GrepTool & FindPathTool — Codebase Search & Navigation

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a dev agent,
I want to search file contents with regex and find files by glob pattern,
So that I can locate code and files efficiently instead of blindly listing and reading directories.

## Acceptance Criteria

1. **Given** a project codebase
   **When** `grep` is called with a regex pattern
   **Then** it returns matching lines with file paths, line numbers, and the matched content
   **And** results are paginated with a default of 20 matches per page

2. **Given** a project codebase
   **When** `grep` is called with a regex pattern and an `include_pattern` glob filter (e.g., `"**/*.rs"`)
   **Then** only files matching the glob are searched

3. **Given** a project codebase with a `.gitignore` file
   **When** `grep` is called
   **Then** files matching `.gitignore` patterns are excluded from search results

4. **Given** a project codebase
   **When** `grep` is called with a `context_lines` parameter
   **Then** the specified number of lines above and below each match are included in the output

5. **Given** a project codebase
   **When** `find_path` is called with a glob pattern (e.g., `"**/*.rs"`, `"src/**/mod.rs"`)
   **Then** it returns matching file paths sorted alphabetically
   **And** results are paginated with a default of 50 matches per page

6. **Given** a project codebase with a `.gitignore` file
   **When** `find_path` is called
   **Then** files matching `.gitignore` patterns are excluded from results

7. **Given** the tools are implemented
   **When** inspecting the code structure
   **Then** `tools/grep.rs` follows the standard rig Tool pattern (serializable struct + args + error enum + Tool trait impl)
   **And** `tools/find_path.rs` follows the standard rig Tool pattern
   **And** both use the `regex` crate (already a dependency) for pattern matching
   **And** file traversal uses the `ignore` crate for .gitignore-aware walking (add to `Cargo.toml`)
   **And** `tools/mod.rs` exports both `GrepTool` and `FindPathTool`
   **And** full unit test suites cover: basic search, glob filtering, gitignore respect, pagination, no-match cases, invalid regex handling

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [ ] Verify `src/tools/read_file.rs` exists and compiles (Story 8.1 delivered) (AC: all)
- [ ] Verify `src/tools/edit_file.rs` exists and compiles (Story 8.2 delivered) (AC: all)
- [ ] Verify `cargo test` passes on current `main` (AC: all)
- [ ] Verify `src/tools/mod.rs` currently exports `FsTool`, `GitTool`, `TerminalTool`, `ReadFileTool`, `EditFileTool` (AC: 7)
- [ ] Read `src/tools/read_file.rs` to confirm established patterns for this epic (struct shape, error enum style, validate_path, test patterns) (AC: 7)

### Task 1: Add New Dependencies to `Cargo.toml`

- [ ] Add `ignore = "0.4"` to `[dependencies]` — .gitignore-aware directory walker (used by ripgrep) (AC: 3, 6)
- [ ] Add `globset = "0.4"` to `[dependencies]` — glob pattern matching for `include_pattern` and `find_path` (AC: 2, 5)
  - [ ] Note: `globset` is from the BurntSushi ecosystem (same author as `regex` and `ignore`), well-maintained, and may already be a transitive dependency of `ignore`
- [ ] Run `cargo check` to verify new dependencies resolve correctly (AC: all)

### Task 2: Create `src/tools/grep.rs` — Struct, Args, Error Enum

- [ ] Create file `src/tools/grep.rs` (AC: 7)
- [ ] Define `GrepTool` struct: `#[derive(Debug, Serialize, Deserialize)]` with `project_root: PathBuf` field (AC: 7)
- [ ] Define `GrepToolArgs` struct: `#[derive(Debug, Deserialize)]` with fields (AC: 1, 2, 4):
  - `regex: String` — regex pattern (required)
  - `include_pattern: Option<String>` — glob filter (e.g., `"**/*.rs"`)
  - `context_lines: Option<u32>` — lines of context before/after each match (default: 2)
  - `offset: Option<u32>` — pagination offset for paginated results (default: 0)
  - Doc comments on each field explaining purpose and defaults
- [ ] Define `GrepToolError` enum: `#[derive(Debug, thiserror::Error)]` with variants (AC: 7):
  - `InvalidRegex { pattern: String, reason: String }` — invalid regex pattern
  - `PathDenied { path: String, reason: String }` — path outside project root
  - `WalkError { reason: String }` — directory traversal error
  - `IoError { path: String, reason: String }` — file read error
  - `InvalidGlob { pattern: String, reason: String }` — invalid include_pattern glob

### Task 3: Implement `GrepTool` Core Methods

- [ ] Implement `GrepTool::new(project_root: PathBuf) -> Self` (AC: 7)
- [ ] Implement `GrepTool::validate_project_root(&self) -> Result<PathBuf, GrepToolError>` — canonicalize and verify project root exists (AC: 3)
- [ ] Implement core search logic in `call()` (AC: 1, 2, 3, 4):
  - [ ] Compile the user's regex pattern via `regex::Regex::new()` — return `InvalidRegex` error on failure
  - [ ] Build a directory walker using `ignore::WalkBuilder::new(&project_root)`:
    - `.hidden(true)` to skip hidden files/dirs (consistent with ripgrep default behavior)
    - `.git_ignore(true)` to respect `.gitignore`
    - `.git_global(true)` to respect global gitignore
    - `.git_exclude(true)` to respect `.git/info/exclude`
    - `.filter_entry(|e| ...)` to skip `.git` directory itself
  - [ ] If `include_pattern` is provided, compile it as a `globset::Glob` and filter entries against it
  - [ ] For each file entry from the walker:
    - Read file content as UTF-8 (skip non-UTF-8/binary files silently — they can't contain text matches)
    - Search each line for regex matches
    - For each match, collect: file path (relative to project root), line number (1-indexed), line content (trimmed trailing newline)
    - If `context_lines` > 0, include the specified number of lines before and after each match
  - [ ] Apply pagination: skip first `offset` matches, return at most 20 matches
  - [ ] Format output as a structured text response with clear separators between matches

### Task 4: Implement `Tool` Trait for `GrepTool`

- [ ] Implement `Tool for GrepTool` with (AC: 1, 7):
  - `const NAME: &'static str = "grep"`
  - `type Error = GrepToolError`
  - `type Args = GrepToolArgs`
  - `type Output = String`
- [ ] Implement `definition()` with comprehensive description and JSON schema (AC: 7):
  - Description must teach the LLM: when to use grep, how regex works, how include_pattern filters, pagination behavior
  - JSON schema with `regex` (required), `include_pattern`, `context_lines`, `offset` (all optional)
  - JSON schema should include `"default": 2` for `context_lines` and `"default": 0` for `offset` so LLM knows defaults without reading description
  - `include_pattern` description must warn: `"Use **/*.rs to match all Rust files recursively, not *.rs (which only matches root)"`
- [ ] Implement `call()` with tracing: `tracing::info!(action = "grep", pattern = %args.regex, ...)` before and after (AC: 7)
- [ ] Return meaningful output even for zero matches: `"No matches found for pattern '...' [searched N files]"` (AC: 1)

### Task 5: Create `src/tools/find_path.rs` — Struct, Args, Error Enum

- [ ] Create file `src/tools/find_path.rs` (AC: 7)
- [ ] Define `FindPathTool` struct: `#[derive(Debug, Serialize, Deserialize)]` with `project_root: PathBuf` field (AC: 7)
- [ ] Define `FindPathToolArgs` struct: `#[derive(Debug, Deserialize)]` with fields (AC: 5):
  - `glob: String` — glob pattern (required)
  - `offset: Option<u32>` — pagination offset (default: 0)
  - Doc comments on each field
- [ ] Define `FindPathToolError` enum: `#[derive(Debug, thiserror::Error)]` with variants (AC: 7):
  - `InvalidGlob { pattern: String, reason: String }` — invalid glob pattern
  - `PathDenied { path: String, reason: String }` — path outside project root
  - `WalkError { reason: String }` — directory traversal error

### Task 6: Implement `FindPathTool` Core Methods

- [ ] Implement `FindPathTool::new(project_root: PathBuf) -> Self` (AC: 7)
- [ ] Implement `FindPathTool::validate_project_root(&self) -> Result<PathBuf, FindPathToolError>` (AC: 6)
- [ ] Implement core path discovery logic in `call()` (AC: 5, 6):
  - [ ] Compile the user's glob pattern via `globset::Glob::new()` — return `InvalidGlob` error on failure
  - [ ] Build a directory walker using `ignore::WalkBuilder::new(&project_root)`:
    - `.hidden(true)` to skip hidden files/dirs (consistent with ripgrep default behavior)
    - `.git_ignore(true)` to respect `.gitignore`
    - `.git_global(true)` to respect global gitignore
    - `.git_exclude(true)` to respect `.git/info/exclude`
    - `.filter_entry(|e| ...)` to skip `.git` directory itself
    - Only collect entries where `entry.file_type().map_or(false, |ft| ft.is_file())` — return files only, not directories
  - [ ] For each file entry from the walker:
    - Compute path relative to project root
    - Test against the compiled glob pattern
    - Collect matching paths
  - [ ] Sort results alphabetically
  - [ ] Apply pagination: skip first `offset` results, return at most 50 results
  - [ ] Format output: one path per line, with total match count header

### Task 7: Implement `Tool` Trait for `FindPathTool`

- [ ] Implement `Tool for FindPathTool` with (AC: 5, 7):
  - `const NAME: &'static str = "find_path"`
  - `type Error = FindPathToolError`
  - `type Args = FindPathToolArgs`
  - `type Output = String`
- [ ] Implement `definition()` with comprehensive description and JSON schema (AC: 7):
  - Description must teach the LLM: when to use find_path vs grep, glob syntax examples, pagination
  - JSON schema with `glob` (required), `offset` (optional)
  - JSON schema should include `"default": 0` for `offset`
  - `glob` description must include examples: `"**/*.rs"` (recursive), `"src/**/mod.rs"` (scoped), `"Cargo.*"` (root only)
- [ ] Implement `call()` with tracing: `tracing::info!(action = "find_path", glob = %args.glob, ...)` before and after (AC: 7)
- [ ] Return meaningful output for zero matches: `"No files found matching pattern '...'"` (AC: 5)
  - If the pattern does NOT contain `**/` or `/`, append a hint: `"Hint: use **/{pattern} to search recursively."`

### Task 8: Update Module Registry (`src/tools/mod.rs`)

- [ ] Add `pub mod grep;` declaration (AC: 7)
- [ ] Add `pub mod find_path;` declaration (AC: 7)
- [ ] Add `pub use grep::GrepTool;` re-export (AC: 7)
- [ ] Add `pub use find_path::FindPathTool;` re-export (AC: 7)
- [ ] Update module doc comment to include GrepTool and FindPathTool descriptions (AC: 7)

### Task 9: Unit Tests — GrepTool (`#[cfg(test)] mod tests` in `grep.rs`)

- [ ] `test_grep_basic_match` — search for a simple pattern in a known file (AC: 1)
- [ ] `test_grep_regex_match` — search with regex special characters (e.g., `fn\s+\w+`) (AC: 1)
- [ ] `test_grep_case_sensitive_default` — verify search IS case-sensitive without `(?i)` flag (AC: 1)
- [ ] `test_grep_case_insensitive_regex` — verify `(?i)` regex flag works (AC: 1)
- [ ] `test_grep_multiple_matches_same_file` — multiple lines match in one file (AC: 1)
- [ ] `test_grep_matches_across_files` — matches found in multiple files (AC: 1)
- [ ] `test_grep_no_matches` — returns zero-match message with file count (AC: 1)
- [ ] `test_grep_include_pattern_filters_files` — only `.rs` files searched when `include_pattern = "**/*.rs"` (AC: 2)
- [ ] `test_grep_include_pattern_no_matches` — include_pattern excludes all files with matches (AC: 2)
- [ ] `test_grep_invalid_include_pattern` — malformed glob returns `InvalidGlob` error (AC: 2)
- [ ] `test_grep_respects_gitignore` — file listed in `.gitignore` is excluded from search (AC: 3)
- [ ] `test_grep_respects_nested_gitignore` — nested `.gitignore` in subdirectory is respected (AC: 3)
- [ ] `test_grep_context_lines_default` — default context (2 lines before/after) is included (AC: 4)
- [ ] `test_grep_context_lines_custom` — custom `context_lines` value works (AC: 4)
- [ ] `test_grep_context_lines_zero` — `context_lines = 0` returns only matching lines (AC: 4)
- [ ] `test_grep_context_lines_at_file_boundary` — context near start/end of file is clamped (AC: 4)
- [ ] `test_grep_pagination_default_limit` — only 20 results returned when more exist (AC: 1)
- [ ] `test_grep_pagination_with_offset` — offset skips first N matches (AC: 1)
- [ ] `test_grep_pagination_offset_beyond_results` — offset past end returns empty (AC: 1)
- [ ] `test_grep_invalid_regex` — invalid regex returns `InvalidRegex` error (AC: 7)
- [ ] `test_grep_skips_binary_files` — binary file content not searched (AC: 1)
- [ ] `test_grep_empty_project` — empty directory returns zero matches (AC: 1)
- [ ] `test_grep_skips_git_directory` — `.git/` contents are never searched (AC: 3)
- [ ] `test_grep_line_numbers_are_one_indexed` — verify line numbers start at 1 (AC: 1)
- [ ] `test_grep_output_format` — verify output includes file path, line number, content (AC: 1)
- [ ] `test_grep_definition_name` — verify `NAME == "grep"` (AC: 7)
- [ ] `test_grep_definition_has_detailed_description` — verify description is comprehensive (AC: 7)
- [ ] `test_grep_serializable` — verify struct is serializable/deserializable (AC: 7)
- [ ] `test_grep_error_is_send_sync` — verify error type implements Send + Sync (AC: 7)

### Task 10: Unit Tests — FindPathTool (`#[cfg(test)] mod tests` in `find_path.rs`)

- [ ] `test_find_path_basic_glob` — find all `.rs` files with `"**/*.rs"` (AC: 5)
- [ ] `test_find_path_specific_pattern` — find `"src/**/mod.rs"` (AC: 5)
- [ ] `test_find_path_exact_filename` — find `"Cargo.toml"` (AC: 5)
- [ ] `test_find_path_wildcard_extension` — find `"**/*.md"` (AC: 5)
- [ ] `test_find_path_no_matches` — returns zero-match message (AC: 5)
- [ ] `test_find_path_results_sorted_alphabetically` — verify alphabetical order (AC: 5)
- [ ] `test_find_path_respects_gitignore` — file listed in `.gitignore` excluded (AC: 6)
- [ ] `test_find_path_respects_nested_gitignore` — nested `.gitignore` respected (AC: 6)
- [ ] `test_find_path_pagination_default_limit` — only 50 results returned when more exist (AC: 5)
- [ ] `test_find_path_pagination_with_offset` — offset skips first N results (AC: 5)
- [ ] `test_find_path_pagination_offset_beyond_results` — offset past end returns empty (AC: 5)
- [ ] `test_find_path_invalid_glob` — malformed glob returns `InvalidGlob` error (AC: 7)
- [ ] `test_find_path_skips_git_directory` — `.git/` paths never returned (AC: 6)
- [ ] `test_find_path_empty_project` — empty directory returns zero matches (AC: 5)
- [ ] `test_find_path_relative_paths` — all returned paths are relative to project root (AC: 5)
- [ ] `test_find_path_includes_total_count` — output header shows total match count (AC: 5)
- [ ] `test_find_path_definition_name` — verify `NAME == "find_path"` (AC: 7)
- [ ] `test_find_path_definition_has_detailed_description` — verify description is comprehensive (AC: 7)
- [ ] `test_find_path_serializable` — verify struct is serializable/deserializable (AC: 7)
- [ ] `test_find_path_error_is_send_sync` — verify error type implements Send + Sync (AC: 7)

### Task 11: Integration Verification

- [ ] Run `cargo fmt` (AC: 7)
- [ ] Run `cargo clippy` with zero warnings (AC: 7)
- [ ] Run `cargo test` — all existing tests + new tests pass (AC: 7)
- [ ] Verify `tools/mod.rs` exports: `FsTool`, `GitTool`, `TerminalTool`, `ReadFileTool`, `EditFileTool`, `GrepTool`, `FindPathTool` (AC: 7)
- [ ] Verify no changes to `fs.rs`, `read_file.rs`, `edit_file.rs`, `git.rs`, `terminal.rs`, `runner.rs`, `read_tool.rs` (AC: 7)

## Dev Notes

### Previous Story Intelligence — Story 8.1 (ReadFileTool) & Story 8.2 (EditFileTool)

**Patterns established in Stories 8.1 and 8.2 that MUST be followed:**
- Tool struct: `#[derive(Debug, Serialize, Deserialize)]` with single `project_root: PathBuf` field
- Args struct: `#[derive(Debug, Deserialize)]` with doc comments on each field
- Error enum: `#[derive(Debug, thiserror::Error)]` with descriptive variants, all fields named
- `validate_path`: `canonicalize()` + `starts_with()` security check — replicate exactly
- `call()` logs with `tracing::info!(action = "grep", ...)` before and after
- JSON schema in `definition()` uses `"type": "integer"` for integers, `"enum": [...]` for constrained strings
- Tests follow `test_{tool}_{behavior}_{scenario}` naming, Arrange → Act → Assert
- All async file I/O via `tokio::fs`
- `static LazyLock<Regex>` at module level for any regex patterns (edition 2024 stable)
- `tempfile::TempDir` for all test fixtures
- Tools do NOT retry internally — all errors bubble to the rig agent
- Security boundary pattern from `FsTool::validate_path()` at `src/tools/fs.rs` lines 96-121

**From previous stories' anti-patterns (carry forward):**
- ❌ NO `unwrap()`/`expect()` in production
- ❌ NO `anyhow` — thiserror only
- ❌ NO `println!` — tracing only
- ❌ NO panic in `call()`
- ❌ NO blocking I/O on the async runtime — use `std::fs` inside `spawn_blocking`, `tokio::fs` elsewhere. Inside `spawn_blocking` closures, synchronous `std::fs::read_to_string` is correct and expected.
- ❌ NO modifying `fs.rs`, `read_file.rs`, `edit_file.rs`, `runner.rs`, `read_tool.rs`

### Architecture Decision 7 — GrepTool Design Spec

**From `architecture.md` Decision 7 (authoritative design):**

```
GrepToolArgs {
    regex: String,                    // Regex pattern (Rust `regex` crate syntax)
    include_pattern: Option<String>,  // Glob filter (e.g., "src/**/*.rs")
    context_lines: Option<u32>,       // Lines of context before/after each match (default: 2)
    max_results: Option<u32>,         // Pagination limit (default: 20)
}
```

**IMPLEMENTATION NOTE — `offset` vs `max_results`:** The architecture spec shows `max_results` as pagination parameter. However, for consistency with how paginated tools work in modern AI coding assistants (and how the epics describe pagination for this tool), use an **`offset: Option<u32>`** parameter instead of `max_results`. The page size is always fixed at 20 for grep. The `offset` parameter tells the tool how many matches to skip before starting to collect results. This allows the LLM to request "page 2" by setting `offset: 20`, "page 3" by setting `offset: 40`, etc. This is more flexible and matches the established pattern. The output should include a header like `"Found N total matches. Showing matches {offset+1}-{offset+page_size}:"` so the LLM knows if there are more results to fetch.

**From architecture spec — Implementation suggestion:**
> Uses `grep -rn --include` via `TerminalTool` internally (or the `grep` crate for pure Rust), with structured output parsing. Returns matches as `{path, line_number, content, context_before, context_after}`.

**IMPLEMENTATION DECISION — Pure Rust (not shelling out):** Use the `ignore` crate for directory walking (it's the same walker used by ripgrep, handles .gitignore natively) combined with the `regex` crate for pattern matching. This is pure Rust, cross-platform, and avoids the fragility of parsing shell output. Do NOT shell out to `grep` or `rg` via TerminalTool.

### Architecture Decision 7 — FindPathTool Design Spec

**From `architecture.md` Decision 7 (authoritative design):**

```
FindPathToolArgs {
    glob: String,                     // Glob pattern (e.g., "**/*.rs", "src/**/mod.rs")
    max_results: Option<u32>,         // Pagination limit (default: 50)
}
```

**Same `offset` change as GrepTool:** Use `offset: Option<u32>` instead of `max_results`. Fixed page size of 50. Output includes total count header.

**Implementation:** Uses the `ignore` crate for .gitignore-aware walking + `globset` crate for glob pattern matching against relative paths.

### New Dependencies — `ignore` and `globset` Crates

**`ignore` crate (version 0.4):**
- Created by Andrew Gallant (BurntSushi), the author of `regex` and `ripgrep`
- Provides a directory walker that natively respects `.gitignore`, `.git/info/exclude`, and global gitignore
- The `WalkBuilder` API allows configuring hidden file handling, max depth, follow symlinks, etc.
- Used by ripgrep, tokei, fd, and other major Rust CLI tools
- **Key API:**
  ```
  use ignore::WalkBuilder;
  
  let walker = WalkBuilder::new(&project_root)
      .hidden(true)          // Skip hidden files/dirs (matches ripgrep default)
      .git_ignore(true)      // Respect .gitignore
      .git_global(true)      // Respect global gitignore
      .git_exclude(true)     // Respect .git/info/exclude
      .build();
  
  for entry in walker {
      let entry = entry?;
      // entry.path() — absolute path
      // entry.file_type() — Some(FileType) for regular entries
      // Filter: entry.file_type().map_or(false, |ft| ft.is_file()) — files only
  }
  ```
- **Note:** The `ignore` walker is synchronous (returns an iterator). Both the walker and file content reading run inside `spawn_blocking`, so use `std::fs::read_to_string` (not `tokio::fs`) — see "Async Considerations" section for the full pattern.

**`globset` crate (version 0.4):**
- Also by BurntSushi, part of the ripgrep ecosystem
- Provides fast glob pattern matching compiled to a matcher
- **Key API:**
  ```
  use globset::Glob;
  
  let glob = Glob::new("**/*.rs")?.compile_matcher();
  glob.is_match("src/tools/grep.rs")  // true
  glob.is_match("README.md")          // false
  ```
- Used for `include_pattern` in GrepTool and the `glob` parameter in FindPathTool
- Supports standard glob syntax: `*`, `**`, `?`, `[...]`, `{a,b}`

**Add to `Cargo.toml` `[dependencies]`:**
```
ignore = "0.4"
globset = "0.4"
```

### GrepTool Output Format — Critical for LLM Usage

The output format must be clear, parseable, and token-efficient. Use this format:

**For matches with no context (`context_lines = 0`):**
```
Found 15 total matches. Showing matches 1-15.

src/tools/mod.rs:3:pub mod fs;
src/tools/mod.rs:4:pub mod git;
src/tools/grep.rs:12:pub struct GrepTool {
src/tools/grep.rs:45:    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
```

Format per line: `{relative_path}:{line_number}:{line_content}`

This matches the `grep -rn` format that LLMs are trained on and recognize naturally.

**For matches with context (`context_lines > 0`):**
```
Found 3 total matches. Showing matches 1-3.

src/tools/mod.rs:
  1| pub mod fs;
  2| pub mod git;
  3: pub mod terminal;  // ← match
  4| 
  5| pub use fs::FsTool;
--
src/config/mod.rs:
 10| /// Load configuration from the YAML file.
 11: pub fn load_config(path: &Path) -> Result<Config, ConfigError> {  // ← match
 12|     let content = std::fs::read_to_string(path)?;
```

Format: `{path}:` header, then context lines with `|` separator, matching line with `:` separator. `--` between match groups.

**For paginated results (more results available):**
```
Found 47 total matches. Showing matches 1-20 (27 more available, use offset: 20 to see next page).

...matches...
```

**For zero matches:**
```
No matches found for pattern 'nonexistent_function' (searched 142 files).
```

### FindPathTool Output Format

**For matches:**
```
Found 12 total matches. Showing results 1-12.

src/tools/mod.rs
src/tools/fs.rs
src/tools/git.rs
src/tools/grep.rs
src/tools/find_path.rs
src/tools/terminal.rs
src/tools/read_file.rs
src/tools/edit_file.rs
src/config/mod.rs
src/session/mod.rs
src/supervisor/mod.rs
src/main.rs
```

One path per line, alphabetically sorted, relative to project root.

**For paginated results:**
```
Found 85 total matches. Showing results 1-50 (35 more available, use offset: 50 to see next page).

...paths...
```

**For zero matches:**
```
No files found matching pattern 'src/**/*.py'.
```

### Tool Definition Descriptions — Critical for LLM Usage

**GrepTool `definition()` description — suggested template:**

> Search file contents in the project using a regex pattern. Returns matching lines with file paths and line numbers.
>
> **Usage:** Provide a `regex` pattern (Rust regex syntax). Use `include_pattern` to limit search to specific file types (e.g., `"**/*.rs"` for Rust files only).
>
> **Results:** Each match shows `file_path:line_number:content`. Results are paginated (20 per page). Use `offset` to get subsequent pages.
>
> **Tips:**
> - Use `\b` for word boundaries: `\bfn\b` matches "fn" but not "pfn"
> - Use `(?i)` prefix for case-insensitive: `(?i)error`
> - Combine with `read_file` to see surrounding context after finding a match
> - Files matching `.gitignore` patterns are automatically excluded
>
> **Prefer `grep` over `find_path` when** you need to find where a symbol, string, or pattern is used in code.
> **Prefer `find_path` over `grep` when** you need to find files by name or extension.

**FindPathTool `definition()` description — suggested template:**

> Find files in the project by glob pattern. Returns matching file paths sorted alphabetically.
>
> **Usage:** Provide a `glob` pattern using standard glob syntax:
> - `**/*.rs` — all Rust files recursively
> - `src/**/mod.rs` — all mod.rs files under src/
> - `Cargo.*` — files starting with "Cargo" in the root
> - `src/tools/*.rs` — Rust files directly in src/tools/
>
> **Results:** One path per line, sorted alphabetically. Results are paginated (50 per page). Use `offset` to get subsequent pages.
>
> **Prefer `find_path` over `grep` when** you need to discover files by name or extension.
> **Prefer `grep` over `find_path` when** you need to find files containing specific code or text.

### Handling the `.git` Directory

The `ignore` crate's `WalkBuilder` with `.git_ignore(true)` does NOT automatically skip the `.git` directory itself. You must explicitly filter it out:

```rust
let walker = WalkBuilder::new(&project_root)
    .git_ignore(true)
    .git_global(true)
    .git_exclude(true)
    .filter_entry(|entry| {
        // Skip .git directory
        !(entry.file_type().map_or(false, |ft| ft.is_dir())
            && entry.file_name() == ".git")
    })
    .build();
```

Alternatively, the `ignore` crate may handle this with `.hidden(true)` (which skips hidden files/dirs). Test both approaches to confirm `.git/` contents are excluded.

### Binary File Detection in GrepTool

When reading files for grep matching, some files may be binary (images, compiled objects, etc.). Strategy:

1. Attempt `std::fs::read_to_string(path)` inside `spawn_blocking` — this returns `Err` for non-UTF-8 files
2. On `Err` → silently skip the file (binary files can't contain text matches), increment `files_searched` counter anyway
3. Do NOT log a warning for every binary file — this would be noisy. Only `tracing::debug!()` if needed
4. Maintain a `files_searched: usize` counter incremented for each file visited (text or binary) — used in zero-match output message `"(searched N files)"`

This is the same approach used by ripgrep: skip non-text files silently.

### Context Lines — Merging Overlapping Contexts

When `context_lines > 0` and two matches are close together (within `2 * context_lines + 1` lines of each other), their context regions overlap. In this case, merge them into a single block to avoid duplicating lines:

```
# Two matches 3 lines apart, context_lines = 2:
# Instead of showing lines 1-5 and 3-7 separately:

src/tools/mod.rs:
  1| pub mod fs;
  2| pub mod git;
  3: pub mod terminal;    // ← match 1
  4| pub mod read_file;
  5: pub mod edit_file;   // ← match 2
  6| 
  7| pub use fs::FsTool;
```

Implementation approach: collect all match line numbers first, then expand each by ±context_lines, merge overlapping ranges, then output each merged range as a single block.

### Async Considerations

The `ignore` crate's `WalkBuilder` produces a synchronous iterator. Options:

1. **Recommended:** Use `tokio::task::spawn_blocking()` to run the entire walk+search in a blocking thread pool task. This prevents blocking the tokio runtime while still being async-compatible.

```rust
async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
    let project_root = self.project_root.clone();
    let regex_str = args.regex.clone();
    // ... prepare other args ...
    
    tokio::task::spawn_blocking(move || {
        // Synchronous walk + search logic here
        // Use std::fs::read_to_string (NOT tokio::fs) inside spawn_blocking
        // The ignore walker and all file reads are synchronous here — this is CORRECT
    })
    .await
    .map_err(|e| GrepToolError::IoError {
        path: "".to_string(),
        reason: format!("Task join error: {e}"),
    })?
}
```

2. Inside `spawn_blocking`, use **`std::fs::read_to_string`** (NOT `tokio::fs`) since we're already on a blocking thread. Using `tokio::fs` inside `spawn_blocking` is unnecessary overhead — `tokio::fs` itself just wraps `std::fs` in `spawn_blocking`, so you'd be double-spawning.

3. **Both tools** (GrepTool and FindPathTool) should use the same `spawn_blocking` pattern since both use the `ignore` walker.

**This is the correct pattern:** `ignore`'s walker is CPU+IO bound work that should run on a blocking thread, not on the async runtime. This does NOT violate the "no blocking I/O" rule — `spawn_blocking` is the sanctioned way to run blocking work in tokio.

### Glob Pattern Matching — Full Path vs Filename

**For GrepTool `include_pattern`:** Match against the **relative path** from project root (e.g., `src/tools/grep.rs`). This allows patterns like `"src/**/*.rs"` to work as expected.

**For FindPathTool `glob`:** Same — match against the **relative path** from project root. The user provides patterns relative to the project root.

**Important:** `globset::Glob` needs to be created with `globset::GlobBuilder::new(pattern).literal_separator(false).build()` or simply `Glob::new(pattern)`. The default behavior of `**` matching path separators is what we want.

**⚠️ CRITICAL `globset` GOTCHA:** `Glob::new("*.rs")` does **NOT** match `src/main.rs` — it only matches files in the root directory. The `*` wildcard does not cross path separators. To match recursively, the pattern must use `**/*.rs`. This is standard glob behavior but is a common LLM mistake. Mitigations:
1. The tool `definition()` description MUST include clear examples showing `**/*.rs` (recursive) vs `*.rs` (root only)
2. Consider adding a hint in the FindPathTool output when zero matches are found and the pattern does NOT contain `**/` or `/`: `"Hint: Use **/*.rs instead of *.rs to search recursively."`
3. The GrepTool `include_pattern` description should similarly warn: `"Use **/*.rs to match all Rust files, not *.rs"`

### Edge Cases to Handle

**GrepTool:**
- Empty regex string → compile with `Regex::new("")` — this matches every line. This is technically valid but wasteful. Return results normally (the pagination will limit output).
- Regex with only anchors (e.g., `^$`) → matches empty lines. Valid, return results.
- Very large files → no special handling for MVP. The `ignore` walker naturally limits to text files.
- Symlinks → `ignore` crate handles symlink following configuration. Default: don't follow symlinks (safe choice).

**FindPathTool:**
- Glob `"*"` → matches all files in root directory (not recursive). Valid.
- Glob `"**"` → matches all files recursively. Valid but potentially large result set — pagination handles this.
- Glob with path separator at start (e.g., `"/src"`) → `globset` handles this. Should still work relative to project root.

### Dependencies — Summary of Changes to `Cargo.toml`

Add these two lines to `[dependencies]`:
```
ignore = "0.4"
globset = "0.4"
```

No other dependency changes. All other crates are already present:
- `rig-core = "0.30"` — Tool trait
- `serde = { version = "1", features = ["derive"] }` — Serialize/Deserialize
- `serde_json = "1"` — JSON schema in tool definition
- `thiserror = "2"` — Error enum
- `tracing = "0.1"` — Structured logging
- `tokio = { version = "1", features = ["full"] }` — Async runtime + spawn_blocking
- `regex = "1"` — Regex pattern matching
- `tempfile = "3"` (dev) — Test fixtures

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code — only in tests
- ❌ **NO** `anyhow::Result` — typed `thiserror` enums only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** panicking in `call()` — always return `Result`
- ❌ **NO** blocking I/O on the async runtime — use `std::fs` inside `spawn_blocking`, `tokio::fs` elsewhere. Inside `spawn_blocking` closures, synchronous `std::fs` is correct and expected (do NOT use `tokio::fs` inside `spawn_blocking`)
- ❌ **NO** modifying `src/tools/fs.rs` — FsTool remains untouched until Story 8.4
- ❌ **NO** modifying `src/tools/read_file.rs` — ReadFileTool is complete
- ❌ **NO** modifying `src/tools/edit_file.rs` — EditFileTool is complete
- ❌ **NO** modifying `src/supervisor/read_tool.rs` — supervisor tool update is Story 8.4
- ❌ **NO** modifying `src/session/runner.rs` — tool registration update is Story 8.5
- ❌ **NO** removing `FsTool` from `src/tools/mod.rs` — that's Story 8.4
- ❌ **NO** shelling out to `grep`, `rg`, `find`, or any external process — pure Rust implementation using `ignore` + `regex` + `globset`
- ❌ **NO** `action: String` multiplexer field — each tool has a single responsibility
- ❌ **NO** internal retry logic — errors bubble to the rig agent
- ❌ **NO** storing compiled regex in the tool struct — compile from user input on each `call()` since the pattern changes every time (unlike outline mode regex in ReadFileTool which is static)
- ❌ **NO** following symlinks — use `ignore` crate default (no symlink follow) for security
- ❌ **NO** searching the `.git` directory — always filter it out explicitly
- ❌ **NO** logging warnings for binary/non-UTF-8 files — skip silently, `tracing::debug!()` at most
- ❌ **NO** returning directories from `FindPathTool` — filter entries to files only (`entry.file_type().map_or(false, |ft| ft.is_file())`)
- ❌ **NO** using `tokio::fs` inside `spawn_blocking` — it double-spawns for no benefit; use `std::fs` directly

### Scope Boundaries

**IN SCOPE for this story:**
- `src/tools/grep.rs` — Full `GrepTool` implementation with pagination, context lines, include_pattern, .gitignore respect + unit tests
- `src/tools/find_path.rs` — Full `FindPathTool` implementation with pagination, .gitignore respect + unit tests
- `src/tools/mod.rs` — Add module declarations and re-exports (alongside existing FsTool, ReadFileTool, EditFileTool)
- `Cargo.toml` — Add `ignore = "0.4"` and `globset = "0.4"` to dependencies

**OUT OF SCOPE — do NOT implement:**
- ListDirectoryTool (Story 8.4)
- FsTool removal (Story 8.4)
- `supervisor/read_tool.rs` migration (Story 8.4)
- `session/runner.rs` preamble or tool registration updates (Story 8.5)
- Agent builder changes (Story 8.5)
- Any changes to `read_file.rs` (Story 8.1 — complete)
- Any changes to `edit_file.rs` (Story 8.2 — complete)

### Files Created/Modified in This Story

| File | Change |
|------|--------|
| `Cargo.toml` | **MODIFY** — Add `ignore = "0.4"` and `globset = "0.4"` to `[dependencies]` |
| `src/tools/grep.rs` | **CREATE** — Full GrepTool implementation + unit tests |
| `src/tools/find_path.rs` | **CREATE** — Full FindPathTool implementation + unit tests |
| `src/tools/mod.rs` | **MODIFY** — Add `pub mod grep;`, `pub mod find_path;`, `pub use grep::GrepTool;`, `pub use find_path::FindPathTool;` + update doc comment |

### Testing Requirements

All tests follow established patterns: `test_{tool}_{behavior}_{scenario}`, Arrange → Act → Assert, `tempfile::TempDir` for test fixtures.

**Test fixture setup helper for both tools:**
Create a helper function that builds a realistic temporary project directory:
```rust
fn create_test_project(dir: &Path) -> PathBuf {
    // Create directory structure:
    // dir/
    //   .git/             → empty directory (CRITICAL: ensures `ignore` crate recognizes .gitignore)
    //   src/
    //     main.rs         → "fn main() {\n    println!(\"hello\");\n}\n"
    //     lib.rs          → "pub mod tools;\npub mod config;\n"
    //     tools/
    //       mod.rs        → "pub mod grep;\npub mod find_path;\n"
    //       grep.rs       → "pub struct GrepTool {...}\nimpl GrepTool {...}\n"
    //   Cargo.toml        → "[package]\nname = \"test\"\n"
    //   README.md         → "# Test Project\nSome content.\n"
    //   .gitignore        → "target/\n*.log\n"
    //   target/
    //     debug/
    //       output.log    → "debug output" (should be ignored by .gitignore)
    //   build.log          → "build log" (should be ignored by .gitignore)
}
```

**⚠️ CRITICAL for gitignore tests:** Always create a `.git/` directory (even empty) in the test fixture root. The `ignore` crate uses the presence of `.git/` to determine the repository root when processing `.gitignore` files. Without `.git/`, the crate may not fully respect `.gitignore` rules. Create it with `std::fs::create_dir_all(dir.join(".git"))`.

**For gitignore tests:** Create a `.gitignore` file in the test project root, add files that match its patterns, and verify they are excluded from results.

**For context lines tests:** Create files with known content where matches are at predictable line numbers, then verify the context output includes the correct surrounding lines.

**For pagination tests:** Create enough matches (>20 for grep, >50 for find_path) and verify only the page-size number of results are returned, with correct offset behavior.

**Test coverage targets:**
- GrepTool: ~29 tests (added `test_grep_case_sensitive_default`)
- FindPathTool: ~20 tests

### Project Structure Notes

After this story, the tools module gains `grep.rs` and `find_path.rs`:

```
src/tools/
├── mod.rs          # Module declarations + pub re-exports (GitTool, FsTool, TerminalTool, ReadFileTool, EditFileTool, GrepTool, FindPathTool)
├── grep.rs         # GrepTool — regex search across file contents (NEW)
├── find_path.rs    # FindPathTool — glob-based file path discovery (NEW)
├── edit_file.rs    # EditFileTool — surgical search-replace, create, overwrite (Story 8.2)
├── read_file.rs    # ReadFileTool — partial reading + outline mode (Story 8.1)
├── fs.rs           # FsTool — legacy 6-action filesystem tool (UNCHANGED — removed in Story 8.4)
├── git.rs          # GitTool — 9 git operations via git2 (UNCHANGED)
└── terminal.rs     # TerminalTool — shell execution with timeout (UNCHANGED)
```

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 8.3: GrepTool & FindPathTool — Codebase Search & Navigation] — Acceptance criteria and user story
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 8: Surgical Development Tooling] — Epic context, dependency chain (8.1→8.2→**8.3**→8.4→8.5), impact metrics
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 7: Surgical Development Tooling] — Full GrepTool and FindPathTool design specs (args, behavior, output format)
- [Source: _bmad-output/planning-artifacts/architecture.md#Rig Tool Implementation Pattern — Standard Structure] — Mandatory tool structure pattern (struct + args + error + Tool impl)
- [Source: _bmad-output/planning-artifacts/architecture.md#Error Type Pattern — Per-Module thiserror Enums] — Per-module thiserror, no anyhow in library modules
- [Source: _bmad-output/planning-artifacts/architecture.md#Tracing Pattern — Structured Spans with Story Context] — Every tool action logged with `action` field
- [Source: _bmad-output/planning-artifacts/architecture.md#Project Structure & Boundaries] — tools/ module layout showing `grep.rs`, `find_path.rs`
- [Source: _bmad-output/planning-artifacts/architect-brief-surgical-tooling.md#Story 8.3] — Architect brief with scope and rationale
- [Source: _bmad-output/project-context.md#Framework-Specific Rules] — 9 tools exposed to the agent, tool design principle (focused tools over action multiplexing)
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — Tool design rules, one tool = one concern
- [Source: _bmad-output/project-context.md#Testing Rules] — Tests inline, `#[cfg(test)] mod tests`, every module must include unit tests
- [Source: _bmad-output/project-context.md#Code Quality & Style Rules] — rustfmt, clippy, doc comments mandatory on public items
- [Source: src/tools/fs.rs#L86-121] — `FsTool::validate_path()` — canonical security boundary pattern to replicate
- [Source: src/tools/fs.rs#L417-495] — `impl Tool for FsTool` — reference for Tool trait implementation (JSON schema, definition, call pattern)
- [Source: src/tools/mod.rs] — Current module registry (add GrepTool and FindPathTool alongside existing exports)
- [Source: _bmad-output/implementation-artifacts/8-1-read-file-tool-partial-reading-outline-mode.md] — Story 8.1 dev notes — established patterns for this epic (struct shape, error enum, validate_path, tests, LazyLock regex, anti-patterns)
- [Source: _bmad-output/implementation-artifacts/8-2-edit-file-tool-surgical-search-replace-editing.md] — Story 8.2 dev notes — confirmed patterns (offset recalculation, atomic writes, parent dir creation, anti-patterns)
- [Source: _bmad-output/implementation-artifacts/4-1-rig-tools-implementation-git-filesystem-terminal.md] — Story 4.1 dev notes — original tool patterns, FsTool design decisions
- [Source: Cargo.toml] — Current dependencies verified; `ignore` and `globset` need to be added

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List