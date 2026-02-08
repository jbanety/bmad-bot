# Story 1.2: CLI Framework & Daemon Lifecycle

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want to launch the daemon with `bmad-bot start` and have it run with structured logging and clean shutdown,
So that I have a controllable long-running process as the foundation for all pipeline features.

## Acceptance Criteria

1. **Given** the project has the clap dependency configured **When** I build the CLI module **Then** clap with derive API defines four subcommands: `init`, `start`, `status`, `logs` **And** each subcommand has auto-generated `--help` documentation

2. **Given** a valid configuration file exists **When** I run `bmad-bot start` **Then** the daemon loads and validates the config, initializes structured tracing (JSON or pretty-print based on config) to stdout/stderr, and enters a polling loop (placeholder that sleeps for the configured interval) **And** tracing is the only logging mechanism — no `println!` or `eprintln!` anywhere **And** sensitive fields (API keys, tokens) are never present in log output

3. **Given** the daemon is running **When** a SIGTERM or SIGINT signal is received **Then** the daemon initiates graceful shutdown via tokio::signal, logs the shutdown event, and exits cleanly with code 0 **And** no partial state is left behind

## Tasks / Subtasks

- [x] Task 1: Implement CLI module with clap derive API (AC: #1)
  - [x] 1.1 Define `Cli` struct with `#[derive(Parser)]` and top-level `--config` option (default: `bmad-bot.yaml`)
  - [x] 1.2 Define `Commands` enum with `#[derive(Subcommand)]` for `init`, `start`, `status`, `logs`
  - [x] 1.3 Implement `Start` subcommand struct (no extra args for MVP)
  - [x] 1.4 Implement `Init`, `Status`, `Logs` subcommand structs as placeholders
  - [x] 1.5 Add `/// doc comments` on all public items for `--help` auto-generation
  - [x] 1.6 Verify `bmad-bot --help`, `bmad-bot --version`, and `bmad-bot start --help` produce correct output

- [x] Task 2: Add logging config field to BotConfig (AC: #2)
  - [x] 2.1 Add `log_format` field to `BotConfig` with `#[serde(default = "default_log_format")]` defaulting to `"pretty"`
  - [x] 2.2 Add `log_level` field to `BotConfig` with `#[serde(default = "default_log_level")]` defaulting to `"info"`
  - [x] 2.3 Validate `log_format` is one of: `"json"`, `"pretty"`
  - [x] 2.4 Validate `log_level` is one of: `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`
  - [x] 2.5 Update `bmad-bot.yaml.example` with new fields and comments

- [x] Task 3: Implement structured tracing setup (AC: #2)
  - [x] 3.1 Create `init_tracing(config: &BotConfig) -> Result<(), CliError>` function in `cli/mod.rs`
  - [x] 3.2 Support JSON format via `tracing_subscriber::fmt().json()` layer
  - [x] 3.3 Support pretty-print format via `tracing_subscriber::fmt()` default layer
  - [x] 3.4 Configure `EnvFilter` from `config.log_level` (with `RUST_LOG` env override)
  - [x] 3.5 Replace the minimal `tracing_subscriber::fmt::init()` from Story 1.1 with this config-driven setup

- [x] Task 4: Implement `start` command handler (AC: #2)
  - [x] 4.1 Create `run_start(config_path: &Path) -> Result<(), CliError>` async function
  - [x] 4.2 Load and validate `BotConfig` from provided path
  - [x] 4.3 Load `BotSecrets` from `.env` and validate against config (see `validate_for_config` in Dev Notes)
  - [x] 4.4 Initialize structured tracing from config
  - [x] 4.5 Wrap config in `Arc<BotConfig>` for sharing
  - [x] 4.6 Log startup info: version, config path, polling interval, git provider, log format
  - [x] 4.7 Enter polling loop placeholder (sleep for `polling_interval_secs`, log each cycle)

- [x] Task 5: Implement graceful shutdown (AC: #3)
  - [x] 5.1 Set up `tokio::signal::ctrl_c()` handler for SIGINT
  - [x] 5.2 Set up `tokio::signal::unix::signal(SignalKind::terminate())` handler for SIGTERM
  - [x] 5.3 Use `tokio::select!` in the polling loop to race between sleep and shutdown signal
  - [x] 5.4 On signal: log shutdown event with `tracing::info!`, break loop, exit cleanly with code 0
  - [x] 5.5 Ensure no partial state is left behind (no files open, no locks held)

- [x] Task 6: Wire CLI into main.rs (AC: #1, #2, #3)
  - [x] 6.1 Replace Story 1.1 placeholder main.rs with full CLI dispatch
  - [x] 6.2 Parse CLI args via `Cli::parse()`
  - [x] 6.3 Match on `Commands` and dispatch to handlers
  - [x] 6.4 `Init` / `Status` / `Logs` → print "Not yet implemented — see Story 1.3/1.4" via `tracing::warn!`
  - [x] 6.5 `Start` → call `run_start()`
  - [x] 6.6 Convert `CliError` to `anyhow::Error` at the main.rs boundary only

- [x] Task 7: Define CliError thiserror enum
  - [x] 7.1 Create `CliError` in `cli/mod.rs` following per-module thiserror pattern
  - [x] 7.2 Variants: `ConfigLoad(ConfigError)`, `TracingInit(String)`, `Signal(std::io::Error)`
  - [x] 7.3 Implement `From<ConfigError>` for seamless error propagation

- [x] Task 8: Write unit tests (AC: #1, #2, #3)
  - [x] 8.1 Test CLI parses `start` subcommand correctly
  - [x] 8.2 Test CLI parses `--config custom.yaml start` correctly
  - [x] 8.3 Test CLI parses all four subcommands without error
  - [x] 8.4 Test `log_format` validation rejects invalid values
  - [x] 8.5 Test `log_level` validation rejects invalid values
  - [x] 8.6 Test default `log_format` is "pretty" and default `log_level` is "info"
  - [x] 8.7 Test tracing initializes without panic for both JSON and pretty modes

- [x] Task 9: Final quality checks
  - [x] 9.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [x] 9.2 Run `cargo clippy` and fix any warnings
  - [x] 9.3 Run `cargo test` and verify all tests pass (including Story 1.1 tests)
  - [x] 9.4 Verify all public items have `///` doc comments
  - [x] 9.5 Manual integration test: `cp bmad-bot.yaml.example bmad-bot.yaml`, create minimal `.env` (empty values ok for this test), run `cargo run -- start`, verify structured log output appears, press Ctrl-C and verify graceful shutdown message, then clean up test files
  - [x] 9.6 Verify `cargo run -- --version` outputs version correctly

## Dev Notes

### Previous Story Intelligence (Story 1.1)

Story 1.1 established:
- **Project scaffolding** with all module stubs, Cargo.toml with all dependencies (including `clap = { version = "4", features = ["derive"] }`)
- **`src/cli/mod.rs`** exists as a stub — this story replaces it with a full implementation
- **`src/config/mod.rs`** implements `BotConfig`, `BotSecrets`, `ConfigError`, `build_http_client()` — this story EXTENDS BotConfig with two new fields and adds `BotSecrets::validate_for_config()` but does NOT rewrite the config module
- **`src/main.rs`** has a minimal skeleton with `tracing_subscriber::fmt::init()` — this story REPLACES that with config-driven tracing and full CLI dispatch
- **All other modules** remain as stubs — do NOT modify them

> **NOTE on pre-tracing errors:** Config loading errors in `run_start()` happen BEFORE `init_tracing()` is called. These errors are displayed via `anyhow`'s Debug format on stderr — this is expected and correct. Do NOT attempt to log config errors via `tracing` before the subscriber is initialized.

### CLI Module Design — `src/cli/mod.rs`

```rust
//! CLI module — clap-based command-line interface for bmad-bot.
//!
//! Provides `init`, `start`, `status`, `logs` subcommands.

use std::path::PathBuf;
use clap::{Parser, Subcommand};

/// BMAD Bot — Autonomous AI developer daemon powered by the BMAD methodology.
#[derive(Parser, Debug)]
#[command(name = "bmad-bot", version, about, long_about = None)]
pub struct Cli {
    /// Path to the configuration file.
    #[arg(long, short, default_value = "bmad-bot.yaml", global = true)]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

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
    Logs,
}
```

### CliError Pattern

```rust
/// Errors originating from CLI operations.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("Configuration error: {0}")]
    Config(#[from] crate::config::ConfigError),

    #[error("Failed to initialize tracing: {reason}")]
    TracingInit { reason: String },

    #[error("Signal handler error: {0}")]
    Signal(#[from] std::io::Error),
}
```

> `anyhow` is used ONLY in `main.rs` to convert `CliError` at the binary boundary. `cli/mod.rs` returns `Result<(), CliError>` exclusively.

### BotConfig Extensions

Add these two fields to the existing `BotConfig` struct in `src/config/mod.rs`:

```rust
#[derive(Debug, Deserialize)]
pub struct BotConfig {
    // ... existing fields from Story 1.1 ...

    /// Log output format: "json" or "pretty". Default: "pretty".
    #[serde(default = "default_log_format")]
    pub log_format: String,

    /// Log level filter: "trace", "debug", "info", "warn", "error". Default: "info".
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_log_format() -> String { "pretty".to_string() }
fn default_log_level() -> String { "info".to_string() }
```

Add `validate_for_config()` to `BotSecrets` in `src/config/mod.rs`. Story 1.1 may or may not have implemented this — if it doesn't exist, add it now:

```rust
impl BotSecrets {
    // load() already exists from Story 1.1

    /// Validates that all required secrets are present based on configured providers.
    /// Must be called AFTER BotConfig is loaded and validated.
    pub fn validate_for_config(&self, config: &BotConfig) -> Result<(), ConfigError> {
        // Check LLM provider secrets
        for (role, role_config) in [
            ("dev", &config.llm.dev),
            ("review", &config.llm.review),
            ("supervisor", &config.llm.supervisor),
        ] {
            match role_config.provider.as_str() {
                "anthropic" => {
                    if self.anthropic_api_key.as_ref().map_or(true, |k| k.is_empty()) {
                        return Err(ConfigError::MissingSecret {
                            env_var: "ANTHROPIC_API_KEY".to_string(),
                            purpose: format!("LLM provider for '{role}' role"),
                        });
                    }
                }
                "openai" => {
                    if self.openai_api_key.as_ref().map_or(true, |k| k.is_empty()) {
                        return Err(ConfigError::MissingSecret {
                            env_var: "OPENAI_API_KEY".to_string(),
                            purpose: format!("LLM provider for '{role}' role"),
                        });
                    }
                }
                "github-models" => {
                    if self.github_models_api_key.as_ref().map_or(true, |k| k.is_empty()) {
                        return Err(ConfigError::MissingSecret {
                            env_var: "GITHUB_MODELS_API_KEY".to_string(),
                            purpose: format!("LLM provider for '{role}' role"),
                        });
                    }
                }
                _ => {} // Unknown provider caught by BotConfig::validate()
            }
        }

        // Check git provider token
        match config.git_provider.provider.as_str() {
            "github" => {
                if self.github_token.as_ref().map_or(true, |k| k.is_empty()) {
                    return Err(ConfigError::MissingSecret {
                        env_var: "GITHUB_TOKEN".to_string(),
                        purpose: "Git provider (GitHub)".to_string(),
                    });
                }
            }
            "gitlab" => {
                if self.gitlab_token.as_ref().map_or(true, |k| k.is_empty()) {
                    return Err(ConfigError::MissingSecret {
                        env_var: "GITLAB_TOKEN".to_string(),
                        purpose: "Git provider (GitLab)".to_string(),
                    });
                }
            }
            _ => {}
        }

        // Telegram token only required if notifications enabled
        if config.notifications.telegram.enabled {
            if self.telegram_bot_token.as_ref().map_or(true, |k| k.is_empty()) {
                return Err(ConfigError::MissingSecret {
                    env_var: "TELEGRAM_BOT_TOKEN".to_string(),
                    purpose: "Telegram notifications (enabled)".to_string(),
                });
            }
        }

        Ok(())
    }
}
```

Add validation to the existing `BotConfig::validate()` method:

```rust
// Inside validate():
let valid_log_formats = ["json", "pretty"];
if !valid_log_formats.contains(&self.log_format.as_str()) {
    return Err(ConfigError::InvalidField {
        field: "log_format".to_string(),
        reason: format!("must be one of: {}", valid_log_formats.join(", ")),
    });
}

let valid_log_levels = ["trace", "debug", "info", "warn", "error"];
if !valid_log_levels.contains(&self.log_level.as_str()) {
    return Err(ConfigError::InvalidField {
        field: "log_level".to_string(),
        reason: format!("must be one of: {}", valid_log_levels.join(", ")),
    });
}
```

### Tracing Setup

```rust
use tracing_subscriber::{fmt, EnvFilter};

/// Initialize structured tracing based on config.
/// Uses `RUST_LOG` env var as override if set, otherwise config.log_level.
pub fn init_tracing(config: &BotConfig) -> Result<(), CliError> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    // NOTE: Each branch calls try_init() independently — do NOT extract the builder
    // into a common variable. The JSON and pretty builders produce different types
    // (JsonFields vs DefaultFields) and cannot be unified without boxing.
    let result = match config.log_format.as_str() {
        "json" => {
            fmt()
                .json()
                .with_env_filter(env_filter)
                .with_target(true)
                .with_thread_ids(false)
                .try_init()
        }
        _ => {
            // "pretty" or fallback
            fmt()
                .with_env_filter(env_filter)
                .with_target(true)
                .with_thread_ids(false)
                .try_init()
        }
    };

    result.map_err(|e| CliError::TracingInit {
        reason: e.to_string(),
    })
}
```

> **IMPORTANT:** This replaces the `tracing_subscriber::fmt::init()` call from Story 1.1's main.rs. Do NOT call both — only this config-driven init.

### Start Command Handler

```rust
use std::sync::Arc;
use tokio::signal;

/// Runs the `start` command: load config, init tracing, enter polling loop.
pub async fn run_start(config_path: &std::path::Path) -> Result<(), CliError> {
    let config = crate::config::BotConfig::load(config_path)?;
    config.validate()?;

    init_tracing(&config)?;

    let secrets = crate::config::BotSecrets::load()?;
    secrets.validate_for_config(&config)?;

    let config = Arc::new(config);

    tracing::info!(
        config_path = %config_path.display(),
        polling_interval_secs = config.polling_interval_secs,
        git_provider = %config.git_provider.provider,
        log_format = %config.log_format,
        "bmad-bot daemon started"
    );

    // Polling loop with graceful shutdown
    run_polling_loop(&config).await?;

    tracing::info!("bmad-bot daemon stopped cleanly");
    Ok(())
}
```

### Graceful Shutdown with tokio::select!

```rust
use std::sync::Arc;
use tokio::time::{sleep, Duration};

/// Placeholder polling loop. Sleeps for configured interval, checks for shutdown.
/// Story 2.1 replaces the sleep with sprint-status.yaml polling.
async fn run_polling_loop(config: &Arc<BotConfig>) -> Result<(), CliError> {
    let interval = Duration::from_secs(config.polling_interval_secs);

    // Set up shutdown signal handlers
    let mut sigterm = tokio::signal::unix::signal(
        tokio::signal::unix::SignalKind::terminate()
    )?;

    loop {
        tokio::select! {
            _ = sleep(interval) => {
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

> **CRITICAL:** `tokio::signal::unix` is only available on Unix. This is acceptable for MVP (macOS/Linux targets). If Windows support is ever needed, gate behind `#[cfg(unix)]` with a fallback.

### Updated main.rs

```rust
#![deny(clippy::all)]
#![warn(dead_code)] // FIXME: Change to #![deny(dead_code)] once all modules have real implementations

mod cli;
mod config;
mod git_provider;
mod notifier;
mod review;
mod session;
mod supervisor;
mod tools;
mod watcher;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    match cli.command {
        cli::Commands::Start => {
            cli::run_start(&cli.config).await?;
        }
        cli::Commands::Init => {
            // Minimal tracing for non-start commands — use try_init to avoid panic
            let _ = tracing_subscriber::fmt::try_init();
            tracing::warn!("'init' command not yet implemented — see Story 1.3");
        }
        cli::Commands::Status => {
            let _ = tracing_subscriber::fmt::try_init();
            tracing::warn!("'status' command not yet implemented — see Story 1.4");
        }
        cli::Commands::Logs => {
            let _ = tracing_subscriber::fmt::try_init();
            tracing::warn!("'logs' command not yet implemented — see Story 1.4");
        }
    }

    Ok(())
}
```

> **NOTE:** The `start` command initializes tracing via config-driven `init_tracing()`. Placeholder commands use `try_init()` (not `init()`) to avoid panics if a subscriber is already set. Story 1.3/1.4 will implement these properly.

### Updated bmad-bot.yaml.example Additions

Add these fields to the existing `bmad-bot.yaml.example` created in Story 1.1:

```yaml
# Logging configuration
# log_format: "pretty" for human-readable, "json" for structured (machine-parseable)
# Override at runtime with RUST_LOG env var for log level.
log_format: pretty            # "pretty" or "json" (default: pretty)
log_level: info               # "trace", "debug", "info", "warn", "error" (default: info)
```

### Testing Requirements

Tests go inline at the bottom of `src/cli/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_cli_parse_start_command() {
        let cli = Cli::try_parse_from(["bmad-bot", "start"]).unwrap();
        assert!(matches!(cli.command, Commands::Start));
        assert_eq!(cli.config.to_str().unwrap(), "bmad-bot.yaml"); // default
    }

    #[test]
    fn test_cli_parse_custom_config_path() {
        let cli = Cli::try_parse_from(["bmad-bot", "--config", "custom.yaml", "start"]).unwrap();
        assert_eq!(cli.config.to_str().unwrap(), "custom.yaml");
    }

    #[test]
    fn test_cli_parse_all_subcommands() {
        for cmd in ["init", "start", "status", "logs"] {
            let result = Cli::try_parse_from(["bmad-bot", cmd]);
            assert!(result.is_ok(), "Failed to parse subcommand: {cmd}");
        }
    }

    #[test]
    fn test_cli_parse_short_config_flag() {
        let cli = Cli::try_parse_from(["bmad-bot", "-c", "my.yaml", "start"]).unwrap();
        assert_eq!(cli.config.to_str().unwrap(), "my.yaml");
    }
}
```

Add validation tests to `src/config/mod.rs` (extending Story 1.1 tests):

```rust
#[test]
fn test_config_validate_rejects_invalid_log_format() {
    // Arrange: BotConfig with log_format = "xml"
    // Act: config.validate()
    // Assert: Err(ConfigError::InvalidField { field: "log_format", .. })
}

#[test]
fn test_config_validate_rejects_invalid_log_level() {
    // Arrange: BotConfig with log_level = "verbose"
    // Act: config.validate()
    // Assert: Err(ConfigError::InvalidField { field: "log_level", .. })
}

#[test]
fn test_config_default_log_format_is_pretty() {
    // Arrange: YAML without log_format field
    // Act: deserialize
    // Assert: config.log_format == "pretty"
}

#[test]
fn test_config_default_log_level_is_info() {
    // Arrange: YAML without log_level field
    // Act: deserialize
    // Assert: config.log_level == "info"
}

#[test]
fn test_secrets_validate_for_config_missing_anthropic_key() {
    // Arrange: BotConfig with llm.dev.provider = "anthropic", BotSecrets with anthropic_api_key = None
    // Act: secrets.validate_for_config(&config)
    // Assert: Err(ConfigError::MissingSecret { env_var: "ANTHROPIC_API_KEY", .. })
}

#[test]
fn test_secrets_validate_for_config_missing_github_token() {
    // Arrange: BotConfig with git_provider.provider = "github", BotSecrets with github_token = None
    // Act: secrets.validate_for_config(&config)
    // Assert: Err(ConfigError::MissingSecret { env_var: "GITHUB_TOKEN", .. })
}

#[test]
fn test_secrets_validate_for_config_telegram_not_required_when_disabled() {
    // Arrange: BotConfig with notifications.telegram.enabled = false, BotSecrets with telegram_bot_token = None
    // Act: secrets.validate_for_config(&config)
    // Assert: Ok(()) — telegram token not required when disabled
}
```

### Anti-Patterns to Avoid

- ❌ **NO** `println!` or `eprintln!` anywhere — all output through `tracing` macros
- ❌ **NO** `unwrap()` or `expect()` in production code (tests only)
- ❌ **NO** `anyhow::Result` in `cli/mod.rs` — use `Result<(), CliError>` with thiserror
- ❌ **NO** logging of sensitive data (API keys, tokens, secrets) — even in debug/trace level
- ❌ **NO** `std::process::exit()` — return errors up to main and let it exit naturally
- ❌ **NO** `block_on()` inside async context or `std::thread::spawn` without justification
- ❌ **NO** modifying modules other than `cli/mod.rs`, `config/mod.rs`, and `main.rs` — all other stubs remain untouched

### Scope Boundaries

**IN SCOPE for this story:**
- `src/cli/mod.rs` — full implementation (Cli struct, Commands, CliError, init_tracing, run_start, run_polling_loop)
- `src/config/mod.rs` — add `log_format` + `log_level` fields, extend `validate()`, extend tests
- `src/main.rs` — replace with full CLI dispatch
- `bmad-bot.yaml.example` — add logging fields

**OUT OF SCOPE — do NOT implement:**
- `init` command interactive prompts (Story 1.3)
- `status` / `logs` command logic (Story 1.4)
- Sprint-status.yaml polling (Story 2.1 — replaced by sleep placeholder)
- BMAD auto-discovery (Story 1.4)
- Any modifications to watcher, session, supervisor, review, tools, git_provider, notifier modules

### References

- [Source: epics.md § Story 1.2: CLI Framework & Daemon Lifecycle] — User story, acceptance criteria
- [Source: architecture.md § Decision 6: Deployment Model] — Foreground process, no self-daemonization, logs to stdout/stderr
- [Source: architecture.md § Tracing Pattern] — Structured spans with story context, mandatory rules
- [Source: architecture.md § Config Pattern] — Validate once, share via Arc
- [Source: architecture.md § Decision Impact Analysis] — Cross-component dependencies, implementation sequence
- [Source: architecture.md § Architectural Boundaries] — cli → watcher → session flow, Arc<BotConfig> shared to all
- [Source: project-context.md § CLI Rules] — Command surface, config validation at startup
- [Source: project-context.md § Daemon Lifecycle] — Watcher → Pre-gate → Session → Supervisor → Review → Notification
- [Source: project-context.md § Language-Specific Rules] — Async tokio, no block_on, tracing exclusively
- [Source: prd.md § CLI Command Surface] — init, start, status, logs descriptions
- [Source: prd.md § Implementation Considerations] — Graceful shutdown, config validation
- [Source: Story 1.1] — Project scaffolding, BotConfig struct, module stubs, main.rs skeleton to replace

## Dev Agent Record

### Agent Model Used

Claude Opus 4 (via Zed)

### Debug Log References

- 2 test failures on first run: `test_secrets_validate_for_config_missing_github_token` and `test_secrets_validate_for_config_telegram_not_required_when_disabled` — caused by VALID_YAML using `openai` for supervisor but test secrets not providing `openai_api_key`. Fixed by adding `openai_api_key: Some("sk-openai-test".to_string())` to affected test fixtures.

### Completion Notes List

- **Task 1:** Implemented `Cli` struct with `#[derive(Parser)]`, `Commands` enum with 4 subcommands (`init`, `start`, `status`, `logs`). All public items have `///` doc comments for auto-generated `--help`. Verified `--help`, `--version`, `start --help` output.
- **Task 2:** Added `log_format` (default `"pretty"`) and `log_level` (default `"info"`) fields to `BotConfig` with serde defaults. Added validation in `BotConfig::validate()`. Updated `bmad-bot.yaml.example`.
- **Task 3:** Implemented `init_tracing()` in `cli/mod.rs` — supports JSON and pretty-print formats via `tracing_subscriber::fmt()`. Uses `EnvFilter` with `RUST_LOG` override. Replaces Story 1.1 hardcoded init.
- **Task 4:** Implemented `run_start()` async function — loads/validates config, loads/validates secrets, initializes tracing, wraps config in `Arc`, logs startup info, enters polling loop.
- **Task 5:** Implemented graceful shutdown with `tokio::select!` racing `ctrl_c()` (SIGINT) and `unix::signal(SIGTERM)` against sleep interval. Logs shutdown event, exits cleanly.
- **Task 6:** Replaced Story 1.1 `main.rs` with full CLI dispatch via `Cli::parse()`. Start dispatches to `run_start()`, other commands use `try_init()` + `tracing::warn!` placeholder. `CliError` → `anyhow::Error` at boundary.
- **Task 7:** Defined `CliError` enum with `Config(ConfigError)`, `TracingInit { reason }`, `Signal(io::Error)`. `From<ConfigError>` and `From<io::Error>` derived via `#[from]`.
- **Task 8:** 16 new tests total — 11 in `cli::tests` (parse, error display, error conversion, tracing init), 7 in `config::tests` (log_format/log_level validation, defaults, secrets validation). All 42 tests pass.
- **Task 9:** `cargo fmt -- --check` clean. `cargo clippy` clean (only pre-existing `dead_code` warnings from Story 1.1 stubs). Manual integration test: daemon starts with structured logs, SIGTERM triggers graceful shutdown. `--version` outputs `bmad-bot 0.1.0`.
- **Decision:** Added `BotConfig::_test_minimal()` helper (doc-hidden) to support CLI tracing tests without requiring full YAML parsing.

### Change Log

- 2026-02-08: Story 1.2 implementation complete — CLI framework, daemon lifecycle, structured tracing, graceful shutdown. All 42 tests pass.

### File List

- `src/cli/mod.rs` — Full implementation (Cli, Commands, CliError, init_tracing, run_start, run_polling_loop, 11 tests)
- `src/config/mod.rs` — Extended with `log_format`, `log_level` fields, validation, `_test_minimal()` helper, 7 new tests
- `src/main.rs` — Replaced with full CLI dispatch via clap
- `bmad-bot.yaml.example` — Added `log_format` and `log_level` fields