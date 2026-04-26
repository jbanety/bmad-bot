# Story 7.8: Branch Management & Git Tools Integration Tests

Status: abandoned

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want integration tests that verify branch creation, base branch resolution, and git tool operations on real (temp) repositories,
So that I'm confident the daemon manages git state correctly across module boundaries.

## Acceptance Criteria

1. **Given** a temp git repo with a `main` branch and an initial commit
   **When** `ensure_story_branch("story/1-2-cli", "main")` is called
   **Then** a new branch `story/1-2-cli` is created from `main`
   **And** the repo HEAD is on `story/1-2-cli`

2. **Given** a temp git repo where branch `story/1-2-cli` already exists
   **When** `ensure_story_branch("story/1-2-cli", "main")` is called again
   **Then** the existing branch is checked out (not duplicated)
   **And** no error is returned

3. **Given** a `StoryInfo` with dependencies `["1-1-scaffolding"]`
   **And** a temp git repo with branches `main` and `story/1-1-scaffolding`
   **When** `determine_base_branch()` is called
   **Then** it returns `"story/1-1-scaffolding"` (last dependency's branch)

4. **Given** a `StoryInfo` with no dependencies
   **When** `determine_base_branch()` is called
   **Then** it returns the default branch (`"main"`)

5. **Given** a temp git repo with uncommitted changes
   **When** `preserve_partial_work()` is called
   **Then** all changes are staged and committed with a descriptive message containing the story key
   **And** the commit exists in the repo's log

## Tasks / Subtasks

- [ ] Task 0: Ensure `src/lib.rs` prerequisite (AC: ALL — BLOCKER)
  - [ ] 0.1 If `src/lib.rs` does not exist (Story 7.1 Task 0 not yet done), create it with `pub mod` declarations for all modules: `config`, `git_provider`, `notifier`, `pipeline`, `review`, `session`, `supervisor`, `tools`, `watcher`
  - [ ] 0.2 Update `src/main.rs` — remove corresponding `mod X;` lines (keep `mod cli;`) and import from `bmad_bot::*`
  - [ ] 0.3 Verify `cargo build` + `cargo test` pass with all existing unit tests

- [ ] Task 1: Create integration test file structure (AC: ALL)
  - [ ] 1.1 If `tests/integration.rs` does not exist yet, create it as the Cargo test binary entry point
  - [ ] 1.2 If `tests/integration/helpers/` does not exist, create the directory structure
  - [ ] 1.3 Create `tests/integration/test_branch_git.rs` for all Story 7.8 tests
  - [ ] 1.4 Declare `mod test_branch_git;` in `tests/integration.rs`

- [ ] Task 2: Implement `ensure_story_branch` integration tests (AC: #1, #2)
  - [ ] 2.1 Test: create a new branch from `main` on a temp repo → verify `BranchAction::Created`, HEAD is on new branch
  - [ ] 2.2 Test: call `ensure_story_branch` twice → second call returns `BranchAction::Reused`, no error
  - [ ] 2.3 Test: create branch from a non-main parent branch → verify branch is created from the correct base commit
  - [ ] 2.4 Test: create branch when base branch does not exist → verify `BranchError::BaseBranchNotFound`
  - [ ] 2.5 Test: call on non-git directory → verify `BranchError::RepoOpenFailed`

- [ ] Task 3: Implement `determine_base_branch` integration tests (AC: #3, #4)
  - [ ] 3.1 Test: story with no dependencies → returns `"main"`
  - [ ] 3.2 Test: story with one dependency whose branch exists locally → returns `"story/{dep_key}"`
  - [ ] 3.3 Test: story with one dependency whose branch does NOT exist → returns `"main"` (fallback)
  - [ ] 3.4 Test: story with multiple dependencies → uses LAST dependency, returns its branch if exists

- [ ] Task 4: Implement end-to-end branch flow integration tests (AC: #1, #3, #4)
  - [ ] 4.1 Test: full flow — `determine_base_branch` → `ensure_story_branch` → verify repo state is correct (chained from dependency)
  - [ ] 4.2 Test: full flow — multi-story chain — create story/1-1, then story/1-2 from 1-1, then story/1-3 from 1-2 → verify each branch's parent commit is correct
  - [ ] 4.3 Test: full flow — dependency branch missing (merged to main) → falls back to main → ensure_story_branch creates from main

- [ ] Task 5: Implement GitTool integration tests (AC: #1, #5) — LOCAL ACTIONS ONLY (no push, no clone)
  - [ ] 5.1 Test: `branch_create` + `checkout` → verify branch exists and HEAD is on it
  - [ ] 5.2 Test: `add` + `commit` → verify commit exists in `log` output
  - [ ] 5.3 Test: `status` on dirty tree → shows modified/new files; `status` on clean tree → "Clean working directory"
  - [ ] 5.4 Test: `diff` shows uncommitted changes
  - [ ] 5.5 Test: full roundtrip — `branch_create` → write files → `add` → `commit` → `log` → verify commit message and SHA

- [ ] Task 6: Implement `preserve_partial_work` integration tests (AC: #5)
  - [ ] 6.1 Test: dirty tree with uncommitted files → WIP commit is created, summary contains "WIP commit: yes" and file names
  - [ ] 6.2 Test: clean tree → no commit created, summary contains "no (clean tree)"
  - [ ] 6.3 Test: preserve_partial_work on a branch created by `ensure_story_branch` → verify the WIP commit exists on the story branch and the commit message contains the story key

- [ ] Task 7: Implement cross-module integration tests (AC: ALL)
  - [ ] 7.1 Test: full lifecycle — create `StoryInfo` → `determine_base_branch` → `ensure_story_branch` → use `GitTool` to write+add+commit on that branch → `preserve_partial_work` on additional dirty changes → verify both commits exist on the story branch
  - [ ] 7.2 Test: cross-module consistency — create branch via `ensure_story_branch("story/x", "main")` → switch HEAD back to main via `GitTool::call(checkout, branch="main")` → switch back to story branch via `GitTool::call(checkout, branch="story/x")` → verify HEAD is on `story/x` and working directory is consistent

## Dev Notes

### Cross-Module Integration Value

This story tests the **interaction between three modules** that manage git state for the daemon:

| Module | Responsibility | Key Functions |
|--------|---------------|---------------|
| `session/branch.rs` | Pre-session branch setup | `determine_base_branch()`, `ensure_story_branch()` |
| `tools/git.rs` | Agent git operations during session | `GitTool::call()` — branch_create, checkout, add, commit, status, diff, log |
| `session/cleanup.rs` | Post-session partial work preservation | `preserve_partial_work()` |

**Why integration tests matter here:** Unit tests already cover each function in isolation. Integration tests verify that these three modules produce **consistent git state** when operating on the same repository — e.g., a branch created by `ensure_story_branch` can be operated on by `GitTool`, and `preserve_partial_work` correctly commits on that branch.

### Architecture Compliance

#### 🚨 CRITICAL — `src/lib.rs` Prerequisite (from Story 7.1 Task 0)

**The project is currently a pure binary crate** — `src/main.rs` only, no `src/lib.rs`. Without `lib.rs`, `use bmad_bot::anything;` will NOT compile. See Story `7-1-integration-test-infrastructure-fixtures.md` Task 0 for full instructions. Summary: create `src/lib.rs` with `pub mod` for all modules except `cli`, update `src/main.rs` to remove `mod X;` lines (keep `mod cli;`), verify `cargo build` + `cargo test` pass.

#### Module Visibility — ✅ Confirmed Public

All required modules are **already declared `pub`** in their parent modules (verified from source):

- ✅ `session/mod.rs` declares `pub mod branch;` and `pub mod cleanup;` — accessible as `bmad_bot::session::branch::*` and `bmad_bot::session::cleanup::*`
- ✅ `tools/mod.rs` declares `pub mod git;` + `pub use git::GitTool;` — accessible as `bmad_bot::tools::git::*`
- ✅ `watcher/mod.rs` exports `pub struct StoryInfo` — accessible as `bmad_bot::watcher::StoryInfo`

**No visibility changes needed.** Once `lib.rs` exists, all types are accessible from integration tests.

#### 🚨 CRITICAL — `use rig::tool::Tool;` Required for GitTool Tests

To call `tool.call(args).await` on a `GitTool` instance, the `Tool` trait MUST be in scope. Without this import, the compiler emits a confusing "method not found" error. Every test file that uses `GitTool::call()` needs:

```rust
use rig::tool::Tool; // REQUIRED — GitTool::call() comes from this trait
```

The `rig-core` crate is in `[dependencies]` (not just dev-deps), so it's accessible from integration tests.

#### 🚨 CRITICAL — Do NOT Test `push` or `clone` Actions

The `GitTool` actions `push` and `clone` require a configured remote. Temp repos have no remote. **Only test local actions:** `branch_create`, `checkout`, `add`, `commit`, `status`, `diff`, `log`. Network git operations are covered separately in Story 7.6 (Git Provider) via mock HTTP.

#### Integration Test Location

All tests go in `tests/integration/test_branch_git.rs`, declared in `tests/integration.rs`:
```rust
mod helpers;
mod test_branch_git;
```

If `tests/integration.rs` doesn't exist yet (Story 7.1 not implemented), create the minimal structure:
```rust
mod helpers;
mod test_branch_git;
```

If `helpers/` doesn't exist yet, create a minimal `tests/integration/helpers/mod.rs` with what this story needs (just the `create_test_repo` helper, or inline it in the test file).

### Technical Requirements

#### Quick API Reference

| Function | Module | Sync/Async | Signature Summary |
|----------|--------|-----------|-------------------|
| `determine_base_branch` | `session::branch` (L104) | **Sync** | `(story: &StoryInfo, repo_path: &Path, default_branch: &str) -> String` |
| `ensure_story_branch` | `session::branch` (L146) | **Sync** | `(path: &Path, branch: &str, base: &str) -> Result<BranchAction, BranchError>` |
| `preserve_partial_work` | `session::cleanup` (L38) | **Async** | `(path: &Path, key: &str, question: &str) -> String` |
| `GitTool::new` | `tools::git` (L93) | Sync | `(repo_path: PathBuf) -> Self` |
| `GitTool::call` | `tools::git` (L540) | **Async** | `(args: GitToolArgs) -> Result<String, GitToolError>` |

**Key types:** `BranchError` (4 variants: `CreationFailed`, `CheckoutFailed`, `BaseBranchNotFound`, `RepoOpenFailed`), `BranchAction` (`Created { branch_name, base_branch }`, `Reused { branch_name }`), `GitToolArgs` (8 fields, only `action` required — all others `Option`), `StoryInfo` (9 pub fields). See source references at bottom for exact definitions.

#### API Behavior Notes

**`determine_base_branch` takes `&Path`, NOT `&Repository`.** Post git2→CLI migration (Story 4.4), all branch functions operate on paths directly. No need to open a git2 repository object. Both `determine_base_branch` and `ensure_story_branch` take `&Path`.

**`preserve_partial_work` NEVER returns an error.** It returns a `String` summary — always. On failure, it returns a fallback message like `"Preservation failed — could not open repo: ..."`. Tests must assert on string content (`summary.contains("WIP commit: yes")`) — there is no `Result` to unwrap.

**`GitTool::call` is async** — all GitTool tests need `#[tokio::test]`. Sync functions (`determine_base_branch`, `ensure_story_branch`) can use `#[test]` or `#[tokio::test]`.

#### 🚨 `GitToolArgs` Has No `Default` — Use Helper

`GitToolArgs` has 8 fields and does NOT derive `Default`. To avoid 8 lines of boilerplate per call, define this helper in the test file:

```rust
/// Build GitToolArgs with only the fields needed — all others default to None.
fn git_args(action: &str) -> bmad_bot::tools::git::GitToolArgs {
    bmad_bot::tools::git::GitToolArgs {
        action: action.to_string(),
        branch: None,
        message: None,
        paths: None,
        url: None,
        remote: None,
        max_count: None,
        from_branch: None,
    }
}
```

Usage pattern — struct update syntax does not work (no `Default`), but field override does:
```rust
let args = bmad_bot::tools::git::GitToolArgs {
    branch: Some("story/1-2-cli".to_string()),
    ..git_args("branch_create")
};
// ERROR: ^^^ struct update syntax requires Default. Instead do:
let mut args = git_args("branch_create");
args.branch = Some("story/1-2-cli".to_string());
```

#### 🚨 Do NOT Use `StoryInfo::from_key_and_status()` for Dependency Tests

The public constructor `StoryInfo::from_key_and_status(key, status, story_dir)` (L96-137 in `watcher/mod.rs`) always sets `dependencies: Vec::new()`. It **cannot** specify dependencies. Using it for AC #3/#4 tests will silently produce a story with no deps, making `determine_base_branch` always return `"main"`. Use the manual `make_story` helper below instead.

#### Git CLI Temp Repo Setup Pattern

> **Post Story 4.4:** `git2` has been removed from the project entirely. All git operations — including test fixtures — use Git CLI subprocess calls.

All tests use this pattern (aligned with production code post Story 4.4):

```rust
fn init_test_repo(dir: &Path) {
    use std::process::Command;
    let run = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command failed");
        assert!(output.status.success(), "git {} failed: {}",
            args.join(" "), String::from_utf8_lossy(&output.stderr));
    };
    run(&["init"]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    run(&["commit", "--allow-empty", "-m", "initial commit"]);
    // Ensure "main" branch exists (default might be "master" depending on git config)
    run(&["branch", "-M", "main"]);
}
```

**If Story 7.1's `create_test_repo` helper already exists** in `tests/integration/helpers/fixtures.rs`, use that instead and skip the inline definition.

#### StoryInfo Construction Helper

Use this helper — NOT `StoryInfo::from_key_and_status()` — because it supports setting dependencies:

```rust
fn make_story(key: &str, deps: Vec<&str>) -> bmad_bot::watcher::StoryInfo {
    let parts: Vec<&str> = key.splitn(3, '-').collect();
    let epic_num: u32 = parts.first().and_then(|p| p.parse().ok()).unwrap_or(1);
    let story_num: u32 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(1);
    let label = parts.get(2).unwrap_or(&"test").to_string();
    bmad_bot::watcher::StoryInfo {
        story_id: format!("{epic_num}.{story_num}"),
        story_key: key.to_string(),
        epic_num,
        story_num,
        label,
        branch_name: format!("story/{key}"),
        specs_path: std::path::PathBuf::from(format!("_bmad-output/implementation-artifacts/{key}.md")),
        dependencies: deps.into_iter().map(String::from).collect(),
        status: "in-progress".to_string(),
    }
}
```

**If Story 7.1's `make_test_story` helper already exists** in `tests/integration/helpers/fixtures.rs`, use that instead — but verify it supports a `deps` parameter.

### Previous Story Intelligence (Stories 7.1 through 7.7)

All stories 7.1–7.7 have been created as `ready-for-dev` context stories but **none have been implemented yet** (no integration test code exists in `tests/integration/`). Key patterns learned from reviewing those stories:

1. **Every story repeats the `lib.rs` blocker** — Task 0 is identical across all stories. Implement it once, and subsequent stories skip it.

2. **Test file naming convention:** `test_{module_name}.rs` (e.g., `test_config.rs`, `test_watcher.rs`, `test_notifier.rs`). For this story: `test_branch_git.rs`.

3. **Mock infrastructure (7.1):** `MockGitProvider`, `MockNotifier`, `MockSessionRunner`, `MockReviewRunner` — these are NOT needed for Story 7.8. Branch management and git tool tests use **real Git CLI operations on temp repos** (no mocking needed).

4. **Story 7.6 (Git Provider)** tests PR creation via mock HTTP — different from this story which tests local git operations via Git CLI subprocess. No overlap.

5. **Helpers pattern:** If Story 7.1 is implemented first, `tests/integration/helpers/fixtures.rs` will contain `create_test_repo()` and `make_test_story()`. If not, this story should create them inline or in a local helper.

### Git Intelligence

Recent commits (last 10):
```
ad4e6e8 docs: add comprehensive README with architecture, quick start, and CLI reference
60def59 docs(stories): create story 7-7 notification flow integration tests
8db8f88 docs(stories): create story 7-6 git provider PR creation integration tests
80e7a09 docs(stories): create story 7-5 session WAL crash recovery integration tests
f1b5f31 docs(stories): create story 7-4 pipeline orchestration integration tests
2df7229 docs(stories): create story 7-3, fix critical lib.rs blocker
e10c275 docs(stories): create story 7-2 config startup validation integration tests
1b260ab docs(stories): create story 7-1 integration test infrastructure
26e2a9c chore(sprint-planning): add Epic 7 integration tests to sprint-status
6532f0a docs: consolidate epic 7 integration tests into epics.md
```

**Observation:** Only story file creation commits — no implementation code committed yet for any Epic 7 stories. All stories 7.1–7.7 are in `ready-for-dev` status. This confirms the dev agent will need to handle the `lib.rs` prerequisite regardless of which story runs first.

### Dependencies Required

All already present in `Cargo.toml`:
- `tempfile = "3"` — dev-dependency for isolated temp directories
- `tokio` with `full` features — for async tests (`preserve_partial_work`, `GitTool::call`)
- `rig-core = "0.30"` — needed for `use rig::tool::Tool;` to call `GitTool::call()` in tests

**No new dependencies needed.**

### Required Imports for Test File

Every integration test file for this story needs these imports at minimum:

```rust
use bmad_bot::session::branch::{ensure_story_branch, determine_base_branch, BranchAction, BranchError};
use bmad_bot::session::cleanup::preserve_partial_work;
use bmad_bot::tools::git::{GitTool, GitToolArgs};
use bmad_bot::watcher::StoryInfo;
use rig::tool::Tool; // REQUIRED — without this, GitTool::call() won't resolve
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
```

### File Structure

```
src/
├── lib.rs                   ← NEW if not exists (Task 0 — BLOCKER, see Story 7.1)
├── main.rs                  ← MODIFIED if lib.rs created (remove mod declarations)
└── session/
    └── mod.rs               ← ✅ Already has: pub mod branch; pub mod cleanup;

tests/
├── e2e/
│   └── mod.rs              # (existing — DO NOT TOUCH)
├── integration.rs           ← NEW if not exists (Cargo test binary entry point)
└── integration/
    ├── helpers/
    │   ├── mod.rs           # Re-exports (if needed by other stories)
    │   └── fixtures.rs      # Shared helpers: init_test_repo, make_story (if not from 7.1)
    └── test_branch_git.rs   ← NEW (all Story 7.8 tests)
```

### Testing Standards

- **Framework:** `#[tokio::test]` for async functions (`preserve_partial_work`, `GitTool::call`), `#[test]` acceptable for sync functions (`determine_base_branch`, `ensure_story_branch`)
- **Isolation:** Every test creates its own `tempfile::tempdir()` — no shared state between tests
- **Naming:** `test_{module}_{behavior}_{scenario}` in snake_case
- **Structure:** Arrange → Act → Assert, always in that order
- **No real APIs:** All tests use real Git CLI on temp repos (fast, deterministic, no network)
- **Tracing is a no-op in tests** — do NOT install a tracing subscriber unless explicitly debugging
- **Assertions:** Use `assert!`, `assert_eq!`, `assert_ne!` — use `.expect("reason")` for unwraps, never bare `.unwrap()` in test assertions
- **Cleanup:** `TempDir` Drop handles cleanup — no manual cleanup needed
- **All tests must complete in < 5 seconds** — Git CLI operations on temp repos are fast

### Project Structure Notes

- Alignment with unified project structure: integration tests in `tests/` per `project-context.md` and `architecture.md`
- Existing `tests/e2e/mod.rs` is reserved for live LLM E2E tests (gated behind `BMAD_E2E=1`) — do NOT modify or mix with integration tests
- Branch naming convention from project-context.md: `story/{epic}-{story}-{label}` (e.g., `story/1-2-cli-framework`)
- Git CLI is used for all git operations — consistent with production code post Story 4.4 migration (git2 removed entirely from project)

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Epic 7 Overview (L854-864)]
- [Source: _bmad-output/planning-artifacts/epics.md — Integration Test Strategy (L864-898)]
- [Source: _bmad-output/planning-artifacts/epics.md — Story 7.8 (L1179-1216)]
- [Source: _bmad-output/planning-artifacts/epics.md — Epic Summary (L1287-1312)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Test Mock Pattern (L510-542)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Project Structure (L561-607)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Rig Tool Implementation Pattern (L376-427)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Git Provider Trait Pattern (L479-510)]
- [Source: _bmad-output/project-context.md — Testing Rules section]
- [Source: _bmad-output/project-context.md — Development Workflow Rules — branch naming]
- [Source: _bmad-output/project-context.md — Critical Don't-Miss Rules section]
- [Source: src/session/branch.rs — BranchError (L21-55), BranchAction (L62-75)]
- [Source: src/session/branch.rs — determine_base_branch (L89-128)]
- [Source: src/session/branch.rs — ensure_story_branch (L146-207)]
- [Source: src/session/branch.rs — checkout_branch (L210-226, private)]
- [Source: src/session/branch.rs — unit tests (L229-523) — 12 tests]
- [Source: src/session/cleanup.rs — preserve_partial_work (L38-137)]
- [Source: src/session/cleanup.rs — unit tests (L195-548) — 8 tests]
- [Source: src/tools/git.rs — GitTool struct (L20-23)]
- [Source: src/tools/git.rs — GitToolArgs (L27-44)]
- [Source: src/tools/git.rs — GitToolError (L48-89)]
- [Source: src/tools/git.rs — GitTool impl (L91-477) — 9 action handlers]
- [Source: src/tools/git.rs — Tool trait impl (L479-653)]
- [Source: src/tools/git.rs — unit tests (L656-1030) — 12 tests]
- [Source: src/watcher/mod.rs — StoryInfo (L66-86)]
- [Source: _bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md — Task 0 lib.rs blocker]
- [Source: _bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md — File Structure (L276-321)]
- [Source: _bmad-output/implementation-artifacts/7-7-notification-flow-integration-tests.md — lib.rs prerequisite pattern]

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List