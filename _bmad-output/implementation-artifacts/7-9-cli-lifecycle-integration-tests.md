# Story 7.9: CLI Lifecycle Integration Tests

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want integration tests that verify the CLI commands interact correctly with daemon state,
So that I'm confident the user experience of init → start → status → logs → stop is coherent.

## Acceptance Criteria

1. **Given** a temp directory with no daemon state file
   **When** `DaemonState::read()` is called
   **Then** `Ok(None)` is returned

2. **Given** a `DaemonState::new_running()` is created and written to a temp state file
   **When** `DaemonState::read()` is called on that file
   **Then** the state is deserialized correctly with matching PID, started_at, and status "running"

3. **Given** a running state is written
   **When** `touch()` is called, then `record_story_processed()` twice, then state is re-written and re-read
   **Then** `stories_processed == 2` and `last_activity` is updated

4. **Given** a running state is written
   **When** `mark_stopped()` is called and state is re-written
   **Then** re-reading shows `status: "stopped"`

5. **Given** a state file exists
   **When** `cleanup()` is called
   **Then** the file is removed
   **And** subsequent `read()` returns `Ok(None)`

6. **Given** a valid `bmad-bot.yaml` is generated via the init flow helpers
   **When** `BotConfig::load()` is called on the generated file
   **Then** the config loads and validates successfully (round-trip test)

## Tasks / Subtasks

- [ ] Task 0: Ensure `src/lib.rs` prerequisite + resolve `cli` module accessibility (AC: ALL — BLOCKER)
  - [ ] 0.1 If `src/lib.rs` does not exist (Story 7.1 Task 0 not yet done), create it with `pub mod` declarations for all modules. See Story `7-1-integration-test-infrastructure-fixtures.md` Task 0.
  - [ ] 0.2 🚨 CRITICAL: Add `pub mod cli;` to `src/lib.rs` so that `DaemonState` is accessible from integration tests (see Architecture Compliance section for full rationale)
  - [ ] 0.3 Remove `mod cli;` from `src/main.rs` (it now comes from `bmad_bot::cli`)
  - [ ] 0.4 Update `src/main.rs` to import CLI types from `bmad_bot::cli::*` instead of the local `mod cli;`
  - [ ] 0.5 Verify `cargo build` + `cargo test` pass with all existing unit tests

- [ ] Task 1: Create integration test file structure (AC: ALL)
  - [ ] 1.1 If `tests/integration.rs` does not exist yet, create it as the Cargo test binary entry point
  - [ ] 1.2 If `tests/integration/helpers/` does not exist, create the directory structure
  - [ ] 1.3 Create `tests/integration/test_cli_lifecycle.rs` for all Story 7.9 tests
  - [ ] 1.4 Declare `mod test_cli_lifecycle;` in `tests/integration.rs`

- [ ] Task 2: Implement DaemonState read/write roundtrip tests (AC: #1, #2)
  - [ ] 2.1 Test: `read()` on non-existent file returns `Ok(None)`
  - [ ] 2.2 Test: construct `DaemonState` manually → `write()` → `read()` → verify all fields match (pid, started_at, status, stories_processed, log_file)
  - [ ] 2.3 Test: `new_running()` → `write()` → `read()` → verify pid matches `std::process::id()`, status is "running", stories_processed is 0

- [ ] Task 3: Implement DaemonState mutation + persistence tests (AC: #3, #4)
  - [ ] 3.1 Test: `new_running()` → `touch()` → `record_story_processed()` × 2 → `write()` → `read()` → assert `stories_processed == 2` and `last_activity` differs from `started_at`
  - [ ] 3.2 Test: `new_running()` → `write()` → re-read → `mark_stopped()` → `write()` → re-read → assert status is "stopped" and `last_activity` is updated
  - [ ] 3.3 Test: verify `touch()` updates `last_activity` but not `status` or `stories_processed`

- [ ] Task 4: Implement DaemonState cleanup tests (AC: #5)
  - [ ] 4.1 Test: `write()` state → verify file exists → `cleanup()` → verify file removed → `read()` returns `Ok(None)`
  - [ ] 4.2 Test: `cleanup()` on non-existent file does not error (idempotent)

- [ ] Task 5: Implement BotConfig load roundtrip test (AC: #6)
  - [ ] 5.1 Test: construct valid `BotConfig` programmatically → serialize to YAML with `serde_yml::to_string()` → write to temp file → `BotConfig::load()` → `validate()` → assert all fields match
  - [ ] 5.2 Test: load from a malformed YAML file → verify `ConfigError` is returned (not a panic) — NOTE: this overlaps with unit test `test_config_invalid_yaml_returns_parse_error` (config/mod.rs L989); keep as a lightweight sanity check from the integration test binary, don't over-specify

- [ ] Task 6: Implement cross-concern integration tests (AC: ALL)
  - [ ] 6.1 Test: full lifecycle — create state → write → touch → record 3 stories → mark_stopped → write → read → verify final state is coherent (stopped, 3 stories, timestamps monotonically ordered)
  - [ ] 6.2 Test: state file is valid JSON — write state → read raw file content → parse as `serde_json::Value` → verify all expected keys exist

## Dev Notes

### Cross-Module Integration Value

This story tests the **daemon state lifecycle** that underpins the CLI user experience:

| Module | Responsibility | Key Types |
|--------|---------------|-----------|
| `cli/state.rs` | Daemon state persistence (JSON file) | `DaemonState`, `STATE_FILE_NAME` |
| `cli/mod.rs` | CLI commands that read/write state | `CliError`, `run_status()`, `run_start()` |
| `config/mod.rs` | Configuration loading + validation | `BotConfig`, `ConfigError`, `BotConfig::load()` |
| `config/discovery.rs` | BMAD auto-discovery | `BmadDiscovery` |

**Why integration tests matter here:** Unit tests in `cli/state.rs` already verify each method individually (14 tests). Integration tests validate the **full lifecycle sequence** — create → mutate → persist → re-read → cleanup — ensuring the JSON serialization roundtrip preserves all state correctly across multiple mutation steps.

### Architecture Compliance

#### 🚨🚨 BLOCKER — `cli` Module Must Be in `lib.rs` for This Story

**Problem:** All previous Epic 7 stories state "`mod cli;` stays in `main.rs` — binary-only." However, `DaemonState` lives in `cli::state`, and integration tests (separate crates in `tests/`) can ONLY access types exported through `lib.rs`. Without `cli` in `lib.rs`, **ACs #1-#5 are impossible to implement.**

**Solution — Add `pub mod cli;` to `lib.rs`:**

```rust
// src/lib.rs
//! bmad-bot library crate — exposes modules for integration tests.
#![deny(clippy::all)]
#![warn(dead_code)]

pub mod cli;        // ← ADDED for Story 7.9 — DaemonState accessibility
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

**Then update `src/main.rs`:**

```rust
#![deny(clippy::all)]
#![warn(dead_code)]

// cli is now in lib.rs — no local `mod cli;` needed
use bmad_bot::cli;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let cli_args = cli::Cli::parse();
    match cli_args.command {
        cli::Commands::Start => { cli::run_start(&cli_args.config).await?; }
        cli::Commands::Init => {
            let _ = tracing_subscriber::fmt::try_init();
            cli::run_init(&cli_args.config).await?;
        }
        cli::Commands::Status => {
            let _ = tracing_subscriber::fmt::try_init();
            cli::run_status(&cli_args.config).await?;
        }
        cli::Commands::Logs { level, tail } => {
            let _ = tracing_subscriber::fmt::try_init();
            cli::run_logs(&cli_args.config, level, Some(tail)).await?;
        }
    }
    Ok(())
}
```

**Why this is safe:**
- `cli` module has no binary-specific code (no `fn main()`). The `#[tokio::main]` and argument parsing are in `main.rs` itself.
- All `cli` dependencies (`clap`, `dialoguer`, `tracing-subscriber`) are already in `[dependencies]`, not `[dev-dependencies]`.
- Moving `cli` to `lib.rs` adds no runtime overhead — it just makes the module visible to `tests/`.
- The existing 70+ unit tests in `cli/mod.rs` and `cli/state.rs` continue to work unchanged.

**Verify after this change:** `cargo build` + `cargo test` must pass all existing tests.

#### Module Visibility — ✅ Confirmed Public

Once `cli` is in `lib.rs`, the following are accessible from integration tests:

- ✅ `cli::state::DaemonState` — all fields `pub`, derives `Serialize` + `Deserialize`
- ✅ `cli::state::STATE_FILE_NAME` — `pub const`
- ✅ `cli::CliError` — `pub enum` with `State` variant (returned by `DaemonState::read/write/cleanup`)
- ✅ `config::BotConfig` — `pub struct` with `pub fn load()` and `pub fn validate()`
- ✅ `config::ConfigError` — `pub enum` (returned by `BotConfig::load`)
- ✅ `config::discovery::BmadDiscovery` — `pub struct` with all `pub` fields (required by `DaemonState::new_running()`)

#### ⚠️ `CliError` Contains `#[from] ConfigError` and `#[from] std::io::Error`

`DaemonState::write()` and `read()` return `Result<_, CliError>`. In integration tests, you'll work with `CliError` directly. Pattern match on `CliError::State { reason }` for state-specific errors, or `CliError::Io(_)` for filesystem errors.

#### Integration Test Location

All tests go in `tests/integration/test_cli_lifecycle.rs`, declared in `tests/integration.rs`:

```rust
mod helpers;
mod test_cli_lifecycle;
```

If `tests/integration.rs` doesn't exist yet (Story 7.1 not implemented), create the minimal structure:

```rust
mod helpers;
mod test_cli_lifecycle;
```

### Technical Requirements

#### Quick API Reference

| Function | Module | Signature Summary |
|----------|--------|-------------------|
| `DaemonState::new_running` | `cli::state` (L39) | `(log_file: PathBuf, bmad_discovery: BmadDiscovery) -> Self` |
| `DaemonState::read` | `cli::state` (L82) | `(path: &Path) -> Result<Option<Self>, CliError>` |
| `DaemonState::write` | `cli::state` (L72) | `(&self, path: &Path) -> Result<(), CliError>` |
| `DaemonState::touch` | `cli::state` (L63) | `(&mut self)` — updates `last_activity` to now |
| `DaemonState::record_story_processed` | `cli::state` (L57) | `(&mut self)` — increments `stories_processed` by 1 |
| `DaemonState::mark_stopped` | `cli::state` (L67) | `(&mut self)` — sets status to "stopped", updates `last_activity` |
| `DaemonState::cleanup` | `cli::state` (L108) | `(path: &Path) -> Result<(), CliError>` — removes state file |
| `DaemonState::is_process_alive` | `cli::state` (L96) | `(pid: u32) -> bool` — POSIX `kill -0` check |
| `BotConfig::load` | `config` (L219) | `(path: &Path) -> Result<Self, ConfigError>` |
| `BotConfig::validate` | `config` (L232) | `(&self) -> Result<(), ConfigError>` |
| `BotConfig::_test_minimal` | `config` (L331) | `(log_format: &str, log_level: &str) -> Self` — public but `#[doc(hidden)]` |

**Key types:** `DaemonState` (7 pub fields: `pid`, `started_at`, `last_activity`, `status`, `log_file`, `bmad_discovery`, `stories_processed`), `BmadDiscovery` (5 pub fields: `bmad_version`, `installed_modules`, `config_path`, `project_root`, `bmad_detected`). See source references at bottom for exact definitions.

**`is_process_alive()` is intentionally NOT tested in this story.** It's a POSIX-specific function (`kill -0`) that tests the OS, not our code. Already covered by 2 unit tests in `cli/state.rs` (`test_is_process_alive_with_current_pid`, `test_is_process_alive_with_zero_pid`).

#### API Behavior Notes

**`DaemonState` serializes to JSON** (not YAML). The state file is `bmad-bot.state.json`. `write()` uses atomic write (tmp + rename). `read()` returns `Ok(None)` if the file doesn't exist — not an error.

**`DaemonState::new_running()` captures the current PID** via `std::process::id()`. In integration tests, the PID will be the test process PID. This is expected — assert `state.pid == std::process::id()`.

**`DaemonState::new_running()` requires a `BmadDiscovery` argument.** You cannot construct a `DaemonState` via `new_running()` without providing one. For tests, construct a minimal `BmadDiscovery` directly:

```rust
let discovery = bmad_bot::config::discovery::BmadDiscovery {
    bmad_version: Some("6.0.0-test".to_string()),
    installed_modules: vec!["bmm".to_string()],
    config_path: None,
    project_root: std::path::PathBuf::from("."),
    bmad_detected: true,
};
```

**Alternative: construct `DaemonState` manually** (all fields are pub):

```rust
let state = bmad_bot::cli::state::DaemonState {
    pid: std::process::id(),
    started_at: "2026-02-08T10:00:00+01:00".to_string(),
    last_activity: "2026-02-08T10:00:00+01:00".to_string(),
    status: "running".to_string(),
    log_file: std::path::PathBuf::from("test.log"),
    bmad_discovery: None,
    stories_processed: 0,
};
```

This avoids the `BmadDiscovery` dependency and gives full control over timestamp values for deterministic assertions.

**`touch()` and `mark_stopped()` capture `chrono::Local::now()`** — timestamps will differ between calls. For assertions, check that `last_activity` CHANGED (not equal to the original), don't assert on exact values.

**🚨 CRITICAL — Add `thread::sleep` between timestamp-dependent operations.** On fast machines, two calls to `chrono::Local::now()` within the same millisecond produce identical timestamps, causing flaky tests. The existing unit tests in `cli/state.rs` use this pattern:

```rust
std::thread::sleep(std::time::Duration::from_millis(10));
```

Insert this sleep BETWEEN `new_running()` / `write()` and any subsequent `touch()` or `mark_stopped()` call. See `test_touch_updates_last_activity` (L193) and `test_mark_stopped_updates_last_activity` (L255) for reference.

**`BotConfig::_test_minimal()` is `#[doc(hidden)]` but `pub`.** It builds a valid config with sensible defaults. However, it takes only `(log_format, log_level)` as parameters — all other fields use hardcoded test values. For the roundtrip test (AC #6), it's simpler to construct a full `BotConfig` manually or use `_test_minimal("pretty", "info")` then serialize + load.

**`BotConfig::load()` does NOT call `validate()` automatically.** The test must call both: `load()` then `validate()`.

#### BotConfig Roundtrip Test Pattern

AC #6 requires generating a valid config file and loading it back. The cleanest approach:

```rust
// Construct a valid BotConfig
let config = bmad_bot::config::BotConfig::_test_minimal("pretty", "info");

// Serialize to YAML
let yaml = serde_yml::to_string(&config).expect("serialize");

// Write to temp file
let dir = tempfile::tempdir().expect("tempdir");
let config_path = dir.path().join("bmad-bot.yaml");
std::fs::write(&config_path, &yaml).expect("write");

// Load + validate
let loaded = bmad_bot::config::BotConfig::load(&config_path).expect("load");
loaded.validate().expect("validate");

// Assert key fields match
assert_eq!(loaded.polling_interval_secs, config.polling_interval_secs);
assert_eq!(loaded.git_provider.provider, config.git_provider.provider);
assert_eq!(loaded.llm.dev.provider, config.llm.dev.provider);
```

**Do NOT test `generate_config_yaml()` from integration tests** — it's a private function in `cli/mod.rs` (L514). The unit tests in `cli/mod.rs` already cover it with `test_generate_config_yaml_roundtrips` and `test_generate_config_yaml_validates`.

### Previous Story Intelligence (Stories 7.1 through 7.8)

Key patterns from reviewing all previous stories:

1. **`lib.rs` blocker is resolved** — Story 7.1 expanded `src/lib.rs` to 12 `pub mod` declarations. `main.rs` retains `mod X;` (dual-crate compilation, `crate::` paths preserved for CLI). Integration tests import via `bmad_bot::`.

2. **Test module registration requires `#[path]` attributes** — e.g., `#[path = "integration/test_cli_lifecycle.rs"] mod test_cli_lifecycle;` in `tests/integration.rs`. Direct `mod test_cli_lifecycle;` does NOT resolve. This story adds `pub mod cli;` which previous stories explicitly excluded. This is the correct resolution for accessing `DaemonState`.

3. **Test file naming convention:** `test_{module_name}.rs`. For this story: `test_cli_lifecycle.rs`.

4. **No mocks needed for this story.** All tests operate on real filesystem state files and real config YAML files in temp directories. No LLM, HTTP, or git mocking required.

5. **`make_test_config(dir)` path layout:** `project_root` = `dir`, `implementation_artifacts` = `dir/_bmad-output/implementation-artifacts`.

6. **Story 7.2 (Config Startup Validation)** also tests `BotConfig::load()` — but focuses on validation edge cases (missing fields, invalid values). Story 7.9 AC #6 tests the happy-path roundtrip only. No overlap.

7. **Story 7.8 (Branch Management)** established the pattern of confirming module visibility definitively (✅ instead of ⚠️) and providing a Quick API Reference table. This story follows the same pattern.

### Git Intelligence

Recent commits (last 5):

```
81e0064 docs(stories): create story 7-8 branch management git tools integration tests and update sprint status
ad4e6e8 docs: add comprehensive README with architecture, quick start, and CLI reference
60def59 docs(stories): create story 7-7 notification flow integration tests and update sprint status
8db8f88 docs(stories): create story 7-6 git provider PR creation integration tests and update sprint status
80e7a09 docs(stories): create story 7-5 session WAL crash recovery integration tests and update sprint status
```

No Epic 7 implementation code committed yet. All stories 7.1–7.8 are `ready-for-dev`.

### Dependencies Required

All already present in `Cargo.toml`:
- `serde_json = "1"` — for reading state file JSON in verification tests
- `serde_yml = "0.0.12"` — for config YAML serialization in roundtrip test
- `tempfile = "3"` — dev-dependency for isolated temp directories
- `chrono = "0.4"` — already a main dependency (used by `DaemonState` internally)

**No new dependencies needed.**

### Required Imports for Test File

Every integration test file for this story needs these imports at minimum:

```rust
use bmad_bot::cli::state::{DaemonState, STATE_FILE_NAME};
use bmad_bot::cli::CliError;
use bmad_bot::config::BotConfig;
use bmad_bot::config::discovery::BmadDiscovery;
use std::path::PathBuf;
use tempfile::TempDir;
```

### File Structure

```
src/
├── lib.rs                   ← MODIFIED: add `pub mod cli;` (BLOCKER for this story)
├── main.rs                  ← MODIFIED: remove `mod cli;`, import from bmad_bot::cli
└── cli/
    ├── mod.rs               ← ✅ Already has: pub mod state;
    └── state.rs             ← ✅ DaemonState, all fields pub, Serialize + Deserialize

tests/
├── e2e/
│   └── mod.rs              # (existing — DO NOT TOUCH)
├── integration.rs           ← NEW if not exists (Cargo test binary entry point)
└── integration/
    ├── helpers/
    │   └── mod.rs           # Re-exports (if needed by other stories)
    └── test_cli_lifecycle.rs  ← NEW (all Story 7.9 tests)
```

### Testing Standards

- **Framework:** All tests can use `#[test]` (no async needed — `DaemonState` methods are sync, `BotConfig::load` is sync)
- **Isolation:** Every test creates its own `tempfile::tempdir()` — no shared state between tests
- **Naming:** `test_{component}_{behavior}_{scenario}` in snake_case
- **Structure:** Arrange → Act → Assert, always in that order
- **Timestamps:** Never assert on exact timestamp values — assert that `last_activity` changed (`!=` original) or use string comparison for format validation (starts with `20`, contains `T`). **Always insert `std::thread::sleep(Duration::from_millis(10))` between timestamp-producing operations** to prevent same-millisecond flakiness.
- **PID:** In tests, `std::process::id()` returns the test runner's PID. Assert equality with `state.pid`, don't hardcode values
- **Assertions:** Use `assert!`, `assert_eq!`, `assert_ne!` — use `.expect("reason")` for unwraps
- **Cleanup:** `TempDir` Drop handles cleanup — no manual cleanup needed
- **All tests must complete in < 2 seconds** — all operations are local filesystem JSON reads/writes

### Project Structure Notes

- Alignment with unified project structure: integration tests in `tests/` per `project-context.md` and `architecture.md`
- Existing `tests/e2e/mod.rs` is reserved for live LLM E2E tests (gated behind `BMAD_E2E=1`) — do NOT modify
- State file format is JSON (not YAML) — `bmad-bot.state.json`
- Config file format is YAML — `bmad-bot.yaml`
- `DaemonState::write()` uses atomic write pattern (write to `.tmp`, then rename) for crash safety

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Epic 7 Overview (L854-864)]
- [Source: _bmad-output/planning-artifacts/epics.md — Integration Test Strategy (L864-898)]
- [Source: _bmad-output/planning-artifacts/epics.md — Story 7.9 (L1216-1254)]
- [Source: _bmad-output/planning-artifacts/epics.md — Epic Summary (L1287-1312)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Project Structure (L561-607)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Test Mock Pattern (L510-542)]
- [Source: _bmad-output/project-context.md — CLI Rules section]
- [Source: _bmad-output/project-context.md — Testing Rules section]
- [Source: _bmad-output/project-context.md — Critical Don't-Miss Rules section]
- [Source: src/cli/state.rs — DaemonState struct (L17-34)]
- [Source: src/cli/state.rs — DaemonState::new_running (L39-51)]
- [Source: src/cli/state.rs — DaemonState::record_story_processed (L57-59)]
- [Source: src/cli/state.rs — DaemonState::touch (L63-65)]
- [Source: src/cli/state.rs — DaemonState::mark_stopped (L67-70)]
- [Source: src/cli/state.rs — DaemonState::write (L72-81)]
- [Source: src/cli/state.rs — DaemonState::read (L82-93)]
- [Source: src/cli/state.rs — DaemonState::is_process_alive (L96-106)]
- [Source: src/cli/state.rs — DaemonState::cleanup (L108-113)]
- [Source: src/cli/state.rs — unit tests (L120-260) — 14 tests]
- [Source: src/cli/mod.rs — CliError (L72-117)]
- [Source: src/cli/mod.rs — pub mod state (L8)]
- [Source: src/cli/mod.rs — generate_config_yaml (L514-529, PRIVATE)]
- [Source: src/cli/mod.rs — run_status (L720-798)]
- [Source: src/cli/mod.rs — unit tests (L1111-1785) — 52 tests]
- [Source: src/config/mod.rs — BotConfig (L75-107)]
- [Source: src/config/mod.rs — BotConfig::load (L219-226)]
- [Source: src/config/mod.rs — BotConfig::validate (L232-297)]
- [Source: src/config/mod.rs — BotConfig::_test_minimal (L331-371)]
- [Source: src/config/mod.rs — BotSecrets (L380-393)]
- [Source: src/config/mod.rs — ConfigError (L23-67)]
- [Source: src/config/discovery.rs — BmadDiscovery (L22-36)]
- [Source: src/config/discovery.rs — BmadDiscovery::discover (L47-88)]
- [Source: src/main.rs — current mod declarations (L4-13) — must be updated]
- [Source: _bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md — Task 0 lib.rs blocker]
- [Source: _bmad-output/implementation-artifacts/7-8-branch-management-git-tools-integration-tests.md — review patterns]

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List