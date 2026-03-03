// Integration test binary entry point.
// Cargo discovers this as `cargo test --test integration`.
// The `tests/integration/` directory contains submodules.

#[path = "integration/helpers/mod.rs"]
mod helpers;

#[path = "integration/test_fixtures.rs"]
mod test_fixtures;

#[path = "integration/test_mocks.rs"]
mod test_mocks;
