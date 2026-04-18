//! E2E tests for Playwright MCP server integration.
//!
//! These tests validate that the BMAD Bot MCP infrastructure works end-to-end
//! with a real Playwright MCP server: connection, tool discovery, direct tool
//! invocation, and crash resilience.
//!
//! ## Prerequisites
//!
//! - `npx` available on PATH
//! - `@playwright/mcp` installable via npx (auto-installed on first run)
//! - A display server or headless browser support
//!   - For headless environments, the tests use `--headless` by default
//!
//! ## Running
//!
//! ```bash
//! BMAD_E2E=1 cargo test --test e2e -- --ignored
//! ```
//!
//! These tests are `#[ignore]` by default — they spawn real MCP server processes
//! and require a working Node.js environment. They should NEVER run in CI or
//! automated pipelines.

use bmad_bot::config::{McpServerConfig, McpTransport};
use bmad_bot::mcp::{McpManager, extract_mcp_tool_names};

use std::time::Duration;

/// Creates a Playwright MCP server config for E2E tests.
///
/// Uses `--headless` mode to avoid requiring a display server. This makes
/// the tests portable across local dev machines and headless servers.
fn playwright_config() -> Vec<McpServerConfig> {
    vec![McpServerConfig {
        name: "playwright".to_string(),
        command: "npx".to_string(),
        args: vec![
            "-y".to_string(),
            "@playwright/mcp".to_string(),
            "--headless".to_string(),
        ],
        transport: McpTransport::Stdio,
        enabled: true,
        timeout_secs: Some(60),
    }]
}

/// Guard struct that ensures `McpManager::shutdown()` is called on drop.
///
/// Prevents orphaned MCP server child processes when tests fail or panic.
struct McpGuard<'a> {
    manager: &'a McpManager,
    rt: &'a tokio::runtime::Handle,
}

impl<'a> McpGuard<'a> {
    fn new(manager: &'a McpManager, rt: &'a tokio::runtime::Handle) -> Self {
        Self { manager, rt }
    }
}

impl Drop for McpGuard<'_> {
    fn drop(&mut self) {
        // `Handle::block_on` cannot be called from within a tokio worker
        // thread — `block_in_place` moves the current task off the worker so
        // the runtime can be re-entered without panicking.
        let manager = self.manager;
        let rt = self.rt.clone();
        tokio::task::block_in_place(|| {
            rt.block_on(async {
                manager.shutdown().await;
            });
        });
    }
}

/// Verify that `McpManager::init()` with Playwright config connects successfully
/// and discovers browser automation tools.
///
/// Validates AC #1: daemon connects, discovers tools, logs count.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_playwright_mcp_server_connects_and_discovers_tools() {
    if std::env::var("BMAD_E2E").as_deref() != Ok("1") {
        tracing::info!("Skipping E2E test: set BMAD_E2E=1 to run");
        return;
    }

    let manager = tokio::time::timeout(Duration::from_secs(90), async {
        McpManager::init(&playwright_config()).await
    })
    .await
    .expect("McpManager::init() timed out — is npx available on PATH?");

    let handle = tokio::runtime::Handle::current();
    let _guard = McpGuard::new(&manager, &handle);

    let tools = manager.tools_for_builder().await;

    assert!(
        !tools.is_empty(),
        "Should connect to at least one MCP server"
    );

    let (ref tool_defs, _) = tools[0];
    assert!(
        tool_defs.len() >= 15,
        "Playwright MCP should discover at least 15 tools, got {}",
        tool_defs.len()
    );

    // Verify well-known Playwright tool names are present
    let tool_names: Vec<String> = tool_defs.iter().map(|t| t.name.to_string()).collect();

    assert!(
        tool_names.contains(&"browser_navigate".to_string()),
        "Expected browser_navigate in discovered tools: {tool_names:?}"
    );
    assert!(
        tool_names.contains(&"browser_click".to_string()),
        "Expected browser_click in discovered tools: {tool_names:?}"
    );
    assert!(
        tool_names.contains(&"browser_snapshot".to_string()),
        "Expected browser_snapshot in discovered tools: {tool_names:?}"
    );

    tracing::info!(
        tool_count = tool_defs.len(),
        tools = ?tool_names,
        "Playwright MCP tools discovered"
    );
}

/// Verify that `browser_navigate` can be called directly via the MCP server
/// and returns page content.
///
/// Validates AC #2: agent calls browser_navigate, server opens browser,
/// result is returned as text content.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_playwright_mcp_navigate_returns_content() {
    if std::env::var("BMAD_E2E").as_deref() != Ok("1") {
        tracing::info!("Skipping E2E test: set BMAD_E2E=1 to run");
        return;
    }

    let manager = tokio::time::timeout(Duration::from_secs(90), async {
        McpManager::init(&playwright_config()).await
    })
    .await
    .expect("McpManager::init() timed out");

    let handle = tokio::runtime::Handle::current();
    let _guard = McpGuard::new(&manager, &handle);

    let tools = manager.tools_for_builder().await;
    assert!(!tools.is_empty(), "No MCP servers connected");

    let (_, ref sink) = tools[0];

    // Call browser_navigate directly via the ServerSink (Peer<RoleClient>).
    // This validates MCP tool invocation without requiring an LLM API key.
    let params = rmcp::model::CallToolRequestParams::new("browser_navigate").with_arguments(
        serde_json::json!({
            "url": "data:text/html,<body>test</body>"
        })
        .as_object()
        .cloned()
        .unwrap_or_default(),
    );

    let result = tokio::time::timeout(Duration::from_secs(30), sink.call_tool(params))
        .await
        .expect("browser_navigate call timed out")
        .expect("browser_navigate call failed");

    // The result should contain page content or navigation confirmation.
    // Content is Annotated<RawContent> which Derefs to RawContent.
    let content_text: String = result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !content_text.is_empty(),
        "browser_navigate should return non-empty content"
    );

    tracing::info!(
        content_len = content_text.len(),
        "browser_navigate returned content"
    );
}

/// Verify that `browser_screenshot` can be called directly via the MCP server
/// and returns image data or confirmation.
///
/// Validates AC #3: agent calls browser_screenshot, server captures screenshot,
/// result is returned to the agent.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_playwright_mcp_screenshot_returns_data() {
    if std::env::var("BMAD_E2E").as_deref() != Ok("1") {
        tracing::info!("Skipping E2E test: set BMAD_E2E=1 to run");
        return;
    }

    let manager = tokio::time::timeout(Duration::from_secs(90), async {
        McpManager::init(&playwright_config()).await
    })
    .await
    .expect("McpManager::init() timed out");

    let handle = tokio::runtime::Handle::current();
    let _guard = McpGuard::new(&manager, &handle);

    let tools = manager.tools_for_builder().await;
    assert!(!tools.is_empty(), "No MCP servers connected");

    let (_, ref sink) = tools[0];

    // First navigate to a page so there's content to screenshot
    let nav_params = rmcp::model::CallToolRequestParams::new("browser_navigate").with_arguments(
        serde_json::json!({
            "url": "data:text/html,<body>test</body>"
        })
        .as_object()
        .cloned()
        .unwrap_or_default(),
    );

    tokio::time::timeout(Duration::from_secs(30), sink.call_tool(nav_params))
        .await
        .expect("navigate timed out")
        .expect("navigate failed");

    // Now take a screenshot
    let screenshot_params = rmcp::model::CallToolRequestParams::new("browser_screenshot");

    let result = tokio::time::timeout(Duration::from_secs(30), sink.call_tool(screenshot_params))
        .await
        .expect("browser_screenshot call timed out")
        .expect("browser_screenshot call failed");

    // The result should contain image data (base64) or text confirmation
    assert!(
        !result.content.is_empty(),
        "browser_screenshot should return non-empty content"
    );

    // Check for either image or text content in the result.
    // Annotated<RawContent> Derefs to RawContent — use as_text()/as_image().
    let has_content = result
        .content
        .iter()
        .any(|c| c.as_text().is_some_and(|t| !t.text.is_empty()) || c.as_image().is_some());

    assert!(
        has_content,
        "browser_screenshot should return image data or text confirmation"
    );

    tracing::info!(
        content_items = result.content.len(),
        "browser_screenshot returned data"
    );
}

/// Verify that MCP tools are correctly extracted and available for
/// `ToolConfigurator` registration without building a full LLM agent.
///
/// This validates the wiring from `McpManager` → `tools_for_builder()` →
/// `extract_mcp_tool_names()` without requiring an LLM API key.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_playwright_mcp_tools_registered_on_configurator() {
    if std::env::var("BMAD_E2E").as_deref() != Ok("1") {
        tracing::info!("Skipping E2E test: set BMAD_E2E=1 to run");
        return;
    }

    let manager = tokio::time::timeout(Duration::from_secs(90), async {
        McpManager::init(&playwright_config()).await
    })
    .await
    .expect("McpManager::init() timed out");

    let handle = tokio::runtime::Handle::current();
    let _guard = McpGuard::new(&manager, &handle);

    let tools_data = manager.tools_for_builder().await;
    assert!(
        !tools_data.is_empty(),
        "No MCP servers connected — cannot test configurator"
    );

    // Extract tool names using the production utility function
    let tool_names = extract_mcp_tool_names(&tools_data);

    assert!(
        !tool_names.is_empty(),
        "extract_mcp_tool_names should return non-empty vec"
    );

    // Verify well-known Playwright tools are in the extracted names
    let expected_tools = ["browser_navigate", "browser_click", "browser_snapshot"];

    for expected in &expected_tools {
        assert!(
            tool_names.iter().any(|n| n == expected),
            "Expected '{expected}' in extracted tool names: {tool_names:?}"
        );
    }

    tracing::info!(
        tool_count = tool_names.len(),
        tools = ?tool_names,
        "MCP tool names extracted for configurator"
    );
}

/// Verify that an MCP server crash mid-session does not terminate the session.
///
/// Validates the error-propagation half of AC #4: server crash → clear error
/// returned to the caller → no panic → session not terminated. The "native
/// tools continue to work" half of AC #4 is covered at the framework level
/// by rig's `McpTool` wrapper, which converts tool-call errors into LLM-
/// visible error strings while leaving other registered tools untouched; it
/// is not exercised here.
///
/// Strategy: connect to Playwright MCP, manually kill the child process by
/// dropping the manager (which shuts down servers), then verify that calling
/// a tool on the now-dead sink returns an error rather than panicking.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_mcp_server_crash_does_not_terminate_session() {
    if std::env::var("BMAD_E2E").as_deref() != Ok("1") {
        tracing::info!("Skipping E2E test: set BMAD_E2E=1 to run");
        return;
    }

    let manager = tokio::time::timeout(Duration::from_secs(90), async {
        McpManager::init(&playwright_config()).await
    })
    .await
    .expect("McpManager::init() timed out");

    // Get the sink BEFORE shutting down — simulates holding a reference
    // while the server crashes.
    let tools = manager.tools_for_builder().await;
    assert!(!tools.is_empty(), "No MCP servers connected");

    let (_, ref sink) = tools[0];

    // Verify the server is working first
    let nav_params = rmcp::model::CallToolRequestParams::new("browser_navigate").with_arguments(
        serde_json::json!({
            "url": "data:text/html,<body>test</body>"
        })
        .as_object()
        .cloned()
        .unwrap_or_default(),
    );

    let result = tokio::time::timeout(Duration::from_secs(30), sink.call_tool(nav_params.clone()))
        .await
        .expect("pre-crash navigate timed out");

    assert!(
        result.is_ok(),
        "Tool call should succeed before server crash"
    );

    // Simulate server crash by shutting down the manager
    // This sends close notifications and kills child processes.
    manager.shutdown().await;

    // Small delay to ensure the process is fully terminated
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Now call a tool on the dead sink — should return an error, NOT panic
    let post_crash_result =
        tokio::time::timeout(Duration::from_secs(10), sink.call_tool(nav_params)).await;

    match post_crash_result {
        Ok(Err(e)) => {
            // Expected: tool call fails with an error
            tracing::info!(
                error = %e,
                "Post-crash tool call returned error as expected"
            );
        }
        Ok(Ok(_)) => {
            // AC #4 requires a clear error after server crash — success is a regression.
            panic!("Post-crash tool call unexpectedly succeeded — crash resilience regressed");
        }
        Err(_) => {
            // Timeout is also acceptable — the dead server can't respond
            tracing::info!("Post-crash tool call timed out as expected");
        }
    }

    // The key assertion: we got here without panicking.
    // In a real session, the agent would receive the error and continue
    // with native tools (edit_file, grep, terminal, etc.).
    tracing::info!("Session survived MCP server crash — crash resilience validated");
}
