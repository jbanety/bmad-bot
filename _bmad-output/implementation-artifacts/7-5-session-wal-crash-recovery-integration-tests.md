# Story 7.5: Session WAL Crash Recovery Integration Tests

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want integration tests that verify crash recovery from WAL files reconstructs a valid session,
so that I'm confident the daemon can survive crashes and resume work without data loss.

## Acceptance Criteria

1. **Given** a temp directory with a valid `.bmad-bot-session.yaml` WAL file containing:
   - `story_key: "1-2-cli"`, `branch_name: "story/1-2-cli"`, `base_branch: "main"`
   - Chat history with 4 messages (2 user, 2 assistant)
   - `provider: "anthropic"`, `model: "claude-sonnet-4-20250514"`
   **When** `SessionRunner::check_and_recover_wal()` is called
   **Then** it returns `Some(RecoveryInfo)` with the correct story info and state
   **And** `story_info_from_wal()` produces a `StoryInfo` with matching key, branch, and label

2. **Given** a recovered WAL state
   **When** `SessionState::to_rig_messages()` is called
   **Then** the returned `Vec<Message>` contains all 4 messages in the correct order with correct roles

3. **Given** a WAL file with corrupted/invalid YAML content
   **When** `check_and_recover_wal()` is called
   **Then** the WAL file is deleted (preventing infinite recovery loops)
   **And** `None` is returned (clean start)

4. **Given** NO WAL file exists
   **When** `check_and_recover_wal()` is called
   **Then** `None` is returned immediately

5. **Given** a valid WAL file exists
   **When** `recover_and_process()` runs with mocked session/review/git_provider/notifier
   **Then** the full pipeline executes for the recovered story
   **And** the WAL file is deleted after processing (regardless of success or failure)

6. **Given** a WAL file exists AND new eligible stories are found in sprint-status
   **When** the daemon startup sequence runs
   **Then** crash recovery is processed FIRST, before any new stories are polled

## Tasks / Subtasks

- [ ] Task 0: Prerequisites — Verify lib.rs and session::state visibility (AC: all)
  - [ ] 0.1 Verify `src/lib.rs` exists with `pub mod session;` (created by Story 7.1 Task 0). If missing, create it (see Dev Notes)
  - [ ] 0.2 Verify `src/session/mod.rs` has `pub use state::{SessionState, ChatMessage};` re-export (Story 7.1 Task 0.4). If missing, add it
  - [ ] 0.3 Verify `bmad_bot::session::runner::{SessionRunner, RecoveryInfo, story_info_from_wal}` is accessible from integration tests
  - [ ] 0.4 Run `cargo build` — must succeed after any visibility fixes

- [ ] Task 1: Create integration test file and module declaration (AC: all)
  - [ ] 1.1 Create `tests/integration/test_session_wal.rs`
  - [ ] 1.2 Add `mod test_session_wal;` in `tests/integration.rs` (create entry point if Story 7.1 not yet implemented)
  - [ ] 1.3 Add imports: `bmad_bot::session::{SessionState, ChatMessage}`, `bmad_bot::session::runner::{SessionRunner, RecoveryInfo, story_info_from_wal}`, `bmad_bot::config::{BotConfig, BotSecrets}`, `bmad_bot::watcher::StoryInfo`

- [ ] Task 2: Create WAL fixture helpers (AC: #1, #2, #3, #4)
  - [ ] 2.1 Add `fn make_valid_wal_state() -> SessionState` — construct via struct literal (all fields `pub`) with `story_key: "1-2-cli"`, `branch_name: "story/1-2-cli"`, `base_branch: "main"`, `provider: "anthropic"`, `model: "claude-sonnet-4-20250514"`, and 4 chat messages
  - [ ] 2.2 Add `async fn write_wal_to_dir(dir: &Path, state: &SessionState)` — calls `state.save()` to write `{dir}/.bmad-bot-session.yaml`
  - [ ] 2.3 Add `fn make_test_config(dir: &Path) -> Arc<BotConfig>` — full `BotConfig` struct literal with `bmad_paths.implementation_artifacts` pointing to `dir` (see Dev Notes for exact pattern)
  - [ ] 2.4 Add `fn make_test_secrets() -> Arc<BotSecrets>` — dummy secrets
  - [ ] 2.5 Add `fn wal_path(dir: &Path) -> PathBuf` — returns `dir.join(".bmad-bot-session.yaml")` (since `SessionRunner.state_file_path` is private)

- [ ] Task 3: Write full save→recover→parse integration test (AC: #1)
  - [ ] 3.1 Create temp dir, build valid `SessionState` via `make_valid_wal_state()`, write WAL via `write_wal_to_dir()`
  - [ ] 3.2 Construct `SessionRunner::new(config, secrets)` with config pointing to temp dir
  - [ ] 3.3 Call `check_and_recover_wal()` → assert `Some(RecoveryInfo)` with correct `story_info` fields (story_key, epic_num, story_num, label, branch_name) and `state` fields (provider, model, chat_history length)

- [ ] Task 4: Write to_rig_messages conversion test (AC: #2)
  - [ ] 4.1 Create `SessionState` with 4 messages, call `to_rig_messages()`, assert length == 4
  - [ ] 4.2 Verify message ordering matches original `chat_history` (compare via debug format or rig accessor)

- [ ] Task 5: Write corrupt WAL test (AC: #3)
  - [ ] 5.1 Create temp dir, write raw garbage string to `.bmad-bot-session.yaml` via `tokio::fs::write`
  - [ ] 5.2 Call `check_and_recover_wal()` → assert `None` AND assert WAL file deleted from disk

- [ ] Task 6: Write no-WAL test (AC: #4)
  - [ ] 6.1 Create empty temp dir, construct `SessionRunner`, call `check_and_recover_wal()` → assert `None`

- [ ] Task 7: Write post-recovery pipeline test (AC: #5)
  - [ ] 7.1 **Prerequisite:** Story 7.4 Task 0 DI refactor must be complete AND `process_recovered_session` must be made `pub(crate)` (see Dev Notes for rationale)
  - [ ] 7.2 Build `StoryPipeline::new_with_components()` with MockDevRunner, MockCodeReviewer, MockGitProvider, MockNotifier
  - [ ] 7.3 Construct `StoryInfo` + `SessionOutcome::Completed` for "1-2-cli", call `process_recovered_session(&story, outcome)`
  - [ ] 7.4 Assert `PipelineResult` has `status: Completed`, `pr_url: Some(...)`, MockGitProvider received `create_pr`, MockNotifier captured notification
  - [ ] 7.5 Repeat with `SessionOutcome::Failed` → assert `status: Error`, PR still created with `[NEEDS REVIEW]` in title
  - [ ] 7.6 Repeat with `SessionOutcome::Escalated` → assert `status: Blocked`, no PR created

- [ ] Task 8: Write recovery-first priority test (AC: #6)
  - [ ] 8.1 Write valid WAL to temp dir, construct pipeline, call `recover_and_process()` → assert `Some(result)` (WAL detected)
  - [ ] 8.2 With no WAL file, call `recover_and_process()` → assert `None` (daemon proceeds to polling)
  - [ ] 8.3 Note: `recover_and_process()` via `new_with_components()` returns `None` (session_runner_for_recovery is None). Test 8.1 requires a real `SessionRunner` — see Dev Notes for approach

- [ ] Task 9: Write legacy WAL backward compatibility test (supplementary)
  - [ ] 9.1 Create WAL with empty `branch_name` but populated `branch` field (pre-4.3 format)
  - [ ] 9.2 Recover → assert `story_info.branch_name` falls back to `branch` value

- [ ] Task 10: Write forward-compatibility test (supplementary)
  - [ ] 10.1 Create WAL with extra unknown YAML fields (e.g., `extra_field: "unknown"`) via raw YAML append
  - [ ] 10.2 Recover → assert success (serde ignores unknown fields since no `#[serde(deny_unknown_fields)]`)

- [ ] Task 11: Verify all tests pass (AC: all)
  - [ ] 11.1 `cargo test --test integration` — all session WAL tests pass
  - [ ] 11.2 `cargo test` — no regressions in 573+ unit tests
  - [ ] 11.3 `cargo clippy` — zero warnings

## Dev Notes

### Cross-Module Integration Value

This story's tests are **not** duplicating the 20+ unit tests already in `src/session/runner.rs` and `src/session/state.rs`. The existing unit tests verify individual functions in isolation using internal helpers (`make_recovery_state()`, `make_runner_test_config()`) that are `pub(crate)` — invisible to external crates.

**What these integration tests uniquely validate:**

1. **Cross-module boundary:** `SessionState::save()` (state module) → `SessionRunner::check_and_recover_wal()` (runner module) → `story_info_from_wal()` (runner module) → `to_rig_messages()` (state module). The full chain crosses private module boundaries and exercises the public API contract.
2. **External crate perspective:** Tests import via `bmad_bot::session::*` — exactly how the `tests/` crate sees the library. Any visibility regression (e.g., removing a `pub use`) breaks these tests immediately.
3. **Config construction from public API:** Unit tests use internal `make_runner_test_config()` which accesses private struct fields. Integration tests must construct `BotConfig` via public constructors or struct literals — validating that all required fields are `pub`.
4. **Pipeline-level recovery (AC #5):** Testing `process_recovered_session()` through `StoryPipeline` with mocked deps — something unit tests in `pipeline.rs` cannot do without the DI refactor from Story 7.4.

### Architecture Compliance

#### 🚨 CRITICAL — `src/lib.rs` Prerequisite (from Story 7.1 Task 0)

**The project is currently a pure binary crate** — `src/main.rs` only, no `src/lib.rs`. Integration tests in `tests/` are separate crates and can ONLY import from a library crate. Without `lib.rs`, `use bmad_bot::anything;` will NOT compile.

**If Story 7.1 Task 0 is NOT yet implemented, you MUST do it first:**

Create `src/lib.rs`:
```rust
//! bmad-bot library crate — exposes modules for integration tests.
#![deny(clippy::all)]
#![warn(dead_code)]

pub mod config;
pub mod git_provider;
pub mod notifier;
pub mod pipeline;
pub mod review;
pub mod session;
pub mod supervisor;
pub mod tools;
pub mod watcher;
```

Then update `src/main.rs` — remove all `mod X;` lines (except `mod cli;` which stays binary-only).

**Verify:** `cargo build` + `cargo test` must pass with all 573+ existing unit tests.

#### 🚨 CRITICAL — `session::state` Visibility Fix

`src/session/mod.rs` declares `mod state;` (PRIVATE). Integration tests cannot access `SessionState` or `ChatMessage`.

**Required fix in `src/session/mod.rs`:** Add these re-exports:
```rust
pub use state::{SessionState, ChatMessage};
```

This is documented as Story 7.1 Task 0.4 but may not be implemented yet. Without this, fixture helpers cannot construct `SessionState` values and `to_rig_messages()` cannot be tested.

**Do NOT change `mod state;` to `pub mod state;`** — only re-export the needed types. `StateError` is internal.

#### 🚨 CRITICAL — `process_recovered_session()` Visibility (for AC #5)

`process_recovered_session()` in `src/pipeline.rs` is currently **private**. To test the post-recovery pipeline path with mocked deps:

**Recommended approach:** Change visibility to `pub(crate)` in `src/pipeline.rs`:
```rust
pub(crate) async fn process_recovered_session(
    &self,
    story: &StoryInfo,
    outcome: SessionOutcome,
) -> PipelineResult {
```

This is the minimal visibility change — accessible from integration tests (which are in a separate crate but `pub(crate)` works for `tests/` via the library crate), while not exposing it to downstream consumers.

**🚨 Correction:** `pub(crate)` is NOT visible from `tests/` (separate crate). You need full `pub`:
```rust
pub async fn process_recovered_session(
    &self,
    story: &StoryInfo,
    outcome: SessionOutcome,
) -> PipelineResult {
```

This is acceptable because `StoryPipeline` itself is only constructed by the daemon — external misuse is unlikely. Add a doc comment noting it's primarily for test support:
```rust
/// Process the outcome of a recovered session through the post-session pipeline.
///
/// Public for integration test access. Production code calls this via
/// [`recover_and_process()`](Self::recover_and_process).
pub async fn process_recovered_session(
```

**If Story 7.4 DI refactor is NOT complete:** Skip Task 7 entirely. Add `// TODO: pipeline-level recovery tests after Story 7.4 DI refactor`. ACs #1-#4, #6 are fully testable without it.

#### 🚨 WAL Deletion Responsibility — Correct Understanding

**WAL deletion happens in `SessionRunner::resume_session()`** (src/session/runner.rs), NOT in `process_recovered_session()` (src/pipeline.rs). The pipeline method handles post-session logic (review → PR → notification) and never touches the WAL file.

**Implications for AC #5 ("WAL file deleted after processing"):**
- WAL deletion is tested at the `SessionRunner` level, not the pipeline level
- `resume_session()` requires a real LLM agent → **cannot be called in integration tests**
- WAL deletion is already covered by the unit test `test_wal_roundtrip_with_chat_history` + the `resume_session` implementation that always deletes in a `finally` block
- For Task 7, test only the post-recovery pipeline behavior (`process_recovered_session`), not WAL deletion
- For Task 8 (AC #6 / recovery priority), `recover_and_process()` via `new_with_components()` returns `None` because `session_runner_for_recovery` is `None` — this confirms the code path but cannot exercise real WAL detection. Test WAL detection via `SessionRunner::check_and_recover_wal()` directly (Tasks 3-6)

#### `SessionRunner.state_file_path` is PRIVATE

Integration tests cannot access `runner.state_file_path`. Derive the WAL path manually in fixtures:

```rust
fn wal_path(dir: &Path) -> PathBuf {
    dir.join(".bmad-bot-session.yaml")
}
```

This mirrors the derivation in `SessionRunner::new()`: `Path::new(&config.bmad_paths.implementation_artifacts).join(".bmad-bot-session.yaml")`.

#### `SessionState` and `RecoveryInfo` Do NOT Implement `Clone`

`SessionState` is `Serialize + Deserialize` but NOT `Clone`. `RecoveryInfo` wraps it by ownership. After passing `RecoveryInfo` to a consuming function (e.g., `resume_session(recovery)`), all fields are gone.

**Impact on test design:** Clone or save any values you need for assertions BEFORE consuming:
```rust
let recovery = runner.check_and_recover_wal().await.unwrap();
// Clone fields BEFORE any consuming operation
let story_key = recovery.story_info.story_key.clone();
let branch = recovery.story_info.branch_name.clone();
let history_len = recovery.state.chat_history.len();
// Now safe to consume recovery
```

For this story's tests (Tasks 3-6), `check_and_recover_wal()` returns an owned `RecoveryInfo` that is NOT consumed further — you can assert on its fields directly. The Clone constraint only matters if you were to pass it to `resume_session()` (which we don't call).

#### Integration Test Location

- All tests: `tests/integration/test_session_wal.rs`
- Declared as `mod test_session_wal;` in `tests/integration.rs`
- Run via `cargo test --test integration`
- If `tests/integration.rs` doesn't exist yet (Story 7.1 not implemented), create it:
```rust
mod helpers;
mod test_session_wal;
```
  And create `tests/integration/helpers/mod.rs` with shared fixture code.

### Technical Requirements

#### Key Type Signatures (exact from codebase)

**`SessionState`** (`src/session/state.rs`):
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionState {
    pub story_id: String,
    pub story_key: String,
    pub branch: String,           // legacy field (pre-4.3)
    pub started_at: String,       // ISO 8601
    pub last_activity: String,    // ISO 8601
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub branch_name: String,      // Story 4.3+ — preferred over `branch`
    #[serde(default)]
    pub base_branch: String,
    pub chat_history: Vec<ChatMessage>,
}
```

**`ChatMessage`** (`src/session/state.rs`):
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,    // "user" or "assistant"
    pub content: String,
}
```

**`RecoveryInfo`** (`src/session/runner.rs`) — NOT Clone:
```rust
#[derive(Debug)]
pub struct RecoveryInfo {
    pub state: SessionState,      // consumed by ownership
    pub story_info: StoryInfo,
}
```

**`story_info_from_wal()`** (`src/session/runner.rs`):
- Parses `story_key` via `splitn(3, '-')` → `epic_num`, `story_num`, `label`
- Prefers `branch_name` over legacy `branch` field (fallback when `branch_name` is empty)
- Sets `dependencies: vec![]` and `status: "in-progress"`
- Builds `specs_path` from `{config.bmad_paths.implementation_artifacts}/{story_key}.md`

**`check_and_recover_wal()`** (`src/session/runner.rs`):
- Returns `None` immediately if WAL file doesn't exist (AC #4)
- Loads WAL → on success: calls `story_info_from_wal()`, returns `Some(RecoveryInfo)` (AC #1)
- Loads WAL → on parse error: deletes corrupt file, returns `None` (AC #3)

**`to_rig_messages()`** (`src/session/state.rs`):
- Maps `"user"` → `Message::user()`, anything else → `Message::assistant()`
- Returns `Vec<Message>` in same order as `chat_history`

**WAL file path derivation:**
```
{config.bmad_paths.implementation_artifacts}/.bmad-bot-session.yaml
```

**Atomic write pattern:** `save()` writes to `.yaml.tmp` then renames — tests verify the final `.yaml` path only.

#### `StoryInfo` struct (from `src/watcher/mod.rs`):
```rust
pub struct StoryInfo {
    pub story_id: String,      // "1.2"
    pub story_key: String,     // "1-2-cli"
    pub epic_num: u32,
    pub story_num: u32,
    pub label: String,         // "cli"
    pub branch_name: String,   // "story/1-2-cli"
    pub specs_path: PathBuf,
    pub dependencies: Vec<String>,
    pub status: String,
}
```

#### Building a `BotConfig` for Integration Tests

**Do NOT use `BotConfig::_test_minimal()`** — it takes `&str` (not `String`), sets `implementation_artifacts` to a relative path (`"_bmad-output/implementation-artifacts"`) that won't match the temp dir, and is `#[doc(hidden)]` internal API.

**Use the full struct literal pattern** from `src/session/runner.rs` L1760-1804, adapted for integration tests:

```rust
use bmad_bot::config::*;

fn make_test_config(artifacts_dir: &Path) -> Arc<BotConfig> {
    Arc::new(BotConfig {
        polling_interval_secs: 10,
        git_provider: GitProviderConfig {
            provider: "github".to_string(),
            repo_owner: "test".to_string(),
            repo_name: "test".to_string(),
            target_branch: "main".to_string(),
        },
        llm: LlmConfig {
            dev: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-model".to_string(),
            },
            review: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-model".to_string(),
            },
            supervisor: LlmRoleConfig {
                provider: "anthropic".to_string(),
                model: "test-model".to_string(),
            },
        },
        notifications: NotificationConfig {
            telegram: TelegramConfig {
                enabled: false,
                chat_id: String::new(),
            },
        },
        bmad_paths: BmadPathsConfig {
            project_root: artifacts_dir
                .parent()
                .unwrap_or(artifacts_dir)
                .display()
                .to_string(),
            output_folder: artifacts_dir.display().to_string(),
            planning_artifacts: artifacts_dir.display().to_string(),
            implementation_artifacts: artifacts_dir.display().to_string(),
        },
        log_format: "pretty".to_string(),
        log_level: "info".to_string(),
        log_file: "test.log".to_string(),
        code_review_enabled: true,
    })
}
```

This is a direct copy of the `make_runner_test_config()` pattern from runner.rs unit tests, which correctly points all paths at the temp dir.

#### Building `BotSecrets` for Tests

```rust
fn make_test_secrets() -> Arc<BotSecrets> {
    Arc::new(BotSecrets {
        anthropic_api_key: Some("test-anthropic-key-DO-NOT-USE".into()),
        openai_api_key: Some("test-openai-key-DO-NOT-USE".into()),
        github_models_api_key: Some("test-ghmodels-key-DO-NOT-USE".into()),
        github_token: Some("test-github-token-DO-NOT-USE".into()),
        gitlab_token: Some("test-gitlab-token-DO-NOT-USE".into()),
        telegram_bot_token: Some("test-telegram-token-DO-NOT-USE".into()),
    })
}
```

#### Building `SessionState` for Tests

`SessionState::new(&StoryInfo, &str, &str)` sets `branch_name` and `base_branch` to empty strings (they're set later via `set_branch_info()`). For WAL recovery tests, construct the struct directly to control all fields:

```rust
fn make_valid_wal_state() -> SessionState {
    SessionState {
        story_id: "1.2".to_string(),
        story_key: "1-2-cli".to_string(),
        branch: "story/1-2-cli".to_string(),
        started_at: "2026-02-08T10:00:00+00:00".to_string(),
        last_activity: "2026-02-08T10:05:00+00:00".to_string(),
        provider: "anthropic".to_string(),
        model: "claude-sonnet-4-20250514".to_string(),
        branch_name: "story/1-2-cli".to_string(),
        base_branch: "main".to_string(),
        chat_history: vec![
            ChatMessage { role: "user".to_string(), content: "DS".to_string() },
            ChatMessage { role: "assistant".to_string(), content: "Starting story 1-2-cli...".to_string() },
            ChatMessage { role: "user".to_string(), content: "Continue.".to_string() },
            ChatMessage { role: "assistant".to_string(), content: "Implementation complete.".to_string() },
        ],
    }
}
```

All fields are `pub` — direct struct construction works. Use `SessionState::save()` to write WAL files (atomic write). Only hand-write raw YAML for the corrupt WAL test (AC #3).

#### `to_rig_messages()` — Asserting Message Content

`rig::completion::Message` internals may not be directly inspectable. The existing unit test in `state.rs` (L309-321) only asserts on `len()` — it cannot inspect roles either.

**Recommended approach:** Verify the conversion completes for all 4 messages, assert length, then spot-check via debug formatting:
```rust
let messages = state.to_rig_messages();
assert_eq!(messages.len(), 4);
// Verify role mapping: first message should be "user" type
let debug_first = format!("{:?}", messages[0]);
assert!(debug_first.to_lowercase().contains("user"),
    "First message should be user type, got: {debug_first}");
```

If rig provides `Message` content accessors, prefer those over debug formatting.

### Previous Story Intelligence (Stories 7.1, 7.4, 6.3)

**Story 7.1 (Integration Test Infrastructure) — ✅ IMPLEMENTED:**
- `tests/integration.rs` entry point uses `#[path]` attributes (NOT plain `mod` declarations). To add a new test module: `#[path = "integration/test_session_wal.rs"] mod test_session_wal;`
- `tests/integration/helpers/mocks.rs` contains `MockGitProvider`, `MockNotifier`, `MockSessionRunner`, `MockReviewRunner`
- `tests/integration/helpers/fixtures.rs` contains `make_test_config`, `make_test_secrets`, `make_test_story`, `write_sprint_status`, `write_wal_file`, `create_test_repo`
- `src/lib.rs` exists with all modules including `pub mod session;`. `pub use state::{SessionState, ChatMessage};` re-export exists in `src/session/mod.rs`
- The `lib.rs` blocker (Task 0) is RESOLVED — skip any Task 0 equivalent
- All mocks are `Send + Sync`, use `Arc<Mutex<Vec<...>>>` for interior mutability
- Uses `tempfile::tempdir()` for filesystem isolation

**Story 7.4 (Pipeline Orchestration):**
- Defines `DevRunner` and `CodeReviewer` traits for DI
- Defines `StoryPipeline::new_with_components()` injectable constructor
- Defines `MockDevRunner` (VecDeque<SessionOutcome>) and `MockCodeReviewer` (VecDeque<ReviewOutcome>)
- `session_runner_for_recovery: None` in `new_with_components()` → `recover_and_process()` returns `None`
- `PipelineTestBuilder` pattern for clean test setup

**Story 6.3 (Crash Recovery — the production implementation):**
- WAL file persisted after every chat turn via atomic write
- Deleted on successful session completion (inside `resume_session()`)
- On startup: check WAL → if exists → recover → resume_session
- Corrupt WAL: delete + clean start (prevents infinite recovery loops)
- `RecoveryInfo` does NOT implement `Clone` — `SessionState` consumed by ownership

**Existing unit tests in `src/session/runner.rs` (20+ tests):**
- `test_check_wal_returns_none_when_no_file` — covered but from internal perspective
- `test_check_wal_returns_some_when_file_exists` — covered but uses internal helpers
- `test_check_wal_deletes_corrupt_file` — covered
- `test_wal_roundtrip_with_chat_history` — save→load→verify via internal config builder
- `test_check_wal_legacy_wal_backward_compat` — legacy WAL format support
- These tests use `pub(crate)` helpers not available from `tests/` — integration tests validate the same behaviors through the **public API surface**

### Git Intelligence

Recent commits:
- `b121b73 feat(6-3): crash recovery` — production WAL recovery implementation
- `dc7d886 feat(session): implement context window limit recovery (Story 6.4)` — context limit also uses WAL
- Stories 7.1-7.4 context stories created (docs only, no code changes yet)
- All 573+ unit tests passing on `main`

### Dependencies Required

- `tempfile` — `tempdir()` filesystem isolation (already in `[dev-dependencies]`)
- `tokio` with `macros` + `rt-multi-thread` — `#[tokio::test]` (already available)
- `serde_yml` — for corrupt WAL test only (already in dependencies)
- `bmad_bot` — library crate (requires `lib.rs`)

### File Structure

```
tests/
├── integration.rs                           # Entry point (cargo test --test integration)
├── integration/
│   ├── helpers/
│   │   ├── mod.rs                           # Re-exports helper modules
│   │   ├── fixtures.rs                      # Shared fixture builders (from 7.1)
│   │   └── mocks.rs                         # Mock implementations (from 7.1)
│   ├── test_session_wal.rs                  # ← THIS STORY's tests
│   ├── test_pipeline.rs                     # Story 7.4 tests
│   └── ...                                  # Other story test files
```

If Story 7.1 infrastructure is not yet built, create the minimal structure:
```
tests/
├── integration.rs                           # mod helpers; mod test_session_wal;
├── integration/
│   ├── helpers/
│   │   └── mod.rs                           # Inline fixtures for this story
│   └── test_session_wal.rs                  # This story's tests
```

### Testing Standards

- **Framework:** `#[tokio::test]` for all async tests (WAL operations are async)
- **Test naming:** `test_wal_{behavior}_{scenario}` — e.g., `test_wal_recovery_valid_returns_recovery_info`
- **Structure:** Arrange → Act → Assert
- **Isolation:** Each test uses its own `tempdir()` — no shared state
- **No real LLM calls:** Tests call `check_and_recover_wal()` (file I/O only). Never call `resume_session()` or `run()` (require real LLM agents)
- **Zero warnings:** `cargo clippy` clean

### Project Structure Notes

- Alignment with `tests/integration/` convention from Story 7.1
- `tests/e2e/` reserved for live LLM tests (gated behind `BMAD_E2E=1`) — do NOT modify
- WAL file uses dot-prefix (`.bmad-bot-session.yaml`) — hidden file convention for transient state

### References

- [Source: src/session/state.rs] — `SessionState`, `ChatMessage`, `save()`, `load()`, `delete()`, `exists()`, `to_rig_messages()`
- [Source: src/session/runner.rs#L38-200] — `RecoveryInfo`, `story_info_from_wal()`, `SessionRunner::new()`, `check_and_recover_wal()`
- [Source: src/session/runner.rs#L1760-1804] — `make_runner_test_config()` pattern (reference for integration test config builder)
- [Source: src/session/runner.rs#L1872-1901] — `make_recovery_state()` + `make_legacy_recovery_state()` (reference patterns)
- [Source: src/session/mod.rs#L1-36] — `mod state;` (private), `SessionOutcome` enum
- [Source: src/pipeline.rs#L429-451] — `recover_and_process()` — calls `check_and_recover_wal()` then `resume_session()` then `process_recovered_session()`
- [Source: src/pipeline.rs#L456-631] — `process_recovered_session()` (PRIVATE — needs `pub` for AC #5)
- [Source: src/config/mod.rs#L329-375] — `BotConfig::_test_minimal()` (DO NOT USE — see Dev Notes)
- [Source: _bmad-output/planning-artifacts/architecture.md#L205-261] — Decision 3: WAL File for Crash & Context Limit Recovery
- [Source: _bmad-output/planning-artifacts/epics.md#L1020-1063] — Story 7.5 acceptance criteria
- [Source: _bmad-output/planning-artifacts/epics.md#L748-778] — Story 6.3 crash recovery spec
- [Source: _bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md] — Test infrastructure, lib.rs blocker, session::state visibility fix
- [Source: _bmad-output/implementation-artifacts/7-4-pipeline-orchestration-integration-tests.md#L126-258] — DI refactor (DevRunner/CodeReviewer traits, new_with_components)
- [Source: _bmad-output/project-context.md] — Rust edition 2024, async tokio, tracing, test mock pattern

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List