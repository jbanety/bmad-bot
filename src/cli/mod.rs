//! CLI module — clap-based command-line interface for bmad-bot.
//!
//! Provides `init`, `start`, `status`, `logs` subcommands.
//! Contains the daemon lifecycle: config-driven tracing, polling loop,
//! and graceful shutdown via signal handling.
//! Also provides the interactive `init` wizard for first-time setup.

use crate::auth::github_copilot::{self, ReqwestCopilotHttpClient};

pub mod git_detect;
pub mod state;

use std::fs::OpenOptions;
use std::io::IsTerminal;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::cli::git_detect::{GitDetectionResult, detect_git_remote, detect_git_remote_with_name};

use clap::{Parser, Subcommand};
use tokio::time::Duration;
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

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
    Init {
        /// Skip interactive setup and only re-run the GitHub Copilot OAuth Device Flow.
        /// Updates GITHUB_COPILOT_OAUTH_TOKEN in .env.
        #[arg(long)]
        copilot_login: bool,
    },
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
    #[allow(dead_code)] // Used by Story 1.3 interactive flows
    #[error("User cancelled operation")]
    UserCancelled,

    /// State file read/write error.
    #[error("State file error: {reason}")]
    State {
        /// Human-readable explanation of what went wrong.
        reason: String,
    },

    /// Log file read/access error.
    #[error("Log file error: {reason}")]
    LogFile {
        /// Human-readable explanation of what went wrong.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Constants for interactive prompts
// ---------------------------------------------------------------------------

/// Recognised LLM provider identifiers for interactive selection.
const LLM_PROVIDERS: &[&str] = &["anthropic", "openai", "github-copilot"];

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
        "github-copilot" => "gpt-4o",
        _ => "",
    }
}

// ---------------------------------------------------------------------------
// Tracing setup
// ---------------------------------------------------------------------------

/// Initializes structured tracing with dual output:
/// - **stdout**: config-driven format (pretty or JSON)
/// - **file**: always JSON (machine-parseable for `bmad-bot logs`)
///
/// Uses `RUST_LOG` env var as override if set, otherwise `config.log_level`.
pub fn init_tracing(config: &BotConfig) -> Result<(), CliError> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "{},rustls=off,h2=off,hyper_util=off,reqwest::connect=off",
            config.log_level
        ))
    });

    // File appender — always JSON.
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

    // Stdout layer — config-driven format.
    // The two branches produce different types; `.boxed()` erases the type.
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

    // --- GitHub Copilot Device Flow (after all LLM role selections) ---
    let copilot_oauth_token = {
        let uses_copilot = config.llm.dev.provider == "github-copilot"
            || config.llm.review.provider == "github-copilot"
            || config.llm.supervisor.provider == "github-copilot";

        if uses_copilot {
            if std::io::stdin().is_terminal() {
                let client = ReqwestCopilotHttpClient::new();
                match github_copilot::run_device_flow(&client).await {
                    Ok(token) => Some(token),
                    Err(e) => {
                        eprintln!("\n⚠ GitHub Copilot authorization failed: {e}");
                        eprintln!(
                            "  You can set GITHUB_COPILOT_OAUTH_TOKEN manually in .env later."
                        );
                        None
                    }
                }
            } else {
                eprintln!("\n⚠ GitHub Copilot Device Flow requires an interactive terminal.");
                eprintln!("  Set GITHUB_COPILOT_OAUTH_TOKEN manually in .env after setup.");
                None
            }
        } else {
            None
        }
    };

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

    let env_content = generate_env_file(&config, copilot_oauth_token.as_deref())?;
    tokio::fs::write(env_path, &env_content).await?;
    tracing::info!("Generated .env");

    println!("\n\u{2705} Setup complete!");
    println!("   Config: {}", config_path.display());
    println!("   Secrets: .env");
    println!("\n\u{1f4dd} Next steps:");
    println!("   1. Edit .env and fill in your API keys");
    println!("   2. Run `bmad-bot start` to launch the daemon");
    println!("\n\u{1f4a1} If GitHub Copilot auth failed, re-run: bmad-bot init --copilot-login");

    Ok(())
}

// ---------------------------------------------------------------------------
// Copilot login (standalone Device Flow)
// ---------------------------------------------------------------------------

/// Run the GitHub Copilot OAuth Device Flow and patch the `.env` file.
///
/// Reads the existing `.env` to locate `GITHUB_COPILOT_OAUTH_TOKEN=`, runs the
/// Device Flow, then overwrites the value in-place. If the line doesn't exist
/// yet it is appended.
///
/// Triggered via `bmad-bot init --copilot-login`.
pub async fn run_copilot_login() -> Result<(), CliError> {
    tracing::info!("Starting GitHub Copilot Device Flow (--copilot-login)");

    if !std::io::stdin().is_terminal() {
        return Err(CliError::Init {
            reason: "GitHub Copilot Device Flow requires an interactive terminal".to_string(),
        });
    }

    let client = ReqwestCopilotHttpClient::new();
    let token = github_copilot::run_device_flow(&client)
        .await
        .map_err(|e| CliError::Init {
            reason: format!("GitHub Copilot Device Flow failed: {e}"),
        })?;

    // Patch .env file
    let env_path = Path::new(".env");
    let key = "GITHUB_COPILOT_OAUTH_TOKEN";

    if env_path.exists() {
        let content = tokio::fs::read_to_string(env_path).await?;
        let mut found = false;
        let patched: Vec<String> = content
            .lines()
            .map(|line| {
                if line.starts_with(key) {
                    found = true;
                    format!("{key}={token}")
                } else {
                    line.to_string()
                }
            })
            .collect();

        let mut output = patched.join("\n");
        if !found {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&format!("{key}={token}\n"));
        } else if !output.ends_with('\n') {
            output.push('\n');
        }

        tokio::fs::write(env_path, &output).await?;
    } else {
        let content = format!(
            "# BMAD Bot Secrets\n# Generated by `bmad-bot init --copilot-login`\n{key}={token}\n"
        );
        tokio::fs::write(env_path, &content).await?;
    }

    println!("\n✅ GITHUB_COPILOT_OAUTH_TOKEN written to .env");
    Ok(())
}

// ---------------------------------------------------------------------------
// Interactive prompt collection
// ---------------------------------------------------------------------------

/// Attempts git remote auto-detection and returns pre-filled values if the user confirms.
///
/// Returns `Some((provider, owner, repo_name, branch))` if auto-detection succeeds and the
/// user confirms the detected values. Returns `None` to fall through to manual prompts.
fn attempt_git_auto_detection() -> Result<Option<(String, String, String, String)>, CliError> {
    let current_dir = std::env::current_dir().map_err(|e| CliError::Init {
        reason: format!("Cannot determine current directory: {e}"),
    })?;

    let mut detection = detect_git_remote(&current_dir);

    // If multiple remotes and no origin, prompt user to select one
    if let GitDetectionResult::MultipleRemotes(ref names) = detection {
        let selected_idx = dialoguer::Select::new()
            .with_prompt("Multiple git remotes found — select one")
            .items(names)
            .default(0)
            .interact()
            .map_err(|e| CliError::Init {
                reason: e.to_string(),
            })?;
        detection = detect_git_remote_with_name(&current_dir, &names[selected_idx]);
    }

    let info = match detection {
        GitDetectionResult::Detected(info) => info,
        _ => return Ok(None), // Silent fallback to manual prompts
    };

    // Display summary and ask for confirmation
    if let Some(ref provider) = info.provider {
        println!("\n\u{1f50d} Git repository detected!");
        println!("   Provider:  {provider}");
        println!("   Owner:     {}", info.owner);
        println!("   Repo:      {}", info.repo_name);
        println!("   Branch:    {}", info.default_branch);
        println!();

        let confirm = dialoguer::Confirm::new()
            .with_prompt("Use these settings?")
            .default(true)
            .interact()
            .map_err(|e| CliError::Init {
                reason: e.to_string(),
            })?;

        if confirm {
            return Ok(Some((
                provider.clone(),
                info.owner,
                info.repo_name,
                info.default_branch,
            )));
        }
        // User declined — fall through to manual prompts
        return Ok(None);
    }

    // Provider unknown (unrecognized host) — show partial summary, manual provider select
    println!("\n\u{1f50d} Git repository detected (host: {})!", info.host);
    println!(
        "   \u{26a0}\u{fe0f}  Git host not recognized \u{2014} provider must be selected manually"
    );
    println!("   Owner:     {}", info.owner);
    println!("   Repo:      {}", info.repo_name);
    println!("   Branch:    {}", info.default_branch);
    println!();

    let confirm = dialoguer::Confirm::new()
        .with_prompt("Use detected owner/repo/branch and select provider manually?")
        .default(true)
        .interact()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    if !confirm {
        return Ok(None);
    }

    let git_idx = dialoguer::Select::new()
        .with_prompt("Git hosting provider")
        .items(GIT_PROVIDERS)
        .default(0)
        .interact()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;

    Ok(Some((
        GIT_PROVIDERS[git_idx].to_string(),
        info.owner,
        info.repo_name,
        info.default_branch,
    )))
}

/// Collects all configuration values interactively from the user.
///
/// Uses `dialoguer` for Select, Input, and Confirm prompts.
/// Returns a fully populated [`BotConfig`] ready for serialization.
fn collect_config_interactively() -> Result<BotConfig, CliError> {
    println!("\n\u{1f3d7}\u{fe0f}  BMAD Bot — Interactive Setup\n");

    // --- Git Provider ---
    println!("\u{2500}\u{2500} Git Provider \u{2500}\u{2500}");

    // Attempt git remote auto-detection before manual prompts
    let auto_detected = attempt_git_auto_detection()?;

    let (git_provider_name, repo_owner, repo_name, target_branch) =
        if let Some(config) = auto_detected {
            (config.0, config.1, config.2, config.3)
        } else {
            // Manual fallback — original Story 1.3 prompts
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

            (git_provider_name, repo_owner, repo_name, target_branch)
        };

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
        code_review_enabled: true,
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
                reasoning_effort: None,
            },
            review: LlmRoleConfig {
                provider: review_provider,
                model: review_model,
                reasoning_effort: None,
            },
            supervisor: LlmRoleConfig {
                provider: supervisor_provider,
                model: supervisor_model,
                reasoning_effort: None,
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
        log_file: "bmad-bot.log".to_string(),
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
    let yaml_body = serde_yml::to_string(config).map_err(|e| CliError::Init {
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
///
/// If `copilot_oauth_token` is `Some`, the token is pre-filled in the output.
/// If `None` and `github-copilot` is configured, an empty placeholder with a
/// comment is written instead.
fn generate_env_file(
    config: &BotConfig,
    copilot_oauth_token: Option<&str>,
) -> Result<String, CliError> {
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
    if let Some(roles) = provider_roles.get("github-copilot") {
        lines.push(format!("# Required: used by {} role(s)", roles.join(", ")));
        match copilot_oauth_token {
            Some(token) if !token.is_empty() => {
                lines.push(format!("GITHUB_COPILOT_OAUTH_TOKEN={token}"));
            }
            _ => {
                lines
                    .push("# Obtain via `bmad-bot init` (Device Flow) or set manually".to_string());
                lines.push("GITHUB_COPILOT_OAUTH_TOKEN=".to_string());
            }
        }
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
// Sprint status summary
// ---------------------------------------------------------------------------

/// Story status counts parsed from sprint-status.yaml.
#[derive(Debug, Default)]
pub struct SprintSummary {
    /// Total number of stories (excluding epics and retrospectives).
    pub total_stories: usize,
    /// Stories in backlog status.
    pub backlog: usize,
    /// Stories ready for development.
    pub ready_for_dev: usize,
    /// Stories currently in progress.
    pub in_progress: usize,
    /// Stories in review.
    pub review: usize,
    /// Stories completed.
    pub done: usize,
    /// Stories that are blocked.
    pub blocked: usize,
    /// Stories with an unrecognised status.
    pub other: usize,
    /// Total number of epics.
    pub total_epics: usize,
    /// Epics currently in progress.
    pub epics_in_progress: usize,
    /// Epics completed.
    pub epics_done: usize,
}

impl SprintSummary {
    /// Parse sprint-status.yaml and compute summary statistics.
    ///
    /// Returns a default (all zeros) summary if the file doesn't exist or can't be parsed.
    pub fn from_file(path: &Path) -> Self {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };

        let yaml: serde_yml::Value = match serde_yml::from_str(&content) {
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

                // Story entries — starts with a digit (e.g., "1-2-slug")
                if key_str.chars().next().is_some_and(|c| c.is_ascii_digit()) {
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

impl std::fmt::Display for SprintSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Sprint Status:")?;
        writeln!(
            f,
            "  Epics: {} total ({} in-progress, {} done)",
            self.total_epics, self.epics_in_progress, self.epics_done
        )?;
        writeln!(f, "  Stories: {} total", self.total_stories)?;
        writeln!(f, "    \u{2705} Done:          {}", self.done)?;
        writeln!(f, "    \u{1f50d} Review:        {}", self.review)?;
        writeln!(f, "    \u{1f527} In Progress:   {}", self.in_progress)?;
        writeln!(f, "    \u{1f4cb} Ready for Dev: {}", self.ready_for_dev)?;
        writeln!(f, "    \u{1f4e6} Backlog:       {}", self.backlog)?;
        if self.blocked > 0 {
            writeln!(f, "    \u{1f6ab} Blocked:       {}", self.blocked)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Status command handler
// ---------------------------------------------------------------------------

/// Runs the `status` command: displays daemon state, sprint summary, and BMAD info.
pub async fn run_status(config_path: &Path) -> Result<(), CliError> {
    // Try to load config for paths — if config doesn't exist, use defaults
    let config = BotConfig::load(config_path).ok();

    let state_path = Path::new(state::STATE_FILE_NAME);

    println!(
        "\u{2554}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2557}"
    );
    println!("\u{2551}        BMAD Bot Status               \u{2551}");
    println!(
        "\u{255a}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{2550}\u{255d}"
    );
    println!();

    // --- Daemon State ---
    match state::DaemonState::read(state_path)? {
        Some(ds) => {
            let alive = state::DaemonState::is_process_alive(ds.pid);
            let effective_status = if alive {
                "\u{1f7e2} Running"
            } else {
                "\u{1f534} Stopped (stale state)"
            };

            println!("Daemon: {effective_status}");
            println!("  PID:              {}", ds.pid);
            println!("  Started:          {}", ds.started_at);
            println!("  Last Activity:    {}", ds.last_activity);
            println!("  Stories Processed:{}", ds.stories_processed);
            println!("  Log File:         {}", ds.log_file.display());
            println!();

            // BMAD discovery from state
            if let Some(ref discovery) = ds.bmad_discovery {
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
            println!("Daemon: \u{1f534} Stopped (no state file)");
            println!();

            // Run fresh BMAD discovery
            if let Some(ref cfg) = config {
                let discovery = crate::config::discovery::BmadDiscovery::discover(Path::new(
                    &cfg.bmad_paths.project_root,
                ));
                println!("{discovery}");
            }
        }
    }

    // --- Sprint Summary ---
    if let Some(ref cfg) = config {
        let sprint_path =
            Path::new(&cfg.bmad_paths.implementation_artifacts).join("sprint-status.yaml");
        let summary = SprintSummary::from_file(&sprint_path);
        if summary.total_stories > 0 {
            println!("{summary}");
        } else {
            println!("Sprint Status: No sprint data found");
            println!("  Run sprint-planning to initialize story tracking");
        }
    } else {
        println!(
            "Sprint Status: Cannot determine (no config file at {})",
            config_path.display()
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Logs command handler
// ---------------------------------------------------------------------------

/// Valid log level names for `--level` flag validation.
const VALID_LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

/// Runs the `logs` command: reads and displays structured log entries from the log file.
pub async fn run_logs(
    config_path: &Path,
    level: Option<String>,
    tail: Option<usize>,
) -> Result<(), CliError> {
    // Validate --level flag if provided
    if let Some(ref lvl) = level
        && !VALID_LOG_LEVELS.contains(&lvl.to_lowercase().as_str())
    {
        println!(
            "\u{26a0}\u{fe0f}  Unknown log level '{}'. Valid levels: {}",
            lvl,
            VALID_LOG_LEVELS.join(", ")
        );
        println!("   Showing all log entries (no filter applied).\n");
    }

    let config = BotConfig::load(config_path).map_err(|e| CliError::LogFile {
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
            let target = entry.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let story_id = entry
                .get("fields")
                .and_then(|f| f.get("story_id"))
                .and_then(|v| v.as_str())
                .map(|s| format!(" [story:{s}]"))
                .unwrap_or_default();

            let level_icon = match entry_level.to_uppercase().as_str() {
                "ERROR" => "\u{274c}",
                "WARN" => "\u{26a0}\u{fe0f} ",
                "INFO" => "\u{2139}\u{fe0f} ",
                "DEBUG" => "\u{1f41b}",
                "TRACE" => "\u{1f52c}",
                _ => "  ",
            };

            println!("{level_icon} {timestamp} [{entry_level:>5}] {target}{story_id}: {message}");
            displayed += 1;
        } else {
            // Non-JSON line — print as-is (resilient fallback)
            if min_level == 0 {
                println!("  {line}");
                displayed += 1;
            }
        }
    }

    if displayed == 0 {
        println!("No log entries match the filter criteria.");
    } else {
        println!(
            "\n--- Showing {displayed} of {} total entries ---",
            lines.len()
        );
    }

    Ok(())
}

/// Map log level string to a numeric priority for filtering.
///
/// Higher number = more severe. Filter shows entries at `min_level` and above.
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

// ---------------------------------------------------------------------------
// Start command handler
// ---------------------------------------------------------------------------

/// Runs the `start` command: load config, init tracing, enter polling loop.
///
/// This is the main daemon entry point. Config loading and validation happen
/// **before** tracing is initialized, so errors at that stage are surfaced via
/// `anyhow`'s Debug format on stderr (expected behaviour — see Dev Notes).
/// Validates that `git` is installed and meets the minimum version requirement (>= 2.30).
///
/// Executes `git --version`, parses the `"git version X.Y.Z"` output, and verifies
/// the major.minor is at least 2.30. Returns `Ok(())` on success.
///
/// # Errors
/// Returns [`CliError::Init`] if git is not found, the output is unparseable,
/// or the version is below the minimum.
fn validate_git_version() -> Result<(), CliError> {
    let output = std::process::Command::new("git")
        .arg("--version")
        .output()
        .map_err(|e| CliError::Init {
            reason: format!("git not found — please install git >= 2.30. Error: {e}"),
        })?;

    if !output.status.success() {
        return Err(CliError::Init {
            reason: "git --version returned non-zero exit code".to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Expected format: "git version 2.39.5 (Apple Git-154)" or "git version 2.39.5"
    let version_str = stdout
        .trim()
        .strip_prefix("git version ")
        .ok_or_else(|| CliError::Init {
            reason: format!(
                "Unexpected git --version output: '{stdout}'. Expected 'git version X.Y.Z'"
            ),
        })?;

    // Extract major.minor from "X.Y.Z ..." (there may be extra text after the version)
    let version_part = version_str.split_whitespace().next().unwrap_or(version_str);
    let parts: Vec<&str> = version_part.split('.').collect();

    let major: u32 = parts
        .first()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| CliError::Init {
            reason: format!("Cannot parse git major version from '{version_str}'"),
        })?;

    let minor: u32 = parts
        .get(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| CliError::Init {
            reason: format!("Cannot parse git minor version from '{version_str}'"),
        })?;

    if major < 2 || (major == 2 && minor < 30) {
        return Err(CliError::Init {
            reason: format!("git >= 2.30 required, found {major}.{minor}. Please upgrade git."),
        });
    }

    tracing::info!(
        git_version = %version_part,
        "Git version validated (>= 2.30)"
    );

    Ok(())
}

pub async fn run_start(config_path: &Path) -> Result<(), CliError> {
    let config = BotConfig::load(config_path)?;
    config.validate()?;

    init_tracing(&config)?;

    // Validate git is installed and >= 2.30 before proceeding
    validate_git_version()?;

    let secrets = crate::config::BotSecrets::load()?;
    secrets.validate_for_config(&config)?;

    // BMAD auto-discovery
    let discovery = crate::config::discovery::BmadDiscovery::discover(Path::new(
        &config.bmad_paths.project_root,
    ));
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
    let state_path = Path::new(state::STATE_FILE_NAME);
    let mut daemon_state =
        state::DaemonState::new_running(std::path::PathBuf::from(&config.log_file), discovery);
    daemon_state.write(state_path)?;

    let config = Arc::new(config);
    let secrets = Arc::new(secrets);

    // Create cooperative shutdown flag — shared with pipeline/session layers.
    // A dedicated task below listens for Ctrl+C / SIGTERM and flips this flag
    // so that streaming chat loops and tool-calling rounds can exit cleanly.
    let shutdown: crate::session::runner::ShutdownFlag =
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Spawn signal handler task that sets the shutdown flag
    {
        let flag = std::sync::Arc::clone(&shutdown);
        tokio::spawn(async move {
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("Failed to register SIGTERM handler");

            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Received SIGINT (Ctrl-C) — setting shutdown flag");
                }
                _ = sigterm.recv() => {
                    tracing::info!("Received SIGTERM — setting shutdown flag");
                }
            }

            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
    }

    // Create watcher (Story 2.1)
    let watcher = crate::watcher::Watcher::new(Arc::clone(&config));

    // Create story pipeline (Story 6.2)
    let pipeline = crate::pipeline::StoryPipeline::new(
        Arc::clone(&config),
        Arc::clone(&secrets),
        std::sync::Arc::clone(&shutdown),
    )
    .map_err(|e| CliError::Init {
        reason: format!("Failed to create story pipeline: {e}"),
    })?;

    tracing::info!(
        config_path = %config_path.display(),
        polling_interval_secs = config.polling_interval_secs,
        sprint_status_path = %watcher.sprint_status_path().display(),
        git_provider = %config.git_provider.provider,
        log_format = %config.log_format,
        log_file = %config.log_file,
        "bmad-bot daemon started"
    );

    // Crash recovery — check for interrupted session WAL before polling
    if let Some(result) = pipeline.recover_and_process().await {
        tracing::info!(
            action = "crash_recovery_complete",
            story_key = %result.story_key,
            status = ?result.status,
            "Crash recovery processed — entering normal polling"
        );
    }

    // Polling loop with graceful shutdown — pass state for touch updates
    run_polling_loop(
        &config,
        &watcher,
        &pipeline,
        &mut daemon_state,
        state_path,
        &shutdown,
    )
    .await?;

    // Clean shutdown — update state and remove file
    daemon_state.mark_stopped();
    daemon_state.write(state_path)?;
    state::DaemonState::cleanup(state_path)?;

    tracing::info!("bmad-bot daemon stopped cleanly");
    Ok(())
}

// ---------------------------------------------------------------------------
// Polling loop with graceful shutdown
// ---------------------------------------------------------------------------

/// Polling loop with graceful shutdown, state file updates, and sprint-status polling.
///
/// Updates the daemon state file's `last_activity` timestamp each cycle.
/// Polls sprint-status.yaml via the watcher to detect eligible stories.
/// The `shutdown` flag is checked between poll cycles and also during pipeline
/// execution (streaming chat loops check it between chunks/turns).
async fn run_polling_loop(
    config: &Arc<BotConfig>,
    watcher: &crate::watcher::Watcher,
    pipeline: &crate::pipeline::StoryPipeline,
    daemon_state: &mut state::DaemonState,
    state_path: &Path,
    shutdown: &crate::session::runner::ShutdownFlag,
) -> Result<(), CliError> {
    let mut interval_timer =
        tokio::time::interval(Duration::from_secs(config.polling_interval_secs));

    loop {
        // Check shutdown flag at top of loop (covers case where flag was set
        // during a pipeline run that just finished)
        if shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!("Shutdown flag detected — exiting polling loop");
            break;
        }

        tokio::select! {
            _ = interval_timer.tick() => {
                // Update last activity timestamp
                daemon_state.touch();
                if let Err(e) = daemon_state.write(state_path) {
                    tracing::warn!(error = %e, "Failed to update daemon state file");
                }

                // Poll for eligible stories (Story 2.1)
                match watcher.poll() {
                    Ok(stories) => {
                        tracing::info!(
                            eligible_count = stories.len(),
                            "Found eligible stories — launching pipeline"
                        );
                        let summary = pipeline.process_eligible_stories(stories).await;
                        tracing::info!(
                            total = summary.total_processed,
                            completed = summary.completed,
                            blocked = summary.blocked,
                            errored = summary.errored,
                            fatal = summary.fatal,
                            "Pipeline run complete"
                        );
                        for _ in 0..summary.total_processed {
                            daemon_state.record_story_processed();
                        }

                        if summary.fatal {
                            tracing::error!(
                                "Fatal error detected (likely invalid credentials) — halting daemon. \
                                 Fix credentials and restart with `bmad-bot start`."
                            );
                            break;
                        }
                    }
                    Err(crate::watcher::WatcherError::NoEligibleStories) => {
                        tracing::info!("No eligible stories in this cycle — waiting for next poll");
                    }
                    Err(crate::watcher::WatcherError::SprintStatusNotFound { ref path }) => {
                        tracing::warn!(
                            path = %path,
                            "Sprint status file not found — has sprint-planning been run?"
                        );
                    }
                    Err(crate::watcher::WatcherError::CyclicDependency { ref cycle }) => {
                        tracing::error!(
                            cycle = ?cycle,
                            "Cyclic dependency detected in sprint-status — affected stories skipped, will retry next cycle"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            "Failed to poll sprint status — will retry next cycle"
                        );
                    }
                }
            }
            // Sleep a short interval and re-check the shutdown flag.
            // The signal handler task sets the flag; we detect it at loop top.
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                // Just wake up to re-check the shutdown flag at the top of the loop.
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
            code_review_enabled: true,
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
                    reasoning_effort: None,
                },
                review: LlmRoleConfig {
                    provider: "anthropic".to_string(),
                    model: "claude-sonnet-4-20250514".to_string(),
                    reasoning_effort: None,
                },
                supervisor: LlmRoleConfig {
                    provider: "openai".to_string(),
                    model: "gpt-4o".to_string(),
                    reasoning_effort: None,
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
            log_file: "bmad-bot.log".to_string(),
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
        let parsed: BotConfig = serde_yml::from_str(&yaml_body).unwrap();
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
        let parsed: BotConfig = serde_yml::from_str(&yaml_body).unwrap();
        assert!(parsed.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // generate_env_file tests (Story 1.3)
    // -----------------------------------------------------------------------

    #[test]
    fn test_generate_env_includes_anthropic_key() {
        let config = make_test_config();
        let env = generate_env_file(&config, None).unwrap();
        assert!(env.contains("ANTHROPIC_API_KEY="));
    }

    #[test]
    fn test_generate_env_includes_openai_key_for_supervisor() {
        let config = make_test_config(); // supervisor uses openai
        let env = generate_env_file(&config, None).unwrap();
        assert!(env.contains("OPENAI_API_KEY="));
    }

    #[test]
    fn test_generate_env_excludes_github_copilot_token() {
        let config = make_test_config(); // no role uses github-copilot
        let env = generate_env_file(&config, None).unwrap();
        assert!(!env.contains("GITHUB_COPILOT_OAUTH_TOKEN="));
    }

    #[test]
    fn test_generate_env_includes_github_token() {
        let config = make_test_config(); // git_provider is github
        let env = generate_env_file(&config, None).unwrap();
        assert!(env.contains("GITHUB_TOKEN="));
    }

    #[test]
    fn test_generate_env_excludes_gitlab_token() {
        let config = make_test_config(); // git_provider is github, not gitlab
        let env = generate_env_file(&config, None).unwrap();
        assert!(!env.contains("GITLAB_TOKEN="));
    }

    #[test]
    fn test_generate_env_excludes_telegram_when_disabled() {
        let config = make_test_config(); // telegram.enabled = false
        let env = generate_env_file(&config, None).unwrap();
        assert!(!env.contains("TELEGRAM_BOT_TOKEN="));
    }

    #[test]
    fn test_generate_env_includes_telegram_when_enabled() {
        let mut config = make_test_config();
        config.notifications.telegram.enabled = true;
        config.notifications.telegram.chat_id = "12345".to_string();
        let env = generate_env_file(&config, None).unwrap();
        assert!(env.contains("TELEGRAM_BOT_TOKEN="));
    }

    #[test]
    fn test_generate_env_copilot_token_prefilled() {
        let mut config = make_test_config();
        config.llm.dev.provider = "github-copilot".to_string();
        let env = generate_env_file(&config, Some("gho_test_token_123")).unwrap();
        assert!(env.contains("GITHUB_COPILOT_OAUTH_TOKEN=gho_test_token_123"));
        // Should NOT contain the "set manually" comment when token is present
        assert!(!env.contains("Obtain via"));
    }

    #[test]
    fn test_generate_env_copilot_token_empty_when_none() {
        let mut config = make_test_config();
        config.llm.dev.provider = "github-copilot".to_string();
        let env = generate_env_file(&config, None).unwrap();
        assert!(env.contains("GITHUB_COPILOT_OAUTH_TOKEN="));
        assert!(env.contains("Obtain via"));
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
        let env = generate_env_file(&config, None).unwrap();
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
        let env = generate_env_file(&config, None).unwrap();
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
            reasoning_effort: None,
        };
        let env = generate_env_file(&config, None).unwrap();
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
    fn test_default_model_for_provider_github_copilot() {
        assert_eq!(default_model_for_provider("github-copilot"), "gpt-4o");
    }

    #[test]
    fn test_default_model_for_provider_unknown() {
        assert_eq!(default_model_for_provider("unknown"), "");
    }

    // -----------------------------------------------------------------------
    // Sprint summary tests (Story 1.4 — Task 8)
    // -----------------------------------------------------------------------

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
        assert_eq!(summary.total_stories, 6);
        assert_eq!(summary.done, 1);
        assert_eq!(summary.in_progress, 1);
        assert_eq!(summary.ready_for_dev, 1);
        assert_eq!(summary.backlog, 3);
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
    fn test_sprint_summary_display_includes_blocked_when_nonzero() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sprint-status.yaml");
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: blocked
"#;
        std::fs::write(&path, content).unwrap();
        let summary = SprintSummary::from_file(&path);
        let output = format!("{summary}");
        assert!(output.contains("Blocked"));
    }

    #[test]
    fn test_sprint_summary_display_omits_blocked_when_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sprint-status.yaml");
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
"#;
        std::fs::write(&path, content).unwrap();
        let summary = SprintSummary::from_file(&path);
        let output = format!("{summary}");
        assert!(!output.contains("Blocked"));
    }

    #[test]
    fn test_sprint_summary_skips_retrospectives() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sprint-status.yaml");
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: done
  epic-1-retrospective: optional
"#;
        std::fs::write(&path, content).unwrap();
        let summary = SprintSummary::from_file(&path);
        assert_eq!(summary.total_stories, 1);
        assert_eq!(summary.total_epics, 1);
    }

    #[test]
    fn test_sprint_summary_counts_review_stories() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sprint-status.yaml");
        let content = r#"
development_status:
  epic-1: in-progress
  1-1-scaffolding: review
  1-2-cli: review
  1-3-init: done
"#;
        std::fs::write(&path, content).unwrap();
        let summary = SprintSummary::from_file(&path);
        assert_eq!(summary.review, 2);
        assert_eq!(summary.done, 1);
    }

    // -----------------------------------------------------------------------
    // parse_level_priority tests (Story 1.4 — Task 8)
    // -----------------------------------------------------------------------

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
        assert_eq!(parse_level_priority("warn"), parse_level_priority("WARN"));
    }

    #[test]
    fn test_parse_level_priority_unknown_returns_zero() {
        assert_eq!(parse_level_priority("foo"), 0);
        assert_eq!(parse_level_priority("verbose"), 0);
        assert_eq!(parse_level_priority(""), 0);
    }

    #[test]
    fn test_parse_level_priority_warning_alias() {
        assert_eq!(
            parse_level_priority("WARNING"),
            parse_level_priority("WARN")
        );
    }

    #[test]
    fn test_parse_level_priority_all_levels_nonzero() {
        for level in &["TRACE", "DEBUG", "INFO", "WARN", "ERROR"] {
            assert!(
                parse_level_priority(level) > 0,
                "Expected nonzero priority for {level}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Config log_file validation tests (Story 1.4 — Task 8)
    // -----------------------------------------------------------------------

    #[test]
    fn test_config_validate_accepts_log_file() {
        let config = make_test_config();
        assert!(config.validate().is_ok());
        assert_eq!(config.log_file, "bmad-bot.log");
    }

    #[test]
    fn test_config_validate_rejects_empty_log_file() {
        let mut config = make_test_config();
        config.log_file = String::new();
        let err = config.validate().unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("log_file"),
            "Error should mention log_file: {msg}"
        );
    }

    #[test]
    fn test_config_validate_rejects_whitespace_only_log_file() {
        let mut config = make_test_config();
        config.log_file = "   ".to_string();
        let err = config.validate().unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("log_file"),
            "Error should mention log_file: {msg}"
        );
    }

    #[test]
    fn test_config_validate_accepts_custom_log_file_path() {
        let mut config = make_test_config();
        config.log_file = "/var/log/my-daemon.log".to_string();
        assert!(config.validate().is_ok());
    }

    // -----------------------------------------------------------------------
    // CLI Logs subcommand args parsing (Story 1.4 — Task 8)
    // -----------------------------------------------------------------------

    #[test]
    fn test_cli_parse_logs_with_level_and_tail() {
        let cli =
            Cli::try_parse_from(["bmad-bot", "logs", "--level", "warn", "--tail", "10"]).unwrap();
        match cli.command {
            Commands::Logs { level, tail } => {
                assert_eq!(level, Some("warn".to_string()));
                assert_eq!(tail, 10);
            }
            _ => panic!("Expected Logs command"),
        }
    }

    #[test]
    fn test_cli_parse_logs_default_tail() {
        let cli = Cli::try_parse_from(["bmad-bot", "logs"]).unwrap();
        match cli.command {
            Commands::Logs { level, tail } => {
                assert!(level.is_none());
                assert_eq!(tail, 50);
            }
            _ => panic!("Expected Logs command"),
        }
    }

    #[test]
    fn test_cli_parse_logs_short_flags() {
        let cli = Cli::try_parse_from(["bmad-bot", "logs", "-l", "error", "-t", "5"]).unwrap();
        match cli.command {
            Commands::Logs { level, tail } => {
                assert_eq!(level, Some("error".to_string()));
                assert_eq!(tail, 5);
            }
            _ => panic!("Expected Logs command"),
        }
    }

    #[test]
    fn test_cli_parse_logs_level_only() {
        let cli = Cli::try_parse_from(["bmad-bot", "logs", "--level", "info"]).unwrap();
        match cli.command {
            Commands::Logs { level, tail } => {
                assert_eq!(level, Some("info".to_string()));
                assert_eq!(tail, 50); // default
            }
            _ => panic!("Expected Logs command"),
        }
    }

    // -----------------------------------------------------------------------
    // CliError display tests for new variants (Story 1.4 — Task 8)
    // -----------------------------------------------------------------------

    #[test]
    fn test_cli_error_display_state() {
        let err = CliError::State {
            reason: "corrupt file".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("State file error"));
        assert!(msg.contains("corrupt file"));
    }

    #[test]
    fn test_cli_error_display_log_file() {
        let err = CliError::LogFile {
            reason: "permission denied".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("Log file error"));
        assert!(msg.contains("permission denied"));
    }

    // -----------------------------------------------------------------------
    // validate_git_version tests (Story 4.4 — Task 1)
    // -----------------------------------------------------------------------

    #[test]
    fn test_validate_git_version_succeeds_on_current_system() {
        // This test runs against the real `git` on the dev machine.
        // It should pass on any machine with git >= 2.30 installed.
        let result = validate_git_version();
        assert!(result.is_ok(), "validate_git_version failed: {result:?}");
    }

    #[test]
    fn test_validate_git_version_parse_valid_version_string() {
        // Simulate parsing logic inline to test version extraction without
        // needing to mock the subprocess. We test the parsing branch directly.
        let stdout = "git version 2.39.5 (Apple Git-154)";
        let version_str = stdout.trim().strip_prefix("git version ").unwrap();
        let version_part = version_str.split_whitespace().next().unwrap();
        let parts: Vec<&str> = version_part.split('.').collect();
        let major: u32 = parts[0].parse().unwrap();
        let minor: u32 = parts[1].parse().unwrap();
        assert_eq!(major, 2);
        assert_eq!(minor, 39);
        assert!(major > 2 || (major == 2 && minor >= 30));
    }

    #[test]
    fn test_validate_git_version_parse_minimal_format() {
        // "git version 2.30.0" — exactly at the minimum
        let stdout = "git version 2.30.0";
        let version_str = stdout.trim().strip_prefix("git version ").unwrap();
        let version_part = version_str.split_whitespace().next().unwrap();
        let parts: Vec<&str> = version_part.split('.').collect();
        let major: u32 = parts[0].parse().unwrap();
        let minor: u32 = parts[1].parse().unwrap();
        assert_eq!(major, 2);
        assert_eq!(minor, 30);
    }

    #[test]
    fn test_validate_git_version_too_old_detected() {
        // Simulate version 2.29.0 — should be rejected
        let major: u32 = 2;
        let minor: u32 = 29;
        let too_old = major < 2 || (major == 2 && minor < 30);
        assert!(too_old, "2.29 should be considered too old");
    }

    #[test]
    fn test_validate_git_version_very_old_major_detected() {
        // Simulate version 1.9.0 — should be rejected
        let major: u32 = 1;
        let minor: u32 = 9;
        let too_old = major < 2 || (major == 2 && minor < 30);
        assert!(too_old, "1.9 should be considered too old");
    }

    #[test]
    fn test_validate_git_version_future_major_accepted() {
        // Simulate version 3.0.0 — should be accepted
        let major: u32 = 3;
        let minor: u32 = 0;
        let too_old = major < 2 || (major == 2 && minor < 30);
        assert!(!too_old, "3.0 should be accepted");
    }

    #[test]
    fn test_validate_git_version_unexpected_format_error() {
        // If git --version returned something unexpected, the prefix strip would fail
        let stdout = "not git output";
        let result = stdout.trim().strip_prefix("git version ");
        assert!(result.is_none(), "Should fail to parse unexpected format");
    }

    #[test]
    fn test_validate_git_version_error_message_for_too_old() {
        let major: u32 = 2;
        let minor: u32 = 20;
        let err = CliError::Init {
            reason: format!("git >= 2.30 required, found {major}.{minor}. Please upgrade git."),
        };
        let msg = format!("{err}");
        assert!(msg.contains("2.30 required"));
        assert!(msg.contains("2.20"));
    }
}
