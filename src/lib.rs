//! bmad-bot library crate — exposes modules for integration tests.
#![deny(clippy::all)]
#![warn(dead_code)]

pub mod auth;
pub mod cli;
pub mod config;
pub mod git_provider;
pub mod llm;
pub mod notifier;
pub mod pipeline;
pub mod review;
pub mod session;
pub mod supervisor;
pub mod tools;
pub mod watcher;
