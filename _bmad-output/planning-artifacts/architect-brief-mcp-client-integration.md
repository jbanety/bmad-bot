---
type: architect-brief
from: Amelia (Dev Agent)
to: Architect & Product Owner
date: '2026-02-15'
subject: 'New Epic Request — MCP Client Integration for Dynamic Tool Discovery'
related_decision: 'Tool architecture (9 native rig tools) + AgentFactory centralized construction'
status: ready-for-po
triggered_by: 'Need to extend agent capabilities with external tools (browser automation, etc.) without maintaining custom implementations'
---

# Architect Brief: MCP Client Integration — Dynamic Tool Discovery

## Context

The BMAD Bot agent currently has 9 native tools implemented as rig `Tool` traits: `edit_file`, `read_file`, `grep`, `find_path`, `list_directory`, `git`, `terminal`, `ask_supervisor`, and rig's built-in `ThinkTool`. These tools cover core development workflows — file manipulation, code search, git operations, and shell access.

However, some use cases require capabilities that are outside the scope of what we want to maintain in-house. The most immediate example: **browser automation via Playwright** for verifying UI changes, running E2E visual checks, or interacting with web applications during story implementation.

The [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) is an open standard by Anthropic for connecting LLMs to external tool servers. A growing ecosystem of MCP servers exists — Playwright, databases, APIs, cloud services — each exposing tools via a standardized JSON-RPC protocol over stdio or HTTP/SSE transport.

Rather than building and maintaining a Playwright tool (or any other specialty tool) ourselves, we can **connect to external MCP servers as a client**, discover their tools at startup, and expose them to the rig agent as first-class tools — indistinguishable from native ones.

## Problem Summary

| Issue | Impact |
|-------|--------|
| Agent lacks browser/UI capabilities | Cannot verify frontend changes, take screenshots, or run browser-based E2E tests |
| Adding each new capability = new rig Tool impl | Maintenance burden grows linearly; domain expertise required for each tool |
| No standard protocol for external tool integration | Each integration is bespoke, tightly coupled |

## Proposed Solution: MCP Client with rig Tool Bridge

### Architecture Overview

```
Daemon startup
  └─ Read mcp_servers config
      └─ For each configured server:
          1. Spawn process (stdio) or connect (HTTP/SSE)
          2. MCP handshake (initialize)
          3. client.list_tools() → discover tool names + JSON schemas
          4. For each tool → create McpToolProxy (implements rig ToolDyn)
          5. Store proxies in McpToolRegistry

Session build (AgentFactory)
  └─ configure_agent_tools!(native tools... + mcp_registry.tools())
      └─ Agent sees all tools uniformly: edit_file, grep, browser_navigate, browser_screenshot...
```

### Key Design Decisions

#### 1. Discovery at startup, not per-session

MCP servers are spawned and tools discovered **once** when the daemon starts (or when config changes). Tool definitions are cached. The agent session receives a flat list of all available tools — native + MCP — at build time.

**Rationale:** MCP servers like Playwright have non-trivial startup time. Spawning per-session wastes resources and adds latency. A long-lived connection with tool caching is the standard MCP pattern.

#### 2. rig `ToolDyn` for dynamic tools (not `Tool`)

The rig `Tool` trait requires `const NAME: &'static str` — a compile-time constant. MCP tools are discovered at runtime, so names aren't known at compile time.

Two options:
- **`ToolDyn` trait** — rig's dynamic dispatch trait for tools. If available and sufficient, this is the clean path.
- **Leaked `&'static str`** — `Box::leak(name.into_boxed_str())` to satisfy the const requirement. Works but is a known Rust escape hatch.

**Recommendation:** Investigate rig's `ToolDyn` / dynamic tool support first. If it supports runtime-defined tool names and schemas, use it. Otherwise, the leak pattern is acceptable for a bounded number of MCP tools discovered at startup (not a real memory leak since they live for the process lifetime).

#### 3. `McpToolProxy` — generic bridge

A single struct that wraps any MCP tool and proxies calls:

```
McpToolProxy {
    name: String,
    description: String,
    input_schema: serde_json::Value,   // JSON Schema from MCP discovery
    client: Arc<McpClient>,            // shared rmcp client handle
}
```

- `definition()` → returns the MCP-provided JSON schema as-is (it's already in JSON Schema format, which is what rig expects)
- `call(args)` → forwards to `client.call_tool(name, args)` → returns text result
- Error handling: MCP errors mapped to rig tool errors

#### 4. Configuration in `bmad-bot.yaml`

```yaml
mcp_servers:
  - name: playwright
    command: npx
    args: ["-y", "@playwright/mcp"]
    transport: stdio           # stdio (default) | http
    enabled: true
    # env:                     # optional extra env vars for the spawned process
    #   DISPLAY: ":99"

  # - name: another-server
  #   command: /usr/local/bin/my-mcp-server
  #   transport: stdio
  #   enabled: false
```

- `enabled: false` allows disabling without removing config
- `transport: stdio` is the default and most common for local servers
- The daemon validates MCP server availability at startup (fail-fast if command not found)
- MCP server failures are **non-blocking** — if Playwright MCP fails to start, log a warning and continue without those tools. The native tools still work.

#### 5. Lifecycle management

- MCP server processes are spawned as child processes (`tokio::process::Command`) managed by the daemon
- Graceful shutdown: daemon sends MCP `close` notification before killing child processes
- Health monitoring: if an MCP server crashes mid-session, tool calls return clear errors; the agent can continue with other tools
- Restart policy: configurable (none / on-failure) — but not in MVP, just log and move on

### Scope Boundaries

| In Scope | Out of Scope |
|----------|--------------|
| MCP **client** — connecting to external MCP servers | MCP **server** — bmad-bot will NOT expose itself as an MCP server. Interaction with bmad-bot goes through BMAD agents, not MCP. |
| stdio transport (MVP) | HTTP/SSE transport (future, if needed) |
| Tool discovery + proxy via rig | MCP Resources and Prompts (not needed for tool augmentation) |
| Config-driven server list | Dynamic server addition at runtime |
| Playwright as primary use case | Any specific MCP server implementation |

### Crate Dependency

**`rmcp`** — the official Rust MCP SDK by Anthropic/ModelContextProtocol.
- Async-first (tokio)
- Client + Server support
- stdio + HTTP transport
- Well-maintained, high reputation
- Crate: [`rmcp`](https://crates.io/crates/rmcp) with features `client` + `transport-child-process`

### Impact on Existing Architecture

| Component | Change |
|-----------|--------|
| `bmad-bot.yaml` / `BotConfig` | New optional `mcp_servers` section |
| `src/mcp/` (new module) | `client.rs` (connection management), `registry.rs` (tool storage), `bridge.rs` (`McpToolProxy` impl) |
| `AgentFactory` / session builder | Accept additional tools from MCP registry alongside native tools |
| `build_preamble()` | Optionally include MCP tool descriptions in preamble for better agent awareness |
| Native tools | **Zero changes** — MCP tools are additive, native tools untouched |
| `configure_agent_tools!` macro | May need extension to accept dynamic tool list |

## Suggested Epic & Story Breakdown

**Epic 9: MCP Client Integration — Dynamic External Tool Discovery**

Integrate the Model Context Protocol (MCP) client to discover and proxy external tools at daemon startup, giving the BMAD agent access to browser automation (Playwright) and any future MCP-compatible tooling — without custom tool implementations.

### Story 9.1: MCP Client Connection & Lifecycle Management

**Scope:**
- New `src/mcp/mod.rs` + `src/mcp/client.rs`
- `McpClientManager` — spawns configured MCP servers, handles initialize handshake, graceful shutdown
- Config parsing: `mcp_servers` section in `BotConfig` (optional, defaults to empty)
- Startup validation: command existence check, timeout on handshake
- Non-blocking failures: log warning if a server fails to start, continue without it
- Integration with daemon graceful shutdown (SIGTERM kills child processes cleanly)
- Unit tests with mocked MCP responses

### Story 9.2: Tool Discovery & McpToolRegistry

**Scope:**
- New `src/mcp/registry.rs`
- `McpToolRegistry` — calls `list_tools()` on each connected server, stores tool definitions
- Deduplication: if two servers expose same tool name, prefix with server name (`playwright_navigate` vs `db_query`)
- Tool metadata cached for session lifetime
- Unit tests for discovery, dedup, caching

**Depends on:** 9.1

### Story 9.3: rig Tool Bridge — McpToolProxy

**Scope:**
- New `src/mcp/bridge.rs`
- `McpToolProxy` implementing rig's dynamic tool interface
- JSON Schema passthrough from MCP to rig (no manual schema rewriting)
- `call()` proxies to `rmcp` client, maps results (text content → String, errors → rig ToolError)
- Timeout handling on tool calls (configurable, default 30s)
- Unit tests with mocked MCP client

**Depends on:** 9.2

### Story 9.4: Agent Integration — Register MCP Tools on Session Build

**Scope:**
- Update `AgentFactory` / `SessionRunner` to accept `McpToolRegistry`
- Register MCP tools alongside native tools when building the agent
- Update preamble to mention available MCP tools if any are configured
- Update `configure_agent_tools!` macro if needed for dynamic tool list
- End-to-end validation: agent sees both native and MCP tools
- Unit tests for tool registration

**Depends on:** 9.3

### Story 9.5: Playwright MCP Validation & Documentation

**Scope:**
- Validate Playwright MCP server integration end-to-end
- Document `mcp_servers` configuration in README / project docs
- Example config for Playwright (`@playwright/mcp`)
- Verify agent can navigate, screenshot, click, fill forms via MCP tools
- Document how to add other MCP servers (generic instructions)
- Manual E2E test (not automated — requires browser environment)

**Depends on:** 9.4

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| rig `Tool` trait doesn't support dynamic names | Use `ToolDyn` or `Box::leak` pattern — both viable, investigate in Story 9.3 |
| MCP server crashes during agent session | Tool calls return errors gracefully; agent continues with remaining tools |
| MCP tool JSON schemas incompatible with rig | MCP and rig both use JSON Schema for tool parameters — high compatibility expected |
| Playwright requires display/browser environment | Document requirements; headless mode works in CI-like environments |
| rmcp crate breaking changes | Pin version in Cargo.toml; SDK is pre-1.0 but actively maintained |

## Success Criteria

- [ ] Agent can call Playwright MCP tools (navigate, screenshot) during a dev session
- [ ] MCP tools appear alongside native tools — agent uses them naturally without special prompting
- [ ] Zero impact on existing native tool functionality
- [ ] MCP server failures don't crash the daemon or block story processing
- [ ] Adding a new MCP server = one config entry, zero code changes