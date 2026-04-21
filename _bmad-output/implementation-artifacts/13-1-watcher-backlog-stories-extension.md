# Story 13.1: Watcher Extension — Backlog Stories Eligible

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the watcher to detect stories in `backlog` status in addition to `ready-for-dev` and `review`,
So that the pipeline can pick up stories at the very beginning of their lifecycle and run the full create-dev-review flow.

## Acceptance Criteria

1. **AC-1: `eligible_stories()` returns stories with status `backlog`, `ready-for-dev`, or `review`**
   - **Given** `src/watcher/mod.rs` currently filters stories to only `ready-for-dev` status via `StoryInfo::is_eligible()`
   - **When** this story is implemented
   - **Then** `eligible_stories()` returns stories with status `backlog`, `ready-for-dev`, or `review`
   - **And** the returned `StoryInfo` includes the `status` field (already present) so the pipeline can route accordingly

2. **AC-2: Dependency rules apply identically to all eligible statuses**
   - **Given** the dependency resolution in `src/watcher/deps.rs`
   - **When** a `backlog` or `review` story's dependencies are evaluated
   - **Then** the same dependency rules apply: a story is only eligible if all its dependencies are resolved (`done`, `superseded`, `absorbed`)
   - **And** cascade blocking applies identically — if a prerequisite has a blocking status (`blocked`, `needs-clarification`), dependent stories are excluded regardless of their own status

3. **AC-3: Status-based priority ordering**
   - **Given** the watcher returns multiple eligible stories with mixed statuses
   - **When** the pipeline selects the next story to process
   - **Then** stories are prioritized: `review` first (resume interrupted work), then `ready-for-dev` (resume after create), then `backlog` (start fresh)
   - **And** within each status group, document-order topo sort applies as before
   - **And** only one story is processed at a time — the pipeline re-polls after each story completes

4. **AC-4: Backward-compatible pipeline guard**
   - **Given** Story 13.2 (Pipeline Orchestrator Refonte) has not yet been implemented
   - **When** the watcher returns stories with statuses the pipeline cannot yet handle
   - **Then** a guard function filters stories to only `ready-for-dev` before the pipeline processes them
   - **And** the guard is applied at both consumption points: the initial `stories` parameter in `process_eligible_stories()` AND the `current_stories` reassignment after each `re_poll_eligible()` call
   - **And** a `tracing::info!` log notes any `backlog` or `review` stories that were eligible but filtered out, with count and story keys
   - **And** when all eligible stories are filtered out by the guard, a distinct `tracing::info!` log says "All eligible stories require pipeline phase routing (Story 13.2) — none processable in current pipeline"
   - **And** once 13.2 removes this guard, all eligible statuses flow through

5. **AC-5: Tests**
   - **Given** the watcher module has existing comprehensive unit tests
   - **When** this story is implemented
   - **Then** new tests verify:
     - `is_eligible()` returns `true` for `backlog`, `ready-for-dev`, and `review`
     - `is_eligible()` returns `false` for `done`, `in-progress`, `blocked`, `needs-clarification`
     - `eligible_stories()` returns all three status types in document order
     - Priority sorting: `review` stories appear before `ready-for-dev`, which appear before `backlog` (tested via `filter_eligible()`, not `eligible_stories()`)
     - The backward-compatible guard filters to `ready-for-dev` only (unit test in `pipeline.rs` tests)
     - Dependency resolution works for `backlog` stories (same rules as `ready-for-dev`)
   - **And** all existing watcher and deps tests continue to pass (with updates to tests broken by the eligibility expansion — see Broken Tests Inventory)
   - **And** `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` passes

## Tasks / Subtasks

- [x] Task 1: Expand `StoryInfo::is_eligible()` to accept three statuses (AC: #1)
  - [x] 1.1 In `src/watcher/mod.rs`, change `is_eligible()` from:
    ```rust
    pub fn is_eligible(&self) -> bool {
        self.status == "ready-for-dev"
    }
    ```
    to:
    ```rust
    pub fn is_eligible(&self) -> bool {
        matches!(self.status.as_str(), "backlog" | "ready-for-dev" | "review")
    }
    ```
  - [x] 1.2 Update the doc comment on `is_eligible()` to reflect the three accepted statuses.
  - [x] 1.3 Update `WatcherError::NoEligibleStories` message from `"No eligible stories found (all stories are either done, in-progress, or backlog)"` to `"No eligible stories found"` (backlog is now eligible, message was misleading).

- [x] Task 2: Add status-priority sorting in `filter_eligible()` (AC: #3)
  - [x] 2.1 In `src/watcher/deps.rs`, add a module-private helper function:
    ```rust
    fn status_priority(status: &str) -> u8 {
        match status {
            "review" => 0,
            "ready-for-dev" => 1,
            "backlog" => 2,
            _ => 3,
        }
    }
    ```
  - [x] 2.2 In `filter_eligible()`, apply `eligible.sort_by_key(|s| status_priority(&s.status))` as the LAST step, immediately before the `Ok((eligible, cascade_count))` return on line 752. Use `sort_by_key` (stable sort) to preserve topo-sort order within each status group.
  - [x] 2.3 This placement is mandatory — NOT in `Watcher::poll()`. Placing it in `filter_eligible()` ensures both `poll()` and `re_poll_eligible()` in `pipeline.rs` get priority-sorted results without duplication.

- [x] Task 3: Add backward-compatible pipeline guard (AC: #4)
  - [x] 3.1 In `src/pipeline.rs`, add a private helper function:
    ```rust
    /// Temporary guard: filters stories to only those the current pipeline can process.
    /// Remove this function when Story 13.2 implements multi-phase pipeline routing.
    fn guard_processable_stories(stories: Vec<StoryInfo>) -> Vec<StoryInfo> {
        let mut skipped_count = 0;
        for s in &stories {
            if s.status != "ready-for-dev" {
                skipped_count += 1;
                tracing::info!(
                    story_key = %s.story_key,
                    status = %s.status,
                    "Story eligible but skipped — pipeline phase routing not yet implemented (Story 13.2)"
                );
            }
        }
        let processable: Vec<StoryInfo> = stories
            .into_iter()
            .filter(|s| s.status == "ready-for-dev")
            .collect();
        if processable.is_empty() && skipped_count > 0 {
            tracing::info!(
                skipped = skipped_count,
                "All eligible stories require pipeline phase routing (Story 13.2) — none processable in current pipeline"
            );
        }
        processable
    }
    ```
  - [x] 3.2 In `process_eligible_stories()`, apply the guard to the initial `stories` parameter at the top of the method, BEFORE entering the loop:
    ```rust
    let mut current_stories = guard_processable_stories(stories);
    ```
  - [x] 3.3 In `process_eligible_stories()`, apply the guard after EVERY `re_poll_eligible()` call inside the loop. The re-poll reassignment (around line 1026) currently does:
    ```rust
    current_stories = fresh_stories;
    ```
    Change to:
    ```rust
    current_stories = guard_processable_stories(fresh_stories);
    ```
  - [x] 3.4 **Both application points are required.** The initial list and every re-polled list must be guarded. Missing either one allows non-`ready-for-dev` stories to reach `process_story()`.

- [x] Task 4: Update `Watcher::poll()` logging (AC: #1)
  - [x] 4.1 The existing `tracing::info!` in `poll()` logs `eligible_count` BEFORE `filter_eligible()` runs. Move the status breakdown log to AFTER `filter_eligible()` returns (the `filtered` variable), so counts reflect what the pipeline actually receives:
    ```rust
    tracing::info!(
        total_stories = all_stories.len(),
        pre_filter_eligible = eligible.len(),
        post_filter_eligible = filtered.len(),
        cascade_blocked = cascade_count,
        backlog = filtered.iter().filter(|s| s.status == "backlog").count(),
        ready_for_dev = filtered.iter().filter(|s| s.status == "ready-for-dev").count(),
        review = filtered.iter().filter(|s| s.status == "review").count(),
        "Sprint status polled"
    );
    ```
  - [x] 4.2 Remove or consolidate the existing two `tracing::info!` calls in `poll()` (lines 416-419 and 431-436) into the single log above to avoid redundant output.

- [x] Task 5: Update broken existing tests and add new tests (AC: #5)
  - [x] 5.1 **Broken Tests Inventory — tests that WILL fail after `is_eligible()` changes:**
    - `test_story_info_is_not_eligible_backlog` (line 539) — asserts `!info.is_eligible()` for `"backlog"`. **Fix:** flip to `assert!(info.is_eligible())` and rename to `test_story_info_is_eligible_backlog`.
    - `test_sprint_status_eligible_stories_empty_when_none_ready` (line 781) — creates statuses `done`, `in-progress`, `backlog` and asserts `eligible.is_empty()`. After the change, the `backlog` story (`1-3-init`) becomes eligible. **Fix:** change the `backlog` status to `in-progress` in the test fixture so the test still validates "no eligible stories" behavior, OR update the assertion to `assert_eq!(eligible.len(), 1)` and verify the eligible story is the backlog one.
    - `test_watcher_poll_returns_no_eligible_stories_error` (line 856) — creates statuses `done`, `in-progress` only (no `backlog`), so this test is SAFE. Verify by reading: it has `1-1: done`, `1-2: in-progress` — no backlog story. **No change needed.**
  - [x] 5.2 In `src/watcher/mod.rs` test module, add new eligibility tests:
    - `test_story_info_is_eligible_review` — status `"review"` returns `is_eligible() == true`
    - `test_story_info_is_not_eligible_in_progress` — status `"in-progress"` returns `false`
    - `test_story_info_is_not_eligible_blocked` — status `"blocked"` returns `false`
  - [x] 5.3 Update `test_sprint_status_eligible_stories_filters_ready_for_dev` (line 742) — rename to `test_sprint_status_eligible_stories_filters_actionable_statuses`. Add `backlog` and `review` status stories to the fixture. Verify all three are returned.
  - [x] 5.4 Add `test_sprint_status_eligible_stories_excludes_in_progress_and_done` — verify these are NOT returned.
  - [x] 5.5 Add priority sorting tests **in `src/watcher/deps.rs` test module** (NOT in `mod.rs` — priority sort lives in `filter_eligible()`). These tests must call `filter_eligible()` to exercise the sort:
    - `test_filter_eligible_priority_review_before_ready_for_dev` — construct a `Vec<StoryInfo>` with two independent stories (no deps): first `ready-for-dev`, second `review`. Construct matching `all_statuses` entries with preceding dependencies as `done`. Call `filter_eligible()`. Assert the `review` story comes first in the result.
    - `test_filter_eligible_priority_ready_for_dev_before_backlog` — similar setup with `ready-for-dev` and `backlog`.
    - `test_filter_eligible_priority_preserves_order_within_group` — two `ready-for-dev` stories in document order. After `filter_eligible()`, same order preserved.
    Example test skeleton:
    ```rust
    #[test]
    fn test_filter_eligible_priority_review_before_ready_for_dev() {
        // Two independent stories in separate epics (no sequential deps)
        let all_statuses = vec![
            ("epic-1".to_string(), "done".to_string()),
            ("1-1-foo".to_string(), "done".to_string()),
            ("epic-2".to_string(), "done".to_string()),
            ("2-1-bar".to_string(), "ready-for-dev".to_string()),
            ("epic-3".to_string(), "done".to_string()),
            ("3-1-baz".to_string(), "review".to_string()),
        ];
        let stories = vec![
            make_story("2-1-bar", "ready-for-dev"),
            make_story("3-1-baz", "review"),
        ];
        let comment_deps = HashMap::new();
        let (result, _) = filter_eligible(stories, &all_statuses, &comment_deps).unwrap();
        assert_eq!(result[0].story_key, "3-1-baz", "review must come before ready-for-dev");
        assert_eq!(result[1].story_key, "2-1-bar");
    }
    ```
  - [x] 5.6 Add `test_backlog_story_deps_enforced` in `src/watcher/deps.rs` tests — a `backlog` story whose dependency is `in-progress` is filtered out by `filter_eligible()`.
  - [x] 5.7 Add `test_guard_processable_stories_filters_non_ready_for_dev` in `src/pipeline.rs` tests:
    ```rust
    #[test]
    fn test_guard_processable_stories_filters_non_ready_for_dev() {
        let stories = vec![
            StoryInfo::from_key_and_status("1-1-foo", "backlog", Path::new("/tmp")).unwrap(),
            StoryInfo::from_key_and_status("2-1-bar", "ready-for-dev", Path::new("/tmp")).unwrap(),
            StoryInfo::from_key_and_status("3-1-baz", "review", Path::new("/tmp")).unwrap(),
        ];
        let result = guard_processable_stories(stories);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].story_key, "2-1-bar");
    }
    ```
  - [x] 5.8 Add `test_guard_processable_stories_returns_empty_when_all_non_ready` — all stories are `backlog`/`review`, result is empty.

- [x] Task 6: Verify full test suite (AC: #5)
  - [x] 6.1 `cargo build` — zero new warnings
  - [x] 6.2 `cargo clippy --all-targets -- -D warnings -A clippy::needless_splitn -A clippy::unnecessary_map_or` — no new warnings introduced (46 pre-existing clippy errors from config/mod.rs and pipeline.rs dead code; none from Story 13.1 changes)
  - [x] 6.3 `cargo test` — 1162 passed, 1 failed (pre-existing `test_build_context_limit_recovery_message_contains_all_sections`)
  - [x] 6.4 Delta: 10 new tests added, 2 existing tests modified (1 flipped assertion + rename, 1 fixture updated). Final count: 1162 (up from 1152 baseline).

## Dev Notes

### Architecture Compliance

- **Decision 2 (Daemon Reads, Agent Writes):** This story does NOT change the daemon's read-only contract. The watcher remains a pure reader of `sprint-status.yaml`. No writes are introduced.
- **Decision 1 (Supervisor Interception / Chat Loop):** Unchanged. The watcher changes are upstream of the session.
- **Error handling pattern:** Per-module `thiserror` enums. `WatcherError::NoEligibleStories` message updated but variant unchanged.
- **Testing pattern:** Inline `#[cfg(test)] mod tests`, descriptive snake_case, Arrange-Act-Assert.

### Critical Implementation Details

**`StoryInfo::is_eligible()` change scope:** This is the ONLY gate function. Both `eligible_stories()` and `Watcher::poll()` delegate to it. Changing it from `== "ready-for-dev"` to `matches!("backlog" | "ready-for-dev" | "review")` propagates everywhere.

**`filter_eligible()` dependency logic is unchanged:** The dependency resolution in `deps.rs` does not check the status of the stories being filtered — it checks the status of their DEPENDENCIES. A story is eligible if its dependencies are resolved (`done`, `superseded`, `absorbed`) and it is not cascade-blocked. The story's own status is irrelevant to dependency resolution. This means `backlog` and `review` stories pass through the existing dependency logic without changes.

**`filter_eligible()` gets one new addition — priority sort:** A `status_priority()` helper and a `sort_by_key` call are added at the end of `filter_eligible()`. This is a modification to `deps.rs`, but it does NOT touch the core dependency logic (adjacency graph, cycle detection, cascade blocking, topo sort). It is a post-processing step on the already-filtered result.

**Priority sorting placement is `filter_eligible()` — not `poll()`.** This is a deterministic choice. Both `Watcher::poll()` and `re_poll_eligible()` in `pipeline.rs` call `filter_eligible()`, so placing the sort there applies it universally without duplication.

**The `re_poll_eligible()` function in `pipeline.rs`** calls `sprint_status.eligible_stories()` and then `watcher_deps::filter_eligible()`. It will automatically pick up the new statuses and priority sort. The backward-compatible guard is applied by the caller (`process_eligible_stories()`), not inside `re_poll_eligible()`, to keep the re-poll function clean and ready for 13.2.

**Call chain verification:** `Watcher::poll()` is called from `run_polling_loop()` in `cli/mod.rs`. The return value is passed to `pipeline.process_eligible_stories()`. The guard function is applied inside `process_eligible_stories()`, which is the single entry point for all story processing. No other code path consumes `poll()` results directly.

### Backward Compatibility — Pipeline Guard (AC-4)

Until Story 13.2 implements multi-phase pipeline routing, the current `process_story()` can only handle `ready-for-dev` stories. If a `backlog` story is processed:
- There is no story spec file yet (`{key}.md` does not exist) — the session would fail trying to load it
- The agent would try to run `dev-story` on a story that hasn't been through `create-story`

**Guard strategy:** A `guard_processable_stories()` function filters eligible stories to `ready-for-dev` only. It is called in TWO places inside `process_eligible_stories()`:
1. On the initial `stories` parameter before entering the loop
2. On every `fresh_stories` result from `re_poll_eligible()` inside the loop

**Distinct logging for "all guarded out" scenario:** When all eligible stories are non-`ready-for-dev` (e.g., only `backlog` stories remain after an epic completes), the guard logs a specific message: "All eligible stories require pipeline phase routing (Story 13.2) — none processable in current pipeline". This distinguishes from the "no eligible stories" scenario where the watcher itself returns empty.

**Why not guard in `eligible_stories()`?** Because the watcher should expose the full picture. The pipeline decides what it can process. This respects the separation of concerns and makes the 13.2 change a pipeline-only modification (remove the guard function and its two call sites).

### Broken Tests Inventory

These existing tests WILL fail after `is_eligible()` changes. Each must be updated:

| Test | File:Line | Why It Breaks | Fix |
|---|---|---|---|
| `test_story_info_is_not_eligible_backlog` | `mod.rs:539` | Asserts `!is_eligible()` for `"backlog"` — now eligible | Flip assertion, rename to `test_story_info_is_eligible_backlog` |
| `test_sprint_status_eligible_stories_empty_when_none_ready` | `mod.rs:781` | Fixture has `1-3-init: backlog` — now eligible, `eligible.is_empty()` fails | Change fixture status from `backlog` to `in-progress`, OR update assertion to `len() == 1` |

Tests that are SAFE (verified by reading fixtures):
- `test_watcher_poll_returns_no_eligible_stories_error` — fixture has only `done` and `in-progress`, no `backlog`
- `test_sprint_status_eligible_stories_excludes_needs_clarification` — fixture has `needs-clarification`, not `backlog`

### Files to Modify

| File | Change Type | Scope |
|---|---|---|
| `src/watcher/mod.rs` | **Modify** | `is_eligible()` — accept 3 statuses; `NoEligibleStories` error message; `poll()` logging; update 2 existing tests + add new tests |
| `src/watcher/deps.rs` | **Modify** | Add `status_priority()` helper; add `sort_by_key` call at end of `filter_eligible()`; add priority sorting tests |
| `src/pipeline.rs` | **Modify** | Add `guard_processable_stories()` function; apply at 2 call sites in `process_eligible_stories()`; add guard unit tests |

**NOT modified:**
- `src/session/` — No session changes; routing is Story 13.2
- `src/review/` — No review changes
- `src/tools/` — No tool changes
- `src/config/` — No config changes
- `Cargo.toml` — No new dependencies

### Existing Code to Reuse

- `StoryInfo::from_key_and_status()` — Already parses any status string. No changes needed.
- `StoryInfo.status` field — Already `pub String`, already populated. No changes needed.
- `deps::RESOLVED_STATUSES` and `deps::BLOCKING_STATUSES` — Unchanged.
- `deps::DependencyGraph::deps_satisfied()` — Checks dependency statuses, not story statuses. Unchanged.
- `deps::tests::make_story()` helper (line 765) — Creates a `StoryInfo` from key and status. Use for new priority sorting tests.

### Anti-Patterns to Avoid

- **DO NOT** add a new field to `StoryInfo` for priority — derive priority from the existing `status` field at sort time.
- **DO NOT** modify `deps.rs` core dependency logic (adjacency graph, cycle detection, cascade blocking, topo sort) — it already works for any status. Only add the post-processing sort.
- **DO NOT** remove the backward-compatible guard before Story 13.2 is implemented and verified.
- **DO NOT** change `SprintStatusFile::load()` or `parse_comment_deps()` — the file format is unchanged.
- **DO NOT** add `"backlog"` or `"review"` to `RESOLVED_STATUSES` in `deps.rs` — these are NOT resolved statuses, they are eligible-for-processing statuses. Resolved means the story's work is complete.
- **DO NOT** apply the pipeline guard in only one of the two consumption points — both the initial `stories` AND every re-polled `fresh_stories` must be guarded.
- **DO NOT** test priority sorting via `eligible_stories()` — that method returns pre-filter document order. Priority sort is applied inside `filter_eligible()` in `deps.rs`, so tests must call `filter_eligible()` directly.
- **DO NOT** place `status_priority()` or the sort call in `Watcher::poll()` — it belongs in `filter_eligible()` so both `poll()` and `re_poll_eligible()` get sorted results.

### Previous Story Intelligence (Story 12.5 — Most Recent in Previous Epic)

- **Baseline test count:** 1148 passing (before 12.5 adds 4), 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)
- **Pre-existing clippy allowances:** `-A clippy::needless_splitn -A clippy::unnecessary_map_or` (both in `src/session/branch.rs`)
- **Test helper available:** `make_test_bot_config()` in `src/watcher/mod.rs::tests` (pub(crate)) — creates a minimal `BotConfig` for watcher tests
- **`write_test_sprint_status()`** helper in watcher tests — writes a sprint-status.yaml file and returns the path
- **`make_story()`** helper in `src/watcher/deps.rs::tests` — creates a `StoryInfo` from key and status
- **`tempfile::tempdir()`** used for all filesystem tests in watcher module

### Git Intelligence — Recent Commits

```
a47a720 feat(epic-12): wire SpawnAgentTool universally in dev + review sessions (Story 12.4)
9b2dbdf feat(epic-12): add SpawnAgentTool with review hardening (Story 12.3)
c29a7ff docs(epic-12): create story 12.3 spec — SpawnAgent tool
ec72cc2 feat(epic-12): simplify ResponseAnalyzer (Story 12.2)
e62467d docs(epic-9): complete code review story 9.3 — fix findings, mark done
```

**Expected commit message:** `feat(epic-13): extend watcher to detect backlog and review stories (Story 13.1)`

### Project Structure Notes

- Changes are confined to `src/watcher/` and a minimal guard in `src/pipeline.rs`
- No new modules, no new files, no new dependencies
- All new code follows existing patterns in these files

### References

- [Source: _bmad-output/planning-artifacts/epics.md:3141–3163 — Story 13.1 AC (Watcher Extension)]
- [Source: _bmad-output/planning-artifacts/epics.md:3480–3503 — Epic 13 Summary and execution strategy]
- [Source: _bmad-output/planning-artifacts/architecture.md:221–238 — Decision 2 (Daemon Reads, Agent Writes)]
- [Source: _bmad-output/planning-artifacts/architecture.md:1096–1169 — Project Structure]
- [Source: _bmad-output/planning-artifacts/architecture.md:1284–1296 — Module communication contracts]
- [Source: _bmad-output/planning-artifacts/architecture.md:660–693 — Decision 10 (Daemon-Orchestrated Consultations)]
- [Source: _bmad-output/project-context.md:62–68 — Daemon Lifecycle (Watcher role)]
- [Source: _bmad-output/project-context.md:109–117 — Testing Rules]
- [Source: src/watcher/mod.rs:141–145 — Current `is_eligible()` implementation]
- [Source: src/watcher/mod.rs:317–322 — Current `eligible_stories()` implementation]
- [Source: src/watcher/mod.rs:392–453 — Current `Watcher::poll()` implementation]
- [Source: src/watcher/mod.rs:539 — `test_story_info_is_not_eligible_backlog` (MUST update)]
- [Source: src/watcher/mod.rs:781 — `test_sprint_status_eligible_stories_empty_when_none_ready` (MUST update)]
- [Source: src/watcher/deps.rs:40–46 — `RESOLVED_STATUSES` and `BLOCKING_STATUSES` constants]
- [Source: src/watcher/deps.rs:660–752 — `filter_eligible()` implementation]
- [Source: src/watcher/deps.rs:765 — `make_story()` test helper]
- [Source: src/pipeline.rs:898–1057 — `process_eligible_stories()` implementation]
- [Source: src/pipeline.rs:1018–1026 — `re_poll_eligible()` call site inside the loop]
- [Source: src/pipeline.rs:1642–1665 — `re_poll_eligible()` implementation]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (claude-opus-4-6)

### Debug Log References

### Completion Notes List

- ✅ Task 1: Expanded `is_eligible()` to accept `backlog`, `ready-for-dev`, `review` via `matches!()`. Updated doc comment and `NoEligibleStories` error message.
- ✅ Task 2: Added `status_priority()` helper and stable `sort_by_key` at end of `filter_eligible()` in deps.rs. Priority: review > ready-for-dev > backlog.
- ✅ Task 3: Added `guard_processable_stories()` in pipeline.rs, applied at both initial stories and re-poll call site. Logs skipped stories and distinct "all guarded out" message.
- ✅ Task 4: Consolidated two `tracing::info!` calls in `poll()` into single log with status breakdown (backlog/ready-for-dev/review counts).
- ✅ Task 5: Fixed 2 broken tests, added 10 new tests across 3 files: eligibility (3), actionable statuses (1), excludes (1), priority sorting (3), backlog deps (1), pipeline guard (2).
- ✅ Task 6: Build passes (zero new warnings), clippy clean for modified files, 1162 tests pass (10 new, 2 modified), 1 pre-existing failure unchanged.

### Change Log

- Extended watcher eligibility to detect `backlog` and `review` stories in addition to `ready-for-dev` (Date: 2026-04-21)
- Added status-priority sorting: review > ready-for-dev > backlog, preserving topo-sort within groups (Date: 2026-04-21)
- Added backward-compatible pipeline guard filtering to `ready-for-dev` only until Story 13.2 (Date: 2026-04-21)
- Consolidated poll logging into single structured log with status breakdown (Date: 2026-04-21)

### File List

- src/watcher/mod.rs (modified) — `is_eligible()` expansion, error message update, consolidated poll logging, 6 new/modified tests
- src/watcher/deps.rs (modified) — `status_priority()` helper, `sort_by_key` in `filter_eligible()`, 4 new priority/dependency tests
- src/pipeline.rs (modified) — `guard_processable_stories()` function, 2 guard call sites, 2 new guard tests

### Review Findings

- [x] [Review][Patch] Stale doc comment on `NoEligibleStories` — still says "ready-for-dev" but eligibility now includes 3 statuses [src/watcher/mod.rs:43] ✅ Fixed
- [x] [Review][Patch] UI/logs show pre-guard eligible count instead of processable count — `batch_start()` moved after guard to report processable count [src/pipeline.rs:899] ✅ Fixed
- [x] [Review][Patch] Missing aggregated guard summary log — added summary with `skipped_count`, `skipped_keys`, and `processable_count` [src/pipeline.rs:guard_processable_stories] ✅ Fixed
