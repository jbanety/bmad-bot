# Story 9.2: Agent Integration — Register MCP Tools on Session Build

Status: ready-for-dev

## Story

As a dev agent,
I want MCP-discovered tools registered alongside my native tools when my session is built,
So that I can use browser automation and other external tools identically to edit_file, grep, terminal, etc.

## Acceptance Criteria

1. **Given** `ToolConfigurator` in `src/llm/agent_factory.rs` currently has a single `tools` field **When** Story 9.2 is complete **Then** `ToolConfigurator` has an additional `mcp_servers: Vec<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)>` field **And** the `configure_agent_tools!` macro initializes `mcp_servers` to `vec![]` by default **And** `ToolConfigurator` exposes a `with_mcp(self, servers: Vec<(Vec<rmcp::model::Tool>, ServerSink)>) -> Self` method for injection

2. **Given** `McpManager` has discovered tools from one or more MCP servers **When** a `ToolConfigurator` is created via `configure_agent_tools!` and `.with_mcp(mcp_manager.tools_for_builder())` **Then** each `configure_*` impl (`configure_anthropic`, `configure_openai_responses`, `configure_openai_completions`) chains `.rmcp_tools(tools, sink)` once per MCP server after the native `.tool()` calls and before `.build()`

3. **Given** no MCP servers are configured (empty vec) **When** the `configure_*` methods execute **Then** behavior is identical to before — no `.rmcp_tools()` calls, native tools only **And** the `AgentConfigurator` trait signature is unchanged

4. **Given** the `ToolConfigurator` impl for 1-tool tuple (supervisor/architect use case in `ToolConfigurator<(T1,)>`) **When** MCP tools are injected via `.with_mcp()` **Then** MCP tools are also chained for the 1-tool configurator **And** the supervisor/architect agent gains MCP tools if configured

5. **Given** `AgentFactory::build()` in `src/llm/agent_factory.rs` is called **When** `McpManager` is available **Then** the call sites in `src/session/runner.rs`, `src/review/mod.rs`, and `src/supervisor/architect.rs` pass MCP data through to the configurator via `.with_mcp()`

6. **Given** MCP tools are registered on an agent **When** `build_preamble()` in `src/session/dev_agent.rs` generates the system prompt **Then** the preamble's tool list section includes the names of available MCP tools (e.g., `browser_navigate`, `browser_screenshot`, etc.) **And** if no MCP tools are configured, the preamble is unchanged

7. **Given** an agent session is started with MCP tools registered **When** the agent receives its tool definitions **Then** both native tools (edit_file, grep, etc.) and MCP tools (browser_navigate, etc.) appear in the tool list **And** the agent can call MCP tools — calls are proxied via rig's `McpTool` to the MCP server transparently

8. **Given** all changes are complete **When** `cargo test` is run **Then** all existing tests pass unchanged (zero regression on native tool registration) **And** new unit tests verify: ToolConfigurator with empty mcp_servers behaves identically, `with_mcp` builder method works, preamble includes MCP tool names when provided

## Tasks / Subtasks

- [ ] Task 1: Add `mcp_servers` field to `ToolConfigurator` and update macro (AC: #1, #3)
  - [ ] 1.1 In `src/llm/agent_factory.rs`, add field to `ToolConfigurator<T>`: `pub mcp_servers: Vec<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)>`
  - [ ] 1.2 Update `configure_agent_tools!` macro to initialize `mcp_servers: vec![]` alongside `tools`:
        ```
        $crate::llm::agent_factory::ToolConfigurator {
            tools: ($($tool,)+),
            mcp_servers: vec![],
        }
        ```
  - [ ] 1.3 Implement `with_mcp()` builder method on `ToolConfigurator<T>` (generic over T — works for any arity):
        ```
        impl<T> ToolConfigurator<T> {
            pub fn with_mcp(mut self, servers: Vec<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)>) -> Self {
                self.mcp_servers = servers;
                self
            }
        }
        ```
  - [ ] 1.4 Add unconditional imports at top of `agent_factory.rs`: `rmcp` is a direct dependency in `Cargo.toml` (added in Story 9.1) — no `#[cfg(feature)]` gate needed on our crate. Add: `use rmcp::model::Tool as McpToolDef; use rmcp::service::ServerSink;` (or use fully qualified paths in the struct field type)

- [ ] Task 2: Update 9-tool `AgentConfigurator` impl to chain MCP tools (AC: #2, #3)
  - [ ] 2.1 In the `impl AgentConfigurator for ToolConfigurator<(T1..T9)>`, refactor each `configure_*` method to chain `.rmcp_tools()` after native `.tool()` calls:
        ```
        fn configure_anthropic(self, builder: AgentBuilder<...>) -> Agent<...> {
            let (t1, t2, t3, t4, t5, t6, t7, t8, t9) = self.tools;
            let mut simple = builder.tool(t1).tool(t2).tool(t3).tool(t4)
                .tool(t5).tool(t6).tool(t7).tool(t8).tool(t9);
            for (tools, sink) in self.mcp_servers {
                simple = simple.rmcp_tools(tools, sink);
            }
            simple.build()
        }
        ```
  - [ ] 2.2 Apply the same pattern to `configure_openai_responses` and `configure_openai_completions`
  - [ ] 2.3 When `mcp_servers` is empty, the `for` loop body never executes — zero behavioral change, no `.rmcp_tools()` calls

- [ ] Task 3: Update 1-tool `AgentConfigurator` impl to chain MCP tools (AC: #4)
  - [ ] 3.1 In the `impl AgentConfigurator for ToolConfigurator<(T1,)>`, apply the same pattern:
        ```
        fn configure_anthropic(self, builder: AgentBuilder<...>) -> Agent<...> {
            let (t1,) = self.tools;
            let mut simple = builder.tool(t1);
            for (tools, sink) in self.mcp_servers {
                simple = simple.rmcp_tools(tools, sink);
            }
            simple.build()
        }
        ```
  - [ ] 3.2 Apply to all three `configure_*` methods
  - [ ] 3.3 This gives the supervisor/architect agent MCP tools when configured

- [ ] Task 4: Update `build_preamble()` to accept optional MCP tool names (AC: #6)
  - [ ] 4.1 Change signature from `pub fn build_preamble() -> String` to `pub fn build_preamble(mcp_tool_names: &[String]) -> String`
  - [ ] 4.2 In the `## Tools` section, if `mcp_tool_names` is non-empty, append: `"\nYou also have access to MCP tools: {names_joined}. Use them like any other tool."` after the existing tool list line
  - [ ] 4.3 When `mcp_tool_names` is empty, the preamble output is byte-identical to current
  - [ ] 4.4 Update existing tests for `build_preamble` to pass `&[]` — they must still pass unchanged

- [ ] Task 5: Update `SessionRunner` to pass MCP data through (AC: #5, #6)
  - [ ] 5.1 Update `SessionRunner::build_preamble()` wrapper (L711-713) to handle MCP tool names internally — this centralizes the preamble MCP logic so `build_agent_for_role()` stays simple:
        ```
        fn build_preamble(&self, _story: &StoryInfo) -> Result<String, ProviderError> {
            let mcp_data = self.mcp_manager.tools_for_builder();
            let mcp_names = crate::mcp::extract_mcp_tool_names(&mcp_data);
            Ok(dev_agent::build_preamble(&mcp_names))
        }
        ```
  - [ ] 5.2 In `SessionRunner::build_agent_for_role()` (L679-701), chain `.with_mcp()` using a single call to `tools_for_builder()`:
        ```
        let mcp_data = self.mcp_manager.tools_for_builder();
        self.agent_factory
            .build(
                role,
                &preamble,
                crate::configure_agent_tools!(
                    git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor,
                    ThinkTool
                )
                .with_mcp(mcp_data),
            )
            .await
        ```
  - [ ] 5.3 Note: `build_preamble()` and `build_agent_for_role()` each call `tools_for_builder()` independently — this is acceptable since agent builds are infrequent and the clone cost is trivial

- [ ] Task 6: Update `ReviewRunner` to pass MCP data through (AC: #5)
  - [ ] 6.1 In `ReviewRunner::run_inner()` (around L300-325), retrieve MCP tool names for preamble and chain `.with_mcp()`:
        ```
        let mcp_data = self.mcp_manager.tools_for_builder();
        let mcp_tool_names = extract_mcp_tool_names(&mcp_data);
        let preamble = dev_agent::build_preamble(&mcp_tool_names);
        // ...
        crate::configure_agent_tools!(
            git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor,
            ThinkTool
        )
        .with_mcp(mcp_data),
        ```
  - [ ] 6.2 Ensure `self.mcp_manager` is accessible (stored in ReviewRunner from Story 9.1)

- [ ] Task 7: Update `ArchitectSession` to pass MCP data through (AC: #4, #5)
  - [ ] 7.1 Add `mcp_manager: Arc<McpManager>` field to `ArchitectSession` struct
  - [ ] 7.2 Update `ArchitectSession::new_with_factory()` to accept `mcp_manager: Arc<McpManager>` and store it
  - [ ] 7.3 Update `ArchitectSession::new()` (legacy constructor used in tests) to pass `Arc::new(McpManager::empty())` as default — preserves backward compatibility for existing tests (`test_architect_session_missing_agent_file`, `test_architect_session_missing_api_key`, `test_architect_session_unsupported_provider`):
        ```
        pub fn new(config: &BotConfig) -> Result<Self, ArchitectSessionError> {
            Self::new_with_factory(config, None, Arc::new(McpManager::empty()))
        }
        ```
  - [ ] 7.4 Update `AskSupervisor::with_architect_from_config()` to accept and forward `mcp_manager: Arc<McpManager>` to `ArchitectSession::new_with_factory()`
  - [ ] 7.5 Update `SessionRunner::create_tools()` and `ReviewRunner::create_tools()` to pass `Arc::clone(&self.mcp_manager)` when constructing `AskSupervisor`
  - [ ] 7.6 In `ArchitectSession::ask()` (L336-360), chain `.with_mcp()` and update preamble using single-call pattern:
        ```
        let mcp_data = self.mcp_manager.tools_for_builder();
        let mcp_tool_names = extract_mcp_tool_names(&mcp_data);
        let preamble = build_preamble(&mcp_tool_names);
        // ...
        crate::configure_agent_tools!(read_tool)
            .with_mcp(mcp_data),
        ```

- [ ] Task 8: Create `extract_mcp_tool_names` free function in `src/mcp/mod.rs` (AC: #6)
  - [ ] 8.1 Create a public free function in `src/mcp/mod.rs` that extracts tool names from the `tools_for_builder()` output. This is a free function (not a method on McpManager) because callers already have the data from `tools_for_builder()` — avoids a second traversal of McpManager internals:
        ```
        /// Extract tool names from MCP server data returned by `McpManager::tools_for_builder()`.
        pub fn extract_mcp_tool_names(
            servers: &[(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)]
        ) -> Vec<String> {
            servers.iter()
                .flat_map(|(tools, _)| tools.iter().map(|t| t.name.to_string()))
                .collect()
        }
        ```
  - [ ] 8.2 Re-export from `src/mcp/mod.rs` so callers use `crate::mcp::extract_mcp_tool_names()`

- [ ] Task 9: Write unit tests (AC: #8)
  - [ ] 9.1 Test `configure_agent_tools!` macro produces `ToolConfigurator` with empty `mcp_servers`
  - [ ] 9.2 Test `with_mcp()` sets the `mcp_servers` field (mock data — construct fake `Vec` if possible, or test at struct level)
  - [ ] 9.3 Test `build_preamble(&[])` output is identical to current hardcoded output
  - [ ] 9.4 Test `build_preamble(&["browser_navigate".into(), "browser_screenshot".into()])` includes MCP tool names
  - [ ] 9.5 Test `build_preamble` with MCP tools still contains all existing assertions (edit_file, read_file, grep, etc.)
  - [ ] 9.6 Test `extract_mcp_tool_names(&[])` returns empty vec
  - [ ] 9.7 Test `extract_mcp_tool_names` with mock data returns expected tool names
  - [ ] 9.8 Verify ALL existing tests pass — `cargo test` must show zero regressions. Critical tests to watch: `test_build_preamble_contains_tool_rules`, `test_build_preamble_contains_english_override`, `test_no_tools_configurator`, all `test_agent_factory_build_*` tests, `test_architect_session_*` tests (must work with legacy `new()` constructor)

## Dev Notes

### Architecture Patterns & Constraints

- **AgentConfigurator trait is UNCHANGED.** Only `ToolConfigurator` struct and its impls are modified. The `NoTools` configurator is completely unaffected. [Source: architect-brief-mcp-client-integration.md#AgentConfigurator adaptation]
- **Error pattern:** No new error types needed for this story. MCP tool registration failures are handled by rig's `McpTool` at call time, not at registration time. [Source: architecture.md#Error Type Pattern]
- **Doc comments:** `///` mandatory on `with_mcp()`, `extract_mcp_tool_names()`, and updated `build_preamble()`. [Source: project-context.md#Code Quality & Style Rules]
- **Non-blocking:** If `mcp_servers` is empty, all code paths are no-ops. Zero overhead, zero behavioral change. [Source: architect-brief-mcp-client-integration.md#Key Design Decisions]

### Source Tree Components to Touch

| File | Action | Details |
|------|--------|---------|
| `src/llm/agent_factory.rs` | Edit | Add `mcp_servers` field to `ToolConfigurator`, update macro, add `with_mcp()`, update all `configure_*` impls |
| `src/session/dev_agent.rs` | Edit | Update `build_preamble()` signature to accept `&[String]`, conditionally append MCP tool names |
| `src/session/runner.rs` | Edit | Update `build_agent_for_role()` to chain `.with_mcp()` and pass MCP tool names to preamble |
| `src/review/mod.rs` | Edit | Update `run_inner()` to chain `.with_mcp()` and pass MCP tool names to preamble |
| `src/supervisor/architect.rs` | Edit | Add `mcp_manager` field, update constructor, chain `.with_mcp()` in `ask()` |
| `src/supervisor/mod.rs` | Edit | Update `with_architect_from_config()` to accept and forward `Arc<McpManager>` |
| `src/mcp/mod.rs` | Edit | Add `extract_mcp_tool_names()` free function, re-export |

### Key Technical Details — rig's `.rmcp_tools()` API

The `AgentBuilderSimple::rmcp_tools()` method signature (from rig-core with `rmcp` feature):

```rust
// rig-core/src/agent/builder.rs
impl<M: CompletionModel> AgentBuilderSimple<M> {
    pub fn rmcp_tools(
        mut self,
        tools: Vec<rmcp::model::Tool>,
        client: rmcp::service::ServerSink,
    ) -> Self { ... }
}
```

Key facts:
- Returns `Self` — chainable in a for loop
- Takes ownership of `Vec<rmcp::model::Tool>` and `ServerSink`
- Both `rmcp::model::Tool` and `ServerSink` (`Peer<RoleClient>`) implement `Clone`
- Must be called AFTER `.tool()` calls (which convert `AgentBuilder` → `AgentBuilderSimple`)
- Must be called BEFORE `.build()`

### ToolConfigurator Refactor Pattern

Current code (9-tool impl, `configure_anthropic` — other two methods are identical pattern):
```rust
fn configure_anthropic(self, builder: AgentBuilder<...>) -> Agent<...> {
    let (t1, t2, t3, t4, t5, t6, t7, t8, t9) = self.tools;
    builder.tool(t1).tool(t2).tool(t3).tool(t4)
        .tool(t5).tool(t6).tool(t7).tool(t8).tool(t9)
        .build()  // <-- currently calls .build() directly
}
```

After this story:
```rust
fn configure_anthropic(self, builder: AgentBuilder<...>) -> Agent<...> {
    let (t1, t2, t3, t4, t5, t6, t7, t8, t9) = self.tools;
    let mut simple = builder.tool(t1).tool(t2).tool(t3).tool(t4)
        .tool(t5).tool(t6).tool(t7).tool(t8).tool(t9);
    for (tools, sink) in self.mcp_servers {
        simple = simple.rmcp_tools(tools, sink);
    }
    simple.build()
}
```

The intermediate `simple` binding is needed because `.tool()` converts `AgentBuilder` to `AgentBuilderSimple`, and `.rmcp_tools()` is only available on `AgentBuilderSimple`.

### MCP Data Flow Through Call Sites

```
McpManager (Arc, in SessionRunner/ReviewRunner/ArchitectSession)
    │
    └─► tools_for_builder() → Vec<(Vec<Tool>, ServerSink)>  [clones per call]
            │
            ├─► extract_mcp_tool_names(&data) → Vec<String>  [for preamble]
            │       └─► build_preamble(&mcp_names)
            │
            └─► configure_agent_tools!(...).with_mcp(data)   [for agent builder]
```

**Single-call pattern:** Each call site calls `tools_for_builder()` once, extracts names from the result, then passes the data to `.with_mcp()`. The `build_preamble()` wrapper in `SessionRunner` calls `tools_for_builder()` separately (its own single call), which is fine since agent builds are infrequent.

### ArchitectSession Constructor Chain — CRITICAL

The MCP data must flow through the full supervisor construction chain:

```
SessionRunner::create_tools()
    └─► AskSupervisor::with_architect_from_config(config, factory, escalation, decisions)
            └─► ArchitectSession::new_with_factory(config, factory)
                    └─► stores agent_factory + project_root

// MUST BECOME:
SessionRunner::create_tools()
    └─► AskSupervisor::with_architect_from_config(config, factory, escalation, decisions, mcp_manager)
            └─► ArchitectSession::new_with_factory(config, factory, mcp_manager)
                    └─► stores agent_factory + project_root + mcp_manager
```

**Updated signatures:**

```rust
// src/supervisor/architect.rs
pub struct ArchitectSession {
    agent_factory: Arc<AgentFactory>,
    project_root: PathBuf,
    mcp_manager: Arc<McpManager>,  // NEW
}

impl ArchitectSession {
    /// Legacy constructor — uses McpManager::empty() for backward compat (tests).
    pub fn new(config: &BotConfig) -> Result<Self, ArchitectSessionError> {
        Self::new_with_factory(config, None, Arc::new(McpManager::empty()))
    }

    pub fn new_with_factory(
        config: &BotConfig,
        factory: Option<Arc<AgentFactory>>,
        mcp_manager: Arc<McpManager>,  // NEW
    ) -> Result<Self, ArchitectSessionError>
}

// src/supervisor/mod.rs
impl AskSupervisor {
    pub fn with_architect_from_config(
        config: &BotConfig,
        factory: Option<Arc<AgentFactory>>,
        escalation_slot: EscalationSlot,
        decision_log: DecisionLog,
        mcp_manager: Arc<McpManager>,  // NEW
    ) -> Result<Self, ArchitectSessionError>
}
```

**Update call sites in:**
- `SessionRunner::create_tools()` (src/session/runner.rs L730-740) — pass `Arc::clone(&self.mcp_manager)`
- `ReviewRunner::create_tools()` (src/review/mod.rs) — pass `Arc::clone(&self.mcp_manager)`

### Preamble Update Design

`build_preamble()` in `src/session/dev_agent.rs` (L45-80) is a pure function. The `SessionRunner::build_preamble()` wrapper (L711-713) centralizes MCP name injection — `ReviewRunner` and `ArchitectSession` call `dev_agent::build_preamble()` directly with their own MCP names.

Current hardcoded tool list:
```
## Tools
You have access to these tools: edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor, plus a built-in think tool for reasoning.
```

After update — when `mcp_tool_names` is non-empty, append one line:
```
You also have access to MCP tools: browser_navigate, browser_screenshot, browser_click, browser_fill, browser_snapshot. Use them like any other tool.
```

When `mcp_tool_names` is empty — output is byte-identical to current. The conditional append avoids polluting the preamble when MCP is not configured.

### Previous Story Intelligence (Story 9.1)

Story 9.1 established:
- `McpManager` struct with `init()`, `empty()`, `tools_for_builder()`, `shutdown()` in `src/mcp/manager.rs`
- `McpServerConfig` and `McpTransport` in `src/config/mod.rs`
- `Arc<McpManager>` is already stored in `SessionRunner`, `ReviewRunner`, and `StoryPipeline`
- `tools_for_builder()` returns `Vec<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)>` — exactly what `.rmcp_tools()` expects
- `ServerSink` extraction uses `(*handle.service).clone()` via `Deref` on `RunningService`
- rmcp version is 0.13 (must match rig-core's internal dependency)

### Testing Standards

- Rust native `#[cfg(test)]` + `cargo test`. No external test runner.
- Inline `#[cfg(test)] mod tests { ... }` at bottom of each module file.
- Descriptive snake_case names: `test_tool_configurator_with_mcp_empty_is_noop`.
- Never call real MCP servers in unit tests.
- Critical existing tests that MUST NOT break:
  - `test_build_preamble_contains_tool_rules`
  - `test_build_preamble_contains_english_override`
  - `test_build_preamble_contains_activation_rules`
  - `test_build_preamble_contains_job_done_sentinel`
  - `test_build_preamble_mentions_tool_usage_best_practices`
  - `test_no_tools_configurator`
  - All `test_agent_factory_build_*` tests

### References

- [Source: _bmad-output/planning-artifacts/architect-brief-mcp-client-integration.md#L194-242] — AgentConfigurator adaptation design, ToolConfigurator refactor, with_mcp builder pattern
- [Source: _bmad-output/planning-artifacts/epics.md#L2145-2197] — Story 9.2 acceptance criteria and scope
- [Source: _bmad-output/planning-artifacts/architecture.md#L625-648] — Error Type Pattern
- [Source: _bmad-output/planning-artifacts/architecture.md#L936-1143] — Project Structure & Module Boundaries
- [Source: _bmad-output/project-context.md] — 45 critical implementation rules
- [Source: _bmad-output/implementation-artifacts/9-1-mcp-server-lifecycle-management-config.md] — Previous story: McpManager API, tools_for_builder() signature, Arc<McpManager> wiring
- [Source: src/llm/agent_factory.rs#L580-700] — Current `configure_agent_tools!` macro, `ToolConfigurator`, all `AgentConfigurator` impls (9-tool and 1-tool)
- [Source: src/llm/agent_factory.rs#L505-540] — `AgentConfigurator` trait definition (UNCHANGED)
- [Source: src/llm/agent_factory.rs#L271-424] — `AgentFactory::build()` method (UNCHANGED)
- [Source: src/session/dev_agent.rs#L45-80] — Current `build_preamble()` with hardcoded tool list
- [Source: src/session/runner.rs#L679-701] — `build_agent_for_role()` call site with `configure_agent_tools!`
- [Source: src/session/runner.rs#L716-744] — `create_tools()` where AskSupervisor is constructed
- [Source: src/review/mod.rs#L316-323] — ReviewRunner `configure_agent_tools!` call site
- [Source: src/review/mod.rs#L360-400] — ReviewRunner `create_tools()`
- [Source: src/supervisor/architect.rs#L120-165] — `ArchitectSession` struct and constructors
- [Source: src/supervisor/architect.rs#L336-360] — `ArchitectSession::ask()` with `configure_agent_tools!(read_tool)` call
- [Source: src/supervisor/mod.rs#L197-212] — `AskSupervisor::with_architect_from_config()` constructor
- [Source: rig-core docs — AgentBuilderSimple::rmcp_tools()] — `pub fn rmcp_tools(self, tools: Vec<Tool>, client: ServerSink) -> Self`

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List