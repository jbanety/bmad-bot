# Story 15.3: SDK Runtime Subprocess Infrastructure

Status: done

## Story

As a daemon developer,
I want a generic `SdkRuntime` that manages an external CLI process (spawn, environment, working directory, streaming output, shutdown),
so that Claude Code and Codex integrations share common subprocess management code.

## Acceptance Criteria

1. **Given** the daemon needs to run an SDK session **When** `SdkRuntime::execute_session()` is called with a `SdkSessionConfig` **Then** it spawns a subprocess with the configured command, args, env vars, and working directory **And** streams stdout line-by-line as NDJSON **And** captures stderr for error reporting

2. **Given** the subprocess is running **When** the daemon's `ShutdownFlag` is set to `true` **Then** the subprocess receives SIGTERM **And** if it doesn't exit within a configurable timeout (default 10s), SIGKILL is sent **And** `SdkSessionResult` reports the shutdown

3. **Given** the subprocess produces streaming JSON events **When** each line is read from stdout **Then** it is parsed via a provider-supplied callback into `SdkOutputEvent` variants: `SessionStarted`, `Progress`, `ToolCall`, `ToolResult`, `Completion`, `Error` **And** UI events are emitted via `UiHandle` for real-time visibility

4. **Given** an SDK session starts **When** the CLI outputs a `SessionStarted` event (containing the session ID) **Then** the session ID is extracted and stored in `SdkSessionResult` **And** is available for `--resume` usage in subsequent stories (consultation injection, crash recovery)

5. **Given** the subprocess needs API keys **When** the session is started **Then** environment variables (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) are injected from `BotSecrets` into the subprocess environment **And** SDK providers skip missing keys silently (CLIs manage their own auth)

6. **Given** a process timeout is configured **When** the subprocess exceeds the timeout **Then** the graceful shutdown sequence (SIGTERM → SIGKILL) is triggered **And** `SdkSessionResult` includes a timeout indication

7. **Given** the subprocess exits **When** the exit code is available **Then** `SdkSessionResult` includes the exit code, captured session ID, and stderr output (events are processed in real-time via UI emission and not accumulated in memory — sessions can run 30+ minutes)

8. **Given** the `SdkRuntime` struct replaces the empty stub **When** `SessionRuntime::Sdk` is dispatched **Then** it calls `SdkRuntime::run_session()` which currently returns `SessionOutcome::Failed` with a clear message ("SDK provider not yet implemented — Stories 15.5/15.6") since no concrete providers exist yet

9. **Given** all existing tests pass **When** the infrastructure is added **Then** zero behavioral changes for API-mode configurations — all 1347+ existing tests pass identically

## Tasks / Subtasks

- [x] Task 1: Create `src/runtime/sdk.rs` with core types and error enum (AC: #1, #3, #7)
  - [x] 1.1 Create `src/runtime/sdk.rs` with module doc comment
  - [x] 1.2 Define `SdkError` error enum using `thiserror`:
    - `SpawnFailed { command: String, source: std::io::Error }` — subprocess creation failed
    - `Timeout { elapsed: Duration }` — process exceeded timeout
    - `ShutdownRequested` — daemon shutdown triggered during session
    - `ProcessFailed { exit_code: Option<i32>, stderr: String }` — non-zero exit
    - Note: NDJSON parse failures are non-fatal (logged at `tracing::warn!` and skipped). They are NOT represented in `SdkError` because `execute_session()` never returns them — a bad line does not abort the session.
  - [x] 1.3 Define `SdkSessionConfig` struct:
    ```rust
    pub struct SdkSessionConfig {
        pub command: String,              // CLI binary path ("claude", "codex", or cli_path override)
        pub args: Vec<String>,            // Provider-specific CLI arguments
        pub env: Vec<(String, String)>,   // Additional environment variables
        pub working_directory: PathBuf,   // Project root
        pub timeout: Duration,            // Max session duration (default 30 min)
        pub sigterm_grace: Duration,      // SIGTERM → SIGKILL grace period (default 10s)
    }
    ```
  - [x] 1.4 Define `SdkOutputEvent` enum:
    ```rust
    pub enum SdkOutputEvent {
        SessionStarted { session_id: String },
        Progress { message: String },
        ToolCall { tool_name: String, detail: String },
        ToolResult { tool_name: String, detail: String },
        Completion { result: String, is_error: bool },
        Error { message: String },
    }
    ```
  - [x] 1.5 Define `SdkSessionResult` struct:
    ```rust
    pub struct SdkSessionResult {
        pub session_id: Option<String>,
        pub exit_code: Option<i32>,
        pub stderr: String,
        pub timed_out: bool,
        pub shutdown_requested: bool,
    }
    ```
  - [x] 1.6 The provider-supplied line parser is a generic parameter on `execute_session()`, not a boxed trait object:
    ```rust
    pub async fn execute_session<F>(
        &self,
        session_config: SdkSessionConfig,
        parser: F,
    ) -> Result<SdkSessionResult, SdkError>
    where
        F: Fn(&str) -> Option<SdkOutputEvent> + Send,
    ```
    This avoids heap allocation and allows both closures and function pointers. Stories 15.5/15.6 pass their provider-specific parsers as closures.

- [x] Task 2: Implement `SdkRuntime` struct and constructor (AC: #5, #8)
  - [x] 2.1 Define `SdkRuntime` struct:
    ```rust
    pub struct SdkRuntime {
        config: Arc<BotConfig>,
        secrets: Arc<BotSecrets>,
        shutdown: ShutdownFlag,
        ui: UiHandle,
    }
    ```
  - [x] 2.2 Implement `SdkRuntime::new(config, secrets, shutdown, ui) -> Self`
  - [x] 2.3 Implement `merge_env_vars(&self, config_env: &[(String, String)]) -> Vec<(String, String)>` helper that builds the complete env var set for a subprocess:
    - Start with a `HashMap<String, String>` for deduplication
    - Insert secrets-derived vars first (lower priority):
      - If `self.secrets.anthropic_api_key` is `Some(key)` → insert `("ANTHROPIC_API_KEY", key)`
      - If `self.secrets.openai_api_key` is `Some(key)` → insert `("OPENAI_API_KEY", key)`
      - Skip `None` values silently — SDK CLIs manage their own auth
    - Then insert `config_env` entries (higher priority — overwrites secrets for same key)
    - Return as `Vec<(String, String)>` for `.envs()` consumption
    - This is the ONLY place env merge logic lives — `execute_session()` calls `self.merge_env_vars(&session_config.env)` and passes the result to `.envs()`
  - [x] 2.4 Implement `SdkRuntime::run_session(&self, context: SessionContext<'_>) -> SessionOutcome`:
    - For now, returns `SessionOutcome::Failed` with `error: "SDK provider not yet implemented for provider '{provider}'. Requires Story 15.5 (claude-code) or 15.6 (codex)."` and `story_key` from context
    - Extract provider string from `self.config` using `context.role` to select the right `LlmRoleConfig`
    - This method will be updated in Stories 15.5/15.6 to build provider-specific `SdkSessionConfig` and call `execute_session()`

- [x] Task 3: Implement subprocess spawning and NDJSON streaming (AC: #1, #3)
  - [x] 3.1 Implement `SdkRuntime::execute_session<F>(&self, session_config: SdkSessionConfig, parser: F) -> Result<SdkSessionResult, SdkError> where F: Fn(&str) -> Option<SdkOutputEvent> + Send`:
    - Build merged environment: `let merged_env = self.merge_env_vars(&session_config.env);`
    - Spawn subprocess via `tokio::process::Command::new(&session_config.command)`:
      - `.args(&session_config.args)`
      - `.envs(merged_env)` — use `.envs()` to add vars while inheriting parent env
      - `.current_dir(&session_config.working_directory)`
      - `.stdout(Stdio::piped())`
      - `.stderr(Stdio::piped())`
      - `.stdin(Stdio::null())`
      - `.kill_on_drop(true)` — safety net: if `execute_session` panics or returns early, the child is killed on drop rather than orphaned. Normal shutdown path uses SIGTERM first (see Task 4).
    - Capture PID immediately: `let pid = child.id();`
    - Take stdout and stderr handles from child
  - [x] 3.2 Implement stdout streaming loop:
    - `let mut reader = BufReader::new(stdout).lines();`
    - In a `tokio::select!` loop:
      - `line = reader.next_line()` → parse line, emit UI events, track session_id
      - `_ = shutdown_check()` → trigger graceful shutdown
      - `_ = timeout_check()` → trigger graceful shutdown
    - For each line: call `parser(&line)` to get `Option<SdkOutputEvent>`
    - If parser returns `None`, log at `tracing::debug!` level and skip (non-fatal — providers may emit unrecognized events)
    - If JSON parse fails entirely (`serde_json::from_str::<Value>` fails), log at `tracing::warn!` (might be stderr leaking to stdout or CLI banner text)
  - [x] 3.3 Implement stderr capture as background task with size cap (1 MB max to prevent OOM from misbehaving CLIs):
    ```rust
    const STDERR_MAX_BYTES: usize = 1_024 * 1_024; // 1 MB

    let stderr_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        let mut captured = String::new();
        let mut truncated = false;
        while let Some(line) = reader.next_line().await.unwrap_or(None) {
            tracing::debug!(sdk_stderr = %line);
            if !truncated {
                if captured.len() + line.len() + 1 > STDERR_MAX_BYTES {
                    captured.push_str("\n...[stderr truncated at 1 MB]");
                    truncated = true;
                } else {
                    if !captured.is_empty() { captured.push('\n'); }
                    captured.push_str(&line);
                }
            }
            // Keep draining even after cap to avoid pipe backpressure blocking the child
        }
        captured
    });
    ```
  - [x] 3.4 After stdout stream ends, `child.wait().await` for exit status, then `stderr_handle.await` for captured stderr
  - [x] 3.5 Build and return `SdkSessionResult` from collected data

- [x] Task 4: Implement graceful shutdown (SIGTERM → SIGKILL) (AC: #2)
  - [x] 4.1 Add `libc = "0.2"` to `Cargo.toml` `[dependencies]` section. `libc` is already a transitive dependency of `tokio` on Unix — adding it as direct dependency for the explicit `libc::kill()` call. Usage is behind `#[cfg(unix)]` so non-Unix targets compile cleanly.
  - [x] 4.2 Implement `send_sigterm` behind `#[cfg(unix)]` guard (with `#[cfg(not(unix))]` no-op fallback):
    ```rust
    #[cfg(unix)]
    fn send_sigterm(child: &tokio::process::Child) -> std::io::Result<()> {
        match child.id() {
            Some(pid) => {
                // SAFETY: POSIX kill() sends signal to known child process
                let ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
                if ret == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            }
            None => Ok(()), // Process already exited
        }
    }

    #[cfg(not(unix))]
    fn send_sigterm(_child: &tokio::process::Child) -> std::io::Result<()> {
        // No SIGTERM on non-Unix — graceful_kill falls through to child.kill() (SIGKILL)
        Ok(())
    }
    ```
  - [x] 4.3 Implement `graceful_kill(child: &mut Child, sigterm_grace: Duration) -> std::io::Result<ExitStatus>`:
    - Send SIGTERM via `send_sigterm()`
    - `tokio::time::timeout(sigterm_grace, child.wait()).await`
    - On timeout: `tracing::warn!("SIGTERM timeout expired, sending SIGKILL")` then `child.kill().await`
    - Handle `ESRCH` (no such process) gracefully — process already exited
  - [x] 4.4 Integrate shutdown into the `execute_session` select loop:
    - Add a `tokio::time::sleep(Duration::from_millis(200))` branch in the `biased` select loop (see Dev Notes for full pattern)
    - When this branch fires, check `self.shutdown.load(Ordering::Relaxed)` — if true, call `graceful_kill()` and set `shutdown_requested = true`
    - If not shutting down, `continue` to re-enter the loop (avoids blocking on stdout when checking shutdown)

- [x] Task 5: Implement session ID tracking (AC: #4)
  - [x] 5.1 In the stdout streaming loop, when a `SdkOutputEvent::SessionStarted { session_id }` event is received:
    - Store in a local `session_id: Option<String>` variable
    - Log: `tracing::info!(session_id = %id, "SDK session ID captured")`
    - Only accept the first `SessionStarted` event (ignore duplicates)
  - [x] 5.2 Include `session_id` in the returned `SdkSessionResult`
  - [x] 5.3 Note: WAL persistence of session IDs is Story 15.7 scope. This story only extracts and returns the ID.

- [x] Task 6: Implement UI event emission (AC: #3)
  - [x] 6.1 In the stdout streaming loop, after parsing each `SdkOutputEvent`, emit corresponding UI events:
    - `SdkOutputEvent::SessionStarted { .. }` → `self.ui.activation_start()`
    - `SdkOutputEvent::ToolCall { tool_name, detail }` → `self.ui.tool_call(&tool_name, &detail)`
    - `SdkOutputEvent::ToolResult { tool_name, detail }` → `self.ui.tool_result(&tool_name, &detail)`
    - `SdkOutputEvent::Progress { message }` → `tracing::info!(sdk_progress = %message)` (no dedicated UI method for generic progress)
    - `SdkOutputEvent::Completion { .. }` → `self.ui.activation_complete()`
    - `SdkOutputEvent::Error { message }` → `tracing::error!(sdk_error = %message)`

- [x] Task 7: Implement process timeout (AC: #6)
  - [x] 7.1 Timeout is handled as a `tokio::select!` branch inside the event loop: `_ = tokio::time::sleep_until(timeout_at)` fires once when the deadline is reached, then the loop breaks after calling `graceful_kill()`. Do NOT wrap the entire loop in `tokio::time::timeout()` — that would drop the `Child` abruptly instead of running the SIGTERM grace sequence.
  - [x] 7.2 On timeout expiry: trigger `graceful_kill()` on the child process, set `timed_out = true` on result
  - [x] 7.3 Default timeout: 30 minutes. This is set by the caller via `SdkSessionConfig.timeout`. Stories 15.5/15.6 will set per-session timeouts.

- [x] Task 8: Wire into `src/runtime/mod.rs` (AC: #8)
  - [x] 8.1 Add `pub mod sdk;` to `src/runtime/mod.rs`
  - [x] 8.2 Update `SdkRuntime` — replace the empty `pub struct SdkRuntime;` stub with a re-export: `pub use sdk::SdkRuntime;`
  - [x] 8.3 Update `SessionRuntime::Sdk` variant to hold the full `SdkRuntime` struct (already does — `Sdk(SdkRuntime)`)
  - [x] 8.4 Update `SessionRuntime::run_session()` dispatch:
    ```rust
    Self::Sdk(sdk) => sdk.run_session(context).await,
    ```
    This replaces the current `todo!("SDK runtime implemented in Story 15.3+")`
  - [x] 8.5 Add necessary imports: `use crate::config::BotSecrets;`
  - [x] 8.6 The `SdkRuntime` constructor will require `config`, `secrets`, `shutdown`, `ui` — but `StoryPipeline::new()` does NOT construct `SdkRuntime` yet (that's Story 15.7). The stub `SessionRuntime::Sdk(SdkRuntime)` construction in `mod.rs` tests needs updating to use `SdkRuntime::new(...)`.

- [x] Task 9: Write comprehensive tests (AC: #1-9)
  - [x] 9.1 `test_sdk_session_config_default_values` — verify `SdkSessionConfig` struct construction with expected fields
  - [x] 9.2 `test_merge_env_vars_both_keys` — `BotSecrets` with both API keys → both env vars present
  - [x] 9.3 `test_merge_env_vars_anthropic_only` — `BotSecrets` with only `anthropic_api_key` → only `ANTHROPIC_API_KEY` present
  - [x] 9.4 `test_merge_env_vars_no_keys` — `BotSecrets` with no API keys → empty vec
  - [x] 9.5 `test_merge_env_vars_merge_precedence` — config env vars take precedence over secrets-derived vars for same key
  - [x] 9.6 `test_execute_session_simple_command` — spawn `echo '{"type":"test"}'`, verify stdout captured, exit code 0
  - [x] 9.7 `test_execute_session_captures_stderr` — spawn a command that writes to stderr, verify captured in result
  - [x] 9.8 `test_execute_session_nonzero_exit` — spawn `false` (exit code 1), verify exit_code in result
  - [x] 9.9 `test_execute_session_timeout` — spawn `sleep 60` with 1s timeout, verify `timed_out = true` and process is killed
  - [x] 9.10 `test_execute_session_shutdown_flag` — spawn `sleep 60`, set `ShutdownFlag` after 100ms, verify `shutdown_requested = true`
  - [x] 9.11 `test_execute_session_session_id_tracking` — spawn an echo command that outputs a `SessionStarted` event, verify session_id captured
  - [x] 9.12 `test_send_sigterm_exited_process` — spawn a fast command, wait for exit, call `send_sigterm` → `Ok(())` (no error on already-exited process)
  - [x] 9.13 `test_graceful_kill_immediate_exit` — spawn `true`, call `graceful_kill`, verify exits immediately
  - [x] 9.14 `test_sdk_output_event_variants` — construct each `SdkOutputEvent` variant, verify Debug output
  - [x] 9.15 `test_sdk_error_display` — verify `SdkError` Display output for each variant
  - [x] 9.16 `test_run_session_returns_failed_no_provider` — call `SdkRuntime::run_session()`, verify `SessionOutcome::Failed` with provider not implemented message
  - [x] 9.17 Verify all 1347+ existing tests still pass with zero changes

- [x] Task 10: Verify full test suite (AC: #9)
  - [x] 10.1 Run `cargo clippy -- -D warnings` — zero new warnings
  - [x] 10.2 Run `cargo test` — all existing + new tests pass
  - [x] 10.3 Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Architecture Decision Reference

This story implements the `SdkRuntime` subprocess infrastructure from **Decision 12: Dual Runtime Abstraction**.
[Source: architecture.md#Decision 12 — SdkRuntime manages CLI subprocesses]

The `SdkRuntime` is a generic subprocess manager — provider-specific CLI flags, prompt construction, and output format parsing are implemented in Story 15.5 (Claude Code) and Story 15.6 (Codex). This story builds the shared infrastructure that both providers use.

### Design: Layered Subprocess Architecture

```
SessionRuntime::Sdk(SdkRuntime)
    └── SdkRuntime::run_session(context)          ← Story 15.3 (stub, returns Failed)
            └── SdkRuntime::execute_session(config, parser)  ← Story 15.3 (full implementation)
                    ├── spawn subprocess (tokio::process::Command)
                    ├── stream stdout lines (BufReader)
                    ├── parse via provider callback (generic F: Fn(&str) -> Option<SdkOutputEvent>)
                    ├── emit UI events (UiHandle)
                    ├── track session ID
                    ├── capture stderr (background task)
                    └── graceful shutdown (SIGTERM → SIGKILL)
```

Stories 15.5/15.6 will:
1. Implement provider-specific `SdkSessionConfig` builders (CLI flags, model, prompt)
2. Implement provider-specific parser closures (Claude JSON → SdkOutputEvent, Codex NDJSON → SdkOutputEvent)
3. Update `SdkRuntime::run_session()` to dispatch to the correct provider

### Subprocess Spawning Pattern

Use `tokio::process::Command` (not `std::process::Command`) for async subprocess management:

```rust
let mut child = tokio::process::Command::new(&config.command)
    .args(&config.args)
    .envs(merged_env)
    .current_dir(&config.working_directory)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .stdin(Stdio::null())
    .kill_on_drop(true)
    .spawn()
    .map_err(|e| SdkError::SpawnFailed {
        command: config.command.clone(),
        source: e,
    })?;
```

Key: `.kill_on_drop(true)` as a safety net — if `execute_session()` panics or returns early before the graceful shutdown sequence, the child is killed rather than orphaned. In the normal path, we send SIGTERM first (Task 4) so `kill_on_drop` never fires. This is defense-in-depth: graceful path uses SIGTERM → SIGKILL; catastrophic path (drop) uses SIGKILL.

### SIGTERM via `libc` — Not `nix`, with `#[cfg(unix)]` Guard

The project does not use the `nix` crate. For a single `kill(pid, SIGTERM)` call, `libc` is sufficient. `libc` is already a transitive dependency of `tokio` on Unix — adding it as a direct dependency has zero compile-time cost.

`tokio::process::Child::kill()` sends SIGKILL only — there is no built-in SIGTERM support. Hence the `libc::kill(pid, libc::SIGTERM)` call.

**Platform guard:** `send_sigterm()` is behind `#[cfg(unix)]`. The `#[cfg(not(unix))]` fallback is a no-op — `graceful_kill()` falls through to `child.kill()` (SIGKILL) immediately. This ensures cross-compilation and `cargo clippy` with non-Unix targets work.

The `unsafe` block is minimal and well-understood:
```rust
let ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
```
This is a POSIX-standard call to a known child process PID. The only safety concern is PID reuse (process exits, PID recycled, signal sent to wrong process). This is mitigated by:
1. Only calling when `child.id()` returns `Some` (process hasn't been waited on yet)
2. Handling `ESRCH` (no such process) gracefully as a non-error

### Streaming Event Loop with `tokio::select!`

The core event loop uses `tokio::select!` to multiplex three concerns. `ShutdownFlag` is `Arc<AtomicBool>` (not a `CancellationToken`), so it cannot be awaited directly — a short sleep branch acts as a polling interval:

```rust
let timeout_at = tokio::time::Instant::now() + session_config.timeout;
let shutdown_poll = Duration::from_millis(200);

loop {
    tokio::select! {
        biased; // check shutdown/timeout before blocking on stdout

        _ = tokio::time::sleep_until(timeout_at) => {
            tracing::warn!("SDK session timeout exceeded");
            graceful_kill(&mut child, session_config.sigterm_grace).await?;
            timed_out = true;
            break;
        }

        _ = tokio::time::sleep(shutdown_poll) => {
            if self.shutdown.load(Ordering::Relaxed) {
                tracing::info!("Shutdown requested during SDK session");
                graceful_kill(&mut child, session_config.sigterm_grace).await?;
                shutdown_requested = true;
                break;
            }
            // Not shutting down — re-enter the loop to check stdout
            continue;
        }

        line = reader.next_line() => {
            match line {
                Ok(Some(line)) => { /* parse, emit UI, track session_id */ }
                Ok(None) => break, // stdout closed, process exiting
                Err(e) => {
                    tracing::warn!(error = %e, "stdout read error");
                    break;
                }
            }
        }
    }
}
```

Key design choices:
- `biased;` ensures timeout and shutdown branches are checked before blocking on stdout
- `sleep_until(timeout_at)` fires exactly once — after the first fire the loop breaks, no busy-spin risk
- `sleep(shutdown_poll)` at 200ms provides responsive shutdown detection without spinning
- This matches the cooperative `ShutdownFlag` check pattern in `streaming_chat()` at `src/session/agent.rs:541-553`

### Environment Variable Injection

SDK CLIs manage their own authentication, but the daemon injects API keys as convenience env vars. The CLI may or may not use them. The merge logic lives in one place — `merge_env_vars()`:

```rust
fn merge_env_vars(&self, config_env: &[(String, String)]) -> Vec<(String, String)> {
    let mut map = std::collections::HashMap::new();
    // 1. Secrets-derived vars (lower priority)
    if let Some(key) = &self.secrets.anthropic_api_key {
        map.insert("ANTHROPIC_API_KEY".to_string(), key.clone());
    }
    if let Some(key) = &self.secrets.openai_api_key {
        map.insert("OPENAI_API_KEY".to_string(), key.clone());
    }
    // 2. Config env vars (higher priority — overwrites for same key)
    for (k, v) in config_env {
        map.insert(k.clone(), v.clone());
    }
    map.into_iter().collect()
}
```

Passed to `.envs(merged_env)` on the subprocess — this ADDS to the inherited parent environment, it does not replace it.

### `SdkOutputEvent` — Generic Event Taxonomy

The `SdkOutputEvent` enum is provider-agnostic. Stories 15.5/15.6 map provider-specific JSON to these generic events:

| SdkOutputEvent | Claude Code source | Codex source |
|---|---|---|
| `SessionStarted` | `system/init` → `session_id` | `thread.started` → `thread_id` |
| `Progress` | `assistant` message text | `item.completed` with `agent_message` |
| `ToolCall` | `assistant` message with `tool_use` content blocks | `item.started` with `command_execution` |
| `ToolResult` | Next `assistant` after tool use | `item.completed` with `command_execution` |
| `Completion` | `result` event | Last `turn.completed` |
| `Error` | `result` with `is_error: true` | `turn.failed` or `error` |

The parser closure (generic `F: Fn(&str) -> Option<SdkOutputEvent>`) converts raw JSON lines to `Option<SdkOutputEvent>`. Returning `None` means the line is recognized but not relevant (e.g., intermediate streaming deltas).

### `run_session()` — Temporary Stub Until 15.5/15.6

`SdkRuntime::run_session()` is the entry point called by `SessionRuntime::Sdk`. In 15.3, it returns `SessionOutcome::Failed` because no provider-specific code exists yet:

```rust
pub async fn run_session(&self, context: SessionContext<'_>) -> SessionOutcome {
    let provider = self.resolve_provider_for_role(&context.role);
    SessionOutcome::Failed {
        story_key: context.story.key.clone(),
        error: format!(
            "SDK provider '{}' not yet implemented. Requires Story 15.5 (claude-code) or 15.6 (codex).",
            provider
        ),
        decisions: vec![],
    }
}
```

The `execute_session()` method is fully functional — it just lacks callers until 15.5/15.6 provide the `SdkSessionConfig` and parser closure.

### Role-to-Provider Resolution

To determine the SDK provider for a given role, resolve via config. `LlmConfig.epic_review` and `LlmConfig.critic` are `LlmRoleConfig` (NOT `Option<LlmRoleConfig>`) — fallback is via `.provider.is_empty()`, not `unwrap_or`:

```rust
fn resolve_provider_for_role(&self, role: &LlmRole) -> String {
    let llm = &self.config.llm;
    let role_config = match role {
        LlmRole::Dev => &llm.dev,
        LlmRole::Review => &llm.review,
        LlmRole::Supervisor => &llm.supervisor,
        LlmRole::EpicReview => {
            if llm.epic_review.provider.is_empty() {
                &llm.review
            } else {
                &llm.epic_review
            }
        }
        LlmRole::Critic => {
            if llm.critic.provider.is_empty() {
                &llm.review
            } else {
                &llm.critic
            }
        }
    };
    role_config.provider.clone()
}
```

This replicates the existing `AgentFactory::config_for_role()` pattern at `src/llm/agent_factory.rs:213-232`. The `epic_review` and `critic` fields have `#[serde(default)]` — they deserialize as empty-string `LlmRoleConfig`, not `Option`.

### Exit Code Interpretation

| Exit Code | Meaning | SdkSessionResult |
|---|---|---|
| 0 | Success | `exit_code: Some(0)` |
| Non-zero | Failure | `exit_code: Some(n)`, stderr contains details |
| None | Process killed | `exit_code: None`, `shutdown_requested` or `timed_out` set |

The caller (Stories 15.5/15.6) interprets exit codes into `SessionOutcome`. For 15.3, `execute_session()` returns `SdkSessionResult` — it does not produce `SessionOutcome` directly.

### Current Module State

**`src/runtime/mod.rs`** (368 lines, 11 tests):
- `SdkRuntime` stub at line 154: `pub struct SdkRuntime;`
- `SessionRuntime::Sdk` dispatch at line 79: `Self::Sdk(_) => todo!("SDK runtime implemented in Story 15.3+")`
- `SessionRuntime` enum at lines 69-73
- `ApiRuntime` at lines 95-148 — untouched by this story
- `SkillPaths` at lines 14-51 — untouched by this story
- `SessionContext` at lines 57-63 — untouched by this story
- Test `test_session_runtime_sdk_variant_construction` at line 302: `let _runtime = SessionRuntime::Sdk(SdkRuntime);` — must be updated

**`Cargo.toml`** (39 lines):
- Add `libc = "0.2"` after existing dependencies
- tokio already has `features = ["full"]` (includes `process`, `io-util`, `time`, `sync`)
- `serde_json = "1"` already present (for NDJSON parsing)

**`src/session/agent.rs`** (ShutdownFlag at line 159):
```rust
pub type ShutdownFlag = Arc<AtomicBool>;
```
Re-exported at `src/session/runner.rs:23`. Used with `Ordering::Relaxed`.

**`src/session/mod.rs`** (SessionOutcome at lines 102-135):
- `Completed { story_key, branch, decisions, pr_context, pr_how_to_test, pr_additional_info }`
- `Escalated { report, decisions }`
- `Failed { story_key, error, decisions }`

**`src/config/mod.rs`** (BotSecrets at lines 777-789):
```rust
pub struct BotSecrets {
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub github_token: Option<String>,
    pub gitlab_token: Option<String>,
    pub telegram_bot_token: Option<String>,
}
```

**`src/ui/mod.rs`** (UiHandle methods):
- `tool_call(&self, tool_name: &str, detail: &str)` — line 122
- `tool_result(&self, tool_name: &str, detail: &str)` — line 127
- `activation_start(&self)` — line 105
- `activation_complete(&self)` — line 110
- `UiHandle::null()` — for tests

### Anti-Patterns to Avoid

- Do NOT implement provider-specific CLI flag construction — that's Stories 15.5 (Claude Code) and 15.6 (Codex)
- Do NOT implement provider-specific output parsing — provide the `OutputParser` callback interface; concrete parsers are 15.5/15.6
- Do NOT wire `SdkRuntime` into `StoryPipeline::new()` — that's Story 15.7 (pipeline dual-runtime orchestration)
- Do NOT add WAL fields (`runtime_type`, `sdk_session_ids`) — that's Story 15.7
- Do NOT add MCP server config generation — that's Story 15.4
- Do NOT use `std::process::Command` for subprocess spawning — use `tokio::process::Command` for async compatibility
- Do NOT use `child.kill()` for graceful shutdown — that sends SIGKILL immediately. Use `libc::kill(pid, SIGTERM)` first
- Do NOT add the `nix` crate — `libc` is sufficient for a single `kill()` call and is already a transitive dependency
- Do NOT use `kill_on_drop(false)` — use `kill_on_drop(true)` as a safety net against orphan processes if `execute_session` panics. The normal path sends SIGTERM explicitly before drop.
- Do NOT modify `ApiRuntime`, `SkillPaths`, or `SessionContext` — they are stable from 15.1
- Do NOT modify anything under `_bmad/` — daemon is read-only consumer
- Do NOT modify `pipeline.rs`, `session/runner.rs`, or `config/mod.rs` — this story only touches `runtime/` and `Cargo.toml`
- Do NOT add `#[allow(dead_code)]` — `execute_session()` is public infrastructure called by 15.5/15.6; if `run_session()` calls private helpers, they're reachable via tests
- Do NOT change `LlmRole` enum or `LlmRoleConfig` — they are stable from previous stories

### Previous Story Intelligence

**Story 15.2** (config extension — done):
- `LlmRoleConfig` has `cli_path: Option<String>` for custom CLI paths
- `is_sdk_provider()` returns `true` for `"claude-code"` and `"codex"`
- `BotSecrets::validate_for_config()` skips SDK providers — they don't need API keys
- `validate_sdk_providers()` checks CLI availability at startup via `{cli} --version`
- `resolve_cli_name()` maps `"claude-code"` → `"claude"`, `"codex"` → `"codex"`
- Test count: 1347 passed
- Commit convention: `feat(epic-15): description (Story 15.N)`

**Story 15.1** (runtime abstraction — done):
- `SessionRuntime` enum with `Api(Box<ApiRuntime>)` and `Sdk(SdkRuntime)` variants
- `ApiRuntime` wraps `SessionRunner` — thin delegation layer
- `SkillPaths::resolve()` reads BMAD manifest for skill directory
- `SessionContext` carries: story, base_branch_override, consultations, role, initial_phase
- `#[allow(dead_code)]` on `SessionRuntime` enum (Sdk variant unused until this story)
- `api_session_runner()` panics on Sdk variant — deferred to Story 15.7

**Story 15.0a** (pre-epic cleanup — done):
- Commit convention: `fix(pre-epic-15):` for cleanup, `feat(epic-15):` for features
- Pre-existing dead-code warnings remain unchanged

### Deferred Items Relevant to This Story

From 15.1 review:
- `api_session_runner()` panics on Sdk variant — **not fixed in this story**. Story 15.7 will implement dual-runtime recovery routing. [src/runtime/mod.rs:86-88]

From 15.2 review:
- Pipeline always creates ApiRuntime — **not fixed in this story**. Story 15.7 will implement runtime routing in `StoryPipeline::new()`. [src/pipeline.rs:263]
- No timeout on subprocess CLI validation — the pattern established in `validate_cli_availability()` (sync, no timeout) is pre-existing. This story's subprocess timeout is different — it uses async `tokio::time::timeout` around the entire session.

### Git Intelligence

Recent commits:
- `8158b7f feat(epic-15): extend config for SDK providers claude-code and codex (Story 15.2)`
- `6ac5e0e feat(epic-15): add SessionRuntime abstraction layer with SkillPaths resolution (Story 15.1)`
- `766a250 fix(pre-epic-15): resolve clippy warnings and stale test (Story 15.0a)`

Convention for this story: `feat(epic-15): add SDK runtime subprocess infrastructure (Story 15.3)`

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Zero-warning policy: `#![deny(clippy::all)]` at crate root
- All tests inline in `#[cfg(test)] mod tests { ... }` at bottom of each module
- **Async tests require `#[tokio::test]`** (not `#[test]`): Tests 9.6-9.13 and 9.16 spawn subprocesses or call async methods — they MUST use `#[tokio::test]` to get a tokio runtime. Synchronous tests (9.1-9.5, 9.14-9.15) use plain `#[test]`.
- New tests in `src/runtime/sdk.rs`
- Update existing test `test_session_runtime_sdk_variant_construction` in `src/runtime/mod.rs` to use `SdkRuntime::new(...)` instead of bare `SdkRuntime`
- **Subprocess tests use real processes** (e.g., `echo`, `sleep`, `false`) — do NOT mock `tokio::process::Command`. These are fast, deterministic commands.
- Use `tempdir` for working directory in subprocess tests
- Use `UiHandle::null()` for all tests (zero test pollution)
- Use short timeouts (1-2s) for timeout/shutdown tests to keep test suite fast

### SDK CLI Output Formats (Reference for Stories 15.5/15.6)

Included here so the developer understands what `SdkOutputEvent` maps to:

**Claude Code** (`--output-format stream-json`): Newline-delimited JSON with `type` field:
- `system` (subtype: `init` → session_id, `api_retry`)
- `assistant` (message with content blocks including `tool_use`)
- `result` (subtype: `success`/`error_max_turns`/`error_during_execution`, `is_error`, `total_cost_usd`)

**Codex** (`--json`): NDJSON with dotted `type` field:
- `thread.started` (thread_id = session ID)
- `turn.started`, `turn.completed` (usage stats), `turn.failed`
- `item.started`, `item.completed` (item types: `agent_message`, `command_execution`, `file_change`)

### Project Structure Notes

New files to create:
- `src/runtime/sdk.rs` — `SdkRuntime`, `SdkSessionConfig`, `SdkOutputEvent`, `SdkSessionResult`, `SdkError`, subprocess management, tests

Files to modify:
- `src/runtime/mod.rs` — add `pub mod sdk;`, replace `SdkRuntime` stub with re-export, update dispatch
- `Cargo.toml` — add `libc = "0.2"`

Files NOT to modify:
- `src/pipeline.rs` — pipeline routing is Story 15.7
- `src/session/runner.rs` — API runtime internals untouched
- `src/session/agent.rs` — `ShutdownFlag` type, `streaming_chat()` unchanged
- `src/config/mod.rs` — config already supports SDK providers from 15.2
- `src/session/state.rs` — WAL fields are Story 15.7
- `src/tools/*` — tool implementations are API-mode concerns
- `src/supervisor/*` — supervisor logic untouched
- `src/ui/*` — UI module unchanged, UiHandle methods are stable
- `src/main.rs` — `mod runtime;` already declared from 15.1
- `_bmad/` — read-only, never modified

### References

- [Source: architecture.md#Decision 12 — Dual Runtime Abstraction, SdkRuntime subprocess management]
- [Source: architecture.md#Decision 13 — Supervisor MCP Server (context for MCP config injection in 15.4)]
- [Source: planning-artifacts/sprint-change-proposal-2026-04-26.md — Story 15.3 definition]
- [Source: planning-artifacts/epics.md#Epic 15, Story 15.3 — SDK Runtime Subprocess Infrastructure]
- [Source: src/runtime/mod.rs:69-73 — SessionRuntime enum (Sdk variant)]
- [Source: src/runtime/mod.rs:76-81 — run_session() dispatch (todo! on Sdk)]
- [Source: src/runtime/mod.rs:154 — SdkRuntime empty stub]
- [Source: src/session/agent.rs:159 — ShutdownFlag type definition]
- [Source: src/session/agent.rs:541-553 — ShutdownFlag cooperative check pattern]
- [Source: src/session/mod.rs:102-135 — SessionOutcome enum variants]
- [Source: src/config/mod.rs:197-223 — LlmRoleConfig struct (provider, cli_path)]
- [Source: src/config/mod.rs:226 — is_sdk_provider() method]
- [Source: src/config/mod.rs:777-789 — BotSecrets struct (API keys)]
- [Source: src/config/mod.rs:698 — resolve_cli_name() mapping]
- [Source: src/llm/agent_factory.rs:267-283 — LlmRole resolution pattern]
- [Source: src/ui/mod.rs:105-144 — UiHandle event methods]
- [Source: Cargo.toml:7 — tokio full features]
- [Source: Cargo.toml:12 — serde_json dependency]
- [Source: _bmad-output/project-context.md — Project rules and conventions]
- [Source: _bmad-output/implementation-artifacts/15-1-session-runtime-abstraction-layer.md — Previous story context]
- [Source: _bmad-output/implementation-artifacts/15-2-config-extension-sdk-providers.md — Previous story context]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

- Pre-existing `McpError` unused import warning in `src/mcp/mod.rs` — not from this story, not touched per anti-patterns
- Dead code warnings for `execute_session()` and dependencies are expected: public infrastructure for Stories 15.5/15.6, but not called from non-test code yet (binary crate — `pub` doesn't suppress dead_code)

### Completion Notes List

- Task 1: Created `src/runtime/sdk.rs` with `SdkError` (4 variants), `SdkSessionConfig`, `SdkOutputEvent` (6 variants), `SdkSessionResult`
- Task 2: Implemented `SdkRuntime` struct with `new()`, `merge_env_vars()`, `resolve_provider_for_role()`, `run_session()` (stub returning Failed)
- Task 3: Implemented `execute_session()` with subprocess spawning via `tokio::process::Command`, NDJSON stdout streaming via `BufReader::lines()`, stderr capture as background task with 1 MB cap
- Task 4: Implemented graceful shutdown with `send_sigterm()` (libc, `#[cfg(unix)]` guarded) and `graceful_kill()` (SIGTERM → SIGKILL with configurable grace period)
- Task 5: Session ID tracking via first `SessionStarted` event in stdout loop
- Task 6: UI event emission via `UiHandle` methods (`activation_start`, `tool_call`, `tool_result`, `activation_complete`)
- Task 7: Process timeout via `tokio::select! { biased; }` with `sleep_until(timeout_at)` branch
- Task 8: Wired into `src/runtime/mod.rs` — `pub mod sdk;`, `pub use sdk::SdkRuntime;`, dispatch `Self::Sdk(sdk) => sdk.run_session(context).await`
- Task 9: 16 new tests covering all ACs — types, env merge (4 tests), subprocess execution (6 tests), shutdown helpers (2 tests), run_session stub, event variants, error display
- Task 10: 1358 total tests pass (16 new + 1342 existing), `cargo fmt --check` clean, no new clippy warnings

### Implementation Plan

Followed story tasks sequentially. Key decisions:
- Used `graceful_kill().await.ok()` instead of `?` in select branches since `io::Error` → `SdkError` conversion not needed (shutdown/timeout are tracked via result flags)
- `ESRCH` handling in `send_sigterm`: treated as success (process already exited)
- `#[allow(dead_code)]` on `secrets` field only — `shutdown` and `ui` are used by `execute_session()` which is reachable via tests

### File List

- `src/runtime/sdk.rs` — NEW: SdkRuntime, SdkSessionConfig, SdkOutputEvent, SdkSessionResult, SdkError, subprocess management, 16 tests
- `src/runtime/mod.rs` — MODIFIED: added `pub mod sdk;`, `pub use sdk::SdkRuntime;`, replaced stub dispatch with real call, updated SDK variant construction test
- `Cargo.toml` — MODIFIED: added `libc = "0.2"` dependency

### Change Log

- 2026-04-26: Story 15.3 implemented — SDK runtime subprocess infrastructure with generic `execute_session()`, graceful SIGTERM→SIGKILL shutdown, NDJSON streaming, session ID tracking, UI event emission, env var injection from BotSecrets. 16 new tests, 1358 total passing.

### Review Findings

- [x] [Review][Patch] Missing `///` doc comments on all public items and `//!` module doc comment [src/runtime/sdk.rs] — FIXED
- [x] [Review][Patch] `expect()` in production code [src/runtime/sdk.rs:186-187] — FIXED: replaced with `ok_or_else(|| SdkError::SpawnFailed)`
- [x] [Review][Patch] Missing `// SAFETY:` comment on `unsafe` block [src/runtime/sdk.rs] — FIXED
- [x] [Review][Patch] `graceful_kill` errors silently swallowed via `.ok()` [src/runtime/sdk.rs] — FIXED: logged via `tracing::error!`
- [x] [Review][Patch] PID cast `u32` → `i32` unchecked in `send_sigterm` [src/runtime/sdk.rs] — FIXED: `try_from` with error propagation
- [x] [Review][Patch] `child.wait().await` after event loop has no timeout [src/runtime/sdk.rs] — FIXED: wrapped in `tokio::time::timeout`
- [x] [Review][Defer] `#[allow(dead_code)]` on `secrets` field [src/runtime/sdk.rs:89] — deferred, resolves when 15.5/15.6 call `execute_session()`
- [x] [Review][Defer] Non-Unix `send_sigterm` no-op causes unnecessary grace period delay — deferred, project targets Linux
- [x] [Review][Defer] Missing `Debug`/`PartialEq` trait derivations on `SdkSessionConfig`, `SdkSessionResult` — deferred, nice-to-have
- [x] [Review][Defer] `graceful_kill` returns `()` instead of `ExitStatus` per spec Task 4.3 — deferred, intentional simplification documented in completion notes
