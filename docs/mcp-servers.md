# MCP Server Integration

## Overview

[MCP (Model Context Protocol)](https://modelcontextprotocol.io/) is an open standard that lets AI agents interact with external tools and services through a unified interface. BMAD Bot supports MCP servers as a **zero-code-change extension mechanism** — add a YAML config entry, restart the daemon, and the agent gains new capabilities.

MCP tools appear alongside native tools (edit_file, grep, terminal, etc.) in the agent's tool list. The agent calls them identically — no special syntax, no adapters, no glue code.

**Key benefits:**

- **Zero code changes** — add any MCP-compatible server via config
- **Graceful degradation** — if an MCP server fails to connect, the daemon continues with native tools only
- **Session resilience** — if an MCP server crashes mid-session, the agent gets an error for that tool call and continues working with all other tools

## Quick Start

1. Ensure `npx` is available on your PATH
2. Add to `bmad-bot.yaml`:

```yaml
mcp_servers:
  - name: playwright
    command: npx
    args: ["-y", "@playwright/mcp"]
    transport: stdio
    enabled: true
```

3. Restart the daemon: `bmad-bot start`
4. Check logs for tool discovery:

```
INFO MCP server connected server="playwright" tool_count=20
INFO MCP initialization complete connected=1 failed=0 total_tools=20
```

The agent now has browser automation tools available in dev sessions and code reviews.

## Configuration Reference

MCP servers are configured under the `mcp_servers` key in `bmad-bot.yaml`. Each entry defines one MCP server:

```yaml
mcp_servers:
  - name: playwright
    command: npx
    args: ["-y", "@playwright/mcp"]
    transport: stdio
    enabled: true
    timeout_secs: 30
```

### Field Reference

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | String | Yes | — | Human-readable identifier. Used in log messages for diagnostics. Must be unique across all configured servers. |
| `command` | String | Yes | — | Executable to spawn. Can be an absolute path or a command on `PATH` (e.g., `npx`, `node`, `/usr/local/bin/my-mcp-server`). |
| `args` | List of String | Yes | — | Arguments passed to the command. For npx-based servers, include `"-y"` to auto-confirm package installation. |
| `transport` | String | No | `stdio` | Transport protocol for communication. Currently only `stdio` is supported. |
| `enabled` | Boolean | No | `true` | Set to `false` to skip this server at startup without removing the config entry. Useful for temporarily disabling a server. |
| `timeout_secs` | Integer | No | `30` | Per-server MCP handshake timeout in seconds. The daemon waits this long for the initial MCP protocol handshake to complete. Increase for slow-starting servers or remote connections. |

### Minimal Config

Only `name`, `command`, and `args` are required. All other fields have sensible defaults:

```yaml
mcp_servers:
  - name: playwright
    command: npx
    args: ["-y", "@playwright/mcp"]
```

### No MCP Servers

If `mcp_servers` is absent or empty, the daemon behaves identically to pre-MCP versions. No MCP connections are attempted, and the agent uses only native tools.

## Playwright Setup

[Playwright MCP](https://github.com/microsoft/playwright-mcp) provides browser automation tools — navigate pages, click elements, fill forms, take screenshots, and more.

### Prerequisites

- **Node.js** (v18+) with `npx` on PATH
- **@playwright/mcp** — installed automatically via `npx -y` on first run
- **Browser environment** — a display server (X11, Wayland, macOS) or headless mode

### Configuration

```yaml
mcp_servers:
  - name: playwright
    command: npx
    args: ["-y", "@playwright/mcp"]
    transport: stdio
    enabled: true
```

### Headless Mode

For servers without a display (CI environments, remote servers, containers), run Playwright in headless mode by adding the `--headless` flag:

```yaml
mcp_servers:
  - name: playwright
    command: npx
    args: ["-y", "@playwright/mcp", "--headless"]
    transport: stdio
    enabled: true
```

Alternatively, set Playwright environment variables before starting the daemon:

```bash
export PLAYWRIGHT_CHROMIUM_HEADLESS=1
bmad-bot start
```

### Expected Tools

When Playwright MCP connects successfully, it discovers approximately 18 browser automation tools (exact count varies by `@playwright/mcp` version):

| Tool | Description |
|------|-------------|
| `browser_navigate` | Navigate to a URL |
| `browser_screenshot` | Capture a page screenshot |
| `browser_click` | Click an element on the page |
| `browser_fill` | Fill a form field with text |
| `browser_type` | Type text into an element |
| `browser_snapshot` | Get an accessibility snapshot of the page |
| `browser_select_option` | Select a dropdown option |
| `browser_hover` | Hover over an element |
| `browser_press_key` | Press a keyboard key |
| `browser_handle_dialog` | Handle browser dialogs (alert, confirm, prompt) |
| `browser_wait_for` | Wait for an element or condition |
| `browser_drag` | Drag and drop between elements |
| `browser_console_messages` | Get browser console output |
| `browser_network_requests` | Get network request log |
| `browser_file_upload` | Upload files to a file input |
| `browser_close` | Close the browser page |
| `browser_resize` | Resize the browser window |
| `browser_tabs` | Manage browser tabs (list, create, close, select) |

The daemon logs all discovered tool names at startup:

```
INFO MCP server connected server="playwright" tool_count=20
```

## Adding a New MCP Server

Adding a new MCP server requires **zero code changes** — only a YAML config entry and a daemon restart.

### Step-by-Step

1. **Find or build an MCP server** — any server implementing the [MCP specification](https://modelcontextprotocol.io/) with stdio transport will work.

2. **Add a config entry** to `bmad-bot.yaml`:

```yaml
mcp_servers:
  - name: my-new-server
    command: npx
    args: ["-y", "@example/my-mcp-server"]
    transport: stdio
    enabled: true
    timeout_secs: 60  # increase if the server is slow to start
```

3. **Restart the daemon:**

```bash
bmad-bot start
```

4. **Check the logs** for tool discovery:

```
INFO MCP server connected server="my-new-server" tool_count=5
```

5. **Verify in agent sessions** — the agent's preamble will list the new MCP tools alongside native tools, and the agent can call them like any other tool.

### Multiple Servers

You can configure as many MCP servers as needed. Each runs as a separate child process:

```yaml
mcp_servers:
  - name: playwright
    command: npx
    args: ["-y", "@playwright/mcp"]
    transport: stdio
    enabled: true

  - name: database-explorer
    command: npx
    args: ["-y", "@example/db-mcp-server"]
    transport: stdio
    enabled: true
    timeout_secs: 60
```

All discovered tools from all servers appear in the agent's tool list.

## Disabling a Server

To temporarily disable an MCP server without removing its configuration, set `enabled: false`:

```yaml
mcp_servers:
  - name: playwright
    command: npx
    args: ["-y", "@playwright/mcp"]
    transport: stdio
    enabled: false  # Server will be skipped at startup
```

**Important:** Configuration is loaded once at daemon startup and shared as an immutable `Arc<BotConfig>` — it is never hot-reloaded. You must **restart the daemon** for any config changes to take effect:

```bash
bmad-bot start
```

## How It Works

For advanced users, here is the technical flow of MCP integration:

### Startup

1. The daemon reads `mcp_servers` from `bmad-bot.yaml`
2. `McpManager::init()` iterates over enabled servers
3. For each server: spawn child process → MCP handshake (initialize exchange) → `list_tools()` to discover available tools
4. Failures are logged via `tracing::warn!()` and skipped — the daemon continues with whatever servers connected successfully
5. `McpManager` is wrapped in `Arc` and shared with `SessionRunner`, `ReviewRunner`, and `StoryPipeline`

### Agent Session Build

1. When building an agent (dev session, code review, or supervisor), `tools_for_builder()` clones each server's discovered tools and `ServerSink`
2. The `ToolConfigurator` chains `.rmcp_tools(tools, sink)` after native `.tool()` calls — rig's `McpTool` wrapper handles serialization and proxying
3. `extract_mcp_tool_names()` provides tool names for the system prompt preamble
4. The agent sees both native and MCP tools in its tool list and calls them identically

### Tool Invocation

When the agent calls an MCP tool:
1. Rig's `McpTool` serializes the call arguments to JSON
2. The call is sent to the MCP server via the `ServerSink` (stdio transport)
3. The MCP server processes the request and returns a result
4. Rig deserializes the response and returns it to the agent as text content

### Shutdown

On daemon exit (`Ctrl+C` or `SIGTERM`), `McpManager::shutdown()` sends MCP close notifications to all connected servers and waits for child processes to exit.

## Troubleshooting

### Common Issues

| Symptom | Cause | Solution |
|---------|-------|----------|
| `MCP server 'X' failed to spawn` | Command not found on PATH | Install the prerequisite (e.g., `npm install -g npx`) or use an absolute path for `command` |
| `MCP server 'X' handshake timed out` | Server took too long to initialize | Increase `timeout_secs` in config (e.g., `60` or `120`). Check if the server requires first-time package download. |
| `MCP server 'X' handshake failed` | Protocol mismatch or server error | Check the MCP server's own logs. Ensure the server implements MCP protocol correctly. |
| `MCP server 'X' tool discovery failed` | Server connected but `list_tools()` failed | Verify the MCP server implements the `tools/list` method. Check server logs. |
| `MCP server 'X' shutdown error` | Server process already exited | Usually harmless — the server may have crashed before shutdown was called. Check earlier log entries for errors. |
| No MCP tools in agent preamble | Server failed to connect or `enabled: false` | Check daemon startup logs for MCP initialization messages. Verify `enabled: true`. |
| MCP tool call returns error mid-session | Server crashed or became unresponsive | The agent receives the error and continues with native tools. The session is **not** terminated. Restart the daemon to reconnect. |

### Verifying Connection

Check the daemon logs at startup for MCP initialization messages:

```
# Successful connection
INFO MCP server connected server="playwright" tool_count=20
INFO MCP initialization complete connected=1 failed=0 total_tools=20

# Failed connection (daemon continues without the server)
WARN MCP server failed — skipping server="playwright" error="MCP server 'playwright' failed to spawn: ..."
INFO MCP initialization complete connected=0 failed=1 total_tools=0

# No servers configured
INFO MCP initialization complete — no servers configured
```

### Server Crash Mid-Session

If an MCP server crashes while an agent session is active:

- The **next tool call** to that server returns an error message to the agent
- The agent **continues working** with all native tools and any other MCP servers that are still running
- The **session is not terminated** — error isolation is handled by rig's `McpTool` wrapper
- To restore the crashed MCP server, **restart the daemon**

This is by design: MCP failures are non-blocking and never crash the daemon or terminate agent sessions.

## Supported Transports

Currently, only the **stdio** transport is supported. The daemon spawns MCP servers as child processes and communicates via standard input/output.

```yaml
transport: stdio  # The only supported value currently
```

Future versions may add support for additional transports as the MCP ecosystem evolves:

- **SSE (Server-Sent Events)** — for remote MCP servers over HTTP
- **WebSocket** — for bidirectional streaming connections

These will be added when the underlying `rmcp` library gains support for them.