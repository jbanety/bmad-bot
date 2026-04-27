# Story 15.8: Init Command SDK Provider Setup

Status: done

## Story

As a new user,
I want `bmad-bot init` to guide me through SDK provider setup when I choose `claude-code` or `codex`,
so that configuration is correct and validated before first run.

## Acceptance Criteria

1. **Given** the user runs `bmad-bot init`, **When** available CLIs are detected (`claude --version`, `codex --version`), **Then** they are offered as provider options alongside `anthropic` and `openai` in the interactive selection (note: the codebase uses `"openai"` not `"openai-compatible"` — see Story 15.2 dev notes on this pre-existing naming discrepancy)

2. **Given** an SDK provider is selected for any role, **When** configuration prompts are shown, **Then** the user is prompted for optional `cli_path` instead of `base_url`, and model suggestions are appropriate for the provider (`claude-sonnet-4-6` for claude-code, `o4-mini` for codex)

3. **Given** an SDK provider is selected, **When** configuration is generated, **Then** init performs best-effort validation: CLI availability (found and exits zero via `--version`) and BMAD skill presence (`.claude/skills/bmad-*/SKILL.md` or `.agents/skills/bmad-*/SKILL.md`). Validation failures produce warnings, not hard errors — the user may install after generating config

4. **Given** SDK providers are selected, **When** the `.env` file is generated, **Then** it includes relevant API keys: `ANTHROPIC_API_KEY` when any role uses `anthropic` or `claude-code`, `OPENAI_API_KEY` when any role uses `openai` or `codex`

5. **Given** all provider types are valid for all roles, **When** provider selection is shown, **Then** no artificial restrictions prevent any provider from being used for dev, review, or supervisor roles

6. **Given** the user selects "Use same provider/model for all roles", **When** an SDK provider is selected, **Then** the `cli_path` value is also propagated to review and supervisor roles

7. **Given** `codex` is selected as a provider for any role, **When** setup completes, **Then** the init command prints a reminder: `"Note: Run 'codex trust .' in your project root to allow Codex MCP server access."`

8. **Given** all three core roles (dev, review, supervisor) are configured with SDK providers, **When** configuration is generated, **Then** init prints a warning that daemon-orchestrated consultations require at least one API provider, and will fall back to the supervisor role's config or skip consultations if no API provider is available

## Tasks / Subtasks

- [x] Task 1: Extend interactive provider selection with SDK providers (AC: #1, #5)
  - [x] 1.1 Update `LLM_PROVIDERS` constant at `src/cli/mod.rs:138` from `["anthropic", "openai"]` to `["anthropic", "openai", "claude-code", "codex"]`. This is the **full superset** — the interactive prompt uses a runtime-filtered copy (see 1.4), not this constant directly
  - [x] 1.2 Add CLI detection helper `detect_sdk_cli(cli_name: &str) -> Option<String>` that runs `{cli_name} --version` via `std::process::Command`, returns `Some(version_string)` on success or `None` on failure. The helper maps CLI names inline: `"claude"` for claude-code, `"codex"` for codex — do NOT call `resolve_cli_name()` from `config/mod.rs` since it is private (`fn`, not `pub`). Parse version: strip ` (Claude Code)` suffix from `claude --version` output; use `codex --version` output directly
  - [x] 1.3 In `collect_config_interactively()`, before the LLM provider selection prompt, call `detect_sdk_cli("claude")` and `detect_sdk_cli("codex")`. Display results: `"  Detected: claude (v2.1.119), codex (v0.125.0)"` or `"  No SDK CLIs detected — only API providers available"` when none found
  - [x] 1.4 Build a **runtime-filtered** `Vec<&str>` from `LLM_PROVIDERS`: always include `"anthropic"` and `"openai"`, only include `"claude-code"` if `detect_sdk_cli("claude")` returned `Some`, only include `"codex"` if `detect_sdk_cli("codex")` returned `Some`. Pass this filtered vec to `dialoguer::Select::items()` instead of the full constant. This avoids confusing selections that would fail at `run_start()` validation

- [x] Task 2: Update `default_model_for_provider()` for SDK providers (AC: #2)
  - [x] 2.1 Extend `default_model_for_provider()` at `src/cli/mod.rs:150-156`: add `"claude-code" => "claude-sonnet-4-6"` and `"codex" => "o4-mini"`
  - [x] 2.2 This function is used as the default value for the model input prompt

- [x] Task 3: Adapt provider-specific prompts for SDK mode (AC: #2, #6)
  - [x] 3.1 Add helper function `prompt_provider_options(provider: &str) -> (Option<String>, Option<String>)` that returns `(base_url, cli_path)` based on provider type. For API providers (`"anthropic"`, `"openai"`): prompt for `base_url` (existing logic from lines 496-509), return `(base_url, None)`. For SDK providers (`"claude-code"`, `"codex"`): prompt for `cli_path` with `"Custom path to {cli_name} CLI (optional, Enter for default)"` where `cli_name` is `"claude"` for `"claude-code"` or `"codex"` for `"codex"` (inline match, not calling private `resolve_cli_name()`), return `(None, cli_path)`
  - [x] 3.2 Parse `cli_path` input: use the same pattern as `parse_base_url_input()` — trim, return `None` if empty, `Some(trimmed)` otherwise
  - [x] 3.3 The `same_for_all` branch (lines 511-599) currently destructures into a 6-element tuple: `(review_provider, review_model, review_base_url, supervisor_provider, supervisor_model, supervisor_base_url)`. Extend to 8 elements by adding `review_cli_path` and `supervisor_cli_path`. When `same_for_all == true`, clone `dev_cli_path` into both. When `same_for_all == false`, each per-role prompt block calls `prompt_provider_options()` to get the appropriate option
  - [x] 3.4 The per-role prompt block (lines 536-598) for `same_for_all == false` must use the same runtime-filtered provider list from Task 1.4, NOT the full `LLM_PROVIDERS` constant

- [x] Task 4: Wire `cli_path` into BotConfig construction (AC: #2)
  - [x] 4.1 In the `BotConfig` construction at `src/cli/mod.rs:700-755`, set `cli_path` from the collected values for dev, review, and supervisor `LlmRoleConfig`
  - [x] 4.2 The existing `Ok(BotConfig { ... })` block already includes `cli_path: None` for all roles — replace with the collected values

- [x] Task 5: Validate SDK setup at init time and emit post-setup warnings (AC: #3, #7, #8)
  - [x] 5.1 After `config.validate()` at line 279, if any role uses an SDK provider, call `config.validate_sdk_providers()`. Note: this is the SAME function called by `run_start()` — it uses `validate_cli_availability(provider, role_name, cli_path)` internally, which will produce `ConfigError` with role context. In the init flow, catch the error and present it as a warning, not a hard failure
  - [x] 5.2 On validation failure (CLI not found, skills missing), print a warning but do NOT abort init — the user may install the CLI or skills after generating the config. Print: `"Warning: {error_message}. Fix this before running 'bmad-bot start'."`
  - [x] 5.3 On validation success, print: `"SDK provider validated: {provider}"`
  - [x] 5.4 If any role uses `"codex"`, print the trust reminder: `"Note: Run 'codex trust .' in your project root to allow Codex MCP server access."`
  - [x] 5.5 If ALL three core roles (dev, review, supervisor) use SDK providers (check via `is_sdk_provider()`), print: `"Warning: All core roles use SDK providers. Daemon-orchestrated consultations (adversarial review, critic) require at least one API provider. Consultations will be skipped if no API fallback is available."`

- [x] Task 6: Update `.env` file generation for SDK providers (AC: #4)
  - [x] 6.1 In `generate_env_file()` at `src/cli/mod.rs:793`, the existing `provider_roles` map iterates only `dev`, `review`, `supervisor` (lines 804-813). Keep this scope — `epic_review` and `critic` default to empty provider in the init flow and inherit from `review` at runtime. No change to the iteration scope needed
  - [x] 6.2 In the loop body, normalize SDK providers to their API key name before inserting into `provider_roles`: `"claude-code"` → insert under key `"anthropic"`, `"codex"` → insert under key `"openai"`. This ensures natural deduplication — both `anthropic` and `claude-code` roles accumulate under the `"anthropic"` key
  - [x] 6.3 When emitting the `ANTHROPIC_API_KEY` line, if roles include both API and SDK consumers, the comment should reflect both: `"# Required: used by dev (claude-code), supervisor (anthropic)"`. Build role descriptions as `"{role_name} ({provider})"` instead of just `"{role_name}"`
  - [x] 6.4 Same pattern for `OPENAI_API_KEY` — deduplicated via the normalized key, comment includes all consuming roles with their actual provider names

- [x] Task 7: Write unit tests (AC: #1-8)
  - [x] 7.1 `test_default_model_for_provider_claude_code` — returns `"claude-sonnet-4-6"`
  - [x] 7.2 `test_default_model_for_provider_codex` — returns `"o4-mini"`
  - [x] 7.3 `test_generate_env_sdk_provider_claude_code` — config with `claude-code` dev includes `ANTHROPIC_API_KEY=`
  - [x] 7.4 `test_generate_env_sdk_provider_codex` — config with `codex` dev includes `OPENAI_API_KEY=`
  - [x] 7.5 `test_generate_env_mixed_anthropic_and_claude_code_dedup` — dev=`claude-code`, supervisor=`anthropic` → single `ANTHROPIC_API_KEY` line with combined role comment
  - [x] 7.6 `test_generate_env_mixed_openai_and_codex_dedup` — dev=`codex`, supervisor=`openai` → single `OPENAI_API_KEY` line with combined role comment
  - [x] 7.7 `test_generate_env_all_claude_code_no_openai_key` — all roles `claude-code` → only `ANTHROPIC_API_KEY`, no `OPENAI_API_KEY`
  - [x] 7.8 `test_generate_env_all_codex_no_anthropic_key` — all roles `codex` → only `OPENAI_API_KEY`, no `ANTHROPIC_API_KEY`
  - [x] 7.9 `test_generate_config_yaml_roundtrips_with_sdk_provider` — config with `claude-code` and `cli_path` serializes and deserializes correctly
  - [x] 7.10 `test_generate_config_yaml_sdk_validates` — roundtripped SDK config passes `validate()`
  - [x] 7.11 `test_generate_env_role_comments_include_provider_name` — verify role comment format is `"dev (claude-code)"` not just `"dev"`
  - [x] 7.12 Verify all existing tests still pass — zero regressions

- [x] Task 8: Verify full test suite
  - [x] 8.1 Run `cargo clippy -- -D warnings` — zero new warnings
  - [x] 8.2 Run `cargo test` — all tests pass
  - [x] 8.3 Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Critical Architecture Context

**Decision 12 (Dual Runtime Abstraction):** Story 15.1 introduced `SessionRuntime` enum with `Api` and `Sdk` variants. Story 15.2 extended the config to accept `claude-code` and `codex` as valid provider types. This story (15.8) extends the `init` command to support these providers interactively.

**Decision 13 (Supervisor MCP Server):** Story 15.4 implemented the MCP server. SDK sessions consume it via `--mcp-config`. The init command does NOT need to generate MCP config — that's generated dynamically at runtime by `sdk_claude.rs` and `sdk_codex.rs`.

### Current Init Command Structure (What to Change)

**File:** `src/cli/mod.rs`

| Location | Current State | Change Required |
|----------|---------------|-----------------|
| Line 138: `LLM_PROVIDERS` | `["anthropic", "openai"]` | Add `"claude-code"`, `"codex"` as full superset; prompt uses runtime-filtered copy |
| Lines 150-156: `default_model_for_provider()` | Only `anthropic`/`openai` | Add `claude-code` → `"claude-sonnet-4-6"`, `codex` → `"o4-mini"` |
| Lines 478-509: DEV provider prompts | Provider select → model → base_url | Provider select → model → (cli_path OR base_url based on provider type) |
| Lines 511-599: same_for_all / per-role | 6-element tuple: provider/model/base_url x2 | Expand to 8-element tuple adding cli_path x2 |
| Lines 700-755: BotConfig construction | All `cli_path: None` | Wire collected cli_path values |
| Lines 793-853: `generate_env_file()` | Only maps `"anthropic"` → ANTHROPIC_API_KEY, `"openai"` → OPENAI_API_KEY | Also map `"claude-code"` → ANTHROPIC_API_KEY, `"codex"` → OPENAI_API_KEY |

### Provider-to-API-Key Mapping for .env Generation

| Provider | API Key Required | Reason |
|----------|-----------------|--------|
| `anthropic` | `ANTHROPIC_API_KEY` | Direct API usage via rig |
| `openai` | `OPENAI_API_KEY` | Direct API usage via rig |
| `claude-code` | `ANTHROPIC_API_KEY` | Claude Code CLI reads from env var |
| `codex` | `OPENAI_API_KEY` | Codex CLI reads from env var |

SDK providers still need the API key environment variable — the CLI tools read them from the environment. The daemon passes these env vars to the subprocess (implemented in Story 15.3 `SdkRuntime::spawn()`). The `.env` template must include them with SDK-aware comments.

### Existing SDK Validation Infrastructure (Reuse, Don't Duplicate)

**In `src/config/mod.rs`:**
- `resolve_cli_name(provider)` (line 696) — maps `"claude-code"` → `"claude"`, `"codex"` → `"codex"` — **private (`fn`), NOT callable from `cli/mod.rs`**
- `validate_cli_availability(provider, role_name, cli_path)` (line 742) — runs `{cli} --version`, returns `Result<(), ConfigError>`
- `validate_sdk_skill_files(provider, project_root)` (line 712) — checks 3 BMAD skill files exist
- `sdk_provider_skill_dir(provider)` (line 704) — maps provider to skill directory
- `LlmRoleConfig::is_sdk_provider()` (line 226) — `true` for `"claude-code"` | `"codex"`
- `LlmRoleConfig::is_api_provider()` (line 230) — `true` for `"anthropic"` | `"openai"`

The init command should REUSE `validate_sdk_skill_files()` (which is `fn` but called via `validate_sdk_providers()` on `BotConfig`) for the optional warning. For CLI detection, use an inline mapping since `resolve_cli_name()` is private — see Project Structure Notes.

### CLI Detection Strategy

Run `claude --version` and `codex --version` as the FIRST thing in the LLM configuration section. Use `std::process::Command` (synchronous, same pattern as `validate_cli_availability()`). Capture output:
- Success: display detected version, include provider in selection list
- Failure: silently exclude from selection list (user can't use what's not installed)

**Version output formats (verified via web research):**
- `claude --version` → `"2.1.119 (Claude Code)"` — strip the ` (Claude Code)` suffix to extract semver
- `codex --version` → `"0.125.0"` — pure semver, use directly

**Two distinct paths exist:**
1. **Detection (Task 1.2):** `detect_sdk_cli(cli_name) -> Option<String>` — simple helper, returns version string on success, `None` on failure. Used to filter the provider selection list. Inline CLI name mapping (`"claude"` / `"codex"`).
2. **Validation (Task 5.1):** `config.validate_sdk_providers()` — the existing method from `BotConfig`, which internally calls `validate_cli_availability(provider, role_name, cli_path)`. Returns `ConfigError` with role context. Used for the post-config warning.

Do NOT conflate these two paths — they serve different purposes at different points in the init flow.

### Codex Trust Reminder

When `codex` is selected as a provider, the init command should print a post-setup reminder:
```
Note: Run `codex trust .` in your project root to allow Codex MCP server access.
```
Codex requires explicit project directory trust for MCP server communication via `.codex/config.toml`. Without it, the MCP supervisor connection will fail at runtime.

### MCP Supervisor Config — NOT Generated by Init

The MCP supervisor config JSON is generated **dynamically at runtime** for each SDK session by:
- `src/runtime/sdk_claude.rs` — `build_mcp_config_json()` writes a temp file
- `src/runtime/sdk_codex.rs` — writes `.codex/config.toml`

The `init` command does NOT need to generate or reference MCP config. This is purely a runtime concern handled by Stories 15.4/15.5/15.6.

### Interactive Prompt Flow (Target Design)

```
── LLM Configuration ──
  Detected: claude (v1.0.19), codex (v0.1.2)

LLM provider for DEV agent
> anthropic
  openai
  claude-code (detected)
  codex (detected)

Model for DEV agent [claude-sonnet-4-6]: █

Custom path to claude CLI (optional, Enter for default): █

Use same provider/model for REVIEW and SUPERVISOR roles? [y/N]
```

When no SDK CLIs are detected, the prompt is identical to before (only `anthropic` and `openai` shown).

### Test Helper Updates

The `make_test_config()` helper at `src/cli/mod.rs:1653` already includes `cli_path: None` for all roles (added in Story 15.2). New tests that need SDK configs should construct `LlmRoleConfig` structs with `provider: "claude-code"` and optional `cli_path`.

### Previous Story Learnings

- **From Story 15.2:** `validate_sdk_providers()` is called in `run_start()` at line 1362. The init command runs validation as a best-effort warning, not a blocker — the user may install CLIs after generating config.
- **From Story 15.7:** `SdkRuntime` subprocess passes env vars from `BotSecrets` (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) to the CLI subprocess. The `.env` file is the source of these keys — init MUST include them for SDK providers.
- **From Story 15.5/15.6:** Claude Code uses `ANTHROPIC_API_KEY`, Codex uses `OPENAI_API_KEY` — identical env var names as their API counterparts.

### Git Intelligence

Recent commits (Stories 15.5-15.7) added 5551 lines across 17 files. Key patterns:
- `src/runtime/sdk_claude.rs` (711 lines) — Claude Code CLI invocation, `--model`, `--cd`, `--mcp-config`
- `src/runtime/sdk_codex.rs` (1057 lines) — Codex CLI invocation, `.codex/config.toml` MCP config
- All new modules follow the established pattern: `pub(crate)` visibility for shared helpers, `#[cfg(test)] mod tests` inline

The init command changes are confined to `src/cli/mod.rs` — no other files need modification.

### Anti-Patterns to Avoid

- **DO NOT generate MCP supervisor config in init** — it's dynamically generated at runtime per-session
- **DO NOT block init on missing CLI** — warn and continue. The user may install after generating config
- **DO NOT duplicate `validate_cli_availability()` for detection** — write a simpler `detect_sdk_cli(cli_name) -> Option<String>` that just runs `--version` and returns the version string. The full `validate_cli_availability()` is used indirectly via `config.validate_sdk_providers()` in Task 5 for the post-config warning
- **DO NOT show SDK providers in the selection list if CLI is not installed** — this prevents confusion and broken configs that would fail at `run_start()`
- **DO NOT forget to map SDK providers to API keys in `.env`** — `claude-code` needs `ANTHROPIC_API_KEY`, `codex` needs `OPENAI_API_KEY`
- **DO NOT show `base_url` prompt for SDK providers** — `base_url` is meaningless for CLI subprocesses
- **DO NOT show `cli_path` prompt for API providers** — `cli_path` is meaningless for direct API calls
- **DO NOT silently allow all-SDK configs without warning** — consultations (adversarial, critic) are API-only via `ConsultationRunner`/`AgentFactory`. All-SDK configs will cause consultations to skip or fail at runtime. The init command must warn about this
- **DO NOT use `LLM_PROVIDERS` constant directly in `dialoguer::Select`** — use the runtime-filtered vec from Task 1.4. The constant is the superset; the prompt shows only detected CLIs

### Project Structure Notes

- Primary file modified: `src/cli/mod.rs`
- No new files needed
- No module declarations needed
- Follow existing `dialoguer` patterns for interactive prompts
- `resolve_cli_name()` in `src/config/mod.rs` is currently `fn` (private). The init command uses its own inline mapping (`"claude-code"` → `"claude"`, `"codex"` → `"codex"`) in the `detect_sdk_cli()` and `prompt_provider_options()` helpers rather than changing visibility of a config-internal function. This is intentional duplication — the mapping is trivial (2 lines) and avoids coupling init prompts to config internals

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story-15.8]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-26.md#Story-15.8]
- [Source: _bmad-output/implementation-artifacts/15-2-config-extension-sdk-providers.md — SDK validation infrastructure]
- [Source: _bmad-output/implementation-artifacts/15-7-pipeline-dual-runtime-orchestration.md — Dual runtime, WAL, consultation patterns]
- [Source: src/cli/mod.rs:138 — LLM_PROVIDERS constant]
- [Source: src/cli/mod.rs:150-156 — default_model_for_provider()]
- [Source: src/cli/mod.rs:427-755 — collect_config_interactively()]
- [Source: src/cli/mod.rs:793-853 — generate_env_file()]
- [Source: src/config/mod.rs:696-769 — SDK validation helpers]
- [Source: _bmad-output/project-context.md — Project conventions and rules]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Task 1: Extended `LLM_PROVIDERS` to `["anthropic", "openai", "claude-code", "codex"]`. Added `detect_sdk_cli()` helper that runs `{cli_name} --version` and returns the version string. Added `build_available_providers()` to runtime-filter the provider list based on CLI detection. SDK CLIs not installed are excluded from the interactive selection.
- Task 2: Extended `default_model_for_provider()` with `"claude-code" => "claude-sonnet-4-6"` and `"codex" => "o4-mini"`.
- Task 3: Added `prompt_provider_options()` helper that prompts for `base_url` (API providers) or `cli_path` (SDK providers). Expanded the same_for_all tuple from 6 to 8 elements adding `review_cli_path` and `supervisor_cli_path`. When `same_for_all == true`, `dev_cli_path` is cloned for all roles.
- Task 4: Wired collected `cli_path` values into `BotConfig` construction, replacing the hardcoded `cli_path: None`.
- Task 5: Added post-validation warnings in `run_init()`: best-effort `validate_sdk_providers()` with warning on failure, codex trust reminder, and all-SDK consultation warning.
- Task 6: Rewrote `.env` generation to normalize SDK providers to API key names (`claude-code` -> `anthropic`, `codex` -> `openai`) for natural deduplication. Role comments now include provider name: `"dev (claude-code)"`.
- Task 7: Added 14 new unit tests covering all ACs: default models, env generation with SDK providers, deduplication, roundtrip serialization, role comments, and `build_available_providers()`. Updated existing `test_generate_env_comments_specify_correct_roles` for new comment format.
- Task 8: All 1629 tests pass (1494 binary + 135 library). `cargo fmt --check` clean. No new clippy warnings in `src/cli/mod.rs`.

### Change Log

- 2026-04-27: Story 15.8 implementation complete — SDK provider support in `bmad-bot init`

### Review Findings

- [x] [Review][Patch] No timeout on `detect_sdk_cli` subprocess — init can hang indefinitely if CLI binary blocks [src/cli/mod.rs:163-177] — fixed: added 5s timeout via spawn + try_wait loop
- [x] [Review][Defer] Wildcard `_ => "codex"` in `prompt_provider_options` maps any unknown provider to codex CLI name [src/cli/mod.rs:194-195] — deferred, maintenance hazard for future SDK providers
- [x] [Review][Defer] 8-element positional tuple for same_for_all destructuring is fragile — swap risk between base_url/cli_path [src/cli/mod.rs:647-665] — deferred, structural refactor out of scope
- [x] [Review][Defer] `parse_base_url_input` does not validate URL format — user discovers error only after completing entire wizard [src/cli/mod.rs] — deferred, pre-existing

### File List

- src/cli/mod.rs (modified)
