#![deny(clippy::all)]
#![warn(dead_code)]

mod cli;

// All non-CLI modules are declared in lib.rs (the library crate).
// Re-export them so `crate::config`, `crate::auth` etc. remain
// visible to the `cli` submodule without changing its imports.
pub use bmad_bot::*;

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
