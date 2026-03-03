//! Integration tests for bmad-bot.
//!
//! Run: `cargo test --test integration`
//! These tests are deterministic — no real API calls, safe for CI.

#[path = "integration/helpers/mod.rs"]
mod helpers;

#[path = "integration/test_mocks.rs"]
mod test_mocks;

#[path = "integration/test_fixtures.rs"]
mod test_fixtures;
