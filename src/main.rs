#![deny(clippy::all)]
#![warn(dead_code)] // FIXME: Change to #![deny(dead_code)] once all modules have real implementations

mod auth;
mod cli;
mod config;
mod git_provider;
mod llm;
mod notifier;
mod pipeline;
mod review;
mod session;
mod supervisor;
mod tools;
mod watcher;

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
