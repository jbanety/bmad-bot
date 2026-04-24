# Story 13.7: Config Init — Project Brief

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer setting up BMAD Bot,
I want to provide a project brief file path during `bmad-bot init`,
So that the Story Critic has a vision anchor independent from BMAD artifacts.

## Acceptance Criteria

1. **AC-1: New prompt in `bmad-bot init` interactive flow**
   - **Given** the `bmad-bot init` interactive flow in `src/cli/mod.rs`
   - **When** this story is implemented
   - **Then** a new "── Story Critic ──" section is added after the Daemon Settings section (after log level prompt, before BMAD paths derivation) with prompt: "Path to project brief file (optional, press Enter to skip)"
   - **And** if a relative path is provided, the file existence is checked with a non-fatal warning if the file doesn't exist yet (the user may create it later)
   - **And** if an absolute path is provided, it is rejected with a message: paths must be relative to the project root
   - **And** the path is stored in `bmad-bot.yaml` as `project_brief: "{path}"` (relative to project root)
   - **And** if skipped (empty input), no `project_brief` field is written (serde `skip_serializing_if = "Option::is_none"`)

2. **AC-2: New optional field in `BotConfig` struct**
   - **Given** the `BotConfig` struct in `src/config/mod.rs`
   - **When** this story is implemented
   - **Then** a new optional field `project_brief: Option<String>` is added with `#[serde(default, skip_serializing_if = "Option::is_none")]`
   - **And** deserialization succeeds with or without the field present in YAML

3. **AC-3: Startup validation (non-fatal warning)**
   - **Given** a `project_brief` path is configured in `bmad-bot.yaml`
   - **When** the daemon starts (after tracing is initialized)
   - **Then** the path is resolved relative to `bmad_paths.project_root`
   - **And** if the resolved file does not exist, a `tracing::warn!(path = %resolved, "Project brief file not found — Critic will use PRD as fallback")` is emitted
   - **And** the daemon continues normally — this is NOT a fatal validation error
   - **And** if the file does exist, a `tracing::info!(path = %resolved, "Project brief file found")` confirms it

4. **AC-4: Example config updated**
   - **Given** the `bmad-bot.yaml.example` file
   - **When** this story is implemented
   - **Then** a commented `project_brief` field is added with explanatory comments about its purpose

5. **AC-5: Tests**
   - **Given** the config changes
   - **When** this story is implemented
   - **Then** config deserialization works with and without `project_brief` field
   - **And** validation passes with `project_brief: None`
   - **And** validation passes with `project_brief: Some("valid-path.md")`
   - **And** `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` passes
   - **And** `cargo test` passes with no new failures beyond pre-existing `test_build_context_limit_recovery_message_contains_all_sections`

## Tasks / Subtasks

- [x] Task 1: Add `project_brief` field to `BotConfig` (AC: #2)
  - [x] 1.1 Add optional field to `BotConfig` struct at `src/config/mod.rs` after `code_review_enabled` (line ~120) and before `mcp_servers` (line ~124). This matches the field ordering in `bmad-bot.yaml.example` where `project_brief` appears after `code_review_enabled` and before `bmad_paths`:
    ```rust
    /// Optional path to a project brief file for the Story Critic's vision anchor.
    /// Relative to `bmad_paths.project_root`. When absent, the Critic falls back
    /// to the PRD as its vision context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_brief: Option<String>,
    ```
  - [x] 1.2 Update `valid_config()` test helper — `config_from_str(VALID_YAML)` already handles missing optional fields via `serde(default)`, so no VALID_YAML change needed. Verify by running tests.

- [x] Task 2: Add startup check for `project_brief` (AC: #3)
  - [x] 2.1 Add a `check_project_brief()` method to `BotConfig` at `src/config/mod.rs` (NOT inside `validate()` — this is a non-fatal warning, not a validation error). The path is resolved relative to `bmad_paths.project_root`:
    ```rust
    /// Log a warning if `project_brief` is configured but the file doesn't exist.
    /// Resolves path relative to `bmad_paths.project_root`. Non-fatal — the Critic
    /// falls back to PRD.
    pub fn check_project_brief(&self) {
        if let Some(ref path) = self.project_brief {
            let resolved = std::path::Path::new(&self.bmad_paths.project_root).join(path);
            if resolved.exists() {
                tracing::info!(
                    path = %resolved.display(),
                    "Project brief file found"
                );
            } else {
                tracing::warn!(
                    path = %resolved.display(),
                    "Project brief file not found — Critic will use PRD as fallback"
                );
            }
        }
    }
    ```
  - [x] 2.2 Call `config.check_project_brief()` in `src/cli/mod.rs` `run_start()` — **AFTER `init_tracing()` (line 1216)**, not after `config.validate()` (line 1209). Tracing must be initialized before emitting `tracing::warn!`/`tracing::info!`. Place the call after `init_tracing()` returns, e.g. around line 1228 (before `validate_git_version()`).

- [x] Task 3: Add project brief prompt in `bmad-bot init` (AC: #1)
  - [x] 3.1 In `src/cli/mod.rs` `collect_config_interactively()`, add a new section after the log level prompt (line 647) and before the BMAD paths derivation (line 649). Follow the existing `dialoguer::Input` pattern used for base_url prompts (lines 486-495) — note: do NOT use `.show_default(false)` since existing optional prompts don't use it:
    ```rust
    println!("\n\u{2500}\u{2500} Story Critic \u{2500}\u{2500}");
    let project_brief_raw: String = dialoguer::Input::new()
        .with_prompt("Path to project brief file (optional, press Enter to skip)")
        .default(String::new())
        .interact_text()
        .map_err(|e| CliError::Init {
            reason: e.to_string(),
        })?;
    let project_brief = {
        let trimmed = project_brief_raw.trim();
        if trimmed.is_empty() {
            None
        } else if std::path::Path::new(trimmed).is_absolute() {
            println!("  \u{26a0} Path must be relative to the project root — absolute paths are not accepted.");
            None
        } else {
            let path = trimmed.to_string();
            if !std::path::Path::new(&path).exists() {
                println!("  \u{26a0} File not found at '{path}' — you can create it later. The Critic will use PRD as fallback until then.");
            }
            Some(path)
        }
    };
    ```
  - [x] 3.2 Add the `project_brief` field to the `BotConfig` construction at line 655-703:
    ```rust
    Ok(BotConfig {
        polling_interval_secs,
        code_review_enabled: true,
        project_brief,  // <-- add after code_review_enabled
        git_provider: GitProviderConfig { ... },
        ...
    })
    ```

- [x] Task 4: Update `bmad-bot.yaml.example` (AC: #4)
  - [x] 4.1 Add commented `project_brief` field after `code_review_enabled` and before `bmad_paths`:
    ```yaml
    # Project brief file for the Story Critic's vision anchor (optional).
    # The Critic uses this to evaluate stories against the original project vision.
    # If absent, the Critic falls back to the PRD.
    # project_brief: "PROJECT_BRIEF.md"
    ```

- [x] Task 5: Add tests (AC: #5)
  - [x] 5.1 Add `test_config_project_brief_none_by_default` — deserialize `VALID_YAML` (no `project_brief` field), assert `config.project_brief.is_none()`, assert `validate()` passes
  - [x] 5.2 Add `test_config_project_brief_some_accepted` — add `project_brief: "brief.md"` to YAML, deserialize, assert `config.project_brief == Some("brief.md".to_string())`, assert `validate()` passes
  - [x] 5.3 Add `test_config_project_brief_not_serialized_when_none` — create a config with `project_brief: None`, serialize to YAML, assert the output does NOT contain the string "project_brief"
  - [x] 5.4 Add `test_check_project_brief_existing_file` — create a temp file, set `config.project_brief = Some(temp_file_path)` and `bmad_paths.project_root` to temp dir, call `check_project_brief()`, assert no panic. (Tracing output is not asserted — just verifies the method executes without error on an existing file.)
  - [x] 5.5 Add `test_check_project_brief_missing_file` — set `config.project_brief = Some("does-not-exist.md")`, call `check_project_brief()`, assert no panic. (Verifies the method handles missing files gracefully.)
  - [x] 5.6 Add `test_check_project_brief_none_is_noop` — set `config.project_brief = None`, call `check_project_brief()`, assert no panic.
  - [x] 5.7 Run `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` — zero new warnings
  - [x] 5.8 Run `cargo test` — all pass, no new failures beyond pre-existing `test_build_context_limit_recovery_message_contains_all_sections`

## Dev Notes

### Architecture Compliance

- **Decision 11 (Story Critic with Persistent Memory):** This story implements the config foundation for the Critic's "founding context." The architecture specifies: "A project brief file provided at `bmad-bot init` (stored in config as `project_brief`). Falls back to PRD if no brief exists." [Source: architecture.md:706]
- **Config pattern:** The `project_brief` field follows the same `Option<String>` + `serde(default, skip_serializing_if)` pattern used by `reasoning_effort` and `base_url` in `LlmRoleConfig` (lines 193-201 of config/mod.rs).
- **Init flow pattern:** The new prompt follows the same `dialoguer::Input::new()` pattern used throughout `collect_config_interactively()`. The prompt section header `"── Story Critic ──"` follows the `"── Daemon Settings ──"` convention (line 612).

### Critical Implementation Details

**This story is config-only — no pipeline or session changes.** The `project_brief` path is stored in `BotConfig` but not yet consumed by any module. Story 13.9 (Critic Agent — Prompt Engineering & Construction) will read `config.project_brief` to load the brief into the Critic's context via `ContextBuilder`. This story establishes the config plumbing; 13.9 wires the consumption.

**Non-fatal validation is a separate method, not inside `validate()`.** The `validate()` method returns `Result<(), ConfigError>` and halts startup on errors. The project brief check is a warning — the daemon should start even if the file is missing. Use a separate `check_project_brief()` method called after `init_tracing()`.

**`check_project_brief()` must be called AFTER `init_tracing()`.** In `run_start()`, `config.validate()` is at line 1209 but `init_tracing()` is at line 1216. Since `check_project_brief()` uses `tracing::warn!`/`tracing::info!`, it must be placed after tracing is initialized (around line 1228), NOT right after `validate()`. Placing it before `init_tracing()` would silently swallow the log messages.

**Path resolution: `check_project_brief()` resolves against `bmad_paths.project_root`.** The stored path is relative. `check_project_brief()` must join it with `self.bmad_paths.project_root` before checking existence. Using `Path::new(raw_path).exists()` would resolve against CWD, which may differ from the project root in deployment.

**Absolute paths are rejected at init time.** `std::path::Path::is_absolute()` guards against paths like `/etc/passwd` which would bypass project root scoping. `Path::join()` with an absolute path ignores the base entirely, so the Critic could read arbitrary files. Rejecting absolute paths at the init prompt prevents this.

**File existence check at init is informational, not blocking.** During `bmad-bot init`, if the user provides a relative path to a file that doesn't exist yet, print a warning but accept it. The user may create the file later. The real check happens at daemon startup via `check_project_brief()`.

**The `project_brief` field in YAML is a plain relative string path.** No canonicalization — store the trimmed user input. The consuming code (Story 13.9) and `check_project_brief()` both resolve it relative to `bmad_paths.project_root`.

**Serde serialization: `skip_serializing_if = "Option::is_none"`** ensures that when `project_brief` is `None`, the field is completely absent from the generated YAML (not `project_brief: ~` or `project_brief: null`). This keeps the generated config clean.

**Serde serialization order follows struct field order.** `serde_yml::to_string()` serializes fields in declaration order. The `project_brief` field is placed after `code_review_enabled` in the struct to match the ordering in `bmad-bot.yaml.example`.

### Previous Story Intelligence (Story 13.6)

- **Baseline test count:** 1187 passing, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)
- **Pre-existing clippy allowances:** `-A clippy::needless_splitn -A clippy::unnecessary_map_or`
- **Story 13.6 modified:** `src/pipeline.rs`, `src/session/runner.rs`, `src/session/agent.rs`, `src/review/mod.rs` — none of these files are touched by Story 13.7
- **Story 13.6 established:** consultation wiring with placeholder critic preambles (`build_placeholder_critic_preamble()` at pipeline.rs:2973, `build_review_critic_preamble()` at pipeline.rs:2954) — these preambles will be upgraded in Story 13.9 to reference `config.project_brief`

### Git Intelligence — Recent Commits

```
21761e0 feat(epic-13): unify code-review phase via SessionRunner with critic consultation (Story 13.6)
b68fc0d feat(epic-13): separate dev phase from review phase in pipeline (Story 13.5)
5f4a497 feat(epic-13): implement create-story phase with consultations (Story 13.4)
63932ed feat(epic-13): add daemon-orchestrated consultation mechanism (Story 13.3)
147f57d feat(epic-13): refactor pipeline into status-based phase router (Story 13.2)
```

Files most recently modified in Epic 13:
- `src/pipeline.rs` — heavy refactoring in 13.2-13.6. NOT touched by this story.
- `src/session/runner.rs` — parameterized with LlmRole in 13.6. NOT touched by this story.
- `src/config/mod.rs` — last modified in Epic 11 (provider cleanup). Touched by this story.
- `src/cli/mod.rs` — last modified in Epic 11 (removed Copilot from provider list). Touched by this story.

### Project Structure Notes

- `src/config/mod.rs` modified — `BotConfig` struct gains `project_brief` field, `check_project_brief()` method added
- `src/cli/mod.rs` modified — `collect_config_interactively()` gains new prompt section, `BotConfig` construction updated
- `bmad-bot.yaml.example` modified — commented `project_brief` example added
- No new files created
- No module dependencies added

### References

- [Source: _bmad-output/planning-artifacts/epics.md:3317-3340 — Story 13.7 AC (Config Init — Project Brief)]
- [Source: _bmad-output/planning-artifacts/architecture.md:704-716 — Decision 11 (Story Critic with Persistent Memory)]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md:110,129,268 — Config updates, CLI changes, Story 13.7 spec]
- [Source: _bmad-output/project-context.md — CLI Rules, Config validation, Testing rules]
- [Source: src/config/mod.rs:73-126 — BotConfig struct (add project_brief field)]
- [Source: src/config/mod.rs:324-412 — validate() method (DO NOT add project_brief check here)]
- [Source: src/config/mod.rs:180-202 — LlmRoleConfig Option pattern (reference for serde attributes)]
- [Source: src/config/mod.rs:677-707 — VALID_YAML test fixture (project_brief absent = None via serde default)]
- [Source: src/config/mod.rs:893-895 — valid_config() test helper]
- [Source: src/cli/mod.rs:416-704 — collect_config_interactively() (add prompt after log level)]
- [Source: src/cli/mod.rs:486-495 — base_url optional prompt pattern (reference — no show_default(false))]
- [Source: src/cli/mod.rs:612-648 — Daemon Settings section (reference for prompt pattern)]
- [Source: src/cli/mod.rs:655-703 — BotConfig construction (add project_brief field)]
- [Source: src/cli/mod.rs:1207-1246 — run_start() startup sequence: validate() at 1209, init_tracing() at 1216, place check_project_brief() after 1216]
- [Source: bmad-bot.yaml.example — Add commented project_brief field]
- [Source: _bmad-output/implementation-artifacts/13-6-code-review-phase-with-critic.md — Previous story intelligence]

### Existing Code to Reuse

- `dialoguer::Input::new()` pattern — used extensively in `collect_config_interactively()` for string prompts
- `#[serde(default, skip_serializing_if = "Option::is_none")]` — used on `LlmRoleConfig.reasoning_effort` (line 193) and `LlmRoleConfig.base_url` (line 199)
- `CliError::Init` — error type for init failures (used at every prompt in the init flow)
- `tracing::info!` / `tracing::warn!` — structured logging with fields (used throughout the codebase)

### Anti-Patterns to Avoid

- **DO NOT** add `project_brief` validation inside `validate()` — the project brief check is non-fatal (warning, not error). Use a separate method. The `validate()` method returns `Err` which halts daemon startup.
- **DO NOT** canonicalize the path — store the trimmed user input as-is. Both `check_project_brief()` and the consuming code (Story 13.9) resolve it relative to `bmad_paths.project_root`.
- **DO NOT** accept absolute paths at init — `Path::is_absolute()` must reject them. Absolute paths bypass `project_root` scoping in `Path::join()`.
- **DO NOT** modify `pipeline.rs` or any other modules — this story is config + CLI only. The brief is consumed in Story 13.9.
- **DO NOT** add `project_brief` to `.env` or `generate_env_file()` — the project brief is a file path, not a secret.
- **DO NOT** add `LlmRole::Critic` — that is Story 13.9. This story only adds the config field.
- **DO NOT** add `critic-memory.md` handling — that is Story 13.8.
- **DO NOT** use `println!` for the startup check — use `tracing::warn!` / `tracing::info!`. Exception: `println!` is allowed in the interactive init flow (CLI context, not daemon).
- **DO NOT** make the prompt blocking if the file doesn't exist — print a warning and accept the path.
- **DO NOT** add a `critic` LLM role to `LlmConfig` — that is Story 13.9. This story only adds `project_brief` to `BotConfig`.
- **DO NOT** use `.show_default(false)` on the dialoguer prompt — existing optional prompts (base_url at lines 486-495) don't use it. Stay consistent.
- **DO NOT** call `check_project_brief()` before `init_tracing()` — tracing messages would be silently dropped.
- **DO** ensure `skip_serializing_if = "Option::is_none"` is set so generated YAML stays clean.
- **DO** follow the existing prompt flow pattern (dialoguer, CliError wrapping, section headers).
- **DO** place the `project_brief` field after `code_review_enabled` in the struct — serde serializes in declaration order, and this matches the `bmad-bot.yaml.example` layout.

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (claude-opus-4-6)

### Debug Log References

### Completion Notes List

- Added `project_brief: Option<String>` field to `BotConfig` with `serde(default, skip_serializing_if)` — field placed after `code_review_enabled` to match YAML example ordering
- Implemented `check_project_brief()` method on `BotConfig` — resolves path relative to `bmad_paths.project_root`, emits `tracing::info!`/`tracing::warn!` (non-fatal)
- Called `check_project_brief()` in `run_start()` after `init_tracing()` — ensures tracing is initialized before log emission
- Added "── Story Critic ──" section in `collect_config_interactively()` with `dialoguer::Input` prompt — rejects absolute paths, warns on missing files, stores relative path
- Updated `bmad-bot.yaml.example` with commented `project_brief` field and explanatory comments
- Added 6 unit tests covering: None by default, Some accepted, not serialized when None, existing file check, missing file check, None is noop
- Updated all 6 `make_test_config()` helpers across codebase with `project_brief: None`
- Test results: 1193 passed, 1 failed (pre-existing `test_build_context_limit_recovery_message_contains_all_sections`)
- Clippy: zero new warnings (all errors are pre-existing dead code/unused items from future story scaffolding)

### File List

- src/config/mod.rs (modified — added `project_brief` field, `check_project_brief()` method, 6 tests, updated `_test_minimal()`)
- src/cli/mod.rs (modified — added Story Critic prompt section, `project_brief` in BotConfig construction, `check_project_brief()` call in `run_start()`, updated test helper)
- bmad-bot.yaml.example (modified — added commented `project_brief` field)
- src/watcher/mod.rs (modified — added `project_brief: None` to test config)
- src/llm/agent_factory.rs (modified — added `project_brief: None` to test config)
- src/review/epic.rs (modified — added `project_brief: None` to test config)
- src/pipeline.rs (modified — added `project_brief: None` to test config)
- src/session/runner.rs (modified — added `project_brief: None` to test config)

### Change Log

- 2026-04-24: Story 13.7 implementation — config init project brief. Added `project_brief` optional field to `BotConfig`, startup validation via `check_project_brief()`, interactive init prompt with absolute path rejection, and 6 unit tests.

### Review Findings

- [x] [Review][Decision] Path traversal with `../` components not rejected — Fixed: added `..` guard in CLI (with re-prompt) and `check_project_brief()`. [src/cli/mod.rs, src/config/mod.rs]
- [x] [Review][Decision] Absolute path rejection silently discards input without re-prompt — Fixed: CLI now loops with `continue` on rejected paths. [src/cli/mod.rs]
- [x] [Review][Patch] CLI file existence check uses CWD instead of project_root — Fixed: now resolves against `project_root`. [src/cli/mod.rs]
- [x] [Review][Patch] Empty string `project_brief` in YAML triggers false positive — Fixed: added empty-string guard in `check_project_brief()`. [src/config/mod.rs]
- [x] [Review][Patch] Absolute paths in YAML bypass CLI guard — Fixed: added absolute-path guard in `check_project_brief()`. [src/config/mod.rs]
- [x] [Review][Defer] Config nesting for critic settings — `project_brief` is a top-level field. As Stories 13.8/13.9 add more critic settings, this will become an organizational orphan. Consider a `[critic]` config section. — deferred, architectural concern for future stories
