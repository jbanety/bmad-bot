//! CLI module — clap-based command-line interface for bmad-bot.
//!
//! Provides `init`, `start`, `status`, `logs` subcommands.
//! Contains the daemon lifecycle: config-driven tracing, polling loop,
//! and graceful shutdown via signal handling.
//! Also provides the interactive `init` wizard for first-time setup.

use std::path::Path;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::time::{Duration, sleep};
use tracing_subscriber::{EnvFilter, fmt};

use crate::config::{
    BmadPathsConfig, BotConfig, ConfigError, GitProviderConfig, LlmConfig, LlmRoleConfig,
    NotificationConfig, TelegramConfig,
};

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
    Signal(std::io::Error),

    /// Init command specific failure.
    #[error("Init failed: {reason}")]
    Init {
        /// Human-readable explanation of what went wrong.
        reason: String,
    },

    /// Generic I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// User cancelled an interactive operation.
    #[error("User cancelled operation")]
    UserCancelled,
}

// ---------------------------------------------------------------------------
// Constants for interactive prompts
// ---------------------------------------------------------------------------

/// Recognised LLM provider identifiers for interactive selection.
const LLM_PROVIDERS: &[&str] = &["anthropic", "openai", "github-models"];

/// Recognised git provider identifiers for interactive selection.
const GIT_PROVIDERS: &[&str] = &["github", "gitlab"];

/// Recognised log output formats.
const LOG_FORMATS: &[&str] = &["pretty", "json"];

/// Recognised log levels.
const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

/// Returns the default model suggestion for a given LLM provider.
fn default_model_for_provider(provider: &str) -> &str {
    match provider {
        "anthropic" => "claude-sonnet-4-20250514",
        "openai" => "gpt-4o",
        "github-models" => "gpt-4o",
        _ => "",
    }
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
// Init command handler
// ---------------------------------------------------------------------------

/// Runs the `init` command: interactive config generation.
///
/// Assumes tracing is already initialized by main.rs (global subscriber).
/// Guides the user through interactive prompts to generate `bmad-bot.yaml`
/// and `.env` files.
pub async fn run_init(config_path: &Path) -> Result<(), CliError> {
    tracing::info!("Starting BMAD Bot interactive setup");

    // Check for existing config
    if config_path.exists() {
        let overwrite = dialoguer::Confirm::new()
            .with_prompt(format!(
                "\u{26a0}\u{fe0f}  {} already exists. Overwrite?",
                config_path.display()
            ))
            .default(false)
            .interact()
            .map_err(|e| CliError::Init {
                reason: e.to_string(),
            })?;

        if !overwrite {
            tracing::info!("Init cancelled — existing config preserved");
            return Ok(());
        }
    }

    // Collect config interactively
    let config = collect_config_interactively()?;

    // Validate the generated config
    config.validate().map_err(CliError::Config)?;

    // Generate and write bmad-bot.yaml
    let yaml_content = generate_config_yaml(&config)?;
    tokio::fs::write(config_path, &yaml_content).await?;
    tracing::info!(path = %config_path.display(), "Generated bmad-bot.yaml");

    // Generate and write .env
    let env_path = Path::new(".env");
    if env_path.exists() {
        let overwrite_env = dialoguer::Confirm::new()
            .with_prompt("\u{26a0}\u{fe0f}  .env already exists. Overwrite?")
            .default(false)
            .interact()
            .map_err(|e| CliError::Init {
                reason: e.to_string(),
            })?;

        if !overwrite_env {
            tracing::info!(".env preserved — skipping secrets file generation");
            println!(
                "\n\u{2705} Configuration written to {}",
                config_path.display()
            );
            println!("\u{23ed}\u{fe0f}  .env was NOT overwritten — update it manually if needed");
            return Ok(());
        }
    }

    let env_content = generate_env_file(&config)?;
    tokio::fs::write(env_path, &env_content).await?;
    tracing::info!("Generated .env");

    println!("\n\u{2705} Setup complete!");
    println!("   Config: {}", config_path.display());
    println!("   Secrets: .env");
    println!("\n\u{1f4dd} Next steps:");
    println!("   1. Edit .env and fill in your API keys");
    println!("   2. Run `bmad-bot start` to launch the daemon");

    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive prompt collection
// ---------------------------------------------------------------------------

/// Collects all configuration values interactively from the user.
///
/// Uses `dialoguer` for Select, Input, and Confirm prompts.
/// Returns a fully populated [`BotConfig`] ready for serialization.
fn collect_config_interactively() -> Result<BotConfig, CliError> {
    println!("\n\u{1f3d7}\u{fe0f}  BMAD Bot — Interactive Setup\n");

    // --- Git Provider ---
    println!("\u{2500}\u{2500} Git Provider \u{2500}\u{2500}");
    let git_idx = dialoguer::Select::new()
        .with_prompt("Git hosting provider")
        .items(GIT_PROVIDERS)
        .default(0)
        .interact()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;
    let git_provider_name = GIT_PROVIDERS[git_idx].to_string();

    let repo_owner: String = dialoguer::Input::new()
        .with_prompt("Repository owner (org or user)")
        .interact_text()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    let repo_name: String = dialoguer::Input::new()
        .with_prompt("Repository name")
        .interact_text()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    let target_branch: String = dialoguer::Input::new()
        .with_prompt("Target branch for PRs")
        .default("main".to_string())
        .interact_text()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    // --- LLM Providers ---
    println!("\n\u{2500}\u{2500} LLM Configuration \u{2500}\u{2500}");
    let dev_provider_idx = dialoguer::Select::new()
        .with_prompt("LLM provider for DEV agent")
        .items(LLM_PROVIDERS)
        .default(0)
        .interact()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;
    let dev_provider = LLM_PROVIDERS[dev_provider_idx].to_string();

    let dev_model: String = dialoguer::Input::new()
        .with_prompt("Model for DEV agent")
        .default(default_model_for_provider(&dev_provider).to_string())
        .interact_text()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    let same_for_all = dialoguer::Confirm::new()
        .with_prompt("Use same provider/model for REVIEW and SUPERVISOR roles?")
        .default(true)
        .interact()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    let (review_provider, review_model, supervisor_provider, supervisor_model) = if same_for_all {
        (
            dev_provider.clone(),
            dev_model.clone(),
            dev_provider.clone(),
            dev_model.clone(),
        )
    } else {
        let rp_idx = dialoguer::Select::new()
            .with_prompt("LLM provider for REVIEW agent")
            .items(LLM_PROVIDERS)
            .default(dev_provider_idx)
            .interact()
            .map_err(|e| CliError::Init {
                reason: e.to_string(),
            })?;
        let rp = LLM_PROVIDERS[rp_idx].to_string();
        let rm: String = dialoguer::Input::new()
            .with_prompt("Model for REVIEW agent")
            .default(default_model_for_provider(&rp).to_string())
            .interact_text()
            .map_err(|e| CliError::Init {
                reason: e.to_string(),
            })?;

        let sp_idx = dialoguer::Select::new()
            .with_prompt("LLM provider for SUPERVISOR agent")
            .items(LLM_PROVIDERS)
            .default(dev_provider_idx)
            .interact()
            .map_err(|e| CliError::Init {
                reason: e.to_string(),
            })?;
        let sp = LLM_PROVIDERS[sp_idx].to_string();
        let sm: String = dialoguer::Input::new()
            .with_prompt("Model for SUPERVISOR agent")
            .default(default_model_for_provider(&sp).to_string())
            .interact_text()
            .map_err(|e| CliError::Init {
                reason: e.to_string(),
            })?;

        (rp, rm, sp, sm)
    };

    // --- Notifications ---
    println!("\n\u{2500}\u{2500} Notifications \u{2500}\u{2500}");
    let telegram_enabled = dialoguer::Confirm::new()
        .with_prompt("Enable Telegram notifications?")
        .default(false)
        .interact()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    let telegram_chat_id = if telegram_enabled {
        dialoguer::Input::new()
            .with_prompt("Telegram chat ID")
            .interact_text()
            .map_err(|e| CliError::Init {
                reason: e.to_string(),
            })?
    } else {
        String::new()
    };

    // --- Daemon Settings ---
    println!("\n\u{2500}\u{2500} Daemon Settings \u{2500}\u{2500}");
    // NOTE: Use .interact() NOT .interact_text() for non-String types.
    // .interact_text() always returns String. .interact() respects the generic T: FromStr.
    let polling_interval_secs: u64 = dialoguer::Input::new()
        .with_prompt("Polling interval (seconds)")
        .default(300u64)
        .interact()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    let project_root: String = dialoguer::Input::new()
        .with_prompt("BMAD project root path")
        .default(".".to_string())
        .interact_text()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    let log_format_idx = dialoguer::Select::new()
        .with_prompt("Log format")
        .items(LOG_FORMATS)
        .default(0) // "pretty"
        .interact()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    let log_level_idx = dialoguer::Select::new()
        .with_prompt("Log level")
        .items(LOG_LEVELS)
        .default(2) // "info"
        .interact()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    // Derive BMAD paths from project root
    let output_folder = format!("{project_root}/_bmad-output");
    let planning_artifacts = format!("{output_folder}/planning-artifacts");
    let implementation_artifacts = format!("{output_folder}/implementation-artifacts");

    // Build BotConfig
    Ok(BotConfig {
        polling_interval_secs,
        git_provider: GitProviderConfig {
            provider: git_provider_name,
            repo_owner,
            repo_name,
            target_branch,
        },
        llm: LlmConfig {
            dev: LlmRoleConfig {
                provider: dev_provider,
                model: dev_model,
            },
            review: LlmRoleConfig {
                provider: review_provider,
                model: review_model,
            },
            supervisor: LlmRoleConfig {
                provider: supervisor_provider,
                model: supervisor_model,
            },
        },
        notifications: NotificationConfig {
            telegram: TelegramConfig {
                enabled: telegram_enabled,
                chat_id: telegram_chat_id,
            },
        },
        bmad_paths: BmadPathsConfig {
            project_root,
            output_folder,
            planning_artifacts,
            implementation_artifacts,
        },
        log_format: LOG_FORMATS[log_format_idx].to_string(),
        log_level: LOG_LEVELS[log_level_idx].to_string(),
    })
}

// ---------------------------------------------------------------------------
// Config YAML generation
// ---------------------------------------------------------------------------

/// Generates a YAML config string with header comments.
///
/// The generated YAML is valid and can be deserialized back into [`BotConfig`].
/// A header comment block includes the generation timestamp and a note about
/// secrets being in `.env`.
fn generate_config_yaml(config: &BotConfig) -> Result<String, CliError> {
    let yaml_body = serde_yaml::to_string(config).map_err(|e| CliError::Init {
        reason: format!("Failed to serialize config to YAML: {e}"),
    })?;

    let header = format!(
        "# BMAD Bot Configuration\n\
         # Generated by `bmad-bot init` on {}\n\
         # Secrets (API keys, tokens) are in .env — never in this file.\n\
         #\n\
         # Reference: bmad-bot.yaml.example for field descriptions.\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    );

    Ok(format!("{header}{yaml_body}"))
}

// ---------------------------------------------------------------------------
// .env file generation
// ---------------------------------------------------------------------------

/// Generates a `.env` file with context-aware placeholders.
///
/// Only includes secrets relevant to the chosen providers. Each secret line
/// has a dynamic comment specifying which roles use that provider.
fn generate_env_file(config: &BotConfig) -> Result<String, CliError> {
    let mut lines = vec![
        "# BMAD Bot Secrets".to_string(),
        "# Generated by `bmad-bot init`".to_string(),
        "# Fill in your API keys below. NEVER commit this file!".to_string(),
        String::new(),
    ];

    // Build a map of provider → list of roles that use it
    let mut provider_roles: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();
    for (role, role_config) in [
        ("dev", &config.llm.dev),
        ("review", &config.llm.review),
        ("supervisor", &config.llm.supervisor),
    ] {
        provider_roles
            .entry(role_config.provider.as_str())
            .or_default()
            .push(role);
    }

    // LLM provider keys — with dynamic role comments
    lines.push("# --- LLM Provider API Keys ---".to_string());
    if let Some(roles) = provider_roles.get("anthropic") {
        lines.push(format!("# Required: used by {} role(s)", roles.join(", ")));
        lines.push("ANTHROPIC_API_KEY=".to_string());
    }
    if let Some(roles) = provider_roles.get("openai") {
        lines.push(format!("# Required: used by {} role(s)", roles.join(", ")));
        lines.push("OPENAI_API_KEY=".to_string());
    }
    if let Some(roles) = provider_roles.get("github-models") {
        lines.push(format!("# Required: used by {} role(s)", roles.join(", ")));
        lines.push("GITHUB_MODELS_API_KEY=".to_string());
    }

    lines.push(String::new());

    // Git provider token
    lines.push("# --- Git Provider Token ---".to_string());
    match config.git_provider.provider.as_str() {
        "github" => {
            lines.push("# Required: GitHub personal access token for PR creation".to_string());
            lines.push("GITHUB_TOKEN=".to_string());
        }
        "gitlab" => {
            lines.push("# Required: GitLab personal access token for MR creation".to_string());
            lines.push("GITLAB_TOKEN=".to_string());
        }
        _ => {}
    }

    lines.push(String::new());

    // Telegram (only if enabled)
    if config.notifications.telegram.enabled {
        lines.push("# --- Notifications ---".to_string());
        lines.push("# Required: Telegram bot token (notifications enabled)".to_string());
        lines.push("TELEGRAM_BOT_TOKEN=".to_string());
        lines.push(String::new());
    }

    Ok(lines.join("\n"))
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
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(CliError::Signal)?;

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

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn make_test_config() -> BotConfig {
        BotConfig {
            polling_interval_secs: 300,
            git_provider: GitProviderConfig {
                provider: "github".to_string(),
                repo_owner: "test-org".to_string(),
                repo_name: "test-repo".to_string(),
                target_branch: "main".to_string(),
            },
            llm: LlmConfig {
                dev: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "claude-sonnet-4-20250514".to_string(),
                },
                review: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "claude-sonnet-4-20250514".to_string(),
                },
                supervisor: LlmRoleConfig {
                    provider: "openai".to_string(),
                    model: "gpt-4o".to_string(),
                },
            },
            notifications: NotificationConfig {
                telegram: TelegramConfig {
                    enabled: false,
                    chat_id: String::new(),
                },
            },
            bmad_paths: BmadPathsConfig {
                project_root: ".".to_string(),
                output_folder: "./_bmad-output".to_string(),
                planning_artifacts: "./_bmad-output/planning-artifacts".to_string(),
                implementation_artifacts: "./_bmad-output/implementation-artifacts".to_string(),
            },
            log_format: "pretty".to_string(),
            log_level: "info".to_string(),
        }
    }

    // -----------------------------------------------------------------------
    // CLI parsing tests (Story 1.2)
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // CliError display / conversion tests (Story 1.2 + 1.3)
    // -----------------------------------------------------------------------

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
    fn test_cli_error_display_init() {
        let err = CliError::Init {
            reason: "prompt failed".to_string(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("Init failed"),
            "Expected 'Init failed' in: {msg}"
        );
        assert!(
            msg.contains("prompt failed"),
            "Expected 'prompt failed' in: {msg}"
        );
    }

    #[test]
    fn test_cli_error_display_io() {
        let err = CliError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        let msg = err.to_string();
        assert!(msg.contains("I/O error"), "Expected 'I/O error' in: {msg}");
    }

    #[test]
    fn test_cli_error_display_user_cancelled() {
        let err = CliError::UserCancelled;
        let msg = err.to_string();
        assert!(
            msg.contains("User cancelled"),
            "Expected 'User cancelled' in: {msg}"
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
        assert!(matches!(cli_err, CliError::Io(_)));
    }

    // -----------------------------------------------------------------------
    // Tracing tests (Story 1.2)
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // generate_config_yaml tests (Story 1.3)
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_config_yaml_roundtrips() {
        let config = make_test_config();
        let yaml = generate_config_yaml(&config).unwrap();
        // Strip comment lines (start with #) before deserializing
        let yaml_body: String = yaml
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let parsed: BotConfig = serde_yaml::from_str(&yaml_body).unwrap();
        assert_eq!(parsed.polling_interval_secs, 300);
        assert_eq!(parsed.git_provider.provider, "github");
        assert_eq!(parsed.llm.dev.provider, "anthropic");
    }

    #[test]
    fn test_generate_config_yaml_validates() {
        let config = make_test_config();
        let yaml = generate_config_yaml(&config).unwrap();
        let yaml_body: String = yaml
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        let parsed: BotConfig = serde_yaml::from_str(&yaml_body).unwrap();
        assert!(parsed.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // generate_env_file tests (Story 1.3)
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_env_includes_anthropic_key() {
        let config = make_test_config();
        let env = generate_env_file(&config).unwrap();
        assert!(env.contains("ANTHROPIC_API_KEY="));
    }

    #[test]
    fn test_generate_env_includes_openai_key_for_supervisor() {
        let config = make_test_config(); // supervisor uses openai
        let env = generate_env_file(&config).unwrap();
        assert!(env.contains("OPENAI_API_KEY="));
    }

    #[test]
    fn test_generate_env_excludes_github_models_key() {
        let config = make_test_config(); // no role uses github-models
        let env = generate_env_file(&config).unwrap();
        assert!(!env.contains("GITHUB_MODELS_API_KEY="));
    }

    #[test]
    fn test_generate_env_includes_github_token() {
        let config = make_test_config(); // git_provider is github
        let env = generate_env_file(&config).unwrap();
        assert!(env.contains("GITHUB_TOKEN="));
    }

    #[test]
    fn test_generate_env_excludes_gitlab_token() {
        let config = make_test_config(); // git_provider is github, not gitlab
        let env = generate_env_file(&config).unwrap();
        assert!(!env.contains("GITLAB_TOKEN="));
    }

    #[test]
    fn test_generate_env_excludes_telegram_when_disabled() {
        let config = make_test_config(); // telegram.enabled = false
        let env = generate_env_file(&config).unwrap();
        assert!(!env.contains("TELEGRAM_BOT_TOKEN="));
    }

    #[test]
    fn test_generate_env_includes_telegram_when_enabled() {
        let mut config = make_test_config();
        config.notifications.telegram.enabled = true;
        config.notifications.telegram.chat_id = "12345".to_string();
        let env = generate_env_file(&config).unwrap();
        assert!(env.contains("TELEGRAM_BOT_TOKEN="));
    }

    #[test]
    fn test_derived_bmad_paths_from_project_root() {
        // Verify the path derivation logic
        let root = "/my/project";
        let output = format!("{root}/_bmad-output");
        let planning = format!("{output}/planning-artifacts");
        let implementation = format!("{output}/implementation-artifacts");
        assert_eq!(planning, "/my/project/_bmad-output/planning-artifacts");
        assert_eq!(
            implementation,
            "/my/project/_bmad-output/implementation-artifacts"
        );
    }

    #[test]
    fn test_generate_env_comments_specify_correct_roles() {
        let config = make_test_config(); // dev+review=anthropic, supervisor=openai
        let env = generate_env_file(&config).unwrap();
        // Anthropic used by dev and review
        assert!(
            env.contains("dev, review") || env.contains("review, dev"),
            "Expected roles for anthropic in env comments, got:\n{env}"
        );
        // OpenAI used by supervisor
        assert!(
            env.contains("supervisor role"),
            "Expected 'supervisor role' in env comments, got:\n{env}"
        );
    }

    #[test]
    fn test_generate_env_gitlab_provider() {
        let mut config = make_test_config();
        config.git_provider.provider = "gitlab".to_string();
        let env = generate_env_file(&config).unwrap();
        assert!(env.contains("GITLAB_TOKEN="));
        assert!(!env.contains("GITHUB_TOKEN="));
    }

    #[test]
    fn test_generate_config_yaml_contains_header_comment() {
        let config = make_test_config();
        let yaml = generate_config_yaml(&config).unwrap();
        assert!(yaml.starts_with("# BMAD Bot Configuration"));
        assert!(yaml.contains("Generated by `bmad-bot init`"));
        assert!(yaml.contains("Secrets"));
    }

    #[test]
    fn test_generate_env_all_roles_same_provider() {
        // All three roles use anthropic — should only have ONE ANTHROPIC_API_KEY line
        let mut config = make_test_config();
        config.llm.supervisor = LlmRoleConfig {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
        };
        let env = generate_env_file(&config).unwrap();
        let count = env.matches("ANTHROPIC_API_KEY=").count();
        assert_eq!(count, 1, "Expected exactly one ANTHROPIC_API_KEY line");
        assert!(!env.contains("OPENAI_API_KEY="));
    }

    // -----------------------------------------------------------------------
    // default_model_for_provider tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_default_model_for_provider_anthropic() {
        assert_eq!(
            default_model_for_provider("anthropic"),
            "claude-sonnet-4-20250514"
        );
    }

    #[test]
    fn test_default_model_for_provider_openai() {
        assert_eq!(default_model_for_provider("openai"), "gpt-4o");
    }

    #[test]
    fn test_default_model_for_provider_github_models() {
        assert_eq!(default_model_for_provider("github-models"), "gpt-4o");
    }

    #[test]
    fn test_default_model_for_provider_unknown() {
        assert_eq!(default_model_for_provider("unknown"), "");
    }
}
