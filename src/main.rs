#![deny(clippy::all)]
#![warn(dead_code)] // FIXME: Change to #![deny(dead_code)] once all modules have real implementations

mod cli;

// Re-export library crate modules so `crate::X` paths in CLI submodules resolve correctly.
// These were previously `mod X;` declarations — now sourced from the library crate.
// Only modules actually referenced via `crate::X` in the CLI submodule are listed here.
use bmad_bot::auth;
use bmad_bot::config;
use bmad_bot::mcp;
use bmad_bot::pipeline;
use bmad_bot::session;
use bmad_bot::watcher;

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
