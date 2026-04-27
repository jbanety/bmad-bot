# Story 15.6: Codex Provider Integration

Status: done

## Story

As a daemon operator,
I want to use `provider: codex` for any LLM role so the daemon delegates sessions to the Codex CLI,
so that I can use OpenAI models with autonomous agent capabilities.

## Acceptance Criteria

1. **Given** a role is configured with `provider: codex` **When** the daemon runs a session for that role **Then** `SdkRuntime::run_session()` dispatches to `run_codex_session()` in `sdk_codex.rs` **And** constructs a `SdkSessionConfig` with Codex CLI invocation **And** calls `execute_session()` with a Codex-specific NDJSON line parser **And** maps `SdkSessionResult` to `SessionOutcome`

2. **Given** the daemon composes the CLI command **When** the session starts **Then** it invokes: `codex exec --json --sandbox workspace-write --ask-for-approval never --model {configured_model} --cd {project_root} -- "{prompt}"` **And** Codex is launched so it discovers project skills (`.agents/skills/`), `AGENTS.md`, and conventions natively **And** `--ask-for-approval never` ensures fully unattended headless operation (no pauses for human input)

3. **Given** the supervisor MCP server is available **When** the session is prepared **Then** MCP config is merged into the project-scoped `.codex/config.toml` file in the project root (preserving existing user settings) **And** the TOML file defines `[mcp_servers.bmad-supervisor]` with `command`, `args`, and `env` fields **And** the original `.codex/config.toml` is backed up before writing and restored after session completes **And** API keys in the `env` section are never passed as CLI arguments

4. **Given** skills are invoked via native slash commands **When** the daemon composes the prompt **Then** it uses `/bmad-dev-story`, `/bmad-create-story`, `/bmad-code-review` with story-specific context (story file path, branch name) **And** the prompt is prefixed with `SYSTEM OVERRIDE: communication_language = English\n\n` to enforce English output (Codex has no `--append-system-prompt` flag) **And** NO system preamble, NO inlined skill content, NO tool usage rules

5. **Given** the session produces NDJSON output **When** each line is parsed **Then** the Codex parser extracts `SdkOutputEvent` variants from Codex event types: `thread.started` → `SessionStarted` (from `thread_id`), `item.started` with tool types → `ToolCall`, `item.completed` with `type: agent_message` → `Completion` (final response text captured by `execute_session` as `completion_text`), `item.completed` with tool types → `ToolResult`, `turn.completed` → ignored (carries only usage stats), `turn.failed` → `Error`, `error` → `Error`

6. **Given** the session completes successfully **When** the process exits with code 0 **Then** `SessionOutcome::Completed` is returned with `branch` from `StoryInfo.branch_name`, `decisions` parsed from the MCP supervisor decisions JSON sidecar file into `Vec<DecisionRecord>`, `pr_context` from the last agent message text (first 2000 chars) **And** if no decisions file exists (no supervisor calls), `decisions` is empty vec

7. **Given** the session fails **When** the process exits with non-zero code **Or** a `turn.failed` event is received **Then** `SessionOutcome::Failed` is returned with the error details from stderr or `turn.failed` error message

8. **Given** escalation was triggered during the session **When** the decisions JSON sidecar contains a `DecisionSource::Escalation` record **Then** `SessionOutcome::Escalated` is returned with an `EscalationReport` built from the escalation decision record

9. **Given** the CLI path can be overridden via config **When** `cli_path` is set on the role's `LlmRoleConfig` **Then** the command uses that path instead of the default `"codex"` **And** `resolve_cli_name("codex")` provides the default `"codex"`

10. **Given** all existing tests pass **When** the Codex provider is added **Then** zero behavioral changes for existing configurations — all 1402+ existing unit tests pass identically

## Tasks / Subtasks

- [x] Task 1: Create `src/runtime/sdk_codex.rs` with Codex provider types (AC: #1, #2, #5)
  - [x] 1.1 Create `src/runtime/sdk_codex.rs` with module doc comment
  - [x] 1.2 Define `CodexEvent` — internal deserialization struct for Codex NDJSON:
    ```rust
    #[derive(Debug, Deserialize)]
    struct CodexEvent {
        #[serde(rename = "type")]
        event_type: String,
        #[serde(default)]
        thread_id: Option<String>,
        #[serde(default)]
        usage: Option<CodexUsage>,
        #[serde(default)]
        error: Option<CodexError>,
        #[serde(default)]
        item: Option<CodexItem>,
        #[serde(default)]
        message: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    struct CodexUsage {
        #[allow(dead_code)]
        #[serde(default)]
        input_tokens: u64,
        #[allow(dead_code)]
        #[serde(default)]
        cached_input_tokens: u64,
        #[allow(dead_code)]
        #[serde(default)]
        output_tokens: u64,
    }

    #[derive(Debug, Deserialize)]
    struct CodexError {
        #[serde(default)]
        message: String,
    }

    #[derive(Debug, Deserialize)]
    struct CodexItem {
        #[allow(dead_code)]
        #[serde(default)]
        id: String,
        #[serde(rename = "type")]
        item_type: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        tool: Option<String>,
        #[serde(default)]
        server: Option<String>,
        #[serde(default)]
        query: Option<String>,
    }
    ```
    Notes:
    - These are private types — only the parser function is public.
    - `CodexEvent` has a flat top-level structure with optional fields. The `type` field discriminates events.
    - `CodexItem` is a union type — different `item_type` values populate different optional fields. Item types: `agent_message`, `reasoning`, `command_execution`, `file_change`, `mcp_tool_call`, `web_search`, `error`.
    - `CodexUsage` captures token counts from `turn.completed` events (informational — not used by daemon, but deserialized for potential future use).
  - [x] 1.3 Implement `pub fn parse_codex_line(line: &str) -> Option<SdkOutputEvent>` — the parser function passed to `execute_session()`:
    ```rust
    pub fn parse_codex_line(line: &str) -> Option<SdkOutputEvent> {
        let event: CodexEvent = serde_json::from_str(line).ok()?;
        match event.event_type.as_str() {
            "thread.started" => {
                let thread_id = event.thread_id?;
                Some(SdkOutputEvent::SessionStarted { session_id: thread_id })
            }
            "turn.started" => Some(SdkOutputEvent::Progress {
                message: "Turn started".to_string(),
            }),
            "turn.completed" => {
                // turn.completed carries usage stats — we emit a Completion event
                // The actual completion text comes from the last item.completed agent_message
                // tracked in the execute_session loop (completion_text field)
                None
            }
            "turn.failed" => {
                let error_msg = event.error
                    .map(|e| e.message)
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| "Turn failed".to_string());
                Some(SdkOutputEvent::Error { message: error_msg })
            }
            "item.started" => {
                let item = event.item?;
                match item.item_type.as_str() {
                    "command_execution" => Some(SdkOutputEvent::ToolCall {
                        tool_name: "command_execution".to_string(),
                        detail: item.command.unwrap_or_default(),
                    }),
                    "file_change" => Some(SdkOutputEvent::ToolCall {
                        tool_name: "file_change".to_string(),
                        detail: String::new(),
                    }),
                    "mcp_tool_call" => Some(SdkOutputEvent::ToolCall {
                        tool_name: format!(
                            "mcp:{}:{}",
                            item.server.as_deref().unwrap_or("unknown"),
                            item.tool.as_deref().unwrap_or("unknown"),
                        ),
                        detail: String::new(),
                    }),
                    "web_search" => Some(SdkOutputEvent::ToolCall {
                        tool_name: "web_search".to_string(),
                        detail: item.query.unwrap_or_default(),
                    }),
                    "reasoning" => Some(SdkOutputEvent::Progress {
                        message: "Reasoning...".to_string(),
                    }),
                    _ => None,
                }
            }
            "item.completed" => {
                let item = event.item?;
                match item.item_type.as_str() {
                    "agent_message" => {
                        let text = item.text.unwrap_or_default();
                        if text.is_empty() {
                            return None;
                        }
                        // Agent messages are the final results — emit as Completion
                        Some(SdkOutputEvent::Completion {
                            result: text,
                            is_error: false,
                        })
                    }
                    "command_execution" | "file_change" | "mcp_tool_call" | "web_search" => {
                        Some(SdkOutputEvent::ToolResult {
                            tool_name: item.item_type.clone(),
                            detail: String::new(),
                        })
                    }
                    "error" => {
                        let msg = item.text
                            .filter(|t| !t.is_empty())
                            .unwrap_or_else(|| "Unknown item error".to_string());
                        Some(SdkOutputEvent::Error { message: msg })
                    }
                    _ => None,
                }
            }
            "item.updated" => None, // Intermediate state — skip
            "error" => {
                let msg = event.message
                    .filter(|m| !m.is_empty())
                    .or_else(|| event.error.map(|e| e.message))
                    .unwrap_or_else(|| "Unknown error".to_string());
                Some(SdkOutputEvent::Error { message: msg })
            }
            _ => None,
        }
    }
    ```
    Notes:
    - Returns `None` for unrecognized or irrelevant events (`turn.completed`, `item.updated`, delta events).
    - `serde_json::from_str().ok()?` silently skips non-JSON lines (CLI startup banners, warnings).
    - `item.completed` with `type: agent_message` emits `Completion` — this is the agent's final response text. The `execute_session()` event loop in `sdk.rs` tracks the last `Completion` event's text in `completion_text`.
    - `item.started` maps work-item types to `ToolCall` events for UI visibility.
    - `item.completed` for tool-like items (`command_execution`, `file_change`, etc.) emits `ToolResult`.
    - `turn.completed` is intentionally skipped — it carries only usage stats. The actual result text was already captured from `item.completed` agent_message. Emitting a `Completion` here would overwrite the meaningful text with empty content.

- [x] Task 2: Implement `build_codex_config()` — session config builder (AC: #2, #3, #4, #9)
  - [x] 2.1 Implement `pub fn build_codex_config(role_config: &LlmRoleConfig, project_root: &Path, prompt: &str) -> SdkSessionConfig`:
    ```rust
    /// Codex reasoning_effort values. Daemon config uses the same names except Codex also
    /// supports "minimal". Pass through validated values; warn on unrecognized ones.
    const VALID_CODEX_REASONING: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];

    pub fn build_codex_config(
        role_config: &LlmRoleConfig,
        project_root: &Path,
        prompt: &str,
    ) -> SdkSessionConfig {
        let command = role_config.cli_path.clone()
            .unwrap_or_else(|| "codex".to_string());

        let mut args = vec![
            "exec".to_string(),
            "--json".to_string(),
            "--sandbox".to_string(),
            "workspace-write".to_string(),
            "--ask-for-approval".to_string(),
            "never".to_string(),
            "--model".to_string(),
            role_config.model.clone(),
            "--cd".to_string(),
            project_root.to_string_lossy().to_string(),
        ];

        if let Some(ref effort) = role_config.reasoning_effort {
            if VALID_CODEX_REASONING.contains(&effort.as_str()) {
                args.push("--config".to_string());
                args.push(format!("model_reasoning_effort={effort}"));
            } else {
                tracing::warn!(
                    effort = %effort,
                    "Unrecognized reasoning_effort for Codex, ignoring. Valid: minimal, low, medium, high, xhigh"
                );
            }
        }

        // "--" separates flags from the positional prompt argument,
        // preventing misparse if prompt starts with "-"
        args.push("--".to_string());
        args.push(prompt.to_string());

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
    - `codex exec` is the non-interactive subcommand. The prompt is a positional argument after `--`.
    - `--sandbox workspace-write` + `--ask-for-approval never` instead of `--full-auto`. The `--full-auto` preset uses `on-request` approvals which can pause for human input in headless mode (e.g., operations outside the workspace, network access). Explicit `never` ensures fully unattended operation. The daemon has its own timeout to catch runaway sessions.
    - `--` separator before the prompt prevents `codex exec` from misinterpreting prompt content as flags.
    - `--json` outputs NDJSON event stream on stdout (equivalent to Claude Code's `--output-format stream-json`).
    - `--model` overrides the model (e.g., `o4-mini`, `gpt-5.5`, `codex-mini`).
    - `--cd` sets the working directory (same as Claude Code).
    - `reasoning_effort` is validated against Codex's accepted values before passing. Codex supports "minimal" which the daemon's `LlmRoleConfig` doesn't expose in validation — it's accepted here since it's a valid Codex value. Unrecognized values are logged and skipped rather than causing a cryptic Codex error.
    - No `--mcp-config` CLI flag — Codex reads MCP config from `.codex/config.toml` in the project root (see Task 4).
    - No `--append-system-prompt` flag — language override is prepended to the prompt in `build_codex_prompt()`.
    - No `--bare` equivalent — Codex discovers `.agents/skills/`, `AGENTS.md` natively.
    - No `--max-turns` equivalent — Codex manages turn limits internally.
    - `env` is empty — `SdkRuntime::merge_env_vars()` injects `OPENAI_API_KEY` at subprocess spawn time.
    - `cli_path` override respected per config (Story 15.2 added this field).

- [x] Task 3: Implement prompt construction per phase (AC: #4)
  - [x] 3.1 Implement `pub fn build_codex_prompt(phase: &str, story: &StoryInfo) -> String`:
    ```rust
    use crate::session::state::{PHASE_CREATE, PHASE_REVIEW};

    pub fn build_codex_prompt(phase: &str, story: &StoryInfo) -> String {
        let skill_cmd = match phase {
            PHASE_CREATE => format!("/bmad-create-story {}", story.story_key),
            PHASE_REVIEW => format!(
                "/bmad-code-review {}",
                story.specs_path.to_string_lossy(),
            ),
            _ => {
                if phase != crate::session::state::PHASE_DEV {
                    tracing::warn!(phase = %phase, "Unknown phase for Codex prompt, defaulting to dev-story");
                }
                format!(
                    "/bmad-dev-story {}",
                    story.specs_path.to_string_lossy(),
                )
            }
        };
        format!(
            "SYSTEM OVERRIDE: communication_language = English\n\n{skill_cmd}"
        )
    }
    ```
    Notes:
    - Uses `PHASE_CREATE` and `PHASE_REVIEW` constants from `session::state` — not raw strings.
    - `tracing::warn!` on unknown phases (same pattern as Claude Code provider).
    - Language override prepended directly in the prompt because Codex has no `--append-system-prompt` flag. The `SYSTEM OVERRIDE:` prefix is recognized by BMAD skills.
    - Same slash command format as Claude Code — Codex discovers skills from `.agents/skills/` and invokes them the same way.
    - Consultations (adversarial, critic) are handled by the pipeline as separate sessions — not part of the prompt.

- [x] Task 4: Implement MCP config via project-scoped `.codex/config.toml` (AC: #3)
  - [x] 4.1 Define a `CodexMcpBackup` struct to hold backup state unambiguously:
    ```rust
    struct CodexMcpBackup {
        project_root: PathBuf,
        original_content: Option<String>, // Some = file existed, None = file didn't exist
    }
    ```
    Notes:
    - This struct is created on successful write and consumed on restore.
    - Avoids the `Option<Option<String>>` confusion — the struct's existence means "write succeeded, cleanup needed."
    - `Drop` is NOT implemented: cleanup is explicit via `restore()` to allow error handling and tracing.
  - [x] 4.2 Implement `fn write_codex_mcp_config(project_root: &Path, mcp_json: &serde_json::Value) -> Result<CodexMcpBackup, std::io::Error>`:
    ```rust
    fn write_codex_mcp_config(
        project_root: &Path,
        mcp_json: &serde_json::Value,
    ) -> Result<CodexMcpBackup, std::io::Error> {
        let codex_dir = project_root.join(".codex");
        let config_path = codex_dir.join("config.toml");

        // Backup existing config if present
        let original_content = if config_path.exists() {
            Some(std::fs::read_to_string(&config_path)?)
        } else {
            None
        };

        // Ensure .codex/ directory exists
        std::fs::create_dir_all(&codex_dir)?;

        // Merge MCP server into existing config (preserve user settings)
        let merged = merge_mcp_into_config(original_content.as_deref(), mcp_json);
        std::fs::write(&config_path, &merged)?;

        Ok(CodexMcpBackup {
            project_root: project_root.to_path_buf(),
            original_content,
        })
    }
    ```
    Notes:
    - Codex reads MCP server config from `.codex/config.toml` in the project root (project-scoped config).
    - If `.codex/config.toml` already exists, its content is preserved — the MCP server section is merged into it (not replaced).
    - Returns `CodexMcpBackup` only on success — the caller always knows cleanup is needed.
    - If the write fails midway, the backup struct is not created, so no cleanup is attempted on a partial state.
  - [x] 4.3 Implement `fn merge_mcp_into_config(existing: Option<&str>, mcp_json: &serde_json::Value) -> String`:
    ```rust
    fn merge_mcp_into_config(existing: Option<&str>, mcp_json: &serde_json::Value) -> String {
        let mut toml = String::new();

        // Preserve existing config content (user's model, sandbox, features, other MCP servers)
        if let Some(content) = existing {
            // Remove any existing [mcp_servers.bmad-supervisor] section to avoid duplicates
            let cleaned = remove_toml_section(content, "mcp_servers.bmad-supervisor");
            toml.push_str(cleaned.trim_end());
            toml.push_str("\n\n");
        }

        // Append our MCP server section
        toml.push_str(&generate_mcp_toml_section(mcp_json));
        toml
    }

    fn remove_toml_section(content: &str, section_prefix: &str) -> String {
        // Remove lines belonging to [section_prefix] and [section_prefix.*] subsections
        let mut result = String::new();
        let mut skip = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                // Check if this header matches our section or a subsection of it
                let header = trimmed.trim_start_matches('[').trim_end_matches(']').trim();
                skip = header == section_prefix || header.starts_with(&format!("{section_prefix}."));
            }
            if !skip {
                result.push_str(line);
                result.push('\n');
            }
        }
        result
    }
    ```
    Notes:
    - Preserves the user's existing config (model, sandbox_mode, features, other MCP servers, profiles, etc.).
    - Only the `[mcp_servers.bmad-supervisor]` section is managed — other sections pass through untouched.
    - If a prior `[mcp_servers.bmad-supervisor]` section exists (e.g., from a crashed session), it is removed before appending the new one.
  - [x] 4.4 Implement `fn generate_mcp_toml_section(mcp_json: &serde_json::Value) -> String`:
    ```rust
    fn generate_mcp_toml_section(mcp_json: &serde_json::Value) -> String {
        let mut toml = String::new();
        if let Some(servers) = mcp_json.get("mcpServers").and_then(|s| s.as_object()) {
            for (name, config) in servers {
                toml.push_str(&format!("[mcp_servers.{name}]\n"));
                if let Some(cmd) = config.get("command").and_then(|c| c.as_str()) {
                    toml.push_str(&format!("command = {}\n", escape_toml_string(cmd)));
                }
                if let Some(args) = config.get("args").and_then(|a| a.as_array()) {
                    let args_str: Vec<String> = args.iter()
                        .filter_map(|a| a.as_str())
                        .map(|a| escape_toml_string(a))
                        .collect();
                    toml.push_str(&format!("args = [{}]\n", args_str.join(", ")));
                }
                toml.push_str("required = true\n");
                if let Some(env) = config.get("env").and_then(|e| e.as_object()) {
                    toml.push_str(&format!("\n[mcp_servers.{name}.env]\n"));
                    for (key, val) in env {
                        if let Some(v) = val.as_str() {
                            toml.push_str(&format!("{key} = {}\n", escape_toml_string(v)));
                        }
                    }
                }
            }
        }
        toml
    }

    fn escape_toml_string(s: &str) -> String {
        // TOML basic strings: escape backslashes and double quotes
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    }
    ```
    Notes:
    - Converts the JSON output of `generate_mcp_supervisor_config()` to Codex's TOML format.
    - `required = true` ensures Codex fails startup if the MCP server doesn't initialize (fail-fast).
    - `escape_toml_string()` handles backslashes (Windows paths) and double quotes in values — prevents malformed TOML.
    - TOML generation is manual string building — simple and predictable, no need for a `toml` crate dependency.
  - [x] 4.5 Implement `fn restore_codex_mcp_config(backup: CodexMcpBackup)`:
    ```rust
    fn restore_codex_mcp_config(backup: CodexMcpBackup) {
        let config_path = backup.project_root.join(".codex/config.toml");
        match backup.original_content {
            Some(original) => {
                if let Err(e) = std::fs::write(&config_path, original) {
                    tracing::warn!(error = %e, "Failed to restore .codex/config.toml");
                }
            }
            None => {
                // No original file — clean up our generated config
                let _ = std::fs::remove_file(&config_path);
                // Remove .codex/ dir if empty (best-effort)
                let _ = std::fs::remove_dir(backup.project_root.join(".codex"));
            }
        }
    }
    ```
    Notes:
    - Takes ownership of `CodexMcpBackup` — consumes it, preventing double-restore.
    - Restores original `.codex/config.toml` if it existed, or removes the generated file.
    - Best-effort cleanup — errors are logged but don't fail the session.
    - The `.codex/` directory is removed only if empty (won't delete user's other files).
  - [x] 4.6 **Project trust documentation:** Add a tracing warning in `run_codex_session()` when MCP config is written:
    ```rust
    tracing::info!(
        path = %config_path.display(),
        "Wrote Codex MCP config — ensure project is trusted (`codex trust .`) for project-scoped config"
    );
    ```
    Note: Codex only reads `.codex/config.toml` for "trusted projects." If the project is not trusted, the MCP supervisor won't be available and `ask_supervisor` calls will fail. This is validated at startup by `validate_sdk_providers()` (Story 15.2) which checks skill files — but MCP discovery is separate. The init command (Story 15.8) should guide users through `codex trust .` as part of setup. For now, emit a log message.

- [x] Task 5: Implement `run_codex_session()` — full session orchestration (AC: #1, #6, #7, #8)
  - [x] 5.1 Implement `pub async fn run_codex_session(runtime: &SdkRuntime, context: super::SessionContext<'_>) -> SessionOutcome`:
    ```rust
    pub async fn run_codex_session(
        runtime: &SdkRuntime,
        context: super::SessionContext<'_>,
    ) -> SessionOutcome {
        let role_config = runtime.config_for_role(&context.role);
        let prompt = build_codex_prompt(context.initial_phase, context.story);

        let project_root = match std::fs::canonicalize(&runtime.config().bmad_paths.project_root) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    raw = %runtime.config().bmad_paths.project_root,
                    "Failed to canonicalize project_root, using raw value"
                );
                PathBuf::from(&runtime.config().bmad_paths.project_root)
            }
        };

        // MCP config: write .codex/config.toml for supervisor phases
        let needs_supervisor = matches!(context.initial_phase, PHASE_CREATE | PHASE_DEV);
        let mcp_backup: Option<CodexMcpBackup> = if needs_supervisor {
            let mcp_json = crate::mcp_server::generate_mcp_supervisor_config(
                &context.story.story_key,
                runtime.config_path(),
                runtime.secrets(),
            );
            match write_codex_mcp_config(&project_root, &mcp_json) {
                Ok(backup) => {
                    tracing::info!(
                        "Wrote Codex MCP config — ensure project is trusted (`codex trust .`) for project-scoped config"
                    );
                    Some(backup)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to write Codex MCP config, proceeding without supervisor");
                    None
                }
            }
        } else {
            None
        };

        let session_config = build_codex_config(role_config, &project_root, &prompt);

        let result = match runtime
            .execute_session(session_config, parse_codex_line)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                // Clean up MCP config before returning
                if let Some(backup) = mcp_backup {
                    restore_codex_mcp_config(backup);
                }
                return SessionOutcome::Failed {
                    story_key: context.story.story_key.clone(),
                    error: e.to_string(),
                    decisions: vec![],
                };
            }
        };

        // Restore .codex/config.toml after session completes
        if let Some(backup) = mcp_backup {
            restore_codex_mcp_config(backup);
        }

        let impl_artifacts_path =
            PathBuf::from(&runtime.config().bmad_paths.implementation_artifacts);
        super::sdk_claude::map_sdk_result_to_outcome(&result, context.story, &impl_artifacts_path).await
    }
    ```
    Notes:
    - Follows the exact same orchestration pattern as `run_claude_code_session()`.
    - MCP config is written to `.codex/config.toml` (not a temp file) because Codex reads from project-scoped config.
    - Cleanup is critical — `.codex/config.toml` is always restored regardless of success/failure.
    - `mcp_backup` is `Option<CodexMcpBackup>` — `Some` means write succeeded and cleanup is needed. No double-Option confusion.
    - `CodexMcpBackup` is consumed by `restore_codex_mcp_config()` (takes ownership), preventing double-restore.
    - Result mapping reuses `map_sdk_result_to_outcome` from `sdk_claude.rs` (see Task 6).
    - Project trust info logged on MCP config write — helps debug "supervisor not available" issues.

- [x] Task 6: Extract shared result mapping to avoid duplication (AC: #6, #7, #8)
  - [x] 6.1 The `map_sdk_result_to_outcome()`, `read_decisions_json_sidecar()`, and `detect_escalation()` functions in `sdk_claude.rs` are provider-agnostic — they operate on `SdkSessionResult`, `StoryInfo`, and `DecisionRecord` types that both providers share.
    **Approach:** Change `map_sdk_result_to_outcome` to `pub(crate)` in `sdk_claude.rs` (keep the same name). Codex calls `super::sdk_claude::map_sdk_result_to_outcome(...)`.
  - [x] 6.2 In `sdk_claude.rs`, change visibility:
    - `map_sdk_result_to_outcome` → `pub(crate) async fn map_sdk_result_to_outcome` (same name, just pub(crate))
    - `read_decisions_json_sidecar` is already `pub` — no change needed
    - `detect_escalation` is already `pub` — no change needed
    - Update `run_claude_code_session()` call site — already calls `map_sdk_result_to_outcome(...)` by unqualified name, no change needed since it's in the same module.
  - [x] 6.3 In `sdk_codex.rs`, call `super::sdk_claude::map_sdk_result_to_outcome(...)` for result mapping.
    Note: No rename. The function name stays `map_sdk_result_to_outcome`. It's pub(crate) so both modules can use it. The dependency direction (codex → claude helpers) is acceptable since these functions have no Claude-specific logic.

- [x] Task 7: Update `SdkRuntime::run_session()` dispatch (AC: #1)
  - [x] 7.1 In `src/runtime/sdk.rs`, update the `run_session()` match to add the Codex arm:
    ```rust
    pub async fn run_session(&self, context: SessionContext<'_>) -> SessionOutcome {
        let provider = self.resolve_provider_for_role(&context.role);
        match provider.as_str() {
            "claude-code" => super::sdk_claude::run_claude_code_session(self, context).await,
            "codex" => super::sdk_codex::run_codex_session(self, context).await,
            other => SessionOutcome::Failed {
                story_key: context.story.story_key.clone(),
                error: format!("SDK provider '{}' not implemented.", other),
                decisions: vec![],
            },
        }
    }
    ```
  - [x] 7.2 Update the error message to remove the "Requires Story 15.6" reference since codex is now implemented.

- [x] Task 8: Wire module into runtime (AC: #1)
  - [x] 8.1 Add `pub mod sdk_codex;` to `src/runtime/mod.rs` (after `pub mod sdk_claude;`)

- [x] Task 9: Write comprehensive tests (AC: #1-10)
  - [x] 9.1 `test_parse_codex_thread_started` — parse `{"type":"thread.started","thread_id":"abc-123"}`, verify `SdkOutputEvent::SessionStarted { session_id: "abc-123" }`
  - [x] 9.2 `test_parse_codex_thread_started_no_thread_id` — parse `{"type":"thread.started"}` (missing thread_id), verify returns `None`
  - [x] 9.3 `test_parse_codex_turn_started` — parse `{"type":"turn.started"}`, verify `SdkOutputEvent::Progress { message: "Turn started" }`
  - [x] 9.4 `test_parse_codex_turn_completed_ignored` — parse `{"type":"turn.completed","usage":{"input_tokens":100,"cached_input_tokens":50,"output_tokens":20}}`, verify returns `None`
  - [x] 9.5 `test_parse_codex_turn_failed` — parse `{"type":"turn.failed","error":{"message":"Rate limit exceeded"}}`, verify `SdkOutputEvent::Error`
  - [x] 9.6 `test_parse_codex_turn_failed_no_message` — parse `{"type":"turn.failed"}`, verify `SdkOutputEvent::Error { message: "Turn failed" }`
  - [x] 9.7 `test_parse_codex_item_started_command_execution` — parse `{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"cargo test","status":"in_progress"}}`, verify `SdkOutputEvent::ToolCall { tool_name: "command_execution", detail: "cargo test" }`
  - [x] 9.8 `test_parse_codex_item_started_file_change` — parse `{"type":"item.started","item":{"id":"item_2","type":"file_change","status":"in_progress"}}`, verify `SdkOutputEvent::ToolCall { tool_name: "file_change" }`
  - [x] 9.9 `test_parse_codex_item_started_mcp_tool_call` — parse `{"type":"item.started","item":{"id":"item_3","type":"mcp_tool_call","server":"bmad-supervisor","tool":"ask_supervisor","status":"in_progress"}}`, verify `SdkOutputEvent::ToolCall { tool_name: "mcp:bmad-supervisor:ask_supervisor" }`
  - [x] 9.10 `test_parse_codex_item_started_web_search` — parse `{"type":"item.started","item":{"id":"item_4","type":"web_search","query":"rust tokio tutorial","status":"in_progress"}}`, verify `SdkOutputEvent::ToolCall { tool_name: "web_search", detail: "rust tokio tutorial" }`
  - [x] 9.11 `test_parse_codex_item_started_reasoning` — parse `{"type":"item.started","item":{"id":"item_5","type":"reasoning","status":"in_progress"}}`, verify `SdkOutputEvent::Progress { message: "Reasoning..." }`
  - [x] 9.12 `test_parse_codex_item_completed_agent_message` — parse `{"type":"item.completed","item":{"id":"item_6","type":"agent_message","text":"All tasks completed successfully."}}`, verify `SdkOutputEvent::Completion { result: "All tasks completed successfully.", is_error: false }`
  - [x] 9.13 `test_parse_codex_item_completed_agent_message_empty` — parse with empty text, verify returns `None`
  - [x] 9.14 `test_parse_codex_item_completed_command_execution` — parse `{"type":"item.completed","item":{"id":"item_7","type":"command_execution","command":"cargo test","status":"completed"}}`, verify `SdkOutputEvent::ToolResult { tool_name: "command_execution" }`
  - [x] 9.15 `test_parse_codex_item_completed_error` — parse `{"type":"item.completed","item":{"id":"item_8","type":"error","text":"Permission denied"}}`, verify `SdkOutputEvent::Error`
  - [x] 9.16 `test_parse_codex_item_updated_ignored` — parse `{"type":"item.updated","item":{"id":"item_9","type":"agent_message","text":"partial"}}`, verify returns `None`
  - [x] 9.17 `test_parse_codex_top_level_error_with_message` — parse `{"type":"error","message":"API key invalid"}`, verify `SdkOutputEvent::Error { message: "API key invalid" }`
  - [x] 9.18 `test_parse_codex_top_level_error_with_error_field` — parse `{"type":"error","error":{"message":"Connection refused"}}`, verify `SdkOutputEvent::Error { message: "Connection refused" }` (tests the `event.error` fallback path when `event.message` is absent)
  - [x] 9.19 `test_parse_codex_top_level_error_no_details` — parse `{"type":"error"}`, verify `SdkOutputEvent::Error { message: "Unknown error" }`
  - [x] 9.20 `test_parse_codex_invalid_json` — parse `"not json"`, verify returns `None`
  - [x] 9.21 `test_parse_codex_unknown_event_type` — parse `{"type":"unknown.event"}`, verify returns `None`
  - [x] 9.22 `test_build_codex_config_basic` — verify command is `"codex"`, args contain `exec`, `--json`, `--sandbox`, `workspace-write`, `--ask-for-approval`, `never`, `--model`, `--cd`, `--` separator, and the prompt as last arg. Verify NO `--full-auto` in args.
  - [x] 9.23 `test_build_codex_config_with_cli_path` — verify `cli_path: Some("/custom/codex")` overrides command
  - [x] 9.24 `test_build_codex_config_with_reasoning_effort` — verify `reasoning_effort: Some("high")` adds `--config model_reasoning_effort=high`
  - [x] 9.25 `test_build_codex_config_invalid_reasoning_effort` — verify `reasoning_effort: Some("turbo")` does NOT add `--config` to args (invalid value is skipped)
  - [x] 9.26 `test_build_codex_config_no_mcp_in_args` — verify no `--mcp-config` in args (MCP is via `.codex/config.toml`)
  - [x] 9.27 `test_build_codex_config_prompt_after_separator` — verify `--` appears in args and prompt is the arg immediately after it
  - [x] 9.28 `test_build_codex_prompt_create` — verify create phase produces `"SYSTEM OVERRIDE: communication_language = English\n\n/bmad-create-story {story_key}"`
  - [x] 9.29 `test_build_codex_prompt_dev` — verify dev phase produces `"SYSTEM OVERRIDE: communication_language = English\n\n/bmad-dev-story {specs_path}"`
  - [x] 9.30 `test_build_codex_prompt_review` — verify review phase produces `"SYSTEM OVERRIDE: communication_language = English\n\n/bmad-code-review {specs_path}"`
  - [x] 9.31 `test_generate_mcp_toml_section` — verify JSON→TOML conversion produces valid `[mcp_servers.bmad-supervisor]` section with `command`, `args`, `required`, and `env` subsection
  - [x] 9.32 `test_generate_mcp_toml_section_escapes_special_chars` — verify paths with backslashes and values with double quotes are properly escaped in TOML output
  - [x] 9.33 `test_escape_toml_string` — verify backslash and quote escaping: `C:\path` → `"C:\\path"`, `say "hello"` → `"say \"hello\""`
  - [x] 9.34 `test_merge_mcp_into_config_no_existing` — verify MCP section generated correctly when no existing config
  - [x] 9.35 `test_merge_mcp_into_config_preserves_user_settings` — verify existing `model = "o4-mini"\nsandbox_mode = "workspace-write"` is preserved and MCP section is appended
  - [x] 9.36 `test_merge_mcp_into_config_replaces_stale_supervisor` — verify if existing config has a `[mcp_servers.bmad-supervisor]` from a crashed session, it is replaced (not duplicated)
  - [x] 9.37 `test_remove_toml_section` — verify section and subsections are removed, other content preserved
  - [x] 9.38 `test_write_restore_codex_mcp_config_no_existing` — write config to empty dir, verify file exists, restore (should delete file and dir)
  - [x] 9.39 `test_write_restore_codex_mcp_config_with_backup` — write existing content, write MCP config, restore, verify original content restored including user settings
  - [x] 9.40 `test_run_session_codex_dispatches` — build `BotConfig` with `provider: "codex"` and `cli_path` pointing to a test script that outputs Codex NDJSON, call `run_session()`, verify `SessionOutcome::Completed` (requires test script — see Dev Notes on testability)
  - [x] 9.41 Verify all 1402+ existing tests still pass with zero changes

- [x] Task 10: Verify full test suite (AC: #10)
  - [x] 10.1 Run `cargo clippy -- -D warnings` — zero new clippy lints
  - [x] 10.2 Run `cargo test` — all existing + new tests pass
  - [x] 10.3 Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Architecture Decision Reference

This story implements the Codex provider adapter from **Decision 12: Dual Runtime Abstraction**.
[Source: architecture.md#Decision 12 — SdkCodexProvider]

The Codex CLI is invoked without any bare/restricted mode so it discovers project skills, `AGENTS.md`, and conventions natively. Skills are invoked via native slash commands — no system preamble, no inlined skill content.

### Design: Codex Session Flow

```
pipeline.rs → SessionRuntime::Sdk(SdkRuntime)
    └── SdkRuntime::run_session(context)
            └── run_codex_session(context)
                    ├── build_codex_prompt(phase, story)           ← "SYSTEM OVERRIDE...\n\n/bmad-dev-story {path}"
                    ├── write_codex_mcp_config(project_root, mcp)  ← .codex/config.toml with [mcp_servers.bmad-supervisor]
                    ├── build_codex_config(role, root, prompt)
                    │       └── SdkSessionConfig { command: "codex", args: ["exec", "--json", ...], ... }
                    ├── execute_session(config, parse_codex_line)
                    │       ├── spawn subprocess (tokio::process::Command)
                    │       ├── stream stdout → parse_codex_line → SdkOutputEvent
                    │       ├── emit UI events (UiHandle)
                    │       ├── track session ID from thread.started
                    │       └── capture completion text from item.completed agent_message
                    ├── restore_codex_mcp_config(project_root, backup)
                    └── map_sdk_result_to_outcome(result, story, decisions)
                            ├── escalation? → SessionOutcome::Escalated
                            ├── success? → SessionOutcome::Completed
                            └── failure → SessionOutcome::Failed
```

### Codex CLI Flags Reference

| Flag | Value | Purpose |
|---|---|---|
| `exec` | subcommand | Non-interactive mode |
| `--json` | boolean | NDJSON event stream on stdout |
| `--sandbox` | `workspace-write` | Allow writes within the workspace |
| `--ask-for-approval` | `never` | Fully unattended — never pause for human approval |
| `--model` | `{configured_model}` | Model override from `LlmRoleConfig.model` |
| `--cd` | `{project_root}` | Working directory for the session |
| `--config` | `key=value` | Override config values (used for reasoning_effort) |
| `--` | separator | Separates flags from the positional prompt |
| (positional) | `"{prompt}"` | The task prompt — after `--` separator |

**NOT used:** `--full-auto` (uses `on-request` approvals which can pause in headless mode — explicit `--sandbox` + `--ask-for-approval never` is safer for daemon), `--dangerously-bypass-approvals-and-sandbox` (too permissive — grants full filesystem and network access), `--ephemeral` (we want session persistence for `codex exec resume`), `--output-last-message` (we capture from stream), `--skip-git-repo-check` (repo expected).

### Codex NDJSON Event Types

| Event Type | Maps To | Key Fields |
|---|---|---|
| `thread.started` | `SessionStarted` | `thread_id` |
| `turn.started` | `Progress` | (none) |
| `turn.completed` | (ignored) | `usage { input_tokens, cached_input_tokens, output_tokens }` |
| `turn.failed` | `Error` | `error { message }` |
| `item.started` + `command_execution` | `ToolCall` | `item { command, status }` |
| `item.started` + `file_change` | `ToolCall` | `item { status }` |
| `item.started` + `mcp_tool_call` | `ToolCall` | `item { server, tool, status }` |
| `item.started` + `web_search` | `ToolCall` | `item { query }` |
| `item.started` + `reasoning` | `Progress` | `item { status }` |
| `item.completed` + `agent_message` | `Completion` | `item { text }` |
| `item.completed` + tool types | `ToolResult` | `item { status }` |
| `item.completed` + `error` | `Error` | `item { text }` |
| `item.updated` | (ignored) | Intermediate state |
| `error` | `Error` | `message` |

### Codex Item Types

| Item Type | Description | Emitted As |
|---|---|---|
| `agent_message` | Final agent text response | `Completion` (on item.completed) |
| `reasoning` | Model reasoning trace | `Progress` (on item.started) |
| `command_execution` | Shell command run | `ToolCall` / `ToolResult` |
| `file_change` | File create/edit/delete | `ToolCall` / `ToolResult` |
| `mcp_tool_call` | MCP server tool invocation | `ToolCall` / `ToolResult` |
| `web_search` | Web search query | `ToolCall` / `ToolResult` |
| `todo_list` | Agent plan/checklist | (ignored) |
| `error` | Error item | `Error` |

### Prompt Construction Per Phase

| Phase | Prompt (with override prefix) | Skill Invoked |
|---|---|---|
| `create` | `SYSTEM OVERRIDE: ...\n\n/bmad-create-story {story_key}` | `bmad-create-story` |
| `dev` (default) | `SYSTEM OVERRIDE: ...\n\n/bmad-dev-story {specs_path}` | `bmad-dev-story` |
| `review` | `SYSTEM OVERRIDE: ...\n\n/bmad-code-review {specs_path}` | `bmad-code-review` |

### MCP Config Strategy for Codex

Codex does not accept `--mcp-config` as a CLI argument. MCP servers are configured via:
- Global: `~/.codex/config.toml`
- Project-scoped: `.codex/config.toml` (in project root)

**Strategy:**
1. Before session: read existing `.codex/config.toml` (if any), backup full content
2. Merge `[mcp_servers.bmad-supervisor]` section into the config (preserve all user settings)
3. If a stale `[mcp_servers.bmad-supervisor]` section exists from a prior crash, remove it before appending
4. Write merged config
5. Run session — Codex discovers MCP server from project config
6. After session: restore original `.codex/config.toml` or delete generated file

**TOML format (appended section):**
```toml
[mcp_servers.bmad-supervisor]
command = "bmad-bot"
args = ["mcp-supervisor", "--config", "/path/to/bmad-bot.yaml", "--story", "15-6-codex"]
required = true

[mcp_servers.bmad-supervisor.env]
ANTHROPIC_API_KEY = "sk-..."
OPENAI_API_KEY = "sk-..."
```

**Project trust requirement:** Codex only reads `.codex/config.toml` for "trusted projects." If the project is not trusted, the MCP supervisor won't load. Story 15.8 (init command) should guide users through `codex trust .`. For now, emit a `tracing::info!` when writing the config to surface this requirement.

**Trade-offs vs Claude Code approach:**
- Claude Code: temp file (auto-cleanup, no repo contamination, inherently safer on crash)
- Codex: project-scoped config (requires manual backup/restore, but is Codex's supported config mechanism)

**Crash safety:** If the daemon crashes (SIGKILL, power loss) before restore, `.codex/config.toml` will contain API keys. Mitigations:
1. `.codex/` should be in `.gitignore` — Story 15.8 init command should verify/add this
2. The merge approach means a stale supervisor section (with API keys) persists but is harmless — it references a `bmad-bot mcp-supervisor` process that won't be running. On next daemon start, the section is replaced.
3. The `remove_toml_section()` logic in `merge_mcp_into_config()` cleans up stale sections from prior crashes before appending fresh config.

### Key Differences from Claude Code Provider (sdk_claude.rs)

| Aspect | Claude Code | Codex |
|---|---|---|
| CLI command | `claude -p "{prompt}"` | `codex exec -- "{prompt}"` |
| Output format flag | `--output-format stream-json` | `--json` |
| Permission mode | `--permission-mode acceptEdits` + `--allowedTools` | `--sandbox workspace-write` + `--ask-for-approval never` |
| Language override | `--append-system-prompt "OVERRIDE: ..."` | Prepended in prompt text |
| MCP config | `--mcp-config {temp_file_path}` (JSON) | `.codex/config.toml` (TOML) |
| Session ID field | `system.init` → `session_id` | `thread.started` → `thread_id` |
| Completion text | `result` event → `result` field | `item.completed` + `agent_message` → `text` field |
| Max turns | `--max-turns 200` | No equivalent (Codex manages internally) |
| Reasoning effort | Not applicable | `--config model_reasoning_effort={value}` |
| Sandbox | N/A | workspace-write (from `--full-auto`) |
| Turn limit errors | `result.error_max_turns` | `turn.failed` |

### Shared Code with Claude Code Provider

The following functions from `sdk_claude.rs` are provider-agnostic and reused by Codex:
- `map_sdk_result_to_outcome()` — decisions reading, escalation detection, result mapping
- `read_decisions_json_sidecar()` — JSON sidecar file reading
- `detect_escalation()` — typed `DecisionSource::Escalation` scanning

Only `map_sdk_result_to_outcome` needs a visibility change to `pub(crate)` — the other two are already `pub`. No renaming. The dependency direction (codex → claude helpers) is acceptable since these functions have no Claude-specific logic.

### Previous Story Intelligence

**Story 15.5** (Claude Code provider — done):
- `parse_claude_code_line()` — parser pattern to follow
- `build_claude_code_config()` — config builder pattern
- `build_claude_code_prompt()` — prompt construction pattern
- `run_claude_code_session()` — orchestration pattern (canonicalize root, MCP config, execute, map result)
- `write_mcp_config_temp_file()` — temp file approach (Codex uses different strategy)
- `map_sdk_result_to_outcome()` — reusable result mapping
- 29 tests, 1402 total passing

**Story 15.3** (SDK runtime — done):
- `SdkRuntime::execute_session(config, parser)` — fully functional subprocess management
- `SdkSessionConfig`, `SdkOutputEvent`, `SdkSessionResult` — provider-agnostic types
- `merge_env_vars()` — injects `OPENAI_API_KEY` (critical for Codex)
- `resolve_provider_for_role()` — role → provider string
- Graceful shutdown: SIGTERM → SIGKILL with configurable grace period

**Story 15.2** (Config extension — done):
- `is_sdk_provider()` → true for "claude-code", "codex"
- `cli_path: Option<String>` on `LlmRoleConfig`
- `resolve_cli_name("codex")` → `"codex"`
- `validate_sdk_providers()` checks CLI availability and skill files

### Git Intelligence

Recent commits follow `feat(epic-15):` convention. Convention for this story: `feat(epic-15): add Codex provider integration (Story 15.6)`

### Current Module State

**`src/runtime/sdk.rs`** (~780 lines, 23 tests):
- `SdkRuntime` struct with `config`, `secrets`, `config_path`, `shutdown`, `ui`
- `run_session()`: dispatches claude-code, fallback for others (line 177-190)
- `execute_session()`: generic subprocess management (line 193+)
- `merge_env_vars()`: API key injection
- `config_for_role()`, `resolve_provider_for_role()`: role config resolution

**`src/runtime/sdk_claude.rs`** (~714 lines, 26 tests):
- `parse_claude_code_line()`: Claude Code JSON parser
- `build_claude_code_config()`: CLI config builder
- `build_claude_code_prompt()`: phase → slash command
- `run_claude_code_session()`: full orchestration
- `map_sdk_result_to_outcome()`: result mapping (to be shared)
- `read_decisions_json_sidecar()`: JSON sidecar reader (already pub)
- `detect_escalation()`: escalation scanner (already pub)

**`src/runtime/mod.rs`** (~386 lines, 11 tests):
- `SessionRuntime` enum: `Api(Box<ApiRuntime>)`, `Sdk(SdkRuntime)`
- `SessionContext` struct
- `pub mod sdk;`, `pub mod sdk_claude;` (add `pub mod sdk_codex;`)

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Zero-warning policy: `#![deny(clippy::all)]` at crate root
- All tests inline in `#[cfg(test)] mod tests { ... }` at bottom of module
- Parser tests (9.1-9.19): plain `#[test]` — synchronous, no subprocess
- Config builder tests (9.20-9.23): plain `#[test]` — pure functions
- Prompt builder tests (9.24-9.26): plain `#[test]` — pure functions
- MCP config tests (9.27-9.29): plain `#[test]` — filesystem operations with `tempdir`
- Integration test (9.30): `#[tokio::test]` — spawns subprocess with test script outputting Codex NDJSON
- Use `UiHandle::null()` for all tests

### Testability Considerations

For Task 9.30 (integration test), create a test helper that writes a temporary shell script outputting Codex NDJSON:
```bash
#!/bin/sh
echo '{"type":"thread.started","thread_id":"test-codex-session-456"}'
echo '{"type":"item.completed","item":{"id":"item_1","type":"agent_message","text":"All done"}}'
```
Set `cli_path` to this script in the test config. Gate with `#[cfg(unix)]`.

For Task 9.27-9.29 (MCP config tests), use `tempfile::TempDir` for isolated filesystem operations.

### Anti-Patterns to Avoid

- Do NOT use `--full-auto` — it sets `--ask-for-approval on-request` which can pause for human input in headless mode. Use explicit `--sandbox workspace-write --ask-for-approval never`
- Do NOT add `--dangerously-bypass-approvals-and-sandbox` — grants full filesystem and network access beyond what the daemon needs
- Do NOT inline skill content or system preamble — SDK mode relies on native skill invocation
- Do NOT implement `codex resume` logic — that's Story 15.7 (crash recovery / consultation injection)
- Do NOT wire `SdkRuntime` into `StoryPipeline::new()` — that's Story 15.7
- Do NOT add WAL fields (`runtime_type`, `sdk_session_ids`) — that's Story 15.7
- Do NOT modify `ApiRuntime`, `SessionRunner`, or any API-mode code
- Do NOT modify `src/mcp_server/` — `generate_mcp_supervisor_config()` used as-is
- Do NOT modify `src/config/mod.rs` — config already supports Codex from 15.2
- Do NOT modify `src/pipeline.rs` — pipeline routing is Story 15.7
- Do NOT pass MCP config JSON as a CLI argument — write to `.codex/config.toml`
- Do NOT replace the entire `.codex/config.toml` — merge the MCP server section into the existing config to preserve user settings
- Do NOT use `Option<Option<String>>` for backup state — use the `CodexMcpBackup` struct for clarity
- Do NOT return `decisions: vec![]` for SDK sessions — pipeline reads from `SessionOutcome.decisions`
- Do NOT use raw string literals for phase matching — use `PHASE_CREATE`, `PHASE_REVIEW` constants
- Do NOT slice Rust strings at byte offsets for truncation — use `chars().take(n)` for UTF-8 safety
- Do NOT add a `toml` crate dependency for config generation — manual string building is sufficient for this simple case
- Do NOT modify `.gitignore` — that's a user responsibility, documented in init command
- Do NOT leave `.codex/config.toml` with API keys after session completes — always restore/cleanup

### Deferred Items

From this story scope — handled by later stories:
- **Pipeline wiring of `SdkRuntime`** — Story 15.7 constructs `SdkRuntime` in `StoryPipeline::new()` and routes phases
- **Session resume via `codex exec resume`** — Story 15.7 implements crash recovery using persisted session IDs
- **WAL `sdk_session_ids` persistence** — Story 15.7 adds `HashMap<String, String>` to WAL
- **Init command Codex setup** — Story 15.8 guides users through SDK provider selection

### Project Structure Notes

New files to create:
- `src/runtime/sdk_codex.rs` — `parse_codex_line()`, `build_codex_config()`, `build_codex_prompt()`, MCP config management, parser types, tests

Files to modify:
- `src/runtime/sdk.rs` — Add `"codex"` arm in `run_session()` dispatch, update error message
- `src/runtime/mod.rs` — Add `pub mod sdk_codex;`
- `src/runtime/sdk_claude.rs` — Change `map_sdk_result_to_outcome` visibility to `pub(crate)`

Files NOT to modify:
- `src/mcp_server/mod.rs` — `generate_mcp_supervisor_config()` used as-is
- `src/config/mod.rs` — config unchanged (already supports codex from 15.2)
- `src/pipeline.rs` — pipeline routing is Story 15.7
- `src/session/*` — session code untouched
- `src/tools/*` — tool implementations untouched
- `src/supervisor/*` — supervisor unchanged (JSON sidecar already implemented in 15.5)
- `src/cli/mod.rs` — no changes needed (JSON sidecar call already added in 15.5)
- `src/main.rs` — no new modules (sdk_codex is a submodule of runtime)
- `Cargo.toml` — no new dependencies (tempfile already present from 15.5)

### References

- [Source: architecture.md#Decision 12 — Dual Runtime Abstraction, SdkCodexProvider]
- [Source: architecture.md#Decision 13 — Supervisor MCP Server, MCP config for SDK sessions]
- [Source: architecture.md#Decision 5 — Amendment: SDK mode uses native skill invocation]
- [Source: planning-artifacts/sprint-change-proposal-2026-04-26.md — Story 15.6 definition]
- [Source: planning-artifacts/epics.md#Epic 15, Story 15.6 — Codex Provider Integration]
- [Source: src/runtime/sdk.rs:177-190 — run_session() dispatch to add "codex" arm]
- [Source: src/runtime/sdk.rs:94-101 — SdkRuntime struct]
- [Source: src/runtime/sdk.rs:193-335 — execute_session() subprocess management]
- [Source: src/runtime/sdk.rs:158-170 — merge_env_vars() API key injection]
- [Source: src/runtime/sdk_claude.rs — Pattern template: parser, config builder, prompt, orchestration]
- [Source: src/runtime/sdk_claude.rs:281-336 — map_sdk_result_to_outcome() to share via pub(crate)]
- [Source: src/runtime/sdk_claude.rs:342-362 — read_decisions_json_sidecar() already pub]
- [Source: src/runtime/sdk_claude.rs:368-375 — detect_escalation() already pub]
- [Source: src/runtime/mod.rs:1-2 — Module declarations (add pub mod sdk_codex)]
- [Source: src/config/mod.rs:696-702 — resolve_cli_name("codex") → "codex"]
- [Source: src/config/mod.rs:704-710 — sdk_provider_skill_dir("codex") → ".agents/skills"]
- [Source: src/config/mod.rs:196-233 — LlmRoleConfig (provider, model, cli_path, reasoning_effort)]
- [Source: src/session/state.rs:18-28 — Phase constants (PHASE_CREATE, PHASE_DEV, PHASE_REVIEW)]
- [Source: src/mcp_server/mod.rs:240-274 — generate_mcp_supervisor_config()]
- [Source: src/watcher/mod.rs:68-88 — StoryInfo struct]
- [Source: _bmad-output/project-context.md — Project rules and conventions]
- [Source: _bmad-output/implementation-artifacts/15-5-claude-code-provider-integration.md — Previous story context]
- [Web: https://developers.openai.com/codex/noninteractive — Codex exec NDJSON format]
- [Web: https://developers.openai.com/codex/cli/reference — Codex CLI flags reference]
- [Web: https://developers.openai.com/codex/mcp — Codex MCP configuration (TOML format)]
- [Web: https://developers.openai.com/codex/config-reference — Codex config.toml schema]
- [Web: https://pkg.go.dev/github.com/picatz/openai/codex — Go SDK with typed event definitions]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

None

### Completion Notes List

- Created `src/runtime/sdk_codex.rs` (~430 lines of production code + ~440 lines of tests) with full Codex provider integration
- Implemented `parse_codex_line()` NDJSON parser mapping all Codex event types (thread.started, turn.*, item.started/completed/updated, error) to `SdkOutputEvent` variants
- Implemented `build_codex_config()` with `codex exec --json --sandbox workspace-write --ask-for-approval never` flags, reasoning_effort validation, and `--` separator for prompt safety
- Implemented `build_codex_prompt()` with `SYSTEM OVERRIDE: communication_language = English` prefix and phase-specific slash commands
- Implemented MCP config management via `.codex/config.toml`: backup/write/merge/restore cycle with stale section cleanup, TOML string escaping, and best-effort restore
- Implemented `run_codex_session()` full orchestration following the same pattern as Claude Code provider
- Changed `map_sdk_result_to_outcome()` visibility to `pub(crate)` in `sdk_claude.rs` for cross-provider reuse
- Added `"codex"` dispatch arm in `SdkRuntime::run_session()`, updated error message to remove Story 15.6 reference
- Added `pub mod sdk_codex;` to `src/runtime/mod.rs`
- 40 new tests (21 parser, 6 config builder, 3 prompt, 7 MCP config, 2 write/restore, 1 integration)
- All 1442 tests pass (1402 existing + 40 new), zero clippy warnings in new code, formatting clean

### Change Log

- 2026-04-27: Implemented Story 15.6 — Codex provider integration with full NDJSON parser, session config builder, MCP config management, and comprehensive tests

### File List

New files:
- src/runtime/sdk_codex.rs

Modified files:
- src/runtime/mod.rs (added `pub mod sdk_codex;`)
- src/runtime/sdk.rs (added `"codex"` dispatch arm, updated error message, updated test assertion)
- src/runtime/sdk_claude.rs (`map_sdk_result_to_outcome` visibility changed to `pub(crate)`)

### Review Findings

- [x] [Review][Patch] Empty error message in `parse_codex_line` fallback path — `or_else` branch does not filter empty `error.message` [src/runtime/sdk_codex.rs:168] ✓ fixed
- [x] [Review][Defer] Race condition on `.codex/config.toml` (concurrent sessions) — deferred, daemon runs one story at a time per project; documented limitation
- [x] [Review][Defer] `remove_toml_section` naive TOML parsing — deferred, spec explicitly chose manual string building over `toml` crate; handles bmad-bot-generated sections correctly
- [x] [Review][Defer] `escape_toml_string` does not escape control characters (newlines, tabs) — deferred, input values from `generate_mcp_supervisor_config()` never contain control chars
- [x] [Review][Defer] Secret leakage in `.codex/config.toml` on daemon crash — deferred, documented trade-off with mitigations planned for Story 15.8 (`.gitignore`, stale cleanup on next run)
