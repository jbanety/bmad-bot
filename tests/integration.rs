// Integration test binary entry point.
// Cargo discovers this as `cargo test --test integration`.

#[path = "integration/helpers/mod.rs"]
mod helpers;

#[path = "integration/test_mocks.rs"]
mod test_mocks;

#[path = "integration/test_fixtures.rs"]
mod test_fixtures;

#[path = "integration/test_config.rs"]
mod test_config;

#[path = "integration/test_watcher.rs"]
mod test_watcher;
