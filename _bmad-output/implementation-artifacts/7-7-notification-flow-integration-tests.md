# Story 7.7: Notification Flow Integration Tests

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want integration tests that verify notification construction and delivery logic,
So that I'm confident the daemon sends correct, well-formatted notifications.

## Acceptance Criteria

1. **Given** a `TelegramNotifier` constructed with a valid config and bot token
   **When** `notify_story()` is called with a `StoryNotification` (completed, with PR link)
   **Then** the formatted message contains the story ID, "completed" status, and the PR URL

2. **Given** a `NotificationConfig` with `telegram.enabled: false`
   **When** `create_notifier()` is called
   **Then** a `NoopNotifier` is returned (not a `TelegramNotifier`)
   **And** calling `notify_story()` on the noop notifier succeeds silently

3. **Given** a `NotificationConfig` with `telegram.enabled: true` but no bot token in secrets
   **When** `create_notifier()` is called
   **Then** a `NoopNotifier` is returned as graceful fallback
   **And** a warning is logged (not an error — notifications are non-blocking)

4. **Given** a list of `PipelineResult` items (2 completed, 1 failed, 1 blocked)
   **When** `build_run_summary()` constructs the `RunSummary`
   **Then** the summary correctly counts: 4 total, 2 completed, 1 failed, 1 blocked
   **And** `notify_run_summary()` on MockNotifier captures a message with all counts

## Tasks / Subtasks

- [ ] Task 0: Verify `src/lib.rs` prerequisite (AC: ALL — BLOCKER)
  - [ ] 0.1 If Story 7.1 Task 0 is NOT yet implemented, create `src/lib.rs` with `pub mod` declarations for all modules: `config`, `git_provider`, `notifier`, `pipeline`, `review`, `session`, `supervisor`, `tools`, `watcher`
  - [ ] 0.2 Update `src/main.rs` — remove corresponding `mod X;` lines (keep `mod cli;` binary-only) and replace with `use bmad_bot::*;` or selective imports
  - [ ] 0.3 Verify `cargo build` compiles and `cargo test` passes all existing 573+ unit tests

- [ ] Task 1: Create integration test file structure (AC: ALL)
  - [ ] 1.1 Create `tests/integration/test_notifier.rs` for all notification tests
  - [ ] 1.2 Register as `mod test_notifier;` in `tests/integration.rs` (create if doesn't exist)
  - [ ] 1.3 Add required imports: `bmad_bot::notifier::*`, `bmad_bot::config::*`

- [ ] Task 2: Test `TelegramNotifier` construction and type dispatch (AC: #1)
  - [ ] 2.1 `test_notifier_telegram_new_success` — construct `TelegramNotifier::new()` with `enabled: true` config and dummy bot token, verify `Ok` returned
  - [ ] 2.2 `test_notifier_telegram_new_disabled_returns_err` — construct with `enabled: false`, verify `Err(NotifierError::Disabled)` returned
  - [ ] 2.3 `test_notifier_story_notification_struct_construction` — construct `StoryNotification` with completed status and PR URL, verify all fields are accessible and correct from the external crate perspective

- [ ] Task 3: Test `create_notifier()` factory — disabled path (AC: #2)
  - [ ] 3.1 `test_notifier_factory_disabled_returns_noop` — construct `NotificationConfig` with `telegram.enabled: false`, call `create_notifier()`, call `notify_story()` on returned notifier, verify `Ok(())` returned (NoopNotifier behavior)
  - [ ] 3.2 `test_notifier_factory_disabled_notify_run_summary_succeeds` — same setup, call `notify_run_summary()` with a `RunSummary`, verify `Ok(())`

- [ ] Task 4: Test `create_notifier()` factory — graceful fallback path (AC: #3)
  - [ ] 4.1 `test_notifier_factory_enabled_no_token_returns_noop` — construct `NotificationConfig` with `telegram.enabled: true`, `BotSecrets` with `telegram_bot_token: None`, call `create_notifier()`, verify returned notifier behaves as NoopNotifier (returns `Ok(())` for both methods)
  - [ ] 4.2 `test_notifier_factory_enabled_empty_token_returns_noop` — same but with `telegram_bot_token: Some("")`, verify NoopNotifier fallback

- [ ] Task 5: Test `create_notifier()` factory — enabled + valid token path (AC: #1)
  - [ ] 5.1 `test_notifier_factory_enabled_with_token_returns_telegram` — construct with `enabled: true` and valid dummy token, call `create_notifier()`, verify returned notifier is NOT a NoopNotifier (call `notify_story()` — it will fail with HTTP error since no real Telegram server, but the error type confirms it's a TelegramNotifier attempting real send, not a NoopNotifier returning Ok)

- [ ] Task 6: Test `RunSummary` construction and `MockNotifier` capture (AC: #4)
  - [ ] 6.1 `test_notifier_run_summary_construction_counts` — construct `RunSummary` manually with 4 stories (2 completed, 1 blocked, 1 errored), verify field counts are correct
  - [ ] 6.2 `test_notifier_run_summary_mixed_statuses_on_mock` — construct `RunSummary`, call `notify_run_summary()` on a MockNotifier (from Story 7.1 helpers), verify the MockNotifier captured exactly 1 summary call with correct total/completed/blocked/errored counts
  - [ ] 6.3 `test_notifier_story_notifications_captured_by_mock` — send 3 `notify_story()` calls to MockNotifier with different statuses, verify all 3 captured with correct story_id, story_key, status, pr_url

- [ ] Task 7: Test `StoryStatus` display and data integrity from external crate (AC: #1, #4)
  - [ ] 7.1 `test_notifier_story_status_display_completed` — verify `StoryStatus::Completed.to_string()` contains "completed"
  - [ ] 7.2 `test_notifier_story_status_display_blocked` — verify `StoryStatus::Blocked.to_string()` contains "blocked"
  - [ ] 7.3 `test_notifier_story_status_display_error` — verify `StoryStatus::Error.to_string()` contains "error"

- [ ] Task 8: Test `NotifierError` variants (AC: ALL)
  - [ ] 8.1 `test_notifier_error_disabled_display` — verify `NotifierError::Disabled` display message
  - [ ] 8.2 `test_notifier_error_types_are_send_sync` — static assert that `NotifierError` is `Send + Sync`

## Dev Notes

### Cross-Module Integration Value

The `src/notifier/mod.rs` already contains 18+ unit tests covering formatting, HTML escaping, factory logic, and NoopNotifier behavior individually. **The real integration value of this story is:**

1. **Cross-module boundary (config → notifier) — Tasks 3-5:** The full chain `NotificationConfig` + `BotSecrets` → `create_notifier()` crosses from `config` into `notifier`. Unit tests in `mod.rs` use locally-constructed config values — integration tests import real `BotSecrets` and `NotificationConfig` types from `bmad_bot::config`.
2. **External crate perspective — All tasks:** Tests import via `bmad_bot::notifier::*` — exactly how the `tests/` crate sees the library. Any visibility regression breaks immediately.
3. **MockNotifier verification — Task 6:** Uses `MockNotifier` from Story 7.1 infrastructure to verify the notifier trait contract works correctly when consumed by the pipeline (or any caller).
4. **RunSummary data integrity — Task 6:** Constructs `RunSummary` with realistic data from the external crate and verifies field accuracy, complementing the pipeline's private `build_run_summary()` unit tests.

### Architecture Compliance

#### 🚨 CRITICAL — `src/lib.rs` Prerequisite (from Story 7.1 Task 0)

**The project is currently a pure binary crate** — `src/main.rs` only, no `src/lib.rs`. Without `lib.rs`, `use bmad_bot::anything;` will NOT compile.

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

Then update `src/main.rs` — remove all `mod X;` lines (except `mod cli;` which stays binary-only) and replace with selective imports from `bmad_bot::`.

**Verify:** `cargo build` + `cargo test` must pass with all 573+ existing unit tests.

#### Module Visibility — Already Correct

The `notifier` module exports everything needed for integration tests:

- `pub enum NotifierError` — all variants public (L24-51)
- `pub enum StoryStatus` — Completed, Blocked, Error (L59-66)
- `pub struct StoryNotification` — all fields public (L80-91)
- `pub struct RunSummary` — all fields public (L95-106)
- `pub trait Notifier` — async trait with `notify_story` + `notify_run_summary` (L125-131)
- `pub struct TelegramNotifier` — however fields are private (constructed via `new()`) (L258-265)
- `pub struct NoopNotifier` — unit struct, public (L365)
- `pub fn create_notifier()` — factory returning `Box<dyn Notifier>` (L397-424)

**Private (NOT accessible from integration tests):**
- `fn format_story_message()` — private (L152)
- `fn format_run_summary()` — private (L187)
- `fn truncate_message()` — private (L234)
- `fn escape_html()` — private (L145)
- `struct TelegramResponse` — private (L110)

**Implication:** AC1's requirement to verify "the formatted message contains the story ID, completed status, and the PR URL" cannot be tested by calling `format_story_message()` directly from integration tests. Instead:
- Verify `TelegramNotifier::new()` succeeds (construction is public)
- Verify `StoryNotification` struct fields carry correct data
- Verify `StoryStatus::Completed.to_string()` produces the expected display string
- The actual message formatting is already thoroughly tested by 4 unit tests in `mod.rs` (L485-544)
- For full AC1 coverage from integration tests, test that `create_notifier()` returns a working `TelegramNotifier` (Task 5.1 — calling `notify_story()` will attempt real HTTP, confirming it's NOT a NoopNotifier)

#### Private `format_story_message` — Design Decision

The formatting functions are intentionally private. **Do NOT make them public just for testing.** The integration test strategy for AC1 is:
1. Verify construction succeeds (`TelegramNotifier::new()`)
2. Verify data types carry correct values (`StoryNotification` fields)
3. Verify display traits work from external crate (`StoryStatus::Display`)
4. Trust unit tests for internal formatting correctness (4 dedicated unit tests exist)

#### Integration Test Location

All tests go in `tests/integration/test_notifier.rs`, declared in `tests/integration.rs`:
```rust
mod helpers;
mod test_notifier;
```

If `tests/integration.rs` doesn't exist yet (Story 7.1 not implemented), create the minimal structure:
```rust
mod test_notifier;
```

### Technical Requirements

#### Key Type Signatures (exact from codebase)

**`src/notifier/mod.rs`:**

```rust
// NotifierError — L24-51
#[derive(Debug, thiserror::Error)]
pub enum NotifierError {
    #[error("HTTP request failed: {reason}")]
    HttpRequest { reason: String },
    #[error("Telegram API error (HTTP {status}): {body}")]
    ApiError { status: u16, body: String },
    #[error("Response parse error: {reason}")]
    ResponseParse { reason: String },
    #[error("Telegram notifications are disabled")]
    Disabled,
}

// StoryStatus — L59-66
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoryStatus {
    Completed,
    Blocked,
    Error,
}

// StoryNotification — L80-91
#[derive(Debug, Clone)]
pub struct StoryNotification {
    pub story_id: String,
    pub story_key: String,
    pub status: StoryStatus,
    pub pr_url: Option<String>,
    pub reason: Option<String>,
}

// RunSummary — L95-106
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub stories: Vec<StoryNotification>,
    pub total_processed: usize,
    pub completed: usize,
    pub blocked: usize,
    pub errored: usize,
}

// Notifier trait — L125-131
#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError>;
    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError>;
}

// TelegramNotifier — L258-284
pub struct TelegramNotifier {
    http_client: ClientWithMiddleware,  // private
    bot_token: String,                  // private
    chat_id: String,                    // private
}

impl TelegramNotifier {
    pub fn new(config: &TelegramConfig, bot_token: String) -> Result<Self, NotifierError> { ... }
}

// NoopNotifier — L365
pub struct NoopNotifier;

// Factory — L397-424
pub fn create_notifier(config: &NotificationConfig, secrets: &BotSecrets) -> Box<dyn Notifier>
```

**`src/config/mod.rs`:**

```rust
// TelegramConfig — L182-189
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub chat_id: String,
}

// NotificationConfig — L175-178
pub struct NotificationConfig {
    pub telegram: TelegramConfig,
}

// BotSecrets — L380-395
pub struct BotSecrets {
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub github_models_api_key: Option<String>,
    pub github_token: Option<String>,
    pub gitlab_token: Option<String>,
    pub telegram_bot_token: Option<String>,
}
```

**`src/pipeline.rs` — `PipelineResult` is PUBLIC, `build_run_summary()` is PRIVATE:**

```rust
// PipelineResult — L94-103 — PUBLIC, accessible via bmad_bot::pipeline::PipelineResult
pub struct PipelineResult {
    pub story_key: String,
    pub status: StoryStatus,       // re-uses notifier::StoryStatus
    pub pr_url: Option<String>,
    pub error_detail: Option<String>,
}

// build_run_summary — L660-694 — PRIVATE, NOT accessible from integration tests
fn build_run_summary(results: &[PipelineResult]) -> RunSummary { ... }
```

**⚠️ AC4 mentions `PipelineResult` and `build_run_summary()` — here's the integration test strategy:**
- `PipelineResult` IS importable via `bmad_bot::pipeline::PipelineResult` — but do NOT use it in notifier tests (it's pipeline's domain)
- `build_run_summary()` is **private** — you CANNOT call it from integration tests
- **Workaround:** Construct `RunSummary` directly using its public fields. All fields are `pub`, so manual construction is trivial and tests the data contract from the external crate perspective
- The pipeline's `build_run_summary()` is already covered by 3 unit tests in `pipeline.rs` (L866-952)

#### `create_notifier()` Internal Logic (L397-424)

The factory function follows this decision tree:
1. If `config.telegram.enabled == false` → `Box::new(NoopNotifier)` + info log
2. If `enabled == true` but `secrets.telegram_bot_token` is `None` or empty → `Box::new(NoopNotifier)` + warn log
3. If `enabled == true` and token present → attempt `TelegramNotifier::new()`:
   - Success → `Box::new(notifier)`
   - Failure → `Box::new(NoopNotifier)` + warn log

This function **never fails** — it always returns a valid `Box<dyn Notifier>`.

#### Distinguishing TelegramNotifier from NoopNotifier

Since `create_notifier()` returns `Box<dyn Notifier>` (trait object), we cannot downcast to check the concrete type. The behavioral distinction is:
- **NoopNotifier:** `notify_story()` → `Ok(())` immediately, no side effects
- **TelegramNotifier:** `notify_story()` → attempts HTTP POST to `https://api.telegram.org/bot{token}/sendMessage` → will fail with `NotifierError::HttpRequest` since there's no real Telegram server in tests

**Test strategy for Task 5.1:** Call `notify_story()` on the returned notifier. If it returns `Ok(())` immediately, it's a NoopNotifier (FAIL). If it returns `Err(NotifierError::HttpRequest { .. })`, it's a TelegramNotifier that attempted a real send (PASS — confirms factory created the right type).

### Previous Story Intelligence (Story 7.6)

**Story 7.6 (Git Provider & PR Creation Integration Tests — IMPLEMENTED):**
- `lib.rs` already existed from Story 7.1 — Story 7.6 only verified it. No "BLOCKER" pattern to copy; lib.rs prerequisite is fully resolved for all downstream stories.
- Established the "Cross-Module Integration Value" section pattern — used here
- Used `tests/integration/test_git_provider.rs` with 12 tests. Module registered via `#[path = "integration/test_git_provider.rs"] mod test_git_provider;` in `tests/integration.rs` (Rust 2024 `#[path]` attribute pattern, NOT plain `mod`).
- Rustls crypto provider was needed for GitHub provider — **NOT needed for notifier tests**. Note: GitHub factory test required `#[tokio::test]` (not `#[test]`) because `GitHubProvider::new()` internally builds an Octocrab client that needs a Tokio runtime, even though `create_provider()` is synchronous.
- `GitHubProvider` and `GitLabProvider` do NOT implement `Debug` — cannot use `{:?}` on `Result` values containing them. Use explicit `match` arms with separate `Err(e) => panic!("...: {e}")` and `Ok(_) => panic!("...")` instead.
- `GitProvider` trait import is unnecessary when calling methods on `Box<dyn GitProvider>` — methods resolve through the dyn type. Importing it triggers `unused_imports` warning.
- Total integration tests after 7.6: 108 (12 new git provider tests).

**Story 7.4 (Pipeline Orchestration):**
- Defines `MockNotifier` with `Arc<Mutex<Vec<...>>>` for captured notifications — Task 6 depends on this
- `build_run_summary()` is tested in pipeline unit tests (3 tests: mixed, all-completed, empty) — integration tests complement by testing `RunSummary` construction from external crate

**Existing unit tests in `src/notifier/mod.rs` (18 tests):**
- `test_story_status_display_*` (3 tests) — Display trait formatting
- `test_escape_html_*` (2 tests) — HTML escaping
- `test_format_story_message_*` (4 tests) — message formatting for all statuses
- `test_format_run_summary_*` (3 tests) — summary formatting including truncation
- `test_noop_notifier_*` (2 tests) — NoopNotifier returns Ok
- `test_telegram_notifier_new_disabled` — disabled config returns Err
- `test_telegram_notifier_send_sync` — Send + Sync static assertion
- `test_create_notifier_disabled_returns_noop` — factory disabled path
- `test_create_notifier_enabled_returns_telegram` — factory enabled path

**Integration tests add value BEYOND these unit tests by:**
- Testing from external crate boundary (visibility regression detection)
- Using real `BotSecrets` + `NotificationConfig` structs from `bmad_bot::config`
- Using `MockNotifier` from Story 7.1 infrastructure for capture verification
- Constructing `RunSummary` manually to test data contract across crate boundary

### Git Intelligence

Recent commits (last 10):
- `8db8f88 docs(stories): create story 7-6 git provider PR creation integration tests and update sprint status`
- `80e7a09 docs(stories): create story 7-5 session WAL crash recovery integration tests and update sprint status`
- `f1b5f31 docs(stories): create story 7-4 pipeline orchestration integration tests`
- `2df7229 docs(stories): create story 7-3, fix critical lib.rs blocker across stories 7-1/7-2/7-3`
- `e10c275 docs(stories): create story 7-2 config startup validation integration tests`
- `1b260ab docs(stories): create story 7-1 integration test infrastructure`
- All 573+ unit tests passing on `main`
- Notifier implementation is stable (Story 6.1 completed, in review)
- No `src/lib.rs` exists yet — Story 7.1 Task 0 not implemented
- No `tests/integration/` directory exists yet

### Dependencies Required

- `tokio` — in `[dependencies]` (NOT `[dev-dependencies]`), so `#[tokio::test]` is available transitively via the `bmad_bot` library crate. No need to add `tokio` as a dev-dependency
- `async-trait` — in `[dependencies]` (NOT `[dev-dependencies]`), so `use async_trait::async_trait;` is available transitively via `bmad_bot`. Needed if creating a local `MockNotifier` that implements the `Notifier` trait
- `bmad_bot` — library crate (requires `lib.rs` from Task 0)
- Story 7.1 `MockNotifier` — for Task 6 capture tests. **If Story 7.1 is not yet implemented**, create a minimal local MockNotifier (see fallback code below)

#### MockNotifier Fallback — Use If Story 7.1 Not Yet Implemented

If `helpers/mocks.rs` doesn't exist yet, add this directly in `test_notifier.rs`:

```rust
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use bmad_bot::notifier::{Notifier, NotifierError, StoryNotification, RunSummary};

/// Minimal mock notifier that captures all calls for assertion.
pub struct MockNotifier {
    story_calls: Arc<Mutex<Vec<StoryNotification>>>,
    summary_calls: Arc<Mutex<Vec<RunSummary>>>,
}

impl MockNotifier {
    pub fn new() -> Self {
        Self {
            story_calls: Arc::new(Mutex::new(Vec::new())),
            summary_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn story_calls(&self) -> Vec<StoryNotification> {
        self.story_calls.lock().unwrap().clone()
    }

    pub fn summary_calls(&self) -> Vec<RunSummary> {
        self.summary_calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl Notifier for MockNotifier {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError> {
        self.story_calls.lock().unwrap().push(notification.clone());
        Ok(())
    }

    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError> {
        self.summary_calls.lock().unwrap().push(summary.clone());
        Ok(())
    }
}
```

**When Story 7.1 IS implemented:** Delete this local mock and switch to `use super::helpers::mocks::MockNotifier;` from the shared infrastructure.

### File Structure

```
tests/
├── integration.rs                           # Entry point (cargo test --test integration)
├── integration/
│   ├── helpers/
│   │   ├── mod.rs                           # Re-exports helper modules
│   │   ├── fixtures.rs                      # Shared fixture builders (from 7.1)
│   │   └── mocks.rs                         # Mock implementations (from 7.1)
│   ├── test_notifier.rs                     # ← THIS STORY's tests
│   ├── test_git_provider.rs                 # Story 7.6 tests
│   ├── test_session_wal.rs                  # Story 7.5 tests
│   ├── test_pipeline.rs                     # Story 7.4 tests
│   └── ...                                  # Other story test files
```

If Story 7.1 infrastructure is not yet built, create the minimal structure:
```
tests/
├── integration.rs                           # mod test_notifier;
├── integration/
│   └── test_notifier.rs                     # This story's tests
```

### Testing Standards

- **Framework:** `#[tokio::test]` for all tests — `notify_story()` and `notify_run_summary()` are async trait methods
- **Synchronous exceptions:** `TelegramNotifier::new()`, `create_notifier()`, `StoryStatus::to_string()` are sync — use `#[test]` for those
- **🚨 Anti-pattern: NEVER use `block_on()` in integration tests.** The existing unit test `test_create_notifier_disabled_returns_noop` uses `tokio::runtime::Runtime::new().unwrap() + rt.block_on()` inside a sync `#[test]` — do NOT follow this pattern. Integration tests should always use `#[tokio::test]` for async operations. Project context rule: *"No `block_on()` inside async context"*
- **Test naming:** `test_notifier_{behavior}_{scenario}` — e.g., `test_notifier_factory_disabled_returns_noop`
- **Structure:** Arrange → Act → Assert
- **Isolation:** No shared mutable state — each test constructs its own config/secrets/notifications
- **No real API calls:** Tests never send to real Telegram API. TelegramNotifier construction is tested (confirming type), but actual `send_message()` calls will fail with HTTP error (expected and asserted)
- **Zero warnings:** `cargo clippy` clean
- **MockNotifier:** If Story 7.1 infrastructure is available, use `MockNotifier` from `helpers/mocks.rs`. If not, use the fallback MockNotifier code from the "Dependencies Required" section above

#### `BotSecrets` Construction Helper for Tests

Every test calling `create_notifier()` must construct a full `BotSecrets`. Use this pattern:

```rust
fn make_test_secrets_with_telegram(token: Option<String>) -> BotSecrets {
    BotSecrets {
        anthropic_api_key: None,
        openai_api_key: None,
        github_models_api_key: None,
        github_token: None,
        gitlab_token: None,
        telegram_bot_token: token,
    }
}

// Usage examples:
// No token:     make_test_secrets_with_telegram(None)
// Empty token:  make_test_secrets_with_telegram(Some("".to_string()))
// Valid token:  make_test_secrets_with_telegram(Some("bot123:ABCDEF-test-DO-NOT-USE".to_string()))
```

Define this helper at the top of `test_notifier.rs` to avoid repetition across tests.

### Project Structure Notes

- Alignment with `tests/integration/` convention from Story 7.1 and Story 7.6
- `tests/e2e/` reserved for live API tests (gated behind `BMAD_E2E=1`) — do NOT modify
- Telegram API calls are E2E scope only — integration tests verify construction, factory logic, and data contracts
- `format_story_message()` and `format_run_summary()` are private — do NOT change visibility. Message formatting is thoroughly covered by 7 unit tests in `src/notifier/mod.rs`

### References

- [Source: src/notifier/mod.rs] — `Notifier` trait, `NotifierError`, `StoryStatus`, `StoryNotification`, `RunSummary`, `TelegramNotifier`, `NoopNotifier`, `create_notifier()`
- [Source: src/notifier/mod.rs#L431-745] — 18 existing unit tests (formatting, factory, noop, send+sync)
- [Source: src/notifier/mod.rs#L397-424] — `create_notifier()` factory decision tree
- [Source: src/notifier/mod.rs#L267-284] — `TelegramNotifier::new()` constructor
- [Source: src/notifier/mod.rs#L365-386] — `NoopNotifier` implementation
- [Source: src/config/mod.rs#L175-189] — `NotificationConfig`, `TelegramConfig` structs
- [Source: src/config/mod.rs#L380-395] — `BotSecrets` struct with `telegram_bot_token` field
- [Source: src/pipeline.rs#L660-694] — private `build_run_summary()` function (integration tests construct `RunSummary` directly instead)
- [Source: src/pipeline.rs#L866-952] — `build_run_summary` unit tests (3 tests covering mixed, all-completed, empty)
- [Source: _bmad-output/planning-artifacts/epics.md#L1105-1137] — Story 7.7 acceptance criteria
- [Source: _bmad-output/planning-artifacts/architecture.md#L510-542] — Test Mock Pattern
- [Source: _bmad-output/planning-artifacts/architecture.md#L561-607] — Project directory structure (notifier module location)
- [Source: _bmad-output/planning-artifacts/architecture.md#L673-687] — External integration points (Telegram API)
- [Source: _bmad-output/implementation-artifacts/7-6-git-provider-pr-creation-integration-tests.md] — lib.rs prerequisite pattern, test file convention
- [Source: _bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md] — MockNotifier spec, lib.rs Task 0 blocker
- [Source: _bmad-output/project-context.md] — Rust edition 2024, async tokio, test mock pattern, no real API calls in tests

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List