# Story 9.1: MCP Server Lifecycle Management & Config

Status: review

## Story

As a daemon operator,
I want the daemon to connect to external MCP servers at startup, discover their tools, and shut them down gracefully,
So that the agent gains access to external capabilities (browser automation, etc.) without custom tool implementations.

## Acceptance Criteria

1. **Given** `Cargo.toml` is updated **When** the project is compiled **Then** `rmcp` is added as a dependency with features `client` + `transport-child-process` **And** the `rmcp` feature flag is enabled on `rig-core` **And** the `rmcp` version is compatible with rig-core's dependency (currently 0.13)

2. **Given** `bmad-bot.yaml` contains an `mcp_servers` section with one or more server entries **When** the daemon parses the config at startup **Then** `BotConfig` exposes a `mcp_servers: Vec<McpServerConfig>` field (defaults to empty vec via `#[serde(default)]`) **And** each entry includes: `name` (String), `command` (String), `args` (Vec<String>), `transport` (enum, default stdio), `enabled` (bool, default true), `timeout_secs` (Option<u64>, default None → uses 30s) **And** entries with `enabled: false` are skipped

3. **Given** no `mcp_servers` section exists in the config **When** the daemon starts **Then** `McpManager` is initialized with zero servers (empty Vec) **And** the daemon operates identically to before — zero behavioral change

4. **Given** a valid `mcp_servers` config with an enabled server (e.g., Playwright) **When** `McpManager::init()` is called during daemon startup **Then** the daemon spawns the server process via rmcp's stdio transport **And** the MCP initialize handshake completes successfully (handled by rmcp) **And** `list_all_tools()` is called on the connected server **And** the discovered `Vec<rmcp::model::Tool>` and `ServerSink` are stored in `McpServerHandle` **And** a `tracing::info!()` log records the server name and number of tools discovered

5. **Given** a configured MCP server command that does not exist on the system (e.g., `npx` not installed) **When** `McpManager::init()` attempts to spawn it **Then** the failure is logged via `tracing::warn!()` with the server name and error details **And** the daemon continues startup without that server's tools **And** other configured MCP servers are still attempted

6. **Given** a configured MCP server that fails the handshake or times out **When** `McpManager::init()` attempts the connection **Then** a per-server configurable timeout (`timeout_secs`, default 30s) bounds the handshake attempt **And** the failure is logged via `tracing::warn!()` **And** the daemon continues without that server

7. **Given** multiple MCP servers are configured and connected **When** `McpManager::tools_for_builder()` is called **Then** it returns `Vec<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)>` — one tuple per connected server **And** each `ServerSink` is cloneable for use across sessions

8. **Given** the daemon receives SIGTERM/SIGINT **When** cooperative shutdown begins **Then** `McpManager::shutdown()` is called **And** MCP `close` notifications are sent to all connected servers before dropping connections **And** child processes are cleaned up

9. **Given** the module is implemented **When** inspecting the code structure **Then** `src/mcp/mod.rs` exports `McpManager` and `McpError` **And** `src/mcp/manager.rs` contains `McpManager`, `McpServerHandle` **And** `McpServerConfig` and `McpTransport` are defined in `src/config/mod.rs` (alongside all other config structs) **And** error types follow the per-module thiserror pattern (`McpError` enum) **And** unit tests cover: empty config, successful connection (mocked), failed spawn, handshake timeout, graceful shutdown, `tools_for_builder()` output shape

## Tasks / Subtasks

- [x] Task 0: Verify rig-core fork supports `rmcp` feature (PREREQUISITE)
  - [x] 0.1 Check if the fork at `https://github.com/jbanety/rig.git` (branch `fix/copilot-streaming-compat`) includes the `rmcp` feature gate in its `Cargo.toml` and the `McpTool` / `AgentBuilder::rmcp_tools()` code
  - [x] 0.2 If the fork is behind upstream: merge upstream rig changes that include rmcp support into the fork branch, push, and verify `cargo check` passes with `features = ["rmcp"]`
  - [x] 0.3 HALT if the fork cannot support rmcp — this must be resolved before any other task

- [x] Task 1: Update `Cargo.toml` dependencies (AC: #1)
  - [x] 1.1 Add `rmcp = { version = "0.13", features = ["client", "transport-child-process"] }` to `[dependencies]`
  - [x] 1.2 Update `rig-core` line to enable the `rmcp` feature: `rig-core = { git = "https://github.com/jbanety/rig.git", branch = "fix/copilot-streaming-compat", features = ["rmcp"] }`
  - [x] 1.3 Run `cargo check` — must pass. Then run `cargo tree -i rmcp` to confirm both our crate and rig-core resolve to the same rmcp version. If versions differ, pin to match rig-core exactly.

- [x] Task 2: Add `McpServerConfig` and `McpTransport` to `src/config/mod.rs` (AC: #2, #3)
  - [x] 2.1 Define `McpServerConfig` struct in `src/config/mod.rs` (same file as `LlmRoleConfig`, `GitProviderConfig`, etc.) with `#[derive(Debug, Clone, Deserialize, Serialize)]`: `name: String`, `command: String`, `args: Vec<String>`, `#[serde(default = "default_mcp_transport")] transport: McpTransport`, `#[serde(default = "default_true")] enabled: bool`, `pub timeout_secs: Option<u64>`
  - [x] 2.2 Define `McpTransport` enum with `#[derive(Debug, Clone, Deserialize, Serialize)]` and `#[serde(rename_all = "lowercase")]`: variant `Stdio` (default). Extensible for future `Http` support
  - [x] 2.3 Add `#[serde(default)] pub mcp_servers: Vec<McpServerConfig>` field to `BotConfig`. Using `Vec` directly (not `Option<Vec>`) — `#[serde(default)]` gives empty vec when absent, avoiding `.unwrap_or` everywhere
  - [x] 2.4 Update `bmad-bot.yaml.example` with a commented-out `mcp_servers` section showing the Playwright example
  - [x] 2.5 Verify ALL existing config tests still pass unchanged — the `VALID_YAML` constant and all `test_config_*` tests must not break

- [x] Task 3: Create `src/mcp/` module structure (AC: #9)
  - [x] 3.1 Create `src/mcp/mod.rs` — re-exports `McpManager`, `McpError` (NOT `McpServerConfig` — that lives in config)
  - [x] 3.2 Create `src/mcp/manager.rs` — `McpManager`, `McpServerHandle`, core logic
  - [x] 3.3 Add `mod mcp;` to `src/main.rs` (alphabetical order, between `mod llm;` and `mod notifier;`)
  - [x] 3.4 Define `McpError` enum with thiserror in `src/mcp/manager.rs`: `SpawnFailed { name: String, source: std::io::Error }`, `HandshakeTimeout { name: String }`, `HandshakeFailed { name: String, reason: String }`, `ToolDiscoveryFailed { name: String, reason: String }`, `ShutdownError { name: String, reason: String }`

- [x] Task 4: Implement `McpManager::init()` and `McpManager::empty()` (AC: #3, #4, #5, #6)
  - [x] 4.1 Implement `McpManager::empty() -> Self` — returns `McpManager { servers: vec![] }`. Used as fallback when init fails or no MCP servers configured.
  - [x] 4.2 Implement `McpManager::init(configs: &[McpServerConfig]) -> Self` (infallible — never returns Err, logs failures and continues). Filter out `enabled: false` entries. If empty after filtering, return `Self::empty()`.
  - [x] 4.3 For each enabled server: spawn child process via `rmcp::transport::TokioChildProcess::new(Command::new(&config.command).configure(|cmd| { cmd.args(&config.args); }))`. On spawn failure (`std::io::Error`): log `tracing::warn!(server = %name, error = %e, "MCP server failed to spawn — skipping")` and continue to next.
  - [x] 4.4 Call `().serve(transport).await` (from `rmcp::service::ServiceExt`) for the MCP handshake — returns `RunningService<RoleClient, ()>`. `RunningService` derefs to `Peer<RoleClient>` which IS `ServerSink`.
  - [x] 4.5 Wrap the handshake in `tokio::time::timeout(Duration::from_secs(config.timeout_secs.unwrap_or(30)), ...)`. On timeout: log `tracing::warn!()` and continue.
  - [x] 4.6 On success: call `service.list_all_tools().await` to discover tools (pagination-safe). On discovery failure: log warning and continue.
  - [x] 4.7 Store `McpServerHandle { name, service: RunningService<RoleClient, ()>, tools: Vec<rmcp::model::Tool> }` for each connected server
  - [x] 4.8 Log per-server: `tracing::info!(server = %name, tool_count = tools.len(), "MCP server connected")`
  - [x] 4.9 Log init summary: `tracing::info!(connected = connected_count, failed = failed_count, total_tools = total_tool_count, "MCP initialization complete")`
  - [x] 4.10 Return `McpManager { servers }` — may be empty if all servers failed

- [x] Task 5: Implement `McpManager::tools_for_builder()` (AC: #7)
  - [x] 5.1 Signature: `pub async fn tools_for_builder(&self) -> Vec<(Vec<rmcp::model::Tool>, rmcp::service::ServerSink)>`
  - [x] 5.2 For each `McpServerHandle`: clone `tools` (Vec<Tool> is Clone) and obtain a `ServerSink` clone. Since `RunningService<RoleClient, ()>` derefs to `Peer<RoleClient>` (= `ServerSink`) and `Peer` is `Clone`, use: `let sink: ServerSink = (*handle.service).clone();`
  - [x] 5.3 The return type matches exactly what `AgentBuilderSimple::rmcp_tools(tools: Vec<rmcp::model::Tool>, client: rmcp::service::ServerSink)` expects

- [x] Task 6: Implement `McpManager::shutdown()` (AC: #8)
  - [x] 6.1 Signature: `pub async fn shutdown(&self)` (infallible)
  - [x] 6.2 For each `McpServerHandle`: call `handle.service.close().await` to send MCP close notification. Note: `close(&mut self)` used instead of `cancel(self)` — allows shutdown from `&self` via `tokio::sync::Mutex` drain pattern.
  - [x] 6.3 Log each: `tracing::info!(server = %name, "MCP server disconnected")`
  - [x] 6.4 Handle errors gracefully — log `tracing::warn!()` but never propagate

- [x] Task 7: Integrate `McpManager` into daemon startup, pipeline, and shutdown (AC: #3, #4, #8)
  - [x] 7.1 In `cli::run_start()` (around L1330): after config load/validation and before pipeline creation, call `let mcp_manager = Arc::new(McpManager::init(&config.mcp_servers).await);`
  - [x] 7.2 Update `StoryPipeline::new()` signature to accept `mcp_manager: Arc<McpManager>` as 4th parameter. Store it in the struct.
  - [x] 7.3 Pass `Arc::clone(&mcp_manager)` to `SessionRunner::new()` — update `SessionRunner` to accept and store `mcp_manager: Arc<McpManager>` (for Story 9.2 usage)
  - [x] 7.4 Pass `Arc::clone(&mcp_manager)` to `ReviewRunner::new()` — update `ReviewRunner` to accept and store `mcp_manager: Arc<McpManager>` (for Story 9.2 usage)
  - [x] 7.5 After polling loop exits, before `daemon_state.mark_stopped()`: call `mcp_manager.shutdown().await` — uses `&self` via mutex drain pattern
  - [x] 7.6 Update all `StoryPipeline::new()` call sites (including test helpers if any) to pass the new parameter
  - [x] 7.7 Verify zero behavioral change when `mcp_servers` is empty — all existing tests pass, daemon operates identically

- [x] Task 8: Write unit tests (AC: #9)
  - [x] 8.1 In `src/config/mod.rs` tests: test `McpServerConfig` deserialization (full config, defaults for transport/enabled/timeout_secs), test `McpTransport` defaults to `Stdio`, test `BotConfig` parses with and without `mcp_servers` section (backward compat), verify ALL existing config tests still pass
  - [x] 8.2 In `src/mcp/manager.rs` tests: test `McpManager::empty()` returns zero servers, `tools_for_builder()` returns empty vec
  - [x] 8.3 Test `McpError` display messages for all variants
  - [x] 8.4 Test `McpManager` is `Send + Sync` (same pattern as `test_agent_factory_is_send_sync`)
  - [x] 8.5 Test `McpServerConfig` filtering: `enabled: false` entries excluded
  - [x] 8.6 Test `McpManager::init(&[])` returns empty manager

## Dev Notes

### Architecture Patterns & Constraints

- **Error pattern:** Per-module `thiserror` enum. `McpError` in `src/mcp/manager.rs`. Never `anyhow` in library modules. [Source: architecture.md#Error Type Pattern]
- **Tracing:** Structured spans with context fields. `tracing::info!()` / `tracing::warn!()` only. Never `println!()`. [Source: project-context.md#Tracing Pattern]
- **Config:** Loaded once, shared as `Arc<BotConfig>`, never mutated. `mcp_servers` uses `Vec<McpServerConfig>` with `#[serde(default)]` — empty vec when absent. [Source: architecture.md#Config Pattern]
- **Shutdown:** Call `mcp_manager.shutdown().await` after polling loop exits, same location as `daemon_state.mark_stopped()`. [Source: project-context.md#Cooperative Shutdown Pattern]
- **Non-blocking failures:** MCP failures NEVER crash the daemon. Log and skip. Daemon continues with native tools. [Source: architect-brief-mcp-client-integration.md#Key Design Decisions]
- **Doc comments:** `///` mandatory on all public items. [Source: project-context.md#Code Quality & Style Rules]

### Config Struct Placement — RESOLVED

All config structs live in `src/config/mod.rs`. This includes `McpServerConfig` and `McpTransport`. This matches the existing pattern: `LlmRoleConfig`, `GitProviderConfig`, `TelegramConfig`, `BmadPathsConfig` are all in `src/config/mod.rs`. Do NOT put config structs in `src/mcp/` — it creates circular dependencies (`config` ↔ `mcp`).

### Source Tree Components to Touch

| File | Action | Details |
|------|--------|---------|
| `Cargo.toml` | Edit | Add `rmcp` dep with `client` + `transport-child-process` features; enable `rmcp` feature on `rig-core` |
| `src/main.rs` | Edit | Add `mod mcp;` between `mod llm;` and `mod notifier;` |
| `src/config/mod.rs` | Edit | Add `McpServerConfig`, `McpTransport` structs + `mcp_servers: Vec<McpServerConfig>` to `BotConfig` |
| `src/mcp/mod.rs` | Create | Module root — re-exports `McpManager`, `McpError` |
| `src/mcp/manager.rs` | Create | `McpManager`, `McpServerHandle`, `McpError`, `init()`, `empty()`, `tools_for_builder()`, `shutdown()` |
| `src/cli/mod.rs` | Edit | `run_start()`: init `McpManager`, pass to pipeline, shutdown on exit |
| `src/pipeline.rs` | Edit | `StoryPipeline::new()` accepts `Arc<McpManager>`, stores it, passes to runners |
| `src/session/runner.rs` | Edit | `SessionRunner::new()` accepts `Arc<McpManager>`, stores it (used in Story 9.2) |
| `src/review/mod.rs` | Edit | `ReviewRunner::new()` accepts `Arc<McpManager>`, stores it (used in Story 9.2) |
| `bmad-bot.yaml.example` | Edit | Add commented-out `mcp_servers` section with Playwright example |

### rmcp API Usage Reference

```rust
// === Complete MCP server connection flow ===
use rmcp::service::{RoleClient, RunningService, ServiceExt};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use tokio::process::Command;

// 1. Spawn child process (stdio transport)
let transport = TokioChildProcess::new(
    Command::new(&config.command).configure(|cmd| { cmd.args(&config.args); })
)?;  // std::io::Error on spawn failure

// 2. MCP handshake — ().serve() performs initialize exchange
let service: RunningService<RoleClient, ()> = tokio::time::timeout(
    Duration::from_secs(config.timeout_secs.unwrap_or(30)),
    ().serve(transport),
).await??;  // outer: timeout, inner: handshake error

// 3. Tool discovery
let tools: Vec<rmcp::model::Tool> = service.list_all_tools().await?;

// 4. ServerSink extraction — RunningService derefs to Peer<RoleClient> (= ServerSink)
//    Peer<RoleClient> is Clone, so:
let sink: rmcp::service::ServerSink = (*service).clone();

// 5. Shutdown
service.cancel().await?;  // sends MCP close notification, waits for child exit
```

### McpServerHandle Design

```rust
/// Holds a connected MCP server's running service and discovered tools.
struct McpServerHandle {
    name: String,
    /// The running MCP service — owns the child process and background task.
    /// Derefs to `Peer<RoleClient>` (= `ServerSink`).
    /// MUST be stored to keep the background task alive.
    service: RunningService<RoleClient, ()>,
    /// Tools discovered via `list_all_tools()` at init time.
    tools: Vec<rmcp::model::Tool>,
}
```

### rmcp Version Constraint — CRITICAL

The rig-core fork depends internally on `rmcp = "0.13"`. Our `Cargo.toml` **MUST** use `rmcp = "0.13"` to avoid duplicate type errors. After adding both deps, verify with:

```bash
cargo tree -i rmcp  # must show exactly ONE rmcp version
```

If versions diverge, pin ours to match rig-core's exact version from `Cargo.lock`.

### Pipeline Integration — Updated Signatures

```rust
// StoryPipeline::new() — add mcp_manager as 4th param
pub fn new(
    config: Arc<BotConfig>,
    secrets: Arc<BotSecrets>,
    shutdown: ShutdownFlag,
    mcp_manager: Arc<McpManager>,  // NEW
) -> Result<Self, PipelineError>

// SessionRunner::new() — add mcp_manager
pub fn new(
    config: Arc<BotConfig>,
    agent_factory: Arc<AgentFactory>,
    shutdown: ShutdownFlag,
    mcp_manager: Arc<McpManager>,  // NEW — stored for Story 9.2
) -> Self

// ReviewRunner::new() — add mcp_manager
pub fn new(
    config: Arc<BotConfig>,
    secrets: Arc<BotSecrets>,
    agent_factory: Arc<AgentFactory>,
    shutdown: ShutdownFlag,
    mcp_manager: Arc<McpManager>,  // NEW — stored for Story 9.2
) -> Self
```

In `cli::run_start()` (around L1330):
```rust
// Init MCP before pipeline — infallible, logs failures internally
let mcp_manager = Arc::new(crate::mcp::McpManager::init(&config.mcp_servers).await);

let pipeline = crate::pipeline::StoryPipeline::new(
    Arc::clone(&config),
    Arc::clone(&secrets),
    std::sync::Arc::clone(&shutdown),
    Arc::clone(&mcp_manager),
)?;

// ... polling loop ...

// After loop, before daemon_state.mark_stopped():
mcp_manager.shutdown().await;
```

### Testing Standards

- Rust native `#[cfg(test)]` + `cargo test`. No external test runner.
- Inline `#[cfg(test)] mod tests { ... }` at bottom of each module file.
- Descriptive snake_case names: `test_mcp_manager_empty_returns_no_servers`.
- Never call real MCP servers in unit tests. Test config deserialization and empty-path logic only.
- E2E tests with real MCP servers (Playwright) belong in Story 9.3 — manual-launch only.

### References

- [Source: _bmad-output/planning-artifacts/architect-brief-mcp-client-integration.md] — Full architect brief with spike findings, design decisions, proposed architecture
- [Source: _bmad-output/planning-artifacts/epics.md#L2057-2271] — Epic 9 definition with all 3 stories and acceptance criteria
- [Source: _bmad-output/planning-artifacts/architecture.md#L625-648] — Error Type Pattern (per-module thiserror)
- [Source: _bmad-output/planning-artifacts/architecture.md#L741-780] — Cooperative Shutdown Pattern
- [Source: _bmad-output/planning-artifacts/architecture.md#L780-810] — Config Pattern (Validate Once, Share via Arc)
- [Source: _bmad-output/planning-artifacts/architecture.md#L936-1143] — Project Structure & Module Boundaries
- [Source: _bmad-output/project-context.md] — 45 critical implementation rules
- [Source: src/llm/agent_factory.rs#L580-700] — `configure_agent_tools!` macro, `ToolConfigurator`, `AgentConfigurator` impls
- [Source: src/config/mod.rs#L75-107] — `BotConfig` struct (all config structs defined here)
- [Source: src/pipeline.rs#L136-190] — `StoryPipeline::new()` current signature and body
- [Source: src/cli/mod.rs#L1250-1370] — `run_start()` daemon startup flow
- [Source: src/session/runner.rs#L294-317] — `SessionRunner::new()` current signature
- [Source: src/review/mod.rs#L191-201] — `ReviewRunner::new()` current signature
- [Source: src/main.rs] — Module declarations (mod ordering)
- [Source: Cargo.toml] — Current dependencies (rig-core fork, no rmcp yet)
- [Source: rig-core docs — McpTool, AgentBuilder::rmcp_tools()] — `pub fn rmcp_tools(self, tools: Vec<Tool>, client: ServerSink) -> Self`
- [Source: rmcp docs — TokioChildProcess, ServiceExt::serve(), ServerSink, list_all_tools(), cancel()]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — no debug issues encountered.

### Completion Notes List

- **Task 0**: Fork at `jbanety/rig` branch `fix/copilot-streaming-compat` already includes `rmcp = { version = "0.13", optional = true }` with feature gate `rmcp = ["dep:rmcp"]`, `McpTool` struct, and `AgentBuilder::rmcp_tools()`. No merge needed.
- **Task 1**: Added `rmcp` dep and enabled `rmcp` feature on `rig-core`. `cargo tree -i rmcp` confirms single `rmcp v0.13.0` resolution shared between `bmad-bot` and `rig-core`.
- **Task 2**: `McpServerConfig`, `McpTransport` structs added to `src/config/mod.rs` alongside existing config structs. `mcp_servers: Vec<McpServerConfig>` field added to `BotConfig` with `#[serde(default)]`. All 6 existing `BotConfig` constructor sites updated with `mcp_servers: vec![]`. `bmad-bot.yaml.example` updated with commented-out example.
- **Task 3**: `src/mcp/mod.rs` and `src/mcp/manager.rs` created. `mod mcp;` added to `main.rs` in alphabetical order. `McpError` enum with 5 variants defined using `thiserror`.
- **Task 4**: `McpManager::init()` is infallible — filters disabled servers, spawns child processes via `TokioChildProcess`, performs MCP handshake with configurable timeout, discovers tools via `list_all_tools()`. All failures logged and skipped.
- **Task 5**: `tools_for_builder()` returns `Vec<(Vec<Tool>, ServerSink)>` — clones tools and extracts `ServerSink` via `Deref` on `RunningService`. Made `async` (requires lock on `tokio::sync::Mutex`).
- **Task 6**: `shutdown(&self)` uses mutex drain pattern — drains servers from the `Mutex<Vec>`, calls `close()` on each `RunningService`. Uses `close(&mut self)` instead of `cancel(self)` to avoid consuming the `McpManager`, enabling shutdown from `Arc<McpManager>`.
- **Task 7**: `McpManager` initialized in `cli::run_start()` after config/secrets load, before pipeline creation. Passed through `StoryPipeline::new()` → `SessionRunner::new()` and `ReviewRunner::new()` as `Arc<McpManager>`. `mcp_manager.shutdown().await` called after polling loop exits. All test helpers updated. `Debug` impl added to `McpManager` for `ReviewRunner`'s `#[derive(Debug)]`.
- **Task 8**: 13 tests in `src/mcp/manager.rs` + 6 tests in `src/config/mod.rs` = 19 new tests. Full suite: 974 tests pass, 0 failures.

### Change Log

- 2026-02-18: Story 9.1 implementation complete — MCP server lifecycle management and config. 19 new tests added, 974 total tests passing.

### File List

- `Cargo.toml` — Added `rmcp` dependency, enabled `rmcp` feature on `rig-core`
- `src/main.rs` — Added `mod mcp;`
- `src/config/mod.rs` — Added `McpServerConfig`, `McpTransport`, `default_true()`, `mcp_servers` field on `BotConfig`, 6 new config tests
- `src/mcp/mod.rs` — Created: module root, re-exports `McpManager`, `McpError`
- `src/mcp/manager.rs` — Created: `McpManager`, `McpServerHandle`, `McpError`, `init()`, `empty()`, `tools_for_builder()`, `shutdown()`, `connect_server()`, 13 unit tests
- `src/cli/mod.rs` — `run_start()`: init `McpManager`, pass to pipeline, shutdown on exit; test helper updated with `mcp_servers: vec![]`
- `src/pipeline.rs` — `StoryPipeline::new()` accepts and passes `Arc<McpManager>` to runners
- `src/session/runner.rs` — `SessionRunner` accepts and stores `Arc<McpManager>`; all test helpers updated
- `src/review/mod.rs` — `ReviewRunner` accepts and stores `Arc<McpManager>`; test helper updated
- `src/watcher/mod.rs` — Test helper updated with `mcp_servers: vec![]`
- `src/llm/agent_factory.rs` — Test helper updated with `mcp_servers: vec![]`
- `bmad-bot.yaml.example` — Added commented-out `mcp_servers` section with Playwright example