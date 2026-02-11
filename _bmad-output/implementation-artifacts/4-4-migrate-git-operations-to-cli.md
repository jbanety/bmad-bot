# Story 4.4: Migrate All Git Operations from git2 to Git CLI

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want all git operations to use the Git CLI instead of the git2 (libgit2) library,
So that the daemon inherits the user's full git configuration (credential manager, SSH agent, commit signing, `.gitconfig` identity) and eliminates the dual auth path (git2 SSH vs HTTPS token workaround).

## Acceptance Criteria

1. **Given** the daemon starts up, **When** the startup validation runs, **Then** it executes `git --version` and verifies git >= 2.30 is available, **And** it fails fast with a clear, actionable error message if git is missing or too old, **And** this check runs in `cli/mod.rs::run_start()` alongside existing config validation.

2. **Given** the `GitTool` in `src/tools/git.rs` currently uses `git2` for 9 actions (clone, checkout, branch_create, add, commit, push, diff, status, log), **When** the migration is applied, **Then** each action is rewritten to use `tokio::process::Command::new("git")` with appropriate arguments, **And** the working directory is always set explicitly via `-C <path>` or `.current_dir(path)`, **And** both stdout and stderr are captured — stderr included in error messages for LLM-readable diagnostics, **And** `--porcelain` flags are used where available (status, diff) for stable, parseable output, **And** non-zero exit codes are mapped to `GitToolError::CommandFailed` with the full stderr content, **And** output is returned as-is (git CLI output is already human/LLM-readable).

3. **Given** the branch management in `src/session/branch.rs` currently uses `git2` for 3 functions (`determine_base_branch`, `ensure_story_branch`, `checkout_branch`), **When** the migration is applied, **Then** each function is rewritten to use `std::process::Command::new("git")` (sync context, called from `spawn_blocking`), **And** `determine_base_branch()` uses `git branch --list` to check branch existence, **And** `ensure_story_branch()` uses `git checkout -b` (create) or `git checkout` (reuse), **And** `checkout_branch()` uses `git checkout`.

4. **Given** the pipeline push in `src/pipeline.rs` currently uses a hybrid HTTPS token workaround, **When** the migration is applied, **Then** `push_branch()` is simplified to `git push origin <branch>` via `tokio::process::Command`, **And** authentication is inherited from the user's git configuration (SSH agent, credential helper, osxkeychain), **And** the HTTPS URL construction workaround is removed.

5. **Given** the session cleanup in `src/session/cleanup.rs` currently uses `git2` for `preserve_partial_work()` (status, add, commit, branch name detection), **When** the migration is applied, **Then** all git2 calls are replaced with `git` CLI subprocess calls, **And** the best-effort, never-error contract is preserved (every individual git CLI call is guarded and failures are logged but do not propagate).

6. **Given** the session runner in `src/session/runner.rs` currently imports `git2::{BranchType, Repository}` for branch resolution AND uses git2 directly in `resume_session()` for crash recovery branch verification, **When** the migration is applied, **Then** the `git2` imports are removed, **And** the `Repository::open()` + `determine_base_branch()` call in `run()` is replaced with the new CLI-based signature, **And** the inline git2 block in `resume_session()` (~L280-289) that calls `Repository::open()` + `repo.find_branch()` is rewritten to use `git branch --list` via `std::process::Command`.

7. **Given** the git remote detection in `src/cli/git_detect.rs` currently uses `git2::Repository::discover()`, `repo.remotes()`, `repo.find_remote()`, and `repo.head()`, **When** the migration is applied, **Then** all functions are rewritten to use `std::process::Command` with `git remote -v`, `git remote`, `git remote get-url <name>`, and `git rev-parse --abbrev-ref HEAD`, **And** all tests are updated to use CLI-based git fixtures.

8. **Given** all git operations have been migrated, **When** the `git2` crate is no longer referenced anywhere, **Then** `git2` is removed from `Cargo.toml`, **And** compile time and binary size are reduced (libgit2 + libssh2 + OpenSSL transitive C dependencies eliminated).

9. **Given** the migration is complete, **When** existing unit tests are updated, **Then** tests mock CLI output (stdout/stderr + exit code) instead of creating in-memory git2 repositories, **And** all tests pass with the new implementation.

10. **Given** the migration is complete, **When** documentation is updated, **Then** `architecture.md` reflects the completed migration (no longer "amendment" but established pattern).

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [ ] Verify `git` is installed on dev machine and >= 2.30
- [ ] Verify all existing tests pass before migration: `cargo test`
- [ ] Read and understand the Git CLI Subprocess Pattern in architecture.md (L739-780)
- [ ] Read the architect brief: `_bmad-output/planning-artifacts/architect-brief-git-cli-migration.md`
- [ ] Identify all `git2` usage via: `grep -rn "git2" src/` — expected in 6 files:
  - `src/tools/git.rs`
  - `src/session/branch.rs`
  - `src/session/runner.rs`
  - `src/session/cleanup.rs`
  - `src/pipeline.rs`
  - `src/cli/git_detect.rs`

### Task 1: Add Git Version Validation to Daemon Startup (AC: #1)

- [ ] In `src/cli/mod.rs`, add a `validate_git_version()` function
  - [ ] Execute `std::process::Command::new("git").arg("--version").output()`
  - [ ] Parse output: `"git version X.Y.Z"` → extract major.minor
  - [ ] Require >= 2.30 — fail with clear error if missing or too old
  - [ ] Error message must be actionable: "git >= 2.30 required, found X.Y or git not found"
- [ ] Call `validate_git_version()` in `run_start()` alongside existing config validation
- [ ] Add unit tests for version parsing (valid, too old, missing, unexpected format)

### Task 2: Migrate `GitTool` — 9 Actions to CLI (AC: #2)

- [ ] Rewrite `src/tools/git.rs` to remove all `git2` imports
- [ ] Remove the `chrono` import — no longer needed (`git log --oneline` formats dates natively). Note: `chrono` stays in `Cargo.toml` as other modules use it.
- [ ] Keep the existing `GitTool`, `GitToolArgs`, `GitToolError` struct/enum shapes (LLM-facing API-compatible)
- [ ] Add/rename error variant: `CommandFailed { action: String, stderr: String, exit_code: i32 }` (replaces `GitError`)
- [ ] Remove `open_repo()` helper (no longer needed)
- [ ] **Convert all action handlers from `fn` to `async fn`** — `tokio::process::Command` is natively async, so `spawn_blocking` wrappers in `call()` for clone/push are no longer needed. This simplifies `call()` significantly.
- [ ] Rewrite each action handler to use `tokio::process::Command`:

  **clone:**
  - [ ] `git clone <url> <path>`

  **checkout:**
  - [ ] `git -C <repo_path> checkout <branch>`

  **branch_create:**
  - [ ] `git -C <repo_path> checkout -b <branch> [<from_branch>]`

  **add:**
  - [ ] `git -C <repo_path> add <paths...>` (use `"."` for stage-all if paths contains `"*"`)

  **commit:**
  - [ ] `git -C <repo_path> commit -m <message>`
  - [ ] No need to construct signature — git CLI uses `.gitconfig` identity automatically
  - [ ] Commit signing happens automatically if user has it configured

  **push:**
  - [ ] `git -C <repo_path> push <remote> <branch>` (default remote: "origin")
  - [ ] Auth inherited from user's git config — no credential callback needed

  **diff:**
  - [ ] `git -C <repo_path> diff` (unstaged changes, like the current git2 impl)

  **status:**
  - [ ] `git -C <repo_path> status --porcelain` for stable, parseable output

  **log:**
  - [ ] `git -C <repo_path> log --oneline -<max_count>`
  - [ ] Output is already human/LLM-readable

- [ ] All actions: capture stdout + stderr, check `output.status.success()`, map errors
- [ ] Update all unit tests — mock CLI via real git repos in tempdir (git init via CLI, not git2)

### Task 3: Migrate `session/branch.rs` — 3 Functions to CLI (AC: #3)

- [ ] Remove all `git2` imports (`BranchType`, `Repository`, `CheckoutBuilder`)
- [ ] Update module doc comment: remove "git2 is a blocking C library" → "functions are synchronous for use with `spawn_blocking`"
- [ ] Change `determine_base_branch()` signature: replace `repo: &Repository` with `repo_path: &Path`

  **`determine_base_branch(story, repo_path, default_branch) -> String`:**
  - [ ] Use `std::process::Command::new("git").arg("-C").arg(repo_path).args(&["branch", "--list", &candidate])` to check if dependency branch exists
  - [ ] Parse output: non-empty stdout means branch exists
  - [ ] **Error handling approach:** Keep the function infallible (returns `String`). If any CLI call fails, log a warning via `tracing::warn!` and fall back to `default_branch` — preserving the current contract.
  - [ ] Keep the same dependency chaining logic

  **`ensure_story_branch(repo_path, branch_name, base_branch) -> Result<BranchAction, BranchError>`:**
  - [ ] Check existence: `git -C <path> branch --list <branch_name>` — non-empty = exists
  - [ ] If exists: `git -C <path> checkout <branch_name>` → return `BranchAction::Reused`
  - [ ] If not exists: `git -C <path> checkout -b <branch_name> <base_branch>` → return `BranchAction::Created`

  **`checkout_branch(repo_path, branch_name) -> Result<(), BranchError>`:**
  - [ ] Now takes `repo_path: &Path` instead of `&Repository`
  - [ ] `git -C <path> checkout <branch_name>`

- [ ] Update `BranchError` variants — replace git2-specific errors with CLI stderr:
  - [ ] `RepoOpenFailed` → can be removed or replaced with a generic `CommandFailed { command, stderr }`
- [ ] All functions remain **synchronous** (still wrapped in `spawn_blocking` by runner.rs)
- [ ] Update all unit tests — use `git init` + `git commit` CLI commands in tempdir fixtures

### Task 3b: Migrate `cli/git_detect.rs` — Remote Detection to CLI (AC: #7)

- [ ] Remove all `git2` imports (`git2::Repository`, `git2::Signature` in tests)
- [ ] Rewrite `detect_git_remote(project_path)`:
  - [ ] Replace `git2::Repository::discover(project_path)` with `git -C <path> rev-parse --git-dir` to verify git repo exists
  - [ ] Replace `repo.remotes()` with `git -C <path> remote` (lists remote names, one per line)
  - [ ] Parse output to get remote names list
- [ ] Rewrite `detect_git_remote_with_name(project_path, remote_name)`:
  - [ ] Same repo discovery via `git -C <path> rev-parse --git-dir`
  - [ ] Delegate to updated `detect_from_repo` equivalent
- [ ] Rewrite `detect_from_repo()`:
  - [ ] Takes `project_path: &Path` instead of `&git2::Repository`
  - [ ] Uses `git -C <path> remote` to list remotes
  - [ ] Same origin → single-remote → multiple-remotes discovery logic
- [ ] Rewrite `detect_single_remote()`:
  - [ ] Replace `repo.find_remote(name)` + `remote.url()` with `git -C <path> remote get-url <name>`
  - [ ] Parse URL output (trim newline) and feed into existing `parse_git_remote_url()`
- [ ] Rewrite `detect_default_branch()`:
  - [ ] Replace `repo.head()?.shorthand()` with `git -C <path> rev-parse --abbrev-ref HEAD`
  - [ ] Fallback to `"main"` if command fails (preserves current behavior)
- [ ] **URL parsing functions are UNCHANGED** — `parse_git_remote_url()`, `parse_ssh_scheme_url()`, `parse_https_url()`, `parse_owner_repo_from_path()`, `map_host_to_provider()` are pure string parsing and do not use git2
- [ ] Update all 12 git2-dependent tests (L479-677) — replace `git2::Repository::init()` and `git2::Signature` with CLI-based test fixtures:
  - [ ] Use `git init`, `git remote add`, `git config`, `git commit --allow-empty` in tempdir
- [ ] **Pure parsing tests (L309-476) are UNCHANGED** — they don't use git2

### Task 4: Migrate `pipeline.rs::push_branch()` (AC: #4)

- [ ] Remove `git2` import from `pipeline.rs`
- [ ] Remove `git_push_token` field from `StoryPipeline` struct (no longer needed for HTTPS workaround)
- [ ] Update `StoryPipeline::new()` to remove token extraction and `git_push_token` field initialization. Note: the `token` variable is still extracted and passed to `create_provider()` — only the stored copy for git2 push is removed.
- [ ] Rewrite `push_branch()`:
  - [ ] `tokio::process::Command::new("git").arg("-C").arg(&repo_path).args(&["push", "origin", branch]).output().await`
  - [ ] Remove the HTTPS URL construction workaround entirely
  - [ ] Remove the anonymous remote + credential callback pattern
  - [ ] Remove `spawn_blocking` wrapper — `tokio::process::Command` is natively async
  - [ ] Auth is now inherited from user's git config
  - [ ] Map non-zero exit to `PipelineError::PrCreation` with stderr

### Task 5: Migrate `session/cleanup.rs::preserve_partial_work()` (AC: #5)

- [ ] Remove `git2` imports
- [ ] Rewrite `preserve_partial_work()` using CLI commands:
  - [ ] `git -C <path> status --porcelain` → check for changes (non-empty output = dirty)
  - [ ] `git -C <path> add .` → stage all
  - [ ] `git -C <path> commit -m "<WIP message>"` → commit
  - [ ] `git -C <path> branch --show-current` → get current branch name for summary (replaces `repo.head()?.shorthand()` at L119-123)
- [ ] Preserve the best-effort, never-error contract: each CLI call individually guarded with `match`
- [ ] Parse changed files from `--porcelain` output (each line has a 2-char status prefix + space + path)
- [ ] Keep the function async (it's called from async context) — use `tokio::process::Command`
- [ ] Update tests to use CLI-based git fixtures

### Task 6: Update `session/runner.rs` — Remove All git2 Usage (AC: #6)

- [ ] Remove `use git2::{BranchType, Repository};` (L36)
- [ ] **Update `run()` method** (~L569-582) — `determine_base_branch()` call site:
  - [ ] Remove `Repository::open(&repo_path)` and its error match arm
  - [ ] Pass `&repo_path` directly: `let base_branch = determine_base_branch(story, &repo_path, &default_branch);`
  - [ ] This simplifies ~15 lines of error handling into 1 line
- [ ] **Update `resume_session()` method** (~L280-289) — **CRITICAL: this has INLINE git2 code**:
  - [ ] Current code at L283-289 does:
    ```
    let repo = Repository::open(&rp).map_err(...)?;
    repo.find_branch(&bn, BranchType::Local).map_err(...)?;
    ```
  - [ ] Replace with CLI call inside the `spawn_blocking`:
    ```
    let output = std::process::Command::new("git")
        .arg("-C").arg(&rp)
        .args(&["branch", "--list", &bn])
        .output()
        .map_err(|e| format!("git branch --list failed: {e}"))?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err(format!("Recovery branch not found: {bn}"));
    }
    Ok(true)
    ```
- [ ] Verify no other git2 usages remain in the file

### Task 7: Remove `git2` from `Cargo.toml` (AC: #8)

- [ ] Remove `git2 = "0.20"` from `[dependencies]`
- [ ] Run `cargo build` — verify zero compilation errors
- [ ] Run `cargo test` — verify all tests pass
- [ ] Verify `Cargo.lock` no longer contains git2, libgit2-sys, libssh2-sys
- [ ] Confirm `chrono` remains in `Cargo.toml` (used by cli/mod.rs, cli/state.rs, session/state.rs, session/escalation.rs, supervisor/decisions.rs)

### Task 8: Update Tests (AC: #9)

- [ ] Replace all `git2::Repository::init()` test fixtures with CLI-based init (see test fixture pattern below)
- [ ] Replace all `git2::Signature` / `repo.commit()` test fixtures with `git commit --allow-empty` CLI calls
- [ ] Files with git2 test fixtures to update:
  - `src/tools/git.rs` — `init_repo_with_commit()` helper + 7 integration tests
  - `src/session/branch.rs` — `init_test_repo()` + `create_branch_with_commit()` helpers + 11 tests
  - `src/session/cleanup.rs` — `init_test_repo()` helper + related tests
  - `src/cli/git_detect.rs` — 12 tests using `git2::Repository::init()` + `git2::Signature`
- [ ] Keep `tempfile` dev-dependency (still used for tempdir fixtures)
- [ ] Test coverage targets remain ~15 tests for branch.rs, full coverage for git.rs actions, full coverage for git_detect.rs

### Task 9: Final Verification (AC: #8, #10)

- [ ] `cargo build` — clean compile, no warnings
- [ ] `cargo test` — all tests pass
- [ ] `cargo clippy` — no warnings
- [ ] `cargo fmt --check` — properly formatted
- [ ] `grep -rn "git2" src/` — returns zero results
- [ ] Verify binary size reduction (compare before/after)

## Dev Notes

### ⚠️ Cross-Cutting Migration — Touches 7 Files Across 4 Modules

This story is a **cross-cutting refactoring** that touches code originally established by multiple stories:
- `src/tools/git.rs` — Story 4.1 (Epic 4) — GitTool with 9 actions
- `src/session/branch.rs` — Story 4.3 (Epic 4) — branch management
- `src/session/runner.rs` — Story 4.2 (Epic 4) — session lifecycle + crash recovery
- `src/session/cleanup.rs` — Story 3.3 area (Epic 3 / escalation) — partial work preservation
- `src/pipeline.rs` — Story 5.1 area (Epic 5 / PR delivery) — push branch
- `src/cli/git_detect.rs` — Story 1.5 (Epic 1) — git remote auto-detection for init
- `src/cli/mod.rs` — Story 1.1 (Epic 1) — add git version validation

After this story, **zero files** should reference `git2`.

### Triggered By: Production Incident (2026-02-10)

The daemon's `push_branch()` failed because `git2` cannot access the SSH agent when the daemon runs as a background process. A hotfix (commits `62929b2` and `eaafa28`) added an HTTPS token workaround, creating a fragile dual-auth path. This story eliminates the root cause.

### Git CLI Subprocess Pattern (from Architecture L739-780)

All git CLI calls MUST follow this pattern:

**Async context** (GitTool actions, pipeline push, cleanup):
```
let output = tokio::process::Command::new("git")
    .arg("-C").arg(&self.repo_path)
    .args(&["status", "--porcelain"])
    .output()
    .await?;

if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(GitToolError::CommandFailed {
        action: "status".to_string(),
        stderr: stderr.into(),
        exit_code: output.status.code().unwrap_or(-1),
    });
}
let stdout = String::from_utf8_lossy(&output.stdout);
```

**Sync context** (session/branch.rs, cli/git_detect.rs — called from `spawn_blocking` or sync init):
```
let output = std::process::Command::new("git")
    .arg("-C").arg(repo_path)
    .args(&["checkout", "-b", &branch_name, &base_branch])
    .output()?;
```

**Mandatory rules:**
- Always use `-C <path>` or `.current_dir(path)` — never rely on process-level cwd
- Capture both stdout and stderr — include stderr in error messages for LLM-readable diagnostics
- Use `--porcelain` flags where available (status, diff) for stable, parseable output
- Use `tokio::process::Command` in async contexts, `std::process::Command` in sync contexts
- Check `output.status.success()` — map non-zero exit codes to the module's thiserror enum

### git2 Action → CLI Mapping Reference

| git2 call | CLI equivalent | Context |
|-----------|---------------|---------|
| `Repository::clone(&url, &path)` | `git clone <url> <path>` | async |
| `repo.revparse_ext(branch)` + `checkout_tree` + `set_head` | `git -C <path> checkout <branch>` | async |
| `repo.branch(name, &commit, false)` + checkout | `git -C <path> checkout -b <name> [<from>]` | async |
| `index.add_all(paths)` + `index.write()` | `git -C <path> add <paths...>` | async |
| `repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents)` | `git -C <path> commit -m <msg>` | async |
| `remote.push(&[refspec], Some(&mut opts))` | `git -C <path> push <remote> <branch>` | async |
| `repo.diff_index_to_workdir(None, None)` | `git -C <path> diff` | async |
| `repo.statuses(None)` | `git -C <path> status --porcelain` | async |
| `revwalk` + `find_commit` loop | `git -C <path> log --oneline -<n>` | async |
| `repo.find_branch(&name, Local).is_ok()` | `git -C <path> branch --list <name>` | sync |
| `repo.set_head()` + `checkout_head()` | `git -C <path> checkout <branch>` | sync |
| `git2::Repository::discover(path)` | `git -C <path> rev-parse --git-dir` | sync |
| `repo.remotes()` | `git -C <path> remote` | sync |
| `repo.find_remote(name)` + `remote.url()` | `git -C <path> remote get-url <name>` | sync |
| `repo.head()?.shorthand()` | `git -C <path> rev-parse --abbrev-ref HEAD` | sync |

### Previous Story Intelligence (Story 4.3)

**Story 4.3** (Pre-Development Preparation & Branch Management) established:
- `determine_base_branch(story, repo, default_branch) -> String` — currently takes `&Repository`, must change to `&Path`
- `ensure_story_branch(repo_path, branch_name, base_branch) -> Result<BranchAction, BranchError>` — sync, wrapped in `spawn_blocking`
- `BranchAction::Created { branch_name, base_branch }` and `BranchAction::Reused { branch_name }`
- `BranchError` enum with 4 variants: `CreationFailed`, `CheckoutFailed`, `BaseBranchNotFound`, `RepoOpenFailed`
- Dependency-aware branch chaining logic (last dependency's branch if exists, else default)
- Tests use `git2::Repository::init()` + `git2::Signature` for fixtures — must be replaced

**Key signature change:** `determine_base_branch()` second parameter changes from `&Repository` to `&Path`. This impacts two call sites in `runner.rs`:
1. `run()` at ~L569: Remove `Repository::open()` match, pass `&repo_path` directly
2. `resume_session()` does NOT call `determine_base_branch()` — but has its own inline git2 block (see below)

### Previous Story Intelligence (Story 4.1)

**Story 4.1** (Rig Tools) established the `GitTool` with git2:
- `GitTool::new(repo_path: PathBuf)` — constructor remains the same
- `open_repo()` helper — **DELETE** (no longer needed with CLI)
- Sync action handlers wrapped in `spawn_blocking` for network ops (clone, push) — **REMOVE** wrappers, make handlers `async fn`
- `GitToolArgs` with `action: String` multiplexer — keep this shape, it's the LLM-facing API
- `GitToolError` enum — update variants to reflect CLI errors
- `TaskJoinError` variant — can be removed if no more `spawn_blocking` in git.rs (all handlers become async). Keep only if some edge case still needs it.

### runner.rs — Two Separate git2 Usages to Migrate

`src/session/runner.rs` has **TWO distinct git2 usage sites**:

**Site 1 — `run()` method (~L569-582):** Uses `Repository::open()` + `determine_base_branch()` for initial branch resolution. After migration, simplifies to a single line since `determine_base_branch()` now takes `&Path`.

**Site 2 — `resume_session()` method (~L280-289):** Uses `Repository::open()` + `repo.find_branch()` DIRECTLY (not via branch.rs) for crash recovery branch verification. This inline git2 code must be rewritten to use `git branch --list` via `std::process::Command` inside the existing `spawn_blocking` closure.

### Pipeline HTTPS Workaround Removal

**Current state in `pipeline.rs::push_branch()`** (~L447-512):
- Constructs HTTPS URL from config (`repo_owner`/`repo_name`)
- Creates anonymous git2 remote with that URL
- Uses `git2::Cred::userpass_plaintext("x-access-token", &token)` for auth
- `git_push_token` field on `StoryPipeline` stores the token

**After migration:** `git push origin <branch>` — inherits whatever auth the user configured. The `git_push_token` field is removed from `StoryPipeline`. The `token` variable in `new()` (L145-167) is still extracted for `create_provider()` — only the stored copy for git2 push is removed.

### git_detect.rs — URL Parsing is Untouched

`src/cli/git_detect.rs` has two categories of code:
1. **git2-dependent functions** (must migrate): `detect_git_remote()`, `detect_git_remote_with_name()`, `detect_from_repo()`, `detect_single_remote()`, `detect_default_branch()`
2. **Pure string parsing** (unchanged): `parse_git_remote_url()`, `parse_ssh_scheme_url()`, `parse_https_url()`, `parse_owner_repo_from_path()`, `map_host_to_provider()`

The key insight: `detect_single_remote()` currently gets the URL via `repo.find_remote(name)?.url()`. After migration, it gets the URL via `git remote get-url <name>`. Once the URL string is obtained, it's fed into the existing `parse_git_remote_url()` pipeline — no parsing changes needed.

### cleanup.rs — The Often-Overlooked git2 User

`preserve_partial_work()` in `src/session/cleanup.rs` uses git2 for 4 operations:
1. `git2::Repository::open(repo_path)` → `git -C <path> rev-parse --git-dir` (or just proceed with commands)
2. `git2::StatusOptions` + `repo.statuses()` → `git -C <path> status --porcelain`
3. `index.add_all()` + `index.write()` + `repo.commit()` → `git -C <path> add .` + `git -C <path> commit -m "..."`
4. `repo.head()?.shorthand()` → `git -C <path> branch --show-current`

The function is best-effort (never returns errors), so each CLI call should be individually guarded.

### Test Fixture Pattern — CLI-Based

Replace all `git2::Repository::init()` test fixtures with:

```rust
fn init_test_repo(dir: &Path) {
    // Initialize repo
    let output = std::process::Command::new("git")
        .args(&["init", dir.to_str().unwrap()])
        .output()
        .expect("git init");
    assert!(output.status.success());

    // Set identity for commits (required in CI/test environments)
    std::process::Command::new("git")
        .arg("-C").arg(dir)
        .args(&["config", "user.email", "test@test.com"])
        .output().expect("git config email");
    std::process::Command::new("git")
        .arg("-C").arg(dir)
        .args(&["config", "user.name", "Test"])
        .output().expect("git config name");

    // Rename default branch to main
    std::process::Command::new("git")
        .arg("-C").arg(dir)
        .args(&["branch", "-M", "main"])
        .output().expect("git branch rename");

    // Create initial empty commit (branch operations need at least one commit)
    std::process::Command::new("git")
        .arg("-C").arg(dir)
        .args(&["commit", "--allow-empty", "-m", "initial"])
        .output().expect("git commit");
}
```

For `git_detect.rs` tests, also add remote setup:
```rust
fn add_remote(dir: &Path, name: &str, url: &str) {
    let output = std::process::Command::new("git")
        .arg("-C").arg(dir)
        .args(&["remote", "add", name, url])
        .output()
        .expect("git remote add");
    assert!(output.status.success());
}
```

### Cargo.toml Cleanup

Current git2 entry:
```toml
git2 = "0.20"
```

After removal, verify `Cargo.lock` no longer pulls:
- `git2`
- `libgit2-sys`
- `libssh2-sys`
- `openssl-sys` (only if no other crate depends on it)

`chrono` remains — used by: `cli/mod.rs`, `cli/state.rs`, `session/state.rs`, `session/runner.rs`, `session/escalation.rs`, `supervisor/decisions.rs`.

Expected benefit: significant compile time reduction (libgit2 C build) and smaller binary.

### Error Type Evolution

**GitToolError** — replace `GitError` variant:
```rust
// BEFORE (git2-based):
#[error("Git {action} failed: {reason}")]
GitError { action: String, reason: String }

// AFTER (CLI-based):
#[error("Git {action} failed (exit code {exit_code}): {stderr}")]
CommandFailed { action: String, stderr: String, exit_code: i32 }
```

Keep `InvalidAction`, `MissingArgument`, `PathError`. The `TaskJoinError` variant can be removed if all handlers become `async fn` (no more `spawn_blocking` inside git.rs). If removed, simplify `call()` accordingly.

**BranchError** — consider replacing `RepoOpenFailed` with:
```rust
CommandFailed { command: String, stderr: String }
```

### Anti-Patterns to Avoid

- ❌ **NO** using `git2` anywhere — the whole point is to remove it completely
- ❌ **NO** constructing HTTPS URLs or credential callbacks — let git CLI handle auth
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** `unwrap()` or `expect()` in production code — only in tests
- ❌ **NO** relying on process-level cwd — always use `git -C <path>` or `.current_dir(path)`
- ❌ **NO** changing the LLM-facing tool API (keep `GitToolArgs` shape and action names identical)
- ❌ **NO** changing `BranchAction` enum shape (keep `Created`/`Reused` variants)
- ❌ **NO** modifying any file under `_bmad/` — daemon is read-only consumer
- ❌ **NO** modifying `sprint-status.yaml` — daemon reads only
- ❌ **NO** changing the `ensure_story_branch` contract (still sync, still returns `Result<BranchAction, BranchError>`)
- ❌ **NO** breaking the best-effort contract of `preserve_partial_work()` — it must never return an error
- ❌ **NO** changing `detect_git_remote()` return types — `GitDetectionResult` enum and `GitRemoteInfo` struct stay identical
- ❌ **NO** modifying pure URL parsing functions in git_detect.rs — they don't use git2

### Scope Boundaries

**IN SCOPE:**
- `src/tools/git.rs` — Full rewrite of 9 action handlers from git2 to async CLI
- `src/session/branch.rs` — Rewrite 3 functions from git2 to sync CLI
- `src/session/cleanup.rs` — Rewrite git2 calls in `preserve_partial_work()` to CLI, including branch name detection
- `src/session/runner.rs` — Remove git2 imports, update `run()` call site, rewrite `resume_session()` inline git2 block
- `src/pipeline.rs` — Rewrite `push_branch()`, remove HTTPS workaround and `git_push_token`
- `src/cli/git_detect.rs` — Rewrite 5 git2-dependent functions to CLI, update 12 tests
- `src/cli/mod.rs` — Add git version validation at startup
- `Cargo.toml` — Remove `git2 = "0.20"`
- All associated unit tests across all 7 affected files

**OUT OF SCOPE — do NOT implement:**
- New git operations not in the current 9-action set
- Changes to `GitToolArgs` field names or action strings
- Changes to `GitRemoteInfo`, `GitDetectionResult` types
- Changes to URL parsing functions in git_detect.rs
- Changes to supervisor, watcher, config, notifier, review, or tools other than git
- CI/CD pipeline setup
- Integration/E2E tests (Epic 7)
- Any changes to BMAD files under `_bmad/`

### Project Structure Notes

Files modified by this story:

```
src/
├── cli/
│   ├── mod.rs              # MODIFY — add validate_git_version() + call in run_start()
│   └── git_detect.rs       # MODIFY — rewrite 5 functions from git2 to std::process::Command
├── tools/
│   └── git.rs              # MAJOR REWRITE — 9 actions from git2 to async tokio::process::Command
├── session/
│   ├── branch.rs           # REWRITE — 3 functions from git2 to std::process::Command
│   ├── runner.rs           # MODIFY — remove git2 imports, update run() + rewrite resume_session() inline git2
│   └── cleanup.rs          # MODIFY — replace git2 calls with CLI in preserve_partial_work()
├── pipeline.rs             # MODIFY — rewrite push_branch(), remove HTTPS workaround + git_push_token field
Cargo.toml                  # MODIFY — remove git2 = "0.20"
```

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 4.4] — Full acceptance criteria and user story
- [Source: _bmad-output/planning-artifacts/architect-brief-git-cli-migration.md] — Complete rationale, scope, trade-offs, migration mapping
- [Source: _bmad-output/planning-artifacts/architecture.md#Git CLI Subprocess Pattern] (L739-780) — Mandatory CLI call pattern
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 7] — Surgical tooling amendment noting git2 → Git CLI
- [Source: _bmad-output/planning-artifacts/architecture.md#Rig Tool Implementation Pattern] (L577-634) — Tool struct/args/error pattern
- [Source: _bmad-output/project-context.md#Technology Stack] — "Git Operations: Git CLI (>= 2.30) via subprocess"
- [Source: _bmad-output/project-context.md#CLI Rules] — "git validation: bmad-bot start verifies git --version >= 2.30"
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — Never rewrite entire files, BMAD files sacred
- [Source: _bmad-output/implementation-artifacts/4-3-pre-development-preparation-branch-management.md#Dev Notes] — branch.rs design, dependency chaining, spawn_blocking usage
- [Source: src/tools/git.rs] — Current git2-based GitTool (9 actions, ~477 lines impl, ~374 lines tests)
- [Source: src/tools/git.rs#L479-652] — Tool trait impl with spawn_blocking wrappers for clone/push
- [Source: src/session/branch.rs#L89-226] — Current git2-based branch management (3 functions)
- [Source: src/session/branch.rs#L229-523] — branch.rs tests using git2 fixtures
- [Source: src/session/runner.rs#L36] — `use git2::{BranchType, Repository}` import to remove
- [Source: src/session/runner.rs#L569-620] — run() branch setup using Repository::open + determine_base_branch
- [Source: src/session/runner.rs#L280-289] — resume_session() INLINE git2 code: Repository::open + find_branch for crash recovery
- [Source: src/session/runner.rs#L330-360] — resume_session() ensure_story_branch call (via branch.rs, signature unchanged)
- [Source: src/session/cleanup.rs#L38-140] — preserve_partial_work() using git2 for status/add/commit/branch-name
- [Source: src/pipeline.rs#L115-128] — StoryPipeline struct with git_push_token field
- [Source: src/pipeline.rs#L130-177] — StoryPipeline::new() constructor extracting token
- [Source: src/pipeline.rs#L447-512] — push_branch() with HTTPS token workaround
- [Source: src/cli/git_detect.rs#L110-216] — 5 functions using git2 for remote detection
- [Source: src/cli/git_detect.rs#L479-677] — 12 tests using git2::Repository::init + git2::Signature
- [Source: Cargo.toml#L9] — `git2 = "0.20"` dependency to remove
- [Source: git log] — Hotfix commits `62929b2` (push_branch) and `eaafa28` (HTTPS auth workaround)

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List