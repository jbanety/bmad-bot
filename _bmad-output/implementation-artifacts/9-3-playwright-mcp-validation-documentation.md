# Story 9.3: Playwright MCP Validation & Documentation

Status: ready-for-dev

## Story

As a daemon operator,
I want validated Playwright MCP integration and clear documentation on adding MCP servers,
So that I can confidently enable browser automation and extend the agent with new MCP tools in the future.

## Acceptance Criteria

1. **Given** Playwright MCP server (`@playwright/mcp`) is installed on the system **When** the daemon starts with a valid `mcp_servers` config entry for Playwright **Then** the daemon connects to the Playwright MCP server **And** discovers browser automation tools (navigate, screenshot, click, fill, etc.) **And** logs the discovered tool names and count

2. **Given** an agent session is active with Playwright MCP tools registered **When** the agent calls `browser_navigate` with a URL **Then** the Playwright MCP server opens a browser and navigates to the URL **And** the result is returned to the agent as text content

3. **Given** an agent session is active with Playwright MCP tools registered **When** the agent calls `browser_screenshot` **Then** the Playwright MCP server captures a screenshot **And** the result (base64 image data or confirmation) is returned to the agent

4. **Given** the Playwright MCP server crashes or becomes unresponsive mid-session **When** the agent calls a Playwright tool **Then** a clear error is returned to the agent via rig's `McpTool` error handling **And** the agent can continue using native tools (edit_file, grep, terminal, etc.) **And** the session is not terminated

5. **Given** a user wants to add a new MCP server (e.g., a database tool) **When** they read the project documentation **Then** `docs/mcp-servers.md` explains the `mcp_servers` config format with all fields, how to add a new server (one config entry, zero code changes), how to disable a server without removing config (`enabled: false`), prerequisites (e.g., `npx` for npm-based MCP servers), and troubleshooting (check daemon logs for tool discovery messages) **And** the Playwright example config is included as a reference

6. **Given** the documentation is complete **When** reviewing `bmad-bot.yaml.example` **Then** it includes a commented-out `mcp_servers` section showing the Playwright example

## Tasks / Subtasks

- [ ] Task 1: Update `bmad-bot.yaml.example` with MCP servers section (AC: #6)
  - [ ] 1.1 Add a commented-out `mcp_servers` section at the end of `bmad-bot.yaml.example`, after the `bmad_paths` section
  - [ ] 1.2 Include the Playwright example with all fields: `name`, `command`, `args`, `transport`, `enabled`, `timeout_secs`
  - [ ] 1.3 Add inline comments explaining each field and usage pattern, including `timeout_secs` (optional, default 30s)
  - [ ] 1.4 Show a second commented-out example entry (e.g., a hypothetical database MCP server) to demonstrate extensibility

- [ ] Task 2: Create `docs/mcp-servers.md` documentation (AC: #5)
  - [ ] 2.1 Create `docs/mcp-servers.md` with comprehensive MCP server documentation. **CRITICAL: Write ALL documentation in English** (`document_output_language = English`). Do NOT use French in docs even though `communication_language` is French.
  - [ ] 2.2 Section: Overview — what MCP is, why the daemon supports it, zero-code-change extensibility
  - [ ] 2.3 Section: Configuration Reference — full `mcp_servers` YAML schema with ALL 6 fields, types, defaults: `name` (String, required), `command` (String, required), `args` (Vec<String>, required), `transport` (String, default: `stdio`), `enabled` (bool, default: `true`), `timeout_secs` (Option<u64>, default: 30 — per-server MCP handshake timeout)
  - [ ] 2.4 Section: Adding a New MCP Server — step-by-step guide (add YAML entry → **restart daemon** → check logs for tool discovery)
  - [ ] 2.5 Section: Playwright Example — complete working config, prerequisites (`npx`, `@playwright/mcp`), expected tool discovery output. **Include headless mode guidance:** document how to run Playwright in headless mode for servers without a display (e.g., `args: ["-y", "@playwright/mcp", "--headless"]` or the `DISPLAY` / `PLAYWRIGHT_BROWSERS_PATH` env vars as appropriate for `@playwright/mcp` version)
  - [ ] 2.6 Section: Disabling a Server — set `enabled: false` and **restart the daemon** to apply. Config is loaded once at startup and shared as `Arc<BotConfig>` (never hot-reloaded). Clarify this requires a restart.
  - [ ] 2.7 Section: Troubleshooting — how to verify connection via daemon logs, common failure scenarios (command not found, handshake timeout, server crash mid-session), what happens when MCP fails (non-blocking, native tools continue)
  - [ ] 2.8 Section: How It Works — brief technical overview for advanced users (startup discovery, `McpManager`, rig's `McpTool`, tool registration alongside native tools)
  - [ ] 2.9 Section: Supported Transports — currently `stdio` only, mention future extensibility

- [ ] Task 3: Create E2E validation test script (AC: #1, #2, #3, #4)
  - [ ] 3.1 **E2E test structure:** The existing `tests/e2e/mod.rs` is a stub with only doc comments and a TODO. It serves as a library module for `tests/e2e.rs` (the test binary entry point, implied by `cargo test --test e2e`). If `tests/e2e.rs` does not exist, create it with `mod e2e;` to re-export the module. Then add `pub mod mcp_playwright;` to `tests/e2e/mod.rs`. Verify `cargo test --test e2e` compiles before adding test functions.
  - [ ] 3.2 Create `tests/e2e/mcp_playwright.rs` with E2E test functions. **Gating pattern:** Each test must check `BMAD_E2E` env var inline at the top of the function body (NOT via a helper function — a helper's `return` only exits the helper, not the test). Use this pattern:
        ```rust
        #[tokio::test]
        #[ignore]
        async fn test_playwright_mcp_server_connects_and_discovers_tools() {
            if std::env::var("BMAD_E2E").is_err() {
                tracing::info!("Skipping E2E test: set BMAD_E2E=1 to run");
                return;  // returns from the test itself
            }
            // ... test body ...
        }
        ```
  - [ ] 3.3 Test: `test_playwright_mcp_server_connects_and_discovers_tools` — verify `McpManager::init()` with Playwright config discovers expected tool names (browser_navigate, browser_screenshot, browser_click, browser_fill, browser_snapshot, etc.). Assert `tools_for_builder()` returns non-empty vec with tool count > 0.
  - [ ] 3.4 Test: `test_playwright_mcp_navigate_returns_content` — invoke `browser_navigate` **directly via `ToolDyn::call()`** on the `McpTool` constructed from the discovered tools and `ServerSink`. Do NOT build a full LLM agent — call the tool directly. Construct the JSON input matching the tool's schema, call it, assert the result contains page content. This validates AC #2 without requiring an LLM API key.
  - [ ] 3.5 Test: `test_playwright_mcp_screenshot_returns_data` — invoke `browser_screenshot` **directly via `ToolDyn::call()`** on the `McpTool`. Same direct-call pattern as 3.4. Assert result contains image data or confirmation. This validates AC #3 without requiring an LLM API key.
  - [ ] 3.6 Test: `test_playwright_mcp_tools_registered_on_configurator` — verify MCP tool registration at the **`ToolConfigurator` level** without building a full agent (which would require an LLM API key). Call `McpManager::init()`, then `tools_for_builder()`, then `extract_mcp_tool_names()` and assert expected tool names are present. This validates the wiring without LLM dependency.
  - [ ] 3.7 All E2E tests must be `#[ignore]` by default — they require a real Playwright MCP server and browser environment. Run with: `BMAD_E2E=1 cargo test --test e2e -- --ignored`
  - [ ] 3.8 Add doc comments explaining prerequisites: `npx` available on PATH, `@playwright/mcp` installable via npx, headless browser support or a display server. Document headless mode: if running on a headless server, use `args: ["-y", "@playwright/mcp", "--headless"]` or set appropriate Playwright env vars.
  - [ ] 3.9 Every test must call `mcp_manager.shutdown().await` before returning (including on assertion failure — use a defer/cleanup pattern or explicit shutdown in each branch) to avoid orphaned child processes

- [ ] Task 4: Validate MCP server crash resilience (AC: #4)
  - [ ] 4.1 In `tests/e2e/mcp_playwright.rs`, add `test_mcp_server_crash_does_not_terminate_session` — start Playwright MCP, kill the child process, verify next tool call returns error (not panic), verify native tools still work
  - [ ] 4.2 Document the crash behavior in `docs/mcp-servers.md` troubleshooting section

- [ ] Task 5: Verify zero-regression on existing functionality (AC: #1, #4)
  - [ ] 5.1 Run `cargo test` — all existing unit tests pass with no changes (run FIRST, before creating new test files)
  - [ ] 5.2 Run `cargo clippy` — zero warnings
  - [ ] 5.3 Run `cargo fmt --check` — no formatting issues
  - [ ] 5.4 Verify daemon starts correctly with empty `mcp_servers` config (no behavioral change)
  - [ ] 5.5 Verify daemon starts correctly with no `mcp_servers` section at all (backward compat)

- [ ] Task 6: Update `README.md` with MCP documentation cross-reference (AC: #5)
  - [ ] 6.1 Add a brief MCP section or bullet point to `README.md` pointing users to `docs/mcp-servers.md` for MCP server configuration and Playwright browser automation setup
  - [ ] 6.2 Keep it minimal — one or two lines with a link, not a duplicate of the full docs

## Dev Notes

### Architecture Patterns & Constraints

- **This is a validation + documentation story.** No new Rust modules, no new structs, no new traits. All MCP infrastructure was built in Stories 9.1 (McpManager, config) and 9.2 (agent integration, ToolConfigurator, preamble). [Source: epics.md#Epic 9 Summary]
- **E2E tests are manual-launch only.** Gate behind `BMAD_E2E=1` env var. Never in CI or automated runs. These tests have real costs (browser spawning, MCP server processes). [Source: project-context.md#Testing Rules]
- **Doc comments** (`///`) mandatory on all public test functions. [Source: project-context.md#Code Quality & Style Rules]
- **No `println!` or `eprintln!`** — use `tracing` for any diagnostic output in test helpers. [Source: project-context.md#Language-Specific Rules]
- **Non-blocking failures:** MCP server crashes during a session MUST NOT crash the daemon or terminate the agent session. The agent continues with native tools. This is enforced by rig's `McpTool` error handling — tool call returns an error string to the LLM, which then decides how to proceed. [Source: architect-brief-mcp-client-integration.md#Risk Assessment]

### Source Tree Components to Touch

| File | Action | Details |
|------|--------|---------|
| `bmad-bot.yaml.example` | Edit | Add commented-out `mcp_servers` section with Playwright example (including `timeout_secs`) |
| `docs/mcp-servers.md` | Create | Comprehensive MCP server documentation (in English) |
| `tests/e2e/mcp_playwright.rs` | Create | E2E validation tests for Playwright MCP integration |
| `tests/e2e/mod.rs` | Edit | Add `pub mod mcp_playwright;` declaration |
| `tests/e2e.rs` | Create (if missing) | Test binary entry point — `mod e2e;` to load the module. Check if this file already exists before creating. |
| `README.md` | Edit | Add MCP section with link to `docs/mcp-servers.md` |

### bmad-bot.yaml.example — Exact Content to Add

Append after the `bmad_paths` section at end of file:

```yaml
# MCP (Model Context Protocol) server configuration — optional
# The daemon connects to configured MCP servers at startup, discovers their tools,
# and exposes them to the dev agent alongside native tools (edit_file, grep, etc.).
# Adding a new MCP server = one config entry here, zero code changes.
# All changes require a daemon restart to take effect.
# See docs/mcp-servers.md for full documentation.
#
# mcp_servers:
#   - name: playwright
#     command: npx
#     args: ["-y", "@playwright/mcp"]
#     transport: stdio        # Only "stdio" supported currently
#     enabled: true           # Set to false to disable without removing config
#     # timeout_secs: 30      # Optional: MCP handshake timeout in seconds (default: 30)
#
#   # Example: adding another MCP server
#   # - name: my-database-tool
#   #   command: npx
#   #   args: ["-y", "@example/db-mcp-server"]
#   #   transport: stdio
#   #   enabled: true
#   #   timeout_secs: 60      # Increase timeout for slow-starting servers
```

### docs/mcp-servers.md — Document Structure

The documentation file should follow this structure:

1. **Title & Overview** — What MCP is, why bmad-bot supports it, the "zero code changes" value proposition
2. **Quick Start** — Minimal config to get Playwright working (5 lines of YAML)
3. **Configuration Reference** — Table of all `McpServerConfig` fields:
   - `name` (String, required) — Human-readable identifier, used in log messages
   - `command` (String, required) — Executable to spawn (e.g., `npx`, `node`, path to binary)
   - `args` (Vec<String>, required) — Arguments passed to the command
   - `transport` (String, default: `stdio`) — Transport protocol. Currently only `stdio` supported
   - `enabled` (bool, default: `true`) — Set `false` to skip without removing config
   - `timeout_secs` (Option<u64>, default: `30`) — Per-server MCP handshake timeout in seconds. Increase for slow-starting servers or remote connections
4. **Playwright Setup** — Prerequisites, installation, complete config, expected log output showing discovered tools. **Include headless mode guidance** for servers without a display (`--headless` flag or env vars)
5. **Adding a New Server** — Step-by-step: add YAML entry → **restart daemon** → check logs
6. **Disabling a Server** — Set `enabled: false` and **restart the daemon**. Config is loaded once at startup (`Arc<BotConfig>`, never hot-reloaded) — changes only take effect after restart.
7. **How It Works** — Technical flow: daemon startup → `McpManager::init()` → spawn process → MCP handshake → `list_tools()` → tools registered on agent via rig's `.rmcp_tools()` → agent uses them like native tools
8. **Troubleshooting** — Common issues table:
   - "Command not found" → install prerequisite, check PATH
   - "Handshake timeout" → increase timeout, check server logs
   - "Server crashed mid-session" → agent gets error, continues with native tools, session not terminated
   - "No tools discovered" → check MCP server implementation, verify `list_tools()` support
9. **Supported Transports** — `stdio` only. Future: SSE, WebSocket (when rmcp adds support)

### Playwright MCP — Expected Tool Discovery

When Playwright MCP (`@playwright/mcp`) connects successfully, `list_tools()` returns approximately these tools (names may vary by Playwright MCP version):

- `browser_navigate` — Navigate to a URL
- `browser_screenshot` — Take a page screenshot
- `browser_click` — Click an element
- `browser_fill` — Fill a form field
- `browser_snapshot` — Get accessibility snapshot of the page
- `browser_type` — Type text into an element
- `browser_select_option` — Select dropdown option
- `browser_hover` — Hover over an element
- `browser_press_key` — Press a keyboard key
- `browser_handle_dialog` — Handle browser dialogs (alert, confirm, prompt)
- `browser_tab_*` — Tab management tools
- `browser_wait_for` — Wait for element/condition
- `browser_drag` — Drag and drop
- `browser_console_messages` — Get console output
- `browser_network_requests` — Get network requests
- `browser_file_upload` — Upload files
- `browser_close` — Close the browser
- `browser_resize` — Resize browser window

The exact count and names depend on the `@playwright/mcp` version installed. The daemon logs `tracing::info!(server = "playwright", tool_count = N, "MCP tools discovered")`.

### E2E Test Design Patterns

**Test binary structure:** `tests/e2e/mod.rs` currently contains only doc comments and a TODO. It is loaded by `tests/e2e.rs` (the integration test binary entry point). If `tests/e2e.rs` does not exist, create it with `mod e2e;`. Add `pub mod mcp_playwright;` to `tests/e2e/mod.rs` to register the new module. Verify compilation with `cargo test --test e2e --no-run` before adding test functions.

```rust
// tests/e2e/mcp_playwright.rs

//! E2E tests for Playwright MCP server integration.
//!
//! These tests require:
//! - `npx` available on PATH
//! - `@playwright/mcp` installable via npx
//! - A display server or headless browser support (use `--headless` for CI-like environments)
//!
//! Run with: BMAD_E2E=1 cargo test --test e2e -- --ignored

use bmad_bot::config::McpServerConfig;
use bmad_bot::mcp::McpManager;
use std::sync::Arc;

/// Creates a Playwright MCP server config for E2E tests.
fn playwright_config() -> Vec<McpServerConfig> {
    vec![McpServerConfig {
        name: "playwright".to_string(),
        command: "npx".to_string(),
        args: vec!["-y".to_string(), "@playwright/mcp".to_string()],
        transport: Default::default(),
        enabled: true,
        timeout_secs: Some(30),
    }]
}

#[tokio::test]
#[ignore]
async fn test_playwright_mcp_server_connects_and_discovers_tools() {
    if std::env::var("BMAD_E2E").is_err() {
        tracing::info!("Skipping: set BMAD_E2E=1 to run");
        return; // returns from the test itself — NOT a helper function
    }
    let manager = McpManager::init(&playwright_config()).await;
    let tools = manager.tools_for_builder();
    assert!(!tools.is_empty(), "Should connect to at least one MCP server");
    // ... assert tool names ...
    manager.shutdown().await;
}
```

**⚠️ CRITICAL: Do NOT use a `require_e2e()` helper function.** A `return` inside a helper only exits the helper, not the calling test — the test would continue and crash on missing MCP server. Always check `BMAD_E2E` inline at the top of each test function.

Key patterns:
- Each test function is `#[tokio::test]` + `#[ignore]` (only runs when explicitly requested via `-- --ignored`)
- Check `BMAD_E2E` env var **inline at top of each test** (NOT via a helper — see warning above)
- Use `tracing::info!()` for skip messages — NEVER `eprintln!()` (project rule: no `eprintln!` in any code)
- Use `McpManager::init()` directly with a `McpServerConfig` for Playwright
- After init, call `tools_for_builder()` and assert tool count > 0
- For tool call tests (navigate, screenshot), call tools **directly via `ToolDyn::call()`** on `McpTool` — do NOT build a full LLM agent (requires API key). Construct the JSON input matching the tool's input schema and invoke the tool directly.
- Clean up: call `mcp_manager.shutdown().await` in every test path (including assertion failures — consider a cleanup guard)
- Timeout each test with `#[tokio::test(flavor = "multi_thread")]` and a `tokio::time::timeout()` wrapper to prevent hanging if MCP server is unresponsive
- For headless environments, consider adding `"--headless"` to args or document the env var approach

### MCP Manager API Reference (from Story 9.1)

```rust
// src/mcp/manager.rs — public API established in Story 9.1

impl McpManager {
    /// Connect to all configured MCP servers. Non-blocking — failures are logged, not propagated.
    pub async fn init(configs: &[McpServerConfig]) -> Self { ... }

    /// Create an empty manager (no MCP servers). Used when mcp_servers config is absent.
    pub fn empty() -> Self { ... }

    /// Get tool data for agent builder registration.
    /// Returns Vec of (tools, server_sink) tuples — one per connected server.
    /// Each call clones the data (ServerSink is Clone).
    pub fn tools_for_builder(&self) -> Vec<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)> { ... }

    /// Gracefully shut down all connected MCP servers.
    pub async fn shutdown(&self) { ... }
}
```

```rust
// src/mcp/mod.rs — utility function established in Story 9.2

/// Extract tool names from MCP server data returned by `McpManager::tools_for_builder()`.
pub fn extract_mcp_tool_names(
    servers: &[(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)]
) -> Vec<String> { ... }
```

### McpServerConfig (from Story 9.1, lives in src/config/mod.rs)

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default = "McpTransport::default")]
    pub transport: McpTransport,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Per-server MCP handshake timeout. Default: 30 seconds.
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    #[default]
    Stdio,
}
```

**All 6 fields must be documented in `docs/mcp-servers.md`.** The `timeout_secs` field was established in Story 9.1 and used as `config.timeout_secs.unwrap_or(30)` in `McpManager::init()`.

### Previous Story Intelligence (Story 9.2)

Story 9.2 established:
- `ToolConfigurator` now has `mcp_servers` field and `with_mcp()` builder method
- `configure_agent_tools!` macro initializes `mcp_servers: vec![]` by default
- All three `configure_*` impls (anthropic, openai_responses, openai_completions) chain `.rmcp_tools()` for each MCP server
- `build_preamble()` in `src/session/dev_agent.rs` accepts `&[String]` MCP tool names and conditionally appends them
- `extract_mcp_tool_names()` in `src/mcp/mod.rs` extracts tool names from `tools_for_builder()` output
- `SessionRunner`, `ReviewRunner`, and `ArchitectSession` all pass MCP data through to agent builders
- `Arc<McpManager>` is stored in `SessionRunner`, `ReviewRunner`, and `StoryPipeline` (from Story 9.1)
- All existing tests pass with zero regression

### Previous Story Intelligence (Story 9.1)

Story 9.1 established:
- `McpManager` struct in `src/mcp/manager.rs` with `init()`, `empty()`, `tools_for_builder()`, `shutdown()`
- `McpServerConfig` and `McpTransport` in `src/config/mod.rs`
- `McpError` per-module thiserror enum in `src/mcp/manager.rs`
- Config: `mcp_servers: Vec<McpServerConfig>` on `BotConfig` with `#[serde(default)]`
- Pipeline integration: `StoryPipeline`, `SessionRunner`, `ReviewRunner` all accept `Arc<McpManager>`
- Daemon startup: `McpManager::init()` called in `run_start()`, `shutdown()` on exit
- rmcp version 0.13 (must match rig-core's internal dependency)
- stdio transport via `TokioChildProcess` + `Command::new().configure()`
- 30-second handshake timeout (configurable)
- Non-blocking: failures logged via `tracing::warn!()`, daemon continues

### Testing Standards

- **Unit tests:** Inline `#[cfg(test)] mod tests` — NOT applicable for this story (no new unit-testable code)
- **E2E tests:** `tests/e2e/mcp_playwright.rs` — `#[ignore]` + `BMAD_E2E=1` inline check. Manual-launch only. Never in CI. [Source: project-context.md#Testing Rules]
- **Test naming:** Descriptive snake_case — `test_playwright_mcp_server_connects_and_discovers_tools`
- **No real LLM calls** in E2E tests for this story — test MCP server connection, tool discovery, and **direct `ToolDyn::call()` invocation** only. Do NOT build full agents via `AgentFactory` (requires API key). Use `McpTool::from_mcp_server()` or equivalent to get a `ToolDyn` and call it directly.
- **No `eprintln!()` or `println!()`** — use `tracing::info!()` for diagnostic output, even in test code [Source: project-context.md#Language-Specific Rules]
- **No `require_e2e()` helper pattern** — `return` inside a helper only exits the helper, not the test. Check `BMAD_E2E` inline at the top of each test function.
- **Clean up resources:** Every test must call `mcp_manager.shutdown().await` to avoid orphaned child processes

### Project Structure Notes

- `docs/` directory exists but is empty — `docs/mcp-servers.md` will be the first documentation file. **Write in English** (document_output_language).
- `tests/e2e/mod.rs` exists (stub with doc comments and TODO) — add `pub mod mcp_playwright;` to register the new test module. Verify `tests/e2e.rs` exists as the test binary entry point (create if missing with `mod e2e;`).
- `bmad-bot.yaml.example` is committed to git — changes here are visible to all users
- `README.md` — add a cross-reference to `docs/mcp-servers.md` so users can discover MCP documentation
- No changes to `src/` code are expected in this story — all infrastructure is in place from 9.1 and 9.2

### References

- [Source: _bmad-output/planning-artifacts/epics.md#L2197-2253] — Story 9.3 acceptance criteria and scope
- [Source: _bmad-output/planning-artifacts/epics.md#L2253-2271] — Epic 9 Summary (execution strategy, key decisions)
- [Source: _bmad-output/planning-artifacts/architect-brief-mcp-client-integration.md#L305-317] — Story 9.3 scope definition
- [Source: _bmad-output/planning-artifacts/architect-brief-mcp-client-integration.md#L317-329] — Risk assessment (Playwright environment, MCP crashes)
- [Source: _bmad-output/planning-artifacts/architect-brief-mcp-client-integration.md#L329-335] — Success criteria
- [Source: _bmad-output/planning-artifacts/architect-brief-mcp-client-integration.md#L164-187] — MCP config format (mcp_servers YAML schema)
- [Source: _bmad-output/planning-artifacts/architecture.md#L936-1006] — Complete Project Directory Structure
- [Source: _bmad-output/planning-artifacts/architecture.md#L882-914] — Test Mock Pattern
- [Source: _bmad-output/project-context.md#L112-121] — Testing Rules (E2E gating, manual launch only)
- [Source: _bmad-output/project-context.md#L121-164] — Code Quality & Style Rules
- [Source: _bmad-output/project-context.md#L187-201] — Critical Don't-Miss Rules
- [Source: _bmad-output/implementation-artifacts/9-1-mcp-server-lifecycle-management-config.md] — McpManager API, config structs, pipeline integration
- [Source: _bmad-output/implementation-artifacts/9-2-agent-integration-register-mcp-tools-on-session-build.md] — ToolConfigurator refactor, with_mcp, build_preamble update, extract_mcp_tool_names
- [Source: bmad-bot.yaml.example] — Current config template (no mcp_servers section yet)
- [Source: @playwright/mcp npm package] — Playwright MCP server, stdio transport, browser automation tools

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List