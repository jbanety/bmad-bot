---
# Story 11.4: Migrate rig Fork to Official Crate

Status: done

## Story

As a maintainer,
I want to use the official `rig-core` crate from crates.io instead of the forked repository,
So that I no longer maintain a fork and benefit from upstream updates.

## Acceptance Criteria

1. **Given** `Cargo.toml` references `rig-core` from `git = "https://github.com/jbanety/rig.git"` branch `fix/copilot-streaming-compat`
   **When** this story is implemented
   **Then** the dependency is changed to `rig-core = { version = "...", features = ["rmcp"] }` from crates.io
   **And** the version selected is the latest stable release that includes the `rmcp` feature

2. **Given** the official `rig-core` crate is used
   **When** `cargo build` is run
   **Then** the project compiles without errors

3. **Given** the official `rig-core` crate is used
   **When** `cargo test` is run
   **Then** all remaining tests pass (Copilot tests already removed in prior stories)

4. **Given** the official `rig-core` crate is used
   **When** `cargo clippy -- -D warnings` is run
   **Then** zero **new** clippy errors are introduced beyond the 34 pre-existing ones (see baseline in Dev Notes)

5. **Given** the fork is no longer needed
   **When** this story is complete
   **Then** the `Cargo.lock` reflects only crates.io dependencies for rig-core (no git sources)

## Not in Scope

- Documentation updates (Story 11.5)
- Adding new features or refactoring existing code beyond what's needed for compilation
- Fixing the 34 pre-existing clippy errors in the codebase (dead_code, unused_imports, etc. — all pre-date Epic 11)
- Fixing the pre-existing test failure `test_build_context_limit_recovery_message_contains_all_sections` in `runner.rs`
- Upgrading other crates unless strictly required by rig-core or rmcp version constraints

## Tasks / Subtasks

- [x] **Task 0: Capture pre-migration baseline** (AC: #4)
  - [x] 0.1 On the current codebase (before any Cargo.toml change), run `cargo clippy -- -D warnings 2>&1 | grep "^error" | wc -l` — confirm count matches 34
  - [x] 0.2 Save the full error list: `cargo clippy -- -D warnings 2>&1 | grep "^error" > /tmp/clippy-baseline.txt`
  - [x] 0.3 Run `cargo test 2>&1 | tail -5` — note the passing test count (1131 as of 11.3)
  - [x] 0.4 Record: `git rev-parse HEAD` as the rollback point

- [x] **Task 1: Research & Version Selection** (AC: #1)
  - [x] 1.1 Fetch the actual latest stable `rig-core` version: `curl -s 'https://crates.io/api/v1/crates/rig-core' | grep '"newest_version"'`
  - [x] 1.2 Verify the `rmcp` feature flag exists in that version's `Cargo.toml`
  - [x] 1.3 Determine the exact `rmcp` crate version required by that rig-core (expected: `rmcp = "1"` — verify in rig-core's published `Cargo.toml`)
  - [x] 1.4 Check for transitive dependency conflicts via `cargo tree` after the change

- [x] **Task 2: Update `Cargo.toml`** (AC: #1, #5)
  - [x] 2.1 Replace `rig-core` git dependency with crates.io version (use `"0.X"` semver range, not `"0.X.Y"` exact pin)
  - [x] 2.2 Update `rmcp` version from `"0.13"` to `"1"` — rig-core 0.35+ requires rmcp 1.x
  - [x] 2.3 Verify `rmcp` features are preserved: `["client", "transport-child-process"]`
  - [x] 2.4 Handle `rustls` potential conflict: the project has `rustls = { version = "0.23", features = ["ring"] }`. Run `cargo tree -d` after the change and check for duplicate `rustls` versions. If rig-core pulls a different rustls major, remove the explicit project dep and let rig-core manage it.
  - [x] 2.5 Run `cargo update` to regenerate `Cargo.lock` — verify no git sources remain for rig-core

- [x] **Task 3: Fix Compilation — The `impl_agent_configurator!` Macro System (HIGHEST RISK)** (AC: #2)
  - [x] 3.1 Understand the scope: `src/llm/agent_factory.rs` contains the `impl_agent_configurator!` macro that generates `AgentConfigurator` impls hardcoded to **two concrete provider types**: `AgentBuilder<anthropic::completion::CompletionModel>` and `AgentBuilder<openai::responses_api::ResponsesCompletionModel>`. If rig-core 0.35.x renamed or moved these types, the entire macro system and the `AgentConfigurator` trait fail to compile.
  - [x] 3.2 Find the new canonical paths: check rig-core docs or source for `anthropic::completion::CompletionModel` and `openai::responses_api::ResponsesCompletionModel` — update in the `AgentConfigurator` trait definition, the `impl_agent_configurator!` macro, and the `BuiltAgent` enum variants.
  - [x] 3.3 Check the `.rmcp_tools(tools, sink)` method on `AgentBuilder` — this is the bridge between rig-core and rmcp. Its signature depends on both libraries simultaneously. Verify it still exists and its parameter types match updated rmcp 1.x types.
  - [x] 3.4 Fix remaining rig import path changes across all 15 affected source files (see complete API surface table below)
  - [x] 3.5 Fix trait signature changes for `Tool`, `StreamingChat`, `CompletionModel`, `StreamingPromptHook`, `HookAction`
  - [x] 3.6 Fix `MultiTurnStreamItem` enum variants and `StreamedAssistantContent` / `StreamedUserContent` variant structure if renamed
  - [x] 3.7 Fix provider client builder chain: `.builder()`, `.api_key()`, `.base_url()`, `.build()` for both `anthropic::Client` and `openai::Client`
  - [x] 3.8 Fix `rig::completion::PromptError` and `rig::completion::CompletionError` manual construction in `streaming_chat()` — the nested variant pattern `PromptError::CompletionError(CompletionError::ResponseError(...))` is brittle
  - [x] 3.9 Iterate: `cargo build` → fix → repeat until zero errors

- [x] **Task 4: Fix rmcp API Breakages** (AC: #2)
  - [x] 4.1 Fix `rmcp::model::Tool` type (used as `McpToolDef` in `agent_factory.rs` and `mcp/manager.rs`)
  - [x] 4.2 Fix `rmcp::service::*` — `RoleClient`, `RunningService`, `ServerSink`, `ServiceExt` (heavy usage in `mcp/manager.rs`)
  - [x] 4.3 Fix `rmcp::transport::*` — `ConfigureCommandExt`, `TokioChildProcess` (used in `mcp/manager.rs`)
  - [x] 4.4 Update the 5 `.with_mcp(mcp_data)` call sites (see list in Dev Notes) — their input type `Vec<(Vec<McpToolDef>, ServerSink)>` depends on both rmcp and the updated `ToolConfigurator`

- [x] **Task 5: Fix Tests** (AC: #3)
  - [x] 5.1 Run `cargo test` and fix all new test failures
  - [x] 5.2 Update test mocks/fixtures if rig or rmcp types changed
  - [x] 5.3 Compare passing test count against baseline from Task 0.3 — investigate any reduction

- [x] **Task 6: Final Verification** (AC: #4, #5)
  - [x] 6.1 Run `cargo clippy -- -D warnings 2>&1 | grep "^error"` — diff against `/tmp/clippy-baseline.txt` from Task 0.2. Zero new entries allowed.
  - [x] 6.2 Run `cargo fmt --check` — fix any formatting issues
  - [x] 6.3 Verify `Cargo.lock` has no git sources: `grep "jbanety/rig" Cargo.lock` — must return nothing
  - [x] 6.4 Run `grep -rn "jbanety/rig" .` — must return nothing (including `Cargo.toml`)

### ⚠️ Recommended Task Execution Order

**Task 0 → Task 1 → Task 2 → Task 3+4 (interleaved, compiler-driven) → Task 5 → Task 6**

Tasks 3 and 4 are interleaved — `cargo build` reports errors from both libraries simultaneously. Prioritize Task 3.1–3.3 first (macro system + `.rmcp_tools()` bridge) as they are the highest-risk, most interconnected pieces.

## Dev Notes

### Epic 11 Context

Epic 11 is a linear chain: **11.1 → 11.2 → 11.3 → 11.4 → 11.5**. Story 11.1 (done) removed the auth module (~1,950 lines deleted). Story 11.2 (done) restructured the `AgentFactory` for the two-provider model with `base_url` support. Story 11.3 (done) cleaned up all remaining Copilot references. This story (11.4) migrates from the rig fork to the official crate. Story 11.5 updates documentation.

### ⚠️ WARNING: `project-context.md` Is Stale — Do Not Trust Its LLM Config Section

`_bmad-output/project-context.md` still references GitHub Copilot as a supported provider, mentions `CopilotTokenCache`, `copilot_requires_responses_api()`, `BuiltAgent::OpenAiCompletions`, and lists `github-copilot` among supported providers. **This is incorrect.** All Copilot code was removed in stories 11.1–11.3. The documentation will be corrected in Story 11.5. For this story, ignore the "Multi-Provider LLM Config" section of `project-context.md` entirely. The source of truth for the current provider model is `src/llm/agent_factory.rs`.

### Why the Fork Existed

The fork (`jbanety/rig`, branch `fix/copilot-streaming-compat`) was created to fix a streaming compatibility issue specific to GitHub Copilot's proxy behavior with non-OpenAI models. Since **Copilot is fully removed** (11.1–11.3), the fork's raison d'être no longer exists. The `BuiltAgent::OpenAiCompletions` variant (Copilot Completions API path) was removed in 11.1. The streaming fix is irrelevant.

### Current Fork vs Official — Version Gap

| Aspect | Fork (current) | Official (target — verify at implementation time) |
|--------|----------------|-------------------|
| rig-core version | 0.30.0 | ~0.35.0 (latest as of 2026-04-16, **confirm on crates.io**) |
| rmcp dependency | 0.13 | ~1.0 (**confirm in rig-core published Cargo.toml**) |
| Default features | `reqwest-tls` | `reqwest` + `rustls` |
| Source | git (jbanety/rig) | crates.io |

**The version 0.35.0 is a research-derived estimate. Run Task 1.1 to get the real current version before editing Cargo.toml.** Five minor version bumps in a pre-1.0 crate = potentially five breaking changes. The codebase nearly doubled from ~35k to ~50k lines. Expect widespread API surface changes.

### Pre-Migration Clippy Baseline (Task 0 — Do First)

Before touching `Cargo.toml`, capture the baseline. As of commit `5746a62` (end of 11.3):
- **`cargo clippy -- -D warnings` reports 34 pre-existing errors** (all dead_code, unused_imports, and style issues in `main.rs` and other modules — none related to rig-core)
- **`cargo test` passes 1131 tests** (1 pre-existing failure: `test_build_context_limit_recovery_message_contains_all_sections`)

AC #4 requires zero **new** clippy errors. Use the diff approach in Task 6.1 to prove this.

### 🚨 HIGHEST RISK: The `impl_agent_configurator!` Macro System

This is the most fragile piece of the migration and is not obvious from a simple grep of rig imports.

**How it works today (`src/llm/agent_factory.rs`):**

The `AgentConfigurator` trait has two methods, each accepting a concrete provider builder type:
- `configure_anthropic(builder: AgentBuilder<anthropic::completion::CompletionModel>) -> Agent<anthropic::completion::CompletionModel>`
- `configure_openai_compatible(builder: AgentBuilder<openai::responses_api::ResponsesCompletionModel>) -> Agent<openai::responses_api::ResponsesCompletionModel>`

The `impl_agent_configurator!` macro generates implementations of this trait for tuples of tools of arities 1–12. Each generated impl:
1. Destructures the tool tuple
2. Chains `.tool(t)` calls on the builder
3. Calls `.rmcp_tools(tools, sink)` for each MCP server pair — **this method is the rig-core/rmcp bridge**
4. Calls `.build()` to produce the final `Agent<_>`

**If rig-core 0.35.x changed:**
- The module path of `anthropic::completion::CompletionModel` → update in: `AgentConfigurator` trait definition, `impl_agent_configurator!` macro body, `BuiltAgent::Anthropic` variant, `AgentFactory::build()` anthropic arm
- The module path of `openai::responses_api::ResponsesCompletionModel` → update in: same locations above for OpenAI variant
- The `.rmcp_tools()` method signature → update in the macro body (and determine new rmcp 1.x types for `tools` and `sink` params)
- The `AgentBuilder<M>` generic parameter or its construction methods → update throughout

**All 5 `configure_agent_tools!(...).with_mcp(mcp_data)` call sites:**

| File | Role | Tool count |
|------|------|------------|
| `src/session/runner.rs` | `LlmRole::Dev` | 8 tools (git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor, ThinkTool) |
| `src/review/mod.rs` | `LlmRole::Review` | 8 tools (git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor, ThinkTool) |
| `src/review/epic.rs` | `LlmRole::EpicReview` | 7 tools (git, read_file, grep, find_path, list_dir, terminal, ThinkTool) |
| `src/supervisor/architect.rs` | `LlmRole::Supervisor` | 7 tools (git, read_file, edit_file, grep, find_path, list_dir, terminal, ThinkTool) |
| Tests in `agent_factory.rs` | test only | 1 tool (ThinkTool) |

The `mcp_data` argument type is `Vec<(Vec<McpToolDef>, ServerSink)>` — both `McpToolDef` (= `rmcp::model::Tool`) and `ServerSink` (= `rmcp::service::ServerSink`) are rmcp types that change with the 0.13 → 1.0 bump.

### 🚨 CRITICAL: rmcp 0.13 → 1.0 Migration

Major semver bump. The project uses rmcp directly in two files:

**`src/mcp/manager.rs`** — heavy usage:
- `rmcp::model::Tool` — tool definition type from MCP servers
- `rmcp::service::{RoleClient, RunningService, ServerSink, ServiceExt}` — server lifecycle
- `rmcp::transport::{ConfigureCommandExt, TokioChildProcess}` — child process transport

**`src/llm/agent_factory.rs`** — light usage:
- `rmcp::model::Tool as McpToolDef` — MCP tool definitions passed to agent builder
- `rmcp::service::ServerSink` — MCP server sink for tool invocation

The return type of `McpManager::tools_for_builder()` is `Vec<(Vec<Tool>, ServerSink)>`. This type signature propagates into every `.with_mcp(mcp_data)` call site even in files that don't directly import rmcp. If either type changes name or module path, all call sites require updating.

### Complete rig API Surface Used (15 source files)

Every rig import that could potentially break:

| Import | Files |
|--------|-------|
| `rig::agent::{Agent, AgentBuilder}` | `agent_factory.rs` |
| `rig::agent::{MultiTurnStreamItem, StreamingPromptHook}` | `session/agent.rs` |
| `rig::agent::HookAction` | `session/agent.rs` |
| `rig::client::CompletionClient` | `agent_factory.rs` |
| `rig::completion::{Chat, CompletionModel, GetTokenUsage, Message}` | `agent_factory.rs`, `session/agent.rs`, `session/state.rs`, `session/runner.rs`, `review/mod.rs`, `review/epic.rs`, `supervisor/architect.rs` |
| `rig::completion::{PromptError, CompletionError}` | `session/agent.rs` (manual error construction) |
| `rig::completion::ToolDefinition` | all 7 `tools/*.rs` files |
| `rig::message::Text` | `session/agent.rs` |
| `rig::streaming::{StreamedAssistantContent, StreamedUserContent, StreamingChat}` | `session/agent.rs` |
| `rig::providers::{anthropic, openai}` | `agent_factory.rs` |
| `rig::tools::think::ThinkTool` | `session/runner.rs`, `review/mod.rs`, `review/epic.rs`, `supervisor/architect.rs` |
| `rig::tool::Tool` | all 7 `tools/*.rs` files, `supervisor/mod.rs` |

**Additional fragile rig API surface not visible via grep (embedded in macro code):**
- `anthropic::completion::CompletionModel` — hardcoded in `AgentConfigurator` trait and `impl_agent_configurator!` macro
- `openai::responses_api::ResponsesCompletionModel` — same
- `AgentBuilder<M>::rmcp_tools(tools, sink)` — method call inside the macro body

### Compilation-Driven Development Strategy

**Step 1:** Change `Cargo.toml` → `cargo build` → collect ALL errors into a file for systematic triage
**Step 2:** Prioritize the macro system (Task 3.1–3.3) — it's the most interconnected
**Step 3:** Fix import path changes across all 15 files (often a one-liner per file)
**Step 4:** Fix trait signature and method changes
**Step 5:** `cargo test` → fix test-specific issues
**Step 6:** Diff clippy output against baseline

### `async-trait` Compatibility Risk

The project depends on `async-trait = "0.1"` and uses it in tool implementations. Rust edition 2024 (used by this project) supports native async traits. If rig-core 0.35.x migrated to native async traits for its `Tool` or `Chat` traits, there may be conflicts between `#[async_trait]` macro usage and native async trait bounds. Watch for "mismatched types" or "impl Trait in return position" errors in tool files.

### `PromptError` / `CompletionError` Manual Construction (Fragile)

In `src/session/agent.rs`, `streaming_chat()` manually constructs error variants:

```src/session/agent.rs
return Err(rig::completion::PromptError::CompletionError(
    rig::completion::CompletionError::ResponseError(
        "Shutdown requested (Ctrl+C)".to_string(),
    ),
));
```

This nested construction depends on the exact variant names and nesting of rig's error types — types that were internal to the streaming implementation. If rig-core restructured its error hierarchy (common in a codebase that doubled in size), this construction will fail. Consider using `PromptError::CompletionError(e)` from a caught error if the direct construction breaks.

### Contingency Plan

If the migration is blocked by a fundamental API incompatibility that cannot be resolved without a major refactor (e.g., rig-core removed the entire `StreamingChat` trait and replaced it with something architecturally different):

1. **Document the blocker** precisely in the Dev Agent Record — which rig-core types/traits changed and what they became
2. **Do not partially break the codebase** — revert to the baseline commit from Task 0.4
3. **Escalate via `ask_supervisor`** — describe the incompatibility and ask whether to: (a) stay on the fork until rig-core stabilizes, (b) pin to a specific intermediate version that doesn't break, or (c) adapt the architecture to the new rig-core API
4. **Update sprint-status.yaml** to `needs-clarification` if supervisor escalates to human

### `Cargo.toml` Expected Change

```Cargo.toml
# BEFORE
rig-core = { git = "https://github.com/jbanety/rig.git", branch = "fix/copilot-streaming-compat", features = ["rmcp"] }
rmcp = { version = "0.13", features = ["client", "transport-child-process"] }

# AFTER (use actual version from Task 1.1)
rig-core = { version = "0.35", features = ["rmcp"] }
rmcp = { version = "1", features = ["client", "transport-child-process"] }
```

**DO NOT** use an exact version like `"0.35.0"` — use the semver range `"0.35"`. Both resolve identically via Cargo (`>= 0.35.0, < 0.36.0`) but the range form is conventional.

### Anti-Patterns to Avoid

1. **DO NOT use exact version pin** (e.g., `version = "0.35.0"`) — use `"0.35"` (semver range for the 0.35.x series). These resolve identically but the range form is the Rust convention for pre-1.0 crates where you want patch updates but not minor-breaking updates.
2. **DO NOT add `default-features = false`** to rig-core unless `cargo tree -d` reveals a concrete rustls version conflict — the defaults (`reqwest` + `rustls`) are what the project needs.
3. **DO NOT refactor existing code** beyond what the compiler forces — this is a dependency migration.
4. **DO NOT remove tests that fail due to API changes** — update them to compile with the new API.
5. **DO NOT downgrade rmcp** — rig-core 0.35.x requires rmcp 1.x; a version mismatch will cause Cargo resolution failure.
6. **DO NOT touch `src/auth/`** — deleted in 11.1; it no longer exists.
7. **DO NOT rewrite the `impl_agent_configurator!` macro from scratch** — update the type paths inside it. The macro structure is correct; only the concrete types embedded in it may have changed.
8. **DO NOT introduce `todo!()` or `unimplemented!()`** — resolve every compilation error properly.
9. **DO NOT trust `project-context.md`'s LLM config section** — it still references Copilot. The source of truth is `src/llm/agent_factory.rs`.
10. **DO NOT count clippy errors to verify AC #4** — diff against the baseline file from Task 0.2 instead.

### Previous Story Intelligence (11.3)

Key learnings:
- **Compiler-driven approach works well** — removing a struct field immediately surfaced all 19 usage sites
- **Pre-existing test failure** — `test_build_context_limit_recovery_message_contains_all_sections` in `runner.rs` was already failing. Ignore it.
- **Pre-existing clippy errors** — 34 errors already present before this story. Do not attempt to fix them — they are out of scope.
- **Provider name is `"openai"`** — NOT `"openai-compatible"`. This was reverted in 11.2 review.
- **`base_url` is wired into BOTH providers** — Anthropic and OpenAI builders both accept `base_url`.
- **`BuiltAgent` has 2 variants:** `Anthropic(Agent<anthropic::completion::CompletionModel>)` and `OpenAiCompatible(Agent<openai::responses_api::ResponsesCompletionModel>)` — these exact type paths are the primary migration target.

### Git Intelligence

Recent commits (most recent first):
- `5746a62` — feat(epic-11): remove Copilot provider, add base_url init prompt (Story 11.3)
- `43c1a5a` — feat(epic-11): add base_url support to AgentFactory for both providers (Story 11.2)
- `07a3b0f` — feat(epic-11): remove GitHub Copilot auth module (Story 11.1)

The codebase is clean after 11.3. No uncommitted changes. Use `5746a62` as rollback point if the migration must be reverted.

### Key File Locations

| File | Role | Expected Impact |
|------|------|-----------------|
| `Cargo.toml` | Dependency declaration | **Change deps** |
| `Cargo.lock` | Lock file | **Regenerated** |
| `src/llm/agent_factory.rs` | `BuiltAgent`, `AgentFactory`, `AgentConfigurator` trait, `impl_agent_configurator!` macro, `configure_agent_tools!` macro | **CRITICAL** — concrete provider types, `.rmcp_tools()` bridge, macro system |
| `src/session/agent.rs` | `streaming_chat()`, `ChatHistoryHook`, tool construction | **HIGH** — `StreamingChat`, `MultiTurnStreamItem`, hooks, error construction |
| `src/mcp/manager.rs` | MCP server lifecycle | **HIGH** — rmcp types throughout |
| `src/tools/*.rs` (7 files) | Tool implementations | **MEDIUM** — `rig::tool::Tool` trait, `ToolDefinition` |
| `src/supervisor/mod.rs` | Supervisor tool dispatch | **MEDIUM** — `ToolDefinition`, `Tool` trait |
| `src/session/runner.rs` | Session runner | **LOW** — `Message`, `ThinkTool`, `.with_mcp()` call site |
| `src/review/mod.rs` | Code review runner | **LOW** — `Message`, `ThinkTool`, `.with_mcp()` call site |
| `src/review/epic.rs` | Epic review runner | **LOW** — `Message`, `ThinkTool`, `.with_mcp()` call site |
| `src/supervisor/architect.rs` | Architect session | **LOW** — `Message`, `ThinkTool`, `.with_mcp()` call site |
| `src/session/state.rs` | Session state | **MINIMAL** — `Message` only |
| `src/session/provider.rs` | API key resolution | **NONE** — no rig imports |

### Project Structure Notes

- No new files or directories created
- No files deleted
- `Cargo.toml` and `Cargo.lock` have guaranteed changes; all source file changes are driven by compilation errors
- After this story, `Cargo.lock` contains zero git-sourced entries for rig-core

### References

- [Source: _bmad-output/planning-artifacts/epics.md § Story 11.4 (L2880–2903)]
- [Source: _bmad-output/planning-artifacts/epics.md § Epic 11 Summary (L2928–2949)]
- [Source: _bmad-output/planning-artifacts/architecture.md § Decision 8: LLM Provider Abstraction (L590–664)]
- [Source: _bmad-output/implementation-artifacts/11-3-clean-provider-routing-config-secrets.md § Dev Notes]
- [Source: _bmad-output/project-context.md § Framework-Specific Rules (L39–112) — WARNING: LLM config section is stale, see note above]
- [Source: _bmad-output/project-context.md § Testing Rules (L112–121)]
- [Source: _bmad-output/project-context.md § Code Quality & Style Rules (L121–169)]
- [Source: src/llm/agent_factory.rs — AgentConfigurator trait, impl_agent_configurator! macro, ToolConfigurator struct]
- [Source: src/mcp/manager.rs — McpManager, rmcp usage]

## Dev Agent Record

### Agent Model Used

Claude Sonnet 4.6

### Debug Log References

- Pre-migration: 34 clippy errors (baseline saved at /tmp/clippy-baseline.txt)
- Pre-migration: 1131 tests passing, 1 pre-existing failure
- Rollback point: git 5746a62
- Post-migration: `cargo check` — 0 new errors, 32 pre-existing warnings
- Post-migration: `cargo test` — 1131 passing, 1 pre-existing failure (unchanged)
- Post-migration: `cargo clippy -- -D warnings` diff — 0 new errors
- Post-migration: `cargo fmt --check` — clean after formatting fix in mcp_playwright.rs

### Completion Notes List

- Task 0: Baseline captured — 34 clippy errors, 1131 tests passing, rollback point 5746a62
- Task 1: rig-core latest stable = 0.35.0 (confirmed via crates.io API); rmcp feature present; rmcp required version = "^1" (verified in 0.35.0 published Cargo.toml)
- Task 2: Cargo.toml updated — git dep replaced with crates.io "0.35", rmcp bumped to "1"; `cargo update` regenerated lock file; no git sources remain; rustls resolves to single version 0.23.38 (no conflict)
- Task 3: Only one compilation error encountered — `rig::agent::StreamingPromptHook` renamed to `rig::agent::PromptHook` in rig 0.35. Fix: import and impl updated in `src/session/agent.rs`. All other rig API paths (`AgentBuilder`, provider types, `.rmcp_tools()`, `MultiTurnStreamItem`, `StreamedAssistantContent`, `StreamedUserContent`, `HookAction`, `PromptError`, `CompletionError`) remained unchanged — the migration was much simpler than anticipated.
- Task 3 (continued): Added `<A as StreamingChat<M, M::StreamingResponse>>::Hook: 'static` bound to `streaming_chat()` function — required by the new `StreamingChat` associated type `Hook` in rig 0.35.
- Task 4: rmcp 1.0 — `mcp/manager.rs` and `llm/agent_factory.rs` compiled without changes. `RoleClient`, `RunningService`, `ServerSink`, `ServiceExt`, `ConfigureCommandExt`, `TokioChildProcess` all retain same module paths. Zero rmcp breakage in production code.
- Task 5: `tests/e2e/mcp_playwright.rs` — `CallToolRequestParam` struct literal construction broken by `#[non_exhaustive]` in rmcp 1.0. Fixed by switching to `CallToolRequestParams::new(name).with_arguments(args)` builder pattern. All 1131 unit tests pass (same baseline count).
- Task 6: `cargo clippy` diff clean, `cargo fmt` applied, `Cargo.lock` has no git sources, no `jbanety/rig` references anywhere.

### Change Log

- Migrate rig-core from git fork to crates.io 0.35, update rmcp to 1.0 (Date: 2026-04-16)

### File List

- Cargo.toml (modified — rig-core git dep → crates.io 0.35, rmcp 0.13 → 1)
- Cargo.lock (regenerated — no git sources)
- src/session/agent.rs (modified — StreamingPromptHook → PromptHook; Hook: 'static bound added)
- tests/e2e/mcp_playwright.rs (modified — CallToolRequestParam struct literal → new() builder)

### Review Findings

- [x] [Review][Defer] `unwrap_or_default()` in test code silently swallows malformed JSON arguments [`tests/e2e/mcp_playwright.rs`] — deferred, pre-existing pattern preserved from old `CallToolRequestParam` struct construction
