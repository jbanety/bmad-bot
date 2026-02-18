//! End-to-end tests for bmad-bot.
//!
//! These tests run actual MCP servers and/or LLM sessions and are expensive.
//! They are gated behind the `BMAD_E2E=1` environment variable and
//! should NEVER run in CI or automated pipelines.
//!
//! To run: `BMAD_E2E=1 cargo test --test e2e -- --ignored`

#[path = "e2e/mcp_playwright.rs"]
mod mcp_playwright;
