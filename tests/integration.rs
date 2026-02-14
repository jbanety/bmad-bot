// Integration test binary entry point.
// Cargo discovers this as: `cargo test --test integration`
//
// Submodules live in `tests/integration/` directory.

#[path = "integration/helpers/mod.rs"]
mod helpers;
#[path = "integration/test_fixtures.rs"]
mod test_fixtures;
#[path = "integration/test_mocks.rs"]
mod test_mocks;
