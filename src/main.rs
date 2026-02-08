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

#[tokio::main]
async fn main() -> Result<()> {
    // Minimal tracing init — Story 1.2 replaces with config-driven setup
    tracing_subscriber::fmt::init();

    tracing::info!("bmad-bot starting");

    // Story 1.2 adds CLI dispatch (clap) and daemon lifecycle here
    Ok(())
}
