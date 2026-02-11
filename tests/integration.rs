//! Integration test binary entry point.
//!
//! Cargo discovers this file as `cargo test --test integration`.
//! Submodules live in `tests/integration/`.

#[path = "integration/helpers/mod.rs"]
mod helpers;
#[path = "integration/test_fixtures.rs"]
mod test_fixtures;
#[path = "integration/test_mocks.rs"]
mod test_mocks;
