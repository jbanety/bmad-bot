//! CLI module — clap-based command-line interface for bmad-bot.
//!
//! Provides `init`, `start`, `status`, `logs` subcommands.
//! Contains the daemon lifecycle: config-driven tracing, polling loop,
//! and graceful shutdown via signal handling.

use std::path::Path;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::time::{Duration, sleep};
use tracing_subscriber::{EnvFilter, fmt};

use crate::config::{BotConfig, ConfigError};

// ---------------------------------------------------------------------------
// CLI structs (clap derive)
// ---------------------------------------------------------------------------

/// BMAD Bot — Autonomous AI developer daemon powered by the BMAD methodology.
///
/// Polls sprint-status.yaml for stories, orchestrates LLM agents to implement
/// them, and creates pull requests automatically.
#[derive(Parser, Debug)]
#[command(name = "bmad-bot", version, about, long_about = None)]
pub struct Cli {
    /// Path to the configuration file.
    #[arg(long, short, default_value = "bmad-bot.yaml", global = true)]
    pub config: std::path::PathBuf,

    /// Subcommand to execute.
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

// ---------------------------------------------------------------------------
// CliError
// ---------------------------------------------------------------------------

/// Errors originating from CLI operations.
///
/// Uses `thiserror` for structured, typed errors. `anyhow` conversion happens
/// only at the `main.rs` boundary.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// A configuration loading or validation error.
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    /// Failed to initialize the tracing subscriber.
    #[error("Failed to initialize tracing: {reason}")]
    TracingInit {
        /// Human-readable explanation of what went wrong.
        reason: String,
    },

    /// Failed to set up a signal handler.
    #[error("Signal handler error: {0}")]
    Signal(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Tracing setup
// ---------------------------------------------------------------------------

/// Initializes structured tracing based on config.
///
/// Uses `RUST_LOG` env var as override if set, otherwise `config.log_level`.
/// Supports two output formats:
/// - `"json"` — machine-parseable structured JSON on stdout
/// - `"pretty"` — human-readable coloured output (default)
pub fn init_tracing(config: &BotConfig) -> Result<(), CliError> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    // NOTE: Each branch calls try_init() independently — the JSON and pretty
    // builders produce different types (JsonFields vs DefaultFields) and cannot
    // be unified without boxing.
    let result = match config.log_format.as_str() {
        "json" => fmt()
            .json()
            .with_env_filter(env_filter)
            .with_target(true)
            .with_thread_ids(false)
            .try_init(),
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

// ---------------------------------------------------------------------------
// Start command handler
// ---------------------------------------------------------------------------

/// Runs the `start` command: load config, init tracing, enter polling loop.
///
/// This is the main daemon entry point. Config loading and validation happen
/// **before** tracing is initialized, so errors at that stage are surfaced via
/// `anyhow`'s Debug format on stderr (expected behaviour — see Dev Notes).
pub async fn run_start(config_path: &Path) -> Result<(), CliError> {
    let config = BotConfig::load(config_path)?;
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

// ---------------------------------------------------------------------------
// Polling loop with graceful shutdown
// ---------------------------------------------------------------------------

/// Placeholder polling loop. Sleeps for the configured interval, checks for
/// shutdown signals each cycle.
///
/// Story 2.1 replaces the sleep with sprint-status.yaml polling.
async fn run_polling_loop(config: &Arc<BotConfig>) -> Result<(), CliError> {
    let interval = Duration::from_secs(config.polling_interval_secs);

    // Set up SIGTERM handler (Unix only — acceptable for MVP targeting macOS/Linux)
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

    #[test]
    fn test_cli_error_display_config() {
        let err = CliError::Config(ConfigError::MissingField {
            field: "test".to_string(),
        });
        let msg = err.to_string();
        assert!(
            msg.contains("Configuration error"),
            "Expected 'Configuration error' in: {msg}"
        );
    }

    #[test]
    fn test_cli_error_display_tracing_init() {
        let err = CliError::TracingInit {
            reason: "already set".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("tracing"), "Expected 'tracing' in: {msg}");
        assert!(
            msg.contains("already set"),
            "Expected 'already set' in: {msg}"
        );
    }

    #[test]
    fn test_cli_error_display_signal() {
        let err = CliError::Signal(std::io::Error::new(
            std::io::ErrorKind::Other,
            "test signal error",
        ));
        let msg = err.to_string();
        assert!(
            msg.contains("Signal handler error"),
            "Expected 'Signal handler error' in: {msg}"
        );
    }

    #[test]
    fn test_cli_error_from_config_error() {
        let config_err = ConfigError::MissingField {
            field: "test_field".to_string(),
        };
        let cli_err: CliError = config_err.into();
        assert!(matches!(cli_err, CliError::Config(_)));
    }

    #[test]
    fn test_cli_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "test");
        let cli_err: CliError = io_err.into();
        assert!(matches!(cli_err, CliError::Signal(_)));
    }

    /// Tracing can only be initialized once per process. This test verifies
    /// that `init_tracing` does not panic for the pretty format. Because test
    /// ordering is non-deterministic and a global subscriber may already be
    /// set, we accept both Ok and Err (TracingInit) as valid outcomes.
    #[test]
    fn test_init_tracing_pretty_does_not_panic() {
        let config = BotConfig::_test_minimal("pretty", "info");
        let result = init_tracing(&config);
        // Either succeeds (first test to run) or fails gracefully (subscriber already set)
        match result {
            Ok(()) => {}                            // success
            Err(CliError::TracingInit { .. }) => {} // acceptable — already initialized
            Err(e) => panic!("Unexpected error: {e}"),
        }
    }

    /// Same as above but for JSON format.
    #[test]
    fn test_init_tracing_json_does_not_panic() {
        let config = BotConfig::_test_minimal("json", "debug");
        let result = init_tracing(&config);
        match result {
            Ok(()) => {}
            Err(CliError::TracingInit { .. }) => {}
            Err(e) => panic!("Unexpected error: {e}"),
        }
    }
}
