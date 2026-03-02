//! BMAD Bot library crate — re-exports modules for integration and E2E tests.
//!
//! The primary entry point is `src/main.rs` (the binary crate). This library
//! crate exists solely to expose internal modules to integration tests in
//! `tests/`. Only modules needed by tests are made public here.

#![deny(clippy::all)]
#![warn(dead_code)]

pub mod config;
pub mod mcp;
