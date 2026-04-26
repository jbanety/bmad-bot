# Story 14.1: Winston Reads Deferred Work

Status: done

## Story

As a daemon operator,
I want the epic review agent (Winston) to read `deferred-work.md` as part of its analysis,
So that accumulated technical debt is evaluated alongside the epic's code quality.

## Acceptance Criteria

1. **Given** `src/review/epic.rs` builds an epic review prompt via `build_epic_review_prompt()` **When** this story is implemented **Then** the prompt includes an instruction to attempt loading `{implementation_artifacts}/deferred-work.md` via `read_file` and to treat a file-not-found error or empty content as "No deferred work items found" **And** Winston is instructed to categorize deferred items by severity (critical/high/medium/low) and by effort (small/medium/large) **And** Winston integrates the deferred items into the "Technical Analysis" section of his report alongside his own code-level findings

2. **Given** `deferred-work.md` does not exist or is empty **When** the epic review runs **Then** Winston notes "No deferred work items found" in his report and continues normally **And** no error is raised — the file is optional

3. **Given** `deferred-work.md` contains items from multiple past reviews (different stories, different dates) **When** Winston reads the file **Then** Winston considers the age and origin of each item — older items that persist across multiple stories are labeled `[OVERDUE]` **And** the report section explicitly calls out items that have been deferred for more than one epic

## Tasks / Subtasks

- [x] Task 1: Extend `build_epic_review_prompt()` to include deferred-work.md loading instruction (AC: #1, #2, #3)
  - [x] 1.1 In `src/review/epic.rs`, function `build_epic_review_prompt()`, locate the `### Files to Load (use your tools)` section (currently items 1-5). Add a new item **#6** after item 5 ("Source code") instructing Winston to use `read_file` to attempt loading `{implementation}/deferred-work.md`. The instruction must say: "If `read_file` returns an error (file not found) or the file is empty, note 'No deferred work items found' in your Technical Analysis and continue normally — the file is optional."
  - [x] 1.2 The instruction must tell Winston to categorize each deferred item by **severity** (critical/high/medium/low) and **effort** (small/medium/large)
  - [x] 1.3 The instruction must tell Winston to derive the **epic number** from the story number in section headings: "The epic number is the integer before the dot in the story number (e.g., `story 11.3` → epic 11). Compare against Epic {epic_num} (use `{{epic_num}}` in the format string so Rust interpolates the actual epic number) to determine how many epic boundaries an item has crossed."
  - [x] 1.4 The instruction must tell Winston to label items that have been deferred across 2+ epic boundaries as `[OVERDUE] N epics` in the analysis output (e.g., `[OVERDUE] 3 epics` for an item from epic 11 when reviewing epic 14)

- [x] Task 2: Add "Deferred Work Analysis" bullet to the "Technical Analysis" report section (AC: #1)
  - [x] 2.1 In the `#### 3. Technical Analysis` section of the prompt, add a new peer-level bullet **after** the existing `**Technical Debt Inventory**` bullet: `- **Deferred Work Analysis**: ...`. Do NOT nest it under "Technical Debt Inventory" — it is a separate peer bullet. Instruct Winston to integrate `deferred-work.md` items alongside his own code-level findings in a structured table format
  - [x] 2.2 The instruction must specify this exact per-item analysis format: `| Source (story + date) | Description | Severity | Effort | Epic span | Recommendation |` where "Epic span" is "current" or `[OVERDUE] N epics` and "Recommendation" is one of: `address before next epic` / `defer further` / `obsolete (resolved)`

- [x] Task 3: Add deferred-work cross-reference to "Recommendations" section (AC: #1, #3)
  - [x] 3.1 In the `#### 4. Recommendations` section of the prompt, add a new bullet after "Debt to address before it compounds": `- **Overdue deferred items**: Explicitly list any [OVERDUE] items from the Deferred Work Analysis that should be resolved before the next epic begins`

- [x] Task 4: Unit tests (AC: #1, #2, #3)
  - [x] 4.1 Add test `test_build_prompt_references_deferred_work_file` — verify `build_epic_review_prompt()` output contains `deferred-work.md` AND that it appears after `Source code` (item 5) by checking that `prompt.find("deferred-work.md").unwrap() > prompt.find("Source code").unwrap()`
  - [x] 4.2 Add test `test_build_prompt_contains_deferred_work_categorization` — verify the prompt contains both "severity" and "effort" AND that they appear in the context of deferred work by checking they appear after `deferred-work.md`
  - [x] 4.3 Add test `test_build_prompt_contains_deferred_work_age_analysis` — verify the prompt contains `[OVERDUE]` and `epic boundaries`
  - [x] 4.4 Add test `test_build_prompt_contains_graceful_missing_file_handling` — verify the prompt contains both `read_file` error handling language and `No deferred work items found`
  - [x] 4.5 Add test `test_build_prompt_deferred_work_in_technical_analysis` — verify "Deferred Work Analysis" appears in the prompt AND appears after "Technical Debt Inventory" (structural ordering)
  - [x] 4.6 Add test `test_build_prompt_recommendations_reference_overdue` — verify `[OVERDUE]` appears after "Recommendations" by checking `prompt.find("[OVERDUE]").unwrap() < prompt.rfind("[OVERDUE]").unwrap()` (at least two occurrences: one in Technical Analysis, one in Recommendations) AND `prompt.rfind("[OVERDUE]").unwrap() > prompt.find("Recommendations").unwrap()`

## Dev Notes

### Architecture Compliance

This story implements FR54: "The epic review agent (Winston) reads `deferred-work.md` and its own code analysis findings to propose pre-epic debt/improvement stories at epic boundaries." Story 14.1 covers the "reads and analyzes" part — the prompt extension. Story 14.2 will cover the "proposes pre-epic stories" part and will consume the structured table output from 14.1's Deferred Work Analysis section.

The architecture maps FR54 to `review/epic.rs` (see architecture.md line 1193).

[Source: `_bmad-output/planning-artifacts/architecture.md` — Requirements to Structure Mapping, line 1193]

### Forward-Compatibility with Story 14.2

Story 14.2 will parse Winston's report to extract pre-epic story proposals. The structured table format in Task 2.2 (`| Source | Description | Severity | Effort | Epic span | Recommendation |`) is designed to be machine-parseable. The `[OVERDUE]` label and `address before next epic` recommendation provide clear signals for 14.2's story generation logic. Do NOT use freeform prose for the deferred-work analysis — 14.2 depends on this structure.

### This Is a Prompt Extension — Minimal Code Change

The epic summary states: "Story 14.1 is a prompt extension — minimal code, mainly Winston's instructions." The only Rust file modified is `src/review/epic.rs`. No new modules, no new structs, no new dependencies.

### Target Function: `build_epic_review_prompt()` at `src/review/epic.rs:588`

This function builds the initial user message for the epic review session. It already:
- References `{implementation}` (the `implementation_artifacts` path) — reuse this variable for the deferred-work.md path
- Has a `### Files to Load (use your tools)` section with 5 numbered items (project context, epic definition, story files, architecture, source code) — add item #6
- Has a 4-section report structure — enhance section 3 (Technical Analysis) and section 4 (Recommendations)

The function signature is `pub fn build_epic_review_prompt(epic_num: u32, config: &BotConfig) -> String`. No signature change needed.

### Rust `format!()` Macro — Critical Constraint

The entire prompt is a single `format!()` call with named arguments: `epic_num`, `project_root`, `planning`, `implementation`, `start_delim`, `end_delim`. When adding new prompt text:
- All `{variable}` references inside the format string are interpolated by Rust — only use the 6 existing named arguments listed above
- Literal curly braces in prompt text must be escaped as `{{` and `}}` (e.g., if writing a format example for Winston)
- Do NOT introduce new format arguments — the existing variables cover all needed paths
- The format string uses `\n\` (newline + backslash continuation) for multiline content — maintain this pattern for all new text including the table header/separator rows
- **Table content**: The markdown table (`| Col1 | Col2 |`) uses pipe `|` characters which are NOT special in Rust format strings — no escaping needed for pipes. Each table row is a separate `\n\` line in the format string

### Three Insertion Points — Locate by Section Heading, Not Line Number

After Task 1 adds content, all subsequent line numbers shift. Locate insertion points by section heading:

1. **Item #6 in "Files to Load"**: Find `5. **Source code**:` → insert item `6. **Deferred work**:` on a new `\n\` line after it, before the empty `\n\` that precedes `### Your Mission`. Use `{implementation}` and `{epic_num}` format variables in the new text (both already exist as named args)
2. **"Deferred Work Analysis" bullet**: Find `- **Technical Debt Inventory**:` → insert `- **Deferred Work Analysis**:` on the NEXT `\n\` line, pushing `- **Cross-Cutting Concerns**:` down one line
3. **"Overdue deferred items" bullet**: Find `- Debt to address before it compounds\n\` → insert `- **Overdue deferred items**:` on the NEXT `\n\` line, pushing `- Patterns to reinforce or abandon\n\` down one line

### Current "Technical Analysis" Section

```
#### 3. Technical Analysis
- **Pattern Consistency**: ...
- **Architecture Adherence**: ...
- **Technical Debt Inventory**: TODOs, shortcuts, known issues — list them with file locations
- **Cross-Cutting Concerns**: ...
- **Codebase Health**: Run `cargo check`, `cargo test`, `cargo clippy`...
```

Insert a new peer-level bullet `- **Deferred Work Analysis**:` directly after `- **Technical Debt Inventory**:`. Do NOT nest it under Technical Debt Inventory — it is a sibling bullet.

### `read_file` Tool Behavior for Missing Files

The `ReadFileTool` returns an **error string** when a file doesn't exist — NOT empty content. Winston discovers non-existence only when `read_file` fails. The prompt instruction must tell Winston to **attempt** the read and treat a file-not-found error response as "no deferred items." Do NOT phrase it as "if the file exists" — Winston has no `stat` tool to pre-check existence.

### deferred-work.md Format (Current State)

The file at `_bmad-output/implementation-artifacts/deferred-work.md` uses this structure:
```markdown
# Deferred Work

## Deferred from: code review of story X.Y (YYYY-MM-DD)

- Description of deferred item 1
- Description of deferred item 2

## Deferred from: code review of story Z.W (YYYY-MM-DD)

- Another deferred item
```

Key characteristics:
- **Section headings** contain the source story and date — the epic number is the integer before the dot in the story number (e.g., `story 11.3` → epic 11)
- **Items are bullet points** — freeform text, no structured severity/effort metadata yet (Winston must assess these)
- **Items accumulate** across epics — the current file has items from epics 9, 11, 12, 13
- **The file is persistent** but may not exist for new projects

### Concrete Example: Expected Deferred Work Analysis Output

Given current `deferred-work.md` content and an epic 14 review, Winston should produce something like:

```markdown
- **Deferred Work Analysis**:

| Source | Description | Severity | Effort | Epic span | Recommendation |
|--------|------------|----------|--------|-----------|----------------|
| story 11.1 (2026-04-15) | `is_transient_llm_error` classifies "unauthorized" as transient after Copilot removal | medium | small | [OVERDUE] 3 epics | address before next epic |
| story 12.3 (2026-04-19) | Unbounded `sessions` HashMap — no eviction | low | medium | [OVERDUE] 2 epics | defer further |
| story 13.9 (2026-04-25) | Symlink escape bypasses path traversal guards | high | small | current | address before next epic |
```

This structured format enables Story 14.2 to parse the table and generate pre-epic story proposals.

### Existing Tests Pattern

Existing prompt tests in `src/review/epic.rs` (inside `mod tests`) follow:
```rust
#[test]
fn test_build_prompt_contains_X() {
    let config = make_test_config();
    let prompt = build_epic_review_prompt(N, &config);
    assert!(prompt.contains("expected string"));
}
```

New tests extend this with **positional assertions** using `prompt.find()` to verify structural ordering, not just substring presence. This prevents satisfying tests by dumping text in the wrong section.

### Previous Story Intelligence

Story 13.11 (UI Events for New Pipeline Phases) was the last completed story. Key learnings:
- Test count: 1259 passing (1 pre-existing failure unrelated)
- Pattern: tests inline in `#[cfg(test)] mod tests { ... }` at bottom of each module
- `make_test_config()` helper already exists — reuse it

### Git Intelligence

Recent commits follow `feat(epic-N): description (Story N.M)` convention. Branch naming: `story/{epic}-{story}-{slug}`. All Epic 13 stories committed successfully.

### Anti-Patterns to Avoid

- Do NOT add new struct fields, enums, or modules — this is purely a prompt text change
- Do NOT modify the function signature of `build_epic_review_prompt()`
- Do NOT add runtime file-existence checks in Rust code — Winston uses `read_file` tool at runtime and handles missing files via prompt instructions
- Do NOT add error types or error handling — the file loading is done by the LLM agent, not by Rust code
- Do NOT modify `build_epic_review_preamble()` — the preamble already lists all available tools
- Do NOT introduce new named format arguments in the `format!()` macro — use only the 6 existing ones
- Do NOT use literal `{` or `}` in prompt text without escaping as `{{`/`}}`

### Project Structure Notes

Files to modify:
- `src/review/epic.rs` — Extend `build_epic_review_prompt()` prompt text (3 insertion points) + add 6 unit tests

Files NOT to modify:
- `src/review/mod.rs` — no structural changes
- `src/pipeline.rs` — no pipeline changes for this story
- `src/config/mod.rs` — no config changes
- Any other file — this is a single-file prompt extension

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Naming: `test_build_prompt_{behavior}` in snake_case
- Structure: Arrange (`make_test_config()`) → Act (`build_epic_review_prompt()`) → Assert
- Use `prompt.find("X").unwrap() > prompt.find("Y").unwrap()` for structural ordering assertions
- All tests inline in `src/review/epic.rs` at bottom, inside existing `mod tests`
- Zero-warning policy: `#![deny(clippy::all)]`

### References

- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 14, Story 14.1 (lines 3509-3531)]
- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 14, Story 14.2 (lines 3533-3561) — forward-compatibility context]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — FR54 mapping (line 1193)]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Enforcement Guidelines (lines 1066-1094)]
- [Source: `src/review/epic.rs:588-668` — `build_epic_review_prompt()` current implementation]
- [Source: `src/review/epic.rs:515-572` — `build_epic_review_preamble()` — DO NOT MODIFY]
- [Source: `src/review/epic.rs` — existing prompt unit tests, inside `mod tests`]
- [Source: `src/review/epic.rs` — `make_test_config()` helper, inside `mod tests`]
- [Source: `_bmad-output/implementation-artifacts/deferred-work.md` — current deferred work file format]
- [Source: `_bmad-output/project-context.md` — project rules and conventions]
- [Source: `_bmad-output/implementation-artifacts/13-11-ui-events-new-phases.md` — previous story context]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

### Completion Notes List

- Task 1: Added item #6 "Deferred work" to "Files to Load" section in `build_epic_review_prompt()`. Instructs Winston to use `read_file` to load `{implementation}/deferred-work.md`, handle missing file gracefully, categorize items by severity/effort, derive epic number from story number, and label overdue items with `[OVERDUE] N epics`.
- Task 2: Added "Deferred Work Analysis" peer-level bullet after "Technical Debt Inventory" in section 3 (Technical Analysis). Includes structured table format with columns: Source, Description, Severity, Effort, Epic span, Recommendation.
- Task 3: Added "Overdue deferred items" bullet to section 4 (Recommendations) after "Debt to address before it compounds".
- Task 4: Added 6 unit tests with positional assertions verifying structural ordering, content presence, and cross-section references for deferred work analysis.

### Change Log

- 2026-04-25: Implemented Story 14.1 — Extended `build_epic_review_prompt()` with deferred-work.md analysis instructions (3 insertion points) and 6 unit tests

### Review Findings

- [x] [Review][Defer] Prompt size growth — no token budget or truncation strategy [src/review/epic.rs] — deferred, pre-existing

### File List

- `src/review/epic.rs` (modified) — Extended `build_epic_review_prompt()` format string + 6 new tests
