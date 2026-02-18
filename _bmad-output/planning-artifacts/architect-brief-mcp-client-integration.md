---
type: architect-brief
from: Amelia (Dev Agent)
to: Architect & Product Owner
date: '2026-02-15'
updated: '2026-02-18'
subject: 'New Epic Request — MCP Client Integration for Dynamic Tool Discovery'
related_decision: 'Tool architecture (9 native rig tools) + AgentFactory centralized construction'
status: ready-for-implementation
triggered_by: 'Need to extend agent capabilities with external tools (browser automation, etc.) without maintaining custom implementations'
---

# Architect Brief: MCP Client Integration — Dynamic Tool Discovery

## Context

The BMAD Bot agent currently has 9 native tools implemented as rig `Tool` traits: `edit_file`, `read_file`, `grep`, `find_path`, `list_directory`, `git`, `terminal`, `ask_supervisor`, and rig's built-in `ThinkTool`. These tools cover core development workflows — file manipulation, code search, git operations, and shell access.

However, the autonomous agent needs to be able to **verify its own work**. When implementing frontend stories, the agent must be able to launch a dev server and check that the application actually works. This requires browser automation capabilities that are outside the scope of what we want to maintain in-house.

The [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) is an open standard by Anthropic for connecting LLMs to external tool servers. A growing ecosystem of MCP servers exists — Playwright, databases, APIs, cloud services — each exposing tools via a standardized JSON-RPC protocol over stdio or HTTP/SSE transport.

Rather than building and maintaining a Playwright tool (or any other specialty tool) ourselves, we can **connect to external MCP servers as a client**, discover their tools at startup, and expose them to the rig agent as first-class tools — indistinguishable from native ones.

## Problem Summary

| Issue | Impact |
|-------|--------|
| Agent lacks browser/UI capabilities | Cannot verify frontend changes, take screenshots, or confirm an app runs correctly after implementation |
| Adding each new capability = new rig Tool impl | Maintenance burden grows linearly; domain expertise required for each tool |
| No standard protocol for external tool integration | Each integration is bespoke, tightly coupled |

## Architecture Spike Findings (2026-02-18)

> **Spike conducted by:** Winston (Architect)
> **Conclusion:** rig already provides native MCP client support. The originally proposed custom bridge layer is unnecessary.

### Key Discoveries

#### 1. `ToolDyn` exists and resolves the dynamic naming question

The `ToolDyn` trait in rig provides runtime-defined tool names (no `const NAME` required). Every `Tool` automatically implements `ToolDyn` via a blanket impl. **The `ToolDyn` vs `Box::leak` question is a non-issue.**

```rust
// rig-core/src/tool/mod.rs
pub trait ToolDyn: WasmCompatSend + WasmCompatSync {
    fn name(&self) -> String;
    fn definition<'a>(&'a self, prompt: String) -> WasmBoxedFuture<'a, ToolDefinition>;
    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>>;
}
```

#### 2. rig has a built-in `McpTool` behind the `rmcp` feature flag

rig already provides `McpTool` — a struct that wraps an MCP tool definition and a `ServerSink` client handle, implementing `ToolDyn`. This is exactly what the brief originally proposed to build as `McpToolProxy`.

```rust
// rig-core/src/tool/mod.rs (behind #[cfg(feature = "rmcp")])
pub struct McpTool {
    definition: rmcp::model::Tool,
    client: rmcp::service::ServerSink,
}

impl McpTool {
    pub fn from_mcp_server(definition: rmcp::model::Tool, client: rmcp::service::ServerSink) -> Self { ... }
}

impl ToolDyn for McpTool { ... }  // Full implementation: definition(), call(), error mapping
```

#### 3. `AgentBuilder` has native `.rmcp_tools()` methods

Both `AgentBuilder` and `AgentBuilderSimple` provide methods to register MCP tools directly:

```rust
// rig-core/src/agent/builder.rs
impl<M: CompletionModel> AgentBuilder<M> {
    pub fn rmcp_tool(self, tool: rmcp::model::Tool, client: rmcp::service::ServerSink) -> AgentBuilderSimple<M> { ... }
    pub fn rmcp_tools(self, tools: Vec<rmcp::model::Tool>, client: rmcp::service::ServerSink) -> AgentBuilderSimple<M> { ... }
}

impl<M: CompletionModel> AgentBuilderSimple<M> {
    pub fn rmcp_tools(mut self, tools: Vec<rmcp::model::Tool>, client: rmcp::service::ServerSink) -> Self { ... }
    pub fn tools(mut self, tools: Vec<Box<dyn ToolDyn>>) -> Self { ... }  // Also accepts any ToolDyn
}
```

#### 4. What this eliminates from the original brief

| Originally Proposed | Status |
|---------------------|--------|
| `src/mcp/bridge.rs` — `McpToolProxy` implementing `ToolDyn` | **Unnecessary** — rig's `McpTool` does this |
| JSON Schema conversion MCP → rig | **Unnecessary** — handled by rig's `From<rmcp::model::Tool> for ToolDefinition` |
| Error mapping MCP → rig `ToolError` | **Unnecessary** — handled by rig's `McpTool::call()` |
| Custom tool registry with dedup logic | **Simplified** — store `ServerSink` + tool list per server, pass to `.rmcp_tools()` |
| `configure_agent_tools!` macro extension | **Simplified** — use `AgentBuilderSimple::rmcp_tools()` after native tools |

## Proposed Solution: MCP Client with rig's Native Support

### Architecture Overview (Updated)

```
Daemon startup
  └─ Read mcp_servers config from bmad-bot.yaml
      └─ For each configured server:
          1. Spawn process via rmcp (stdio transport)
          2. MCP handshake (initialize) — handled by rmcp
          3. client.list_tools() → Vec<rmcp::model::Tool>
          4. Store ServerSink + tool list in McpManager

Session build (AgentFactory)
  └─ AgentConfigurator chains:
      1. .tool(native_tool_1).tool(native_tool_2)...           → AgentBuilderSimple
      2. .rmcp_tools(pw_tools, pw_sink)                        → server 1 (e.g. Playwright)
         .rmcp_tools(db_tools, db_sink)                        → server 2 (if configured)
         ... one .rmcp_tools() call per MCP server              
      3. .build()                                              → Agent<M>
      └─ Agent sees all tools uniformly: edit_file, grep, browser_navigate, browser_screenshot...
```

### Key Design Decisions

#### 1. Discovery at startup, not per-session

MCP servers are spawned and tools discovered **once** when the daemon starts (or when config changes). Tool definitions are cached. The agent session receives a flat list of all available tools — native + MCP — at build time.

**Rationale:** MCP servers like Playwright have non-trivial startup time. Spawning per-session wastes resources and adds latency. A long-lived connection with tool caching is the standard MCP pattern.

#### 2. Use rig's native `rmcp` feature — no custom bridge

~~The original brief proposed building `McpToolProxy` to bridge MCP tools into rig.~~

**Updated decision:** Enable the `rmcp` feature flag on `rig-core` and use the built-in `McpTool` + `AgentBuilder::rmcp_tools()`. Zero custom bridging code needed.

**Dependencies to add to `Cargo.toml`:**
- `rig-core` with feature `rmcp` enabled
- `rmcp` with features `client` + `transport-child-process` (for spawning MCP server processes)

**Version constraint:** rig-core currently depends on `rmcp = "0.13"`. The latest rmcp is 0.16. Our `Cargo.toml` must pin a version compatible with rig's dependency (currently 0.13). If a newer rmcp version is needed, the rig fork must be updated first.

#### 3. `McpManager` — lightweight connection + lifecycle manager

A single struct that owns the MCP server processes and their client handles:

```rust
pub struct McpManager {
    servers: Vec<McpServerHandle>,
}

struct McpServerHandle {
    name: String,
    sink: rmcp::service::ServerSink,      // client handle for tool calls
    tools: Vec<rmcp::model::Tool>,         // discovered tools
    // child process managed by rmcp's transport layer
}
```

- `McpManager::init(config) → Result<Self>` — spawns all configured servers, runs handshake + list_tools
- `McpManager::tools_for_builder() → Vec<(Vec<rmcp::model::Tool>, ServerSink)>` — returns one tuple per connected MCP server, ready for chaining `.rmcp_tools()` calls on the agent builder
- `McpManager::shutdown()` — graceful close of all MCP connections

Note: `McpTool` derives `Clone` and `ServerSink` is `Clone`, so sharing handles across sessions is safe.

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

- MCP server processes are spawned via rmcp's transport layer (which wraps `tokio::process::Command`)
- Graceful shutdown: `McpManager` sends MCP `close` notification before dropping connections
- Health monitoring: if an MCP server crashes mid-session, tool calls return clear errors via rig's `McpTool`; the agent can continue with other tools
- Restart policy: not in MVP — just log and move on

#### 6. `AgentConfigurator` adaptation

The current `AgentConfigurator` trait methods return `Agent<M>` directly. They need to be updated to accept optional MCP tools.

**Recommended approach — modify `ToolConfigurator` struct, NOT the trait:**

The `AgentConfigurator` trait signature stays unchanged. Instead, add an MCP field to the existing `ToolConfigurator` struct:

```rust
pub struct ToolConfigurator<T> {
    pub tools: T,
    pub mcp_servers: Vec<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)>,
}
```

In each `configure_*` impl, after chaining native `.tool()` calls, chain `.rmcp_tools()` per server before `.build()`:

```rust
fn configure_anthropic(self, builder: AgentBuilder<M>) -> Agent<M> {
    let (t1, t2, ...) = self.tools;
    let mut simple = builder.tool(t1).tool(t2)...;  // → AgentBuilderSimple
    for (tools, sink) in self.mcp_servers {
        simple = simple.rmcp_tools(tools, sink);
    }
    simple.build()
}
```

The `configure_agent_tools!` macro initializes `mcp_servers: vec![]` by default. A builder method allows injection:

```rust
impl<T> ToolConfigurator<T> {
    pub fn with_mcp(mut self, servers: Vec<(Vec<rmcp::model::Tool>, ServerSink)>) -> Self {
        self.mcp_servers = servers;
        self
    }
}
```

**Call site change is minimal:**
```rust
let configurator = configure_agent_tools!(git, read_file, edit_file, ...)
    .with_mcp(mcp_manager.tools_for_builder());
factory.build(role, &preamble, configurator).await?
```

This approach keeps the `AgentConfigurator` trait stable, existing call sites without MCP work unchanged, and the `NoTools` configurator is unaffected.

### Scope Boundaries

| In Scope | Out of Scope |
|----------|--------------|
| MCP **client** — connecting to external MCP servers | MCP **server** — bmad-bot will NOT expose itself as an MCP server |
| stdio transport (MVP) | HTTP/SSE transport (future, if needed) |
| Tool discovery via rig's native rmcp support | Custom tool bridge / schema conversion (not needed) |
| Config-driven server list | Dynamic server addition at runtime |
| Playwright as primary validation use case | Any specific MCP server implementation |

### Crate Dependencies

- **`rmcp`** — the official Rust MCP SDK by Anthropic/ModelContextProtocol
  - Async-first (tokio)
  - Client + Server support
  - stdio + HTTP transport
  - Crate: [`rmcp`](https://crates.io/crates/rmcp) with features `client` + `transport-child-process`

- **`rig-core`** — enable the `rmcp` feature flag (currently not enabled)
  - Provides `McpTool`, `AgentBuilder::rmcp_tools()`, `ToolDyn` integration

### Impact on Existing Architecture

| Component | Change |
|-----------|--------|
| `Cargo.toml` | Add `rmcp` dependency; enable `rmcp` feature on `rig-core` |
| `bmad-bot.yaml` / `BotConfig` | New optional `mcp_servers` section |
| `src/mcp/` (new module) | `mod.rs` + `manager.rs` — connection lifecycle + tool discovery |
| `AgentConfigurator` trait | Accept optional MCP tool data, chain `.rmcp_tools()` before `.build()` |
| `build_preamble()` | Optionally list available MCP tools for agent awareness |
| Native tools | **Zero changes** — MCP tools are additive, native tools untouched |

## Epic & Story Breakdown (Updated)

**Epic 9: MCP Client Integration — Dynamic External Tool Discovery**

Integrate the Model Context Protocol (MCP) client to connect to external MCP servers at daemon startup, giving the BMAD agent access to browser automation (Playwright) and any future MCP-compatible tooling — leveraging rig's native rmcp support.

### Story 9.1: MCP Server Lifecycle Management & Config

**Scope:**
- Add `rmcp` to `Cargo.toml`; enable `rmcp` feature on `rig-core`
- New `src/mcp/mod.rs` + `src/mcp/manager.rs`
- `McpManager` — spawns configured MCP servers via rmcp's stdio transport, handles initialize handshake, graceful shutdown
- Config parsing: `mcp_servers` section in `BotConfig` (optional, defaults to empty)
- Startup validation: command existence check, timeout on handshake
- Non-blocking failures: log warning if a server fails to start, continue without it
- `list_tools()` on each connected server — store `ServerSink` + tool definitions
- Integration with daemon graceful shutdown (close MCP connections cleanly)
- Unit tests with mocked MCP server

### Story 9.2: Agent Integration — Register MCP Tools on Session Build

**Scope:**
- Update `AgentConfigurator` trait to accept MCP tool data alongside native tools
- Chain `.rmcp_tools(tools, sink)` on `AgentBuilderSimple` before `.build()`
- Pass `McpManager` (or its tool data) through `AgentFactory` to the configurator
- Update preamble to mention available MCP tools if any are configured
- End-to-end validation: agent sees both native and MCP tools, can call them
- Unit tests for tool registration

**Depends on:** 9.1

### Story 9.3: Playwright MCP Validation & Documentation

**Scope:**
- Validate Playwright MCP server integration end-to-end
- Document `mcp_servers` configuration in README / project docs
- Example config for Playwright (`@playwright/mcp`)
- Verify agent can navigate, screenshot, click, fill forms via MCP tools
- Document how to add other MCP servers (generic instructions)
- Manual E2E test (not automated — requires browser environment)

**Depends on:** 9.2

## Risk Assessment (Updated)

| Risk | Status | Mitigation |
|------|--------|------------|
| ~~rig `Tool` trait doesn't support dynamic names~~ | **Resolved** — `ToolDyn` exists, `McpTool` implements it | N/A |
| ~~MCP tool JSON schemas incompatible with rig~~ | **Resolved** — rig handles conversion natively | N/A |
| rig's `rmcp` feature flag on our fork | Low risk | Our fork tracks upstream; feature is stable in rig |
| MCP server crashes during agent session | Moderate | Tool calls return errors gracefully via rig's `McpTool`; agent continues with remaining tools |
| Playwright requires display/browser environment | Low | Document requirements; headless mode works in CI-like environments |
| rmcp crate breaking changes | Low | Pin version in Cargo.toml; SDK is actively maintained |
| `AgentConfigurator` refactor breaks existing tool registration | Low | Existing `ToolConfigurator` impls are modified to pass through MCP data; native tool registration is unchanged |

## Success Criteria

- [ ] Agent can call Playwright MCP tools (navigate, screenshot) during a dev session
- [ ] MCP tools appear alongside native tools — agent uses them naturally without special prompting
- [ ] Zero impact on existing native tool functionality
- [ ] MCP server failures don't crash the daemon or block story processing
- [ ] Adding a new MCP server = one config entry, zero code changes