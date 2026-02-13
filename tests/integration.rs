//! Integration test binary entry point.
//!
//! Cargo discovers this file as a test binary → `cargo test --test integration`.
//! All test modules live under `tests/integration/` as submodules.

#[path = "integration/helpers/mod.rs"]
mod helpers;
#[path = "integration/test_fixtures.rs"]
mod test_fixtures;
#[path = "integration/test_mocks.rs"]
mod test_mocks;
