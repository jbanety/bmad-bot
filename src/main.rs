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
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Minimal tracing init — Story 1.2 replaces with config-driven setup
    // Defaults to info level; override with RUST_LOG env var
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("bmad-bot starting");

    // Story 1.2 adds CLI dispatch (clap) and daemon lifecycle here
    Ok(())
}
