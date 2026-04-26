# Story 14.3: Inject Pre-Epic Stories into Sprint Status

Status: done

## Story

As a daemon operator,
I want Winston's approved pre-epic stories to be automatically added to `sprint-status.yaml`,
So that the linear pipeline processes them before the next epic's regular stories.

## Acceptance Criteria

1. **Given** Winston's epic review report contains proposed pre-epic stories **When** the epic review phase completes (the report is generated and saved) **Then** the daemon parses the "Pre-Epic Stories" section from Winston's report **And** each proposed story is added to `sprint-status.yaml` under the next epic with status `backlog` **And** pre-epic stories are inserted BEFORE the regular stories of the next epic (position `X-0` ensures document-order topo sort processes them first)

2. **Given** the naming convention `{N+1}-0-pre-epic-{N+1}-{slug}` **When** multiple pre-epic stories are generated **Then** they are numbered with sub-indices to maintain order: `5-0a-pre-epic-5-fix-error-handling`, `5-0b-pre-epic-5-missing-tests`, etc. **And** dependencies between pre-epic stories are set sequentially (0b depends on 0a) to ensure ordered processing

3. **Given** pre-epic stories are inserted into `sprint-status.yaml` **When** the daemon's next poll cycle runs **Then** the watcher picks up the `backlog` pre-epic stories as eligible **And** they are processed through the full pipeline (create → adversarial → critic → dev → review) like any regular story **And** the linear pipeline naturally processes all `X-0*` stories before `X-1`, `X-2`, etc. due to document order

4. **Given** the `sprint-status.yaml` is updated with pre-epic stories **When** the update is complete **Then** the changes are committed with message `chore(sprint-status): add pre-epic-{N+1} debt stories from epic-{N} review` **And** `tracing::info!` logs the number of pre-epic stories injected

5. **Given** Winston's report contains "No pre-epic stories proposed" or the Pre-Epic Stories section is missing **When** the daemon attempts to parse **Then** no stories are injected **And** `tracing::info!` logs "No pre-epic stories to inject" **And** no commit is created

6. **Given** the epic review failed (`EpicReviewOutcome::Failed`) **When** the failure report is generated **Then** no pre-epic story parsing is attempted (only successful reviews produce valid proposals)

7. **Given** pre-epic stories have already been injected for epic {N+1} (e.g., after a crash recovery re-trigger) **When** `inject_pre_epic_stories()` runs again with the same or different stories **Then** existing pre-epic story keys are skipped (no duplicates) **And** only genuinely new keys are appended **And** `tracing::info!` logs how many were skipped vs. injected

## Tasks / Subtasks

- [x] Task 1: Fix `StoryInfo::from_key_and_status()` to handle alphanumeric story numbers (AC: #3)
  - [x] 1.1 In `src/watcher/mod.rs`, function `from_key_and_status()` at line 118, change the `story_num` parsing from `parts.next()?.parse().ok()?` to a version that strips trailing alpha characters before parsing. For key `"15-0a-pre-epic-15-fix-something"`, the second segment is `"0a"` — strip `"a"` → parse `"0"` as `u32`. This allows the watcher to recognize pre-epic story keys while maintaining backward compatibility with numeric-only keys. Additionally, preserve the full alphanumeric segment for `story_id` display: use the raw segment (e.g., `"0a"`) in the `story_id` string (`"15.0a"`) so that logging and display can distinguish between pre-epic stories. The `story_num: u32` field uses the stripped numeric value (`0`) for sorting/dependency purposes only
  - [x] 1.2 Add unit test `test_story_info_from_pre_epic_key` — verify `from_key_and_status("15-0a-pre-epic-15-fix-error-handling", "backlog", &dir)` returns `Some(StoryInfo)` with `epic_num = 15`, `story_num = 0`, `story_id = "15.0a"`, `story_key = "15-0a-pre-epic-15-fix-error-handling"`, `status = "backlog"`, `is_eligible() = true`
  - [x] 1.3 Add unit test `test_story_info_from_pre_epic_key_sub_index_b` — verify `from_key_and_status("15-0b-pre-epic-15-add-tests", "backlog", &dir)` returns `Some(StoryInfo)` with `story_num = 0`, `story_id = "15.0b"`
  - [x] 1.4 Add unit test `test_story_info_existing_numeric_keys_unchanged` — verify that regular keys like `"14-3-inject-pre-epic-stories"` still parse correctly with `story_num = 3`, `story_id = "14.3"` (backward compatibility)

- [x] Task 2: Add `PreEpicStory` struct and `parse_pre_epic_stories()` function (AC: #1, #2, #5)
  - [x] 2.1 In `src/review/epic.rs`, add a new public struct `PreEpicStory` with fields: `story_key: String`, `title: String`, `source: String`, `severity: String`, `effort: String`, `justification: String`, `related_deferred_items: String`. Derive `Debug, Clone`. Place it after the `EpicReviewOutcome` enum (after line 92)
  - [x] 2.2 Add a new public function `parse_pre_epic_stories(report: &str, next_epic: u32) -> Vec<PreEpicStory>`. The parsing strategy:
    1. Find the section starting with `#### 5. Pre-Epic Stories for Epic {next_epic}` (or just `Pre-Epic Stories for Epic {next_epic}`) in the report text
    2. If not found, return empty Vec
    3. Check for "No pre-epic stories proposed" within that section → return empty Vec
    4. Split on `##### ` to isolate individual story blocks
    5. For each block: extract the story key from the first line (heading text), then extract fields by looking for `- **Title:**`, `- **Source:**`, `- **Severity:**`, `- **Effort:**`, `- **Justification:**`, `- **Related Deferred Items:**` lines with prefix matching. For multi-line field values (e.g., Justification spanning multiple lines), capture all text until the next `- **` prefix or end of block
    6. Stop processing blocks when encountering content that starts with `**Must-Do` or `**Can Defer` (grouping sections come after all blocks)
    7. Validate the parsed story key slug: must be lowercase ASCII + hyphens only, max 40 characters after the `pre-epic-{N}-` prefix. Log `tracing::warn!` and skip the story if validation fails
    8. Skip any block that fails to parse (missing key or fields) — log a `tracing::warn!` and continue
    9. Return the Vec of successfully parsed stories
  - [x] 2.3 Add test `test_parse_pre_epic_stories_normal_report` — provide a report string containing section 5 with two story blocks matching the exact format from 14.2's template. Verify both stories are parsed with correct fields
  - [x] 2.4 Add test `test_parse_pre_epic_stories_no_section` — report without a Pre-Epic Stories section. Verify empty Vec returned
  - [x] 2.5 Add test `test_parse_pre_epic_stories_no_items` — report with section containing "No pre-epic stories proposed — codebase is clean for Epic 15". Verify empty Vec returned
  - [x] 2.6 Add test `test_parse_pre_epic_stories_single_story` — report with exactly one story block. Verify Vec with one entry
  - [x] 2.7 Add test `test_parse_pre_epic_stories_malformed_block_skipped` — report with one valid and one malformed block (missing Title field). Verify only the valid one is returned
  - [x] 2.8 Add test `test_parse_pre_epic_stories_stops_at_grouping` — report with story blocks followed by `**Must-Do Before Epic 15:**` and `**Can Defer Further:**` sections. Verify grouping text is not parsed as story blocks
  - [x] 2.9 Add test `test_parse_pre_epic_stories_rejects_invalid_slug` — report with a story block whose slug contains uppercase or underscores (e.g., `15-0a-pre-epic-15-Fix_Error`). Verify the story is skipped and not included in the result
  - [x] 2.10 Add test `test_parse_pre_epic_stories_multiline_justification` — report with a story block whose Justification field spans two lines before the next `- **Related Deferred Items:**` line. Verify the full justification text is captured

- [x] Task 3: Add `inject_pre_epic_stories()` function in pipeline (AC: #1, #2, #4, #5, #7)
  - [x] 3.1 In `src/pipeline.rs`, add a new async function `inject_pre_epic_stories(sprint_status_path: &Path, stories: &[PreEpicStory], next_epic: u32) -> Result<usize, String>` near the other helper functions (after `commit_sprint_status()` around line 3177). The injection strategy:
    1. Read `sprint-status.yaml` as a string
    2. If `stories` is empty, return `Ok(0)`
    3. **Idempotency guard:** For each story in `stories`, check if the `story_key` already exists in the file content (substring search for `{story_key}:`). Filter out stories whose keys are already present. Log `tracing::info!` for each skipped duplicate. If all stories are duplicates after filtering, return `Ok(0)`
    4. Find the insertion point — look for the `epic-{next_epic}:` line in the file
    5. **If `epic-{next_epic}:` exists:** Insert pre-epic story entries immediately after the epic line (before the first regular story of that epic)
    6. **If `epic-{next_epic}:` does NOT exist:** Derive `current_epic = next_epic.saturating_sub(1)`. Find the last entry of epic `current_epic` (or the `epic-{current_epic}-retrospective:` line) and insert a blank comment line + `  epic-{next_epic}: backlog` followed by the pre-epic story entries after it. If `current_epic` is `0` (edge case), append at end of `development_status:` block
    7. Each story entry is formatted as: `  {story_key}: backlog` on a single line (two-space indent). For sequential dependencies, append the comment inline on the SAME line: `  {story_key}: backlog  # depends-on: {prev_story_key}` (matching the existing sprint-status.yaml convention). The first story (0a) has no dependency comment
    8. Write the modified content back to the file
    9. Return `Ok(count_of_actually_injected)` (excluding skipped duplicates)
  - [x] 3.2 Add test `test_inject_pre_epic_stories_into_existing_epic` — create a temp sprint-status.yaml with `epic-15: backlog` and `15-1-first-story: backlog`. Inject 2 pre-epic stories. Verify they appear between `epic-15:` and `15-1-first-story:` in the file
  - [x] 3.3 Add test `test_inject_pre_epic_stories_creates_epic_entry` — create a temp sprint-status.yaml ending with epic 14 entries. Inject 2 pre-epic stories for epic 15. Verify `epic-15: backlog` is added and stories appear after it
  - [x] 3.4 Add test `test_inject_pre_epic_stories_sequential_dependencies` — inject 3 stories (0a, 0b, 0c). Verify: 0a has no depends-on comment, 0b line contains `# depends-on: {0a_key}` on the same line as the status, 0c line contains `# depends-on: {0b_key}` inline
  - [x] 3.5 Add test `test_inject_pre_epic_stories_empty_input` — inject empty Vec. Verify file is unchanged and returns `Ok(0)`
  - [x] 3.6 Add test `test_inject_pre_epic_stories_preserves_existing_content` — verify all existing entries, comments, and structure are preserved after injection
  - [x] 3.7 Add test `test_inject_pre_epic_stories_idempotent_on_rerun` — inject 2 stories, then call `inject_pre_epic_stories()` again with the same stories. Verify the file has no duplicates and the second call returns `Ok(0)`

- [x] Task 4: Integrate in `run_epic_gate_inner()` (AC: #1, #4, #5, #6)
  - [x] 4.1 In `src/pipeline.rs`, function `run_epic_gate_inner()`, after the report is extracted from the outcome (line 1870) and BEFORE saving the report to disk (line 1872), add the pre-epic story injection logic:
    1. Only proceed if `review_succeeded` is `true` (AC #6 — failed reviews produce no valid proposals)
    2. Compute `let next_epic = epic_num + 1;`
    3. Call `parse_pre_epic_stories(&report, next_epic)` (import from `crate::review::epic`)
    4. If stories is not empty:
       a. Call `inject_pre_epic_stories(sprint_status_path, &stories, next_epic).await`
       b. On success, log `tracing::info!(action = "pre_epic_stories_injected", count = stories.len(), next_epic = next_epic, "Injected {count} pre-epic stories for epic {next_epic}")`
       c. Commit with `commit_sprint_status(repo_path, sprint_status_path, &format!("chore(sprint-status): add pre-epic-{next_epic} debt stories from epic-{epic_num} review")).await`
       d. On injection error, log `tracing::error!` but continue — pre-epic story injection is non-blocking
    5. If stories is empty, log `tracing::info!(action = "no_pre_epic_stories", next_epic = next_epic, "No pre-epic stories to inject for epic {next_epic}")`
  - [x] 4.2 Add `parse_pre_epic_stories` to the existing `use crate::review::epic::{...}` import at `src/pipeline.rs:28`. The `PreEpicStory` type is referenced by `inject_pre_epic_stories()` parameter, so it must also be imported. Add both: `use crate::review::epic::{..., parse_pre_epic_stories, PreEpicStory};`

## Dev Notes

### CRITICAL: Watcher Parser Incompatibility

`StoryInfo::from_key_and_status()` at `src/watcher/mod.rs:118` parses story numbers as `u32`:
```rust
let story_num: u32 = parts.next()?.parse().ok()?;
```

Pre-epic story keys use alphanumeric sub-indices (e.g., `15-0a-pre-epic-15-fix-something`). The second segment `"0a"` fails `u32::parse()`, causing `from_key_and_status()` to return `None`. The watcher silently skips these stories.

**Fix:** Strip trailing ASCII alpha characters before parsing: `"0a"` → numeric `0`, `"0b"` → numeric `0`. Store the stripped numeric value in `story_num: u32` (used for sorting in `max_by_key` and dependency inference). For `story_id` (display/logging), use the raw alphanumeric segment: `"15.0a"`, `"15.0b"` — this avoids ambiguous log entries where all pre-epic stories would otherwise show as `"15.0"`.

The topological sort and dependency resolution operate on `story_key` (the full string key), not `story_num`, so this change is safe for ordering.

**Latent issue in `deps.rs` (non-breaking):** The `key_lookup` at `deps.rs:394-400` and `shorthand_lookup` at `deps.rs:464-471` both key on `(epic_num, story_num)` or `format!("{}-{}", epic_num, story_num)`. Multiple pre-epic stories with `story_num = 0` collide in these maps. This is harmless because: (1) auto-sequential inference (`story_num > 1` check at line 414) is false for `story_num = 0`, so no predecessor is looked up; (2) pre-epic stories use explicit `# depends-on:` comments with full keys, which resolve via the full-key path at `deps.rs:526`, bypassing the shorthand map entirely.

**Latent issue in `pipeline.rs:2078` (non-breaking):** `max_by_key(|s| s.story_num)` selects the "last" done story for branch checkout before the epic review. Pre-epic stories have `story_num = 0`, so regular stories (story_num >= 1) always win. In a hypothetical epic containing ONLY pre-epic stories (no regular stories), the max returns a pre-epic story's branch — acceptable but worth noting.

This fix is **mandatory** for AC #3 — without it, the watcher will never see pre-epic stories as eligible, even after they're injected into sprint-status.yaml.

### Architecture Compliance

This story completes the daemon-side implementation of FR54: "The epic review agent (Winston) reads `deferred-work.md` and its own code analysis findings to propose pre-epic debt/improvement stories at epic boundaries."

- Story 14.1: Prompt extension — Winston reads deferred-work.md (done)
- Story 14.2: Output format — Winston produces structured story blocks (done)
- **Story 14.3: Daemon-side parsing and sprint-status injection** (this story)
- Story 14.4: Purge processed items from deferred-work.md (next)

The architecture maps FR54 to `review/epic.rs`. The parsing function belongs there (domain knowledge of report format). The injection function belongs in `pipeline.rs` (domain knowledge of sprint-status management and git operations).

[Source: `_bmad-output/planning-artifacts/architecture.md` — Requirements to Structure Mapping, FR54 row, line 1193]

### Integration Point: `run_epic_gate_inner()` at `src/pipeline.rs:1848-1964`

This is the orchestrator for the epic gate flow. The report is extracted at line 1864-1870:

```rust
let (report, review_succeeded, error_summary) = match &outcome {
    EpicReviewOutcome::Completed { report, .. } => (report.clone(), true, None),
    EpicReviewOutcome::Failed { reason, .. } => {
        let failure_report = generate_failure_report(epic_num, reason);
        (failure_report, false, Some(reason.clone()))
    }
};
```

The pre-epic injection goes **after line 1870** (report available as `String`) and **before line 1872** (report saved to disk). The injection modifies `sprint-status.yaml` which is then committed. The existing commit at line 1898-1909 (retro status → "review") handles the retrospective status. The pre-epic commit is **separate** — it has a different message and purpose.

**Sequence after modification:**
1. Extract report from outcome (existing, line 1864-1870)
2. **NEW: Parse pre-epic stories from report**
3. **NEW: Inject into sprint-status.yaml if any found**
4. **NEW: Commit sprint-status with pre-epic injection message**
5. Save report to disk (existing, line 1872-1883)
6. Update retro status to "review" (existing, line 1886-1896)
7. Commit retro status update (existing, line 1898-1909)

### Report Section 5 Format (From Story 14.2)

Winston's section 5 output follows this exact structure:

```markdown
#### 5. Pre-Epic Stories for Epic 15

##### 15-0a-pre-epic-15-fix-transient-error-classification
- **Title:** Fix transient error classification after Copilot removal
- **Source:** deferred-work
- **Severity:** medium
- **Effort:** small
- **Justification:** `is_transient_llm_error` classifies "unauthorized" as retryable since epic 11
- **Related Deferred Items:** story 11.1 (2026-04-15) item 1

##### 15-0b-pre-epic-15-add-mcp-timeout-validation
- **Title:** Reject zero-value MCP server timeout in config validation
- **Source:** deferred-work
- **Severity:** medium
- **Effort:** small
- **Justification:** `timeout_secs: 0` causes immediate handshake timeout
- **Related Deferred Items:** story 9.3 (2026-04-18) item 1

**Must-Do Before Epic 15:**
Stories 15-0a and 15-0b — both medium severity × small effort.

**Can Defer Further:**
(none)
```

**Parsing invariants:**
- All `#####` story blocks appear BEFORE the grouping sections
- Each `#####` heading contains the full story key
- Each story block has exactly 6 `- **Field:**` lines in fixed order
- Grouping sections use bold text (`**Must-Do...**`), not `#####` headings
- "No pre-epic stories proposed" means zero blocks
- **Multi-line fields:** Winston is an LLM — field values (especially Justification) may span multiple lines before the next `- **` prefix. The parser must capture all text between one `- **Field:**` and the next `- **` or end-of-block

### Sprint-Status YAML Writing Pattern

The codebase uses **string-based modification** (not serde_yml round-trips) to preserve comments and formatting. The pattern from `update_story_status()` at `src/session/cleanup.rs:263-304` uses regex find-and-replace.

For injection, use **line-based insertion** — find the target line, insert new lines after it:
- Two-space indentation: `  15-0a-pre-epic-15-fix-something: backlog`
- Sequential dependency as inline comment on the SAME line (matches existing sprint-status convention): `  15-0b-pre-epic-15-add-tests: backlog  # depends-on: 15-0a-pre-epic-15-fix-something`
- Do NOT put `# depends-on:` on a separate line — the existing `parse_comment_deps()` function at `watcher/mod.rs:243` expects the comment on the same line as `key: value`
- Preserve all existing lines, comments, and blank lines

**Insertion point logic:**
1. Search for line matching `^\s*epic-{next_epic}\s*:` (regex)
2. If found → insert after this line, before the first story entry of that epic
3. If not found → find the last line of the previous epic block (either the last story entry or the `epic-{N}-retrospective:` line), insert a blank line + `  epic-{next_epic}: backlog` + story entries after it

### Next Epic May Not Exist in Sprint-Status

When reviewing epic 14 and proposing stories for epic 15, `epic-15:` likely doesn't exist in `sprint-status.yaml` yet (sprint-planning hasn't been run). The injection function MUST handle this by:
1. Creating the `epic-{next_epic}: backlog` entry
2. Placing it after the current epic's entries
3. Adding pre-epic stories underneath

When sprint-planning later runs, it will find `epic-15:` already exists and add regular stories after the pre-epic ones.

### Git Commit Pattern

Use the existing `commit_sprint_status()` at `src/pipeline.rs:3115-3177`:
- Stages the file with `git add`
- Checks for staged changes with `git diff --cached --quiet`
- Commits with the provided message
- Uses `tokio::process::Command` for async git CLI

Commit message per AC #4: `chore(sprint-status): add pre-epic-{N+1} debt stories from epic-{N} review`

### Document-Order Topo Sort Guarantees Processing Order

`DependencyGraph::topological_sort()` at `src/watcher/deps.rs:99-170` uses a `BinaryHeap<Reverse<(usize, String)>>` where `usize` is the document position in sprint-status.yaml. When multiple stories have in_degree 0, the one appearing first in the file is processed first.

By inserting `15-0a-...`, `15-0b-...` BEFORE `15-1-...` in sprint-status.yaml, the topo sort guarantees pre-epic stories are processed first. The sequential `# depends-on:` comments between pre-epic stories (0b → 0a, 0c → 0b) further enforce ordering.

### Forward-Compatibility with Story 14.4

Story 14.4 ("Purge Processed Deferred Items") will read the `related_deferred_items` field from pre-epic story files to know which items to purge from `deferred-work.md`. The `PreEpicStory` struct's `related_deferred_items` field preserves this mapping from Winston's report. Story 14.4 will access it from the story file (created during the create-story pipeline phase), not from the struct directly.

### Error Handling Strategy

Pre-epic story injection is **best-effort, non-blocking**:
- If parsing fails → log warning, inject nothing, continue with gate flow
- If injection fails → log error, continue with gate flow
- If commit fails → log error, continue (the changes are in the working tree, next commit may pick them up)
- A failed injection does NOT prevent the epic gate from completing

This follows the existing pattern in `run_epic_gate_inner()` where report save failures (line 1876-1883) and push failures (line 1912-1920) are logged but non-blocking.

**Crash-recovery duplicate prevention:** If the daemon crashes after committing the pre-epic injection but before updating the retrospective status to `review`, `scan_pending_epic_reviews()` re-triggers the epic gate on restart. Winston runs a new review, possibly proposing the same stories. The idempotency guard in `inject_pre_epic_stories()` (AC #7) prevents duplicate entries by checking if each story key already exists in the file before injecting.

**Report vs. sprint-status discrepancy:** The report is saved to disk AFTER injection. If injection fails, the saved report still contains the "Pre-Epic Stories" section with proposals that were never injected. This is acceptable — the report documents what Winston proposed, not what was acted on. The human reviewer can see the proposals and manually inject if needed.

### Existing Tests Pattern

Tests in `src/review/epic.rs` use `make_test_config()` helper (line 1290) and positional assertions:
```rust
#[test]
fn test_parse_pre_epic_stories_normal_report() {
    let report = "...section 5 content...";
    let stories = parse_pre_epic_stories(&report, 15);
    assert_eq!(stories.len(), 2);
    assert_eq!(stories[0].story_key, "15-0a-pre-epic-15-fix-something");
}
```

Tests in `src/pipeline.rs` use `tempfile` for filesystem tests and `tokio::test` for async tests.

Tests in `src/watcher/mod.rs` use `tempdir` and construct `StoryInfo` via `from_key_and_status()`.

### Previous Story Intelligence

Story 14.2 (Pre-Epic Story Generation) was the last completed story. Key learnings:
- `build_epic_review_prompt()` is a single `format!()` call, now at lines 588-733
- Story 14.2 added `next_epic` as a 7th named argument and section 5
- The prompt format for pre-epic stories is heading-based blocks (not tables) — designed for this story's parser
- Tests added by 14.2 verify the prompt contains section 5 formatting
- Commit convention: `feat(epic-14): description (Story 14.M)`
- All 37 epic review tests pass. Full test suite: 1272 passed, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections` in `session/runner.rs`)

### Git Intelligence

Last 2 commits:
- `6af49ea feat(epic-14): add pre-epic story generation to epic review prompt (Story 14.2)` — modified `src/review/epic.rs`
- `83bca96 feat(epic-14): add deferred work analysis to epic review prompt (Story 14.1)` — modified `src/review/epic.rs`

Convention: `feat(epic-N): description (Story N.M)`.

### Anti-Patterns to Avoid

- Do NOT use `serde_yml` to serialize sprint-status.yaml — it strips comments. Use string-based insertion
- Do NOT modify `build_epic_review_prompt()` or `build_epic_review_preamble()` — the report format is already correct from 14.2
- Do NOT make pre-epic injection blocking — a failure must not prevent the epic gate from completing
- Do NOT add explicit dependencies between pre-epic and regular stories (e.g., `15-1 depends-on 15-0c`) — document-order topo sort handles this naturally
- Do NOT parse the grouping sections ("Must-Do", "Can Defer") — only parse the `#####` story blocks. All stories are injected regardless of grouping
- Do NOT modify the `EpicReviewOutcome` enum — it's already correct
- Do NOT modify the existing `update_story_status()` function — it updates existing entries. Pre-epic injection creates new entries (different operation)
- Do NOT use `unwrap()` in production code — use `?` or handle errors
- Do NOT forget to handle the case where `epic-{next_epic}:` doesn't exist in sprint-status.yaml
- Do NOT use shorthand references (e.g., `15-0a`) in `# depends-on:` comments for pre-epic stories — all pre-epic stories have `story_num = 0`, which makes the shorthand `"15-0"` ambiguous (multiple stories collide in `shorthand_lookup` at `deps.rs:464-471`). Always use the FULL story key (e.g., `15-0a-pre-epic-15-fix-something`). The full-key path at `deps.rs:526` resolves correctly

### Project Structure Notes

Files to modify:
- `src/watcher/mod.rs` — Fix `StoryInfo::from_key_and_status()` to handle `0a`/`0b` + preserve alphanumeric story_id + 3 tests
- `src/review/epic.rs` — Add `PreEpicStory` struct + `parse_pre_epic_stories()` function with slug validation + 8 tests
- `src/pipeline.rs` — Add `inject_pre_epic_stories()` with idempotency guard + integration in `run_epic_gate_inner()` + 6 tests + import

Files NOT to modify:
- `src/review/mod.rs` — no re-exports needed (pipeline can import from `crate::review::epic` directly)
- `src/session/cleanup.rs` — existing `update_story_status()` is for modifying existing entries, not creating new ones
- `src/watcher/deps.rs` — dependency resolution already handles document-order sorting correctly
- `src/config/mod.rs` — no config changes
- Any BMAD files or planning artifacts

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Async tests: `#[tokio::test]` for functions using `tokio::fs`
- Naming: `test_{function}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert
- Temp files: use `tempfile::NamedTempFile` or `tempdir::TempDir` for sprint-status.yaml tests
- All tests inline in their respective modules, inside existing `mod tests` blocks
- Zero-warning policy: `#![deny(clippy::all)]`

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 14, Story 14.3 (lines 3563-3591)]
- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 14, Story 14.2 (lines 3533-3561) — pre-epic story format definition]
- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 14 Summary and Execution Strategy (lines 3622-3638)]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Requirements to Structure Mapping, FR54 row (line 1193)]
- [Source: `_bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md` — Epic 14 description, section 4]
- [Source: `src/pipeline.rs:1848-1964` — `run_epic_gate_inner()` — integration point]
- [Source: `src/pipeline.rs:1864-1870` — Report extraction from `EpicReviewOutcome`]
- [Source: `src/pipeline.rs:3115-3177` — `commit_sprint_status()` — git commit helper]
- [Source: `src/review/epic.rs:680-718` — Section 5 prompt format (Story 14.2)]
- [Source: `src/review/epic.rs:745-779` — `extract_report()` — report extraction pattern]
- [Source: `src/review/epic.rs:77-92` — `EpicReviewOutcome` enum]
- [Source: `src/review/epic.rs:1290-1348` — `make_test_config()` test helper]
- [Source: `src/session/cleanup.rs:263-304` — `update_story_status()` — string-based YAML modification pattern]
- [Source: `src/watcher/mod.rs:98-139` — `StoryInfo::from_key_and_status()` — parser to fix]
- [Source: `src/watcher/mod.rs:141-144` — `is_eligible()` — backlog stories are eligible]
- [Source: `src/watcher/deps.rs:99-170` — `topological_sort()` — document-order tiebreaker]
- [Source: `src/watcher/deps.rs:390-433` — `derive_story_dependencies()` — `key_lookup` and `shorthand_lookup` collision context]
- [Source: `src/watcher/deps.rs:443-547` — `resolve_comment_deps()` — full-key resolution path at line 526]
- [Source: `src/pipeline.rs:2074-2079` — `max_by_key(|s| s.story_num)` — pre-epic story_num=0 context]
- [Source: `src/watcher/mod.rs:243-301` — `parse_comment_deps()` — inline comment parsing (depends-on must be on same line)]
- [Source: `_bmad-output/implementation-artifacts/14-2-pre-epic-story-generation.md` — previous story dev notes and learnings]
- [Source: `_bmad-output/implementation-artifacts/14-1-winston-reads-deferred-work.md` — story 14.1 dev notes]
- [Source: `_bmad-output/project-context.md` — project rules and conventions]

### Review Findings

- [x] [Review][Patch] Partial re-injection breaks dependency chain — first new story loses depends-on to last existing one [src/pipeline.rs:3268-3280]
- [x] [Review][Patch] AC #7: Missing aggregate skipped/injected log + silent Ok(0) arm [src/pipeline.rs:1908]
- [x] [Review][Patch] Story key validation bypassed when pre-epic prefix is absent [src/review/epic.rs:158-178]
- [x] [Review][Defer] Parser section boundary relies on sentinel strings — no hard scope limit [src/review/epic.rs:129] — deferred, pre-existing

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

### Completion Notes List

- Task 1: Modified `StoryInfo::from_key_and_status()` in `src/watcher/mod.rs` to strip trailing alpha characters from story number segments before parsing as `u32`. The raw alphanumeric segment is preserved in `story_id` for display (e.g., `"15.0a"`). Added 3 unit tests covering pre-epic keys and backward compatibility.
- Task 2: Added `PreEpicStory` struct and `parse_pre_epic_stories()` function in `src/review/epic.rs`. The parser finds section 5 in Winston's report, splits on `#####` headings, extracts 7 fields per story block with multi-line support, validates slug format, and stops at grouping sections. Added helper `extract_field()` for field extraction. 8 unit tests covering normal, empty, malformed, and edge cases.
- Task 3: Added `inject_pre_epic_stories()` async function in `src/pipeline.rs`. Handles both existing and missing epic entries, idempotency via substring key check, sequential dependency comments, and preserves all existing content. 6 async tests covering injection, creation, dependencies, empty input, content preservation, and idempotency.
- Task 4: Integrated pre-epic injection into `run_epic_gate_inner()` after report extraction and before report save. Only runs for successful reviews (AC #6). Commits with descriptive message. Errors are logged but non-blocking.

### Change Log

- 2026-04-26: Implemented all 4 tasks for Story 14.3 — watcher parser fix, pre-epic story parsing, sprint-status injection, and pipeline integration.

### File List

- `src/watcher/mod.rs` — Modified `from_key_and_status()` for alphanumeric story numbers + 3 tests
- `src/review/epic.rs` — Added `PreEpicStory` struct, `parse_pre_epic_stories()`, `extract_field()` + 8 tests
- `src/pipeline.rs` — Added `inject_pre_epic_stories()`, integrated in `run_epic_gate_inner()`, updated imports + 6 tests
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — Status updated to review
