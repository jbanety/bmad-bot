//! bmad-bot library crate — exposes modules for integration tests.
//!
//! The primary entry point is `src/main.rs` (the binary crate). This library
//! crate exists to expose internal modules to integration tests in `tests/`.

#![deny(clippy::all)]
#![warn(dead_code)]

pub mod auth;
pub mod cli;
pub mod config;
pub mod git_provider;
pub mod llm;
pub mod mcp;
pub mod notifier;
pub mod pipeline;
pub mod review;
pub mod session;
pub mod supervisor;
pub mod tools;
pub mod watcher;
