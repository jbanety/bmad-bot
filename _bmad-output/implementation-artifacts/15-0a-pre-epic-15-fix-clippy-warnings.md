# Story 15.0a: Fix Clippy Warnings and Stale Test

Status: done

## Story

As a daemon developer,
I want all clippy warnings and the pre-existing test failure resolved,
So that the codebase compiles cleanly under `cargo clippy -- -D warnings` and the full test suite passes with zero failures.

## Acceptance Criteria

1. **Given** `cargo clippy -- -D warnings` is run **When** this story is complete **Then** clippy reports zero errors for the 5 issues identified below (other pre-existing dead-code warnings are NOT in scope — they require `#![warn(dead_code)]` → `#![deny(dead_code)]` migration, which is a separate effort)

2. **Given** `cargo test` is run **When** this story is complete **Then** all tests pass including the previously failing `test_build_context_limit_recovery_message_contains_all_sections` **And** no new test failures are introduced

3. **Given** the unused import `DeferredItemRef` at `src/pipeline.rs:29` **When** the fix is applied **Then** either the import is removed OR the full path usage at line 3483 is replaced with the imported name — both approaches are acceptable, but the import warning must be resolved

## Tasks / Subtasks

- [x] Task 1: Fix redundant pattern matching in `purge_deferred_items()` (AC: #1)
  - [x] 1.1 In `src/pipeline.rs:3513`, change `} else if let Some(_) = &current_heading {` to `} else if current_heading.is_some() {`. This is the exact fix clippy suggests. The semantics are identical — the binding `_` is never used

- [x] Task 2: Fix unused import `DeferredItemRef` in pipeline.rs (AC: #1, #3)
  - [x] 2.1 In `src/pipeline.rs:3483`, change `refs: &[crate::review::epic::DeferredItemRef]` to `refs: &[DeferredItemRef]`. The import at line 29 already brings `DeferredItemRef` into scope — the full path is redundant and causes the unused-import warning. Do NOT remove the import; use it instead
  - [x] 2.2 Verify no other occurrence of `crate::review::epic::DeferredItemRef` exists in non-test code (test code at line 6493 has its own `use` statement and is fine)

- [x] Task 3: Fix needless `splitn` in `session/branch.rs` (AC: #1)
  - [x] 3.1 In `src/session/branch.rs:126`, change `last_dep.splitn(2, '-').next()` to `last_dep.split('-').next()`. When only the first element is consumed via `.next()`, `splitn(2, ...)` is equivalent to `split(...)` — clippy lint `needless_splitn`

- [x] Task 4: Fix `map_or` → `is_some_and` in `session/branch.rs` (AC: #1)
  - [x] 4.1 In `src/session/branch.rs:130`, change `dep_epic_num.map_or(false, |dep_epic| dep_epic != story.epic_num)` to `dep_epic_num.is_some_and(|dep_epic| dep_epic != story.epic_num)`. This is the exact replacement clippy suggests — clearer intent with no semantic change

- [x] Task 5: Fix stale test `test_build_context_limit_recovery_message_contains_all_sections` (AC: #2)
  - [x] 5.1 In `src/session/runner.rs:3556`, the assertion `assert!(msg.contains("summary text"), "Should contain the summary")` fails because `build_context_limit_recovery_message()` (lines 1100-1123) no longer takes or includes a summary parameter — it only takes `story` and `formatted_exchanges`. The function was refactored to remove the summary but the test was not updated. **Fix:** Remove this single assertion (line 3556). The remaining assertions in the test (`SESSION RECOVERY`, `Context Window Limit Reached`, `exchange text`, `Current Story`) are still valid against the current function implementation
  - [x] 5.2 Verify that adjacent tests in the same function group (`test_build_context_limit_recovery_message_includes_story_path`, `test_build_context_limit_recovery_message_does_not_contain_project_context`, `test_build_context_limit_recovery_message_reason`) still pass — they should be unaffected

- [x] Task 6: Verify clean build (AC: #1, #2)
  - [x] 6.1 Run `cargo clippy -- -D warnings` — the 5 targeted errors must be resolved. Pre-existing dead-code warnings (32 total) will still appear as warnings because the crate uses `#![warn(dead_code)]`, not `#![deny(dead_code)]` — this is expected and out of scope
  - [x] 6.2 Run `cargo test` — all tests must pass (1310 passed, 0 failed). The previously failing test is now fixed
  - [x] 6.3 Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Root Cause Analysis

All 5 issues are pre-existing technical debt that accumulated across epics 13-14:

| Issue | File | Introduced | Root Cause |
|-------|------|------------|------------|
| Redundant pattern matching | `pipeline.rs:3513` | Story 14.4 | LLM agent used verbose pattern instead of `is_some()` |
| Unused import | `pipeline.rs:29,3483` | Story 14.4 | `purge_deferred_items()` uses full path instead of imported name |
| Needless splitn | `session/branch.rs:126` | Epic 4 | Pre-existing, surfaced by newer clippy lint |
| map_or → is_some_and | `session/branch.rs:130` | Epic 4 | Pre-existing, `is_some_and` stabilized in Rust 1.70 |
| Stale test assertion | `session/runner.rs:3556` | Epic 6 | Function refactored to remove `summary` param, test not updated |

### This Is a Mechanical Fix — No Design Decisions

Every change is a direct substitution suggested by clippy or removing a stale assertion. No architectural decisions, no new code paths, no behavior changes. Each fix can be verified by reading the before/after diff.

### Exact Changes Summary

```
src/pipeline.rs:3513    if let Some(_) = &current_heading  →  if current_heading.is_some()
src/pipeline.rs:3483    refs: &[crate::review::epic::DeferredItemRef]  →  refs: &[DeferredItemRef]
src/session/branch.rs:126  last_dep.splitn(2, '-').next()  →  last_dep.split('-').next()
src/session/branch.rs:130  dep_epic_num.map_or(false, |dep_epic| ...)  →  dep_epic_num.is_some_and(|dep_epic| ...)
src/session/runner.rs:3556  DELETE: assert!(msg.contains("summary text"), "Should contain the summary");
```

### Anti-Patterns to Avoid

- Do NOT fix the 32 dead-code warnings — they require changing `#![warn(dead_code)]` to `#![deny(dead_code)]` in `src/main.rs:2`, which is a larger cleanup task
- Do NOT modify any behavior — these are all syntactic/cosmetic fixes
- Do NOT add new tests — the existing test suite covers all modified code paths
- Do NOT refactor surrounding code — fix only the 5 identified issues

### Previous Story Intelligence

Story 14.4 (Purge Processed Deferred Items) was the last completed story. Key context:
- `purge_deferred_items()` at `pipeline.rs:3481-3590` is where the redundant pattern matching lives
- The `DeferredItemRef` import issue is in the same import block as other Epic 14 additions
- Test count before this story: 1309 passed, 1 failed (the stale test we're fixing)

### Git Intelligence

Last commit: `61b0acd feat(epic-14): purge resolved deferred items on pre-epic story completion (Story 14.4)`
Convention: `feat(epic-N): description (Story N.M)` — for this pre-epic story use: `fix(pre-epic-15): resolve clippy warnings and stale test (Story 15.0a)`

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Zero-warning policy: `#![deny(clippy::all)]` at crate root
- All tests inline in their respective modules
- No new tests needed — existing tests validate the unchanged behavior

### Project Structure Notes

Files to modify:
- `src/pipeline.rs` — 2 changes (lines 3483, 3513)
- `src/session/branch.rs` — 2 changes (lines 126, 130)
- `src/session/runner.rs` — 1 change (line 3556, remove assertion)

Files NOT to modify:
- `src/main.rs` — do not change `#![warn(dead_code)]`
- `src/review/epic.rs` — no changes needed
- Any test helpers or configuration files

### References

- [Source: Epic 14 Review Report — Section 3: Technical Analysis, Codebase Health]
- [Source: `cargo clippy -- -D warnings` output — 5 targeted errors]
- [Source: `src/pipeline.rs:3513` — redundant pattern matching in `purge_deferred_items()`]
- [Source: `src/pipeline.rs:29,3483` — unused import / full path usage]
- [Source: `src/session/branch.rs:126,130` — needless splitn, unnecessary map_or]
- [Source: `src/session/runner.rs:1100-1123` — `build_context_limit_recovery_message()` current implementation (no summary param)]
- [Source: `src/session/runner.rs:3544-3565` — stale test with "summary text" assertion]
- [Source: `_bmad-output/project-context.md` — project rules and conventions]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

- All 5 clippy fixes applied as single-line substitutions per story spec
- `cargo clippy` confirms zero non-dead-code errors remaining
- `cargo test`: 1310 passed, 0 failed (previously 1309 passed, 1 failed)
- `cargo fmt --check`: pre-existing formatting diffs in unrelated code (not introduced by this story)
- Dead-code warnings (31 total) remain as expected — out of scope per story spec

### Completion Notes List

- Task 1: Changed `if let Some(_) = &current_heading` → `if current_heading.is_some()` in pipeline.rs:3513
- Task 2: Changed `refs: &[crate::review::epic::DeferredItemRef]` → `refs: &[DeferredItemRef]` in pipeline.rs:3483. Confirmed only test code (line 6493) has remaining full-path usage
- Task 3: Changed `splitn(2, '-').next()` → `split('-').next()` in session/branch.rs:126
- Task 4: Changed `map_or(false, ...)` → `is_some_and(...)` in session/branch.rs:130
- Task 5: Removed stale assertion `assert!(msg.contains("summary text"), ...)` from session/runner.rs:3556. All 5 adjacent tests pass
- Task 6: Verified clippy (0 targeted errors), tests (1310/0), fmt (pre-existing only)

### Change Log

- 2026-04-26: Applied 5 mechanical fixes — 4 clippy lint resolutions + 1 stale test assertion removal

### Review Findings

- [x] [Review][Defer] Silent skip of non-parseable dependency keys [src/session/branch.rs:126] — deferred, pre-existing
- [x] [Review][Defer] Potential panic on empty `dependencies` vector [src/session/branch.rs:123] — deferred, pre-existing

### File List

- `src/pipeline.rs` — 2 changes (lines 3483, 3513)
- `src/session/branch.rs` — 2 changes (lines 126, 130)
- `src/session/runner.rs` — 1 change (line 3556, removed stale assertion)
