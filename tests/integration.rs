//! Integration test binary entry point.
//!
//! Cargo discovers `tests/integration.rs` and compiles `tests/integration/`
//! as its submodule tree. Run via `cargo test --test integration`.

#[path = "integration/helpers/mod.rs"]
mod helpers;

#[path = "integration/test_fixtures.rs"]
mod test_fixtures;

#[path = "integration/test_mocks.rs"]
mod test_mocks;

#[path = "integration/test_config.rs"]
mod test_config;

#[path = "integration/test_watcher.rs"]
mod test_watcher;
