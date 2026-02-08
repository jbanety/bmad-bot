# Story 4.1: Rig Tools Implementation (Git, Filesystem, Terminal)

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the agent to have access to git, filesystem, and terminal tools during development sessions,
So that the agent can perform all operations needed to develop a story autonomously.

## Acceptance Criteria

1. **Given** the tools module is initialized **When** the git tool (`tools/git.rs`) is built **Then** it follows the standard rig Tool pattern (serializable struct + `GitToolArgs` + `GitToolError` thiserror enum + Tool trait impl) **And** it exposes git operations via git2: clone, checkout, branch create, add, commit, push, diff, status, log **And** the tool NAME is `git` and the definition description is detailed enough for the LLM to use correctly **And** every `call()` logs the action and result via `tracing` with story context

2. **Given** the tools module is initialized **When** the filesystem tool (`tools/fs.rs`) is built **Then** it follows the standard rig Tool pattern with `FsToolArgs` + `FsToolError` **And** it exposes file operations via std::fs / tokio::fs: read file, write file, list directory, create directory, delete, check existence **And** every `call()` logs the action and result via `tracing`

3. **Given** the tools module is initialized **When** the terminal tool (`tools/terminal.rs`) is built **Then** it follows the standard rig Tool pattern with `TerminalToolArgs` + `TerminalToolError` **And** it exposes command execution via tokio::process: run command, capture stdout/stderr, return exit code **And** every `call()` logs the command and result via `tracing`

4. **Given** any tool encounters an error **When** the error is handled **Then** it never panics — always returns `Result` with a descriptive error **And** errors bubble up to the rig agent loop which decides how to proceed

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [x] Verify stub files exist: `src/tools/mod.rs`, `src/tools/git.rs`, `src/tools/fs.rs`, `src/tools/terminal.rs`
- [x] Verify `rig-core = "0.30"` and `git2 = "0.20"` are in `Cargo.toml` dependencies
- [x] Verify `tokio` with `full` features is available (needed for `tokio::process`, `tokio::fs`, `tokio::task::spawn_blocking`)
- [x] Verify `serde`, `serde_json`, `thiserror`, `tracing` are all in dependencies
- [x] Run `cargo check` to confirm clean baseline (only pre-existing dead_code warnings expected)
- [x] Review `src/supervisor/read_tool.rs` and `src/supervisor/mod.rs` for reference rig Tool implementations

### Task 1: Implement Git Tool (`src/tools/git.rs`)

- [x] **1.1** Define `GitTool` struct
  - [x] `#[derive(Debug, Serialize, Deserialize)]`
  - [x] Field: `repo_path: PathBuf` — absolute path to the git repository root
  - [x] Constructor: `pub fn new(repo_path: PathBuf) -> Self`
  - [x] **CRITICAL:** The struct holds ONLY configuration (`PathBuf`). Never store `git2::Repository` — open fresh on each `call()` invocation. Required for `Serialize/Deserialize` and `Send + Sync`.

- [x] **1.2** Define `GitToolArgs` struct
  - [x] `#[derive(Debug, Deserialize)]`
  - [x] Field: `action: String` — one of: `clone`, `checkout`, `branch_create`, `add`, `commit`, `push`, `diff`, `status`, `log`
  - [x] Field: `branch: Option<String>` — branch name for checkout/branch_create
  - [x] Field: `message: Option<String>` — commit message for commit
  - [x] Field: `paths: Option<Vec<String>>` — file paths for add (glob patterns like `["*"]` for stage all)
  - [x] Field: `url: Option<String>` — remote URL for clone
  - [x] Field: `remote: Option<String>` — remote name for push (default: "origin")
  - [x] Field: `max_count: Option<usize>` — max entries for log (default: 10)
  - [x] Field: `from_branch: Option<String>` — base branch when creating a new branch (default: HEAD)

- [x] **1.3** Define `GitToolError` thiserror enum
  - [x] `InvalidAction { action: String }` — unknown action string
  - [x] `GitError { action: String, reason: String }` — wraps git2 errors with action context
  - [x] `MissingArgument { action: String, argument: String }` — required arg not provided
  - [x] `PathError { reason: String }` — repo path issues
  - [x] `TaskJoinError { reason: String }` — `spawn_blocking` join failure
  - [x] Ensure `Send + Sync` (thiserror derives this if inner types are Send + Sync)

- [x] **1.4** Implement `Tool for GitTool`
  - [x] `const NAME: &'static str = "git"`
  - [x] `type Error = GitToolError`
  - [x] `type Args = GitToolArgs`
  - [x] `type Output = String`
  - [x] `definition()`: JSON schema with `"enum"` constraint on the `action` field listing all 9 valid actions — this prevents the LLM from sending invalid actions
  - [x] `call()`: match on `args.action` and dispatch to private helper methods

- [x] **1.5** Implement private helper methods on `GitTool`
  - [x] `fn open_repo(&self) -> Result<git2::Repository, GitToolError>` — opens repo at `self.repo_path`
  - [x] `fn handle_clone(&self, url: &str) -> Result<String, GitToolError>` — clone remote repo to `self.repo_path`. **MUST** be wrapped in `tokio::task::spawn_blocking` (network I/O).
  - [x] `fn handle_checkout(&self, branch: &str) -> Result<String, GitToolError>` — checkout existing branch (set HEAD, checkout tree)
  - [x] `fn handle_branch_create(&self, branch: &str, from_branch: Option<&str>) -> Result<String, GitToolError>` — create new branch from HEAD or specified base, then checkout
  - [x] `fn handle_add(&self, paths: &[String]) -> Result<String, GitToolError>` — stage files via index (`add_all` with glob patterns or `add_path` for specific files)
  - [x] `fn handle_commit(&self, message: &str) -> Result<String, GitToolError>` — create commit on current branch with staged changes (use default signature from repo config)
  - [x] `fn handle_push(&self, remote: &str, branch: &str) -> Result<String, GitToolError>` — push branch to remote. **MUST** be wrapped in `tokio::task::spawn_blocking` (network I/O). Credential callback chain: try SSH agent first → then credential helper (userpass_plaintext from env) → fail with descriptive error.
  - [x] `fn handle_diff(&self) -> Result<String, GitToolError>` — diff working directory against HEAD (unstaged changes)
  - [x] `fn handle_status(&self) -> Result<String, GitToolError>` — return file statuses (new, modified, deleted, renamed) as formatted text
  - [x] `fn handle_log(&self, max_count: usize) -> Result<String, GitToolError>` — return last N commit messages with short SHA and author
  - [x] Each helper MUST log via `tracing::info!(action = "git_{action}", ...)` before and after operation
  - [x] Each helper MUST convert `git2::Error` → `GitToolError::GitError` with descriptive context

- [x] **1.6** Write unit tests (bottom of file, `#[cfg(test)] mod tests`)
  - [x] `test_git_tool_definition_name` — NAME is "git"
  - [x] `test_git_tool_definition_has_detailed_description` — description contains key action words
  - [x] `test_git_tool_definition_action_enum` — JSON schema `action` field has `"enum"` array with 9 values
  - [x] `test_git_tool_args_deserialize_minimal` — only `action` field
  - [x] `test_git_tool_args_deserialize_full` — all fields populated
  - [x] `test_git_tool_error_is_send_sync` — compile-time assertion
  - [x] `test_git_tool_error_display` — all variants produce descriptive messages
  - [x] `test_git_tool_serializable` — serialize/deserialize round-trip
  - [x] `test_git_tool_invalid_action_returns_error` — unknown action string
  - [x] `test_git_tool_missing_branch_for_checkout` — missing required arg
  - [x] `test_git_tool_missing_message_for_commit` — missing required arg
  - [x] `test_git_tool_init_status_on_new_repo` — init a temp repo, call status
  - [x] `test_git_tool_add_commit_log_roundtrip` — init repo, create file, add, commit, verify in log
  - [x] `test_git_tool_branch_create_and_checkout` — create branch, verify HEAD points to it
  - [x] `test_git_tool_diff_shows_changes` — modify file, call diff, verify output contains change info
  - [x] All tests use `tempfile::TempDir` for isolated repos
  - [x] All tests use `git2::Repository::init()` for setup — never real repos

### Task 2: Implement Filesystem Tool (`src/tools/fs.rs`)

- [x] **2.1** Define `FsTool` struct
  - [x] `#[derive(Debug, Serialize, Deserialize)]`
  - [x] Field: `project_root: PathBuf` — absolute path to project root for security boundary
  - [x] Constructor: `pub fn new(project_root: PathBuf) -> Self`
  - [x] **CRITICAL:** Holds ONLY configuration. Never cache file handles or directory iterators.

- [x] **2.2** Define `FsToolArgs` struct
  - [x] `#[derive(Debug, Deserialize)]`
  - [x] Field: `action: String` — one of: `read`, `write`, `list`, `mkdir`, `delete`, `exists`
  - [x] Field: `path: String` — relative path from project root
  - [x] Field: `content: Option<String>` — file content for write action
  - [x] Field: `recursive: Option<bool>` — for mkdir (create parent dirs) and delete (remove directories)

- [x] **2.3** Define `FsToolError` thiserror enum
  - [x] `InvalidAction { action: String }`
  - [x] `PathDenied { path: String, reason: String }` — path outside project root
  - [x] `NotFound { path: String }`
  - [x] `IoError { action: String, path: String, reason: String }` — wraps std::io::Error
  - [x] `MissingArgument { action: String, argument: String }`

- [x] **2.4** Implement path validation
  - [x] `fn validate_path(&self, requested: &str) -> Result<PathBuf, FsToolError>` — resolve path, canonicalize for existing paths (or parent for new files), verify within `project_root` boundary
  - [x] Security: reject `..` traversal that escapes project root
  - [x] For write/mkdir: validate parent directory exists and is within project root (the target file/dir may not exist yet)

- [x] **2.5** Implement `Tool for FsTool`
  - [x] `const NAME: &'static str = "filesystem"`
  - [x] `type Error = FsToolError`
  - [x] `type Args = FsToolArgs`
  - [x] `type Output = String`
  - [x] `definition()`: JSON schema with `"enum"` constraint on the `action` field listing all 6 valid actions
  - [x] `call()`: match on `args.action` and dispatch to handlers

- [x] **2.6** Implement action handlers
  - [x] `handle_read(path)` — `tokio::fs::read_to_string`, return file content as-is
  - [x] `handle_write(path, content)` — `tokio::fs::write`, create parent dirs if needed, return `"Written {N} bytes to {path}"`
  - [x] `handle_list(path)` — `tokio::fs::read_dir`, return formatted listing: `"[dir] src/\n[file] main.rs (1234 bytes)"`
  - [x] `handle_mkdir(path, recursive)` — `tokio::fs::create_dir` or `create_dir_all`, return `"Created directory {path}"`
  - [x] `handle_delete(path, recursive)` — `tokio::fs::remove_file` or `remove_dir_all`, return `"Deleted {path}"`
  - [x] `handle_exists(path)` — check path existence, return `"exists: true (file)"` / `"exists: true (directory)"` / `"exists: false"`
  - [x] Each handler MUST log via `tracing::info!(action = "fs_{action}", path = %path, ...)`

- [x] **2.7** Write unit tests
  - [x] `test_fs_tool_definition_name` — NAME is "filesystem"
  - [x] `test_fs_tool_definition_has_detailed_description`
  - [x] `test_fs_tool_definition_action_enum` — JSON schema `action` field has `"enum"` array with 6 values
  - [x] `test_fs_tool_args_deserialize_minimal` — action + path only
  - [x] `test_fs_tool_args_deserialize_full` — all fields
  - [x] `test_fs_tool_error_is_send_sync`
  - [x] `test_fs_tool_error_display` — all variants
  - [x] `test_fs_tool_serializable` — round-trip
  - [x] `test_fs_tool_invalid_action`
  - [x] `test_fs_tool_path_denied_outside_root` — path traversal blocked
  - [x] `test_fs_tool_read_existing_file`
  - [x] `test_fs_tool_read_not_found`
  - [x] `test_fs_tool_write_new_file`
  - [x] `test_fs_tool_write_overwrites_existing`
  - [x] `test_fs_tool_write_creates_parent_dirs`
  - [x] `test_fs_tool_list_directory`
  - [x] `test_fs_tool_list_empty_directory`
  - [x] `test_fs_tool_mkdir_single`
  - [x] `test_fs_tool_mkdir_recursive`
  - [x] `test_fs_tool_delete_file`
  - [x] `test_fs_tool_delete_directory_recursive`
  - [x] `test_fs_tool_exists_true_file`
  - [x] `test_fs_tool_exists_true_directory`
  - [x] `test_fs_tool_exists_false`
  - [x] `test_fs_tool_write_missing_content` — returns MissingArgument error
  - [x] All tests use `tempfile::TempDir`

### Task 3: Implement Terminal Tool (`src/tools/terminal.rs`)

- [x] **3.1** Define `TerminalTool` struct
  - [x] `#[derive(Debug, Serialize, Deserialize)]`
  - [x] Field: `working_dir: PathBuf` — default working directory for commands
  - [x] Field: `timeout_secs: u64` — maximum execution time per command (default: 30)
  - [x] Constructor: `pub fn new(working_dir: PathBuf, timeout_secs: u64) -> Self`

- [x] **3.2** Define `TerminalToolArgs` struct
  - [x] `#[derive(Debug, Deserialize)]`
  - [x] Field: `command: String` — shell command to execute
  - [x] Field: `working_dir: Option<String>` — override working directory (relative to project root or absolute)
  - [x] Field: `timeout_secs: Option<u64>` — override default timeout for this command

- [x] **3.3** Define `TerminalToolError` thiserror enum
  - [x] `ExecutionFailed { command: String, reason: String }` — process spawn/IO error
  - [x] `Timeout { command: String, timeout_secs: u64 }` — command exceeded timeout
  - [x] `InvalidWorkingDir { path: String, reason: String }` — specified working dir doesn't exist
  - [x] **NO `NonZeroExit` variant** — non-zero exits are returned as `Ok(output)` with exit code in the output string (see Dev Notes)

- [x] **3.4** Implement `Tool for TerminalTool`
  - [x] `const NAME: &'static str = "terminal"`
  - [x] `type Error = TerminalToolError`
  - [x] `type Args = TerminalToolArgs`
  - [x] `type Output = String`
  - [x] `definition()`: detailed description explaining the tool runs shell commands, has timeout protection, returns combined output with exit code, and that non-zero exit is NOT an error
  - [x] `call()`: execute command via `tokio::process::Command`, always return `Ok` for completed commands regardless of exit code

- [x] **3.5** Implement command execution
  - [x] Use `tokio::process::Command::new("sh").arg("-c").arg(&command)` for shell interpretation
  - [x] Set working directory from `args.working_dir` (if provided) or `self.working_dir`
  - [x] Use `tokio::time::timeout()` to enforce timeout
  - [x] Capture stdout and stderr via `output()` (waits for completion)
  - [x] **Always return `Ok(output)`** for completed commands — include exit code, stdout, and stderr in the output string. Only return `Err` for process spawn failures, timeouts, and invalid working dirs.
  - [x] Cap combined output at ~50KB. If exceeded, truncate with `[... truncated, total {N} bytes]`.
  - [x] Log command before execution: `tracing::info!(action = "terminal_exec", command = %cmd, working_dir = %dir, "Executing command")`
  - [x] Log result after: `tracing::info!(action = "terminal_result", exit_code = %code, stdout_len = %len, "Command completed")`
  - [x] On timeout: `tracing::warn!(action = "terminal_timeout", command = %cmd, timeout_secs = %t, "Command timed out")`

- [x] **3.6** Write unit tests
  - [x] `test_terminal_tool_definition_name` — NAME is "terminal"
  - [x] `test_terminal_tool_definition_has_detailed_description`
  - [x] `test_terminal_tool_args_deserialize_minimal` — command only
  - [x] `test_terminal_tool_args_deserialize_full` — all fields
  - [x] `test_terminal_tool_error_is_send_sync`
  - [x] `test_terminal_tool_error_display` — all variants
  - [x] `test_terminal_tool_serializable` — round-trip
  - [x] `test_terminal_tool_echo_command` — runs `echo hello` and verifies output contains "hello"
  - [x] `test_terminal_tool_exit_code_zero` — runs `true`, exit code 0 in output
  - [x] `test_terminal_tool_nonzero_exit_returns_ok` — runs `false`, verify result is `Ok` with exit code 1 in output
  - [x] `test_terminal_tool_captures_stderr` — runs command that writes to stderr, verify captured in output
  - [x] `test_terminal_tool_working_dir_override` — run `pwd` with specific dir
  - [x] `test_terminal_tool_timeout_kills_process` — run `sleep 60` with 1s timeout, verify Timeout error
  - [x] `test_terminal_tool_invalid_working_dir` — non-existent dir
  - [x] `test_terminal_tool_multiline_output` — runs command producing multiple lines
  - [x] All tests use real shell commands (safe, local-only commands)
  - [x] Tests requiring specific shell behavior use `cfg(unix)` guard

### Task 4: Update Module Registry (`src/tools/mod.rs`)

- [x] **4.1** Update `src/tools/mod.rs`
  - [x] Add public re-exports: `pub mod git;`, `pub mod fs;`, `pub mod terminal;`
  - [x] Re-export key types for ergonomic imports: `pub use git::GitTool;`, `pub use fs::FsTool;`, `pub use terminal::TerminalTool;`
  - [x] Add module-level documentation describing the tools module purpose and its three tools
  - [x] Do NOT add registration helper functions yet — that's Story 4.2 (agent setup)

### Task 5: Integration Verification

- [x] **5.1** Run `cargo check` — zero errors
- [x] **5.2** Run `cargo test` — all new tests pass, all existing tests still pass (zero regressions)
- [x] **5.3** Run `cargo clippy` — zero new warnings (pre-existing dead_code warnings acceptable)
- [x] **5.4** Run `cargo fmt` — all code formatted
- [x] **5.5** Verify each tool can be instantiated and its `definition()` called — confirms rig Tool trait impl compiles and runs

### Task 6: Cargo.toml Dependency Check

- [x] **6.1** Verify no new dependencies needed — all required crates are already present:
  - `rig-core = "0.30"` — Tool trait
  - `git2 = "0.20"` — git operations
  - `tokio = { version = "1", features = ["full"] }` — async fs, process, timeout, spawn_blocking
  - `serde = { version = "1", features = ["derive"] }` — Serialize/Deserialize
  - `serde_json = "1"` — JSON schema for tool definitions
  - `thiserror = "2"` — error enums
  - `tracing = "0.1"` — structured logging
  - `tempfile = "3"` (dev-dependency) — test fixtures

## Dev Notes

### Previous Story Intelligence & Established Patterns

Stories 3.1–3.4 (Epic 3 — Intelligent Supervision) established the rig Tool pattern used throughout this story:
- `AskSupervisor` in `src/supervisor/mod.rs` — canonical reference: `Serialize + Deserialize` struct, dedicated `Args` (Deserialize), dedicated `Error` enum (thiserror), `impl Tool` with `NAME`, `Error`, `Args`, `Output` types
- `ReadFile` in `src/supervisor/read_tool.rs` — canonical reference for path validation (canonicalize + starts_with security boundary). This is the **supervisor's read-only** tool. `FsTool` is the **agent's full read-write** tool. They are intentionally separate — do NOT merge or modify `read_tool.rs`.
- `call()` always logs with `tracing::info!(action = "...")` structured fields
- All errors are typed `thiserror` enums — no `anyhow` in modules
- Tests follow `test_{module}_{behavior}_{scenario}` naming, Arrange → Act → Assert
- Config shared as `Arc<BotConfig>` with `project_root: PathBuf` — this is the path to pass to tool constructors
- Project uses `serde_yml` crate (NOT `serde_yaml` — migrated in early stories), though tools only need `serde` + `serde_json`

### Core Design Principles

**Tool struct rule:** Tool structs hold ONLY configuration data (`PathBuf`, `u64`). Never store open resources like `git2::Repository`, file handles, or process handles. Resources are opened fresh on each `call()`. This is mandatory for `Serialize/Deserialize` round-trips and `Send + Sync` safety.

**Error retry strategy:** Tools do NOT retry internally (MVP). All errors bubble up to the rig agent, which can retry by calling the tool again. This is consistent with architecture Decision 4, Layer 2: from the tool's perspective, all errors are unrecoverable — the agent decides whether and how to retry. [Source: architecture.md#Decision 4: Error Propagation]

**JSON schema `enum` constraint:** All tools use `"enum": [...]` in the JSON schema for the `action` parameter. This constrains the LLM to valid actions at the schema level and prevents `InvalidAction` errors.

### Git Tool — git2 API Reference

The git tool wraps `git2` (libgit2 bindings). All operations open the repo fresh via `Repository::open()`.

**Key patterns (condensed):**
- **open:** `git2::Repository::open(&self.repo_path)?`
- **add:** `repo.index()?.add_all(paths, IndexAddOption::DEFAULT, None)?; index.write()?`
- **commit:** `repo.signature()?` for author, `index.write_tree()?` for tree, `repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &[&parent])?`
- **branch_create:** `repo.branch(name, &commit, false)?` then checkout
- **checkout:** `repo.revparse_ext(branch)?` then `checkout_tree` + `set_head`
- **status:** `repo.statuses(None)?` → iterate entries, map `Status` flags to labels
- **diff:** `repo.diff_index_to_workdir(None, None)?` → `diff.print(DiffFormat::Patch, ...)`
- **log:** `repo.revwalk()?` → push_head, set_sorting(TIME), take(max_count)

**Push credential callback chain** (architecture requires SSH + credential helper support [Source: architecture.md#External Integration Points]):
```rust
callbacks.credentials(|_url, username, allowed_types| {
    if allowed_types.contains(git2::CredentialType::SSH_KEY) {
        return git2::Cred::ssh_key_from_agent(username.unwrap_or("git"));
    }
    if allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT) {
        // Fallback: credential helper or environment-based credentials
        return git2::Cred::credential_helper(&repo_config, _url, username);
    }
    Err(git2::Error::from_str("no suitable credentials found"))
});
```

**Async safety — `spawn_blocking` for network ops:**
git2 is synchronous (C libgit2). Local operations (status, diff, commit, log, add, checkout, branch_create) are sub-millisecond disk I/O — safe to run directly in async context. **Network operations (clone, push) MUST be wrapped in `tokio::task::spawn_blocking()`** to avoid blocking the tokio runtime:
```rust
async fn handle_push_async(&self, remote: &str, branch: &str) -> Result<String, GitToolError> {
    let repo_path = self.repo_path.clone();
    let remote = remote.to_string();
    let branch = branch.to_string();
    tokio::task::spawn_blocking(move || {
        // All synchronous git2 push logic here
    })
    .await
    .map_err(|e| GitToolError::TaskJoinError { reason: e.to_string() })?
}
```

**`clone` context:** Included per AC requirements but unlikely in normal daemon operation — the project repo is always pre-cloned. Don't over-invest in clone edge cases.

**Standardized output formats:**
- **status:** `"M src/main.rs\nA src/tools/git.rs\nD old_file.rs"` (git-style status letters)
- **log:** `"abc1234 | Author Name | 2026-02-07 | commit message summary"` (one line per commit)
- **diff:** Raw unified diff output (same as `git diff`)
- **commit:** `"Committed abc1234: {message}"` (short SHA + message)
- **branch_create:** `"Created and checked out branch '{name}' from {base}"`
- **checkout:** `"Checked out branch '{name}'"`
- **add:** `"Staged {N} file(s)"`
- **push:** `"Pushed branch '{name}' to remote '{remote}'"`
- **clone:** `"Cloned {url} to {path}"`

### Filesystem Tool — Security Boundary Pattern

The filesystem tool MUST enforce a project root boundary. Same pattern as `ReadFile` in `src/supervisor/read_tool.rs` (canonicalize + starts_with check), but extended for write operations.

**Write path validation for non-existent targets:** canonicalize the **parent** directory (which must exist), verify it's within project root, then join the filename:
```rust
fn validate_path_for_new(&self, requested: &str) -> Result<PathBuf, FsToolError> {
    let full_path = self.project_root.join(requested);
    if let Some(parent) = full_path.parent() {
        if parent.exists() {
            let canonical_parent = parent.canonicalize().map_err(|_| FsToolError::PathDenied { ... })?;
            let canonical_root = self.project_root.canonicalize().map_err(|_| FsToolError::PathDenied { ... })?;
            if !canonical_parent.starts_with(&canonical_root) {
                return Err(FsToolError::PathDenied { ... });
            }
            if let Some(file_name) = full_path.file_name() {
                return Ok(canonical_parent.join(file_name));
            }
        }
    }
    Ok(full_path) // Fallback — will fail at IO if truly invalid
}
```

**Standardized output formats:**
- **read:** File content as-is (plain text)
- **write:** `"Written {N} bytes to {path}"`
- **list:** `"[dir] src/\n[file] main.rs (1234 bytes)\n[file] lib.rs (567 bytes)"` (one entry per line)
- **mkdir:** `"Created directory {path}"`
- **delete:** `"Deleted {path}"`
- **exists:** `"exists: true (file)"` / `"exists: true (directory)"` / `"exists: false"`

### Terminal Tool — Design Decisions

**Non-zero exit is NOT an error.** The terminal tool always returns `Ok(output)` for completed commands, regardless of exit code. The `TerminalToolError` enum has NO `NonZeroExit` variant. Rationale:
- Many valid commands return non-zero (`grep` with no matches → 1, `cargo test` on failure → 1)
- The LLM agent needs full output to reason about results
- Returning `Err` would cause rig to stop tool calling, losing context

Only `Err` for: process spawn failure (`ExecutionFailed`), timeout (`Timeout`), invalid working dir (`InvalidWorkingDir`).

**Standardized output format:**
```
Exit code: 0
--- stdout ---
(stdout content)
--- stderr ---
(stderr content, if any)
```

**Output truncation:** Cap combined output at ~50KB. If exceeded, truncate with `[... truncated, total {N} bytes]`.

### Integration with Future Stories (Epic 4)

**Story 4.2** (Agent Session Setup & Chat Loop) will:
- Import `GitTool`, `FsTool`, `TerminalTool` from `tools` module
- Register all 3 tools (plus `AskSupervisor`) via rig's `.tool()` builder method
- Pass `BotConfig.project_root` to `FsTool::new()` and `GitTool::new()`
- Pass `BotConfig.project_root` and a configurable timeout to `TerminalTool::new()`

**Story 4.3** (Pre-Development Preparation) will:
- Use the git tool to create branches (`branch_create` action) and checkout
- Use the filesystem tool to read previous story files
- Use the terminal tool to run `cargo check` and similar validation commands

**Fallback strategy (JB's decision):** If git2 proves too cumbersome for the LLM agent at runtime, the terminal tool can be used as a CLI-based git fallback (`terminal` tool calling `git` commands directly). The git tool implementation should be solid but pragmatic.

### Files Created/Modified in This Story

| File | Change |
|------|--------|
| `src/tools/mod.rs` | **MODIFY** — Replace stub with pub module declarations and re-exports |
| `src/tools/git.rs` | **MODIFY** — Replace stub with full `GitTool`, `GitToolArgs`, `GitToolError`, `impl Tool`, unit tests |
| `src/tools/fs.rs` | **MODIFY** — Replace stub with full `FsTool`, `FsToolArgs`, `FsToolError`, `impl Tool`, unit tests |
| `src/tools/terminal.rs` | **MODIFY** — Replace stub with full `TerminalTool`, `TerminalToolArgs`, `TerminalToolError`, `impl Tool`, unit tests |

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code — only in tests
- ❌ **NO** `anyhow::Result` in tool modules — typed `thiserror` enums only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** panicking in `call()` — always return `Result`
- ❌ **NO** storing open resources (`git2::Repository`, file handles) in tool structs — open fresh on each `call()`
- ❌ **NO** running git2 network operations (clone, push) without `spawn_blocking` — blocks tokio runtime
- ❌ **NO** returning `Err` for non-zero exit codes in terminal tool — always `Ok(output)` with exit code in string
- ❌ **NO** real remote git operations in tests — use `git2::Repository::init()` with `tempfile::TempDir`
- ❌ **NO** hardcoded paths — use `PathBuf` parameters
- ❌ **NO** blocking file I/O in async context — use `tokio::fs` for filesystem operations
- ❌ **NO** modifying `src/supervisor/read_tool.rs` — the supervisor's read-only tool is separate
- ❌ **NO** implementing agent session setup or tool registration — that's Story 4.2
- ❌ **NO** implementing branch naming conventions or pre-dev preparation — that's Story 4.3
- ❌ **NO** adding new crate dependencies — all needed crates are already in Cargo.toml
- ❌ **NO** logging sensitive data (API keys, tokens, file contents with secrets) — log paths and action names only
- ❌ **NO** executing dangerous shell commands in tests — use safe commands like `echo`, `pwd`, `true`, `ls`
- ❌ **NO** internal retry logic in tools — all errors bubble to the agent, which decides on retry

### Scope Boundaries

**IN SCOPE for this story:**
- `src/tools/git.rs` — Full git tool with all 9 actions via git2
- `src/tools/fs.rs` — Full filesystem tool with all 6 actions via tokio::fs
- `src/tools/terminal.rs` — Full terminal tool with shell execution via tokio::process
- `src/tools/mod.rs` — Module declarations and re-exports
- Unit tests for all tools

**OUT OF SCOPE — do NOT implement:**
- Tool registration with rig agent builder (Story 4.2)
- Agent session setup or chat loop (Story 4.2)
- Branch naming conventions or pre-dev preparation (Story 4.3)
- Code review session (Epic 5)
- Any session-level orchestration
- WAL persistence integration
- Credential management beyond git2's built-in credential callbacks

### Testing Requirements

All tests follow established patterns: `test_{tool}_{behavior}_{scenario}`, Arrange → Act → Assert, `tempfile::TempDir` for fixtures.

**Test coverage targets:**
- **git.rs**: ~15 tests — definition (incl. schema enum), args, errors, init/add/commit/log/branch/checkout/diff/status roundtrips
- **fs.rs**: ~21 tests — definition (incl. schema enum), args, errors, read/write/list/mkdir/delete/exists, path security
- **terminal.rs**: ~13 tests — definition, args, errors, echo/exit-code/nonzero-ok/stderr/timeout/working-dir
- **Total**: ~49 new tests, 0 regressions on existing ~323 tests

### Dev Dependencies Required

No new dependencies needed. All required crates are already present in `Cargo.toml`:
- `rig-core = "0.30"` — Tool trait (note: architecture doc references v0.29 but Cargo.toml has v0.30 — follow Cargo.toml)
- `git2 = "0.20"` — git operations
- `tokio` with `full` features — async runtime, fs, process, time, spawn_blocking
- `serde` + `serde_json` — serialization
- `thiserror = "2"` — error enums
- `tracing = "0.1"` — structured logging
- `tempfile = "3"` (dev-dependency) — test fixtures

### Project Structure Notes

After this story, the tools module is fully implemented:

```
src/tools/
├── mod.rs          # Module declarations + pub re-exports (GitTool, FsTool, TerminalTool)
├── git.rs          # GitTool — 9 git operations via git2, spawn_blocking for clone/push
├── fs.rs           # FsTool — 6 filesystem operations with project_root security boundary
└── terminal.rs     # TerminalTool — shell execution with timeout, non-zero exit = Ok
```

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 4.1: Rig Tools Implementation (Git, Filesystem, Terminal)] — Acceptance criteria and user story
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 4: Autonomous Development Session] — "launches a rig agent session with registered tools (git, filesystem, terminal, ask_supervisor)"
- [Source: _bmad-output/planning-artifacts/architecture.md#Rig Tool Implementation Pattern — Standard Structure] — Mandatory tool structure pattern
- [Source: _bmad-output/planning-artifacts/architecture.md#Error Type Pattern — Per-Module thiserror Enums] — Per-module thiserror, no anyhow in library modules
- [Source: _bmad-output/planning-artifacts/architecture.md#Tracing Pattern — Structured Spans with Story Context] — Every tool action logged with `action` field
- [Source: _bmad-output/planning-artifacts/architecture.md#Test Mock Pattern — Deterministic LLM Responses] — Test naming convention, Arrange-Act-Assert
- [Source: _bmad-output/planning-artifacts/architecture.md#Project Structure & Boundaries] — tools/ module layout
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 1: Supervisor Interception Model] — "Tools registered at agent build time via `.tool()`"
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 4: Error Propagation] — Layer 2: tools bubble unrecoverable errors to rig agent
- [Source: _bmad-output/planning-artifacts/architecture.md#Data Flow] — Step 4: "builds rig agent with 4 tools"
- [Source: _bmad-output/planning-artifacts/architecture.md#External Integration Points] — "git2 | SSH key or credential helper"
- [Source: _bmad-output/planning-artifacts/prd.md#Functional Requirements] — FR9: "expose git, filesystem, and terminal tools via rig tool calling"
- [Source: _bmad-output/project-context.md#Framework-Specific Rules] — "Tools exposed to the agent via rig: git, filesystem, terminal"
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — "No silent failures", "No unwrap/expect in production"
- [Source: _bmad-output/project-context.md#Testing Rules] — "Tests inline, #[cfg(test)] mod tests", "Every new module must include basic unit tests"
- [Source: src/supervisor/read_tool.rs] — Reference rig Tool: path validation, canonicalize + starts_with security
- [Source: src/supervisor/mod.rs] — Reference rig Tool: AskSupervisor with action dispatch, tracing, error handling

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

- Initial `cargo check` baseline: 40 pre-existing dead_code warnings, zero errors
- Post-implementation `cargo check`: 56 warnings (all dead_code — tools not yet consumed by Story 4.2), zero errors
- `cargo clippy`: Fixed 3 clippy errors (2× collapsible `if let` in fs.rs, 1× manual loop counter in git.rs). Zero clippy errors remaining.
- `cargo fmt`: Clean after formatting pass
- `cargo test`: 380 passed, 0 failed (323 pre-existing + 57 new tool tests)

### Completion Notes List

- **Task 0**: All prerequisites verified — stubs existed, all dependencies present in Cargo.toml, `cargo check` baseline clean
- **Task 1 (git.rs)**: Implemented `GitTool` with 9 actions via git2. `open_repo()` opens fresh on each call. `clone`/`push` wrapped in `spawn_blocking`. Push uses SSH agent → credential helper fallback chain. Commit falls back to `bmad-bot@localhost` signature if repo config has none. 15 unit tests covering definition, args, errors, serialization, and git operations (init/add/commit/log/branch/checkout/diff/status roundtrips).
- **Task 2 (fs.rs)**: Implemented `FsTool` with 6 actions via tokio::fs. Dual path validation: `validate_path` for existing files (canonicalize + starts_with), `validate_path_for_new` for write/mkdir targets (canonicalize parent). `handle_write` auto-creates parent dirs with ancestor-walking security check. Directory listings sorted alphabetically. 21 unit tests covering all actions + path traversal security.
- **Task 3 (terminal.rs)**: Implemented `TerminalTool` via `sh -c` with tokio timeout. Non-zero exits return `Ok(output)` with exit code in string — no `NonZeroExit` error variant. Output capped at ~50KB with truncation notice. 16 unit tests (shell-dependent tests gated with `#[cfg(unix)]`). Includes `truncate_output` unit tests.
- **Task 4 (mod.rs)**: Updated with `pub mod` + `pub use` re-exports for `GitTool`, `FsTool`, `TerminalTool`. Module-level doc comments describe all three tools.
- **Task 5**: `cargo check` zero errors, `cargo test` 380/380, `cargo clippy` zero new errors, `cargo fmt` clean. All tool definitions callable — verified via test_*_definition_name tests.
- **Task 6**: No new dependencies needed — all crates were already in Cargo.toml.
- **Decision**: `handle_commit` falls back to `git2::Signature::now("bmad-bot", "bmad-bot@localhost")` when repo has no configured user — prevents failures in CI/test environments.

### Change Log

- 2026-02-08: Story 4.1 implementation complete — GitTool (9 actions via git2), FsTool (6 actions via tokio::fs with project-root security boundary), TerminalTool (shell execution with timeout, non-zero exit = Ok). 57 new tests, 0 regressions. All clippy/fmt clean.

### File List

- `src/tools/mod.rs` — MODIFIED: Added pub module declarations and re-exports for GitTool, FsTool, TerminalTool
- `src/tools/git.rs` — MODIFIED: Full GitTool implementation with GitToolArgs, GitToolError, Tool trait impl, 9 git actions, 15 unit tests
- `src/tools/fs.rs` — MODIFIED: Full FsTool implementation with FsToolArgs, FsToolError, path validation, Tool trait impl, 6 fs actions, 21 unit tests
- `src/tools/terminal.rs` — MODIFIED: Full TerminalTool implementation with TerminalToolArgs, TerminalToolError, Tool trait impl, shell execution with timeout, 16 unit tests
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — MODIFIED: Story 4-1 status updated ready-for-dev → in-progress → review