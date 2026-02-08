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
            // Tracing is global — main.rs owns the subscriber for non-start commands
            let _ = tracing_subscriber::fmt::try_init();
            cli::run_init(&cli.config).await?;
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
