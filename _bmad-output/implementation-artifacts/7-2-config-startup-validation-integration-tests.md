# Story 7.2: Config → Startup Validation Integration Tests

Status: ready-for-dev

## Story

As a developer,
I want integration tests that verify the full config loading and validation pipeline,
So that I'm confident the daemon rejects bad configs and accepts good ones end-to-end.

## Acceptance Criteria

1. **Given** a temp directory with a valid `bmad-bot.yaml` and `.env` file
   **When** the integration test loads config via `BotConfig::load()` then `BotConfig::validate()` then `BotSecrets::load()` then `BotSecrets::validate_for_config()`
   **Then** the full pipeline succeeds and returns a valid `BotConfig` and `BotSecrets`

2. **Given** a temp directory with a `bmad-bot.yaml` missing a required field (e.g., `polling_interval_secs: 0`)
   **When** the integration test runs the full load → validate pipeline
   **Then** a descriptive `ConfigError` is returned at the validation step
   **And** the error message identifies the exact field that failed

3. **Given** a temp directory with valid config but `.env` missing a required API key for the configured LLM provider
   **When** the integration test runs load → validate → secrets-validate pipeline
   **Then** a `ConfigError::MissingSecret` is returned
   **And** the error identifies which provider key is missing

4. **Given** a temp directory with a valid config
   **When** `BmadDiscovery::discover()` is called on a directory with a `_bmad/` structure
   **Then** the discovery detects BMAD, finds installed modules, and extracts the version
   **And** calling it on a directory without `_bmad/` returns `bmad_detected: false`

5. **Given** a valid config is loaded
   **When** `build_http_client()` is called
   **Then** a `ClientWithMiddleware` is returned with retry middleware configured (3 retries, exponential backoff)

## Tasks / Subtasks

- [ ] Task 1: Create integration test file `tests/integration/test_config.rs` (AC: #1–#5)
  - [ ] 1.1 Add `mod test_config;` declaration in `tests/integration.rs`
  - [ ] 1.2 Import required types from `bmad_bot::config` and `bmad_bot::config::discovery`

- [ ] Task 2: Write valid config round-trip test (AC: #1)
  - [ ] 2.1 Use `make_test_config()` from helpers to build a valid `BotConfig`
  - [ ] 2.2 Serialize to YAML via `serde_yml::to_string()` and write to `{tempdir}/bmad-bot.yaml`
  - [ ] 2.3 Call `BotConfig::load(path)` → `validate()` → assert `Ok(())`
  - [ ] 2.4 Construct `BotSecrets` directly with `make_test_secrets()` and call `validate_for_config(&config)` → assert `Ok(())`

- [ ] Task 3: Write invalid config rejection tests (AC: #2)
  - [ ] 3.1 Test `polling_interval_secs: 0` → `ConfigError::InvalidField`
  - [ ] 3.2 Test unknown git provider → `ConfigError::InvalidField`
  - [ ] 3.3 Test unknown LLM provider → `ConfigError::InvalidField`
  - [ ] 3.4 Test empty `bmad_paths.project_root` → `ConfigError::MissingField`
  - [ ] 3.5 Test invalid YAML syntax → `ConfigError::YamlParse`
  - [ ] 3.6 Test `BotConfig::load()` on nonexistent file → `ConfigError::FileRead`
  - [ ] 3.7 For each error, assert the error message contains the offending field name

- [ ] Task 4: Write secrets validation tests (AC: #3)
  - [ ] 4.1 Build valid config with `provider: "anthropic"`, construct `BotSecrets` with `anthropic_api_key: None` → assert `ConfigError::MissingSecret`
  - [ ] 4.2 Build valid config with `provider: "github"`, construct `BotSecrets` with `github_token: None` → assert `ConfigError::MissingSecret`
  - [ ] 4.3 Build valid config with Telegram enabled, construct `BotSecrets` with `telegram_bot_token: None` → assert `ConfigError::MissingSecret`
  - [ ] 4.4 Verify each error contains the expected env var name

- [ ] Task 5: Write BMAD discovery integration tests (AC: #4)
  - [ ] 5.1 Create temp dir with `_bmad/bmm/config.yaml` (version comment) and `_bmad/core/` → assert `bmad_detected: true`, modules found, version extracted
  - [ ] 5.2 Create temp dir without `_bmad/` → assert `bmad_detected: false`, empty modules
  - [ ] 5.3 Create temp dir with partial `_bmad/` (no config.yaml) → assert detected, no version

- [ ] Task 6: Write HTTP client builder test (AC: #5)
  - [ ] 6.1 Call `build_http_client()` → assert it returns without panicking
  - [ ] 6.2 Verify the returned value is a `ClientWithMiddleware` (type assertion via binding)

## Dev Notes

### Architecture Compliance

#### Integration Test Location
- All tests for this story go in `tests/integration/test_config.rs`
- This file is declared as `mod test_config;` in `tests/integration.rs` (created by Story 7.1)
- Run via `cargo test --test integration` — no special flags needed

#### No env var manipulation in tests
- `BotSecrets::load()` reads from real env vars via `dotenvy` — this is process-global and NOT safe for parallel test execution
- **Do NOT call `BotSecrets::load()` in integration tests.** Instead, construct `BotSecrets` directly using `make_test_secrets()` from Story 7.1 helpers, then selectively set fields to `None` for missing-secret tests
- This avoids env var pollution between tests and keeps tests deterministic
- The only aspect of `BotSecrets::load()` that matters (dotenvy integration) is implicitly tested by the daemon itself; integration tests focus on `validate_for_config()`

### Technical Requirements

#### 🚨 Prerequisite: `src/lib.rs` (from Story 7.1 Task 0)
This story requires the `lib.rs` created by Story 7.1 Task 0. Without it, `use bmad_bot::config::BotConfig;` will not compile because the project is currently a pure binary crate (`main.rs` only). Verify that `src/lib.rs` exists with `pub mod config;` before writing any integration tests. If Story 7.1 has not been implemented yet, Task 0 from that story MUST be completed first.

**Import paths after lib.rs exists:**
```rust
use bmad_bot::config::{BotConfig, BotSecrets, ConfigError, build_http_client};
use bmad_bot::config::discovery::BmadDiscovery;
```

#### Config YAML Writing for Tests
`BotConfig` derives `Serialize`, so valid configs can be written to temp files. Define a local helper in `test_config.rs` to avoid duplicating this across every test:
```rust
/// Write a valid BotConfig YAML to a temp directory and return the file path.
fn write_valid_config_yaml(dir: &Path) -> PathBuf {
    let config = make_test_config(dir);
    let yaml = serde_yml::to_string(&config).expect("serialize");
    let path = dir.join("bmad-bot.yaml");
    std::fs::write(&path, &yaml).expect("write");
    path
}
```

Usage:
```rust
let tmp = tempfile::tempdir().unwrap();
let path = write_valid_config_yaml(tmp.path());
let loaded = BotConfig::load(&path).expect("load");
loaded.validate().expect("validate");
```

For invalid configs, manually write YAML strings with specific defects rather than serializing a `BotConfig` (which would always produce structurally valid YAML):
```rust
let bad_yaml = r#"
polling_interval_secs: 0
git_provider:
  provider: github
  repo_owner: test
  repo_name: test
llm:
  dev: { provider: anthropic, model: test }
  review: { provider: anthropic, model: test }
  supervisor: { provider: anthropic, model: test }
notifications:
  telegram: { enabled: false }
bmad_paths:
  project_root: "."
  output_folder: "out"
  planning_artifacts: "out/planning"
  implementation_artifacts: "out/impl"
"#;
std::fs::write(tempdir.path().join("bmad-bot.yaml"), bad_yaml).expect("write");
let config = BotConfig::load(&tempdir.path().join("bmad-bot.yaml")).expect("load");
let err = config.validate().unwrap_err();
assert!(matches!(err, ConfigError::InvalidField { .. }));
```

#### Key API Signatures (exact from codebase)

**`BotConfig`** (`src/config/mod.rs`):
```rust
impl BotConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError>;
    pub fn validate(&self) -> Result<(), ConfigError>;
}
```

**`BotSecrets`** (`src/config/mod.rs`):
```rust
impl BotSecrets {
    pub fn load() -> Result<Self, ConfigError>;  // ⚠️ reads env vars — do NOT use in integration tests
    pub fn validate_for_config(&self, config: &BotConfig) -> Result<(), ConfigError>;
}
```
All fields on `BotSecrets` are `pub Option<String>` — construct directly for tests.

**`BmadDiscovery`** (`src/config/discovery.rs`):
```rust
impl BmadDiscovery {
    pub fn discover(project_root: &Path) -> Self;  // never fails
}
```
Returns struct with `pub` fields: `bmad_detected: bool`, `bmad_version: Option<String>`, `installed_modules: Vec<String>`, `config_path: Option<PathBuf>`, `project_root: PathBuf`.

**`build_http_client`** (`src/config/mod.rs`):
```rust
pub fn build_http_client() -> ClientWithMiddleware;
```

#### ConfigError Variants to Assert Against

```rust
pub enum ConfigError {
    FileRead { path: String, source: std::io::Error },
    YamlParse(serde_yml::Error),
    InvalidField { field: String, reason: String },
    MissingField { field: String },
    MissingSecret { env_var: String, purpose: String },
    DotenvError(dotenvy::Error),
}
```

Use `matches!()` for variant checking:
```rust
assert!(matches!(err, ConfigError::InvalidField { ref field, .. } if field == "polling_interval_secs"));
assert!(matches!(err, ConfigError::MissingSecret { ref env_var, .. } if env_var == "ANTHROPIC_API_KEY"));
```

#### Validation Rules Enforced by `BotConfig::validate()`
- `polling_interval_secs` must be > 0
- `log_format` must be `"json"` or `"pretty"`
- `log_level` must be one of `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`
- `git_provider.provider` must be `"github"` or `"gitlab"`
- Each LLM role provider must be `"anthropic"`, `"openai"`, or `"github-models"`
- `bmad_paths.project_root`, `output_folder`, `planning_artifacts`, `implementation_artifacts` must be non-empty
- `log_file` must be non-empty

#### BMAD Discovery Directory Structure for Tests
```
{tempdir}/
├── _bmad/
│   ├── bmm/
│   │   └── config.yaml    ← contains "# Version: 6.0.0-Beta.7"
│   ├── core/
│   ├── _config/
│   └── _memory/
```
Known modules scanned by discovery: `bmm`, `core`, `_config`, `_memory`.

### Previous Story Intelligence (Story 7.1)

- **Cargo test convention (UPDATED):** `tests/integration.rs` is the binary entry point, `tests/integration/` is the submodule directory. Due to **Rust 2024 edition**, module resolution from `tests/integration.rs` does NOT automatically look in `tests/integration/`. New test modules must be registered using `#[path]` attributes:
  ```rust
  #[path = "integration/test_config.rs"]
  mod test_config;
  ```
- **`lib.rs` BLOCKER is resolved:** `src/lib.rs` exists with `pub mod` for ALL non-CLI modules: `auth`, `config`, `git_provider`, `llm`, `mcp`, `notifier`, `pipeline`, `review`, `session`, `supervisor`, `tools`, `watcher`. `mod cli;` remains binary-only in `main.rs`.
- **`SessionState` and `ChatMessage` re-exported:** Accessible via `bmad_bot::session::{SessionState, ChatMessage}` (re-exported from private `state` submodule)
- **Fixture imports:** `use crate::helpers::fixtures::{make_test_config, make_test_secrets};`
- **Mock imports:** `use crate::helpers::mocks::{MockGitProvider, MockNotifier, MockSessionRunner, MockReviewRunner};`
- **MockSessionRunner/MockReviewRunner pattern:** Use closure-based outcome factory: `.with_outcome(|story| SessionOutcome::Completed { ... })`
- **Additional fixture:** `make_test_session_state(story_key)` builds a minimal `SessionState` for WAL tests
- **Temp dir pattern:** Always use `tempfile::tempdir()` — cleanup is automatic via `Drop`
- **Test naming:** `test_{module}_{behavior}_{scenario}` in snake_case
- **Structure:** Arrange → Act → Assert

### Dependencies Required

All already present — no new dependencies needed:
- `tempfile = "3"` (dev-dependency)
- `serde_yml = "0.0.12"` (main dependency, used for YAML serialization in tests)

**Prerequisite from Story 7.1:** `src/lib.rs` must exist with `pub mod config;` — see Story 7.1 Task 0.

### File Structure

```
tests/
├── integration.rs                    # Add: mod test_config;
└── integration/
    ├── helpers/
    │   ├── mod.rs
    │   ├── mocks.rs
    │   └── fixtures.rs
    ├── test_mocks.rs
    ├── test_fixtures.rs
    └── test_config.rs                ← NEW (this story)
```

### Testing Standards

- Use `#[test]` for sync tests (config load/validate are sync), `#[tokio::test]` only if needed
- Use `tempfile::tempdir()` for every test touching the filesystem
- Never leave artifacts on disk — tempdir handles cleanup via Drop
- Test names: `test_config_{behavior}_{scenario}` (e.g., `test_config_valid_roundtrip_succeeds`, `test_config_zero_polling_rejected`)
- Use `assert!(matches!(...))` for error variant matching with field guards

### Sprint-Status YAML Comments Are NOT Functional
The real `sprint-status.yaml` has comments like `# depends-on: 7-1`. These are **YAML comments stripped by the parser** — they have ZERO effect on dependency resolution. Dependencies are computed exclusively by `derive_dependencies()` from story numbering. This is irrelevant for config tests but noted for consistency across the epic.

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Story 7.2 (L897-933)]
- [Source: _bmad-output/planning-artifacts/epics.md — Integration Test Strategy (L822-856)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Config Pattern (L449-479)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Test Mock Pattern (L510-542)]
- [Source: _bmad-output/project-context.md — Testing Rules section]
- [Source: src/config/mod.rs — BotConfig::load (L219-226), BotConfig::validate (L232-297)]
- [Source: src/config/mod.rs — BotSecrets (L380-393), validate_for_config (L420-497)]
- [Source: src/config/mod.rs — ConfigError (L23-67)]
- [Source: src/config/mod.rs — build_http_client (L509-515)]
- [Source: src/config/mod.rs — BmadPathsConfig, GitProviderConfig, LlmConfig, etc. (L135-202)]
- [Source: src/config/discovery.rs — BmadDiscovery::discover, KNOWN_MODULES]
- [Source: _bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md — Cargo test convention, fixture patterns]

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List