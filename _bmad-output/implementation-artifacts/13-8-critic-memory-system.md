# Story 13.8: Critic Memory System

Status: done

## Story

As a daemon operator,
I want a persistent memory file that accumulates the Story Critic's observations across all stories,
so that the Critic can reference previous decisions and maintain vision continuity throughout the sprint.

## Acceptance Criteria

1. **Given** the implementation artifacts directory, **when** the first Critic invocation occurs, **then** a `critic-memory.md` file is created at `{implementation_artifacts}/critic-memory.md` if it does not exist, initialized with a header: `# Story Critic Memory` and the current date.

2. **Given** the Critic agent completes a review (story review or decision resolution), **when** the Critic produces its output, **then** the Critic agent appends a new section to `critic-memory.md` with: timestamp and story key, type of review, key observations and rationale, decisions made and why, any concerns or patterns noticed across stories. The Critic manages the format of its own memory — no rigid structure is imposed by the daemon. _(Daemon-side: satisfied by preamble instructions in Task 4 that tell the Critic how to update its memory. Actual Critic behavior is refined in Story 13.9.)_

3. **Given** `critic-memory.md` grows over time, **when** the file exceeds a configurable size threshold (default: 50KB), **then** a `tracing::warn!` is emitted suggesting manual review or summarization. The pipeline does NOT auto-truncate — the Critic's memory is sacred and only the human should decide to prune it.

4. **Given** a new sprint starts or the user wants a fresh Critic, **when** the user deletes or renames `critic-memory.md`, **then** the next Critic invocation creates a fresh memory file. No error occurs — absence of memory is a valid starting state.

5. **Given** the implementation artifacts directory does not exist or `ensure_exists()` fails, **when** a Critic consultation is about to start, **then** the pipeline logs a `tracing::warn!`, excludes `critic-memory.md` from `context_files`, and the consultation proceeds without memory (degraded mode). The consultation must never crash due to memory file unavailability.

## Tasks / Subtasks

- [x] Task 1: Create `src/critic/mod.rs` module with `CriticMemory` struct (AC: #1, #4, #5)
  - [x] 1.1 Create `src/critic/mod.rs` with `CriticMemory` struct holding `file_path: PathBuf` and `size_threshold_bytes: u64`
  - [x] 1.2 Implement `CriticMemory::new(impl_artifacts_dir: &str, project_root: &str, threshold_kb: u64)` — resolves absolute path: `Path::new(project_root).join(impl_artifacts_dir).join("critic-memory.md")`
  - [x] 1.3 Implement `ensure_exists(&self) -> Result<(), CriticMemoryError>` — creates parent directories if missing via `std::fs::create_dir_all`; creates file with `# Story Critic Memory\n\nInitialized: {date}\n` header if file missing; no-op if file already exists
  - [x] 1.4 Implement `path(&self) -> &Path` — returns the resolved absolute file path
  - [x] 1.5 Implement `check_size_threshold(&self)` — reads file metadata, emits `tracing::warn!("Critic memory file exceeds {}KB threshold ({} bytes), consider manual review or summarization", threshold_kb, actual_size)` if over threshold; silently succeeds if file missing
  - [x] 1.6 Implement `prepare_context_path(&self) -> Option<String>` — calls `ensure_exists()`, on success returns `Some(path as string)`, on failure logs `tracing::warn!` and returns `None` (degraded mode)
  - [x] 1.7 Add `pub mod critic;` to `src/lib.rs`
  - [x] 1.8 Define `CriticMemoryError` with `thiserror` as a single-variant wrapper: `#[error("Critic memory I/O error: {0}")] Io(#[from] std::io::Error)`

- [x] Task 2: Add `critic_memory_threshold_kb` config field to `BotConfig` (AC: #3)
  - [x] 2.1 Add `critic_memory_threshold_kb: Option<u64>` to `BotConfig` with `#[serde(default, skip_serializing_if = "Option::is_none")]`
  - [x] 2.2 Default to `50` when `None` (apply default in `CriticMemory::new`, not serde)
  - [x] 2.3 Add commented field in `bmad-bot.yaml.example`: `# critic_memory_threshold_kb: 50  # Size threshold (KB) before warning about Critic memory file growth`
  - [x] 2.4 Update all `make_test_config()` helpers with `critic_memory_threshold_kb: None`

- [x] Task 3: Integrate `CriticMemory` into `StoryPipeline` (AC: #1, #3, #5)
  - [x] 3.1 Add `critic_memory: CriticMemory` field to `StoryPipeline` struct in `src/pipeline.rs`
  - [x] 3.2 Construct `CriticMemory` in `StoryPipeline::new()` using `config.bmad_paths.implementation_artifacts`, `config.bmad_paths.project_root`, and `config.critic_memory_threshold_kb.unwrap_or(50)`
  - [x] 3.3 In `build_create_story_consultations()`: call `self.critic_memory.prepare_context_path()` — if `Some(path)`, insert it as the **first** element of the critic consultation's `context_files` vec (before the story file path) so the Critic reads its memory first; if `None`, build configs without memory file (degraded mode, consultation still runs)
  - [x] 3.4 In `build_review_consultations()`: same pattern — `prepare_context_path()`, insert as first `context_files` element if available
  - [x] 3.5 Add `check_critic_memory_size(&self)` call in the pipeline phase runner methods (e.g., `run_create_story_phase`, `run_review_phase`) **after** each critic consultation completes, not in config builders — this is when the Critic may have appended new content

- [x] Task 4: Update critic preambles to reference memory file (AC: #2)
  - [x] 4.1 In `build_placeholder_critic_preamble()`: add a `## Memory` section with instructions:
    - "You have a persistent memory file (critic-memory.md) loaded in your context. It may be empty on your first invocation — this is normal."
    - "After completing your review, update your memory file using the edit_file tool:"
    - "1. Use read_file to read the current content of critic-memory.md"
    - "2. Use edit_file in overwrite mode to write the complete content: all existing content plus your new observation section appended at the end"
    - "Include in your new section: date, story key, review type (Story Review), key observations, and any cross-story patterns you notice."
    - "NEVER use edit_file in create mode on critic-memory.md — it already exists. Use overwrite mode to preserve and extend the full content."
  - [x] 4.2 In `build_review_critic_preamble()`: add similar `## Memory` section adapted for decision resolution context:
    - Same read_file → overwrite pattern
    - "Include in your new section: date, story key, review type (Decision Resolution), decisions made with rationale, and references to prior decisions if relevant."
  - [x] 4.3 Both preambles already use `ConsultationToolSet::Restricted` which includes `edit_file` — no tool set changes needed

- [x] Task 5: Tests (AC: #1, #3, #4, #5)
  - [x] 5.1 `test_critic_memory_creates_file_when_missing` — verify file creation with correct header containing `# Story Critic Memory` and a date
  - [x] 5.2 `test_critic_memory_idempotent_ensure_exists` — call `ensure_exists()` twice, file content unchanged after second call
  - [x] 5.3 `test_critic_memory_creates_parent_directories` — verify `ensure_exists()` creates missing parent dirs
  - [x] 5.4 `test_critic_memory_path_returns_correct_path` — verify absolute path resolution: `project_root/impl_artifacts/critic-memory.md`
  - [x] 5.5 `test_critic_memory_check_size_no_warning_under_threshold` — small file, no warning
  - [x] 5.6 `test_critic_memory_check_size_warns_over_threshold` — file > threshold emits warning
  - [x] 5.7 `test_critic_memory_missing_file_no_error` — `check_size_threshold()` on non-existent file succeeds silently
  - [x] 5.8 `test_critic_memory_config_default_threshold` — `None` config defaults to 50KB
  - [x] 5.9 `test_critic_memory_config_custom_threshold` — custom value respected
  - [x] 5.10 `test_critic_memory_prepare_context_path_returns_some` — successful prepare returns `Some(path_string)`
  - [x] 5.11 `test_critic_memory_prepare_context_path_returns_none_on_failure` — unwritable path returns `None` (degraded mode)
  - [x] 5.12 `test_pipeline_create_story_consultations_include_memory_path` — verify `build_create_story_consultations` output includes memory path as first element of critic consultation's `context_files`
  - [x] 5.13 `test_pipeline_review_consultations_include_memory_path` — verify `build_review_consultations` output includes memory path as first element of `context_files`
  - [x] 5.14 Verify `cargo clippy` passes with zero new warnings (use existing allowances: `-A clippy::needless_splitn -A clippy::unnecessary_map_or`)
  - [x] 5.15 Verify `cargo test` passes — baseline: 1193 passed, 1 pre-existing failure

### Review Findings

- [x] [Review][Patch] I/O side effects in config builders — moved `prepare_context_path()` to phase runners, builders now accept `Option<String>` parameter. [src/pipeline.rs]
- [x] [Review][Patch] Missing `///` doc comments on all public items in `src/critic/mod.rs` — added doc comments to all public types and methods. [src/critic/mod.rs]
- [x] [Review][Patch] `tracing::warn!` uses positional format args instead of structured fields — converted to structured `tracing` fields. [src/critic/mod.rs]
- [x] [Review][Patch] `check_size_threshold()` silently swallows ALL metadata errors — now distinguishes `NotFound` (silent) from other errors (logged at warn level). [src/critic/mod.rs]
- [x] [Review][Patch] Integer overflow: `threshold_kb * 1024` — replaced with `saturating_mul(1024)`. [src/critic/mod.rs]
- [x] [Review][Patch] `critic_memory_threshold_kb` not validated — added `> 0` check in `BotConfig::validate()`. [src/config/mod.rs]
- [x] [Review][Patch] No YAML deserialization test for `critic_memory_threshold_kb` — added 3 tests (default None, valid value, zero rejected). [src/config/mod.rs]
- [x] [Review][Patch] TOCTOU in `ensure_exists()` — replaced `exists()` + `write()` with atomic `OpenOptions::create_new(true)`, `AlreadyExists` fallback. [src/critic/mod.rs]
- [x] [Review][Defer] Tests don't verify `tracing::warn` output — no tracing-test pattern in project [src/critic/mod.rs] — deferred, consistent with existing test approach
- [x] [Review][Defer] `to_string_lossy()` on non-UTF-8 paths — pre-existing codebase pattern [src/critic/mod.rs:62] — deferred, pre-existing
- [x] [Review][Defer] `recover_and_process` skips `check_size_threshold` when code review disabled — WAL rework in Story 13.10 [src/pipeline.rs:2441] — deferred, addressed by upcoming story

## Dev Notes

### Architecture Compliance

- **Decision 11 (Story Critic):** This story implements the persistent memory subsystem. The Critic manages its own memory format — the daemon only creates the file, provides it as context, and checks size. No auto-truncation. [Source: architecture.md#Decision-11]
- **Decision 10 (Daemon-Orchestrated Consultations):** Memory file is passed as `context_files` to existing `ConsultationConfig` structs. No changes to consultation mechanism itself. [Source: architecture.md#Decision-10]
- **Error pattern:** `CriticMemoryError` is a thin `#[from] std::io::Error` wrapper — the struct only does filesystem I/O, so a multi-variant enum adds no value. [Source: architecture.md#Error-Type-Pattern]
- **Config pattern:** Optional field with `serde(default, skip_serializing_if)` — same pattern as `project_brief`, `reasoning_effort`, `base_url`. [Source: config/mod.rs]

### Critical Implementation Details

- **File location:** `Path::new(project_root).join(implementation_artifacts).join("critic-memory.md")` — resolve to absolute path at construction time. The `implementation_artifacts` field is relative (e.g., `_bmad-output/implementation-artifacts`).
- **Parent directory creation:** `ensure_exists()` must call `std::fs::create_dir_all` on the parent directory before writing. This handles fresh clones where the directory tree may not exist yet.
- **Not committed to git:** Architecture lists this as persistent but not committed. Already handled — implementation-artifacts is in `.gitignore`.
- **Critic manages its format:** The daemon ONLY creates the initial header. The Critic agent updates content via `edit_file` tool. Do NOT impose any markdown structure beyond the initial header.
- **Degraded mode:** If `ensure_exists()` fails (permissions, disk), log a warning and proceed without memory. `build_context_xml` (consultation.rs:310-314) returns `Err(ContextFileNotFound)` for missing files — so the memory path must only be added to `context_files` when the file is confirmed to exist. The `prepare_context_path()` method encapsulates this: ensure + return path on success, warn + return None on failure.
- **No side effects in config builders:** `build_create_story_consultations` and `build_review_consultations` currently return `Vec<ConsultationConfig>` with no I/O. Keep them pure — call `prepare_context_path()` to get the path string, then pass it into the config builder. The size threshold check goes in the phase runner methods after consultations complete.
- **Size threshold timing:** Check size _after_ the consultation completes (in the phase runner), not before. This catches the growth from the just-completed Critic invocation rather than being one cycle behind.
- **Preamble updates are minimal:** Story 13.9 will do the full preamble rewrite with Critic identity. Here, just add a `## Memory` section with explicit edit_file usage instructions.
- **`LlmRole` unchanged:** Critic consultations currently use `LlmRole::Review`. Adding `LlmRole::Critic` is Story 13.9's scope. Do NOT add it here.
- **`ConsultationToolSet::Restricted` already includes `edit_file`** (see `consultation.rs:28-30`). No tool set changes needed.
- **`project_brief` context injection is deferred to Story 13.9.** This story only adds critic-memory.md to context_files. The project brief (or PRD fallback) will be added when the full Critic agent construction is implemented.
- **`context_files` ordering:** Memory file goes first, story file second. The Critic should read its history before reviewing the artifact. `build_context_xml` iterates `context_files` in order and the ContextBuilder preserves insertion order.

### edit_file Append Pattern for Preamble

The `edit_file` tool has three modes: `edit` (search-replace), `create` (new file), `overwrite` (full rewrite). None is a native "append" operation. The preamble must instruct the Critic to:
1. `read_file` to get current content
2. `edit_file` with `mode: "overwrite"` to write the full content (existing + new section)

Do NOT instruct the Critic to use `edit` mode for appending — finding a reliable search anchor at the end of a free-format file is fragile. Do NOT use `create` mode — the file already exists and `create` will error.

### Source Tree Components to Touch

| File | Change |
|------|--------|
| `src/critic/mod.rs` | **NEW** — `CriticMemory` struct, error type, file lifecycle methods |
| `src/lib.rs` | Add `pub mod critic;` |
| `src/config/mod.rs` | Add `critic_memory_threshold_kb: Option<u64>` field + serde attrs |
| `src/pipeline.rs` | Add `critic_memory` field to `StoryPipeline`, wire into consultation builders and phase runners |
| `bmad-bot.yaml.example` | Add commented `critic_memory_threshold_kb` field |

### Anti-Patterns to Avoid

- DO NOT add `LlmRole::Critic` — that is Story 13.9
- DO NOT modify `ConsultationRunner`, `ConsultationConfig`, or `ConsultationToolSet` — infrastructure is already complete
- DO NOT impose a rigid markdown template for memory entries — the Critic decides its own format
- DO NOT auto-truncate or rotate the memory file — human-only decision
- DO NOT use `println!` — daemon code uses `tracing` only
- DO NOT write the `ContextBuilder` integration — `context_files` is already read as XML context by `ConsultationRunner::build_context_xml()` (consultation.rs:306-320)
- DO NOT modify `consultation.rs` — all integration goes through pipeline.rs consultation config builders and phase runners
- DO NOT add `project_brief` to context_files — deferred to Story 13.9
- DO NOT let `ensure_exists()` failure crash the consultation — always degrade gracefully
- DO NOT add I/O side effects inside config builder methods — keep them pure, do I/O in phase runners or via `prepare_context_path()`

### Previous Story Intelligence (Story 13.7)

- **Test baseline:** 1193 passed, 1 pre-existing failure
- **Clippy allowances:** `-A clippy::needless_splitn -A clippy::unnecessary_map_or`
- **Config pattern established:** `project_brief: Option<String>` with `#[serde(default, skip_serializing_if = "Option::is_none")]` — follow same pattern for `critic_memory_threshold_kb`
- **Test config helpers:** 6 `make_test_config()` functions across modules need `critic_memory_threshold_kb: None` added: `src/config/mod.rs`, `src/watcher/mod.rs`, `src/llm/agent_factory.rs`, `src/review/epic.rs`, `src/pipeline.rs`, `src/session/runner.rs`
- **File list from 13.7:** `src/config/mod.rs`, `src/cli/mod.rs`, `bmad-bot.yaml.example`, `src/watcher/mod.rs`, `src/llm/agent_factory.rs`, `src/review/epic.rs`, `src/pipeline.rs`, `src/session/runner.rs`
- **Deferred concern from 13.7 review:** "Config nesting for critic settings — marked for future architectural review when Stories 13.8/13.9 add more critic settings." For this story, a flat `critic_memory_threshold_kb` field is fine — nesting decision can be revisited in 13.9 when `LlmRole::Critic` config is added.

### Git Intelligence

Recent commits follow pattern: `feat(epic-13): description (Story 13.X)`
- `cedf83b` Story 13.7: added `project_brief` config field
- `21761e0` Story 13.6: unified code-review phase with critic consultation
- `b68fc0d` Story 13.5: separated dev/review phases
- `5f4a497` Story 13.4: create-story phase with consultations
- `63932ed` Story 13.3: daemon-orchestrated consultation mechanism

### Testing Standards

- Framework: `#[tokio::test]` for async, `#[test]` for sync
- Naming: `test_{module}_{behavior}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert
- Use `tempdir` or `tempfile` crate for filesystem tests (check if already in `Cargo.toml`, otherwise use `std::env::temp_dir()`)
- `NullRenderer` for UI in tests
- Mock all external dependencies

### Project Structure Notes

- New `src/critic/` module follows existing module pattern (e.g., `src/notifier/`, `src/mcp/`)
- `CriticMemory` is a lightweight struct — no trait needed since there's only one implementation
- Module is small enough for a single `mod.rs` file (no sub-files needed)

### References

- [Source: architecture.md#Decision-11] Story Critic design: persistent memory, no auto-truncation, critic-managed format
- [Source: architecture.md#Decision-10] Daemon-orchestrated consultations: context_files mechanism
- [Source: architecture.md#Error-Type-Pattern] Module-specific error enums with thiserror
- [Source: architecture.md#Configuration-Files] critic-memory.md listed as persistent, not committed
- [Source: pipeline.rs:1394-1464] Consultation config builders — integration point
- [Source: pipeline.rs:2954-2988] Current critic preambles — add memory instructions
- [Source: consultation.rs:24-31] ConsultationToolSet::Restricted includes edit_file for critic memory
- [Source: consultation.rs:306-320] build_context_xml returns Err(ContextFileNotFound) for missing files — memory path must only be in context_files when file exists
- [Source: config/mod.rs:120-130] project_brief field — pattern to follow for new config field
- [Source: tools/edit_file.rs:36-45] EditFileToolArgs — modes: edit (search-replace), create, overwrite

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

N/A

### Completion Notes List

- Created `src/critic/mod.rs` with `CriticMemory` struct, `CriticMemoryError`, and all lifecycle methods (ensure_exists, path, check_size_threshold, prepare_context_path)
- Added `critic_memory_threshold_kb: Option<u64>` config field to `BotConfig` following the `project_brief` pattern
- Integrated `CriticMemory` into `StoryPipeline`: field, constructor, and wired into both `build_create_story_consultations` and `build_review_consultations` with memory path as first `context_files` element
- Added `check_size_threshold()` calls after consultation completion in both `run_create_pipeline` and `run_review_pipeline`
- Updated both critic preambles (`build_placeholder_critic_preamble`, `build_review_critic_preamble`) with `## Memory` section containing read_file → edit_file overwrite instructions
- Added `mod critic;` to both `src/main.rs` and `src/lib.rs`
- Updated 7 `make_test_config()` helpers across modules with `critic_memory_threshold_kb: None`
- Added commented example in `bmad-bot.yaml.example`
- 15 new tests: 11 unit tests in `src/critic/mod.rs` + 4 integration tests in `src/pipeline.rs` (including 2 preamble assertion tests)
- Test results: 1208 passed, 1 failed (pre-existing `test_build_context_limit_recovery_message_contains_all_sections`)
- No new clippy warnings introduced

### Change Log

- 2026-04-24: Implemented Story 13.8 — Critic Memory System with persistent memory file, config integration, pipeline wiring, preamble updates, and comprehensive tests

### File List

- src/critic/mod.rs (NEW)
- src/lib.rs (MODIFIED)
- src/main.rs (MODIFIED)
- src/config/mod.rs (MODIFIED)
- src/cli/mod.rs (MODIFIED)
- src/pipeline.rs (MODIFIED)
- src/watcher/mod.rs (MODIFIED)
- src/llm/agent_factory.rs (MODIFIED)
- src/review/epic.rs (MODIFIED)
- src/session/runner.rs (MODIFIED)
- bmad-bot.yaml.example (MODIFIED)
