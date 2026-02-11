#![deny(clippy::all)]
#![warn(dead_code)] // FIXME: Change to #![deny(dead_code)] once all modules have real implementations

mod cli;

// Re-export library crate modules so cli can use crate:: paths
pub use bmad_bot::auth;
pub use bmad_bot::config;
pub use bmad_bot::git_provider;
pub use bmad_bot::llm_context;
pub use bmad_bot::llm_logging;
pub use bmad_bot::notifier;
pub use bmad_bot::pipeline;
pub use bmad_bot::review;
pub use bmad_bot::session;
pub use bmad_bot::supervisor;
pub use bmad_bot::tools;
pub use bmad_bot::watcher;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    // Install rustls CryptoProvider before any TLS usage (required by rustls 0.23+)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls CryptoProvider");

    let cli = cli::Cli::parse();

    match cli.command {
        cli::Commands::Start => {
            cli::run_start(&cli.config).await?;
        }
        cli::Commands::Init { copilot_login } => {
            // Tracing is global — main.rs owns the subscriber for non-start commands
            let _ = tracing_subscriber::fmt::try_init();
            if copilot_login {
                cli::run_copilot_login().await?;
            } else {
                cli::run_init(&cli.config).await?;
            }
        }
        cli::Commands::Status => {
            let _ = tracing_subscriber::fmt::try_init();
            cli::run_status(&cli.config).await?;
        }
        cli::Commands::Logs { level, tail } => {
            let _ = tracing_subscriber::fmt::try_init();
            cli::run_logs(&cli.config, level, Some(tail)).await?;
        }
    }

    Ok(())
}
