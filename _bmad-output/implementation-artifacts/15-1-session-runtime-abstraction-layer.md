# Story 15.1: Session Runtime Abstraction Layer

Status: done

## Story

As a daemon developer,
I want the session execution logic abstracted behind a `SessionRuntime` enum with an `Api` variant wrapping the current rig-based flow,
So that a second `Sdk` variant can be added without modifying existing code.

## Acceptance Criteria

1. **Given** the daemon processes a story **When** a session needs to be started for any pipeline phase **Then** `pipeline.rs` calls `SessionRuntime::run_session()` instead of directly calling `SessionRunner` methods **And** a new `src/runtime/mod.rs` module defines `SessionRuntime` enum with `Api(ApiRuntime)` variant

2. **Given** the current session execution flow (build agent, run session, handle tools) **When** the abstraction is applied **Then** `ApiRuntime` is a thin wrapper delegating to the existing `SessionRunner` with zero behavioral changes **And** system preamble construction (`build_preamble()`, `build_create_preamble()`, `build_review_preamble()`) is scoped to `ApiRuntime` only

3. **Given** skill paths are currently hardcoded to `.claude/skills/` in `pipeline.rs`, `review/mod.rs`, `session/runner.rs` **When** the abstraction is applied **Then** a centralized `resolve_skill_path(skill_name)` reads `_bmad/_config/manifest.yaml` -> `ides[]` and maps to the correct directory **And** all hardcoded `.claude/skills/` references in production code are replaced **And** skill paths are resolved once at daemon startup and cached -- not read from disk on every call

4. **Given** the `Sdk` variant is not yet implemented **When** the enum is defined **Then** `Sdk` variant exists as a stub (`todo!()`) -- wired in subsequent stories

5. **Given** all existing tests pass **When** the abstraction is applied **Then** zero behavioral changes -- all 1310+ tests pass identically

## Tasks / Subtasks

- [x] Task 1: Create `src/runtime/mod.rs` module with `SessionRuntime` enum (AC: #1, #4)
  - [x] 1.1 Create `src/runtime/mod.rs` defining `SessionRuntime` enum with `Api(ApiRuntime)` and `Sdk(SdkRuntime)` variants
  - [x] 1.2 Define `ApiRuntime` as a thin wrapper that holds the existing `SessionRunner` (already constructed). `ApiRuntime` does NOT duplicate `SessionRunner`'s dependencies -- it receives an already-constructed `SessionRunner` and delegates to it. The only added responsibility is preamble selection based on role/phase.
  - [x] 1.3 Define `SdkRuntime` as a stub struct with `todo!()` in its `run_session()` method
  - [x] 1.4 Implement `SessionRuntime::run_session()` that dispatches to the appropriate variant
  - [x] 1.5 Create `src/runtime/api.rs` for `ApiRuntime` implementation (if the module grows too large) — kept in mod.rs, module size is manageable (~150 lines of production code)
  - [x] 1.6 Add `mod runtime;` to `src/main.rs` (lines 4-17, where ALL modules are declared -- NOT `src/lib.rs` which only re-exports for integration tests)

- [x] Task 2: Implement skill path resolution with startup caching (AC: #3)
  - [x] 2.1 Add `SkillPaths` struct that holds pre-resolved paths for all three skills (`bmad-dev-story`, `bmad-create-story`, `bmad-code-review`). This struct is constructed once at daemon startup and passed down to the runtime and pipeline.
  - [x] 2.2 Add `SkillPaths::resolve(project_root: &Path) -> SkillPaths` factory method that reads `_bmad/_config/manifest.yaml` -> `ides[]` array, maps the first IDE name to the skill directory, and builds all three paths. No disk I/O after initial construction.
  - [x] 2.3 IDE-to-path mapping: `"claude-code"` -> `.claude/skills/`, `"codex"` -> `.agents/skills/`. For API mode (rig), use the first available IDE from the manifest's `ides` list
  - [x] 2.4 If manifest file doesn't exist or `ides` array is empty, fall back to `.claude/skills/` with a `tracing::warn!` (backward compatibility). This is NOT an error -- existing installations without manifest support must keep working.
  - [x] 2.5 Write unit tests for `SkillPaths::resolve()`: manifest with `claude-code`, manifest with `codex`, missing manifest fallback, empty ides fallback

- [x] Task 3: Replace hardcoded skill paths in `pipeline.rs` (AC: #3)
  - [x] 3.1 Add `SkillPaths` field to `StoryPipeline` struct, populated at construction from the cached resolution
  - [x] 3.2 Replace `".claude/skills/bmad-create-story/SKILL.md"` at line ~405 with `self.skill_paths.create_story`
  - [x] 3.3 Replace `".claude/skills/bmad-code-review/SKILL.md"` at line ~1223 with `self.skill_paths.code_review`
  - [x] 3.4 Update tests at lines ~5420-5421 and ~5489 to construct `SkillPaths` (or use the default fallback) instead of hardcoded literals
  - [x] 3.5 Also update any `SessionRunner::new()` calls in pipeline test fixtures (lines ~5273, ~5644, ~5713, etc.) if they reference hardcoded skill paths

- [x] Task 4: Replace hardcoded skill paths in `session/runner.rs` and `review/mod.rs` (AC: #3)
  - [x] 4.1 Replace `".claude/skills/bmad-dev-story/SKILL.md"` default at `runner.rs` line ~382 -- either accept the resolved path via constructor parameter or via the `SkillPaths` struct
  - [x] 4.2 Replace `".claude/skills/bmad-code-review/SKILL.md"` at `review/mod.rs` line ~584 -- pass the resolved path in from the caller (pipeline or `SkillPaths`)
  - [x] 4.3 Update `SessionRunner::new()` calls in `runner.rs` test code (12+ test functions at lines ~2710, ~2739, ~2760, ~2787, ~2815, ~2985, ~3016, ~3054, ~3171, ~3213, ~3497, ~3516) to provide the resolved or default skill path

- [x] Task 5: Wire `SessionRuntime` into `pipeline.rs` (AC: #1, #2)
  - [x] 5.1 In `StoryPipeline`, replace `session_runner: SessionRunner` field with `session_runtime: SessionRuntime`. The `SessionRunner` is still constructed exactly as before (line ~248-257) but wrapped in `ApiRuntime` then `SessionRuntime::Api`
  - [x] 5.2 Replace direct `session_runner.run()` call at line ~694 with `session_runtime.run_session()`
  - [x] 5.3 Replace direct `session_runner.run_with_consultations()` calls at lines ~401-410 and ~1219-1230 with equivalent runtime dispatch
  - [x] 5.4 Move preamble construction (`build_preamble()`, `build_create_preamble()`, `build_review_preamble()`) from pipeline.rs call sites into `ApiRuntime` -- pipeline passes role and phase via `SessionContext`, `ApiRuntime` selects and builds the correct preamble internally
  - [x] 5.5 `EpicReviewRunner` remains unchanged -- it is a separate execution path (post-epic retrospective) that does NOT go through `SessionRuntime`. Out of scope for this story.

- [x] Task 6: Verify all tests pass with zero behavioral changes (AC: #5)
  - [x] 6.1 Run `cargo clippy -- -D warnings` -- zero new warnings (33 pre-existing dead_code warnings unchanged)
  - [x] 6.2 Run `cargo test` -- all 1321 tests pass (1310 existing + 11 new runtime tests)
  - [x] 6.3 Run `cargo fmt --check` -- no formatting issues

## Dev Notes

### Architecture Decision Reference

This story implements **Decision 12: Dual Runtime Abstraction** from the architecture document.
[Source: architecture.md, Decision 12 — SessionRuntime Enum]

The `SessionRuntime` enum is a permanent parallel architecture, not a migration. Both `Api` and `Sdk` variants are first-class runtimes. Story 15.1 establishes the abstraction; subsequent stories (15.3-15.7) fill in the `Sdk` variant.

### Scope Clarification: What Is and Is Not Abstracted

**In scope -- abstracted behind `SessionRuntime`:**
- `SessionRunner` (dev and create pipeline phases) -- wrapped by `ApiRuntime`
- The review phase when called via `session_runner.run_with_consultations()` with `LlmRole::Review` in pipeline.rs

**Out of scope -- separate execution paths, NOT abstracted in this story:**
- `ReviewRunner` (`src/review/mod.rs`) -- independent runner with its own `AgentFactory`-based agent construction, used for the standalone review flow. It has its own hardcoded skill path at line ~584 which IS replaced with resolved paths, but `ReviewRunner` itself is NOT wrapped in `SessionRuntime`. Future stories may unify it.
- `EpicReviewRunner` (`src/review/epic.rs`) -- post-epic retrospective runner. Completely independent, untouched.

### Current Code to Refactor

**Session execution entry points (pipeline -> session):**
- `pipeline.rs` line ~248-257: `SessionRunner::new()` construction with 8 constructor parameters
- `pipeline.rs` line ~694: `self.session_runner.run(story, base_branch_override).await`
- `pipeline.rs` line ~401-410: `self.session_runner.run_with_consultations(...)` for create phase
- `pipeline.rs` line ~1219-1230: `self.session_runner.run_with_consultations(...)` for review phase

**SessionRunner key methods:**
- `session/runner.rs` line ~711-726: `run()` -- simple wrapper calling `run_with_consultations()` with empty consultations
- `session/runner.rs` line ~737-746: `run_with_consultations()` -- main entry with 7 parameters (excluding `&self`): `story`, `base_branch_override`, `consultations`, `skill_path_override`, `preamble_override`, `role`, `initial_phase`
- `session/runner.rs` line ~324-355: `SessionRunner` struct fields and `new()` constructor

**All three preamble methods (must be scoped to ApiRuntime only):**
- `session/agent.rs` line ~245: `build_preamble(mcp_tool_names, model)` -- dev session preamble
- `session/agent.rs` line ~320: `build_create_preamble()` -- create-story preamble
- `session/agent.rs` line ~369: `build_review_preamble()` -- review session preamble
- Pipeline call sites: line ~398 (`build_create_preamble()`), line ~1215 (`build_review_preamble()`), `build_preamble()` called internally by runner at line ~1018

**BuiltAgent (stays in API mode, not moved):**
- `llm/agent_factory.rs` line ~77-82: `BuiltAgent` enum (Anthropic, OpenAiCompatible)
- `llm/agent_factory.rs` line ~254-262: `AgentFactory::build()` signature
- `llm/agent_factory.rs` line ~96-109: `BuiltAgent::stream_chat()` dispatch

### Hardcoded Skill Paths to Replace

**Production code (4 locations):**

| File | Line | Current Value |
|------|------|---------------|
| `pipeline.rs` | ~405 | `".claude/skills/bmad-create-story/SKILL.md"` |
| `pipeline.rs` | ~1223 | `".claude/skills/bmad-code-review/SKILL.md"` |
| `session/runner.rs` | ~382 | `".claude/skills/bmad-dev-story/SKILL.md"` |
| `review/mod.rs` | ~584 | `".claude/skills/bmad-code-review/SKILL.md"` |

**Test code (pipeline.rs -- 2 locations + runner.rs -- 12+ test fixtures):**

| File | Lines | Current Value |
|------|-------|---------------|
| `pipeline.rs` | ~5420-5421 | `".claude/skills/bmad-create-story/SKILL.md"`, `".claude/skills/bmad-dev-story/SKILL.md"` |
| `pipeline.rs` | ~5489 | `".claude/skills/bmad-code-review/SKILL.md"` |
| `session/runner.rs` | ~2710, ~2739, ~2760, ~2787, ~2815, ~2985, ~3016, ~3054, ~3171, ~3213, ~3497, ~3516 | `SessionRunner::new()` test fixtures that inherit the default skill path |

**Documentation/comment references (5 locations -- OUT OF SCOPE for code changes, leave as-is):**

| File | Lines | Context |
|------|-------|---------|
| `session/agent.rs` | ~12, ~881, ~893 | Module doc comments, dual-mode behavior examples |
| `llm/agent_factory.rs` | ~119 | Doc comment with example path |
| `session/state.rs` | ~122 | Field doc with example skill path |

These doc references describe the concept, not resolve paths at runtime. They may be updated in a follow-up documentation pass but are NOT blocking for this story.

### BMAD Manifest Structure

The manifest at `_bmad/_config/manifest.yaml` has this structure:

```yaml
installation:
  version: 6.3.0
ides:
  - claude-code
```

The `ides` array lists installed IDE/CLI integrations. The mapping convention:
- `"claude-code"` -> `.claude/skills/{skill_name}/SKILL.md`
- `"codex"` -> `.agents/skills/{skill_name}/SKILL.md`

For API mode (rig), use the first IDE in the list to resolve skill paths. The daemon does NOT manage skill installation -- BMAD's installer handles that.

### Skill Path Resolution Design -- Resolve Once, Use Everywhere

```rust
pub struct SkillPaths {
    pub dev_story: String,
    pub create_story: String,
    pub code_review: String,
}

impl SkillPaths {
    pub fn resolve(project_root: &Path) -> Self {
        let manifest_path = project_root.join("_bmad/_config/manifest.yaml");
        let base = match Self::read_skill_base(&manifest_path) {
            Some(base) => base,
            None => {
                tracing::warn!("BMAD manifest not found or empty ides[], falling back to .claude/skills/");
                ".claude/skills".to_string()
            }
        };
        Self {
            dev_story: format!("{base}/bmad-dev-story/SKILL.md"),
            create_story: format!("{base}/bmad-create-story/SKILL.md"),
            code_review: format!("{base}/bmad-code-review/SKILL.md"),
        }
    }
}
```

This struct is constructed once at daemon startup (in `StoryPipeline::new()` or `run_start()`), stored as a field, and passed by reference wherever skill paths are needed. No `Result` return, no `?` operator at call sites -- the fallback to `.claude/skills/` guarantees construction always succeeds.

### SessionRuntime Enum Design

```rust
pub enum SessionRuntime {
    Api(ApiRuntime),   // rig-based: BuiltAgent, streaming_chat, custom tools
    Sdk(SdkRuntime),   // CLI subprocess: NDJSON streaming, native tools (stub in 15.1)
}

impl SessionRuntime {
    pub async fn run_session(&self, context: SessionContext) -> SessionOutcome {
        match self {
            Self::Api(api) => api.run_session(context).await,
            Self::Sdk(_) => todo!("SDK runtime implemented in Story 15.3+"),
        }
    }
}
```

**`SessionContext` carries these 5 fields** (replaces the 7 parameters of `run_with_consultations()`):

```rust
pub struct SessionContext<'a> {
    pub story: &'a StoryInfo,
    pub base_branch_override: Option<&'a str>,
    pub consultations: Vec<ConsultationConfig>,
    pub role: LlmRole,
    pub initial_phase: &'a str,
}
```

`SessionContext` does NOT carry `preamble_override` or `skill_path_override`:
- **Preamble:** `ApiRuntime` selects the preamble internally based on `role` and `initial_phase`. SDK runtime ignores preambles entirely. The preamble is an API-mode implementation detail.
- **Skill path:** `ApiRuntime` holds a reference to `SkillPaths` and resolves the correct skill based on `role`/`initial_phase`. SDK runtime discovers skills natively.

**`ApiRuntime` is a thin wrapper, not a replacement:**

```rust
pub struct ApiRuntime {
    session_runner: SessionRunner,  // receives already-constructed runner
    skill_paths: SkillPaths,        // pre-resolved at startup
}
```

`ApiRuntime::run_session()` does:
1. Select preamble from role/phase (calls `build_preamble()`, `build_create_preamble()`, or `build_review_preamble()`)
2. Select skill path from `self.skill_paths` based on role/phase
3. Delegate to `self.session_runner.run_with_consultations(story, base_branch, consultations, skill_path, preamble, role, phase)`

No duplication of `SessionRunner`'s dependencies. No structural changes to `SessionRunner` itself.

### ShutdownFlag Propagation

`ShutdownFlag` is `Arc<AtomicBool>` (defined in `session/agent.rs` line ~159, re-exported from `session/runner.rs` line ~23). Currently propagated: `cli/run_start()` -> `StoryPipeline` -> `SessionRunner` -> `streaming_chat()`. The runtime abstraction preserves this chain: `StoryPipeline` -> `SessionRuntime::Api(ApiRuntime)` -> `ApiRuntime.session_runner` -> `streaming_chat()`. No changes to `ShutdownFlag` propagation -- `SessionRunner` still holds it as before.

### WAL State Considerations

`SessionState` (in `session/state.rs` line ~94-137) already has a `skill_path: String` field. Story 15.1 does NOT add `runtime_type` or `sdk_session_ids` -- those are Story 15.7 concerns. The `skill_path` field will store the resolved path from `SkillPaths` instead of the hardcoded value.

### Anti-Patterns to Avoid

- `ApiRuntime` is a thin wrapper around an existing `SessionRunner` -- do NOT duplicate `SessionRunner`'s constructor parameters or dependencies inside `ApiRuntime`. Receive an already-constructed `SessionRunner`.
- Do NOT modify `SessionRunner`'s public API, `BuiltAgent`, `AgentFactory`, or `streaming_chat()` -- they remain API-mode internals unchanged
- Do NOT implement `SdkRuntime` beyond a stub -- that's Stories 15.3-15.6
- Do NOT add WAL fields (`runtime_type`, `sdk_session_ids`) -- that's Story 15.7
- Do NOT change consultation handling logic -- consultations are orthogonal to runtime type
- Do NOT add config changes for SDK providers -- that's Story 15.2
- Do NOT break existing API-mode behavior -- this is a pure refactoring with zero functional changes
- Do NOT modify anything under `_bmad/` -- the daemon reads the manifest, never writes it
- Do NOT abstract `ReviewRunner` or `EpicReviewRunner` behind `SessionRuntime` -- they are independent execution paths, out of scope
- Do NOT return `Result` from `SkillPaths::resolve()` -- the fallback to `.claude/skills/` guarantees construction always succeeds, avoiding `?` propagation into callers that don't return `Result`

### Previous Story Intelligence

Story 15.0a (pre-epic cleanup) was the last completed story. Key learnings:
- All 5 clippy fixes were mechanical substitutions -- no design decisions involved
- Test count: 1310 passed, 0 failed (clean baseline)
- Commit convention: `feat(epic-15): description (Story 15.1)` -- use `refactor` prefix since this is a refactoring story
- Pre-existing dead-code warnings (31 total) remain as `#![warn(dead_code)]` -- do not fix in this story
- `cargo fmt --check` has pre-existing formatting diffs in unrelated code -- not introduced by this story

### Git Intelligence

Recent commits follow convention: `feat(epic-N): description (Story N.M)`. For this refactoring story use: `refactor(epic-15): introduce SessionRuntime abstraction layer (Story 15.1)`.

Last commit: `766a250 fix(pre-epic-15): resolve clippy warnings and stale test (Story 15.0a)`

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Zero-warning policy: `#![deny(clippy::all)]` at crate root
- All tests inline in their respective modules
- New `src/runtime/mod.rs` module MUST include unit tests for: `SkillPaths::resolve()` (4 cases: claude-code, codex, missing manifest, empty ides), `SessionRuntime::run_session()` dispatch (mock or minimal), `ApiRuntime` construction and delegation
- Existing tests: update pipeline tests at lines ~5420, ~5489, and runner.rs test fixtures at 12+ locations to use `SkillPaths` instead of hardcoded literals

### Project Structure Notes

New files to create:
- `src/runtime/mod.rs` -- `SessionRuntime` enum, `SkillPaths`, `SessionContext`, `ApiRuntime`, `SdkRuntime` stub, routing logic
- `src/runtime/api.rs` (optional, only if mod.rs grows too large) -- `ApiRuntime` implementation

Files to modify:
- `src/main.rs` -- Add `mod runtime;` to the module declarations (lines 4-17)
- `src/pipeline.rs` -- Replace `session_runner` field with `session_runtime`, replace hardcoded skill paths, move preamble selection into `ApiRuntime`
- `src/session/runner.rs` -- Replace hardcoded skill path default, update test fixtures
- `src/review/mod.rs` -- Replace hardcoded skill path (receive resolved path from caller)

Files NOT to modify:
- `src/lib.rs` -- Module tree is in `main.rs`, not here
- `src/llm/agent_factory.rs` -- `BuiltAgent` and `AgentFactory` stay as-is (API-mode internals)
- `src/session/agent.rs` -- `build_preamble()`, `build_create_preamble()`, `build_review_preamble()`, `streaming_chat()` stay where they are. `ApiRuntime` calls them, but they don't move.
- `src/session/state.rs` -- WAL structure unchanged in this story
- `src/tools/*` -- Tool implementations are API-mode concerns, untouched
- `src/supervisor/*` -- Supervisor logic untouched
- `src/review/epic.rs` -- `EpicReviewRunner` untouched, out of scope
- `_bmad/_config/manifest.yaml` -- Read-only, never modified by daemon

### References

- [Source: architecture.md#Decision 12 — Dual Runtime Abstraction, SessionRuntime Enum]
- [Source: architecture.md#Decision 5 — Amendment: API-only; SDK uses native skill invocation]
- [Source: architecture.md#Decision 8 — Amendment: Dual runtime, BuiltAgent for API, SdkSession for SDK]
- [Source: planning-artifacts/sprint-change-proposal-2026-04-26.md — Story 15.1 definition]
- [Source: planning-artifacts/epics.md#Epic 15, Story 15.1 — Session Runtime Abstraction Layer]
- [Source: src/pipeline.rs lines ~140-161 — StoryPipeline struct definition (session_runner, epic_review_runner fields)]
- [Source: src/pipeline.rs lines ~248-257, ~401-410, ~694, ~1219-1230 — Current SessionRunner usage]
- [Source: src/session/runner.rs lines ~324-355, ~382, ~711-746 — SessionRunner struct and methods]
- [Source: src/session/agent.rs lines ~159, ~245, ~320, ~369, ~512 — ShutdownFlag, all 3 preamble builders, streaming_chat]
- [Source: src/review/mod.rs lines ~320-340, ~584 — ReviewRunner independent runner, hardcoded skill path]
- [Source: src/llm/agent_factory.rs lines ~77-82, ~96-109, ~254-262 — BuiltAgent and AgentFactory]
- [Source: src/main.rs lines ~4-17 — Module tree declarations (where mod runtime must go)]
- [Source: _bmad/_config/manifest.yaml — BMAD manifest with ides[] array]
- [Source: _bmad-output/project-context.md — Project rules and conventions]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

None — clean implementation with no blocking issues.

### Completion Notes List

- Created `src/runtime/mod.rs` with `SessionRuntime` enum (`Api(Box<ApiRuntime>)`, `Sdk(SdkRuntime)`), `ApiRuntime` thin wrapper, `SdkRuntime` stub, `SkillPaths` resolve-once registry, and `SessionContext` struct
- `SkillPaths::resolve()` reads `_bmad/_config/manifest.yaml` → `ides[]` array, maps IDE name to skill directory, with `.claude/skills/` fallback on missing manifest or empty ides
- `ApiRuntime::run_session()` selects preamble and skill path based on `initial_phase` (create → `build_create_preamble()`, review → `build_review_preamble()`, dev → None), then delegates to `SessionRunner::run_with_consultations()`
- `SessionRunner::new()` now accepts a `skill_path: String` parameter (was hardcoded), with `#[allow(clippy::too_many_arguments)]`
- `ReviewRunner::new()` now accepts a `skill_path: String` parameter, used at `activate_agent()` call site
- `StoryPipeline` now holds `session_runtime: SessionRuntime` and `skill_paths: SkillPaths` instead of `session_runner: SessionRunner`
- Pipeline's 3 session call sites (create, dev, review) replaced with `session_runtime.run_session(SessionContext { ... })`
- WAL recovery methods access the underlying `SessionRunner` via `session_runtime.api_session_runner()`
- `SessionRuntime::Api` uses `Box<ApiRuntime>` to satisfy `clippy::large_enum_variant`
- 11 new unit tests in `runtime::tests`: SkillPaths resolution (claude-code, codex, missing manifest, empty ides, first-ide-wins, ide mapping), phase config resolution (create, review, dev), enum variant construction
- All 9 pipeline test fixtures and 12+ runner.rs test fixtures updated for new constructor signatures
- Test results: 1321 passed (was 1310), 0 failed, 0 new clippy warnings

### File List

- `src/runtime/mod.rs` — NEW: SessionRuntime enum, ApiRuntime, SdkRuntime stub, SkillPaths, SessionContext, unit tests
- `src/main.rs` — MODIFIED: added `mod runtime;` declaration
- `src/pipeline.rs` — MODIFIED: replaced `session_runner` with `session_runtime`/`skill_paths`, updated 3 call sites and 9 test fixtures
- `src/session/runner.rs` — MODIFIED: `SessionRunner::new()` now accepts `skill_path` param, updated 12 test fixtures
- `src/review/mod.rs` — MODIFIED: `ReviewRunner` now stores and uses `skill_path` field, updated 2 test fixtures

### Review Findings

- [x] [Review][Patch] `resolve_phase_config` catch-all `_` arm is fragile — import `PHASE_DEV`, add explicit match arm, add `tracing::warn` on catch-all for unknown phases [src/runtime/mod.rs:128] ✓ Fixed
- [x] [Review][Defer] `api_session_runner()` panics on Sdk variant — crash recovery path is not runtime-aware, must be addressed in Story 15.7 [src/runtime/mod.rs:90, src/pipeline.rs:2748-2882]
- [x] [Review][Defer] `SkillPaths::resolve()` does not validate skill file existence on disk — Story 15.2 covers startup validation [src/runtime/mod.rs:22-37]
- [x] [Review][Defer] `SessionRunner.skill_path` serves dual purpose (dev default + recovery fallback) — pre-existing, not introduced by this change [src/session/runner.rs:564-573]

### Change Log

- 2026-04-26: Implemented SessionRuntime abstraction layer — pure refactoring with zero behavioral changes (Story 15.1)
