//! BMAD Bot library crate — re-exports modules for integration and E2E tests.
//!
//! The primary entry point is `src/main.rs` (the binary crate). This library
//! crate exists solely to expose internal modules to integration tests in
//! `tests/`. Only modules needed by tests are made public here.

#![deny(clippy::all)]
#![warn(dead_code)]

pub mod config;
pub mod mcp;

// Re-exported for integration tests. These modules mirror main.rs declarations.
pub mod auth;
pub mod cli;
pub mod git_provider;
pub mod llm;
pub mod notifier;
pub mod pipeline;
pub mod review;
pub mod session;
pub mod supervisor;
pub mod tools;
pub mod watcher;
