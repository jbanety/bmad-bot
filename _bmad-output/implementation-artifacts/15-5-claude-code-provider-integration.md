# Story 15.5: Claude Code Provider Integration

Status: done

## Story

As a daemon operator,
I want to use `provider: claude-code` for any LLM role so the daemon delegates sessions to the Claude Code CLI,
so that I benefit from Claude Code's built-in tools, context management, and agentic capabilities.

## Acceptance Criteria

1. **Given** a role is configured with `provider: claude-code` **When** the daemon runs a session for that role **Then** `SdkRuntime::run_session()` constructs a `SdkSessionConfig` with Claude Code CLI invocation **And** calls `execute_session()` with a Claude Code-specific line parser **And** maps `SdkSessionResult` to `SessionOutcome`

2. **Given** the daemon composes the CLI command **When** the session starts **Then** it invokes: `claude -p "{prompt}" --output-format stream-json --allowedTools "Read,Edit,Write,Bash,Grep,Glob,WebSearch,Agent,Skill,Monitor,ToolSearch" --permission-mode acceptEdits --model {configured_model} --cd {project_root}` **And** Claude Code is launched **without `--bare`** so it discovers project skills (`.claude/skills/`), `CLAUDE.md`, and conventions natively **And** `--max-turns 200` prevents runaway sessions

3. **Given** the supervisor MCP server is available **When** the CLI is invoked **Then** `--mcp-config '{supervisor_mcp_json}'` is passed so the session can call `ask_supervisor` **And** the MCP config JSON is generated via `generate_mcp_supervisor_config()` from `mcp_server` module

4. **Given** skills are invoked via native slash commands **When** the daemon composes the prompt **Then** it uses `/bmad-dev-story`, `/bmad-create-story`, `/bmad-code-review` with story-specific context (story file path, branch name) **And** `--append-system-prompt "OVERRIDE: communication_language = English"` is added to enforce English output from the agent (matching API mode's language override behavior) **And** NO system preamble, NO inlined skill content, NO tool usage rules

5. **Given** the session produces streaming JSON output **When** each line is parsed **Then** the Claude Code parser extracts `SdkOutputEvent` variants from Claude Code's event types: `system/init` → `SessionStarted`, `assistant` with `tool_use` → `ToolCall`, `user` with tool results → `ToolResult`, `result/success` → `Completion`, `result/error_*` → `Error`, `system/api_retry` → `Progress`

6. **Given** the session completes successfully **When** the `result` event has `subtype: "success"` **And** the process exits with code 0 **Then** `SessionOutcome::Completed` is returned with `branch` from `StoryInfo.branch_name`, `decisions` parsed from the MCP supervisor decisions markdown file into `Vec<DecisionRecord>`, `pr_context` from the result text (first 2000 chars) **And** if no decisions file exists (no supervisor calls), `decisions` is empty vec

7. **Given** the session fails **When** the process exits with non-zero code **Or** the `result` event has `is_error: true` **Then** `SessionOutcome::Failed` is returned with the error details from stderr or `result.errors[]`

8. **Given** escalation was triggered during the session **When** the decisions file contains a `DecisionSource::Escalation` record **Then** `SessionOutcome::Escalated` is returned with an `EscalationReport` built from the escalation decision record

9. **Given** the CLI path can be overridden via config **When** `cli_path` is set on the role's `LlmRoleConfig` **Then** the command uses that path instead of the default `"claude"` **And** `resolve_cli_name("claude-code")` provides the default `"claude"`

10. **Given** all existing tests pass **When** the Claude Code provider is added **Then** zero behavioral changes for existing API-mode configurations — all 1374+ existing unit tests pass identically

## Tasks / Subtasks

- [x] Task 1: Create `src/runtime/sdk_claude.rs` with Claude Code provider types (AC: #1, #2, #5)
  - [x] 1.1 Create `src/runtime/sdk_claude.rs` with module doc comment
  - [x] 1.2 Define `ClaudeCodeEvent` — internal deserialization struct for Claude Code streaming JSON:
    ```rust
    #[derive(Debug, Deserialize)]
    struct ClaudeCodeEvent {
        #[serde(rename = "type")]
        event_type: String,
        #[serde(default)]
        subtype: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        message: Option<ClaudeCodeMessage>,
        #[serde(default)]
        result: Option<String>,
        #[serde(default)]
        is_error: Option<bool>,
        #[serde(default)]
        errors: Option<Vec<String>>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        total_cost_usd: Option<f64>,
    }

    #[derive(Debug, Deserialize)]
    struct ClaudeCodeMessage {
        #[serde(default)]
        content: Vec<ClaudeCodeContentBlock>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type")]
    enum ClaudeCodeContentBlock {
        #[serde(rename = "text")]
        Text { text: String },
        #[serde(rename = "tool_use")]
        ToolUse { name: String, #[serde(default)] id: String },
        #[serde(other)]
        Other,
    }
    ```
    Notes:
    - These are private types — only the parser function is public.
    - `ClaudeCodeContentBlock` uses `#[serde(tag = "type")]` for inline tagging, with `#[serde(other)]` to handle unknown content block types gracefully.
    - `ClaudeCodeEvent` flattens all fields at the top level — Claude Code events have varying shapes but always have `type`.
  - [x] 1.3 Implement `pub fn parse_claude_code_line(line: &str) -> Option<SdkOutputEvent>` — the parser function passed to `execute_session()`:
    ```rust
    pub fn parse_claude_code_line(line: &str) -> Option<SdkOutputEvent> {
        let event: ClaudeCodeEvent = serde_json::from_str(line).ok()?;
        match event.event_type.as_str() {
            "system" => match event.subtype.as_deref() {
                Some("init") => {
                    let session_id = event.session_id?;
                    Some(SdkOutputEvent::SessionStarted { session_id })
                }
                Some("api_retry") => Some(SdkOutputEvent::Progress {
                    message: "API retry in progress".to_string(),
                }),
                _ => None, // compact_boundary, other system events — skip
            },
            "assistant" => {
                // Extract first tool_use from content blocks if present
                let message = event.message?;
                for block in &message.content {
                    if let ClaudeCodeContentBlock::ToolUse { name, .. } = block {
                        return Some(SdkOutputEvent::ToolCall {
                            tool_name: name.clone(),
                            detail: String::new(),
                        });
                    }
                }
                // Text-only assistant message — progress
                for block in &message.content {
                    if let ClaudeCodeContentBlock::Text { text } = block {
                        if !text.is_empty() {
                            // UTF-8 safe truncation — never slice at byte offset
                            let truncated = if text.chars().count() > 200 {
                                let end: String = text.chars().take(200).collect();
                                format!("{end}...")
                            } else {
                                text.clone()
                            };
                            return Some(SdkOutputEvent::Progress { message: truncated });
                        }
                    }
                }
                None
            }
            "user" => {
                // Tool result messages — we only emit a ToolResult event
                Some(SdkOutputEvent::ToolResult {
                    tool_name: String::new(),
                    detail: String::new(),
                })
            }
            "result" => {
                let is_error = event.is_error.unwrap_or(false);
                if is_error {
                    let error_msg = event.errors
                        .and_then(|e| e.into_iter().next())
                        .unwrap_or_else(|| "Unknown error".to_string());
                    Some(SdkOutputEvent::Error { message: error_msg })
                } else {
                    Some(SdkOutputEvent::Completion {
                        result: event.result.unwrap_or_default(),
                        is_error: false,
                    })
                }
            }
            _ => None, // stream_event, status, hook events — skip
        }
    }
    ```
    Notes:
    - Returns `None` for unrecognized or irrelevant events (stream deltas, hooks, etc.)
    - `serde_json::from_str().ok()?` silently skips non-JSON lines (CLI banners, warnings)
    - Only the first `tool_use` block per assistant message is reported — multiple tool uses in one turn are common but we only need one UI event per assistant message
    - `user` messages (tool results) are aggregated as generic ToolResult — the tool name is already tracked from the preceding ToolCall

- [x] Task 2: Implement `build_claude_code_config()` — session config builder (AC: #2, #3, #4, #9)
  - [x] 2.1 Implement `pub fn build_claude_code_config(role_config: &LlmRoleConfig, project_root: &Path, prompt: &str, mcp_config_path: Option<&Path>) -> SdkSessionConfig`:
    ```rust
    pub fn build_claude_code_config(
        role_config: &LlmRoleConfig,
        project_root: &Path,
        prompt: &str,
        mcp_config_path: Option<&Path>,
    ) -> SdkSessionConfig {
        let command = role_config.cli_path.clone()
            .unwrap_or_else(|| "claude".to_string());

        let mut args = vec![
            "-p".to_string(),
            prompt.to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--model".to_string(),
            role_config.model.clone(),
            "--permission-mode".to_string(),
            "acceptEdits".to_string(),
            "--allowedTools".to_string(),
            "Read,Edit,Write,Bash,Grep,Glob,WebSearch,Agent,Skill,Monitor,ToolSearch".to_string(),
            "--max-turns".to_string(),
            "200".to_string(),
            "--cd".to_string(),
            project_root.to_string_lossy().to_string(),
            "--append-system-prompt".to_string(),
            "OVERRIDE: communication_language = English".to_string(),
        ];

        if let Some(mcp_path) = mcp_config_path {
            args.push("--mcp-config".to_string());
            args.push(mcp_path.to_string_lossy().to_string());
        }

        SdkSessionConfig {
            command,
            args,
            env: Vec::new(), // API keys injected by SdkRuntime::merge_env_vars()
            working_directory: project_root.to_path_buf(),
            timeout: Duration::from_secs(30 * 60), // 30 min default
            sigterm_grace: Duration::from_secs(10),
        }
    }
    ```
    Notes:
    - No `--bare` flag: Claude Code discovers `.claude/skills/`, `CLAUDE.md`, and project conventions natively
    - `--allowedTools` auto-approves all core tools for fully autonomous operation
    - `--max-turns 200` prevents runaway sessions (sensible default for story development)
    - `--permission-mode acceptEdits` auto-approves file edits; `--allowedTools` extends auto-approval to Bash and other tools
    - `env` is empty — `SdkRuntime::merge_env_vars()` handles API key injection at subprocess spawn time
    - `cli_path` override respected per config (Story 15.2 added this field)
    - `mcp_config_path` is a file path to a temp file containing the MCP JSON (NOT inline JSON — avoids secrets in `ps aux`). See Task 4.5 for temp file creation.

- [x] Task 3: Implement prompt construction per phase (AC: #4)
  - [x] 3.1 Implement `pub fn build_claude_code_prompt(phase: &str, story: &StoryInfo) -> String`:
    ```rust
    use crate::session::state::{PHASE_CREATE, PHASE_REVIEW};

    pub fn build_claude_code_prompt(phase: &str, story: &StoryInfo) -> String {
        match phase {
            PHASE_CREATE => format!(
                "/bmad-create-story {}",
                story.story_key,
            ),
            PHASE_REVIEW => format!(
                "/bmad-code-review {}",
                story.specs_path.to_string_lossy(),
            ),
            _ => {
                // PHASE_DEV and any consultation phase default to dev-story
                if phase != crate::session::state::PHASE_DEV && !phase.is_empty() {
                    tracing::warn!(phase = %phase, "Unknown phase for Claude Code prompt, defaulting to dev-story");
                }
                format!(
                    "/bmad-dev-story {}",
                    story.specs_path.to_string_lossy(),
                )
            }
        }
    }
    ```
    Notes:
    - Uses `PHASE_CREATE` and `PHASE_REVIEW` constants from `session::state` — not raw strings. This prevents silent breakage if constant values change.
    - `tracing::warn!` on unknown phases (defensive, same pattern as `ApiRuntime::resolve_phase_config`)
    - SDK mode invokes skills via native slash commands — Claude Code discovers SKILL.md files from `.claude/skills/` automatically
    - `create` phase passes the story key for identification (create-story skill discovers from sprint-status)
    - `dev` and `review` phases pass the story specs path for context
    - No system preamble, no inlined skill content, no tool usage rules — Claude Code handles everything natively
    - Consultations (adversarial, critic) are handled by the pipeline as separate sessions — not part of the prompt

- [x] Task 4: Update `SdkRuntime::run_session()` to dispatch Claude Code sessions (AC: #1, #6, #7, #8)
  - [x] 4.1 Replace the stub `run_session()` in `src/runtime/sdk.rs` with provider dispatch logic:
    ```rust
    pub async fn run_session(&self, context: SessionContext<'_>) -> SessionOutcome {
        let provider = self.resolve_provider_for_role(&context.role);
        match provider.as_str() {
            "claude-code" => sdk_claude::run_claude_code_session(self, context).await,
            other => SessionOutcome::Failed {
                story_key: context.story.story_key.clone(),
                error: format!(
                    "SDK provider '{}' not yet implemented. Requires Story 15.6 (codex).",
                    other
                ),
                decisions: vec![],
            },
        }
    }
    ```
    Note: The orchestration function `run_claude_code_session` lives in `sdk_claude.rs`, not in `sdk.rs`. This keeps provider-specific logic contained in its own module and prevents `sdk.rs` from growing with each new provider. The function takes `&SdkRuntime` as its first parameter to access config, secrets, execute_session, etc. This requires making `execute_session()`, `config_for_role()`, `merge_env_vars()`, `config_path`, `config`, `secrets`, `shutdown`, and `ui` accessible — expose them via `pub(crate)` methods or fields.
  - [x] 4.2 Implement `async fn run_claude_code_session(&self, context: SessionContext<'_>) -> SessionOutcome`:
    - Resolve `LlmRoleConfig` for the role via `self.config_for_role(&context.role)`
    - Build prompt via `sdk_claude::build_claude_code_prompt(context.initial_phase, context.story)`
    - Resolve `project_root` to absolute path: `std::fs::canonicalize(&self.config.bmad_paths.project_root)` — the config may contain `"."` which must be resolved to an absolute path for `--cd` and MCP config paths. On failure, fall back to the raw value with a `tracing::warn!`.
    - Generate MCP supervisor config via temp file (see Task 4.5)
    - Build `SdkSessionConfig` via `sdk_claude::build_claude_code_config(role_config, &project_root, &prompt, mcp_config_path.as_deref())`
    - Call `self.execute_session(session_config, sdk_claude::parse_claude_code_line).await`
    - **Handle `Result`:** `execute_session()` returns `Result<SdkSessionResult, SdkError>`. On `Err(e)`, immediately return `SessionOutcome::Failed { story_key, error: e.to_string(), decisions: vec![] }`. On `Ok(result)`, proceed to mapping (Task 4.3).
    - Map `SdkSessionResult` → `SessionOutcome` (see Task 4.3)
    - Clean up the temporary MCP config file after the session completes (best-effort, ignore errors)
  - [x] 4.3 Implement result mapping logic — `map_sdk_result_to_outcome()`:
    - After `execute_session()` returns, read decisions from JSON sidecar: `read_decisions_json_sidecar(&impl_artifacts_path, &story_key).await` (see Task 7.2). Returns `Vec<DecisionRecord>` — empty if no file exists.
    - **Escalation check:** Call `detect_escalation(&decisions)` (see Task 7.3). If returns `Some((question, reason))`, build `EscalationReport::new(story_key, question, reason, branch_name, partial_work_summary)` and return `SessionOutcome::Escalated { report, decisions }`.
    - **Success check:** If `result.exit_code == Some(0)` and no escalation, return `SessionOutcome::Completed` with:
      - `story_key`: from context
      - `branch`: from `context.story.branch_name`
      - `decisions`: from the JSON sidecar (populated, NOT empty — pipeline's `format_pr_decisions_section()` reads these for the PR description)
      - `pr_context`: from `SdkSessionResult.completion_text` (the result event's text, truncated to 2000 chars via UTF-8 safe `chars().take(2000)`)
      - `pr_how_to_test`: `None` (SDK sessions don't produce structured test instructions)
      - `pr_additional_info`: `None`
    - **Failure:** Otherwise return `SessionOutcome::Failed` with stderr content and error details, plus `decisions` from the sidecar (decisions may have been recorded before the failure)
  - [x] 4.4 Add `pub(crate) fn config_for_role()` helper method on `SdkRuntime` — returns `&LlmRoleConfig` for a given role (similar to `resolve_provider_for_role()` but returns the config, not just the provider string). Also add `pub(crate)` accessors for `config`, `secrets`, `config_path` fields so `sdk_claude.rs` can access them.
  - [x] 4.5 Implement MCP config via temp file (NOT argv) to avoid secrets in process listing:
    - `generate_mcp_supervisor_config()` produces a JSON value that includes API keys in the `env` field
    - Writing this JSON as a CLI argument (`--mcp-config '{json}'`) exposes API keys in `ps aux` and `/proc/{pid}/cmdline`
    - Instead: write the MCP config JSON to a temporary file (`tempfile::NamedTempFile`) and pass the file path to `--mcp-config {path}`
    - The temp file is automatically cleaned up when the `NamedTempFile` is dropped (after session completes)
    - `--mcp-config` accepts both inline JSON strings and file paths — passing a file path is safer
    - MCP config is only generated when the phase needs supervisor access: `PHASE_CREATE` and `PHASE_DEV` phases need `ask_supervisor`; `PHASE_REVIEW` and consultation phases do NOT (review uses its own LLM role, consultations are daemon-orchestrated). Pass `None` as `mcp_config_path` for review and consultation phases to avoid starting an unnecessary MCP supervisor process.
    - Add `tempfile = "3"` to `Cargo.toml` if not already present (check first — it may be a transitive dependency)

- [x] Task 5: Add `config_path` field to `SdkRuntime` (AC: #3)
  - [x] 5.1 Add `config_path: PathBuf` field to `SdkRuntime` struct:
    ```rust
    pub struct SdkRuntime {
        config: Arc<BotConfig>,
        secrets: Arc<BotSecrets>,
        config_path: PathBuf,
        shutdown: ShutdownFlag,
        ui: UiHandle,
    }
    ```
  - [x] 5.2 Update `SdkRuntime::new()` to accept `config_path: PathBuf` parameter
  - [x] 5.3 Update all `SdkRuntime::new()` call sites — currently only in tests within `sdk.rs` and `mod.rs`. The pipeline construction site (Story 15.7) will wire the real config path.
  - [x] 5.4 The config path is needed by `generate_mcp_supervisor_config()` to pass `--config` to the `bmad-bot mcp-supervisor` subprocess. The MCP subprocess needs to know where to load its config from.

- [x] Task 6: Store completion text from parser for `pr_context` (AC: #6)
  - [x] 6.1 The `execute_session()` parser callback (`Fn(&str) -> Option<SdkOutputEvent>`) is a pure function — it can't accumulate state. To capture the last `Completion` result text, add `last_completion_text: Option<String>` tracking in the stdout event loop of `execute_session()`:
    - After parsing each event, if it's `SdkOutputEvent::Completion { result, .. }`, store `result.clone()` in a local variable
    - After the event loop, store the last completion text in `SdkSessionResult`
  - [x] 6.2 Add `pub completion_text: Option<String>` field to `SdkSessionResult`:
    ```rust
    pub struct SdkSessionResult {
        pub session_id: Option<String>,
        pub exit_code: Option<i32>,
        pub stderr: String,
        pub timed_out: bool,
        pub shutdown_requested: bool,
        pub completion_text: Option<String>,
    }
    ```
  - [x] 6.3 Update all `SdkSessionResult` construction sites (in `execute_session()` and tests) to include the new field

- [x] Task 7: Write JSON sidecar for decisions + escalation detection (AC: #6, #8)
  - [x] 7.1 Add `pub async fn write_decisions_json_sidecar(decisions: &[DecisionRecord], output_dir: &Path, story_key: &str) -> Result<(), DecisionError>` to `src/supervisor/decisions.rs`:
    - Write a JSON sidecar file alongside the existing markdown: `{story_key}-SUPERVISOR-DECISIONS.json`
    - Format: `[{ "question": "...", "answer": "...", "source": "rule_engine|llm_fallback|escalation", "reasoning": "..." }, ...]`
    - The markdown file remains for human readability; the JSON sidecar is for programmatic access
    - Called from `run_mcp_supervisor()` in `cli/mod.rs` right after `write_decisions_file()` (add the call site)
    - IMPORTANT: This is a small, justified modification to `supervisor/decisions.rs` and `cli/mod.rs` — it enables SDK mode to read decisions without fragile markdown parsing. Only adds a new function and one call site.
  - [x] 7.2 Implement `pub async fn read_decisions_json_sidecar(impl_artifacts_dir: &Path, story_key: &str) -> Vec<DecisionRecord>` in `sdk_claude.rs`:
    - Read `{impl_artifacts_dir}/{story_key}-SUPERVISOR-DECISIONS.json`
    - If file doesn't exist, return empty vec (no supervisor calls happened)
    - Deserialize into `Vec<DecisionRecord>` — structured data, not fragile markdown parsing
    - Return the decisions for inclusion in `SessionOutcome.decisions` so the pipeline's `format_pr_decisions_section()` works correctly (the pipeline reads decisions from `SessionOutcome`, NOT from disk)
  - [x] 7.3 Implement `fn detect_escalation(decisions: &[DecisionRecord]) -> Option<(String, String)>` in `sdk_claude.rs`:
    - Scan the parsed decisions for any record with `DecisionSource::Escalation`
    - If found, return `Some((question, reason))` extracted from the `DecisionRecord` fields
    - This replaces the fragile "search for markdown strings" approach with a typed check on structured data

- [x] Task 8: Wire module into runtime (AC: #1)
  - [x] 8.1 Add `pub mod sdk_claude;` to `src/runtime/mod.rs` (after `pub mod sdk;`)
  - [x] 8.2 Remove `#[allow(dead_code)]` from `secrets` field on `SdkRuntime` — it's now used by `run_claude_code_session()` for MCP config generation

- [x] Task 9: Write comprehensive tests (AC: #1-10)
  - [x] 9.1 `test_parse_claude_code_system_init` — parse `{"type":"system","subtype":"init","session_id":"abc-123"}`, verify `SdkOutputEvent::SessionStarted { session_id: "abc-123" }`
  - [x] 9.2 `test_parse_claude_code_system_init_no_session_id` — parse `{"type":"system","subtype":"init"}` (missing session_id), verify returns `None`
  - [x] 9.3 `test_parse_claude_code_assistant_tool_use` — parse assistant message with `tool_use` content block, verify `SdkOutputEvent::ToolCall` with correct tool name
  - [x] 9.4 `test_parse_claude_code_assistant_text_only` — parse assistant message with only text, verify `SdkOutputEvent::Progress`
  - [x] 9.5 `test_parse_claude_code_user_tool_result` — parse `{"type":"user"}`, verify `SdkOutputEvent::ToolResult`
  - [x] 9.6 `test_parse_claude_code_result_success` — parse `{"type":"result","subtype":"success","result":"Done","is_error":false}`, verify `SdkOutputEvent::Completion { is_error: false }`
  - [x] 9.7 `test_parse_claude_code_result_error` — parse `{"type":"result","subtype":"error_max_turns","is_error":true,"errors":["Max turns"]}`, verify `SdkOutputEvent::Error`
  - [x] 9.8 `test_parse_claude_code_api_retry` — parse `{"type":"system","subtype":"api_retry"}`, verify `SdkOutputEvent::Progress`
  - [x] 9.9 `test_parse_claude_code_stream_event_ignored` — parse `{"type":"stream_event"}`, verify returns `None`
  - [x] 9.10 `test_parse_claude_code_invalid_json` — parse `"not json"`, verify returns `None`
  - [x] 9.11 `test_parse_claude_code_unknown_type` — parse `{"type":"unknown_event"}`, verify returns `None`
  - [x] 9.12 `test_build_claude_code_config_basic` — verify command is `"claude"`, args contain `-p`, `--output-format`, `--model`, `--permission-mode`, `--allowedTools`, `--max-turns`, `--cd`
  - [x] 9.13 `test_build_claude_code_config_with_cli_path` — verify `cli_path: Some("/custom/claude")` overrides command
  - [x] 9.14 `test_build_claude_code_config_with_mcp` — verify `--mcp-config` arg added when `mcp_config_path` is `Some`
  - [x] 9.15 `test_build_claude_code_config_without_mcp` — verify no `--mcp-config` arg when `mcp_config_path` is `None`
  - [x] 9.15b `test_build_claude_code_config_has_language_override` — verify args contain `--append-system-prompt` with `"OVERRIDE: communication_language = English"`
  - [x] 9.16 `test_build_claude_code_prompt_create` — verify create phase produces `/bmad-create-story {story_key}`
  - [x] 9.17 `test_build_claude_code_prompt_dev` — verify dev phase produces `/bmad-dev-story {specs_path}`
  - [x] 9.18 `test_build_claude_code_prompt_review` — verify review phase produces `/bmad-code-review {specs_path}`
  - [x] 9.19 `test_run_session_claude_code_dispatches` — mock a simple echo command that outputs a `system/init` + `result/success` JSON, call `run_session()` with provider `"claude-code"`, verify `SessionOutcome::Completed` (requires test-specific override — see Dev Notes on testability)
  - [x] 9.20 `test_run_session_unknown_sdk_provider_fails` — call `run_session()` with provider `"unknown"`, verify `SessionOutcome::Failed`
  - [x] 9.21 `test_config_for_role_returns_correct_config` — verify `config_for_role()` returns the right `LlmRoleConfig` for each role
  - [x] 9.22 `test_detect_escalation_found` — create a `Vec<DecisionRecord>` with one `DecisionSource::Escalation` record, verify `detect_escalation()` returns `Some((question, reason))`
  - [x] 9.23 `test_detect_escalation_not_found` — create decisions with only `RuleEngine` and `LlmFallback` sources, verify `detect_escalation()` returns `None`
  - [x] 9.24 `test_detect_escalation_empty_decisions` — empty vec, verify returns `None`
  - [x] 9.25 `test_read_decisions_json_sidecar_missing_file` — call with non-existent path, verify returns empty vec
  - [x] 9.26 `test_read_decisions_json_sidecar_valid_json` — write a temp JSON file with decision records, read it back, verify round-trip
  - [x] 9.27 `test_write_decisions_json_sidecar` — write decisions, read the file, verify valid JSON array with correct fields
  - [x] 9.28 Verify all 1374+ existing tests still pass with zero changes

- [x] Task 10: Verify full test suite (AC: #10)
  - [x] 10.1 Run `cargo clippy -- -D warnings` — zero new clippy lints
  - [x] 10.2 Run `cargo test` — all existing + new tests pass
  - [x] 10.3 Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Architecture Decision Reference

This story implements the Claude Code provider adapter from **Decision 12: Dual Runtime Abstraction**.
[Source: architecture.md#Decision 12 — SdkClaudeCodeProvider]

The Claude Code CLI is invoked without `--bare` so it discovers project skills, `CLAUDE.md`, and conventions natively. Skills are invoked via native slash commands — no system preamble, no inlined skill content.

### Design: Claude Code Session Flow

```
pipeline.rs → SessionRuntime::Sdk(SdkRuntime)
    └── SdkRuntime::run_session(context)
            └── run_claude_code_session(context)
                    ├── build_claude_code_prompt(phase, story)     ← "/bmad-dev-story {path}"
                    ├── generate_mcp_supervisor_config(...)        ← MCP JSON for --mcp-config
                    ├── build_claude_code_config(role, root, prompt, mcp)
                    │       └── SdkSessionConfig { command: "claude", args: [...], ... }
                    ├── execute_session(config, parse_claude_code_line)
                    │       ├── spawn subprocess (tokio::process::Command)
                    │       ├── stream stdout → parse_claude_code_line → SdkOutputEvent
                    │       ├── emit UI events (UiHandle)
                    │       ├── track session ID from system/init
                    │       └── capture completion text from result event
                    └── map_sdk_result_to_outcome(result, story, decisions)
                            ├── escalation? → SessionOutcome::Escalated
                            ├── success? → SessionOutcome::Completed
                            └── failure → SessionOutcome::Failed
```

### Claude Code CLI Flags Reference

| Flag | Value | Purpose |
|---|---|---|
| `-p` | `"{prompt}"` | Print mode — non-interactive, outputs to stdout |
| `--output-format` | `stream-json` | NDJSON streaming on stdout |
| `--model` | `{configured_model}` | Model override from `LlmRoleConfig.model` |
| `--permission-mode` | `acceptEdits` | Auto-approve file reads and edits |
| `--allowedTools` | `Read,Edit,...` | Auto-approve listed tools (no permission prompts) |
| `--max-turns` | `200` | Prevent runaway sessions |
| `--cd` | `{project_root}` | Working directory for the session |
| `--append-system-prompt` | `"OVERRIDE: communication_language = English"` | Enforce English output (matches API mode language override) |
| `--mcp-config` | `{file_path}` | Path to temp file with MCP server config for supervisor |
| ~~`--bare`~~ | NOT USED | Must NOT use — Claude Code needs to discover skills and CLAUDE.md |

**NOT used:** `--bare` (blocks skill discovery), `--dangerously-skip-permissions` (too permissive), `--resume` (Story 15.7 for crash recovery), `--verbose` (too much output), `--include-partial-messages` (stream deltas not needed).

### Claude Code Streaming JSON Event Types

| Event Type | Subtype | Maps To | Key Fields |
|---|---|---|---|
| `system` | `init` | `SessionStarted` | `session_id` |
| `system` | `api_retry` | `Progress` | `attempt`, `retry_delay_ms` |
| `system` | `compact_boundary` | (ignored) | Context compaction |
| `assistant` | — | `ToolCall` or `Progress` | `message.content[]` |
| `user` | — | `ToolResult` | Tool results |
| `result` | `success` | `Completion` | `result`, `total_cost_usd`, `duration_ms` |
| `result` | `error_*` | `Error` | `errors[]`, `is_error: true` |
| `stream_event` | — | (ignored) | Token deltas (not needed) |

Result subtypes: `success`, `error_max_turns`, `error_during_execution`, `error_max_budget_usd`, `error_max_structured_output_retries`.

### Prompt Construction Per Phase

| Phase | Prompt | Skill Invoked |
|---|---|---|
| `create` | `/bmad-create-story {story_key}` | `bmad-create-story` |
| `dev` (default) | `/bmad-dev-story {specs_path}` | `bmad-dev-story` |
| `review` | `/bmad-code-review {specs_path}` | `bmad-code-review` |

Consultations (adversarial, critic) are separate sessions orchestrated by the pipeline (Story 15.7). They use the same `run_session()` dispatch but with `LlmRole::Review` or `LlmRole::Critic` and a different prompt.

### Config Path Resolution

`generate_mcp_supervisor_config()` requires the config file path so the `bmad-bot mcp-supervisor` subprocess can load the same config. The `SdkRuntime` needs this path.

**Approach:** Add `config_path: PathBuf` to `SdkRuntime`. Currently `BotConfig` is loaded in `run_start()` via `BotConfig::load(config_path)` at `src/cli/mod.rs:1325`. The `config_path` is passed from `Cli.config` (clap-parsed `--config` arg). The pipeline construction happens later but has access to the config path.

For this story, `SdkRuntime::new()` gains a `config_path` parameter. The actual wiring from pipeline to runtime is Story 15.7. Tests use a dummy `PathBuf::from("test-config.yaml")`.

### Decisions File Strategy for SDK Mode

In API mode, `DecisionRecord`s accumulate in-memory via the `DecisionLog` (shared `Arc<Mutex<Vec<DecisionRecord>>>`). The session runner passes them directly to `SessionOutcome`. The pipeline's `format_pr_decisions_section(&decisions)` then reads these in-memory records to build the PR description's "Supervisor Decisions" section.

In SDK mode, the MCP supervisor runs as a separate process. Its decisions are written to `{story_key}-SUPERVISOR-DECISIONS.md` on process exit (Story 15.4). The daemon does NOT share memory with the MCP subprocess.

**CRITICAL:** The pipeline reads decisions from `SessionOutcome.decisions` (in-memory), NOT from disk. Returning `decisions: vec![]` would silently produce empty "Supervisor Decisions" sections in all SDK PRs.

**Strategy — JSON sidecar file:**
1. Add `write_decisions_json_sidecar()` to `supervisor/decisions.rs` — writes a structured `{story_key}-SUPERVISOR-DECISIONS.json` alongside the existing markdown file
2. Call it from `run_mcp_supervisor()` in `cli/mod.rs` right after `write_decisions_file()` (1 new line)
3. After `execute_session()` returns, `read_decisions_json_sidecar()` reads the JSON file back into `Vec<DecisionRecord>`
4. These are returned in `SessionOutcome.decisions` — the pipeline's PR generation works identically to API mode
5. Escalation detection: `detect_escalation()` scans the typed `Vec<DecisionRecord>` for `DecisionSource::Escalation` — no fragile markdown string searching

This approach requires a small, justified modification to `supervisor/decisions.rs` (+1 function) and `cli/mod.rs` (+1 call). The trade-off is correct PR descriptions vs zero modifications to existing code.

### Escalation Detection in SDK Mode

In API mode, escalation is detected via `EscalationSlot` (shared `Arc<Mutex<Option<EscalationInfo>>>`). This doesn't work for SDK mode since the MCP supervisor is a separate process.

**Detection approach (via JSON sidecar):**
1. After `execute_session()` returns, read `{impl_artifacts}/{story_key}-SUPERVISOR-DECISIONS.json` into `Vec<DecisionRecord>` (see Task 7.2)
2. Call `detect_escalation(&decisions)` — scans for any record with `DecisionSource::Escalation` (typed check, not string matching)
3. If found, build `EscalationReport` with: story_key, question (from `DecisionRecord.question`), reason (from `DecisionRecord.reasoning`), branch_name (from `StoryInfo`), partial_work_summary (from session stderr or "SDK session completed with escalation")

**Timing guarantee:** The MCP subprocess writes the JSON sidecar on exit (after `serve_stdio()` returns). The SDK CLI closes MCP connections before exiting. By the time `execute_session()` returns (after `child.wait()`), the MCP supervisor process has already exited and written its files.

### MCP Config Security — Temp File, Not argv

`generate_mcp_supervisor_config()` includes API keys in the `env` field. Passing this as a CLI argument (`--mcp-config '{json}'`) would expose API keys in `ps aux`, `/proc/{pid}/cmdline`, and any process monitor.

**Approach:** Write the MCP config JSON to a `tempfile::NamedTempFile`. Pass the file path to `--mcp-config {path}`. The temp file is automatically deleted when the `NamedTempFile` handle is dropped after the session completes. The `NamedTempFile` must be held alive for the duration of the session (keep it in scope until after `execute_session()` returns).

### Project Root Resolution

`BotConfig.bmad_paths.project_root` is a `String` (commonly `"."`). The `--cd` flag for Claude Code needs an absolute or at least unambiguous path. If the daemon is started from a different working directory than the repo root, `"."` would resolve incorrectly.

**Approach:** In `run_claude_code_session()`, resolve the project root to an absolute path via `std::fs::canonicalize()`. On failure (e.g., path doesn't exist), fall back to the raw value with a `tracing::warn!`. This also ensures the MCP config's `--config` path is absolute.

### Testability Considerations

`run_claude_code_session()` calls `execute_session()` which spawns a real subprocess. For unit tests:
- **Parser tests** (Tasks 9.1-9.11): Test `parse_claude_code_line()` directly with JSON strings — no subprocess needed
- **Config builder tests** (Tasks 9.12-9.18): Test `build_claude_code_config()` and `build_claude_code_prompt()` — pure functions
- **Integration test** (Task 9.19): Use `echo` to produce fake Claude Code JSON output. Build a `BotConfig` with `provider: "claude-code"` and `cli_path` pointing to a test script that outputs the expected NDJSON. This requires a shell wrapper — see test implementation.
- **Provider dispatch test** (Task 9.20): Call `run_session()` with an unknown provider, verify `Failed` outcome

For Task 9.19, create a test helper that writes a temporary shell script:
```bash
#!/bin/sh
echo '{"type":"system","subtype":"init","session_id":"test-session-123"}'
echo '{"type":"result","subtype":"success","result":"All done","is_error":false}'
```
Set `cli_path` to this script in the test config.

### Previous Story Intelligence

**Story 15.4** (Supervisor MCP Server — done):
- `generate_mcp_supervisor_config(story_key, config_path, secrets)` → `serde_json::Value`
- `SupervisorMcpServer::create(answer_provider, decision_log)` — MCP server handler
- `serve_stdio(server)` — blocks on stdio transport
- `run_mcp_supervisor(config_path, story_key)` — CLI handler
- Decisions written to `{story_key}-SUPERVISOR-DECISIONS.md` on exit
- `McpServerError` enum for error handling
- 16 tests, 1374 total passing

**Story 15.3** (SDK runtime — done):
- `SdkRuntime::execute_session(config, parser)` — fully functional subprocess management
- `SdkSessionConfig { command, args, env, working_directory, timeout, sigterm_grace }`
- `SdkOutputEvent` enum — 6 variants for parsed events
- `SdkSessionResult { session_id, exit_code, stderr, timed_out, shutdown_requested }`
- `merge_env_vars()` — API key injection
- `resolve_provider_for_role()` — role → provider string
- `run_session()` — stub returning `SessionOutcome::Failed`
- Graceful shutdown: SIGTERM → SIGKILL with configurable grace period
- UI events emitted via `UiHandle`
- 16 tests, 1358 total

**Story 15.2** (Config extension — done):
- `is_sdk_provider()` → true for "claude-code", "codex"
- `cli_path: Option<String>` on `LlmRoleConfig`
- `resolve_cli_name("claude-code")` → `"claude"`
- `validate_sdk_providers()` checks CLI availability and skill files

**Story 15.1** (Runtime abstraction — done):
- `SessionRuntime::Sdk(SdkRuntime)` dispatch to `sdk.run_session(context)`
- `SessionContext { story, base_branch_override, consultations, role, initial_phase }`
- `SkillPaths` — resolved from BMAD manifest

### Git Intelligence

Recent commits follow `feat(epic-15):` convention:
- `489869d feat(epic-15): add Supervisor MCP Server over stdio (Story 15.4)`
- `fa6ae46 feat(epic-15): add SDK runtime subprocess infrastructure (Story 15.3)`

Convention for this story: `feat(epic-15): add Claude Code provider integration (Story 15.5)`

### Current Module State

**`src/runtime/sdk.rs`** (~450 lines, 16 tests):
- `SdkRuntime` struct: `config`, `secrets`, `shutdown`, `ui`
- `run_session()`: stub at line 159, returns `SessionOutcome::Failed`
- `execute_session()`: fully functional subprocess management at line 172
- `merge_env_vars()`: API key injection at line 120
- `resolve_provider_for_role()`: role → provider at line 134
- `SdkSessionConfig`, `SdkOutputEvent`, `SdkSessionResult`, `SdkError` types

**`src/runtime/mod.rs`** (~400 lines, 11 tests):
- `SessionRuntime` enum: `Api(Box<ApiRuntime>)`, `Sdk(SdkRuntime)` at line 74
- `SessionContext` struct at line 61
- `pub mod sdk;` at line 13

**`src/mcp_server/mod.rs`** (~500 lines, 16 tests):
- `generate_mcp_supervisor_config()` at line 240
- `SupervisorMcpServer` struct and handler
- `serve_stdio()` entry point

**`src/config/mod.rs`** (~2000+ lines, 70+ tests):
- `LlmRoleConfig` at line 196: `provider`, `model`, `reasoning_effort`, `base_url`, `cli_path`
- `LlmConfig` at line 175: `dev`, `review`, `supervisor`, `epic_review`, `critic`
- `BotConfig` at line 74: `llm`, `bmad_paths`, etc.
- `BotSecrets` at line 777: API keys

**`src/session/mod.rs`** (~300 lines):
- `SessionOutcome` at line 102: `Completed`, `Escalated`, `Failed`
- `EscalationReport` at `session/escalation.rs:46`

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Zero-warning policy: `#![deny(clippy::all)]` at crate root
- All tests inline in `#[cfg(test)] mod tests { ... }` at bottom of each module
- Parser tests (9.1-9.11): plain `#[test]` — synchronous, no subprocess
- Config builder tests (9.12-9.18): plain `#[test]` — pure functions
- Integration tests (9.19): `#[tokio::test]` — spawns subprocess
- Dispatch tests (9.20-9.21): `#[tokio::test]` — calls async `run_session()`
- New tests in `src/runtime/sdk_claude.rs` (parser, config builder, prompt) and `src/runtime/sdk.rs` (dispatch, integration)
- Use `UiHandle::null()` for all tests

### Anti-Patterns to Avoid

- Do NOT use `--bare` flag — Claude Code must discover `.claude/skills/`, `CLAUDE.md`, and conventions natively
- Do NOT inline skill content or system preamble — SDK mode relies on native skill invocation
- Do NOT implement `--resume` logic — that's Story 15.7 (crash recovery / consultation injection)
- Do NOT wire `SdkRuntime` into `StoryPipeline::new()` — that's Story 15.7 (pipeline dual-runtime orchestration)
- Do NOT add WAL fields (`runtime_type`, `sdk_session_ids`) — that's Story 15.7
- Do NOT modify `ApiRuntime`, `SessionRunner`, or any API-mode code — completely separate path
- Do NOT modify `src/mcp_server/` — `generate_mcp_supervisor_config()` is used as-is
- Do NOT modify `src/config/mod.rs` — config already supports SDK providers from 15.2
- Do NOT modify `src/pipeline.rs` — pipeline routing is Story 15.7
- Do NOT pass MCP config JSON as a CLI argument — write to temp file to avoid secrets in `ps aux`
- Do NOT return `decisions: vec![]` for SDK sessions — the pipeline's `format_pr_decisions_section()` reads from `SessionOutcome.decisions`, not from disk. Empty decisions = empty PR section.
- Do NOT use raw string literals for phase matching — use `PHASE_CREATE`, `PHASE_REVIEW` constants from `session::state`
- Do NOT slice Rust strings at byte offsets for truncation — use `chars().take(n)` to avoid UTF-8 panics
- Do NOT parse `--include-partial-messages` stream events — too noisy, not needed
- Do NOT add `--verbose` flag — generates too much output for the daemon's needs
- Do NOT attempt to share `DecisionLog` between the daemon and MCP subprocess — they are separate processes, separate memory spaces
- Do NOT modify anything under `_bmad/` — daemon is read-only consumer

### Deferred Items

From this story scope — handled by later stories:
- **Pipeline wiring of `SdkRuntime`** — Story 15.7 will construct `SdkRuntime` in `StoryPipeline::new()` and route phases to the appropriate runtime
- **Session resume via `--resume`** — Story 15.7 will implement crash recovery and consultation injection using the persisted session ID
- **WAL `sdk_session_ids` persistence** — Story 15.7 will add `HashMap<String, String>` to WAL for session ID tracking per phase
- **Codex provider** — Story 15.6 will add `SdkCodexProvider` in `sdk_codex.rs` with Codex-specific CLI flags and NDJSON parsing
- **Init command SDK setup** — Story 15.8 will guide users through SDK provider selection in `bmad-bot init`

### Project Structure Notes

New files to create:
- `src/runtime/sdk_claude.rs` — `parse_claude_code_line()`, `build_claude_code_config()`, `build_claude_code_prompt()`, parser types, tests

Files to modify:
- `src/runtime/sdk.rs` — Replace `run_session()` stub with dispatch logic, add `config_for_role()`, expose fields via `pub(crate)`, add `config_path` field, add `completion_text` to `SdkSessionResult`
- `src/runtime/mod.rs` — Add `pub mod sdk_claude;`
- `src/supervisor/decisions.rs` — Add `write_decisions_json_sidecar()` function (new, 1 function)
- `src/cli/mod.rs` — Add `write_decisions_json_sidecar()` call in `run_mcp_supervisor()` after `write_decisions_file()` (1 new line)
- `Cargo.toml` — Add `tempfile = "3"` to dependencies (if not already present)

Files NOT to modify:
- `src/mcp_server/mod.rs` — `generate_mcp_supervisor_config()` used as-is
- `src/config/mod.rs` — config unchanged
- `src/pipeline.rs` — pipeline routing is Story 15.7
- `src/session/*` — session code untouched
- `src/tools/*` — tool implementations untouched (API-mode only)
- `src/main.rs` — no new modules declared (sdk_claude is a submodule of runtime)
- `_bmad/` — read-only, never modified

### References

- [Source: architecture.md#Decision 12 — Dual Runtime Abstraction, SdkClaudeCodeProvider]
- [Source: architecture.md#Decision 13 — Supervisor MCP Server, --mcp-config injection]
- [Source: architecture.md#Decision 5 — Amendment: SDK mode uses native skill invocation]
- [Source: planning-artifacts/sprint-change-proposal-2026-04-26.md — Story 15.5 definition]
- [Source: planning-artifacts/epics.md#Epic 15, Story 15.5 — Claude Code Provider Integration]
- [Source: src/runtime/sdk.rs:159-169 — run_session() stub to replace]
- [Source: src/runtime/sdk.rs:94-100 — SdkRuntime struct (needs config_path field)]
- [Source: src/runtime/sdk.rs:172-309 — execute_session() subprocess management]
- [Source: src/runtime/sdk.rs:120-132 — merge_env_vars() API key injection]
- [Source: src/runtime/sdk.rs:134-156 — resolve_provider_for_role()]
- [Source: src/runtime/sdk.rs:52-87 — SdkSessionConfig, SdkOutputEvent, SdkSessionResult types]
- [Source: src/runtime/mod.rs:61-67 — SessionContext struct]
- [Source: src/runtime/mod.rs:74-93 — SessionRuntime enum dispatch]
- [Source: src/mcp_server/mod.rs:240-274 — generate_mcp_supervisor_config()]
- [Source: src/config/mod.rs:196-223 — LlmRoleConfig (provider, model, cli_path)]
- [Source: src/config/mod.rs:175-193 — LlmConfig (dev, review, supervisor, epic_review, critic)]
- [Source: src/config/mod.rs:696-702 — resolve_cli_name() helper]
- [Source: src/config/mod.rs:74-137 — BotConfig struct]
- [Source: src/config/mod.rs:277-286 — BmadPathsConfig (project_root, implementation_artifacts)]
- [Source: src/config/mod.rs:777-789 — BotSecrets struct]
- [Source: src/session/mod.rs:102-135 — SessionOutcome enum]
- [Source: src/session/escalation.rs:46-88 — EscalationReport struct]
- [Source: src/session/state.rs:18-28 — Phase constants (PHASE_CREATE, PHASE_DEV, PHASE_REVIEW)]
- [Source: src/watcher/mod.rs:68-88 — StoryInfo struct (story_key, branch_name, specs_path)]
- [Source: src/supervisor/decisions.rs:262-274 — write_decisions_file()]
- [Source: src/supervisor/decisions.rs:339 — format_pr_decisions_section() — reads from in-memory Vec, NOT disk]
- [Source: src/cli/mod.rs:1312-1314 — run_mcp_supervisor() decisions file writing (add JSON sidecar call here)]
- [Source: src/pipeline.rs:499,613,781,916,1049 — format_pr_decisions_section(&decisions) call sites — uses SessionOutcome.decisions]
- [Source: _bmad-output/project-context.md — Project rules and conventions]
- [Source: _bmad-output/implementation-artifacts/15-4-supervisor-mcp-server.md — Previous story context]
- [Source: _bmad-output/implementation-artifacts/15-3-sdk-runtime-subprocess-infrastructure.md — Previous story context]
- [Source: _bmad-output/implementation-artifacts/15-2-config-extension-sdk-providers.md — Previous story context]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

None — clean implementation with no blocking issues.

### Completion Notes List

- Task 1: Created `src/runtime/sdk_claude.rs` with `ClaudeCodeEvent`, `ClaudeCodeMessage`, `ClaudeCodeContentBlock` private deserialization types and `parse_claude_code_line()` public parser function. Parser handles system/init, system/api_retry, assistant (tool_use + text), user (tool results), result (success + error), and silently skips unknown event types and non-JSON lines.
- Task 2: Implemented `build_claude_code_config()` that constructs `SdkSessionConfig` with all required Claude Code CLI flags (-p, --output-format stream-json, --model, --permission-mode acceptEdits, --allowedTools, --max-turns 200, --cd, --append-system-prompt for English override). Supports `cli_path` override and optional `--mcp-config` for supervisor.
- Task 3: Implemented `build_claude_code_prompt()` mapping phases to native slash commands: create→`/bmad-create-story`, review→`/bmad-code-review`, dev (default)→`/bmad-dev-story`. Uses `PHASE_CREATE`/`PHASE_REVIEW` constants from session::state.
- Task 4: Replaced `run_session()` stub with provider dispatch — `"claude-code"` dispatches to `run_claude_code_session()`, unknown providers return `Failed`. Implemented full orchestration: project root canonicalization, MCP config via temp file (security: avoids secrets in ps aux), session execution with parser, result mapping with escalation detection. Added `pub(crate)` accessors for `config()`, `secrets()`, `config_path()`, `config_for_role()`.
- Task 5: Added `config_path: PathBuf` field to `SdkRuntime` and updated `new()` signature. Updated all 3 call sites (tests in sdk.rs, mod.rs).
- Task 6: Added `completion_text: Option<String>` to `SdkSessionResult`. Updated `execute_session()` event loop to track last Completion result text. Updated all result construction sites.
- Task 7: Added `write_decisions_json_sidecar()` to `supervisor/decisions.rs` — writes structured JSON alongside the existing markdown. Added call site in `cli/mod.rs` `run_mcp_supervisor()` after `write_decisions_file()`. Implemented `read_decisions_json_sidecar()` and `detect_escalation()` in `sdk_claude.rs` for typed decision reading and escalation detection.
- Task 8: Added `pub mod sdk_claude;` to `src/runtime/mod.rs`. Removed `#[allow(dead_code)]` from `secrets` field.
- Task 9: 29 new tests covering parser (11 tests), config builder (6 tests), prompt builder (3 tests), escalation detection (3 tests), JSON sidecar read/write (4 tests), dispatch (2 tests). All pass.
- Task 10: Zero new clippy warnings from modified files. 1402 unit tests pass. Formatting clean.

### Review Findings

- [x] [Review][Patch] #1 — Sidecar path: pass `impl_artifacts_path` explicitly from config instead of deriving from `specs_path.parent()` — FIXED
- [x] [Review][Patch] #2 — Clippy: collapsible `if let` + `!text.is_empty()` → use let-chain — FIXED
- [x] [Review][Patch] #3 — Clippy: `std::io::Error::new(ErrorKind::Other, e)` → `std::io::Error::other(e)` — FIXED
- [x] [Review][Patch] #4 — Corrupt JSON sidecar: added `tracing::warn!` on parse error — FIXED
- [x] [Review][Patch] #5 — `resolve_provider_for_role` now delegates to `config_for_role` — FIXED
- [x] [Review][Patch] #6 — `needs_supervisor` now uses `PHASE_CREATE`/`PHASE_DEV` constants — FIXED
- [x] [Review][Patch] #7 — Integration test gated with `#[cfg(unix)]` — FIXED
- [x] [Review][Patch] #8 — Empty completion text filtered to `None` via `.filter(|t| !t.is_empty())` — FIXED
- [x] [Review][Patch] #9 — Empty phase `""` now triggers warning (removed `!phase.is_empty()` guard) — FIXED
- [x] [Review][Defer] #10 — `timed_out`/`shutdown_requested` ignored in outcome mapping [sdk_claude.rs:290] — deferred, spec-explicit (Task 4.3 checks exit_code only), Story 15.7 scope
- [x] [Review][Defer] #11 — Hardcoded CLI params (`--allowedTools`, `--max-turns 200`, `--permission-mode acceptEdits`) for all roles — deferred, intentional per architecture decision
- [x] [Review][Defer] #12 — `truncate_str` in decisions.rs uses byte length comparison vs char count — deferred, pre-existing
- [x] [Review][Defer] #13 — `config_for_role` uses empty string `provider` as sentinel for "unconfigured" — deferred, pre-existing config design (Story 15.2)
- [x] [Review][Defer] #14 — `specs_path` with bare filename: `.parent()` returns `Some("")` not `None`, guard ineffective — deferred, defensive concern

### Change Log

- 2026-04-27: Story 15.5 implementation complete — Claude Code provider integration with 29 new tests

### File List

New files:
- src/runtime/sdk_claude.rs

Modified files:
- src/runtime/sdk.rs (config_path field, completion_text, run_session dispatch, pub(crate) accessors)
- src/runtime/mod.rs (pub mod sdk_claude, test callsite update)
- src/supervisor/decisions.rs (write_decisions_json_sidecar function + 2 tests)
- src/cli/mod.rs (JSON sidecar call in run_mcp_supervisor)
- src/mcp/mod.rs (pre-existing unused import fix)
- Cargo.toml (tempfile moved from dev-dependencies to dependencies)
