// Integration test binary entry point.
// Run with: `cargo test --test integration`

#[path = "integration/helpers/mod.rs"]
mod helpers;

#[path = "integration/test_mocks.rs"]
mod test_mocks;

#[path = "integration/test_fixtures.rs"]
mod test_fixtures;

#[path = "integration/test_config.rs"]
mod test_config;
