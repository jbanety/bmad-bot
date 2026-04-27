//! MCP (Model Context Protocol) client integration.
//!
//! This module manages connections to external MCP servers, discovers their
//! tools at startup, and provides them to the agent via `tools_for_builder()`.

mod manager;

pub use manager::McpManager;

/// Extract tool names from MCP server data returned by [`McpManager::tools_for_builder()`].
///
/// This is a free function (not a method on `McpManager`) because callers
/// already have the data from `tools_for_builder()` — avoids a second
/// traversal of `McpManager` internals.
pub fn extract_mcp_tool_names(
    servers: &[(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)],
) -> Vec<String> {
    servers
        .iter()
        .flat_map(|(tools, _)| tools.iter().map(|t| t.name.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mcp_tool_names_empty_returns_empty() {
        let result = extract_mcp_tool_names(&[]);
        assert!(result.is_empty(), "Empty input must return empty vec");
    }
}
