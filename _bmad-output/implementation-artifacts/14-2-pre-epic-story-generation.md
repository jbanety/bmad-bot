# Story 14.2: Pre-Epic Story Generation

Status: done

## Story

As a daemon operator,
I want Winston to propose pre-epic cleanup stories from both `deferred-work.md` and his own code analysis findings,
So that technical debt and improvements are addressed before the next epic's feature work begins.

## Acceptance Criteria

1. **Given** Winston completes his epic review analysis (code analysis + deferred work review) **When** the report is generated **Then** a new section **"Pre-Epic Stories for Epic {N+1}"** is appended to the report **And** each proposed story follows this format:
   - Story key: `{N+1}-0a-pre-epic-{N+1}-{slug}` with sequential sub-indices (0a, 0b, 0c, ...) for multiple stories (per Story 14.3 convention)
   - Title: descriptive, action-oriented
   - Source: `deferred-work` or `epic-review-finding` or `both`
   - Severity: critical/high/medium/low
   - Estimated effort: small/medium/large
   - Justification: why this should be addressed before epic N+1 feature work
   - Related deferred items: list of `deferred-work.md` source story references this story would resolve (if applicable)

2. **Given** Winston identifies findings from his own code analysis that are not in `deferred-work.md` **When** he generates pre-epic stories **Then** these findings are included alongside deferred items — the two sources are merged into a unified prioritized list **And** the report distinguishes the source of each proposed story (deferred vs epic-review vs both) **And** overlapping items (same issue found in both deferred-work.md and code analysis) are de-duplicated into a single story with source `both`

3. **Given** Winston evaluates the combined list of proposed stories **When** the report is generated **Then** stories are split into two groups: **Must-Do Before Epic {N+1}** (critical/high severity OR `[OVERDUE]` items) and **Can Defer Further** (remaining items with rationale) **And** within each group stories are ordered by severity × inverse-effort ratio (highest priority first) **And** if one group is empty Winston states so explicitly

4. **Given** `deferred-work.md` was empty or missing AND Winston's own code analysis found no issues warranting cleanup **When** he generates the Pre-Epic Stories section **Then** he writes "No pre-epic stories proposed — codebase is clean for Epic {N+1}" **And** no story blocks are emitted

## Tasks / Subtasks

- [x] Task 1: Add 5th report section "Pre-Epic Stories" to `build_epic_review_prompt()` (AC: #1, #2, #3, #4)
  - [x] 1.1 In `src/review/epic.rs`, function `build_epic_review_prompt()`, add a local variable `let next_epic = epic_num + 1;` before the `format!()` call, and add `next_epic = next_epic` to the named arguments of the `format!()` macro
  - [x] 1.2 Update the doc comment above `build_epic_review_prompt()` from "four-section" to "five-section" (line 581: "Provides the four-section report structure" → "Provides the five-section report structure")
  - [x] 1.3 After the `#### 4. Recommendations` section content (after "run build commands. Do not guess — verify." and BEFORE the `CRITICAL CONSTRAINTS:` block), add a new `#### 5. Pre-Epic Stories for Epic {next_epic}` section. The section description is a report structure instruction (like sections 1-4) — Winston will produce the actual content in his report output between the `<<EPIC_REVIEW_REPORT_START>>` / `<<EPIC_REVIEW_REPORT_END>>` delimiters. Include these instructions for Winston:
    - Reference findings from the Deferred Work Analysis table (section 3) and his own code analysis — do NOT re-read `deferred-work.md`
    - De-duplicate overlapping items: if the same issue appears in both deferred-work.md and code analysis, emit one story with source `both`
    - For each proposed story, output a structured block (format defined in Task 1.4)
    - Story keys use sequential sub-indices: `{next_epic}-0a-pre-epic-{next_epic}-...`, `{next_epic}-0b-pre-epic-{next_epic}-...`, etc.
    - Slug constraints: short kebab-case (lowercase ASCII, hyphens only, max 40 characters)
    - Source must be one of: `deferred-work`, `epic-review-finding`, `both`
    - Severity: critical/high/medium/low; Effort: small/medium/large
    - All story blocks (`#####` headings) MUST appear first, THEN the grouping sections. This ordering is required for Story 14.3's parser to split on `##### ` reliably
    - After all story blocks, split into two groups: **Must-Do Before Epic {next_epic}** (critical/high severity OR `[OVERDUE]` items) and **Can Defer Further** (remaining items with rationale). If either group is empty, state so explicitly
    - If no items warrant pre-epic stories, write "No pre-epic stories proposed — codebase is clean for Epic {next_epic}" and emit no story blocks
    - End the section with: "You MUST follow the exact per-story block format below — no variations, no extra fields, no tables. Story 14.3 parses this output programmatically."
  - [x] 1.4 The structured entry format must use a clear per-story block (not a table) so that Story 14.3 can parse individual stories. Include this exact format template in the prompt:
    ```
    ##### {next_epic}-0a-pre-epic-{next_epic}-{{slug}}
    - **Title:** descriptive action-oriented title
    - **Source:** deferred-work | epic-review-finding | both
    - **Severity:** critical | high | medium | low
    - **Effort:** small | medium | large
    - **Justification:** why this must be addressed before epic {next_epic}
    - **Related Deferred Items:** source story references from deferred-work.md (e.g., "story 11.1 item 1, story 9.3 item 1"), or "N/A" if source is epic-review-finding
    ```
    Note: `{{slug}}` is escaped in the Rust format string and appears as `{slug}` in the prompt output. `{next_epic}` is Rust-interpolated and appears as the actual number (e.g., `15`).

- [x] Task 2: Unit tests (AC: #1, #2, #3, #4)
  - [x] 2.1 Add test `test_build_prompt_contains_pre_epic_stories_section` — verify `build_epic_review_prompt(14, &config)` output contains `Pre-Epic Stories for Epic 15` and that it appears after `Recommendations` by checking `prompt.find("Pre-Epic Stories").unwrap() > prompt.find("Recommendations").unwrap()`
  - [x] 2.2 Add test `test_build_prompt_pre_epic_story_key_format` — verify the prompt contains the sub-indexed key convention by checking for `0a-pre-epic-` in the output (the Rust format string `{next_epic}-0a-pre-epic-{next_epic}-` interpolates to e.g. `15-0a-pre-epic-15-`)
  - [x] 2.3 Add test `test_build_prompt_pre_epic_source_distinction` — verify the prompt contains all three source values: `deferred-work`, `epic-review-finding`, and `both`
  - [x] 2.4 Add test `test_build_prompt_pre_epic_prioritization_groups` — verify the prompt contains both grouping labels: `Must-Do Before Epic` and `Can Defer Further`
  - [x] 2.5 Add test `test_build_prompt_pre_epic_next_epic_number` — call `build_epic_review_prompt(7, &config)` and verify the prompt contains `Epic 8` in the Pre-Epic Stories section (proving next_epic = epic_num + 1). Use positional assertion: `prompt[pre_epic_pos..].contains("Epic 8")` where `pre_epic_pos = prompt.find("Pre-Epic Stories").unwrap()`
  - [x] 2.6 Add test `test_build_prompt_pre_epic_no_items_fallback` — verify the prompt contains the fallback text `No pre-epic stories proposed`
  - [x] 2.7 Add test `test_build_prompt_pre_epic_format_strictness` — verify the prompt contains the format enforcement instruction (e.g., `exact` and `Story 14.3 parses`)

### Review Findings

- [x] [Review][Patch] Prompt body says "four sections" instead of "five sections" [src/review/epic.rs:628] — fixed
- [x] [Review][Patch] `{{{{slug}}}}` renders `{{slug}}` instead of `{slug}` in LLM output — confirmed with rustc [src/review/epic.rs:690,691,697] — fixed
- [x] [Review][Patch] Missing within-group ordering instruction (severity × inverse-effort ratio per AC #3) [src/review/epic.rs:707-710] — fixed

## Dev Notes

### Architecture Compliance

This story continues the implementation of FR54: "The epic review agent (Winston) reads `deferred-work.md` and its own code analysis findings to propose pre-epic debt/improvement stories at epic boundaries." Story 14.1 covered "reads and analyzes." Story 14.2 covers "proposes pre-epic stories" — the structured output format that Winston must produce in section 5 of his report.

The architecture maps FR54 to `review/epic.rs`.

[Source: `_bmad-output/planning-artifacts/architecture.md` — Requirements to Structure Mapping, FR54 row]

### Forward-Compatibility with Story 14.3

Story 14.3 will parse the "Pre-Epic Stories" section from Winston's report to inject them into `sprint-status.yaml`. The per-story block format (heading-based, not table) is designed for reliable regex/string parsing:
- Each story starts with a `#####` heading containing the story key — 14.3 can split on `##### ` to isolate individual proposals
- The story key in the heading follows the exact convention: `{N+1}-0a-pre-epic-{N+1}-{slug}` with sub-indices (per epics.md Story 14.3 AC: "numbered with sub-indices to maintain order: `5-0a-pre-epic-5-...`, `5-0b-pre-epic-5-...`")
- Structured `- **Field:**` lines are parseable with simple prefix matching
- All `#####` story blocks appear BEFORE the grouping sections — 14.3 can stop parsing at the first non-`#####` line after the section heading
- The "Must-Do" vs "Can Defer Further" groupings reference story keys by name, letting 14.3 decide which stories to inject immediately

Do NOT use a markdown table for pre-epic stories — tables are harder to parse when cells contain variable-length text, and the heading-based format gives each story a clear delimiter.

### This Is a Prompt Extension — Minimal Code Change

The epic summary states: "Story 14.2 defines the output format Winston uses for story proposals." Like Story 14.1, the only Rust file modified is `src/review/epic.rs`. No new modules, no new structs, no new dependencies.

### Target Function: `build_epic_review_prompt()` at `src/review/epic.rs:588`

This function builds the initial user message for the epic review session. After Story 14.1, it:
- Has named args: `epic_num`, `project_root`, `planning`, `implementation`, `start_delim`, `end_delim`
- Has a `### Files to Load` section with 6 items (items 1-5 original + item 6 "Deferred work" added by 14.1)
- Has 4 report sections: Epic Recap, Functional Testing Guide, Technical Analysis (with Deferred Work Analysis sub-bullet from 14.1), Recommendations (with Overdue deferred items sub-bullet from 14.1)
- Story 14.2 adds `next_epic` as a 7th named argument and a 5th report section

### Rust `format!()` Macro — Critical Constraint

The entire prompt is a single `format!()` call. When adding new content:
- Add `next_epic = next_epic` to the named arguments list at the end of the format!() call
- Compute `let next_epic = epic_num + 1;` before the format!() call
- All `{variable}` references are interpolated by Rust — only use the 7 named arguments (6 existing + `next_epic`)
- Literal curly braces in prompt text (like `{slug}`) must be escaped as `{{slug}}`
- The format string uses `\n\` (newline + backslash continuation) for multiline content
- Pipe `|` characters are NOT special in Rust format strings — no escaping needed

### Insertion Point — After Section 4, Before CRITICAL CONSTRAINTS

The report structure in the prompt describes sections 1-4, then has a paragraph ("Use your tools to explore the codebase thoroughly..."), then the `CRITICAL CONSTRAINTS:` block. The new section 5 description goes AFTER the "verify" line and BEFORE the `CRITICAL CONSTRAINTS:` block.

All five section descriptions sit in the same block of prompt text — they are instructions telling Winston what to produce. Winston's actual report output (containing the filled-in sections) appears between the `<<EPIC_REVIEW_REPORT_START>>` / `<<EPIC_REVIEW_REPORT_END>>` delimiters. The section descriptions are NOT inside the delimiters themselves — they describe the report structure that Winston fills in between the delimiters.

Locate by finding:
```
         run build commands. Do not guess — verify.\n\
```
Insert section 5 on new `\n\` lines after this, before the `\n\` line containing `CRITICAL CONSTRAINTS:`.

### deferred-work.md Current State (for Context)

The file at `_bmad-output/implementation-artifacts/deferred-work.md` currently has items from epics 9, 11, 12, and 13. Winston's Deferred Work Analysis table (section 3, from 14.1) classifies each item with severity, effort, and epic span. Section 5 instructions must tell Winston to **reference his own table from section 3** rather than re-parse deferred-work.md — avoid duplicate analysis.

### Existing Tests Pattern

Tests follow the same pattern established in Story 14.1:
```rust
#[test]
fn test_build_prompt_{behavior}() {
    let config = make_test_config();
    let prompt = build_epic_review_prompt(N, &config);
    assert!(prompt.contains("expected string"));
    // Positional assertions:
    assert!(prompt.find("A").unwrap() > prompt.find("B").unwrap());
}
```

New tests go in the existing `mod tests` block at the bottom of `src/review/epic.rs`, in a new comment-delimited section: `// Pre-epic story generation tests (Story 14.2)`.

### Previous Story Intelligence

Story 14.1 (Winston Reads Deferred Work) was the last completed story. Key learnings:
- `build_epic_review_prompt()` is a single `format!()` call at line 588-691
- The format string uses `\n\` continuations throughout
- Story 14.1 added content at 3 insertion points (item 6, Deferred Work Analysis bullet, Overdue deferred items bullet) — all within the existing format!() call
- Tests use `make_test_config()` helper and positional assertions via `prompt.find()`
- 6 tests were added by 14.1, in a `// Deferred work prompt tests (Story 14.1)` section
- The commit was `feat(epic-14): add deferred work analysis to epic review prompt (Story 14.1)`

### Git Intelligence

Last commit: `83bca96 feat(epic-14): add deferred work analysis to epic review prompt (Story 14.1)` — modified only `src/review/epic.rs`. The convention is `feat(epic-N): description (Story N.M)`.

### Anti-Patterns to Avoid

- Do NOT add new struct fields, enums, or modules — this is purely a prompt text change + one local variable
- Do NOT modify the function signature of `build_epic_review_prompt()`
- Do NOT modify `build_epic_review_preamble()` — the preamble lists available tools, not report structure
- Do NOT use a markdown table for pre-epic stories — use heading-based blocks for 14.3 parseability
- Do NOT instruct Winston to re-read `deferred-work.md` in section 5 — he already analyzed it in section 3's Deferred Work Analysis. Section 5 should reference section 3's findings
- Do NOT use literal `{` or `}` in prompt text without escaping as `{{`/`}}` — except for the 7 named format args
- Do NOT add runtime file operations or new error types — this is LLM prompt text only
- Do NOT allow slugs with underscores, spaces, or uppercase — enforce kebab-case (lowercase ASCII + hyphens, max 40 chars)

### Note: `epic_num + 1` Arithmetic

`epic_num` is `u32`. The addition `epic_num + 1` would panic on `u32::MAX` in debug mode. This is a non-issue in practice (epic numbers are single/double digits), but if clippy flags it, use `epic_num.saturating_add(1)` or `epic_num + 1` with a brief comment. Do not add a runtime guard for this.

### Project Structure Notes

Files to modify:
- `src/review/epic.rs` — Extend `build_epic_review_prompt()` with section 5 + update doc comment + add 7 unit tests

Files NOT to modify:
- `src/review/mod.rs` — no structural changes
- `src/pipeline.rs` — no pipeline changes for this story (14.3 will add parsing)
- `src/config/mod.rs` — no config changes
- Any other file — this is a single-file prompt extension

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Naming: `test_build_prompt_{behavior}` in snake_case
- Structure: Arrange (`make_test_config()`) → Act (`build_epic_review_prompt()`) → Assert
- Use `prompt.find("X").unwrap() > prompt.find("Y").unwrap()` for structural ordering assertions
- All tests inline in `src/review/epic.rs` at bottom, inside existing `mod tests`
- New tests go in a dedicated comment-delimited section after the Story 14.1 tests
- Zero-warning policy: `#![deny(clippy::all)]`

### Concrete Example: Expected Section 5 Output

Given an epic 14 review, Winston should produce something like:

```markdown
#### 5. Pre-Epic Stories for Epic 15

##### 15-0a-pre-epic-15-fix-transient-error-classification
- **Title:** Fix transient error classification after Copilot removal
- **Source:** deferred-work
- **Severity:** medium
- **Effort:** small
- **Justification:** `is_transient_llm_error` classifies "unauthorized" as retryable since epic 11 — 3 epics overdue. Quick fix prevents wasted retry cycles.
- **Related Deferred Items:** story 11.1 (2026-04-15) item 1

##### 15-0b-pre-epic-15-add-mcp-timeout-validation
- **Title:** Reject zero-value MCP server timeout in config validation
- **Source:** deferred-work
- **Severity:** medium
- **Effort:** small
- **Justification:** `timeout_secs: 0` causes immediate handshake timeout — silent failure mode since epic 9.
- **Related Deferred Items:** story 9.3 (2026-04-18) item 1

**Must-Do Before Epic 15:**
Stories 15-0a and 15-0b — both medium severity × small effort, quick fixes that eliminate silent failure modes.

**Can Defer Further:**
(none — all items are recommended for immediate resolution)
```

Key structural invariants for 14.3 parsing:
- All `#####` story blocks appear before the grouping sections
- Each `#####` heading contains the full story key
- Each story block has exactly 6 `- **Field:**` lines in fixed order
- Grouping sections use bold text (`**Must-Do...**`), not headings — so `##### ` splitting isolates only story blocks

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 14, Story 14.2 section]
- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 14, Story 14.3 section — sub-index convention: "numbered with sub-indices to maintain order: 5-0a, 5-0b"]
- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 14 Summary and Execution Strategy section]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Requirements to Structure Mapping, FR54 row]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Configuration Files table, deferred-work.md row]
- [Source: `src/review/epic.rs:588-691` — `build_epic_review_prompt()` current implementation (post-14.1)]
- [Source: `src/review/epic.rs:515-572` — `build_epic_review_preamble()` — DO NOT MODIFY]
- [Source: `src/review/epic.rs` — Story 14.1 deferred work prompt tests, section "Deferred work prompt tests (Story 14.1)"]
- [Source: `src/review/epic.rs` — `make_test_config()` helper, inside `mod tests`]
- [Source: `_bmad-output/implementation-artifacts/14-1-winston-reads-deferred-work.md` — previous story context and learnings]
- [Source: `_bmad-output/implementation-artifacts/deferred-work.md` — current deferred work file (items from epics 9, 11, 12, 13)]
- [Source: `_bmad-output/project-context.md` — project rules and conventions]
- [Source: `src/pipeline.rs` — `run_epic_gate_inner()` method — how report is saved and consumed]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — clean implementation, no debugging needed.

### Completion Notes List

- ✅ Task 1: Extended `build_epic_review_prompt()` with section 5 "Pre-Epic Stories for Epic {next_epic}". Added `let next_epic = epic_num + 1;` local variable and `next_epic` named argument to format!(). Section 5 instructs Winston to reference his own Deferred Work Analysis (section 3) and code analysis findings, de-duplicate overlapping items, emit heading-based story blocks (not tables) for 14.3 parseability, split into Must-Do/Can Defer groups, and handle the empty case. Updated doc comment from "four-section" to "five-section".
- ✅ Task 2: Added 7 unit tests in a dedicated `// Pre-epic story generation tests (Story 14.2)` section: section presence + ordering, story key format, source distinction (3 values), prioritization groups, next_epic arithmetic, no-items fallback, format strictness enforcement. All 37 epic review tests pass (30 existing + 7 new). Full test suite: 1272 passed, 1 pre-existing failure (unrelated: `test_build_context_limit_recovery_message_contains_all_sections` in session/runner.rs).

### Change Log

- 2026-04-26: Added Pre-Epic Stories section (section 5) to epic review prompt and 7 unit tests (Story 14.2)

### File List

- src/review/epic.rs (modified — prompt extension + 7 tests)
