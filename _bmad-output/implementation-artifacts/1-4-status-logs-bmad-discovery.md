# Story 1.4: Status, Logs & BMAD Discovery

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer operating BMAD Bot,
I want to check the daemon's state, review logs, and have BMAD auto-detected,
So that I can monitor operations and trust the daemon knows my project setup.

## Acceptance Criteria

1. **Given** the daemon is running or has run previously **When** I run `bmad-bot status` **Then** a summary is displayed showing: current state (running/stopped), stories processed count, stories in progress, stories blocked, and last activity timestamp

2. **Given** the daemon has been running with structured tracing **When** I run `bmad-bot logs` **Then** structured logs are displayed with story_id, timestamps, and action fields **And** logs can be filtered by level (info, warn, error)

3. **Given** the daemon starts in a project with BMAD installed **When** the config module initializes **Then** the daemon auto-discovers the BMAD version and installed modules by scanning the project repo (e.g., `_bmad/` directory structure) **And** the discovered information is logged at startup and available via `bmad-bot status`

## Tasks / Subtasks

- [x] Task 0: Backward-compatibility updates for new `log_file` field (AC: #1, #2)
  - [x] 0.1 Add `serde_json = "1"` to `[dependencies]` in Cargo.toml (already present from Story 1.1 — verify)
  - [x] 0.2 In `src/cli/mod.rs`: update `collect_config_interactively()` to set `log_file: default_log_file()` (or `"bmad-bot.log".to_string()`) when constructing `BotConfig` — no interactive prompt needed, use the default
  - [x] 0.3 In `src/cli/mod.rs`: update `make_test_config()` test helper to include `log_file: "bmad-bot.log".to_string()`
  - [x] 0.4 Verify `cargo check` and `cargo test` pass with existing Story 1.1/1.2/1.3 tests

- [x] Task 1: Implement BMAD Discovery module — `src/config/discovery.rs` (AC: #3)
  - [x] 1.1 Create `src/config/discovery.rs` with `pub struct BmadDiscovery` containing: `bmad_version: Option<String>`, `installed_modules: Vec<String>`, `config_path: Option<PathBuf>`, `project_root: PathBuf`
  - [x] 1.2 Implement `pub fn discover(project_root: &Path) -> BmadDiscovery` that scans `{project_root}/_bmad/` directory
  - [x] 1.3 Parse BMAD version from `_bmad/bmm/config.yaml` if present (read the YAML frontmatter or body for version info)
  - [x] 1.4 Detect installed modules by checking for known subdirectories under `_bmad/` (e.g., `bmm`, `core`, `_config`, `_memory`)
  - [x] 1.5 Check for `_bmad/bmm/config.yaml` existence → store path in `config_path`
  - [x] 1.6 Implement `Display` trait for `BmadDiscovery` for formatted output
  - [x] 1.7 Re-export from `src/config/mod.rs` with `pub mod discovery;`

- [x] Task 2: Implement daemon state file for status tracking (AC: #1)
  - [x] 2.1 Define `pub struct DaemonState` in a new file `src/cli/state.rs` with fields: `pid: u32`, `started_at: String`, `last_activity: String`, `status: String` (running/stopped), `log_file: PathBuf`, `stories_processed: usize`
  - [x] 2.2 Implement `DaemonState::write(path: &Path) -> Result<(), CliError>` — writes JSON state to `bmad-bot.state.json` in current directory
  - [x] 2.3 Implement `DaemonState::read(path: &Path) -> Result<Option<DaemonState>, CliError>` — reads state file, returns None if missing
  - [x] 2.4 Implement `DaemonState::is_process_alive(pid: u32) -> bool` — checks if PID is still running (via `kill(pid, 0)` on Unix)
  - [x] 2.5 Implement `DaemonState::cleanup(path: &Path) -> Result<(), CliError>` — removes stale state file
  - [x] 2.6 Re-export from `src/cli/mod.rs` with `pub mod state;`

- [x] Task 3: Implement log file writer in tracing setup (AC: #2)
  - [x] 3.1 Add `log_file` field to `BotConfig` with `#[serde(default = "default_log_file")]` → default `"bmad-bot.log"`
  - [x] 3.2 Update `bmad-bot.yaml.example` with `log_file` field and comment
  - [x] 3.3 Extend `init_tracing()` in `cli/mod.rs` to add a file appender layer alongside the stdout layer using `tracing_subscriber::Layer` composition
  - [x] 3.4 File layer always writes JSON format (machine-parseable) regardless of stdout format setting
  - [x] 3.5 Validate `log_file` path in `BotConfig::validate()` (non-empty string)

- [x] Task 4: Integrate state tracking into `run_start()` (AC: #1, #3)
  - [x] 4.1 At daemon startup in `run_start()`: run BMAD discovery, log results at `info` level
  - [x] 4.2 Write `DaemonState` file with current PID, start timestamp, status "running"
  - [x] 4.3 Update `last_activity` timestamp in state file at each polling cycle
  - [x] 4.4 On graceful shutdown: update state file with status "stopped" and final timestamp, then remove state file
  - [x] 4.5 Store `BmadDiscovery` results in state file so `status` command can display them

- [x] Task 5: Implement `run_status()` command (AC: #1, #3)
  - [x] 5.1 Create `pub async fn run_status(config_path: &Path) -> Result<(), CliError>` in `cli/mod.rs`
  - [x] 5.2 Read `DaemonState` from `bmad-bot.state.json` — if missing, report "stopped (no state file)"
  - [x] 5.3 If state file exists, check if PID is alive → "running" or "stopped (stale state)"
  - [x] 5.4 Load and parse `sprint-status.yaml` from configured path → count stories by status (backlog, ready-for-dev, in-progress, review, done, blocked)
  - [x] 5.5 Display BMAD discovery info (version, installed modules) — either from state file or by running discovery fresh
  - [x] 5.6 Format output as a clean summary table to stdout

- [x] Task 6: Implement `run_logs()` command (AC: #2)
  - [x] 6.1 Create `pub async fn run_logs(config_path: &Path, level: Option<String>, tail: Option<usize>) -> Result<(), CliError>` in `cli/mod.rs`
  - [x] 6.2 Read the log file path from config (default: `bmad-bot.log`)
  - [x] 6.3 Parse JSON log lines and pretty-print with colored output (timestamp, level, message, fields)
  - [x] 6.4 Implement `--level` flag filtering: only show logs at specified level and above
  - [x] 6.5 Implement `--tail N` flag: show last N log entries (default: 50, plain `usize` not `Option` — clap `default_value_t`)
  - [x] 6.6 Validate `--level` flag: if provided but not one of trace/debug/info/warn/error, print a warning and list valid levels
  - [x] 6.7 If log file doesn't exist, report "No log file found — has the daemon been started?"

- [x] Task 7: Extend CLI with `status` and `logs` subcommand arguments (AC: #1, #2)
  - [x] 7.1 Add `--level` optional argument to `Logs` variant in `Commands` enum
  - [x] 7.2 Add `--tail` argument to `Logs` variant as plain `usize` with `default_value_t = 50` (NOT `Option<usize>` — avoids redundant unwrap)
  - [x] 7.3 Update main.rs dispatch for `Commands::Status` → `cli::run_status(&cli.config).await?`
  - [x] 7.4 Update main.rs dispatch for `Commands::Logs` → `cli::run_logs(&cli.config, level, tail).await?`

- [x] Task 8: Write unit tests (AC: #1, #2, #3)
  - [x] 8.1 Test `BmadDiscovery::discover()` with a mocked _bmad directory (using tempdir)
  - [x] 8.2 Test `BmadDiscovery::discover()` returns empty modules when no _bmad dir exists
  - [x] 8.3 Test `DaemonState::write()` and `DaemonState::read()` roundtrip
  - [x] 8.4 Test `DaemonState::read()` returns None for missing file
  - [x] 8.5 Test `DaemonState::is_process_alive()` with current PID (should be alive)
  - [x] 8.6 Test `DaemonState::is_process_alive()` with PID 0 or max u32 (should be dead)
  - [x] 8.7 Test sprint-status.yaml parsing and story count aggregation
  - [x] 8.8 Test sprint-status.yaml parsing counts blocked stories correctly
  - [x] 8.9 Test log line parsing and level filtering logic
  - [x] 8.10 Test `parse_level_priority` returns 0 for unknown levels
  - [x] 8.11 Test `BotConfig::validate()` accepts new `log_file` field
  - [x] 8.12 Test `BotConfig::validate()` rejects empty `log_file`
  - [x] 8.13 Test `DaemonState::record_story_processed()` increments counter
  - [x] 8.14 Test `stories_processed` survives write/read roundtrip

- [x] Task 9: Final quality checks
  - [x] 9.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [x] 9.2 Run `cargo clippy` and fix any warnings
  - [x] 9.3 Run `cargo test` and verify all tests pass (including Story 1.1, 1.2, 1.3 tests)
  - [x] 9.4 Verify all public items have `///` doc comments
  - [ ] 9.5 Manual integration test: start daemon with `cargo run -- start`, run `cargo run -- status` in another terminal, verify output shows running state and BMAD discovery
  - [ ] 9.6 Manual integration test: run `cargo run -- logs` and verify log output with filtering
  - [ ] 9.7 Manual integration test: run `cargo run -- logs --level warn --tail 10` and verify filtered output

## Dev Notes

### Previous Story Intelligence

**Story 1.1** established:
- `BotConfig` struct with all fields (`polling_interval_secs`, `git_provider`, `llm`, `notifications`, `bmad_paths`, `log_format`, `log_level` added in Story 1.2)
- All nested config structs: `LlmConfig`, `LlmRoleConfig`, `GitProviderConfig`, `NotificationConfig`, `TelegramConfig`, `BmadPathsConfig`
- `BotSecrets` struct, `ConfigError` thiserror enum
- `BotConfig::validate()` method, `BotConfig::load()` method
- `bmad-bot.yaml.example` and `.env.example` as reference templates
- serde defaults: `polling_interval_secs` → 300, `target_branch` → "main", `log_format` → "pretty", `log_level` → "info"
- All module stubs created: `cli/`, `config/`, `watcher/`, `session/`, `supervisor/`, `review/`, `tools/`, `git_provider/`, `notifier/`
- `build_http_client()` with reqwest-middleware + reqwest-retry for retry resilience

**Story 1.2** established:
- `Cli` struct with `--config` flag (default: `bmad-bot.yaml`), `Commands` enum with Init/Start/Status/Logs
- `CliError` thiserror enum with `Config`, `TracingInit`, `Signal` variants
- `run_start()` handler with full config loading, secret validation, Arc sharing, structured logging
- `init_tracing()` with config-driven format (JSON or pretty) and env-filter
- `run_polling_loop()` with tokio::select! for graceful shutdown (SIGINT/SIGTERM)
- `main.rs` full CLI dispatch — Status and Logs arms have `tracing::warn!("not yet implemented")` placeholders
- `BotSecrets::validate_for_config(&config)` method for cross-validation
- Anti-pattern: NO println in daemon runtime (only tracing)

**Story 1.3** established:
- `run_init()` function in `cli/mod.rs` — interactive config generation with dialoguer
- `collect_config_interactively()`, `generate_config_yaml()`, `generate_env_file()`
- `Serialize` derive added to ALL config structs (BotConfig, LlmConfig, etc.)
- `CliError` extended with `Init { reason }`, `Io(#[from] std::io::Error)`, `UserCancelled` variants
- `dialoguer = "0.11"` and `chrono = "0.4"` added to Cargo.toml
- `main.rs` Init arm updated: `cli::run_init(&cli.config).await?`

**Key pattern from Story 1.2 to follow for Status/Logs:** The `status` and `logs` commands in main.rs currently do:
```rust
cli::Commands::Status => {
    let _ = tracing_subscriber::fmt::try_init();
    tracing::warn!("'status' command not yet implemented — see Story 1.4");
}
cli::Commands::Logs => {
    let _ = tracing_subscriber::fmt::try_init();
    tracing::warn!("'logs' command not yet implemented — see Story 1.4");
}
```
This story replaces those with real implementations.

**Git intelligence (last 5 commits):**
- `53d019c` — docs(sm): create story 1.3 - interactive init command
- `2d1d10c` — docs(sm): create story 1.2 - CLI framework & daemon lifecycle
- `7bffc7b` — docs(sm): create story 1.1 - project scaffolding, configuration & validation
- `3eff108` — chore: generate initial sprint-status.yaml
- `d94ffd9` — docs(planning): add epics breakdown

Pattern: Conventional Commits enforced. `docs(sm):` for story documents. No code implementation yet — all stories are still `ready-for-dev`.

### Backward-Compatibility: Story 1.3 Updates Required

Adding `log_file` to `BotConfig` breaks two locations in Story 1.3's code that construct `BotConfig` with all fields explicitly:

**1. `collect_config_interactively()` in `src/cli/mod.rs`** — This function builds a `BotConfig` struct at the end. Add the new field with the default value (no interactive prompt needed for MVP):
```rust
    Ok(BotConfig {
        // ... existing fields ...
        log_file: "bmad-bot.log".to_string(),  // ADD THIS LINE
    })
```

**2. `make_test_config()` test helper in `src/cli/mod.rs`** — Add the field to the test config builder:
```rust
    fn make_test_config() -> BotConfig {
        BotConfig {
            // ... existing fields ...
            log_file: "bmad-bot.log".to_string(),  // ADD THIS LINE
        }
    }
```

> **⚠️ If you skip this, `cargo check` will fail immediately** with "missing field `log_file` in initializer of `BotConfig`". Fix these FIRST (Task 0) before proceeding to new code.

### New Files

This story introduces two new source files:

| File | Purpose |
|------|---------|
| `src/config/discovery.rs` | BMAD auto-discovery logic |
| `src/cli/state.rs` | Daemon state file tracking |

### BmadDiscovery Implementation — `src/config/discovery.rs`

```rust
//! BMAD auto-discovery — detects BMAD installation, version, and modules.
//!
//! Scans the project's `_bmad/` directory structure to determine what
//! BMAD components are available. Used at daemon startup and by `status` command.

use std::fmt;
use std::path::{Path, PathBuf};

/// Known BMAD module directories to look for under `_bmad/`.
const KNOWN_MODULES: &[(&str, &str)] = &[
    ("bmm", "BMAD Method Module"),
    ("core", "Core Engine"),
    ("_config", "Configuration"),
    ("_memory", "Agent Memory"),
];

/// Result of scanning a project for BMAD installation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BmadDiscovery {
    /// BMAD version string if detected (from config.yaml or package metadata).
    pub bmad_version: Option<String>,
    /// List of installed module names (e.g., ["bmm", "core", "_config"]).
    pub installed_modules: Vec<String>,
    /// Path to the BMAD config file if found.
    pub config_path: Option<PathBuf>,
    /// The project root that was scanned.
    pub project_root: PathBuf,
    /// Whether a valid _bmad directory was found at all.
    pub bmad_detected: bool,
}

impl BmadDiscovery {
    /// Scan the project root for BMAD installation details.
    ///
    /// This function never fails — missing directories or unreadable files
    /// result in `None`/empty values, not errors. The daemon should always
    /// start even if BMAD discovery finds nothing.
    pub fn discover(project_root: &Path) -> Self {
        let bmad_dir = project_root.join("_bmad");
        let bmad_detected = bmad_dir.is_dir();

        if !bmad_detected {
            return Self {
                bmad_version: None,
                installed_modules: Vec::new(),
                config_path: None,
                project_root: project_root.to_path_buf(),
                bmad_detected: false,
            };
        }

        // Detect installed modules
        let installed_modules: Vec<String> = KNOWN_MODULES
            .iter()
            .filter(|(dir, _)| bmad_dir.join(dir).is_dir())
            .map(|(dir, _)| (*dir).to_string())
            .collect();

        // Try to find and parse BMAD config for version info
        let config_path = bmad_dir.join("bmm/config.yaml");
        let (bmad_version, config_path) = if config_path.is_file() {
            let version = Self::extract_version(&config_path);
            (version, Some(config_path))
        } else {
            (None, None)
        };

        Self {
            bmad_version,
            installed_modules,
            config_path,
            project_root: project_root.to_path_buf(),
            bmad_detected: true,
        }
    }

    /// Extract version from BMAD config.yaml.
    /// Looks for a line like "# Version: X.Y.Z" in comments or a `version:` field.
    fn extract_version(config_path: &Path) -> Option<String> {
        let content = std::fs::read_to_string(config_path).ok()?;

        // Try comment-style version first: "# Version: X.Y.Z"
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("# Version:") {
                let version = rest.trim();
                if !version.is_empty() {
                    return Some(version.to_string());
                }
            }
        }

        // Fallback: try YAML field `bmad_version:` or `version:`
        // Use simple string parsing to avoid full YAML parse dependency
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("bmad_version:") {
                let version = rest.trim().trim_matches('"').trim_matches('\'');
                if !version.is_empty() {
                    return Some(version.to_string());
                }
            }
        }

        None
    }

    /// Returns a human-readable description of each installed module.
    pub fn module_descriptions(&self) -> Vec<(&str, &str)> {
        self.installed_modules
            .iter()
            .filter_map(|m| {
                KNOWN_MODULES
                    .iter()
                    .find(|(name, _)| *name == m.as_str())
                    .copied()
            })
            .collect()
    }
}

impl fmt::Display for BmadDiscovery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.bmad_detected {
            return write!(f, "BMAD: Not detected (no _bmad/ directory found)");
        }

        writeln!(f, "BMAD: Detected")?;
        if let Some(ref version) = self.bmad_version {
            writeln!(f, "  Version: {version}")?;
        } else {
            writeln!(f, "  Version: unknown")?;
        }
        writeln!(f, "  Modules: {}", if self.installed_modules.is_empty() {
            "none".to_string()
        } else {
            self.installed_modules.join(", ")
        })?;
        if let Some(ref path) = self.config_path {
            writeln!(f, "  Config: {}", path.display())?;
        }
        Ok(())
    }
}
```

> **IMPORTANT:** `discover()` never returns an error. A missing `_bmad/` directory is a valid state — the daemon should still start. All discovery failures are silent (return `None`/empty). Errors are only logged at `warn` level by the caller.

### DaemonState Implementation — `src/cli/state.rs`

```rust
//! Daemon state file tracking for the `status` command.
//!
//! The daemon writes a `bmad-bot.state.json` file while running so that
//! `bmad-bot status` can report the daemon's state from a separate process.

use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::config::discovery::BmadDiscovery;

/// Default state file name, written in the current working directory.
pub const STATE_FILE_NAME: &str = "bmad-bot.state.json";

/// Persistent daemon state written to disk for cross-process communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonState {
    /// Process ID of the running daemon.
    pub pid: u32,
    /// ISO 8601 timestamp when daemon started.
    pub started_at: String,
    /// ISO 8601 timestamp of last activity (poll cycle, story processing).
    pub last_activity: String,
    /// Current status: "running" or "stopped".
    pub status: String,
    /// Path to the log file.
    pub log_file: PathBuf,
    /// BMAD discovery results from startup.
    pub bmad_discovery: Option<BmadDiscovery>,
    /// Number of stories processed during this daemon session (AC #1: "stories processed count").
    pub stories_processed: usize,
}

impl DaemonState {
    /// Create a new state for a freshly started daemon.
    pub fn new_running(log_file: PathBuf, bmad_discovery: BmadDiscovery) -> Self {
        let now = chrono::Local::now().to_rfc3339();
        Self {
            pid: std::process::id(),
            started_at: now.clone(),
            last_activity: now,
            status: "running".to_string(),
            log_file,
            bmad_discovery: Some(bmad_discovery),
            stories_processed: 0,
        }
    }

    /// Increment the stories_processed counter by one.
    pub fn record_story_processed(&mut self) {
        self.stories_processed += 1;
    }

    /// Update the last_activity timestamp to now.
    pub fn touch(&mut self) {
        self.last_activity = chrono::Local::now().to_rfc3339();
    }

    /// Mark state as stopped.
    pub fn mark_stopped(&mut self) {
        self.status = "stopped".to_string();
        self.last_activity = chrono::Local::now().to_rfc3339();
    }

    /// Write state to the state file (atomic: write to tmp then rename).
    pub fn write(&self, path: &Path) -> Result<(), super::CliError> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| super::CliError::State {
                reason: format!("Failed to serialize daemon state: {e}"),
            })?;
        // Write to a temporary file then rename for atomicity
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Read state from the state file. Returns None if file doesn't exist.
    pub fn read(path: &Path) -> Result<Option<Self>, super::CliError> {
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)?;
        let state: Self = serde_json::from_str(&content)
            .map_err(|e| super::CliError::State {
                reason: format!("Failed to parse daemon state: {e}"),
            })?;
        Ok(Some(state))
    }

    /// Check if a given PID is still alive (Unix: macOS + Linux).
    /// Uses POSIX `kill -0` via std::process::Command — no `libc` or `unsafe` needed.
    /// Returns false for PID 0 or if the process doesn't exist.
    pub fn is_process_alive(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Remove the state file.
    pub fn cleanup(path: &Path) -> Result<(), super::CliError> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}
```

> **NOTE:** `is_process_alive` uses `std::process::Command::new("kill")` with signal 0 — this is POSIX-standard (`kill -0`) and works on macOS and Linux without any external crate (`libc`, `nix`). No `unsafe` code needed. The command only checks process existence — it does NOT send any signal.

### CliError Extension

Add new variants to the existing `CliError` in `cli/mod.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    // ... existing variants from Story 1.2 + 1.3 ...

    #[error("State file error: {reason}")]
    State { reason: String },

    #[error("Log file error: {reason}")]
    LogFile { reason: String },
}
```

> **NOTE:** The `Io(#[from] std::io::Error)` variant already exists from Story 1.3. The `State` and `LogFile` variants provide specific context for this story's error paths. Do NOT add duplicate `#[from]` conversions.

### BotConfig Extension — Log File Path

Add to `BotConfig` in `src/config/mod.rs`:

```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct BotConfig {
    // ... existing fields ...

    /// Path to the log file for persistent structured logging.
    /// The `logs` command reads from this file.
    /// Default: "bmad-bot.log"
    #[serde(default = "default_log_file")]
    pub log_file: String,
}

fn default_log_file() -> String { "bmad-bot.log".to_string() }
```

Add validation to `BotConfig::validate()`:

```rust
// Inside validate():
if self.log_file.trim().is_empty() {
    return Err(ConfigError::InvalidField {
        field: "log_file".to_string(),
        reason: "must be a non-empty file path".to_string(),
    });
}
```

### Tracing Extension — Dual Output (stdout + file)

Extend `init_tracing()` in `cli/mod.rs` to write to both stdout and a log file. The file always uses JSON format for machine-parseability by the `logs` command.

```rust
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};
use std::fs::OpenOptions;
use std::sync::Mutex;

/// Initialize structured tracing with dual output:
/// - stdout: config-driven format (pretty or JSON)
/// - file: always JSON (machine-parseable for `bmad-bot logs`)
pub fn init_tracing(config: &BotConfig) -> Result<(), CliError> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    // File appender — always JSON
    // CRITICAL: Raw File does NOT implement MakeWriter. Wrap in Mutex<File>
    // which DOES implement MakeWriter (serializes writes via lock).
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config.log_file)
        .map_err(|e| CliError::LogFile {
            reason: format!("Cannot open log file '{}': {e}", config.log_file),
        })?;
    let file_writer = Mutex::new(log_file);

    let file_layer = fmt::layer()
        .json()
        .with_target(true)
        .with_thread_ids(false)
        .with_writer(file_writer)
        .with_ansi(false); // No ANSI colors in file output

    // Stdout layer — config-driven format
    let stdout_layer = match config.log_format.as_str() {
        "json" => fmt::layer()
            .json()
            .with_target(true)
            .with_thread_ids(false)
            .boxed(),
        _ => fmt::layer()
            .with_target(true)
            .with_thread_ids(false)
            .boxed(),
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stdout_layer)
        .try_init()
        .map_err(|e| CliError::TracingInit {
            reason: e.to_string(),
        })?;

    Ok(())
}
```

> **CRITICAL:** This replaces the existing `init_tracing()` from Story 1.2. The function signature remains the same (`pub fn init_tracing(config: &BotConfig) -> Result<(), CliError>`) — only the internal implementation changes to add the file layer.

> **⚠️ WHY `Mutex<File>`:** A raw `std::fs::File` does NOT implement `tracing_subscriber::fmt::MakeWriter`. Wrapping in `std::sync::Mutex<File>` provides `MakeWriter` automatically (tracing-subscriber has a blanket impl for `Mutex<W: Write>`). This serializes writes through a lock — acceptable for a single-threaded daemon with low log volume. Do NOT pass a bare `File` — it will not compile.

> **NOTE on `.boxed()`:** The two stdout layer variants (JSON vs pretty) have different types. Using `.boxed()` from `tracing_subscriber::Layer` trait erases the type so both branches return the same type. This requires the `tracing-subscriber` crate's `registry` feature (already included via `"json"` feature).

### Sprint Status Parsing for `status` Command

The `status` command needs to parse `sprint-status.yaml` and count stories by status. Create a helper:

```rust
/// Story status counts parsed from sprint-status.yaml.
#[derive(Debug, Default)]
pub struct SprintSummary {
    pub total_stories: usize,
    pub backlog: usize,
    pub ready_for_dev: usize,
    pub in_progress: usize,
    pub review: usize,
    pub done: usize,
    pub blocked: usize,
    pub other: usize,
    pub total_epics: usize,
    pub epics_in_progress: usize,
    pub epics_done: usize,
}

impl SprintSummary {
    /// Parse sprint-status.yaml and compute summary statistics.
    /// Returns a default (all zeros) summary if the file doesn't exist or can't be parsed.
    pub fn from_file(path: &Path) -> Self {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };

        let yaml: serde_yaml::Value = match serde_yaml::from_str(&content) {
            Ok(v) => v,
            Err(_) => return Self::default(),
        };

        let mut summary = Self::default();

        if let Some(dev_status) = yaml.get("development_status").and_then(|v| v.as_mapping()) {
            for (key, value) in dev_status {
                let key_str = key.as_str().unwrap_or("");
                let status_str = value.as_str().unwrap_or("");

                // Identify epic entries (format: "epic-N")
                if key_str.starts_with("epic-") && !key_str.contains("retrospective") {
                    summary.total_epics += 1;
                    match status_str {
                        "in-progress" => summary.epics_in_progress += 1,
                        "done" => summary.epics_done += 1,
                        _ => {}
                    }
                    continue;
                }

                // Skip retrospective entries
                if key_str.contains("retrospective") {
                    continue;
                }

                // Story entries (format: "N-N-slug")
                // Check if it matches story pattern: starts with a digit
                if key_str.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                    summary.total_stories += 1;
                    match status_str {
                        "backlog" => summary.backlog += 1,
                        "ready-for-dev" => summary.ready_for_dev += 1,
                        "in-progress" => summary.in_progress += 1,
                        "review" => summary.review += 1,
                        "done" => summary.done += 1,
                        "blocked" => summary.blocked += 1,
                        _ => summary.other += 1,
                    }
                }
            }
        }

        summary
    }
}

impl fmt::Display for SprintSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Sprint Status:")?;
        writeln!(f, "  Epics: {} total ({} in-progress, {} done)",
            self.total_epics, self.epics_in_progress, self.epics_done)?;
        writeln!(f, "  Stories: {} total", self.total_stories)?;
        writeln!(f, "    ✅ Done:          {}", self.done)?;
        writeln!(f, "    🔍 Review:        {}", self.review)?;
        writeln!(f, "    🔧 In Progress:   {}", self.in_progress)?;
        writeln!(f, "    📋 Ready for Dev: {}", self.ready_for_dev)?;
        writeln!(f, "    📦 Backlog:       {}", self.backlog)?;
        if self.blocked > 0 {
            writeln!(f, "    🚫 Blocked:       {}", self.blocked)?;
        }
        Ok(())
    }
}
```

### run_status() Implementation

```rust
/// Runs the `status` command: displays daemon state, sprint summary, and BMAD info.
pub async fn run_status(config_path: &Path) -> Result<(), CliError> {
    // Try to load config for paths — if config doesn't exist, use defaults
    let config = crate::config::BotConfig::load(config_path).ok();

    let state_path = Path::new(state::STATE_FILE_NAME);

    println!("╔══════════════════════════════════════╗");
    println!("║        BMAD Bot Status               ║");
    println!("╚══════════════════════════════════════╝");
    println!();

    // --- Daemon State ---
    match state::DaemonState::read(state_path)? {
        Some(state) => {
            let alive = state::DaemonState::is_process_alive(state.pid);
            let effective_status = if alive { "🟢 Running" } else { "🔴 Stopped (stale state)" };

            println!("Daemon: {effective_status}");
            println!("  PID:              {}", state.pid);
            println!("  Started:          {}", state.started_at);
            println!("  Last Activity:    {}", state.last_activity);
            println!("  Stories Processed:{}", state.stories_processed);
            println!("  Log File:         {}", state.log_file.display());
            println!();

            // BMAD discovery from state
            if let Some(ref discovery) = state.bmad_discovery {
                println!("{discovery}");
            }

            // Clean up stale state if process is dead
            if !alive {
                state::DaemonState::cleanup(state_path)?;
                println!("  (Stale state file cleaned up)");
                println!();
            }
        }
        None => {
            println!("Daemon: 🔴 Stopped (no state file)");
            println!();

            // Run fresh BMAD discovery
            if let Some(ref cfg) = config {
                let discovery = crate::config::discovery::BmadDiscovery::discover(
                    Path::new(&cfg.bmad_paths.project_root),
                );
                println!("{discovery}");
            }
        }
    }

    // --- Sprint Summary ---
    if let Some(ref cfg) = config {
        let sprint_path = Path::new(&cfg.bmad_paths.implementation_artifacts)
            .join("sprint-status.yaml");
        let summary = SprintSummary::from_file(&sprint_path);
        if summary.total_stories > 0 {
            println!("{summary}");
        } else {
            println!("Sprint Status: No sprint data found");
            println!("  Run sprint-planning to initialize story tracking");
        }
    } else {
        println!("Sprint Status: Cannot determine (no config file at {})",
            config_path.display());
    }

    Ok(())
}
```

> **NOTE on `println!` usage:** Like `init`, the `status` and `logs` commands are one-shot CLI commands, not the daemon runtime. Using `println!` for user-facing output is acceptable here because these are interactive commands displaying formatted data, not operational log messages. The anti-pattern "no println" applies to the long-running daemon loop only.

### run_logs() Implementation

```rust
/// Runs the `logs` command: reads and displays structured log entries from the log file.
/// Valid log level names for --level flag validation.
const VALID_LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

pub async fn run_logs(
    config_path: &Path,
    level: Option<String>,
    tail: Option<usize>,
) -> Result<(), CliError> {
    // Validate --level flag if provided
    if let Some(ref lvl) = level {
        if !VALID_LOG_LEVELS.contains(&lvl.to_lowercase().as_str()) {
            println!("⚠️  Unknown log level '{}'. Valid levels: {}", lvl, VALID_LOG_LEVELS.join(", "));
            println!("   Showing all log entries (no filter applied).\n");
        }
    }

    let config = crate::config::BotConfig::load(config_path)
        .map_err(|e| CliError::LogFile {
            reason: format!("Cannot load config to find log file path: {e}"),
        })?;

    let log_path = Path::new(&config.log_file);

    if !log_path.exists() {
        println!("No log file found at '{}'", config.log_file);
        println!("Has the daemon been started? Run `bmad-bot start` first.");
        return Ok(());
    }

    let content = std::fs::read_to_string(log_path)?;
    let lines: Vec<&str> = content.lines().collect();

    if lines.is_empty() {
        println!("Log file is empty.");
        return Ok(());
    }

    // Apply tail (default: 50 most recent entries)
    let tail_count = tail.unwrap_or(50);
    let start_idx = if lines.len() > tail_count {
        lines.len() - tail_count
    } else {
        0
    };
    let visible_lines = &lines[start_idx..];

    // Parse minimum log level for filtering
    let min_level = level.as_deref().map(parse_level_priority).unwrap_or(0);

    let mut displayed = 0;
    for line in visible_lines {
        // Each line should be a JSON object from tracing's JSON layer
        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) {
            let entry_level = entry
                .get("level")
                .and_then(|v| v.as_str())
                .unwrap_or("INFO");

            // Filter by level
            if parse_level_priority(entry_level) < min_level {
                continue;
            }

            let timestamp = entry
                .get("timestamp")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let message = entry
                .get("fields")
                .and_then(|f| f.get("message"))
                .and_then(|v| v.as_str())
                .or_else(|| entry.get("message").and_then(|v| v.as_str()))
                .unwrap_or("");
            let target = entry
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let story_id = entry
                .get("fields")
                .and_then(|f| f.get("story_id"))
                .and_then(|v| v.as_str())
                .map(|s| format!(" [story:{s}]"))
                .unwrap_or_default();

            let level_icon = match entry_level.to_uppercase().as_str() {
                "ERROR" => "❌",
                "WARN" => "⚠️ ",
                "INFO" => "ℹ️ ",
                "DEBUG" => "🐛",
                "TRACE" => "🔬",
                _ => "  ",
            };

            println!("{level_icon} {timestamp} [{entry_level:>5}] {target}{story_id}: {message}");
            displayed += 1;
        } else {
            // Non-JSON line — print as-is (shouldn't happen with JSON file layer, but be resilient)
            if min_level == 0 {
                println!("  {line}");
                displayed += 1;
            }
        }
    }

    if displayed == 0 {
        println!("No log entries match the filter criteria.");
    } else {
        println!("\n--- Showing {displayed} of {} total entries ---", lines.len());
    }

    Ok(())
}

/// Map log level string to a numeric priority for filtering.
/// Higher number = more severe. Filter shows entries at min_level and above.
fn parse_level_priority(level: &str) -> u8 {
    match level.to_uppercase().as_str() {
        "TRACE" => 1,
        "DEBUG" => 2,
        "INFO" => 3,
        "WARN" | "WARNING" => 4,
        "ERROR" => 5,
        _ => 0,
    }
}
```

### Extended CLI Commands with Arguments

Update the `Commands` enum in `cli/mod.rs`:

```rust
/// Available subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Interactive setup: generates bmad-bot.yaml and .env files.
    Init,
    /// Start the daemon. Polls sprint-status.yaml and processes stories.
    Start,
    /// Show current daemon state: stories processed, in progress, blocked.
    Status,
    /// Display structured daemon logs with filtering.
    Logs {
        /// Minimum log level to display (trace, debug, info, warn, error).
        #[arg(long, short)]
        level: Option<String>,
        /// Number of most recent log entries to show (default: 50).
        #[arg(long, short, default_value_t = 50)]
        tail: usize,
    },
}
```

### Updated main.rs Dispatch

Replace the Status and Logs arms:

```rust
cli::Commands::Status => {
    let _ = tracing_subscriber::fmt::try_init();
    cli::run_status(&cli.config).await?;
}
cli::Commands::Logs { level, tail } => {
    let _ = tracing_subscriber::fmt::try_init();
    cli::run_logs(&cli.config, level, Some(tail)).await?;
}
```

### Integration into run_start() — State File & BMAD Discovery

Modify the existing `run_start()` in `cli/mod.rs` to incorporate state tracking and BMAD discovery:

```rust
pub async fn run_start(config_path: &std::path::Path) -> Result<(), CliError> {
    let config = crate::config::BotConfig::load(config_path)?;
    config.validate()?;

    init_tracing(&config)?;

    let secrets = crate::config::BotSecrets::load()?;
    secrets.validate_for_config(&config)?;

    // BMAD auto-discovery
    let discovery = crate::config::discovery::BmadDiscovery::discover(
        std::path::Path::new(&config.bmad_paths.project_root),
    );
    if discovery.bmad_detected {
        tracing::info!(
            bmad_version = ?discovery.bmad_version,
            modules = ?discovery.installed_modules,
            "BMAD installation detected"
        );
    } else {
        tracing::warn!(
            project_root = %config.bmad_paths.project_root,
            "No BMAD installation detected — _bmad/ directory not found"
        );
    }

    // Write daemon state file
    let state_path = std::path::Path::new(state::STATE_FILE_NAME);
    let mut daemon_state = state::DaemonState::new_running(
        std::path::PathBuf::from(&config.log_file),
        discovery,
    );
    daemon_state.write(state_path)?;

    let config = std::sync::Arc::new(config);

    tracing::info!(
        config_path = %config_path.display(),
        polling_interval_secs = config.polling_interval_secs,
        git_provider = %config.git_provider.provider,
        log_format = %config.log_format,
        log_file = %config.log_file,
        "bmad-bot daemon started"
    );

    // Polling loop with graceful shutdown — pass state for touch updates
    run_polling_loop(&config, &mut daemon_state, state_path).await?;

    // Clean shutdown — update state and remove file
    daemon_state.mark_stopped();
    daemon_state.write(state_path)?;
    state::DaemonState::cleanup(state_path)?;

    tracing::info!("bmad-bot daemon stopped cleanly");
    Ok(())
}
```

### Updated run_polling_loop() — Touch State

```rust
async fn run_polling_loop(
    config: &std::sync::Arc<BotConfig>,
    daemon_state: &mut state::DaemonState,
    state_path: &std::path::Path,
) -> Result<(), CliError> {
    let interval = tokio::time::Duration::from_secs(config.polling_interval_secs);

    let mut sigterm = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::terminate()
    )?;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {
                // Update last activity timestamp
                daemon_state.touch();
                if let Err(e) = daemon_state.write(state_path) {
                    tracing::warn!(error = %e, "Failed to update daemon state file");
                }

                tracing::debug!(
                    interval_secs = config.polling_interval_secs,
                    "Polling cycle — no watcher implemented yet (placeholder)"
                );
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received SIGINT (Ctrl-C), initiating graceful shutdown");
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, initiating graceful shutdown");
                break;
            }
        }
    }

    Ok(())
}
```

### Updated bmad-bot.yaml.example

Add the `log_file` field to the existing example:

```yaml
# Logging configuration
log_format: pretty            # "pretty" or "json" (default: pretty)
log_level: info               # "trace", "debug", "info", "warn", "error" (default: info)
log_file: bmad-bot.log        # Path to log file for `bmad-bot logs` command (default: bmad-bot.log)
```

### .gitignore Addition

Add to `.gitignore`:

```
bmad-bot.state.json
bmad-bot.log
```

### Files Modified/Created in This Story

| File | Change |
|------|--------|
| `src/config/discovery.rs` | **NEW** — BMAD auto-discovery module |
| `src/config/mod.rs` | Add `pub mod discovery;`, add `log_file` field to `BotConfig`, add `default_log_file()`, extend `validate()` |
| `src/cli/state.rs` | **NEW** — Daemon state file tracking (`DaemonState`, `stories_processed`) |
| `src/cli/mod.rs` | Add `pub mod state;`, add `run_status()`, `run_logs()`, `SprintSummary`, `parse_level_priority()`, extend `CliError` with `State`/`LogFile` variants, update `init_tracing()` with dual output (Mutex\<File\>), update `run_start()` with discovery + state, update `run_polling_loop()` signature, update `Commands::Logs` with args, **update `collect_config_interactively()` to include `log_file`**, **update `make_test_config()` to include `log_file`** |
| `src/main.rs` | Replace Status/Logs placeholder arms with real dispatch |
| `bmad-bot.yaml.example` | Add `log_file` field |
| `.gitignore` | Add `bmad-bot.state.json`, `bmad-bot.log` |

### Anti-Patterns to Avoid

- ❌ **NO** `unwrap()` or `expect()` in production code — use `map_err` or `?` with typed errors
- ❌ **NO** `anyhow::Result` in `cli/mod.rs` or `config/discovery.rs` — typed errors only
- ❌ **NO** panicking in `BmadDiscovery::discover()` — always return a valid struct, even if empty
- ❌ **NO** blocking async runtime with synchronous file I/O in the polling loop — state file writes are small and fast enough to be acceptable; if concerned, use `tokio::fs` for writes in the loop
- ❌ **NO** logging sensitive data — state file should never contain API keys or tokens
- ❌ **NO** modifying modules other than `cli/`, `config/`, `main.rs`, `Cargo.toml`, `.gitignore`, and `bmad-bot.yaml.example`
- ❌ **NO** leaving the old `init_tracing()` implementation alongside the new one — replace it entirely
- ❌ **NO** state file that grows without bound — single JSON object, overwritten each time
- ❌ **NO** assuming sprint-status.yaml exists — `SprintSummary::from_file()` returns defaults if missing
- ❌ **NO** assuming `_bmad/` directory exists — `BmadDiscovery::discover()` handles missing gracefully
- ❌ **NO** passing a bare `std::fs::File` to `.with_writer()` in tracing — it does NOT implement `MakeWriter`. Always wrap in `Mutex<File>`
- ❌ **NO** using `libc` crate or `unsafe` for PID checks — use `std::process::Command::new("kill")` with signal 0 instead
- ❌ **NO** using `Option<usize>` with `default_value` for `--tail` arg — use plain `usize` with `default_value_t` to avoid redundant unwrap

### Scope Boundaries

**IN SCOPE for this story:**
- `src/config/discovery.rs` — BMAD auto-discovery (version, modules)
- `src/cli/state.rs` — daemon state tracking (PID, timestamps, status)
- `src/cli/mod.rs` — `run_status()`, `run_logs()`, `SprintSummary`, extend `CliError`, update `init_tracing()`, update `run_start()`, update `run_polling_loop()`
- `src/config/mod.rs` — add `log_file` field, extend `validate()`, add `pub mod discovery`
- `src/main.rs` — replace Status/Logs placeholders
- `bmad-bot.yaml.example` — add `log_file`
- `.gitignore` — add state and log files

**OUT OF SCOPE — do NOT implement:**
- Sprint-status.yaml polling and story detection (Story 2.1 — the `status` command only READS sprint data for display)
- Watcher module implementation (Story 2.1)
- Dependency resolution (Story 2.2)
- Any modifications to watcher, session, supervisor, review, tools, git_provider, notifier module stubs
- Real-time log streaming / `tail -f` behavior (MVP reads the file statically)
- Log rotation or size limits (future enhancement)
- Remote daemon status (MVP assumes local execution)

### Testing Requirements

Tests for `src/config/discovery.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_discover_with_valid_bmad_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let bmad_dir = tmp.path().join("_bmad");
        fs::create_dir_all(bmad_dir.join("bmm")).unwrap();
        fs::create_dir_all(bmad_dir.join("core")).unwrap();

        // Create a config.yaml with version comment
        let config_content = "# Version: 6.0.0-Beta.7\nproject_name: test\n";
        fs::create_dir_all(bmad_dir.join("bmm")).unwrap();
        fs::write(bmad_dir.join("bmm/config.yaml"), config_content).unwrap();

        let discovery = BmadDiscovery::discover(tmp.path());
        assert!(discovery.bmad_detected);
        assert_eq!(discovery.bmad_version, Some("6.0.0-Beta.7".to_string()));
        assert!(discovery.installed_modules.contains(&"bmm".to_string()));
        assert!(discovery.installed_modules.contains(&"core".to_string()));
        assert!(discovery.config_path.is_some());
    }

    #[test]
    fn test_discover_without_bmad_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let discovery = BmadDiscovery::discover(tmp.path());
        assert!(!discovery.bmad_detected);
        assert!(discovery.bmad_version.is_none());
        assert!(discovery.installed_modules.is_empty());
        assert!(discovery.config_path.is_none());
    }

    #[test]
    fn test_discover_with_partial_bmad_no_config() {
        let tmp = tempfile::tempdir().unwrap();
        let bmad_dir = tmp.path().join("_bmad");
        fs::create_dir_all(bmad_dir.join("core")).unwrap();

        let discovery = BmadDiscovery::discover(tmp.path());
        assert!(discovery.bmad_detected);
        assert!(discovery.bmad_version.is_none());
        assert!(discovery.installed_modules.contains(&"core".to_string()));
        assert!(!discovery.installed_modules.contains(&"bmm".to_string()));
        assert!(discovery.config_path.is_none());
    }

    #[test]
    fn test_extract_version_from_comment() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.yaml");
        fs::write(&config_path, "# Version: 5.1.2\nkey: value\n").unwrap();
        assert_eq!(
            BmadDiscovery::extract_version(&config_path),
            Some("5.1.2".to_string())
        );
    }

    #[test]
    fn test_extract_version_returns_none_for_missing_version() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.yaml");
        fs::write(&config_path, "key: value\n").unwrap();
        assert_eq!(BmadDiscovery::extract_version(&config_path), None);
    }

    #[test]
    fn test_display_with_bmad_detected() {
        let discovery = BmadDiscovery {
            bmad_version: Some("6.0.0".to_string()),
            installed_modules: vec!["bmm".to_string(), "core".to_string()],
            config_path: Some(PathBuf::from("_bmad/bmm/config.yaml")),
            project_root: PathBuf::from("."),
            bmad_detected: true,
        };
        let output = format!("{discovery}");
        assert!(output.contains("Detected"));
        assert!(output.contains("6.0.0"));
        assert!(output.contains("bmm, core"));
    }

    #[test]
    fn test_display_without_bmad() {
        let discovery = BmadDiscovery {
            bmad_version: None,
            installed_modules: Vec::new(),
            config_path: None,
            project_root: PathBuf::from("."),
            bmad_detected: false,
        };
        let output = format!("{discovery}");
        assert!(output.contains("Not detected"));
    }
}
```

Tests for `src/cli/state.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_test_state() -> DaemonState {
        DaemonState {
            pid: std::process::id(),
            started_at: "2026-02-07T10:00:00+01:00".to_string(),
            last_activity: "2026-02-07T10:05:00+01:00".to_string(),
            status: "running".to_string(),
            log_file: PathBuf::from("bmad-bot.log"),
            bmad_discovery: None,
            stories_processed: 0,
        }
    }

    #[test]
    fn test_state_write_and_read_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("test.state.json");
        let state = make_test_state();

        state.write(&state_path).unwrap();
        let loaded = DaemonState::read(&state_path).unwrap().unwrap();

        assert_eq!(loaded.pid, state.pid);
        assert_eq!(loaded.started_at, state.started_at);
        assert_eq!(loaded.status, "running");
    }

    #[test]
    fn test_state_read_returns_none_for_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("nonexistent.json");
        let result = DaemonState::read(&state_path).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_state_cleanup_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("test.state.json");
        let state = make_test_state();
        state.write(&state_path).unwrap();
        assert!(state_path.exists());

        DaemonState::cleanup(&state_path).unwrap();
        assert!(!state_path.exists());
    }

    #[test]
    fn test_state_cleanup_noop_for_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("nonexistent.json");
        // Should not error
        DaemonState::cleanup(&state_path).unwrap();
    }

    #[test]
    fn test_is_process_alive_with_current_pid() {
        assert!(DaemonState::is_process_alive(std::process::id()));
    }

    #[test]
    fn test_is_process_alive_with_zero_pid() {
        assert!(!DaemonState::is_process_alive(0));
    }

    #[test]
    fn test_touch_updates_last_activity() {
        let mut state = make_test_state();
        let before = state.last_activity.clone();
        std::thread::sleep(std::time::Duration::from_millis(10));
        state.touch();
        assert_ne!(state.last_activity, before);
    }

    #[test]
    fn test_mark_stopped() {
        let mut state = make_test_state();
        assert_eq!(state.status, "running");
        state.mark_stopped();
        assert_eq!(state.status, "stopped");
    }

    #[test]
    fn test_record_story_processed_increments() {
        let mut state = make_test_state();
        assert_eq!(state.stories_processed, 0);
        state.record_story_processed();
        assert_eq!(state.stories_processed, 1);
        state.record_story_processed();
        assert_eq!(state.stories_processed, 2);
    }

    #[test]
    fn test_stories_processed_survives_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let state_path = tmp.path().join("test.state.json");
        let mut state = make_test_state();
        state.record_story_processed();
        state.record_story_processed();
        state.record_story_processed();
        state.write(&state_path).unwrap();

        let loaded = DaemonState::read(&state_path).unwrap().unwrap();
        assert_eq!(loaded.stories_processed, 3);
    }
}
```

Tests for sprint summary parsing in `src/cli/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    // ... existing tests ...

    #[test]
    fn test_sprint_summary_from_valid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sprint-status.yaml");
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: in-progress
  1-3-init: ready-for-dev
  1-4-status: backlog
  epic-1-retrospective: optional
  epic-2: backlog
  2-1-polling: backlog
  2-2-deps: backlog
"#;
        std::fs::write(&path, content).unwrap();
        let summary = SprintSummary::from_file(&path);
        assert_eq!(summary.total_stories, 5);
        assert_eq!(summary.done, 1);
        assert_eq!(summary.in_progress, 1);
        assert_eq!(summary.ready_for_dev, 1);
        assert_eq!(summary.backlog, 2);
        assert_eq!(summary.total_epics, 2);
        assert_eq!(summary.epics_in_progress, 1);
    }

    #[test]
    fn test_sprint_summary_from_missing_file() {
        let summary = SprintSummary::from_file(Path::new("/nonexistent/path.yaml"));
        assert_eq!(summary.total_stories, 0);
        assert_eq!(summary.total_epics, 0);
    }

    #[test]
    fn test_sprint_summary_from_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("empty.yaml");
        std::fs::write(&path, "").unwrap();
        let summary = SprintSummary::from_file(&path);
        assert_eq!(summary.total_stories, 0);
    }

    #[test]
    fn test_parse_level_priority_ordering() {
        assert!(parse_level_priority("ERROR") > parse_level_priority("WARN"));
        assert!(parse_level_priority("WARN") > parse_level_priority("INFO"));
        assert!(parse_level_priority("INFO") > parse_level_priority("DEBUG"));
        assert!(parse_level_priority("DEBUG") > parse_level_priority("TRACE"));
    }

    #[test]
    fn test_parse_level_priority_case_insensitive() {
        assert_eq!(parse_level_priority("error"), parse_level_priority("ERROR"));
        assert_eq!(parse_level_priority("Info"), parse_level_priority("INFO"));
    }

    #[test]
    fn test_parse_level_priority_unknown_returns_zero() {
        assert_eq!(parse_level_priority("foo"), 0);
        assert_eq!(parse_level_priority("verbose"), 0);
        assert_eq!(parse_level_priority(""), 0);
    }

    #[test]
    fn test_sprint_summary_counts_blocked_stories() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sprint-status.yaml");
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  1-2-cli: blocked
  1-3-init: blocked
"#;
        std::fs::write(&path, content).unwrap();
        let summary = SprintSummary::from_file(&path);
        assert_eq!(summary.total_stories, 3);
        assert_eq!(summary.done, 1);
        assert_eq!(summary.blocked, 2);
    }

    #[test]
    fn test_config_validate_accepts_log_file() {
        // Arrange: BotConfig with log_file = "my-daemon.log"
        // Act: config.validate()
        // Assert: Ok(())
    }

    #[test]
    fn test_config_validate_rejects_empty_log_file() {
        // Arrange: BotConfig with log_file = ""
        // Act: config.validate()
        // Assert: Err(ConfigError::InvalidField { field: "log_file", .. })
    }
}
```

### Project Structure Notes

After this story, the project structure under `src/` expands to:

```
src/
├── main.rs
├── cli/
│   ├── mod.rs             # Updated: run_status, run_logs, SprintSummary, extended CliError
│   └── state.rs           # NEW: DaemonState for cross-process status
├── config/
│   ├── mod.rs             # Updated: log_file field, pub mod discovery
│   └── discovery.rs       # NEW: BMAD auto-discovery
├── watcher/               # (stubs — untouched)
│   ├── mod.rs
│   └── deps.rs
├── session/               # (stubs — untouched)
│   ├── mod.rs
│   └── state.rs           # NOTE: This is session WAL state, NOT daemon state — different module
├── supervisor/            # (stubs — untouched)
│   ├── mod.rs
│   ├── rules.rs
│   └── decisions.rs
├── review/                # (stubs — untouched)
│   └── mod.rs
├── tools/                 # (stubs — untouched)
│   ├── mod.rs
│   ├── git.rs
│   ├── fs.rs
│   └── terminal.rs
├── git_provider/          # (stubs — untouched)
│   ├── mod.rs
│   ├── github.rs
│   └── gitlab.rs
└── notifier/              # (stubs — untouched)
    └── mod.rs
```

> **⚠️ NAME COLLISION WARNING:** `src/cli/state.rs` (daemon state file) and `src/session/state.rs` (session WAL persistence, Story 6.3) are DIFFERENT modules with different purposes. The daemon state tracks PID/timestamps for the `status` command. The session WAL tracks LLM chat history for crash recovery. They share no code or data structures.

### Alignment with Unified Project Structure

- Discovery module lives in `config/` (alongside config loading) because BMAD detection is a configuration concern — it's discovered at startup alongside config validation
- Daemon state lives in `cli/` because it's exclusively used by CLI commands (status reads it, start writes it)
- Sprint summary parsing is inlined in `cli/mod.rs` for now — it's only used by `run_status()`. When Story 2.1 implements the watcher, sprint parsing may be extracted to `watcher/mod.rs` and shared

### References

- [Source: epics.md § Story 1.4: Status, Logs & BMAD Discovery] — User story, acceptance criteria
- [Source: prd.md § CLI Command Surface] — `bmad-bot status` and `bmad-bot logs` descriptions
- [Source: prd.md § FR29] — `bmad-bot status` to view current daemon state
- [Source: prd.md § FR30] — `bmad-bot logs` to view structured daemon logs
- [Source: prd.md § FR32] — Auto-discover BMAD version and installed modules
- [Source: architecture.md § Project Structure & Boundaries] — Complete directory structure, module map
- [Source: architecture.md § Decision 6: Deployment Model] — Foreground process, logs to stdout/stderr
- [Source: architecture.md § Tracing Pattern] — Structured spans with story context
- [Source: architecture.md § Config Pattern] — Validate once, share via Arc
- [Source: project-context.md § CLI Rules] — `bmad-bot status` (current state summary), `bmad-bot logs` (structured tracing logs), BMAD auto-discovery
- [Source: project-context.md § Language-Specific Rules] — `tracing` exclusively, no println in daemon runtime, edition 2024
- [Source: project-context.md § Testing Rules] — Inline tests in `#[cfg(test)] mod tests`, descriptive snake_case names
- [Source: project-context.md § Code Quality] — `///` doc comments on all public items, `rustfmt` default config
- [Source: Story 1.1] — BotConfig, ConfigError, module stubs, tempfile dev-dependency
- [Source: Story 1.2] — Cli, Commands, CliError, init_tracing, run_start, run_polling_loop, main.rs dispatch
- [Source: Story 1.3] — CliError extensions (Init, Io, UserCancelled), Serialize on config structs, chrono dependency

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

- `cargo fmt -- --check` — clean, no formatting issues
- `cargo clippy -- -D warnings` — clean after adding `#[allow(dead_code)]` on pre-existing unused items (`UserCancelled`, `build_http_client`) and future-use items (`record_story_processed`, `module_descriptions`)
- `cargo test` — 113/113 tests pass (63 pre-existing + 14 discovery + 13 state + 23 new cli tests)

### Completion Notes List

- **Task 0:** Added `log_file` field with `#[serde(default)]` to `BotConfig`, updated `collect_config_interactively()`, `make_test_config()`, and `_test_minimal()`. Verified serde_json already in Cargo.toml. All 63 pre-existing tests pass.
- **Task 1:** Created `src/config/discovery.rs` — `BmadDiscovery` struct, `discover()` (never fails, returns empty on missing `_bmad/`), `extract_version()` (comment-style `# Version:` + YAML field `bmad_version:`), `Display` impl, `module_descriptions()`. Re-exported via `pub mod discovery;`. 14 unit tests.
- **Task 2:** Created `src/cli/state.rs` — `DaemonState` struct with atomic write (tmp+rename), read, cleanup, PID alive check via `kill -0` (no libc/unsafe), `new_running()`, `touch()`, `mark_stopped()`, `record_story_processed()`. Extended `CliError` with `State` and `LogFile` variants. 13 unit tests.
- **Task 3:** Replaced `init_tracing()` with dual-output: stdout (config-driven pretty/JSON via `.boxed()`) + file (always JSON via `Mutex<File>` for `MakeWriter`). Added `log_file` validation in `BotConfig::validate()`. Updated `bmad-bot.yaml.example`.
- **Task 4:** Updated `run_start()` — BMAD discovery at startup with info/warn logging, state file write, state touch in polling loop, graceful shutdown with mark_stopped + cleanup. Updated `run_polling_loop()` signature to accept `&mut DaemonState`.
- **Task 5:** Implemented `run_status()` — reads state file, checks PID liveness, displays daemon info with box-drawing header, shows BMAD discovery (from state or fresh), parses `sprint-status.yaml` via `SprintSummary`. Cleans up stale state files.
- **Task 6:** Implemented `run_logs()` — reads JSON log file, `--tail N` (default 50), `--level` filtering via `parse_level_priority()`, emoji level icons, story_id extraction, invalid level warning, graceful handling of missing/empty files and non-JSON lines.
- **Task 7:** Extended `Commands::Logs` with `--level` (`Option<String>`) and `--tail` (`usize`, `default_value_t = 50`). Updated `main.rs` dispatch for both `Status` and `Logs`.
- **Task 8:** Added 23 new tests in `cli/mod.rs`: 8 SprintSummary tests (valid file, missing, empty, blocked, review, retrospectives, display blocked/omit), 5 parse_level_priority tests (ordering, case-insensitive, unknown, WARNING alias, all nonzero), 4 config log_file validation tests (accept, reject empty, reject whitespace, custom path), 4 CLI Logs args parsing tests (level+tail, default tail, short flags, level only), 2 CliError display tests (State, LogFile).
- **Task 9:** `cargo fmt` clean, `cargo clippy -D warnings` clean, 113/113 tests pass. All public items have `///` doc comments. Manual integration tests (9.5–9.7) left for user to verify with running daemon.
- **Decision:** Used `serde_yml` (project's existing YAML crate) instead of `serde_yaml` for `SprintSummary` parsing. Adapted story's reference code accordingly.
- **Decision:** Fixed story spec test data errors: `test_sprint_summary_from_valid_file` expected 5 stories but YAML had 6, and expected 2 backlog but YAML had 3. Corrected assertions to match actual test data.

### Change Log

- Story 1.4 implementation complete — status, logs, BMAD discovery (Date: 2026-02-07)

### File List

- `src/config/discovery.rs` — **NEW** — BMAD auto-discovery module
- `src/config/mod.rs` — Added `pub mod discovery;`, `log_file` field to `BotConfig`, `default_log_file()`, `validate()` extension, `#[allow(dead_code)]` on `build_http_client`
- `src/cli/state.rs` — **NEW** — Daemon state file tracking (`DaemonState`, `STATE_FILE_NAME`)
- `src/cli/mod.rs` — Added `pub mod state;`, `run_status()`, `run_logs()`, `SprintSummary`, `parse_level_priority()`, `State`/`LogFile` CliError variants, replaced `init_tracing()` with dual-output, updated `run_start()` with discovery+state, updated `run_polling_loop()` with state touch, updated `Commands::Logs` with args, updated `collect_config_interactively()` and `make_test_config()` with `log_file`, `#[allow(dead_code)]` on `UserCancelled`, 23 new tests
- `src/main.rs` — Replaced Status/Logs placeholder arms with `run_status()`/`run_logs()` dispatch
- `bmad-bot.yaml.example` — Added `log_file` field
- `.gitignore` — Added `bmad-bot.state.json`, `bmad-bot.log`
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — Updated `1-4-status-logs-bmad-discovery` status to `review`