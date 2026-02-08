//! End-to-end tests for bmad-bot.
//!
//! These tests run actual LLM sessions and are expensive (token cost).
//! They are gated behind the `BMAD_E2E=1` environment variable and
//! should NEVER run in CI or automated pipelines.
//!
//! To run: `BMAD_E2E=1 cargo test --test e2e`
//!
//! TODO: Implemented starting in Epic 4
