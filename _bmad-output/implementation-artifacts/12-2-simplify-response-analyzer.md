# Story 12.2: Simplify ResponseAnalyzer

Status: done

## Story

As a daemon operator,
I want the `ResponseAnalyzer` to focus only on essential detection patterns,
So that it no longer carries persona/menu-specific auto-response logic that is irrelevant with skill-based sessions.

## Acceptance Criteria

1. **Remove persona-driven auto-response patterns** — The following pattern categories and their corresponding match blocks in `analyze()` are deleted from `src/session/analyzer.rs`:
   - `PROCEED_PATTERNS` (priority 3) — "Should I proceed?" confirmation auto-responses
   - `STEP_BY_STEP_PATTERNS` (priority 4) — step-by-step detection
   - `YOLO_PATTERNS` (priority 5) — YOLO/batch mode questions
   - `REVIEW_FIX_PATTERNS` (priority 5.5) — review fix decision auto-responses
   - `STORY_SELECTION_PATTERNS` (priority 6) — story selection auto-responses
   - All `ResponseAction::Continue { reply }` variants that carried hardcoded menu responses are removed with their patterns

2. **Retain essential detections** — The simplified `ResponseAnalyzer` keeps exactly these detections in priority order:
   - Priority 0: `<<BMAD_JOB_DONE>>` sentinel → `Completed`
   - Priority 1: Escalation slot check → `Escalated`
   - Priority 1.5: `REVIEW_COMPLETE_PATTERNS` → `Completed`
   - Priority 2: `COMPLETION_SIGNALS` substring → `Completed`
   - Priority 2.5: `COMPLETION_REGEX_PATTERNS` regex → `Completed`
   - Default: `NoReply` (NOT `Continue { reply: "Continue." }`)

3. **Default action is `NoReply`** — Unrecognized responses result in `ResponseAction::NoReply` instead of `Continue { reply: "Continue." }`. This is a **semantic change only** — the analyzer signals "nothing to say" rather than crafting a reply. The call sites (`runner.rs`, `review/mod.rs`) already handle `NoReply` by sending `"Continue."`, so runtime behavior is unchanged. The separation matters for Epic 13: when consultations are introduced, the call sites will distinguish between `NoReply` (send "Continue.") and `Continue { reply }` (inject consultation findings).

4. **`Continue { reply }` preserved as extension point** — The `ResponseAction::Continue { reply }` variant remains in the enum (not deleted) for Epic 13 daemon-orchestrated consultations where the daemon injects critic/adversarial findings.

5. **`story_key` parameter removed from `analyze()`** — Since `STORY_SELECTION_PATTERNS` is deleted, the `story_key` parameter is no longer needed. Remove it from the method signature and update all call sites.

6. **Supervisor `rules.rs` untouched** — Overlapping patterns in the supervisor rule engine (confirmations, step-by-step, story selection) remain. They serve a different purpose: answering agent questions via `ask_supervisor` tool, not auto-responding to workflow prompts.

7. **`strip_agent_artifacts()` untouched** — This function is independent of the analyzer patterns and must not be modified.

8. **Tests updated** — Tests for removed patterns are deleted. Tests for retained patterns pass. New test `test_analyzer_default_is_no_reply` verifies the default action.

9. **Zero new warnings** — `cargo clippy` and `cargo build` produce zero new warnings. All unused variables, stale doc comments, and dead code from the removal are cleaned up. Existing pre-existing clippy errors in `src/session/branch.rs` (untouched) remain.

10. **Test count** — Baseline is 1133 passing, 1 pre-existing failure. Story removes 10 tests for deleted patterns, adds 1 new test. Final expected count: **1124 passing**, 1 pre-existing failure.

11. **Stale doc comments updated** — All doc comments that reference removed patterns or parameters are updated across all touched files.

### Acknowledged Deviation: `Failed` Detection

The epic AC (epics.md:3007) lists `Failed — detection of fatal error patterns` as an essential detection. This story does **not** add `Failed` because:

- No `Failed` variant exists in the current `ResponseAction` enum — adding one is additive scope beyond "simplify."
- Fatal errors from LLM API calls (timeouts, 429, 500s) are already handled at the runner level via retry with exponential backoff and `SessionOutcome::Failed` — they never reach the analyzer.
- Text-based fatal error detection (agent says "I cannot continue") requires defining new patterns and behaviors, which is new feature work, not simplification.
- The `Escalated` variant already covers agent-signaled problems via the `ask_supervisor` → escalation slot mechanism.

If `Failed` detection is needed for agent-text-level fatal errors (e.g., agent outputs "FATAL: ..."), it should be a separate story or added in Epic 13 alongside the consultation mechanism. This deviation is documented here so the PM/architect can decide.

## Tasks / Subtasks

- [x] Task 1: Remove pattern constants from `src/session/analyzer.rs` (AC: #1)
  - [x] 1.1 Delete the `REVIEW_FIX_PATTERNS` const array and its doc comment (the `/// Review fix decision patterns` block)
  - [x] 1.2 Delete the `PROCEED_PATTERNS` const array and its doc comment (the `/// Confirmation/proceed patterns` block)
  - [x] 1.3 Delete the `STEP_BY_STEP_PATTERNS` const array and its doc comment (the `/// Step-by-step detection patterns` block)
  - [x] 1.4 Delete the `YOLO_PATTERNS` const array and its doc comment (the `/// YOLO/batch mode patterns` block)
  - [x] 1.5 Delete the `STORY_SELECTION_PATTERNS` const array and its doc comment (the `/// Story selection patterns` block)
- [x] Task 2: Simplify `analyze()` method in `src/session/analyzer.rs` (AC: #1, #2, #3, #5)
  - [x] 2.1 Remove the `// Priority 3: Confirmation/proceed patterns` block (the `if PROCEED_PATTERNS...` block returning `Continue { reply: "Yes, proceed." }`)
  - [x] 2.2 Remove the `// Priority 4: Step-by-step detection` block (the `if STEP_BY_STEP_PATTERNS...` block)
  - [x] 2.3 Remove the `// Priority 5: YOLO/mode questions` block (the `if YOLO_PATTERNS...` block)
  - [x] 2.4 Remove the `// Priority 5.5: Review fix decision` block (the `if REVIEW_FIX_PATTERNS...` block)
  - [x] 2.5 Remove the `// Priority 6: Story selection` block (the `if STORY_SELECTION_PATTERNS...` block)
  - [x] 2.6 Change the `// Priority 7: Default` return from `ResponseAction::Continue { reply: "Continue.".to_string() }` to `ResponseAction::NoReply`
  - [x] 2.7 Remove `story_key: &str` parameter from the `analyze()` method signature
  - [x] 2.8 Update `analyze()` doc comment — remove priority 3-6 from the list, update default description, remove `story_key` from `# Arguments`
- [x] Task 3: Update doc comments in `src/session/analyzer.rs` (AC: #2, #11)
  - [x] 3.1 Update `ResponseAnalyzer` struct doc comment — replace priority list with simplified order (0, 1, 1.5, 2, 2.5, default=NoReply), remove references to confirmation/proceed/YOLO/story-selection
  - [x] 3.2 Update module-level doc comment (lines 1-11) — simplify description to reflect essential-detections-only purpose
  - [x] 3.3 Update `REVIEW_COMPLETE_PATTERNS` doc comment — remove the sentence referencing `REVIEW_FIX_PATTERNS` at priority 5.5 ("`to avoid the step 5 summary...from triggering REVIEW_FIX_PATTERNS at priority 5.5`"). The concern no longer exists since `REVIEW_FIX_PATTERNS` is deleted
- [x] Task 4: Update call sites (AC: #5, #9, #11)
  - [x] 4.1 `src/session/runner.rs` — remove `&story.story_key` third argument from the `self.analyzer.analyze(...)` call (search for `.analyzer.analyze(` or `analyzer.analyze(`)
  - [x] 4.2 `src/review/mod.rs` — remove `&story_reply` third argument from the `self.analyzer.analyze(...)` call
  - [x] 4.3 `src/review/mod.rs` — delete the `let story_reply = story.specs_path.display().to_string();` variable assignment (it becomes unused after 4.2)
  - [x] 4.4 `src/review/mod.rs` — update the `drive_review_session()` doc comment: remove the paragraph about `story_reply` ("The `story_reply` parameter for the analyzer uses `story.specs_path`...") and update the "Normal phase" description to say "analyze responses with `ResponseAnalyzer` for completion/escalation detection" instead of "auto-respond to workflow prompts"
- [x] Task 5: Update tests in `src/session/analyzer.rs` (AC: #8, #10)
  - [x] 5.1 Delete `test_analyzer_detects_proceed_question`
  - [x] 5.2 Delete `test_analyzer_detects_step_by_step`
  - [x] 5.3 Delete `test_analyzer_detects_yolo_question`
  - [x] 5.4 Delete `test_analyzer_proceed_various_phrases`
  - [x] 5.5 Delete `test_analyzer_story_selection_replies_with_story_key`
  - [x] 5.6 Delete `test_analyzer_detects_review_fix_decision`
  - [x] 5.7 Delete `test_analyzer_detects_fix_automatically_pattern`
  - [x] 5.8 Delete `test_analyzer_review_fix_does_not_false_positive`
  - [x] 5.9 Delete `test_analyzer_review_complete_priority_over_fix_patterns` — this test verified review complete fires before fix patterns; with `REVIEW_FIX_PATTERNS` gone, the priority ordering is meaningless. Delete entirely.
  - [x] 5.10 Delete `test_analyzer_sentinel_takes_priority_over_proceed` — with `PROCEED_PATTERNS` gone, this just duplicates `test_analyzer_detects_sentinel_completion`. Delete entirely.
  - [x] 5.11 Update `test_analyzer_case_insensitive` — remove the proceed patterns section (the `let proceed_cases = vec![...]` block and its assertions). Keep only the completion signal section.
  - [x] 5.12 Update `test_analyzer_default_continues` — rename to `test_analyzer_default_is_no_reply` and change expected action from `Continue { reply: "Continue.".to_string() }` to `NoReply`
  - [x] 5.13 Update ALL remaining test `analyze()` calls — remove the third `story_key` argument. Every call changes from `analyzer.analyze(response, &slot, "key")` to `analyzer.analyze(response, &slot)`. Affected tests: all sentinel tests, completion tests, escalation tests, review complete tests, case-insensitive test, no-false-positive tests, regex tests.
  - [x] 5.14 Add `test_analyzer_unrecognized_responses_return_no_reply` — verify that various unrecognized responses (working text, questions, partial progress) all return `NoReply`
- [x] Task 6: Verify build and tests (AC: #9, #10)
  - [x] 6.1 Run `cargo build` — zero new errors
  - [x] 6.2 Run `cargo clippy` — zero new warnings (check for unused variables, stale imports)
  - [x] 6.3 Run `cargo test` — expected 1124 passing, 1 pre-existing failure. All remaining tests pass, 10 tests removed, 1 test added.

## Dev Notes

### Architecture Compliance

- **Pattern:** `ResponseAnalyzer` is a stateless unit struct (`analyzer.rs:221`). It stays stateless after this change.
- **Enum dispatch:** `ResponseAction` enum at `analyzer.rs:22-40`. Keep all 4 variants — `Continue { reply }` is the Epic 13 extension point for consultation injection.
- **Call sites:** Only 2 places call `analyzer.analyze()`:
  - `src/session/runner.rs` — dev session chat loop (search for `self.analyzer.analyze(`)
  - `src/review/mod.rs` — review session chat loop (search for `.analyzer.analyze(`)
- **NoReply handling at call sites — unchanged:** After this story, the analyzer returns `NoReply` as default instead of `Continue { reply: "Continue." }`. Both call sites already handle `NoReply` by sending `"Continue."`:
  - `runner.rs` — `NoReply | Continue { .. }` arm sends `"Continue."` for `NoReply`
  - `review/mod.rs` — `NoReply => "Continue.".to_string()`
  - These match arms STAY as-is. The semantic distinction matters for Epic 13 when the call sites will differentiate `NoReply` (generic continue) from `Continue { reply }` (injected consultation).
- **Imports stay:** Module-level `use regex::Regex;` and `use std::sync::LazyLock;` remain needed for `COMPLETION_REGEX_PATTERNS`. The `strip_agent_artifacts()` function has its own scoped imports.

### What NOT to Change

- **DO NOT** modify `src/supervisor/rules.rs` — its proceed/step-by-step patterns serve `ask_supervisor`, not the chat loop
- **DO NOT** modify `strip_agent_artifacts()` — independent utility function
- **DO NOT** remove `ResponseAction::Continue { reply }` from the enum — needed for Epic 13
- **DO NOT** modify `src/supervisor/architect.rs` — Architect session unchanged
- **DO NOT** modify `src/session/runner.rs` match arms for `NoReply | Continue` — only remove the `story_key` argument from the `analyze()` call
- **DO NOT** change `ContextBuilder`, `build_preamble()`, or `AgentFactory` — unchanged for this story

### REVIEW_COMPLETE_PATTERNS — Future Cleanup Note

`REVIEW_COMPLETE_PATTERNS` is retained as a fuzzy fallback for review session completion detection. Under skill-based sessions, the review skill should emit `<<BMAD_JOB_DONE>>` like the dev skill — making these fuzzy patterns potentially redundant. However, removing them now is risky: the code-review skill may not always emit the sentinel (model variance). These patterns are a low-cost safety net. If future runs confirm the sentinel is reliable for review sessions, these can be removed in a cleanup story.

### File Impact Summary

| File | Change Type | Scope |
|------|-------------|-------|
| `src/session/analyzer.rs` | **Major** — remove 5 pattern constants, simplify `analyze()`, remove `story_key` param, change default to `NoReply`, update all doc comments, update stale `REVIEW_COMPLETE_PATTERNS` doc | ~200 lines removed (patterns + match blocks + tests), ~15 lines added (new test + doc updates) |
| `src/session/runner.rs` | **Trivial** — remove `&story.story_key` arg from `analyze()` call | 1 line changed |
| `src/review/mod.rs` | **Minor** — remove `&story_reply` arg from `analyze()` call, delete unused `story_reply` variable, update `drive_review_session()` doc comment | ~5 lines changed |

### Testing Approach

- **Deleted tests (10):** `test_analyzer_detects_proceed_question`, `test_analyzer_detects_step_by_step`, `test_analyzer_detects_yolo_question`, `test_analyzer_proceed_various_phrases`, `test_analyzer_story_selection_replies_with_story_key`, `test_analyzer_detects_review_fix_decision`, `test_analyzer_detects_fix_automatically_pattern`, `test_analyzer_review_fix_does_not_false_positive`, `test_analyzer_review_complete_priority_over_fix_patterns`, `test_analyzer_sentinel_takes_priority_over_proceed`
- **Updated tests (~15):** All remaining `analyze()` calls lose the third `story_key` argument. `test_analyzer_case_insensitive` loses proceed section. `test_analyzer_default_continues` renamed and expectation changed.
- **New test (1):** `test_analyzer_unrecognized_responses_return_no_reply`
- **Preserved tests:** All sentinel tests (4), completion signal tests (3), escalation tests (2), review complete tests (2), no-false-positive tests (2), strip_agent_artifacts tests (all), completion regex tests (all), ResponseAction trait tests (2)
- **Verification:** `cargo test` — expected **1124 passing** (1133 - 10 + 1), 1 pre-existing failure

### Project Structure Notes

- All changes confined to `src/session/` and `src/review/` — existing project structure maintained
- No new files, no deleted files — modifications only
- No dependency changes in `Cargo.toml`

### Previous Story Intelligence (12.1)

From Story 12-1 completion notes:
- Struct field approach used for `skill_path` (not parameter threading) — precedent for clean design
- `build_preamble()` retains persona instructions for Architect compatibility — do not touch
- Baseline: **1133 tests passing, 1 pre-existing failure** (`test_build_context_limit_recovery_message_contains_all_sections`)
- Pre-existing: 2 clippy errors in `src/session/branch.rs` — untouched, not related
- Agent model: anthropic/claude-sonnet-4-6

### Git Intelligence

Recent commits:
- `d9c7103` — docs(epic-12): complete code review story 12.1
- `c9e7c34` — feat(epic-12): parameterize activation by skill (Story 12.1)
- Conventional commits used: `feat`, `docs`, `chore`, `fix`
- Expected commit: `refactor(session): simplify ResponseAnalyzer for skill-based sessions (Story 12.2)`

### References

- [Source: _bmad-output/planning-artifacts/epics.md:2985-3013 — Epic 12, Story 12.2 AC]
- [Source: _bmad-output/planning-artifacts/epics.md:3007 — `Failed` detection requirement (acknowledged deviation)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Decision 1 amendment, Decision 5 amendment (skill-based activation)]
- [Source: _bmad-output/implementation-artifacts/12-1-parameterize-activation-by-skill.md — Previous story learnings]
- [Source: src/session/analyzer.rs — Current ResponseAnalyzer implementation (1097 lines)]
- [Source: src/session/runner.rs — Dev session call site (search `.analyzer.analyze(`)]
- [Source: src/review/mod.rs:530-540 — `story_reply` variable and doc comment to clean up]
- [Source: _bmad-output/project-context.md — Project rules and conventions]

## Dev Agent Record

### Agent Model Used

anthropic/claude-opus-4-7 (1M context) — via Claude Code CLI, `bmad-dev-story` skill.

### Debug Log References

- Baseline `cargo test` before changes: **1133 passing, 1 failing** (pre-existing `test_build_context_limit_recovery_message_contains_all_sections`).
- Baseline `cargo build` warnings: **32** (included `variant NoReply is never constructed`).
- Final `cargo test` after changes: **1124 passing, 1 failing** (same pre-existing test).
- Final `cargo build` warnings: **31** — one fewer than baseline. The previous `variant NoReply is never constructed` warning is gone (NoReply is now returned as the default). The `variant Continue is never constructed` warning that would have appeared (since analyzer no longer constructs `Continue`) was suppressed with a targeted `#[allow(dead_code)]` on the variant, with a doc comment pointing to Epic 13 as the justification.
- `cargo clippy` errors: only the two pre-existing clippy errors in `src/session/branch.rs` (untouched, explicitly allowed by AC #9). All other clippy errors/warnings in touched files are clean.

### Completion Notes List

- **Pattern constants removed (Task 1):** `REVIEW_FIX_PATTERNS`, `PROCEED_PATTERNS`, `STEP_BY_STEP_PATTERNS`, `YOLO_PATTERNS`, `STORY_SELECTION_PATTERNS` and their doc comments are deleted from `src/session/analyzer.rs`.
- **`analyze()` simplified (Task 2):** Priority 3, 4, 5, 5.5, 6 match blocks removed. Default returns `ResponseAction::NoReply` (was `Continue { reply: "Continue." }`). `story_key: &str` parameter removed. Doc comment, including priority list and `# Arguments`, updated.
- **Doc comments refreshed (Task 3):** Module-level doc, `ResponseAnalyzer` struct doc, and `REVIEW_COMPLETE_PATTERNS` doc updated to reflect the essential-detections-only purpose. Stale reference to `REVIEW_FIX_PATTERNS` at priority 5.5 removed from the `REVIEW_COMPLETE_PATTERNS` doc.
- **Call sites updated (Task 4):** `src/session/runner.rs` and `src/review/mod.rs` drop the third argument from `analyzer.analyze(...)`. In `review/mod.rs`, the unused `story_reply` variable is deleted and the `drive_review_session()` doc comment is rewritten — the `story_reply` paragraph is removed and the "Normal phase" description now reads "analyze responses with `ResponseAnalyzer` for completion/escalation detection". The match arms for `NoReply | Continue { .. }` in both call sites stay untouched, as required by Dev Notes.
- **Tests updated (Task 5):** 10 tests deleted (listed in story 5.1–5.10), the case-insensitive test had its proceed-patterns section removed, `test_analyzer_default_continues` was renamed to `test_analyzer_default_is_no_reply` and re-asserts `NoReply`. All remaining tests' `analyzer.analyze()` calls lost the third `story_key` argument. One new test added: `test_analyzer_unrecognized_responses_return_no_reply`, which covers working text, questions, and patterns that used to be auto-answered (proceed/step-by-step/YOLO/story-selection/review-fix).
- **`Continue` variant preserved for Epic 13 (AC #4):** `ResponseAction::Continue { reply }` was retained in the enum. To satisfy AC #9 (zero new warnings) while keeping the unused variant, a `#[allow(dead_code)]` with a doc comment explaining the Epic 13 extension point was added on the variant. Removed the forward-compatibility comment on `NoReply` and replaced it with a description of its current role (generic no-op distinct from `Continue` for future consultation injection).
- **Untouched by design:** `src/supervisor/rules.rs`, `strip_agent_artifacts()` and its tests, `src/supervisor/architect.rs`, `ContextBuilder`/`build_preamble`/`AgentFactory`, and all of `src/session/branch.rs` (which still has its 2 pre-existing clippy errors). The `NoReply | Continue { .. }` match arms in `runner.rs` and the `NoReply => "Continue.".to_string()` arm in `review/mod.rs` are preserved — the semantic distinction becomes meaningful in Epic 13.
- **Test count matches story exactly:** 1133 − 10 deleted + 1 new = **1124 passing**, plus the 1 pre-existing failing test, as specified in AC #10.
- **`Failed` detection deviation (acknowledged in spec):** Not added, for the reasons documented in the story's "Acknowledged Deviation: `Failed` Detection" section. The `ResponseAction` enum is unchanged beyond the `#[allow(dead_code)]` annotation on `Continue`.

### File List

- `src/session/analyzer.rs` — **modified**: removed 5 pattern constants and their match blocks, simplified `analyze()` signature/body/doc, updated module-level doc and `ResponseAnalyzer` struct doc, refreshed `REVIEW_COMPLETE_PATTERNS` doc, added `#[allow(dead_code)]` + justification comment on `ResponseAction::Continue`, deleted 10 tests, renamed/updated 1 test, added `test_analyzer_unrecognized_responses_return_no_reply`, updated all remaining tests to drop `story_key` argument.
- `src/session/runner.rs` — **modified**: removed `&story.story_key` third argument from `self.analyzer.analyze(...)` call in the dev-session chat loop. Match arms unchanged.
- `src/review/mod.rs` — **modified**: removed `&story_reply` third argument from `self.analyzer.analyze(...)` call, deleted the now-unused `let story_reply = story.specs_path.display().to_string();` line, rewrote the `drive_review_session()` doc comment (removed the `story_reply` paragraph and reworded the Normal phase description).
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — **modified**: `12-2-simplify-response-analyzer` status transitioned `ready-for-dev → in-progress → review`; `last_updated` bumped to `2026-04-18`.
- `_bmad-output/implementation-artifacts/12-2-simplify-response-analyzer.md` — **modified**: checked all Tasks/Subtasks, filled Dev Agent Record (Agent Model, Debug Log References, Completion Notes, File List), added Change Log, set Status to `review`.

## Change Log

| Date       | Author | Summary                                                                                                 |
|------------|--------|---------------------------------------------------------------------------------------------------------|
| 2026-04-18 | JB (via Claude Opus 4.7) | Story 12.2 implemented: simplified `ResponseAnalyzer` by removing persona/menu auto-response patterns (PROCEED, STEP_BY_STEP, YOLO, REVIEW_FIX, STORY_SELECTION), changed default action to `NoReply`, removed `story_key` parameter from `analyze()`, updated call sites in `session/runner.rs` and `review/mod.rs`, refreshed doc comments, removed 10 tests + added 1 new test. `Continue { reply }` variant preserved (with `#[allow(dead_code)]`) as the Epic 13 extension point. Status: `ready-for-dev → review`. |
