//! MCP (Model Context Protocol) client integration.
//!
//! This module manages connections to external MCP servers, discovers their
//! tools at startup, and provides them to the agent via `tools_for_builder()`.

mod manager;

pub use manager::{McpError, McpManager};
