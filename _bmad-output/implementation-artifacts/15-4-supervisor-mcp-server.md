# Story 15.4: Supervisor MCP Server

Status: done

## Story

As a daemon developer,
I want the supervisor's `ask_supervisor` capability exposed as an MCP server (stdio transport),
so that SDK-mode sessions can access the supervisor via their native MCP integration.

## Acceptance Criteria

1. **Given** a new `src/mcp_server/` module **When** built **Then** it implements a stdio-transport MCP server exposing a single tool: `ask_supervisor` with `question: String` and optional `context: String` parameters **And** returns `{ "answer": "...", "source": "rule_engine|llm_fallback|escalation" }` as structured JSON in the MCP tool result — all three tiers use this same response shape (escalation sets `answer` to an empty string and adds `"escalation": true`)

2. **Given** an SDK session calls `ask_supervisor` via MCP **When** the MCP server receives the request **Then** it delegates to the existing 3-tier cascade: rule engine → LLM fallback → human escalation **And** decision logging is preserved — MCP tool calls logged identically to rig tool calls via the same `DecisionLog` and `DecisionRecord` types

3. **Given** the daemon starts an SDK session **When** it prepares the MCP config **Then** it generates a dynamic MCP server config JSON:
   ```json
   {
     "mcpServers": {
       "bmad-supervisor": {
         "command": "bmad-bot",
         "args": ["mcp-supervisor", "--story", "{story_key}", "--config", "{config_path}"],
         "env": { "ANTHROPIC_API_KEY": "...", "OPENAI_API_KEY": "..." }
       }
     }
   }
   ```
   **And** API keys from `BotSecrets` are injected in the `env` field (only keys that are `Some`)
   **And** a new hidden CLI subcommand `bmad-bot mcp-supervisor` handles the server process

4. **Given** the MCP supervisor server starts **When** it receives an `ask_supervisor` call **Then** the rule engine is evaluated first (deterministic, free) **And** on NoMatch with an `AnswerProvider` configured, the LLM fallback (ArchitectSession) is invoked **And** on LLM failure or no `AnswerProvider`, escalation is signaled via MCP error content

5. **Given** escalation is required **When** neither rules nor LLM can answer **Then** the MCP tool returns `CallToolResult::error()` with a clear escalation message including the question and reason **And** the calling SDK session can interpret this as an escalation signal

6. **Given** the MCP server needs LLM fallback **When** the supervisor role is configured with an API provider (anthropic/openai-compatible) **Then** the `ArchitectSession` is constructed via `AgentFactory` using the supervisor's provider config **And** no MCP config is passed to the supervisor's own session (anti-recursion guard)

7. **Given** all existing tests pass **When** the MCP server module is added **Then** zero behavioral changes for existing API-mode configurations — all 1358+ existing unit tests pass identically

8. **Given** the `mcp_server` module is built **When** the `bmad-bot mcp-supervisor` subcommand is invoked **Then** it loads config + secrets, constructs the supervisor (rule engine + optional ArchitectSession), starts the MCP server on stdio, and blocks until the client disconnects

9. **Given** the MCP server process exits (client disconnect or signal) **When** decisions were recorded during the session **Then** the decisions are persisted to `{implementation_artifacts}/{story_key}-SUPERVISOR-DECISIONS.md` **And** the file format matches the existing `write_decisions_file()` output

## Tasks / Subtasks

- [x] Task 1: Add `rmcp` server features to `Cargo.toml` (AC: #1)
  - [x] 1.1 Update `rmcp` dependency to add `"server"`, `"transport-io"`, and `"macros"` features:
    ```toml
    rmcp = { version = "1", features = ["client", "transport-child-process", "server", "transport-io", "macros"] }
    ```
    CRITICAL: The `"macros"` feature is required for `#[tool_router]`, `#[tool_handler]`, and `#[tool]` proc macros. It is a *default* feature of rmcp, but since this project uses explicit feature lists (not `default-features = true`), `"server"` alone does NOT pull in `"macros"` — the feature dependency chain is: `server` → `transport-async-rw` + `schemars` + `pastey`, while `macros` → `rmcp-macros` + `pastey`. They are independent.
  - [x] 1.2 Verify `cargo check` succeeds with new features — no version conflicts

- [x] Task 2: Create `src/mcp_server/mod.rs` with the MCP server handler (AC: #1, #2, #4, #5)
  - [x] 2.1 Create `src/mcp_server/mod.rs` with module doc comment
  - [x] 2.2 Define `SupervisorMcpServer` struct (includes `tool_router` field required by `#[tool_router]` macro):
    ```rust
    #[derive(Clone, Debug)]
    pub struct SupervisorMcpServer {
        rule_engine: Arc<RuleEngine>,
        answer_provider: Option<Arc<dyn AnswerProvider>>,
        decision_log: DecisionLog,
        tool_router: ToolRouter<SupervisorMcpServer>,
    }
    ```
    Notes:
    - `Clone` is required by rmcp (handler shared across requests). `Debug` is required by `ServerHandler` trait bounds.
    - `RuleEngine` and `AnswerProvider` are wrapped in `Arc` for cloneability. `DecisionLog` already wraps `Arc<Mutex<Vec<DecisionRecord>>>` — `Clone` natively.
    - `tool_router: ToolRouter<Self>` is generated by the `#[tool_router]` macro and initialized via `Self::tool_router()`.
    - `story_key` is NOT a field on this struct — it is only used by the CLI handler (`run_mcp_supervisor()`) for the decisions file path. Storing it here would be dead weight.
  - [x] 2.4 Define `AskSupervisorParams` for MCP tool parameter extraction:
    ```rust
    #[derive(Debug, serde::Deserialize, rmcp::schemars::JsonSchema)]
    pub struct AskSupervisorParams {
        #[schemars(description = "The specific question or doubt you need answered.")]
        pub question: String,
        #[schemars(description = "Optional additional context: code snippets, error messages, or relevant workflow state.")]
        pub context: Option<String>,
    }
    ```
  - [x] 2.5 Implement `SupervisorMcpServer` with `#[tool_router]` and `#[tool_handler]`:
    ```rust
    use rmcp::{ServerHandler, tool, tool_router, tool_handler};
    use rmcp::model::*;
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::handler::server::router::tool::ToolRouter;

    #[tool_router]
    impl SupervisorMcpServer {
        /// Construct a new supervisor MCP server.
        ///
        /// This is the ONLY constructor — `#[tool_router]` generates `Self::tool_router()`
        /// which must be called here to initialize the `tool_router` field.
        pub fn create(
            answer_provider: Option<Box<dyn AnswerProvider>>,
            decision_log: DecisionLog,
        ) -> Self {
            Self {
                rule_engine: Arc::new(RuleEngine::new()),
                answer_provider: answer_provider.map(|p| Arc::from(p)),
                decision_log,
                tool_router: Self::tool_router(),
            }
        }

        #[tool(description = "Ask the supervisor a question when you encounter a doubt, blocker, decision point, or need clarification during your work.")]
        async fn ask_supervisor(
            &self,
            Parameters(params): Parameters<AskSupervisorParams>,
        ) -> Result<CallToolResult, rmcp::ErrorData> {
            self.handle_question(params.question, params.context).await
        }
    }

    #[tool_handler]
    impl ServerHandler for SupervisorMcpServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(
                ServerCapabilities::builder().enable_tools().build(),
            )
            .with_server_info(Implementation::new("bmad-supervisor", env!("CARGO_PKG_VERSION")))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "BMAD Supervisor — answers developer questions using project documentation. \
                 Use the ask_supervisor tool when you encounter doubts, blockers, or decision points."
                    .to_string(),
            )
        }
    }
    ```
    IMPORTANT: `handle_question()` is defined in a **separate `impl SupervisorMcpServer` block** (Task 2.6), NOT inside the `#[tool_router]` block. Methods inside `#[tool_router]` are either constructors or `#[tool]`-annotated tool handlers. Putting `handle_question()` there would confuse the macro into treating it as a tool.
  - [x] 2.6 Implement the 3-tier cascade in a **separate `impl` block** (NOT inside `#[tool_router]`):
    ```rust
    /// Private methods — separate impl block from the #[tool_router] block.
    impl SupervisorMcpServer {
        async fn handle_question(
            &self,
            question: String,
            context: Option<String>,
        ) -> Result<CallToolResult, rmcp::ErrorData> {
            tracing::info!(action = "mcp_ask_supervisor", question = %question, "MCP supervisor tool invoked");

            // Step 1: Rule engine (deterministic, free)
            let result = self.rule_engine.evaluate(&question);
            match result {
                RuleResult::Matched { rule_name, answer } => {
                    self.decision_log.record(DecisionRecord::new(
                        question, context, answer.clone(),
                        DecisionSource::RuleEngine { rule_name },
                        "Matched deterministic rule pattern".to_string(), vec![],
                    ));
                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::json!({ "answer": answer, "source": "rule_engine" }).to_string()
                    )]))
                }
                RuleResult::NoMatch => {
                    // Step 2: LLM fallback
                    match &self.answer_provider {
                        Some(provider) => {
                            match provider.ask(&question, context.as_deref()).await {
                                Ok(response) => {
                                    self.decision_log.record(DecisionRecord::new(
                                        question, context, response.clone(),
                                        DecisionSource::LlmFallback,
                                        "Answered by BMAD Architect agent session".to_string(),
                                        vec!["Rule engine had no matching pattern".to_string()],
                                    ));
                                    Ok(CallToolResult::success(vec![Content::text(
                                        serde_json::json!({ "answer": response, "source": "llm_fallback" }).to_string()
                                    )]))
                                }
                                Err(e) => {
                                    // Step 3: Escalation (LLM failed)
                                    let reason = format!("Architect session failed: {e}");
                                    self.build_escalation_response(question, context, reason)
                                }
                            }
                        }
                        None => {
                            // No LLM fallback — escalate immediately
                            self.build_escalation_response(
                                question, context,
                                "No LLM fallback configured and no rule matched".to_string(),
                            )
                        }
                    }
                }
            }
        }

        fn build_escalation_response(
            &self,
            question: String,
            context: Option<String>,
            reason: String,
        ) -> Result<CallToolResult, rmcp::ErrorData> {
            self.decision_log.record(DecisionRecord::new(
                question.clone(), context, String::new(),
                DecisionSource::Escalation,
                format!("Escalated to human: {reason}"),
                vec!["Rule engine had no matching pattern".to_string()],
            ));
            // Response shape matches AC #1: always includes "answer" + "source"
            // Escalation adds "escalation" + "reason" for SDK session detection
            Ok(CallToolResult::error(vec![Content::text(
                serde_json::json!({
                    "answer": "",
                    "source": "escalation",
                    "escalation": true,
                    "question": question,
                    "reason": reason,
                }).to_string()
            )]))
        }
    }
    ```
    CRITICAL: Escalation returns `CallToolResult::error()` (not `Err(McpError)`) so the LLM agent receives structured context. The response shape is consistent across all three tiers: always includes `"answer"` and `"source"` (escalation sets `answer` to empty string). Escalation adds `"escalation": true`, `"question"`, and `"reason"` for SDK session detection.
  - [x] 2.7 Add `pub mod mcp_server;` to `src/main.rs` module declarations (after `mod mcp;`)

- [x] Task 3: Implement `serve_stdio()` — the server entry point (AC: #8)
  - [x] 3.1 Implement `pub async fn serve_stdio(server: SupervisorMcpServer) -> Result<(), McpServerError>`:
    ```rust
    pub async fn serve_stdio(server: SupervisorMcpServer) -> Result<(), McpServerError> {
        use rmcp::ServiceExt;
        let service = server
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| McpServerError::ServeFailed { reason: e.to_string() })?;
        service.waiting().await
            .map_err(|e| McpServerError::ConnectionClosed { reason: e.to_string() })?;
        Ok(())
    }
    ```
  - [x] 3.2 Define `McpServerError` enum:
    ```rust
    #[derive(Debug, thiserror::Error)]
    pub enum McpServerError {
        #[error("MCP server failed to start: {reason}")]
        ServeFailed { reason: String },
        #[error("MCP server connection closed: {reason}")]
        ConnectionClosed { reason: String },
        #[error("Config error: {0}")]
        Config(#[from] crate::config::ConfigError),
    }
    ```

- [x] Task 4: Add `mcp-supervisor` hidden CLI subcommand (AC: #3, #8)
  - [x] 4.1 Add `McpServer` variant to `CliError` enum in `src/cli/mod.rs`:
    ```rust
    /// MCP server error.
    #[error("MCP server error: {0}")]
    McpServer(#[from] crate::mcp_server::McpServerError),
    ```
  - [x] 4.2 Add `McpSupervisor` variant to `Commands` enum in `src/cli/mod.rs`:
    ```rust
    /// [hidden] Start MCP supervisor server for SDK sessions.
    #[command(hide = true)]
    McpSupervisor {
        /// Story key for decision logging context.
        #[arg(long)]
        story: String,
    },
    ```
    Note: `#[command(hide = true)]` hides from `--help` output — internal use only.
  - [x] 4.3 Add match arm in `src/main.rs`:
    ```rust
    cli::Commands::McpSupervisor { story } => {
        // CRITICAL: tracing MUST write to stderr, never stdout (MCP protocol uses stdout)
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .init();
        cli::run_mcp_supervisor(&cli.config, &story).await?;
    }
    ```
  - [x] 4.4 Implement `pub async fn run_mcp_supervisor(config_path: &Path, story_key: &str) -> Result<(), CliError>` in `src/cli/mod.rs`:
    - Load config via `BotConfig::load(config_path)?`
    - Load secrets via `BotSecrets::load()?` (this calls `dotenvy::dotenv()` internally — loads `.env` from CWD if present, so API keys are available via `std::env::var()`)
    - Validate secrets for config: `secrets.validate_for_config(&config)?`
    - Build `AgentFactory` from config + secrets (needed for ArchitectSession LLM fallback)
    - Create an `McpManager::empty()` (no MCP client connections for the supervisor — anti-recursion)
    - Build optional `AnswerProvider`: check if supervisor provider is an API provider via `!config.llm.supervisor.is_sdk_provider()`. If API, construct `ArchitectSession::new_with_factory(&config, Some(Arc::clone(&factory)), mcp_manager)`. If it fails, log warning and proceed without LLM fallback (rule engine only, escalation on miss). If SDK provider, log warning and skip LLM fallback entirely.
    - Create `DecisionLog::new()` for this session
    - Construct `SupervisorMcpServer::create(answer_provider, decision_log.clone())`
    - Call `mcp_server::serve_stdio(server).await`
    - After server exits (regardless of how — normal disconnect or error), write decisions file if non-empty: `decisions::write_decisions_file(&decision_log.records(), &decisions_path, story_key).await`. This is best-effort — if the process is SIGKILL'd, this won't run, but SIGKILL is a last resort. For normal MCP disconnects (stdin EOF), this code runs reliably.
    - The decisions file path is: `{implementation_artifacts}/{story_key}-SUPERVISOR-DECISIONS.md`
    - Note on signal handling: The MCP server process lifecycle is managed by the SDK CLI (which spawns it as a child process). When the SDK session ends, the CLI closes stdin → rmcp `service.waiting()` returns → post-cleanup runs. If the CLI sends SIGTERM, the process exits before cleanup. This is acceptable — decisions are advisory, and the main story's decisions are committed separately by the pipeline. A full signal handler (like `run_start()`) would add complexity for minimal benefit on a short-lived child process.

- [x] Task 5: Implement MCP config generation for SDK sessions (AC: #3)
  - [x] 5.1 Add `pub fn generate_mcp_supervisor_config(story_key: &str, config_path: &Path, secrets: &BotSecrets) -> serde_json::Value` to `src/mcp_server/mod.rs`:
    ```rust
    pub fn generate_mcp_supervisor_config(
        story_key: &str,
        config_path: &Path,
        secrets: &BotSecrets,
    ) -> serde_json::Value {
        // Build env vars map — inject API keys so the child process has them
        // even if .env is not in CWD. Only include keys that are set.
        let mut env = serde_json::Map::new();
        if let Some(key) = &secrets.anthropic_api_key {
            env.insert("ANTHROPIC_API_KEY".to_string(), serde_json::Value::String(key.clone()));
        }
        if let Some(key) = &secrets.openai_api_key {
            env.insert("OPENAI_API_KEY".to_string(), serde_json::Value::String(key.clone()));
        }

        serde_json::json!({
            "mcpServers": {
                "bmad-supervisor": {
                    "command": std::env::current_exe()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| "bmad-bot".to_string()),
                    "args": [
                        "mcp-supervisor",
                        "--story", story_key,
                        "--config", config_path.to_string_lossy()
                    ],
                    "env": serde_json::Value::Object(env)
                }
            }
        })
    }
    ```
    Notes:
    - Uses `std::env::current_exe()` for the absolute path to `bmad-bot`, falling back to `"bmad-bot"` (relies on `$PATH`).
    - The `env` field is critical: the MCP supervisor child process is spawned by the SDK CLI (not by the daemon), so it does NOT inherit the daemon's process environment. The SDK CLI may have API keys in its env (injected by `SdkRuntime`), but that's not guaranteed for all transports. Explicit `env` injection ensures the supervisor always has the keys it needs for LLM fallback.
    - The child process also calls `BotSecrets::load()` (which does `dotenvy::dotenv()`) as a second line of defense, but this depends on `.env` being in the CWD.
  - [x] 5.2 This function is called by Stories 15.5/15.6 when building `SdkSessionConfig` — they inject it as `--mcp-config '{json}'` (Claude Code) or write it to `.codex/config.toml` (Codex). Not called in this story's production code, but the function must exist and be tested.

- [x] Task 6: Write comprehensive tests (AC: #1-9)
  - [x] 6.1 `test_supervisor_mcp_server_construction` — construct `SupervisorMcpServer::create()` with no AnswerProvider, verify it builds without panic
  - [x] 6.2 `test_supervisor_mcp_server_with_mock_provider` — construct with `MockAnswerProvider`, verify it builds
  - [x] 6.3 `test_handle_question_rule_match` — call `handle_question()` with "Should I proceed?", verify rule engine match, parse response JSON and assert `"source": "rule_engine"` and `"answer"` is non-empty
  - [x] 6.4 `test_handle_question_llm_fallback_success` — construct with `MockAnswerProvider(response: "Use the existing pattern")`, call `handle_question()` with a non-matching question, parse response JSON and assert `"source": "llm_fallback"` and `"answer"` contains the mock response
  - [x] 6.5 `test_handle_question_llm_fallback_failure_escalates` — construct with `MockAnswerProvider(should_fail: true)`, call with non-matching question, verify `CallToolResult.is_error` is true, parse JSON and assert `"escalation": true`, `"answer": ""`, `"source": "escalation"`
  - [x] 6.6 `test_handle_question_no_provider_escalates` — construct with `None` AnswerProvider, call with non-matching question, verify `CallToolResult.is_error` is true, assert same escalation JSON shape
  - [x] 6.7 `test_decision_logging_on_rule_match` — after rule match, verify `decision_log.records()` has 1 record with `DecisionSource::RuleEngine`
  - [x] 6.8 `test_decision_logging_on_llm_fallback` — after LLM fallback, verify record with `DecisionSource::LlmFallback`
  - [x] 6.9 `test_decision_logging_on_escalation` — after escalation, verify record with `DecisionSource::Escalation`
  - [x] 6.10 `test_escalation_response_includes_answer_field` — verify ALL three response paths include `"answer"` key in JSON (consistency check for AC #1)
  - [x] 6.11 `test_generate_mcp_supervisor_config_structure` — verify generated JSON has `mcpServers.bmad-supervisor.command` and `.args` with correct story key and `--config` path
  - [x] 6.12 `test_generate_mcp_supervisor_config_includes_env_vars` — construct `BotSecrets` with both API keys, verify generated JSON has `mcpServers.bmad-supervisor.env.ANTHROPIC_API_KEY` and `.OPENAI_API_KEY`
  - [x] 6.13 `test_generate_mcp_supervisor_config_no_keys` — construct `BotSecrets` with no API keys, verify `env` object is empty (no `None` values serialized)
  - [x] 6.14 `test_server_info` — call `get_info()` on the server, verify capabilities include tools, server name is "bmad-supervisor"
  - [x] 6.15 `test_ask_supervisor_params_deserialization` — verify `AskSupervisorParams` deserializes from JSON with `question` only and with `question` + `context`
  - [x] 6.16 `test_mcp_server_error_display` — verify `McpServerError` Display output for each variant
  - [x] 6.17 Verify all 1358+ existing tests still pass with zero changes — 1374 total (16 new + 1358 existing)

- [x] Task 7: Verify full test suite (AC: #7, #9)
  - [x] 7.1 Run `cargo clippy -- -D warnings` — zero new clippy lints (1 dead_code warning on `generate_mcp_supervisor_config` — API for Stories 15.5/15.6, consistent with 40+ pre-existing dead_code warnings)
  - [x] 7.2 Run `cargo test` — 1374 unit tests + 135 integration tests pass, 0 failures
  - [x] 7.3 Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Architecture Decision Reference

This story implements **Decision 13: Supervisor MCP Server — stdio Transport for SDK Sessions**.
[Source: architecture.md#Decision 13]

The MCP server exposes the supervisor's `ask_supervisor` capability via stdio transport so SDK-mode sessions (Claude Code, Codex) can access it via their native MCP integration. The supervisor logic (rule engine → LLM fallback → escalation) is identical to the rig tool — only the transport changes.

### Design: MCP Server Architecture

```
SDK Session (Claude Code / Codex)
    └── calls `ask_supervisor` via MCP (stdio JSON-RPC)
            └── bmad-bot mcp-supervisor (child process)
                    └── SupervisorMcpServer
                            ├── RuleEngine::evaluate()      ← Tier 1: deterministic
                            ├── AnswerProvider::ask()        ← Tier 2: LLM fallback (ArchitectSession)
                            └── Escalation (error response)  ← Tier 3: human needed
```

The MCP server runs as a separate `bmad-bot mcp-supervisor` process, started by the SDK session's MCP infrastructure. It is NOT a long-running daemon — it lives for the duration of the SDK session.

### rmcp Server Implementation Pattern

The project already uses `rmcp` as a CLIENT (`features = ["client", "transport-child-process"]`). Adding `"server"`, `"transport-io"`, and `"macros"` features enables server-side functionality.

**Feature dependency chain (rmcp uses explicit features, not defaults):**
- `server` → `transport-async-rw` + `schemars` + `pastey` (types, async transport, JSON schema)
- `macros` → `rmcp-macros` + `pastey` (proc macros: `#[tool]`, `#[tool_router]`, `#[tool_handler]`)
- `transport-io` → `transport-async-rw` + `tokio/io-std` (stdio server transport)

`server` does NOT pull in `macros` — they are independent features. Both are default features of rmcp, but since this project specifies features explicitly, both must be listed.

**Key rmcp server types:**
- `rmcp::ServerHandler` — trait to implement for the server
- `rmcp::ServiceExt` — provides `.serve()` method
- `rmcp::transport::stdio()` — returns `(tokio::io::Stdin, tokio::io::Stdout)` for stdio transport
- `rmcp::model::CallToolResult` — tool result with `.success()` and `.error()` constructors
- `rmcp::model::Content` — content type with `::text()` for string content
- `rmcp::handler::server::wrapper::Parameters` — extracts typed params from tool calls
- `rmcp::handler::server::router::tool::ToolRouter` — routes tool calls to handler methods
- `#[tool_router]`, `#[tool_handler]`, `#[tool]` — procedural macros for tool definition

**Server struct requirements:**
- Must implement `Clone` AND `Debug` (rmcp shares handler across requests, `ServerHandler` has `Debug` bound)
- Must have a `tool_router: ToolRouter<Self>` field when using full `#[tool_router]` + `#[tool_handler]` pattern
- Parameter structs must derive `serde::Deserialize` and `rmcp::schemars::JsonSchema` (note: rmcp v1.x uses schemars 1.2.x, not 0.8.x)

**Stdio transport pattern:**
```rust
let service = server.serve(rmcp::transport::stdio()).await?;
service.waiting().await?;  // blocks until disconnect
```

### CRITICAL: Stdout is the MCP Protocol Channel

The MCP server uses stdio transport — stdout is reserved for JSON-RPC messages. **All logging must go to stderr**, never stdout. Use:
```rust
tracing_subscriber::fmt()
    .with_writer(std::io::stderr)
    .with_ansi(false)
    .init();
```

This is configured in the `run_mcp_supervisor()` CLI handler, NOT in the MCP server module. The module trusts the caller to configure logging correctly.

### Anti-Recursion Guard

**The supervisor's own ArchitectSession does NOT receive MCP config.** The supervisor IS the MCP backend — passing it its own config would create an infinite loop. Specifically:

1. The `ArchitectSession` used for LLM fallback is constructed with `McpManager::empty()` — no MCP connections
2. The `AgentFactory` for the supervisor uses the supervisor's LLM provider config (typically API-mode: anthropic/openai-compatible)
3. If the supervisor is itself configured as an SDK provider (claude-code/codex), the LLM fallback cannot work — log an error and proceed with rule engine only (escalation on miss). This edge case is documented but NOT blocked by config validation (Story 15.8 will guide users away from this)

### Supervisor Provider Edge Case

The ArchitectSession constructs a rig agent via `AgentFactory::build(LlmRole::Supervisor, ...)`. This works when the supervisor's provider is API-based (anthropic, openai-compatible). If the supervisor is configured with an SDK provider, `AgentFactory` cannot build a rig agent for it — the ArchitectSession will fail to construct.

**Handling:** In `run_mcp_supervisor()`, attempt to build the `ArchitectSession`. If construction fails (e.g., SDK provider configured for supervisor), log a warning:
```
"Supervisor LLM fallback not available: {error}. Rule engine will handle questions; unmatched questions will escalate."
```
Proceed with `answer_provider: None`. This is the correct behavior — the rule engine covers most questions, and escalation is the safe fallback.

### Existing Supervisor Code Reuse

This story reuses existing types extensively — NO duplication:

| Existing Type | Location | Usage in MCP Server |
|---|---|---|
| `RuleEngine` | `supervisor/rules.rs` | Tier 1 evaluation (wrapped in Arc for Clone) |
| `RuleResult` | `supervisor/rules.rs` | Pattern matching on evaluate() result |
| `AnswerProvider` trait | `supervisor/architect.rs` | Tier 2 LLM fallback (wrapped in Arc for Clone) |
| `ArchitectSession` | `supervisor/architect.rs` | Concrete AnswerProvider implementation |
| `MockAnswerProvider` | `supervisor/architect.rs` | Test mock |
| `DecisionLog` | `supervisor/decisions.rs` | Thread-safe decision recording |
| `DecisionRecord` | `supervisor/decisions.rs` | Individual decision entry |
| `DecisionSource` | `supervisor/decisions.rs` | Source enum (RuleEngine/LlmFallback/Escalation) |
| `write_decisions_file()` | `supervisor/decisions.rs` | Persist decisions to markdown |
| `AgentFactory` | `llm/agent_factory.rs` | Build rig agent for ArchitectSession |
| `BotConfig` | `config/mod.rs` | Load daemon config |
| `BotSecrets` | `config/mod.rs` | Load API keys |

### Difference Between MCP Server and Rig Tool

| Aspect | Rig Tool (`AskSupervisor`) | MCP Server (`SupervisorMcpServer`) |
|---|---|---|
| Transport | In-process rig tool call | JSON-RPC over stdio |
| Lifecycle | Lives within the agent session | Separate child process |
| Escalation | Returns `SupervisorError::EscalationRequired` → rig stops agent loop | Returns `CallToolResult::error()` with JSON context |
| EscalationSlot | Writes to shared `Arc<Mutex<Option<EscalationInfo>>>` for session loop detection | Not needed — SDK session handles tool errors natively |
| Clone | Not required (Serialize/Deserialize) | Required (rmcp shares handler) |
| Decision persistence | Session module calls `write_decisions_file()` | MCP server calls `write_decisions_file()` on exit |

### `AnswerProvider` Trait — Arc Wrapping for Clone

The `AnswerProvider` trait is `Send + Sync + Debug` but not `Clone`. Since `SupervisorMcpServer` must be `Clone` (rmcp requirement), the provider is wrapped in `Arc<dyn AnswerProvider>`:

```rust
answer_provider: Option<Arc<dyn AnswerProvider>>,
```

`Arc<dyn AnswerProvider>` is `Clone + Send + Sync` — satisfies all rmcp requirements.

### MCP Config Generation — `current_exe()` + API Key Injection

The `generate_mcp_supervisor_config()` function uses `std::env::current_exe()` to resolve the absolute path to the `bmad-bot` binary. This is critical because the MCP server child process is spawned by the SDK CLI (not by the daemon directly), so it may not have `bmad-bot` in its `$PATH`. Fallback to `"bmad-bot"` if `current_exe()` fails.

The `env` field explicitly injects API keys from `BotSecrets` into the MCP config. This ensures the child process has the keys regardless of whether `.env` is in its CWD. The chain is: daemon loads `.env` → passes keys to SDK CLI via env → SDK CLI spawns MCP server with env from config. Without explicit `env`, the MCP server relies on `dotenvy::dotenv()` finding `.env` in its CWD — fragile if the SDK CLI changes working directory.

### Module Placement Decision

New module: `src/mcp_server/mod.rs` — separate from the existing `src/mcp/` module (MCP client).

- `src/mcp/` = MCP CLIENT (connects to external MCP servers, discovers tools for the agent)
- `src/mcp_server/` = MCP SERVER (exposes `ask_supervisor` to external CLI sessions)

The naming is explicit to avoid confusion. The architecture document specifies `src/mcp_server/` as the target module.

### Previous Story Intelligence

**Story 15.3** (SDK runtime subprocess infrastructure — done):
- `SdkRuntime::execute_session()` spawns CLI subprocesses with env vars, NDJSON streaming, graceful shutdown
- `SdkSessionConfig` carries command, args, env, working directory, timeouts
- `SdkOutputEvent` enum for parsed CLI output
- 16 new tests, 1358 total passing
- Commit convention: `feat(epic-15): description (Story 15.N)`

**Story 15.2** (config extension — done):
- `LlmRoleConfig` has `cli_path: Option<String>` for custom CLI paths
- `is_sdk_provider()` returns `true` for `"claude-code"` and `"codex"`
- `resolve_cli_name()` maps `"claude-code"` → `"claude"`, `"codex"` → `"codex"`

**Story 15.1** (runtime abstraction — done):
- `SessionRuntime` enum with `Api(Box<ApiRuntime>)` and `Sdk(SdkRuntime)` variants
- `SkillPaths::resolve()` reads BMAD manifest for skill directory
- `SessionContext` carries story, base_branch_override, consultations, role, initial_phase

**Story 9.1** (MCP client — done):
- `McpManager::init()` connects to external MCP servers via stdio transport
- `McpManager::empty()` returns a manager with no connections (used in tests and now for anti-recursion)
- Uses `rmcp` crate with `client` + `transport-child-process` features

### Git Intelligence

Recent commits:
- `fa6ae46 feat(epic-15): add SDK runtime subprocess infrastructure (Story 15.3)`
- `8158b7f feat(epic-15): extend config for SDK providers claude-code and codex (Story 15.2)`
- `6ac5e0e feat(epic-15): add SessionRuntime abstraction layer with SkillPaths resolution (Story 15.1)`

Convention for this story: `feat(epic-15): add supervisor MCP server for SDK sessions (Story 15.4)`

### Current Module State

**`src/supervisor/mod.rs`** (~420 lines, ~20 tests):
- `AskSupervisor` struct: implements `rig::tool::Tool` with 3-tier cascade
- `AskSupervisorArgs { question: String, context: Option<String> }`
- `SupervisorError` enum: `RuleEngineError`, `EscalationRequired`, `LlmFallbackNotImplemented`
- `EscalationSlot` type alias: `Arc<Mutex<Option<EscalationInfo>>>`
- Constructor: `with_architect_from_config(config, factory, escalation_slot, decision_log, mcp_manager)`

**`src/supervisor/rules.rs`** (~300 lines, ~16 tests):
- `RuleEngine` struct with `evaluate(&self, question: &str) -> RuleResult`
- `RuleResult::Matched { rule_name, answer }` / `RuleResult::NoMatch`
- `RulePattern` enum: `Contains`, `StartsWithAny`, `AnyOf`
- Default rules: ~8 covering confirmation, permission, step-by-step, story selection

**`src/supervisor/architect.rs`** (~340 lines, ~12 tests):
- `ArchitectSession` struct: implements `AnswerProvider`
- `AnswerProvider` trait: `async fn ask(&self, question: &str, context: Option<&str>) -> Result<String, ArchitectSessionError>`
- `MockAnswerProvider` struct: `{ response: String, should_fail: bool }`
- Constructor: `new_with_factory(config, factory, mcp_manager)`
- Uses `AgentFactory` for centralized provider construction

**`src/supervisor/decisions.rs`** (~200 lines, ~8 tests):
- `DecisionLog` struct (Clone via `Arc<Mutex<Vec<DecisionRecord>>>`)
- `DecisionRecord::new(question, context, answer, source, reasoning, alternatives)`
- `DecisionSource` enum: `RuleEngine { rule_name }`, `LlmFallback`, `Escalation`
- `write_decisions_file(decisions, output_path, story_key) -> Result<(), DecisionError>`

**`src/cli/mod.rs`** (~1430 lines):
- `Commands` enum: `Init`, `Start`, `Status`, `Logs { level, tail }`
- `run_start()` at line 1248: loads config, starts MCP manager, builds pipeline
- `CliError` enum with `Config`, `TracingInit`, `Signal`, `Init`, `Io` variants

**`src/main.rs`** (52 lines):
- Module declarations: `mod cli; mod config; ... mod mcp; ... mod runtime; mod supervisor; ...`
- Match on `cli.command`: `Start`, `Init`, `Status`, `Logs`

**`Cargo.toml`** (41 lines):
- `rmcp = { version = "1", features = ["client", "transport-child-process"] }` — add `"server"`, `"transport-io"`, `"macros"`
- `rig-core = { version = "0.35", features = ["rmcp"] }`
- tokio with `features = ["full"]`

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Zero-warning policy: `#![deny(clippy::all)]` at crate root
- All tests inline in `#[cfg(test)] mod tests { ... }` at bottom of each module
- Async tests use `#[tokio::test]` (tests 6.3-6.10 call async `handle_question()`)
- Sync tests use `#[test]` (tests 6.1-6.2, 6.11-6.16)
- Use `MockAnswerProvider` from `supervisor::architect` for LLM fallback tests
- Use `DecisionLog::new()` for fresh decision state in each test
- New tests in `src/mcp_server/mod.rs`

### Anti-Patterns to Avoid

- Do NOT duplicate the rule engine or decision logging logic — reuse `supervisor::rules::RuleEngine` and `supervisor::decisions::DecisionLog` directly
- Do NOT implement the `EscalationSlot` mechanism in the MCP server — SDK sessions handle tool errors natively; the MCP server returns `CallToolResult::error()` instead
- Do NOT use `println!()` or `eprintln!()` anywhere in the MCP server module — tracing to stderr is configured by the caller (`run_mcp_supervisor`)
- Do NOT start any MCP CLIENT connections from the MCP server process — use `McpManager::empty()` for the ArchitectSession
- Do NOT pass MCP config to the supervisor's own ArchitectSession — this would cause infinite recursion
- Do NOT add `Err(McpError::...)` for escalation — use `CallToolResult::error()` so the LLM receives structured context instead of a protocol error
- Do NOT modify `src/supervisor/mod.rs`, `src/supervisor/rules.rs`, `src/supervisor/architect.rs`, or `src/supervisor/decisions.rs` — all types are reused as-is
- Do NOT modify `src/mcp/` (existing MCP client) — the MCP server is a separate module
- Do NOT modify `src/pipeline.rs`, `src/session/runner.rs`, or `src/runtime/sdk.rs` — MCP config injection into SDK sessions is Story 15.5/15.6 scope
- Do NOT add `nightly` or unstable Rust features
- Do NOT use `#[allow(dead_code)]` — all public items are either called from `main.rs` or tested

### Deferred Items

From this story scope — handled by later stories:
- **MCP config injection into SDK sessions** — Story 15.5 (Claude Code: `--mcp-config`) and Story 15.6 (Codex: `.codex/config.toml`) will call `generate_mcp_supervisor_config()` when building `SdkSessionConfig`
- **Pipeline integration of MCP supervisor** — Story 15.7 will wire the MCP supervisor server lifecycle into the pipeline for SDK sessions
- **Config validation of supervisor SDK provider edge case** — Story 15.8 (init command) will guide users away from configuring the supervisor as an SDK provider
- **WAL persistence of MCP server decisions** — Story 15.7 will integrate decision file persistence into the pipeline shutdown flow

### Project Structure Notes

New files to create:
- `src/mcp_server/mod.rs` — `SupervisorMcpServer`, `McpServerError`, `AskSupervisorParams`, `serve_stdio()`, `generate_mcp_supervisor_config()`, tests

Files to modify:
- `src/main.rs` — add `mod mcp_server;`, add `Commands::McpSupervisor` match arm
- `src/cli/mod.rs` — add `McpSupervisor` variant to `Commands` enum, implement `run_mcp_supervisor()`
- `Cargo.toml` — add `"server"`, `"transport-io"`, and `"macros"` features to `rmcp`

Files NOT to modify:
- `src/supervisor/*` — all types reused as-is, no changes needed
- `src/mcp/*` — existing MCP client untouched
- `src/runtime/*` — SDK runtime unchanged
- `src/pipeline.rs` — pipeline integration is Story 15.7
- `src/session/*` — session code untouched
- `src/config/mod.rs` — config unchanged
- `src/tools/*` — tool implementations untouched
- `src/ui/*` — UI module unchanged
- `_bmad/` — read-only, never modified

### References

- [Source: architecture.md#Decision 13 — Supervisor MCP Server, stdio transport, anti-recursion guard]
- [Source: architecture.md#Decision 12 — Dual Runtime Abstraction, SessionRuntime enum]
- [Source: architecture.md#Decision 1 — Supervisor Interception, SDK mode amendment]
- [Source: planning-artifacts/sprint-change-proposal-2026-04-26.md — Story 15.4 definition]
- [Source: planning-artifacts/epics.md#Epic 15, Story 15.4 — Supervisor MCP Server]
- [Source: src/supervisor/mod.rs — AskSupervisor struct, 3-tier cascade, EscalationSlot]
- [Source: src/supervisor/rules.rs — RuleEngine, RuleResult, RulePattern]
- [Source: src/supervisor/architect.rs — ArchitectSession, AnswerProvider trait, MockAnswerProvider]
- [Source: src/supervisor/decisions.rs — DecisionLog, DecisionRecord, DecisionSource, write_decisions_file()]
- [Source: src/mcp/mod.rs — existing MCP client module (separate from new mcp_server)]
- [Source: src/mcp/manager.rs — McpManager::init(), McpManager::empty(), MCP client lifecycle]
- [Source: src/cli/mod.rs — Commands enum, Cli struct (clap derive), run_start() lifecycle]
- [Source: src/main.rs — module declarations, command dispatch]
- [Source: src/config/mod.rs:777-789 — BotSecrets struct (API keys)]
- [Source: src/llm/agent_factory.rs — AgentFactory::new(), AgentFactory::build(), LlmRole enum]
- [Source: Cargo.toml:9 — rmcp dependency with current features]
- [Source: _bmad-output/project-context.md — Project rules and conventions]
- [Source: _bmad-output/implementation-artifacts/15-3-sdk-runtime-subprocess-infrastructure.md — Previous story context]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- `schemars` crate needed as direct dependency — `rmcp::schemars::JsonSchema` derive macro generates code referencing `schemars` as a crate directly.
- `CallToolResult.is_error` is `Option<bool>` in rmcp v1.4.0, not `bool` — tests use `assert_eq!(result.is_error, Some(true))`.
- `ServerInfo.server_info` is `Implementation` directly (not `Option`) in rmcp v1.4.0 — adjusted test accordingly.

### Completion Notes List

- Implemented `SupervisorMcpServer` with `#[tool_router]`/`#[tool_handler]` rmcp macros exposing `ask_supervisor` MCP tool
- 3-tier cascade (rule engine → LLM fallback → escalation) identical to rig-based `AskSupervisor` tool, all types reused from `supervisor/` module
- Escalation returns `CallToolResult::error()` with structured JSON including `answer`, `source`, `escalation`, `question`, `reason`
- Hidden CLI subcommand `bmad-bot mcp-supervisor --story <key>` with tracing to stderr (stdout reserved for MCP JSON-RPC)
- `run_mcp_supervisor()` constructs supervisor with anti-recursion guard (McpManager::empty), detects SDK provider edge case
- `generate_mcp_supervisor_config()` creates MCP config JSON with `current_exe()` path and API key injection for SDK sessions
- `serve_stdio()` starts MCP server on stdio transport, blocks until client disconnects
- Best-effort decision persistence to `{story_key}-SUPERVISOR-DECISIONS.md` on server exit
- 16 new tests covering all acceptance criteria, 1374 total tests passing

### File List

New files:
- `src/mcp_server/mod.rs` — SupervisorMcpServer, McpServerError, AskSupervisorParams, serve_stdio(), generate_mcp_supervisor_config(), 16 tests

Modified files:
- `Cargo.toml` — Added `"server"`, `"transport-io"`, `"macros"` features to rmcp; added `schemars = "1"` dependency
- `src/main.rs` — Added `mod mcp_server;` declaration and `Commands::McpSupervisor` match arm with stderr tracing
- `src/cli/mod.rs` — Added `McpSupervisor` command variant, `McpServer` error variant, `run_mcp_supervisor()` function

### Review Findings

- [x] [Review][Defer] `validate_for_config()` potentiellement trop strict pour le contexte MCP supervisor [src/cli/mod.rs:1259] — deferred, le subprocess MCP n'a besoin que des secrets LLM supervisor, mais validate_for_config vérifie tous les secrets configurés (Telegram, Git provider, etc.)
- [x] [Review][Defer] `story_key` non sanitisé avant construction de chemin fichier [src/cli/mod.rs:1308] — deferred, pre-existing pattern (story_key vient du sprint-status.yaml contrôlé par le daemon)
- [x] [Review][Defer] `generate_mcp_supervisor_config` dead code warning en production [src/mcp_server/mod.rs:240] — deferred, API pour Stories 15.5/15.6 (call sites pas encore implémentés)
- [x] [Review][Defer] Pas de test d'intégration stdio end-to-end [src/mcp_server/mod.rs:213] — deferred, E2E tests sont manual-launch-only dans ce projet

### Change Log

- 2026-04-27: Code review complete — 0 patch, 4 defer, 19 dismissed
- 2026-04-26: Story 15.4 implementation — Supervisor MCP Server for SDK sessions
