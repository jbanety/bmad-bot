# Story 14.4: Purge Processed Deferred Items

Status: done

## Story

As a daemon operator,
I want resolved deferred items to be removed from `deferred-work.md` when their corresponding pre-epic stories are completed,
So that the deferred work file remains current and doesn't accumulate stale resolved items.

## Acceptance Criteria

1. **Given** a pre-epic story reaches `done` status **When** the pipeline completes the story **Then** the daemon checks if the story key matches the pre-epic naming convention (`X-0*-pre-epic-*`) **And** if it matches, the daemon reads the story file to find the "Related Deferred Items" section (listing which `deferred-work.md` items this story resolved) **And** the corresponding items are removed from `deferred-work.md` **And** a `tracing::info!` logs: "Purged {count} resolved items from deferred-work.md"

2. **Given** `deferred-work.md` contains items under section headings (e.g., `## Deferred from: code review of story-3.3 (2026-03-18)`) **When** all items under a section heading are removed **Then** the section heading is also removed to keep the file clean

3. **Given** a pre-epic story resolves some but not all items from a deferred section **When** the purge runs **Then** only the resolved items (matched by section heading + item position) are removed **And** remaining items in the section are preserved

4. **Given** `deferred-work.md` becomes empty after purging **When** all items have been resolved **Then** the file is NOT deleted — it is left with only the top-level heading `# Deferred Work` as a placeholder for future deferred items **And** the daemon commits the cleanup with message `chore(deferred): purge resolved items from pre-epic-{N+1} stories`

5. **Given** a regular story (non-pre-epic) reaches `done` status **When** the pipeline completes the story **Then** no purge logic is triggered **And** no deferred-work.md access occurs

6. **Given** the story file does not contain a "Related Deferred Items" section or the section contains "none" **When** the purge is attempted **Then** no items are removed **And** `tracing::info!` logs "No related deferred items to purge" **And** no commit is created

7. **Given** the `deferred-work.md` file does not exist **When** the purge is triggered **Then** no error is raised **And** `tracing::info!` logs "deferred-work.md not found, skipping purge"

## Tasks / Subtasks

- [x] Task 1: Add `is_pre_epic_story()` utility function (AC: #1, #5)
  - [x] 1.1 In `src/pipeline.rs`, add a new function `fn is_pre_epic_story(story_key: &str) -> bool` near the other helper functions (after `has_uncommitted_sprint_status()` around line 3162). Implementation: use a `regex::Regex` (or simple string matching) to check if the key matches pattern `^\d+-0[a-z]-pre-epic-`. Alternatively, use `lazy_static` or `std::sync::LazyLock` for compiled regex. A non-regex approach: split on `-`, verify the second segment starts with `0` followed by a single lowercase letter, and the key contains `pre-epic-`
  - [x] 1.2 Add test `test_is_pre_epic_story_valid_key` — verify `is_pre_epic_story("15-0a-pre-epic-15-fix-error-handling")` returns `true`
  - [x] 1.3 Add test `test_is_pre_epic_story_sub_index_b` — verify `is_pre_epic_story("15-0b-pre-epic-15-add-tests")` returns `true`
  - [x] 1.4 Add test `test_is_pre_epic_story_regular_key` — verify `is_pre_epic_story("14-3-inject-pre-epic-stories")` returns `false`
  - [x] 1.5 Add test `test_is_pre_epic_story_epic_key` — verify `is_pre_epic_story("epic-14")` returns `false`

- [x] Task 2: Add `DeferredItemRef` struct and `parse_related_deferred_items()` function (AC: #1, #6)
  - [x] 2.1 In `src/review/epic.rs`, add a new struct `DeferredItemRef` with fields: `section_story_id: String` (e.g., `"11.1"`), `section_date: String` (e.g., `"2026-04-15"`), `item_number: usize` (1-indexed). Derive `Debug, Clone, PartialEq`. Place it after the `PreEpicStory` struct (after line 115)
  - [x] 2.2 Add a new public function `parse_related_deferred_items(story_file_content: &str) -> Vec<DeferredItemRef>`. The parsing strategy:
    1. Search for `## Related Deferred Items` section heading in the story file
    2. If not found, search for `- **Related Deferred Items:**` inline field (the field format from `PreEpicStory`)
    3. If neither found, return empty Vec
    4. Extract the text block after the heading/field
    5. For each line, try to match the pattern `story {story_id} ({date}) item {number}` using regex `r"story\s+(\d+\.\d+[a-z]?)\s+\((\d{4}-\d{2}-\d{2})\)\s+item\s+(\d+)"`. This handles both `story 11.1 (2026-04-15) item 1` and multi-reference strings like `story 11.1 (2026-04-15) item 1, story 9.3 (2026-04-18) item 1`
    6. For each match: extract `section_story_id`, `section_date`, `item_number` (parse as usize)
    7. If the content is exactly `"none"` or `"None"` (after trimming), return empty Vec
    8. Return the Vec of parsed refs. Log `tracing::warn!` for lines that look like references but fail to parse
  - [x] 2.3 Add test `test_parse_related_deferred_items_section_heading` — story file with `## Related Deferred Items\n\n- story 11.1 (2026-04-15) item 1\n- story 9.3 (2026-04-18) item 1`. Verify 2 refs parsed
  - [x] 2.4 Add test `test_parse_related_deferred_items_inline_field` — story file with `- **Related Deferred Items:** story 11.1 (2026-04-15) item 1`. Verify 1 ref parsed
  - [x] 2.5 Add test `test_parse_related_deferred_items_none` — story file with `## Related Deferred Items\n\nnone`. Verify empty Vec
  - [x] 2.6 Add test `test_parse_related_deferred_items_missing_section` — story file without any related deferred items section. Verify empty Vec
  - [x] 2.7 Add test `test_parse_related_deferred_items_comma_separated` — inline field with `story 11.1 (2026-04-15) item 1, story 11.1 (2026-04-15) item 2`. Verify 2 refs parsed, both with `section_story_id = "11.1"`, items 1 and 2
  - [x] 2.8 Add test `test_parse_related_deferred_items_multiline` — section with multiple lines, each referencing a different story/item. Verify all refs parsed

- [x] Task 3: Add `purge_deferred_items()` function (AC: #1, #2, #3, #4, #7)
  - [x] 3.1 In `src/pipeline.rs`, add a new async function `async fn purge_deferred_items(deferred_work_path: &Path, refs: &[DeferredItemRef]) -> Result<usize, String>` near the other helper functions (after `inject_pre_epic_stories()` or `commit_sprint_status()`). The purge strategy:
    1. If `refs` is empty, return `Ok(0)`
    2. If `deferred_work_path` does not exist, return `Ok(0)` (caller logs info)
    3. Read `deferred-work.md` as a string
    4. Parse the file into sections: each section starts with `## Deferred from: code review of story {story_id} ({date})` and contains bullet items (lines starting with `- `)
    5. Group refs by `(section_story_id, section_date)` for efficient lookup
    6. For each section in the file, check if any refs target it
    7. If refs target this section: collect all bullet items in order, mark targeted items for removal by item_number (1-indexed). CRITICAL: process item removals from highest index to lowest within each section to avoid index shifting
    8. After removing targeted items: if the section has zero remaining bullets, mark the entire section (heading + items) for removal
    9. Reconstruct the file content from remaining sections
    10. Ensure the top-level `# Deferred Work` heading is always preserved (even if all sections removed)
    11. Trim trailing whitespace/blank lines but ensure file ends with a single newline
    12. Write the modified content back to the file
    13. Return `Ok(count_of_actually_removed_items)` — items whose section/index didn't exist in the file are skipped (not counted, not errors)
  - [x] 3.2 Add test `test_purge_deferred_items_single_item` — create temp deferred-work.md with 2 sections, 2 items each. Purge 1 item from section 1. Verify: item removed, section heading preserved, other items untouched, returns `Ok(1)`
  - [x] 3.3 Add test `test_purge_deferred_items_all_items_in_section` — purge both items from a section. Verify: entire section (heading + items) removed, returns `Ok(2)`
  - [x] 3.4 Add test `test_purge_deferred_items_all_sections_empty` — purge all items from all sections. Verify: only `# Deferred Work\n` remains, returns `Ok(total_items)`
  - [x] 3.5 Add test `test_purge_deferred_items_partial_section` — section with 3 items, purge items 1 and 3. Verify: item 2 remains, section heading preserved, returns `Ok(2)`
  - [x] 3.6 Add test `test_purge_deferred_items_nonexistent_ref` — ref pointing to section that doesn't exist in file. Verify: file unchanged, returns `Ok(0)`
  - [x] 3.7 Add test `test_purge_deferred_items_empty_refs` — empty refs Vec. Verify: file unchanged, returns `Ok(0)`
  - [x] 3.8 Add test `test_purge_deferred_items_file_not_found` — path to non-existent file. Verify: returns `Ok(0)` (no error)
  - [x] 3.9 Add test `test_purge_deferred_items_preserves_other_sections` — purge items from section A. Verify: sections B, C unchanged (content, spacing, formatting preserved)

- [x] Task 4: Integrate purge in `run_review_pipeline()` (AC: #1, #4, #5, #6, #7)
  - [x] 4.1 In `src/pipeline.rs`, function `run_review_pipeline()`, after the Phase 7 sprint-status commit succeeds (inside the `Ok(()) => {` arm at line 1353, after the push branch block ends at line 1361), add the deferred work purge logic:
    1. Check `if is_pre_epic_story(story_key)` — if false, skip entirely (AC #5)
    2. Build `story_file_path` from `self.config.bmad_paths.implementation_artifacts` + `{story_key}.md`
    3. Read the story file via `tokio::fs::read_to_string(&story_file_path).await`
    4. If file read fails → `tracing::warn!` + skip (non-blocking)
    5. Call `parse_related_deferred_items(&story_content)` (import from `crate::review::epic`)
    6. If refs is empty → `tracing::info!(action = "no_deferred_items_to_purge", story_key = %story_key, "No related deferred items to purge")` + skip
    7. Build `deferred_work_path` from `self.config.bmad_paths.implementation_artifacts` + `deferred-work.md`
    8. If `deferred_work_path` does not exist → `tracing::info!("deferred-work.md not found, skipping purge")` + skip (AC #7)
    9. Call `purge_deferred_items(&deferred_work_path, &refs).await`
    10. On success with count > 0:
        a. Extract `epic_num` from story_key (first segment before `-`)
        b. Call `commit_sprint_status(repo_path, &deferred_work_path, &format!("chore(deferred): purge resolved items from pre-epic-{epic_num} stories")).await` — reuse existing commit helper (it works with any file path despite the name)
        c. `tracing::info!(action = "deferred_items_purged", count = count, story_key = %story_key, "Purged {count} resolved items from deferred-work.md")`
        d. On commit error → `tracing::error!` but continue (non-blocking, same pattern as pre-epic injection)
    11. On success with count == 0 → `tracing::info!("No matching deferred items found in deferred-work.md")`
    12. On purge error → `tracing::error!` but continue (non-blocking)
  - [x] 4.2 Add `parse_related_deferred_items` and `DeferredItemRef` to the existing `use crate::review::epic::{...}` import at `src/pipeline.rs:28` (or wherever the existing import line is)

### Review Findings

- [x] [Review][Patch] `tracing::info!` "Purged {count}..." fires unconditionally even when commit fails [src/pipeline.rs:1421-1426] — fixed: gated on commit success via match
- [x] [Review][Patch] `purge_deferred_items` rewrites file even when no items were actually removed [src/pipeline.rs:3558-3580] — fixed: early return when total_removed == 0
- [x] [Review][Patch] No test for multi-line continuation items; blank lines within items silently dropped [src/pipeline.rs:3509-3516] — fixed: added test_purge_deferred_items_multiline_continuation + test_purge_deferred_items_no_rewrite_when_zero_removed
- [x] [Review][Defer] Content between top-level heading and first `## Deferred from:` section silently dropped during reconstruction [src/pipeline.rs:3295-3301] — deferred, pre-existing
- [x] [Review][Defer] No warning when a DeferredItemRef target section is not found in deferred-work.md [src/pipeline.rs:3527-3541] — deferred, pre-existing
- [x] [Review][Defer] No integration test for pipeline orchestration path (read story → parse refs → purge → commit) [src/pipeline.rs:1362-1459] — deferred, pre-existing
- [x] [Review][Defer] Regex compiled on every function call instead of using LazyLock [src/review/epic.rs:145, src/pipeline.rs:3524] — deferred, pre-existing

## Dev Notes

### CRITICAL: Deferred-Work.md File Format

Current file at `_bmad-output/implementation-artifacts/deferred-work.md` (138 lines). Exact structure:

```markdown
# Deferred Work

## Deferred from: code review of story 11.1 (2026-04-15)

- First bullet item text...
- Second bullet item text...

## Deferred from: code review of story 11.2 (2026-04-15)

- Bullet item...
- Another bullet item...
```

**Parsing rules:**
- Top-level heading: `# Deferred Work` (always line 1, always preserved)
- Section headings: `## Deferred from: code review of story {story_id} ({date})`
- Items: lines starting with `- ` (single dash + space) under each section
- Some items are multi-line (continuation lines don't start with `- `)
- Blank lines separate sections and their items
- Sections appear in chronological order of code reviews

**Current sections in file (18 total):** stories 11.1, 11.2, 11.3, 11.4, 11.5, 9.3, 12.1, 12.3, 13.2, 13.4, 13.3, 13.5, 13.6, 13.7, 13.8, 13.9, 13.10, 12.4, 14.1, 14.3

### Related Deferred Items Reference Format

Winston's report uses this format for the `related_deferred_items` field:

```
- **Related Deferred Items:** story 11.1 (2026-04-15) item 1
```

Where `item 1` refers to the 1st bullet point (1-indexed) under the matching section heading in deferred-work.md. The reference `story 11.1 (2026-04-15) item 1` maps to the first `- ` line under `## Deferred from: code review of story 11.1 (2026-04-15)`.

Multiple references may appear comma-separated or on multiple lines:
```
story 11.1 (2026-04-15) item 1, story 9.3 (2026-04-18) item 1
```

When `related_deferred_items` is `"none"`, no purge is needed.

The story FILE (created by create-story phase) may present this in two formats:
1. **Section heading:** `## Related Deferred Items` followed by bullet list
2. **Inline field:** `- **Related Deferred Items:** ...` (matching `PreEpicStory` struct format)

The parser must handle both formats.

### Multi-Line Bullet Items

Some deferred items span multiple lines. Example from deferred-work.md line 5-6:

```
- `is_transient_llm_error` in `src/session/runner.rs` still classifies "unauthorized" and "token expired" 
  as transient retry-worthy errors, but the token-refresh recovery mechanism was removed in Story 11.1.
```

When counting "item N", count only lines starting with `- ` (the top-level bullets). Continuation lines (indented or not starting with `- `) belong to the preceding bullet. When removing an item, remove the bullet line AND all continuation lines until the next `- ` or section heading or blank line followed by a section heading.

### Integration Point: `run_review_pipeline()` at `src/pipeline.rs:1106-1407`

The story completion flow in Phase 7 (lines 1309-1390):

```
1309: // Phase 7 — Mark story done in sprint-status.yaml, commit & push
1313:     update_story_status(..., "done")
1321:         unblock_dependents(...)
1346:         commit_sprint_status(...)
1353:             Ok(()) => {
1354:                 push_branch(...)  // non-blocking
1361:             }          ← INSERT PURGE LOGIC HERE
1362:             }
1363:             Err(e) => { ... return fatal ... }
1388:         }
1389:     }
1390: }
1392: // Phase 8 — Notify
```

The purge logic goes inside the `Ok(()) => {` arm of the commit match, after the push branch block (line 1361), before the closing `}` at line 1362. This ensures purge only runs when:
- Story is successfully marked "done"
- Sprint-status is committed
- We haven't returned with a fatal error

### Reusing `commit_sprint_status()` for Deferred-Work Commits

The `commit_sprint_status()` function at `src/pipeline.rs:3174-3236` is generic despite its name — it takes any `Path` and runs `git add` + `git diff --cached` + `git commit`. Reuse it directly for committing deferred-work.md:

```rust
commit_sprint_status(
    &self.config.bmad_paths.project_root,
    &deferred_work_path,
    &format!("chore(deferred): purge resolved items from pre-epic-{epic_num} stories"),
).await
```

Do NOT create a duplicate commit helper. The function works correctly with any file path.

### Architecture Compliance

This story completes Epic 14 — the full FR54 deferred work processing cycle:

- Story 14.1: Prompt extension — Winston reads deferred-work.md (done)
- Story 14.2: Output format — Winston produces structured story blocks (done)
- Story 14.3: Daemon-side parsing and sprint-status injection (done)
- **Story 14.4: Purge processed items from deferred-work.md** (this story)

FR54 maps to `review/epic.rs` for domain logic. The parsing function (`parse_related_deferred_items`) belongs in `review/epic.rs` (domain knowledge of deferred work format). The purge function (`purge_deferred_items`) belongs in `pipeline.rs` (filesystem operations and git commit pattern). This follows the exact same split as Story 14.3 where `parse_pre_epic_stories()` is in `review/epic.rs` and `inject_pre_epic_stories()` is in `pipeline.rs`.

[Source: `_bmad-output/planning-artifacts/architecture.md` — FR54 row, line 1193]
[Source: `_bmad-output/planning-artifacts/architecture.md` — deferred-work.md definition, line 1341]

### Error Handling Strategy

Deferred item purging is **best-effort, non-blocking** — identical pattern to pre-epic story injection (Story 14.3):
- If story file read fails → log warning, skip purge, continue
- If related items parsing returns empty → log info, skip, continue
- If deferred-work.md doesn't exist → log info, skip, continue
- If purge fails → log error, continue
- If commit fails → log error, continue (changes in working tree, next commit may pick them up)
- A failed purge does NOT prevent the story from completing or the notification from being sent

This follows the established pattern in `run_review_pipeline()` where push failures (line 1354-1361) and PR comment failures (line 1290-1306) are logged but non-blocking.

### Pre-Epic Story Detection Pattern

Pre-epic story keys follow the convention: `{epic_num}-0{letter}-pre-epic-{epic_num}-{slug}`

Examples:
- `15-0a-pre-epic-15-fix-transient-error-classification`
- `15-0b-pre-epic-15-add-mcp-timeout-validation`

Detection: the second segment starts with `0` followed by a single lowercase letter (e.g., `0a`, `0b`). Regular stories have numeric-only second segments (e.g., `14-3-inject-...`). The key also contains `pre-epic-` as a substring.

Simple detection without regex:
```rust
fn is_pre_epic_story(story_key: &str) -> bool {
    let parts: Vec<&str> = story_key.splitn(3, '-').collect();
    if parts.len() < 3 { return false; }
    let seg = parts[1];
    seg.starts_with('0')
        && seg.len() == 2
        && seg.chars().nth(1).map_or(false, |c| c.is_ascii_lowercase())
        && story_key.contains("pre-epic-")
}
```

### Section Removal Logic

When all bullets under a section are removed, the section heading AND any trailing blank lines must be removed. Be careful to preserve the structure of neighboring sections.

Algorithm for reconstructing the file:
1. Parse into list of `(section_heading, Vec<bullet_items>)`
2. For each section, remove targeted items
3. Filter out sections with zero remaining items
4. Reconstruct from remaining sections
5. Always preserve `# Deferred Work\n` at the top

### Item Counting: Only Top-Level Bullets

"Item 1" means the 1st line starting with `- ` in the section. Multi-line bullets (continuation lines) are part of the same item. When counting items for index matching, only count `- ` lines.

Example section:
```markdown
## Deferred from: code review of story 11.1 (2026-04-15)

- First item text that spans    ← item 1
  multiple lines here
- Second item text               ← item 2
```

"item 1" = "First item text that spans\n  multiple lines here"
"item 2" = "Second item text"

### Forward-Compatibility: No Further Stories Depend on This

This is the final story in Epic 14. No downstream stories depend on its output format or behavior. The purge is a cleanup operation — it removes stale data but produces no artifacts consumed by other stories.

### Previous Story Intelligence

Story 14.3 (Inject Pre-Epic Stories) was the last completed story. Key learnings:
- String-based file modification (not serde_yml) to preserve comments/formatting — same approach needed here
- `commit_sprint_status()` is the canonical commit helper — reuse it directly
- Tests use `tempfile` for filesystem tests and `tokio::test` for async
- Commit convention: `feat(epic-14): description (Story 14.M)`
- The `PreEpicStory.related_deferred_items` field format is defined in `src/review/epic.rs:113-114`
- Test sample data in `epic.rs:1517` shows format: `story 11.1 (2026-04-15) item 1`
- Full test suite: 1272 passed (+ Story 14.3 additions), 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)

### Git Intelligence

Last 3 commits:
- `16cc984 feat(epic-14): inject pre-epic stories into sprint-status from epic review (Story 14.3)` — modified `src/pipeline.rs`, `src/review/epic.rs`, `src/watcher/mod.rs`
- `6af49ea feat(epic-14): add pre-epic story generation to epic review prompt (Story 14.2)` — modified `src/review/epic.rs`
- `83bca96 feat(epic-14): add deferred work analysis to epic review prompt (Story 14.1)` — modified `src/review/epic.rs`

Convention: `feat(epic-N): description (Story N.M)`.

### Anti-Patterns to Avoid

- Do NOT use `serde_yml` to deserialize/serialize deferred-work.md — it's markdown, not YAML. Use string-based parsing and manipulation
- Do NOT delete `deferred-work.md` when empty — leave it with only the `# Deferred Work` heading
- Do NOT make the purge blocking — a failure must not prevent the pipeline from completing
- Do NOT modify `parse_pre_epic_stories()` or `inject_pre_epic_stories()` — they are complete from Story 14.3
- Do NOT modify `update_story_status()` — it handles sprint-status entries, not deferred-work
- Do NOT create a duplicate commit helper — reuse `commit_sprint_status()` which works with any file path
- Do NOT use `unwrap()` in production code — use `?` or handle errors
- Do NOT match items by full text content — use section heading + item index as specified in the reference format
- Do NOT modify any BMAD files or planning artifacts
- Do NOT add the purge logic outside the `Ok(()) =>` arm of the sprint-status commit match — if the commit failed, we return with a fatal error so the purge would never run, but placing it correctly makes the intent clear

### Project Structure Notes

Files to modify:
- `src/review/epic.rs` — Add `DeferredItemRef` struct + `parse_related_deferred_items()` function + tests
- `src/pipeline.rs` — Add `is_pre_epic_story()` + `purge_deferred_items()` + integration in `run_review_pipeline()` + import update + tests

Files NOT to modify:
- `src/watcher/mod.rs` — no changes needed (pre-epic detection at watcher level already handled by Story 14.3)
- `src/session/cleanup.rs` — `update_story_status()` is for sprint-status entries
- `src/review/mod.rs` — no re-exports needed (pipeline can import from `crate::review::epic` directly)
- `src/config/mod.rs` — no config changes
- `_bmad-output/implementation-artifacts/deferred-work.md` — not modified during development; only modified at runtime by the purge function

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Async tests: `#[tokio::test]` for functions using `tokio::fs`
- Naming: `test_{function}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert
- Temp files: use `tempfile::NamedTempFile` or `tempdir::TempDir` for deferred-work.md tests
- All tests inline in their respective modules, inside existing `mod tests` blocks
- Zero-warning policy: `#![deny(clippy::all)]`
- Regex in tests: use the same regex crate already in `Cargo.toml` (check with `grep regex Cargo.toml`)

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 14, Story 14.4 (lines 3593-3621)]
- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 14 Summary and Execution Strategy (lines 3622-3638)]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — FR54, Deferred Work Processing (line 1193)]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — deferred-work.md file definition (line 1341)]
- [Source: `_bmad-output/implementation-artifacts/deferred-work.md` — current file format (138 lines, 18 sections)]
- [Source: `src/pipeline.rs:1106-1407` — `run_review_pipeline()` — integration point]
- [Source: `src/pipeline.rs:1309-1390` — Phase 7: mark done + commit sprint-status]
- [Source: `src/pipeline.rs:1353-1362` — `Ok(()) =>` arm where purge should be inserted]
- [Source: `src/pipeline.rs:3174-3236` — `commit_sprint_status()` — reusable commit helper]
- [Source: `src/pipeline.rs:3238-3380` — `inject_pre_epic_stories()` — sibling function pattern]
- [Source: `src/review/epic.rs:98-115` — `PreEpicStory` struct — `related_deferred_items` field]
- [Source: `src/review/epic.rs:122-224` — `parse_pre_epic_stories()` — sibling parser pattern]
- [Source: `src/review/epic.rs:226-244` — `extract_field()` — field extraction helper]
- [Source: `src/review/epic.rs:1517` — test data showing reference format `story 11.1 (2026-04-15) item 1`]
- [Source: `src/review/epic.rs:1525` — test data showing reference format `story 9.3 (2026-04-18) item 1`]
- [Source: `_bmad-output/implementation-artifacts/14-3-inject-pre-epic-stories-sprint-status.md` — previous story dev notes, architecture compliance, patterns]
- [Source: `_bmad-output/project-context.md` — project rules and conventions]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — clean implementation, no debug issues.

### Completion Notes List

- Task 1: Added `is_pre_epic_story()` in `src/pipeline.rs` using string-based detection (no regex). Validates second segment starts with `0` + lowercase letter and key contains `pre-epic-`. 4 tests added.
- Task 2: Added `DeferredItemRef` struct and `parse_related_deferred_items()` in `src/review/epic.rs`. Handles both `## Related Deferred Items` section heading and `- **Related Deferred Items:**` inline field formats. Uses regex to extract story_id, date, and item_number. 6 tests added.
- Task 3: Added `purge_deferred_items()` async function in `src/pipeline.rs`. Parses deferred-work.md into sections, removes targeted items by section heading + 1-indexed position, removes empty sections, always preserves `# Deferred Work` heading. 8 tests added.
- Task 4: Integrated purge logic into `run_review_pipeline()` Phase 7, inside the `Ok(())` arm after push. Only triggers for pre-epic stories. Non-blocking: all errors logged but don't halt pipeline. Updated imports to include `DeferredItemRef` and `parse_related_deferred_items`.
- Total: 18 new tests, all passing. 1 pre-existing test failure unchanged (`test_build_context_limit_recovery_message_contains_all_sections`). Full suite: 1307 passed, 1 failed (pre-existing).

### Change Log

- 2026-04-26: Implemented Story 14.4 — purge processed deferred items from deferred-work.md when pre-epic stories complete

### File List

- `src/pipeline.rs` — Added `is_pre_epic_story()`, `purge_deferred_items()`, integration in `run_review_pipeline()`, import update, 12 tests
- `src/review/epic.rs` — Added `DeferredItemRef` struct, `parse_related_deferred_items()`, 6 tests
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — Status update: 14-4 ready-for-dev → in-progress → review
- `_bmad-output/implementation-artifacts/14-4-purge-processed-deferred-items.md` — Story file updates
