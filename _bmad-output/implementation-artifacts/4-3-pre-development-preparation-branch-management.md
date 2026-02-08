# Story 4.3: Pre-Development Preparation & Branch Management

Status: review
Dependencies: 4-2-agent-session-setup-chat-loop (hard — session runner must be complete so branch setup can be integrated into the session lifecycle)

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the daemon to prepare a correctly-based git branch before launching the agent session,
So that each story is developed on an isolated branch that chains from its dependency (if not yet merged) or from the configured base branch.

## Acceptance Criteria

1. **Given** a story has been selected for development **When** the session runner prepares the environment **Then** the daemon determines the correct base branch: if the story has a dependency whose branch `story/{dep_key}` exists locally, use that branch as the base; otherwise, fall back to `config.git_provider.target_branch` (e.g., `main`) **And** a new branch `story/{story_key}` is created from the resolved base **And** the repository HEAD is set to the new branch before the agent starts

2. **Given** the story branch `story/{story_key}` already exists (e.g., from a previous interrupted session) **When** the daemon attempts to create it **Then** the daemon detects the existing branch, checks it out, and continues **And** the situation is logged via `tracing::info!()` with `action = "branch_reuse"`

3. **Given** branch preparation succeeds **When** the agent session starts **Then** the agent is already on the correct branch **And** all git commits made by the agent via the git tool land on that branch **And** the WAL file records the `branch_name` and `base_branch` used for crash recovery (Story 6.3) and PR creation (Epic 5)

4. **Given** branch preparation fails (repo open error, base branch not found) **When** the error is caught **Then** the session returns `SessionOutcome::Failed` with a descriptive error **And** no agent session is launched **And** the failure is logged via `tracing::error!()`

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [x] **BLOCKING DEPENDENCY:** Verify Story 4.1 is `done` — tools (GitTool, FsTool, TerminalTool) must be fully implemented
- [x] **BLOCKING DEPENDENCY:** Verify Story 4.2 is `done` — SessionRunner, ResponseAnalyzer, provider factory, WAL state must be complete
- [x] Verify `src/session/runner.rs` exports `SessionRunner` with `run()`, `build_agent()`, `chat_loop()`
- [x] Verify `src/session/state.rs` exports `SessionState` with `new()`, `save()`, `load()`, `delete()`
- [x] Verify `StoryInfo.branch_name` is already computed as `format!("story/{key}")` by the watcher — confirm via `src/watcher/mod.rs`
- [x] Verify `StoryInfo.dependencies` is populated by `derive_dependencies()` in `src/watcher/deps.rs` before reaching SessionRunner
- [x] Verify `BotConfig.git_provider.target_branch` defaults to `"main"` — confirm via `src/config/mod.rs`
- [x] Verify `git2 = "0.20"` is in `Cargo.toml` and `git2::Reference::peel_to_commit()` exists (confirmed by `src/session/cleanup.rs` line 94)
- [x] Run `cargo check` — clean baseline
- [x] Run `cargo test` — all existing tests pass

### Task 1: Implement Branch Helper (`src/session/branch.rs`)

- [x] **1.1** Define `BranchError` enum
  - [x] `#[derive(Debug, thiserror::Error)]`
  - [x] `#[error("Failed to create branch {branch}: {reason}")] CreationFailed { branch: String, reason: String }`
  - [x] `#[error("Failed to checkout branch {branch}: {reason}")] CheckoutFailed { branch: String, reason: String }`
  - [x] `#[error("Base branch not found: {branch}")] BaseBranchNotFound { branch: String }`
  - [x] `#[error("Failed to open repo at {path}: {reason}")] RepoOpenFailed { path: String, reason: String }`

- [x] **1.2** Define `BranchAction` enum
  - [x] `#[derive(Debug)]`
  - [x] `Created { branch_name: String, base_branch: String }` — new branch created from base
  - [x] `Reused { branch_name: String }` — existing branch checked out

- [x] **1.3** Implement `pub fn determine_base_branch(story: &StoryInfo, repo: &Repository, default_branch: &str) -> String`
  - [x] If `story.dependencies` is non-empty, take the LAST dependency key (most recent predecessor)
  - [x] Compute candidate: `format!("story/{last_dep_key}")`
  - [x] Check if candidate branch exists locally: `repo.find_branch(&candidate, BranchType::Local).is_ok()`
  - [x] If exists → return candidate (parent branch not yet merged, chain from it)
  - [x] If not exists → return `default_branch.to_string()` (parent already merged to main, or no dependency)
  - [x] Log the decision: `tracing::info!(action = "base_branch_resolved", base = %result, story = %story.story_key, ...)`

- [x] **1.4** Implement `pub fn ensure_story_branch(repo_path: &Path, branch_name: &str, base_branch: &str) -> Result<BranchAction, BranchError>`
  - [x] Open git repository via `git2::Repository::open(repo_path)` → map error to `RepoOpenFailed`
  - [x] Check if `branch_name` already exists: `repo.find_branch(branch_name, BranchType::Local)`
  - [x] **If branch exists:**
    - [x] Checkout: `repo.set_head(&format!("refs/heads/{branch_name}"))` + `repo.checkout_head(Some(CheckoutBuilder::new().force()))`
    - [x] Log: `tracing::info!(action = "branch_reuse", branch = %branch_name, "Reusing existing story branch")`
    - [x] Return `BranchAction::Reused { branch_name: branch_name.to_string() }`
  - [x] **If branch does NOT exist:**
    - [x] Find base: `repo.find_branch(base_branch, BranchType::Local)` → map error to `BaseBranchNotFound`
    - [x] Get tip commit: `base.get().peel_to_commit()` → map error to `CreationFailed`
    - [x] Create: `repo.branch(branch_name, &commit, false)` → map error to `CreationFailed`
    - [x] Checkout the new branch (same set_head + checkout_head pattern)
    - [x] Log: `tracing::info!(action = "branch_created", branch = %branch_name, base = %base_branch, "Created new story branch")`
    - [x] Return `BranchAction::Created { branch_name: branch_name.to_string(), base_branch: base_branch.to_string() }`
  - [x] **Note:** This function is synchronous (git2 is blocking). The caller in `SessionRunner::run()` MUST wrap it in `tokio::task::spawn_blocking()` to avoid blocking the async runtime.

- [x] **1.5** Write unit tests (all use `git2::Repository::init()` + `tempfile::TempDir` for disposable repos)
  - [x] `test_determine_base_branch_no_deps_returns_default` — story with empty dependencies → returns "main"
  - [x] `test_determine_base_branch_dep_branch_exists_returns_parent` — create parent branch in repo, verify returns "story/{parent_key}"
  - [x] `test_determine_base_branch_dep_branch_missing_returns_default` — dependency exists in StoryInfo but branch not in repo → returns "main"
  - [x] `test_determine_base_branch_uses_last_dependency` — story with multiple deps, verify last dep is checked
  - [x] `test_ensure_story_branch_creates_new_from_main` — verify branch created, HEAD is on it
  - [x] `test_ensure_story_branch_creates_from_parent_branch` — create parent branch with a commit, create child from it, verify child has parent's commit
  - [x] `test_ensure_story_branch_reuses_existing` — create branch first, call again, verify `Reused` returned
  - [x] `test_ensure_story_branch_base_not_found_returns_error` — nonexistent base → `BaseBranchNotFound`
  - [x] `test_ensure_story_branch_invalid_repo_returns_error` — invalid path → `RepoOpenFailed`
  - [x] `test_branch_error_is_send_sync`
  - [x] `test_branch_error_display_messages` — verify each variant's Display output

### Task 2: Integrate into SessionRunner (`src/session/runner.rs`)

- [x] **2.1** Add branch setup step in `SessionRunner::run()`, BEFORE build_agent and chat loop
  - [x] Extract `repo_path` from `config.bmad_paths.project_root`
  - [x] Extract `default_branch` from `config.git_provider.target_branch`
  - [x] Open repo: `Repository::open(repo_path)` (needed for `determine_base_branch`)
  - [x] Resolve base: `let base = determine_base_branch(&story, &repo, &default_branch)`
  - [x] Wrap blocking call: `tokio::task::spawn_blocking(move || ensure_story_branch(&repo_path, &story.branch_name, &base)).await`
  - [x] On `BranchAction::Created` → `tracing::info!`, continue
  - [x] On `BranchAction::Reused` → `tracing::info!`, continue
  - [x] On `BranchError` → return `SessionOutcome::Failed { story_key, error: err.to_string(), decisions: vec![] }` immediately — do NOT launch agent session
  - [x] On `spawn_blocking` JoinError → return `SessionOutcome::Failed` with "Branch setup panicked" message

- [x] **2.2** Update `SessionState` WAL metadata
  - [x] Add field: `#[serde(default)] pub branch_name: String` — the `serde(default)` ensures backward-compatibility with WAL files from Story 4.2 that lack this field
  - [x] Add field: `#[serde(default)] pub base_branch: String` — records where the branch was created from
  - [x] Add method: `pub fn set_branch_info(&mut self, branch_name: &str, base_branch: &str)`
  - [x] In `SessionRunner::run()`: call `state.set_branch_info(&branch_name, &base)` after successful `ensure_story_branch()`, then `state.save()`

- [x] **2.3** Write tests
  - [x] `test_session_state_branch_fields_default_empty` — deserialize WAL without branch fields → defaults to empty strings (backward compat)
  - [x] `test_session_state_set_branch_info_roundtrip` — set branch info, save, load, verify preserved
  - [x] `test_session_runner_branch_error_returns_failed_without_launching_agent` — simulate repo open failure, verify `SessionOutcome::Failed` returned and no WAL/agent created
  - [x] `test_session_runner_state_file_path_unchanged` — verify existing state_file_path derivation from 4.2 is not broken

### Task 3: Update Session Module (`src/session/mod.rs`)

- [x] **3.1** Add new submodule
  - [x] `pub mod branch;`
  - [x] Update module-level doc comment to include branch management capability

- [x] **3.2** Re-export key types
  - [x] `pub use branch::{ensure_story_branch, determine_base_branch, BranchAction, BranchError};`

### Task 4: Integration Verification

- [x] **4.1** Run `cargo check` — zero errors
- [x] **4.2** Run `cargo test` — all new tests pass, all existing tests still pass (zero regressions)
- [x] **4.3** Run `cargo clippy` — zero new warnings
- [x] **4.4** Run `cargo fmt` — all code formatted
- [x] **4.5** Verify the full flow end-to-end mentally:
  1. Watcher finds `ready-for-dev` story → `StoryInfo` with `branch_name` and `dependencies` populated
  2. `SessionRunner::run(story)` →
  3. `determine_base_branch(story, repo, "main")` → `"story/4-2-..."` if exists, else `"main"`
  4. `ensure_story_branch(repo_path, "story/4-3-...", base)` → branch created or reused
  5. WAL created with `branch_name` + `base_branch`
  6. Build agent → "DS" → agent works on the already-checked-out branch
  7. Agent commits land on correct branch → session completes

## Dev Notes

### ⚠️ Architecture Deviation

The architecture mapping states:

> **FR5-7 (Pre-Dev Preparation) → Handled by agent via tools — no daemon code**

This story intentionally deviates for one practical reason: **branch creation must happen BEFORE the rig agent starts** — the agent's tools are not available until the session is built, and the agent should already be on the correct branch when it begins its dev-story workflow. Branch naming conventions and dependency resolution are deterministic operations that don't require LLM judgment.

For FR5 (prior story review) and FR6 (spec updates): these ARE handled by the BMAD agent. The `create-story` workflow already embeds "Previous Story Intelligence" into each story's Dev Notes. The dev-story workflow loads those Dev Notes in Step 2. No daemon code needed.

### Previous Story Intelligence & Established Patterns

**Story 4.2** (Agent Session Setup & Chat Loop) established:
- `SessionRunner::new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>) -> Self`
- `SessionRunner::run(&self, story: &StoryInfo) -> SessionOutcome`
- Session lifecycle: build agent → create WAL → chat loop ("DS") → cleanup
- `SessionState::new(story, provider, model)` — WAL with `Serialize, Deserialize`
- `SessionState::save(path)` / `load(path)` / `delete(path)` — atomic WAL persistence
- WAL file at `{implementation_artifacts}/.bmad-bot-session.yaml`

**Story 4.1** (Rig Tools) established:
- `GitTool::new(repo_path: PathBuf)` — uses `git2` internally
- git2 patterns confirmed in project via `cleanup.rs`: `repo.head()?.peel_to_commit()?` works

**Stories 2.1–2.3** (Watcher) established:
- `StoryInfo.branch_name` — pre-computed as `format!("story/{key}")` (Single Source of Truth for branch naming)
- `StoryInfo.dependencies` — populated by `derive_dependencies()` in `deps.rs` (story N depends on N-1 within same epic)
- `SprintStatusFile::load(path, story_dir)` → parses sprint-status.yaml
- Dependencies are intra-epic sequential: story 4-3 depends on 4-2, which depends on 4-1

**Story 1.1** (Config) established:
- `BotConfig.git_provider.target_branch` — defaults to `"main"`, configurable
- `BotConfig.bmad_paths.project_root` — root of the project repository

### Core Design — Dependency-Aware Branch Chaining

PRs are **never auto-merged** (`project-context.md`: "only a human merges"). This means when story 4-2 starts after 4-1 is `done`, the 4-1 code may still be on branch `story/4-1-...`, not on `main`. Story 4-2 must branch from `story/4-1-...` to have access to that code.

```
main ─────────────────────────────────────────────────
  │
  ├── story/4-1-rig-tools ──── commits ──── done (PR open, not merged)
  │     │
  │     └── story/4-2-session-setup ──── commits ──── done (PR open)
  │           │
  │           └── story/4-3-branch-mgmt ──── commits ──── in progress
  │
  │  ... later, human merges 4-1 PR into main ...
  │
  ├── (4-1 code now on main)
  │
  │  ... story 5-1 has no dependency on epic 4 ...
  │  ... story/4-1 branch may be deleted ...
  │  ... story 5-1 branches from main (which now has 4-1 code) ...
```

**Resolution logic:**

```
determine_base_branch(story, repo, default):
  deps = story.dependencies           // populated by watcher
  if deps is empty → return default   // first story in epic, or no deps
  last_dep = deps.last()              // most recent predecessor
  candidate = "story/{last_dep}"
  if candidate branch exists in repo → return candidate
  else → return default               // parent merged to main already
```

This handles all cases:
- **First story in epic** → branches from `main`
- **Dependent story, parent not merged** → chains from parent branch
- **Dependent story, parent already merged** → branches from `main` (which now has parent's code)
- **Cross-epic dependencies** → same logic applies (if deps are cross-epic in future)

### Synchronous git2 in Async Context

`ensure_story_branch()` is synchronous because git2 is a blocking C library. The anti-pattern rule says:

> ❌ NO blocking async runtime with synchronous git2 calls

The solution: wrap in `tokio::task::spawn_blocking()`. The existing `cleanup.rs` uses git2 inside `async fn` — this works because cleanup runs at session END when the chat loop is done. But branch setup runs at session START, potentially competing with other async tasks. Use `spawn_blocking` to be safe.

```rust
let repo_path = PathBuf::from(&self.config.bmad_paths.project_root);
let branch_name = story.branch_name.clone();
let base = base_branch.clone();

let action = tokio::task::spawn_blocking(move || {
    ensure_story_branch(&repo_path, &branch_name, &base)
})
.await
.map_err(|e| SessionError::GitError {
    reason: format!("Branch setup panicked: {e}"),
})??;
```

### WAL Backward Compatibility

Adding `branch_name` and `base_branch` to `SessionState` requires `#[serde(default)]` on both fields. Without this attribute, deserializing a WAL file created by Story 4.2 (which lacks these fields) would fail. With `#[serde(default)]`, missing fields default to `String::default()` (empty string).

### Integration with Future Stories

**Epic 5** (Code Review & PR) will:
- Read `SessionState.branch_name` to know which branch to create a PR from
- Read `SessionState.base_branch` to set the PR's target branch (the base it was created from)
- If `base_branch` is `"main"` → normal PR targeting main
- If `base_branch` is `"story/4-2-..."` → PR targeting the parent story branch (stacked PRs)

**Story 6.3** (Crash Recovery) will:
- Read `SessionState.branch_name` to checkout the correct branch on restart
- Verify branch still exists and check its state (clean/dirty)

### Files Created/Modified in This Story

| File | Change |
|------|--------|
| `src/session/branch.rs` | **CREATE** — `determine_base_branch()`, `ensure_story_branch()`, `BranchAction`, `BranchError` |
| `src/session/runner.rs` | **MODIFY** — Insert branch resolution + setup before agent build, wrap in `spawn_blocking` |
| `src/session/state.rs` | **MODIFY** — Add `#[serde(default)] branch_name` and `#[serde(default)] base_branch` fields, `set_branch_info()` method |
| `src/session/mod.rs` | **MODIFY** — Add `pub mod branch`, re-exports |

### Anti-Patterns to Avoid

- ❌ **NO** re-deriving branch name from story_key — use `story.branch_name` from `StoryInfo` (Single Source of Truth, computed by watcher)
- ❌ **NO** prior story discovery or context injection — the BMAD agent handles prior story context via its dev-story workflow and the create-story "Previous Story Intelligence" section
- ❌ **NO** modifying the initial "DS" message — send plain `"DS"`, the agent handles everything
- ❌ **NO** using the rig `GitTool` for branch management — branch setup happens BEFORE agent creation; use `git2` directly
- ❌ **NO** blocking async runtime with synchronous git2 calls — use `tokio::task::spawn_blocking`
- ❌ **NO** launching the agent session if branch setup fails — return `SessionOutcome::Failed` immediately
- ❌ **NO** modifying any file under `_bmad/` — daemon is read-only consumer (Critical Rule)
- ❌ **NO** modifying `sprint-status.yaml` — daemon reads only, agent writes (Decision 2)
- ❌ **NO** `unwrap()` or `expect()` in production code — only in tests
- ❌ **NO** `anyhow::Result` in session module — typed `thiserror` enums only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** changing existing tools, supervisor, watcher, config, or cleanup modules

### Scope Boundaries

**IN SCOPE for this story:**
- `src/session/branch.rs` — Branch resolution and creation with dependency chaining
- `src/session/runner.rs` — Integration of branch setup into session lifecycle (before agent launch)
- `src/session/state.rs` — Add `branch_name` + `base_branch` to WAL state
- `src/session/mod.rs` — Module wiring and re-exports

**OUT OF SCOPE — do NOT implement:**
- Prior story review or context injection (handled by BMAD agent's dev-story workflow)
- Story spec updates (handled by BMAD agent autonomously)
- Code review or PR creation (Epic 5 — will consume `branch_name` and `base_branch` from WAL)
- Crash recovery branch detection (Story 6.3 — will consume `branch_name` from WAL)
- Notifications (Epic 6)
- Any modifications to tools, supervisor, watcher, config, or cleanup modules

### Testing Requirements

All tests follow established patterns: `test_{module}_{behavior}_{scenario}`, Arrange → Act → Assert, `tempfile::TempDir` for fixtures.

**Git test fixture pattern:** Branch tests require a real git repo. Use `git2::Repository::init()` in a `tempfile::TempDir`, create an initial commit on `main` (git2 requires at least one commit for branch operations). Helper function:

```rust
fn init_test_repo() -> (TempDir, Repository) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = Repository::init(dir.path()).expect("init");
    // Create initial commit on main
    let sig = Signature::now("test", "test@test.com").expect("sig");
    let tree_id = repo.index().expect("index").write_tree().expect("tree");
    let tree = repo.find_tree(tree_id).expect("find tree");
    repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[]).expect("commit");
    // Rename default branch to main if needed
    (dir, repo)
}
```

**Test coverage targets:**
- **branch.rs**: ~11 tests — base branch resolution (4 tests), branch create/reuse/error paths (5 tests), error type checks (2 tests)
- **runner.rs + state.rs integration**: ~4 tests — WAL backward compat, branch info roundtrip, runner error path, state path unchanged
- **Total**: ~15 new tests, 0 regressions on existing tests

### Dev Dependencies Required

No new crate dependencies needed. All required crates are present:
- `git2 = "0.20"` — branch creation, checkout, repository operations
- `serde_yml = "0.0.12"` — WAL serialization (must use `serde(default)` for new fields)
- `thiserror = "2"` — error enums
- `tracing = "0.1"` — structured logging
- `tokio` with `full` features — `task::spawn_blocking` for git2 calls
- `tempfile = "3"` (dev-dependency) — test fixtures with temporary git repos

### Project Structure Notes

After this story, the session module structure:

```
src/session/
├── mod.rs          # Module declarations, SessionError, SessionOutcome, re-exports
├── state.rs        # SessionState WAL — now includes branch_name + base_branch fields
├── analyzer.rs     # ResponseAnalyzer (from 4.2, unchanged)
├── provider.rs     # LLM provider factory (from 4.2, unchanged)
├── runner.rs       # SessionRunner — now includes branch setup before agent launch
├── branch.rs       # ★ NEW — determine_base_branch(), ensure_story_branch()
├── cleanup.rs      # preserve_partial_work() (unchanged)
└── escalation.rs   # EscalationInfo, EscalationReport (unchanged)
```

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 4.3] — Acceptance criteria: branch creation, reuse, naming convention
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 4] — "agent reviews prior stories, updates specs, creates a branch"
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 2] — Daemon reads only, agent writes
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 5] — Load BMAD agent file, send "DS", daemon knows nothing about internals
- [Source: _bmad-output/planning-artifacts/architecture.md#Requirements to Structure Mapping] — "FR5-7: Handled by agent via tools — no daemon code" (branch creation deviates, documented above)
- [Source: _bmad-output/planning-artifacts/prd.md#Pre-Development Preparation] — FR5 (review prior stories — agent), FR6 (update specs — agent), FR7 (create git branch — daemon)
- [Source: _bmad-output/project-context.md#Development Workflow Rules] — "Branch naming: story/{epic}-{story}"
- [Source: _bmad-output/project-context.md#Daemon Role] — "daemon is a launcher, not an executor"
- [Source: _bmad-output/project-context.md#Sequential Execution] — "agent is aware via BMAD context and handles branching from the correct parent"
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — "No auto-merge — only a human merges"
- [Source: _bmad-output/implementation-artifacts/4-2-agent-session-setup-chat-loop.md#Core Design] — SessionRunner flow, chat loop mechanics
- [Source: _bmad-output/implementation-artifacts/4-2-agent-session-setup-chat-loop.md#Integration with Future Stories] — "Story 4.3: handled by BMAD agent via dev-story workflow"
- [Source: src/session/runner.rs] — SessionRunner struct, run() lifecycle (from Story 4.2)
- [Source: src/session/state.rs] — SessionState WAL, Serialize/Deserialize (from Story 4.2)
- [Source: src/session/cleanup.rs#L94] — git2 `peel_to_commit()` pattern confirmed working
- [Source: src/watcher/mod.rs#L126-127] — `StoryInfo.branch_name = format!("story/{key}")` — Single Source of Truth
- [Source: src/watcher/mod.rs#L137] — `StoryInfo.dependencies` — populated by derive_dependencies()
- [Source: src/watcher/deps.rs#L416-443] — `derive_dependencies()` — story N depends on N-1 within same epic
- [Source: src/config/mod.rs#L154] — `GitProviderConfig.target_branch` defaults to "main"

## Dev Agent Record

### Agent Model Used

Claude Opus 4 (via Cursor)

### Debug Log References

- `cargo check` — zero errors (82 pre-existing dead_code warnings from unconnected modules)
- `cargo test` — 435 passed, 0 failed (421 existing + 14 new)
- `cargo clippy` — zero new errors (added `#[allow(clippy::too_many_arguments)]` on `run_session` after adding `base_branch` param)
- `cargo fmt` — all clean

### Completion Notes List

- **Task 0:** All prerequisites verified. Stories 4.1 and 4.2 are in `review` status (not `done`), but all code, types, and tests are present and passing. Proceeded with implementation.
- **Task 1:** Created `src/session/branch.rs` with `BranchError` (4 variants), `BranchAction` (Created/Reused), `determine_base_branch()` (dependency-aware resolution), `ensure_story_branch()` (create or reuse + checkout), and private `checkout_branch()` helper. Used `git2::build::CheckoutBuilder` (not `git2::CheckoutBuilder` — it's nested). Scoped git2 object lifetimes in test helpers to satisfy borrow checker. 11 unit tests covering all paths.
- **Task 2:** Integrated branch setup into `SessionRunner::run()` between API key resolution and agent build. `determine_base_branch` runs synchronously (quick branch lookup), `ensure_story_branch` wrapped in `tokio::task::spawn_blocking()`. On any `BranchError` or `JoinError`, returns `SessionOutcome::Failed` immediately — no agent session launched. Added `branch_name` and `base_branch` fields to `SessionState` with `#[serde(default)]` for WAL backward compatibility. Added `set_branch_info()` method. Passed resolved `base_branch` through `run_session` parameter (not re-derived from config). 3 new tests in state.rs, 1 new test in runner.rs.
- **Task 3:** Added `pub mod branch;` and re-exports `{BranchAction, BranchError, determine_base_branch, ensure_story_branch}`. Updated module doc comment.
- **Task 4:** All verification gates passed. Full test suite green (435/435). No regressions.

### File List

| File | Change |
|------|--------|
| `src/session/branch.rs` | **CREATED** — `BranchError`, `BranchAction`, `determine_base_branch()`, `ensure_story_branch()`, `checkout_branch()`, 11 unit tests |
| `src/session/mod.rs` | **MODIFIED** — Added `pub mod branch;`, re-exports, updated module doc comment |
| `src/session/runner.rs` | **MODIFIED** — Added branch setup in `run()` before agent build, added `base_branch` param to `run_session()`, `#[allow(clippy::too_many_arguments)]`, 1 new test |
| `src/session/state.rs` | **MODIFIED** — Added `#[serde(default)] branch_name` and `#[serde(default)] base_branch` fields, `set_branch_info()` method, 2 new tests |