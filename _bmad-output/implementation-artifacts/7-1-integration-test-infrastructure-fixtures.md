# Story 7.1: Integration Test Infrastructure & Fixtures

Status: abandoned

## Story

As a developer,
I want a shared test infrastructure with mock implementations and fixture builders,
So that all integration tests can be written concisely and consistently.

## Acceptance Criteria

1. **Given** a new `tests/integration/` directory is created
   **When** I inspect the test helpers module
   **Then** the following mock implementations exist:
   - `MockGitProvider` implementing `GitProvider` trait — configurable to return `Ok(PrInfo { ... })` or `Err(GitProviderError::...)` for `create_pr`, `add_comment`, and `get_pr_url`
   - `MockNotifier` implementing `Notifier` trait — captures all `notify_story` and `notify_run_summary` calls into a `Vec` for later assertion
   - `MockSessionRunner` — standalone struct that returns a configurable `SessionOutcome` (Completed / Escalated / Failed)
   - `MockReviewRunner` — standalone struct that returns a configurable `ReviewOutcome` (Completed / Skipped / Failed)

2. **Given** the fixture module exists
   **When** I call fixture builder functions
   **Then** the following helpers are available:
   - `make_test_config(dir)` → valid `BotConfig` with sensible defaults (polling=60, provider=github, review=enabled)
   - `make_test_secrets()` → valid `BotSecrets` with dummy tokens (never real keys)
   - `make_test_story(key, label, deps)` → valid `StoryInfo` with specified key, label, branch, and dependency list
   - `write_sprint_status(dir, stories)` → writes a valid `sprint-status.yaml` to a temp directory with given story entries and statuses
   - `write_wal_file(dir, state)` → writes a valid `.bmad-bot-session.yaml` WAL file to a temp directory
   - `create_test_repo(dir)` → initializes a git repo with an initial commit in a temp directory

3. **Given** the test infrastructure is built
   **When** I run `cargo test --test integration`
   **Then** all infrastructure tests compile and pass
   **And** the mock implementations satisfy the trait bounds (`Send + Sync`)

## Tasks / Subtasks

- [ ] Task 0: Create `src/lib.rs` to expose crate modules for integration tests (AC: #3 — BLOCKER)
  - [ ] 0.1 Create `src/lib.rs` with `pub mod` declarations for all modules needed by integration tests: `config`, `watcher`, `git_provider`, `notifier`, `session`, `review`, `pipeline`
  - [ ] 0.2 Remove the corresponding `mod X;` declarations from `src/main.rs` and replace with `use bmad_bot::*;` or selective `use bmad_bot::{config, watcher, ...};` imports
  - [ ] 0.3 Keep `mod cli;` in `main.rs` (CLI is binary-only, not needed by integration tests)
  - [ ] 0.4 Add `pub use session::state::{SessionState, ChatMessage};` re-export in `src/session/mod.rs` (currently `mod state;` is private)
  - [ ] 0.5 Verify `cargo build` still compiles, `cargo test` passes all existing 573+ unit tests

- [ ] Task 1: Create `tests/integration/` directory structure (AC: #1, #3)
  - [ ] 1.1 Create `tests/integration.rs` as the Cargo test binary entry point
  - [ ] 1.2 Create `tests/integration/helpers/mod.rs` to re-export all helpers
  - [ ] 1.3 Create `tests/integration/helpers/mocks.rs` for mock implementations
  - [ ] 1.4 Create `tests/integration/helpers/fixtures.rs` for fixture builders

- [ ] Task 2: Implement `MockGitProvider` (AC: #1)
  - [ ] 2.1 Create struct with `Arc<Mutex<...>>` fields for configurable return values
  - [ ] 2.2 Implement `GitProvider` trait (`create_pr`, `add_comment`, `get_pr_url`)
  - [ ] 2.3 Add call-tracking `Vec` for assertions (which methods were called, with what args)
  - [ ] 2.4 Verify `Send + Sync` bound satisfaction

- [ ] Task 3: Implement `MockNotifier` (AC: #1)
  - [ ] 3.1 Create struct with `Arc<Mutex<Vec<...>>>` for captured notifications
  - [ ] 3.2 Implement `Notifier` trait (`notify_story`, `notify_run_summary`)
  - [ ] 3.3 Provide `calls()` / `story_calls()` / `summary_calls()` accessor methods for assertions
  - [ ] 3.4 Verify `Send + Sync` bound satisfaction

- [ ] Task 4: Implement `MockSessionRunner` (AC: #1)
  - [ ] 4.1 Create standalone struct with configurable `SessionOutcome` return
  - [ ] 4.2 Implement `async fn run(&self, story: &StoryInfo) -> SessionOutcome`
  - [ ] 4.3 Implement `async fn check_and_recover_wal(&self) -> Option<RecoveryInfo>` (returns None)
  - [ ] 4.4 Add call tracking for verification

- [ ] Task 5: Implement `MockReviewRunner` (AC: #1)
  - [ ] 5.1 Create standalone struct with configurable `ReviewOutcome` return
  - [ ] 5.2 Implement `async fn run(&self, story: &StoryInfo) -> ReviewOutcome`
  - [ ] 5.3 Add call tracking for verification

- [ ] Task 6: Implement fixture builder functions (AC: #2)
  - [ ] 6.1 `make_test_config(dir)` — builds a complete valid `BotConfig` using provided temp dir
  - [ ] 6.2 `make_test_secrets()` — builds `BotSecrets` with dummy tokens for all providers
  - [ ] 6.3 `make_test_story(key, label, deps)` — parses key to build complete `StoryInfo`
  - [ ] 6.4 `write_sprint_status(dir, entries)` — writes valid YAML from `Vec<(&str, &str)>` containing ALL entry types (epics, stories, retrospectives) under `development_status:`
  - [ ] 6.5 `write_wal_file(dir, state)` — writes valid WAL YAML from `SessionState`
  - [ ] 6.6 `create_test_repo(dir)` — initializes git repo with initial commit via Git CLI (`git init`, `git commit`)

- [ ] Task 7: Write self-verification tests (AC: #3)
  - [ ] 7.1 Test `MockGitProvider` returns configured values and tracks calls
  - [ ] 7.2 Test `MockNotifier` captures notifications correctly
  - [ ] 7.3 Test `MockSessionRunner` returns configured outcomes
  - [ ] 7.4 Test `MockReviewRunner` returns configured outcomes
  - [ ] 7.5 Test all fixture builders produce valid data structures
  - [ ] 7.6 Test `write_sprint_status` writes parseable YAML
  - [ ] 7.7 Test `write_wal_file` writes parseable WAL YAML
  - [ ] 7.8 Test `create_test_repo` creates a valid git repo with HEAD commit
  - [ ] 7.9 Test all mock types satisfy `Send + Sync` bounds

## Dev Notes

### Architecture Compliance

#### 🚨🚨 BLOCKER — Create `src/lib.rs` (Task 0)

**The project is currently a pure binary crate** (`src/main.rs` only, no `src/lib.rs`). All modules are declared as `mod X;` (private) in `main.rs`. Integration tests in `tests/` are **separate crates** and can ONLY import from a **library crate**. Without a `lib.rs`, `use bmad_bot::anything;` will not compile.

**Required fix — create `src/lib.rs`:**
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

**Then update `src/main.rs`** — remove all `mod X;` lines (except `mod cli;` which stays binary-only) and import from the library crate:
```rust
#![deny(clippy::all)]
#![warn(dead_code)]

mod cli;

use anyhow::Result;
use clap::Parser;
// All other modules now come from bmad_bot::* via lib.rs
```

**Why `mod cli;` stays in `main.rs`:** The CLI module is binary-specific (clap `Parser`, `main()` dispatch). Integration tests don't need it. Keeping it in `main.rs` avoids exposing binary concerns.

**Verify after this change:** `cargo build` succeeds, `cargo test` passes all 573+ existing unit tests. The unit tests inside each module (`#[cfg(test)] mod tests`) continue to work because they're part of the library crate.

#### Test Directory Convention
- Place all integration test code under `tests/integration/` with `tests/integration.rs` as the Cargo-discovered entry point
- The existing `tests/e2e/` directory is reserved for live LLM E2E tests (gated behind `BMAD_E2E=1`) — do NOT modify it
- Integration tests run via `cargo test --test integration` — deterministic, no real API calls, safe for CI

#### Module Visibility
- After creating `lib.rs`, types are accessible via `bmad_bot::{module}::{Type}` (e.g., `bmad_bot::config::BotConfig`, `bmad_bot::watcher::Watcher`)
- Key types that must be `pub` (and already are): `BotConfig`, `BotSecrets`, `StoryInfo`, `GitProvider`, `GitProviderError`, `CreatePrParams`, `PrInfo`, `Notifier`, `NotifierError`, `StoryNotification`, `RunSummary`, `StoryStatus`, `SessionOutcome`, `ReviewOutcome`, `SprintStatusFile`
- **🚨 `SessionState` and `ChatMessage` need re-export:** `src/session/mod.rs` declares `mod state;` (private). **Required fix in Task 0.4:** Add `pub use state::{SessionState, ChatMessage};` to `src/session/mod.rs`. Without this, `write_wal_file()` cannot use the `SessionState` type.
- If any other type needed by integration tests is not `pub`, adjust visibility minimally with `pub use` re-exports rather than making entire modules public

#### Mock Design Pattern
Follow the architecture's **Test Mock Pattern** from `architecture.md`:
- LLM responses: static/deterministic — never call real providers
- All mocks must be `Send + Sync` (required by `async_trait` bounds)
- Use `Arc<Mutex<Vec<...>>>` for interior mutability in async-safe mock state
- Test naming: `test_{module}_{behavior}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert, always in that order
- **Use a builder pattern** for mock configuration to keep future integration tests readable:
  ```rust
  MockGitProvider::new()
      .with_create_pr(Ok(PrInfo { id: "1".into(), url: "https://...".into(), number: 1 }))
      .with_add_comment(Ok(()))
  ```
  Each `with_*` method stores the return value; the trait impl returns it when called. This pattern scales cleanly across Stories 7.2–7.10.

#### MockNotifier vs. NoopNotifier
- The codebase already has `NoopNotifier` in `src/notifier/mod.rs` — it silently succeeds and discards all data (used when Telegram is disabled)
- `MockNotifier` is **different**: it captures every call into a `Vec` so tests can assert on what was sent, how many times, and with what data
- Do NOT reuse `NoopNotifier` for integration tests — always use `MockNotifier` when assertions on notification content are needed

### Technical Requirements

#### Trait Signatures to Mock (exact from codebase)

**`GitProvider` trait** (`src/git_provider/mod.rs`):
```rust
#[async_trait]
pub trait GitProvider: Send + Sync {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError>;
    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError>;
    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError>;
}
```

**`Notifier` trait** (`src/notifier/mod.rs`):
```rust
#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError>;
    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError>;
}
```

#### Session/Review Runner Note
`SessionRunner` and `ReviewRunner` are **concrete structs** (not traits) in the current codebase. They take `Arc<BotConfig>` + `Arc<BotSecrets>` and internally build rig agents. For mock purposes:
- Create standalone mock structs (`MockSessionRunner`, `MockReviewRunner`) that mimic the public API surface
- These do NOT implement a shared trait with the real runners (the codebase doesn't define one)
- Story 7.4 (Pipeline Orchestration Integration Tests) will address how to inject these mocks into `StoryPipeline` — likely by introducing a trait abstraction or a test-only constructor
- For this story, just build the mock structs with matching method signatures that return configurable outcomes

#### Existing Fixture Code to Reuse/Align With
- `src/watcher/mod.rs` has `pub(crate) fn make_test_bot_config(artifacts_dir: &Path) -> BotConfig` (L718-763) — use this as reference for `make_test_config()`, but the integration test version must be standalone (can't access `pub(crate)` from `tests/`)
- `src/config/mod.rs` has `pub fn _test_minimal(log_format, log_level)` — public but marked `#[doc(hidden)]`, limited (no path customization)
- `src/session/state.rs` has `fn make_test_story()` in its test module — pattern reference for `make_test_story()`

#### Key Type Structures (for fixture builders)

**`BotConfig`** requires:
- `polling_interval_secs: u64` (default 300, use 60 for tests)
- `git_provider: GitProviderConfig { provider, repo_owner, repo_name, target_branch }`
- `llm: LlmConfig { dev, review, supervisor }` — each `LlmRoleConfig { provider, model }`
- `notifications: NotificationConfig { telegram: TelegramConfig { enabled, chat_id } }`
- `bmad_paths: BmadPathsConfig { project_root, output_folder, planning_artifacts, implementation_artifacts }`
- `log_format: String`, `log_level: String`, `log_file: String`
- `code_review_enabled: bool`

**`BotSecrets`** requires:
- `anthropic_api_key: Option<String>` — use `Some("test-anthropic-key-DO-NOT-USE".into())`
- `openai_api_key: Option<String>` — use `Some("test-openai-key-DO-NOT-USE".into())`
- `github_models_api_key: Option<String>` — use `Some("test-ghmodels-key-DO-NOT-USE".into())`
- `github_token: Option<String>` — use `Some("test-github-token-DO-NOT-USE".into())`
- `gitlab_token: Option<String>` — use `Some("test-gitlab-token-DO-NOT-USE".into())`
- `telegram_bot_token: Option<String>` — use `Some("test-telegram-token-DO-NOT-USE".into())`

**`StoryInfo`** requires:
- Parse `key` (e.g., `"7-1-integration-test-infrastructure"`) to extract `epic_num`, `story_num`, `label`
- `story_id`: `"{epic_num}.{story_num}"`
- `branch_name`: `"story/{key}"`
- `specs_path`: `PathBuf::from(format!("_bmad-output/implementation-artifacts/{key}.md"))`
- `dependencies`: `Vec<String>` from provided `deps` parameter
- `status`: `"ready-for-dev"` by default

**`SessionState`** for WAL writing:
- `story_id`, `story_key`, `branch`, `started_at`, `last_activity`, `provider`, `model`
- `branch_name`, `base_branch` (default empty strings, `serde(default)`)
- `chat_history: Vec<ChatMessage>` where `ChatMessage { role, content }`

#### Sprint-Status YAML Format
```yaml
generated: 2026-02-08
project: test-project
project_key: TEST
tracking_system: file-system
story_location: "{dir}"

development_status:
  epic-1: in-progress
  1-1-story-slug: ready-for-dev
  1-2-another-story: backlog
  epic-1-retrospective: optional
```

**🚨 CRITICAL — `write_sprint_status()` must write ALL entry types:**
The `entries` parameter accepts epic entries (`"epic-1", "in-progress"`), story entries (`"1-1-slug", "ready-for-dev"`), and retrospective entries (`"epic-1-retrospective", "optional"`). All go under `development_status:` as flat key-value pairs. `SprintStatusFile::load()` parses the entire mapping and `stories()` filters out non-story entries internally.

**🚨 CRITICAL — Sprint-status YAML comments are NOT functional:**
The real `sprint-status.yaml` has comments like `# depends-on: 7-1`. These are **YAML comments stripped by the parser** — they have ZERO effect on dependency resolution. Dependencies are computed **exclusively** by `derive_dependencies()` from story numbering: story N.M depends on N.(M-1) within the same epic. Never write tests that rely on YAML comments for dependency data.

#### Git Repo Initialization (via Git CLI)

> **Post Story 4.4:** `git2` has been removed from the project entirely. All git operations — including test fixtures — use Git CLI subprocess calls.

```rust
fn create_test_repo(dir: &Path) {
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

### Dependencies Required
All already present in `Cargo.toml`:
- `tempfile = "3"` (dev-dependency) — for isolated temp directories
- `async-trait = "0.1"` — for trait impls in mocks (already a main dependency)
- `tokio` with `full` features — for async test runtime (already a main dependency)
- `serde_yml = "0.0.12"` — for YAML serialization in fixture writers

No new dependencies needed. Note: `git2` was removed from the project in Story 4.4 — test fixtures use Git CLI instead.

### File Structure to Create

```
src/
├── lib.rs                   ← NEW (Task 0 — BLOCKER: enables integration test imports)
├── main.rs                  ← MODIFIED (remove mod declarations, use bmad_bot::*)
└── session/
    └── mod.rs               ← MODIFIED (add pub use state::{SessionState, ChatMessage};)

tests/
├── e2e/
│   └── mod.rs              # (existing — DO NOT TOUCH)
├── integration.rs           ← NEW (Cargo test binary entry point)
└── integration/
    ├── helpers/
    │   ├── mod.rs           # Re-exports: pub mod mocks; pub mod fixtures;
    │   ├── mocks.rs         # MockGitProvider, MockNotifier, MockSessionRunner, MockReviewRunner
    │   └── fixtures.rs      # make_test_config, make_test_secrets, make_test_story, write_sprint_status, write_wal_file, create_test_repo
    ├── test_mocks.rs        # Self-verification tests for mock implementations
    └── test_fixtures.rs     # Self-verification tests for fixture builders
```

#### Integration Test Entry Point Pattern — Cargo Convention
**🚨 CRITICAL: Cargo test discovery depends on exact naming.**

The correct multi-file integration test layout is:
```
tests/
├── integration.rs           # Cargo discovers this as a test binary → `cargo test --test integration`
└── integration/             # Submodule directory for integration.rs
    ├── helpers/
    │   ├── mod.rs           # pub mod mocks; pub mod fixtures;
    │   ├── mocks.rs         # MockGitProvider, MockNotifier, MockSessionRunner, MockReviewRunner
    │   └── fixtures.rs      # make_test_config, make_test_secrets, etc.
    ├── test_mocks.rs        # Self-verification tests for mock implementations
    └── test_fixtures.rs     # Self-verification tests for fixture builders
```

**How it works:**
- `tests/integration.rs` is the **test binary entry point** — Cargo discovers it automatically
- `tests/integration/` is the **submodule directory** that Cargo links to `integration.rs`
- Inside `tests/integration.rs`, declare: `mod helpers;` and `mod test_mocks;` and `mod test_fixtures;`
- Running `cargo test --test integration` compiles and runs this binary only
- **Do NOT** name it `tests/integration/mod.rs` alone — Cargo won't discover it as a test binary without the root `tests/integration.rs` file

### Project Structure Notes

- Alignment: Integration tests go in `tests/` per project-context.md and architecture.md
- The existing `tests/e2e/mod.rs` follows a different pattern (single file). The integration tests are more complex and warrant a directory structure
- Naming: `tests/integration.rs` + `tests/integration/` directory follows standard Cargo multi-file integration test layout

### Testing Standards
- Use `#[tokio::test]` for all async tests
- Use `tempfile::tempdir()` for every test that touches the filesystem
- Never leave test artifacts on disk — tempdir handles cleanup via Drop
- All assertions use `assert!`, `assert_eq!`, `assert_ne!` — no `unwrap()` in assertions (use `.expect("reason")` if needed)
- Test names: `test_{component}_{behavior}_{scenario}`
- **Tracing is a no-op in tests:** Many modules call `tracing::info!()` / `tracing::warn!()`. Without a subscriber initialized, these are silent no-ops. Do NOT install a tracing subscriber in integration tests unless explicitly debugging — it adds noise without value.

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Epic 7 Overview (L812-822)]
- [Source: _bmad-output/planning-artifacts/epics.md — Integration Test Strategy (L822-856)]
- [Source: _bmad-output/planning-artifacts/epics.md — Story 7.1 (L856-897)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Test Mock Pattern (L510-542)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Project Structure (L561-607)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Enforcement Guidelines (L542-561)]
- [Source: _bmad-output/project-context.md — Testing Rules section]
- [Source: _bmad-output/project-context.md — Critical Don't-Miss Rules section]
- [Source: src/git_provider/mod.rs — GitProvider trait (L124-133)]
- [Source: src/notifier/mod.rs — Notifier trait (L125-131)]
- [Source: src/session/mod.rs — SessionOutcome enum]
- [Source: src/review/mod.rs — ReviewOutcome enum (L108-130)]
- [Source: src/session/runner.rs — SessionRunner struct (L124-133)]
- [Source: src/review/mod.rs — ReviewRunner struct (L140-147)]
- [Source: src/config/mod.rs — BotConfig (L75-107), BotSecrets (L380-393)]
- [Source: src/watcher/mod.rs — StoryInfo (L66-86), make_test_bot_config (L718-763)]
- [Source: src/session/state.rs — SessionState (L82-111), ChatMessage (L23-28)]
- [Source: Cargo.toml — dev-dependencies: tempfile already available]
- [Source: Story 4.4 — git2 removed from project, all git operations now use Git CLI subprocess]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List