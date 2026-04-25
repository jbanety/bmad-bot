# Story 13.11: UI Events for New Pipeline Phases

Status: done

## Story

As a developer monitoring the daemon,
I want terminal UI events for all new pipeline phases and consultations,
So that I can follow the full create→adversarial→critic→dev→review flow in real-time.

## Acceptance Criteria

1. **Given** the `UiRenderer` trait in `src/ui/renderer.rs` **When** this story is implemented **Then** the following new event methods are added under a `// ── Consultation events ──` separator:
   - `consultation_start(&self, consultation_type: &str, story_key: &str, detail: Option<&str>)` — starts sub-spinner "Consulting {display_name}..."
   - `consultation_complete(&self, consultation_type: &str, findings_count: usize, duration: std::time::Duration)` — resolves spinner with findings summary and elapsed time; appends ", memory updated" for critic-type consultations
   - `consultation_error(&self, consultation_type: &str, error: &str)` — resolves spinner with error glyph and message
   - `critic_memory_update(&self, story_key: &str)` — lightweight hook for future renderers; no visible output in ConsoleRenderer (the ", memory updated" suffix in `consultation_complete` already covers the visual)
   **And** `phase_start(&self, phase_name: &str)` is reused for top-level phases (already exists)

2. **Given** the `ConsoleRenderer` implementation in `src/ui/console.rs` **When** new events are emitted **Then** the visual output follows the existing vocabulary (indent + animated `◉` for spinners, `●` green for resolved, `✗` red for errors):
   ```
     ◉ Create Story                                (existing phase_start, 2-space indent)
       ◉ 🔍 Consulting adversarial reviewer...     (sub-spinner via create_spinner(msg, sub:true), 4-space indent)
       ● Adversarial review: 7 findings [1m 12s]   (resolved with glyph_ok + duration)
       ◉ 🧠 Consulting story critic...             (sub-spinner)
       ● Story critic: 3 observations, memory updated [45s]  (resolved, critic appends suffix)
     ● Create Story [3m 47s]                       (existing phase_complete)
     ◉ Dev Session                                 (existing phase_start)
     ...
     ◉ Code Review                                 (existing phase_start)
       ◉ 🧠 Consulting review critic for 2 decision-needed findings...  (sub-spinner with detail)
       ● Review critic: 1 observation, memory updated [32s]             (resolved)
     ● Code Review [2m 05s]                        (existing phase_complete)
   ```
   **And** in plain mode, all emoji are stripped and ASCII fallbacks apply: `[ok]` for `●`, `...` for `◉` animation, no `🔍`/`🧠`

3. **Given** the `NullRenderer` implementation in `src/ui/null.rs` **When** new events are emitted **Then** all 4 new methods are no-ops (consistent with existing pattern)

4. **Given** the `check_consultation_triggers()` function in `src/session/runner.rs` **When** a consultation is triggered and completes **Then**:
   - `consultation_start` is emitted BEFORE `consultation_runner.execute()` with timing capture via `Instant::now()`
   - On success: `consultation_complete` is emitted with approximate findings count and elapsed duration, followed by `critic_memory_update` if the label contains "critic"
   - On failure: `consultation_error` is emitted (resolves spinner with `✗` glyph), then existing `tracing::warn!` continues

5. **Given** the review-critic consultation **When** `consultation_start` is emitted **Then** the `detail` parameter contains the count of decision-needed findings extracted from the response via regex match count (e.g., `Some("2 decision-needed findings")`) **And** for adversarial and critic consultations, `detail` is `None`

## Tasks / Subtasks

- [x] Task 1: Add 4 new methods to `UiRenderer` trait (AC: #1)
  - [x] 1.1 Add `consultation_start(&self, consultation_type: &str, story_key: &str, detail: Option<&str>)` to the trait in `src/ui/renderer.rs` after the Phase events section, under a new `// ── Consultation events ──` separator. Doc comment: `/// A consultation agent has started (sub-phase of an active session).`
  - [x] 1.2 Add `consultation_complete(&self, consultation_type: &str, findings_count: usize, duration: std::time::Duration)` with doc comment: `/// A consultation agent completed and produced findings.`
  - [x] 1.3 Add `consultation_error(&self, consultation_type: &str, error: &str)` with doc comment: `/// A consultation agent failed.`
  - [x] 1.4 Add `critic_memory_update(&self, story_key: &str)` with doc comment: `/// The Story Critic updated its persistent memory file.`
  - [x] 1.5 Update `TestRenderer` in the `#[cfg(test)]` block of `renderer.rs` — add no-op implementations for all 4 new methods
  - [x] 1.6 Update `test_ui_renderer_all_methods_take_shared_ref` test — add calls to all 4 new methods (this test already proves object safety and `&self` compliance, so no separate object-safety test is needed)

- [x] Task 2: Add 4 delegate methods to `UiHandle` (AC: #1)
  - [x] 2.1 Add `consultation_start(&self, consultation_type: &str, story_key: &str, detail: Option<&str>)` to `UiHandle` in `src/ui/mod.rs` under a new `// ── Consultation events ──` separator after Session events. Delegation: `self.0.consultation_start(consultation_type, story_key, detail)`
  - [x] 2.2 Add `consultation_complete(&self, consultation_type: &str, findings_count: usize, duration: Duration)` — same delegation pattern
  - [x] 2.3 Add `consultation_error(&self, consultation_type: &str, error: &str)` — same delegation pattern
  - [x] 2.4 Add `critic_memory_update(&self, story_key: &str)` — same delegation pattern
  - [x] 2.5 Update `test_null_renderer_all_methods_compile_and_succeed` test in `mod.rs` — add calls to all 4 new methods

- [x] Task 3: Add `consultation_display_name` helper + implement 4 methods in `ConsoleRenderer` (AC: #2)
  - [x] 3.1 Add a private helper `fn consultation_display_name(consultation_type: &str) -> (&str, &str)` that returns `(display_name, noun)`:
    - `"adversarial"` → `("Adversarial review", "findings")`
    - `"critic"` → `("Story critic", "observations")`
    - `"review-critic"` → `("Review critic", "observations")`
    - anything else → capitalize first char of `consultation_type`, noun = `"findings"` (return owned Strings via `(String, &str)` if needed)
  - [x] 3.2 Implement `consultation_start` — call `create_spinner(msg, sub: true)`, store with key `format!("consult:{consultation_type}")`. The spinner message is built as:
    - Fancy: `"{emoji} Consulting {display_name}..."` or `"{emoji} Consulting {display_name} for {detail}..."` if `detail.is_some()`. Emoji: `🔍` for `"adversarial"`, `🧠` for anything containing `"critic"`, `📋` default
    - Plain: `"Consulting {display_name}..."` or `"Consulting {display_name} for {detail}..."` — NO emoji (emoji stripped in plain mode to avoid garbled output in non-Unicode terminals)
    - The `story_key` parameter is NOT rendered (already visible in parent `story_start` line) — it is passed for future renderer backends
    - `create_spinner` handles indent (4-space for `sub: true`) and animated `◉` prefix — do NOT include `└─` in the message
  - [x] 3.3 Implement `consultation_complete` — resolve the sub-spinner via `take_spinner("consult:{consultation_type}")`. Print the resolved line: `"{indent}{glyph_ok()} {display_name}: {count} {noun} [{duration}]"`. Rules:
    - `indent` = `"    "` if spinner was sub (from `take_spinner`), else `"  "` (fallback if no spinner found)
    - If `findings_count == 0`: use `"no {noun}"` instead of `"0 {noun}"`
    - If `findings_count == 1`: use singular form — `"1 finding"` / `"1 observation"`
    - If consultation_type contains `"critic"`: append `", memory updated"` before the duration bracket. This produces the single-line output matching the epic spec
    - Duration formatted via existing `format_duration()` helper
    - If `take_spinner` returns `None` (no matching spinner), still print the resolved line — defensive against missing `consultation_start` calls
  - [x] 3.4 Implement `consultation_error` — resolve the sub-spinner via `take_spinner("consult:{consultation_type}")`. Print: `"{indent}{glyph_err()} {display_name} — {error}"`. Same indent logic as `consultation_complete`. Follows `phase_error` visual pattern
  - [x] 3.5 Implement `critic_memory_update` — no-op in ConsoleRenderer (the ", memory updated" suffix is already rendered by `consultation_complete` for critic types). Add `tracing::debug!(action = "critic_memory_update", story_key = %story_key, "Critic memory update event received")` for observability

- [x] Task 4: Implement `NullRenderer` for the 4 new methods (AC: #3)
  - [x] 4.1 Add 4 empty method implementations to `NullRenderer` in `src/ui/null.rs`: `consultation_start`, `consultation_complete`, `consultation_error`, `critic_memory_update` — all no-ops, matching the existing pattern with `_`-prefixed parameters

- [x] Task 5: Emit UI events in `check_consultation_triggers()` (AC: #4, #5)
  - [x] 5.1 Before `self.consultation_runner.execute()` at line 2538 in `src/session/runner.rs`:
    - Compute `detail: Option<String>` — for "review-critic" label, count regex matches: `let match_count = state.compiled_regex.find_iter(response).count(); let detail = if state.config.label == "review-critic" { Some(format!("{match_count} decision-needed findings")) } else { None };`
    - Capture timing: `let consultation_start_time = std::time::Instant::now();`
    - Emit: `self.ui.consultation_start(&state.config.label, &session_state.story_key, detail.as_deref());`
  - [x] 5.2 Inside the `Ok(findings)` arm (line 2539-2546):
    - Compute elapsed: `let consultation_duration = consultation_start_time.elapsed();`
    - Compute approximate count: `let findings_count = findings.lines().filter(|l| !l.trim().is_empty()).count();` — this is an approximation (line count as proxy); acceptable because the number is informational, not programmatic
    - Emit: `self.ui.consultation_complete(&state.config.label, findings_count, consultation_duration);`
    - If `state.config.label.contains("critic")`: emit `self.ui.critic_memory_update(&session_state.story_key);`
  - [x] 5.3 Inside the `Err(e)` arm (line 2549-2561):
    - Compute elapsed: `let consultation_duration = consultation_start_time.elapsed();` (need to move `consultation_start_time` declaration before the match, or use `_` — elapsed is informational)
    - Emit: `self.ui.consultation_error(&state.config.label, &e.to_string());` — this resolves the spinner with `✗` error glyph, giving the operator a clear visual distinction from "no findings"
    - Do NOT emit `critic_memory_update` on error (failed consultations don't update memory)

- [x] Task 6: Unit tests (AC: #1, #2, #3, #4)
  - [x] 6.1 `mod.rs` — test: `test_ui_handle_consultation_events_compile` — call all 4 new methods on `UiHandle::null()` with representative parameters to verify compilation and no panic
  - [x] 6.2 `console.rs` — test: `test_console_consultation_start_complete_lifecycle` — create `ConsoleRenderer::new(true, false)` (plain mode), call `consultation_start("adversarial", "13-11", None)`, verify `has_active_spinners() == true`. Then call `consultation_complete("adversarial", 5, Duration::from_secs(30))`, verify `has_active_spinners() == false` (spinner cleaned up). No panic
  - [x] 6.3 `console.rs` — test: `test_console_consultation_complete_without_start` — call `consultation_complete("unknown", 3, Duration::from_secs(10))` without prior `consultation_start` — must not panic (defensive: if spinner key not found, just print the resolved line)
  - [x] 6.4 `console.rs` — test: `test_console_consultation_error_resolves_spinner` — `consultation_start("critic", "13-11", None)` → `consultation_error("critic", "LLM timeout")` → verify `has_active_spinners() == false`. Spinner must be cleaned up on error path too
  - [x] 6.5 `console.rs` — test: `test_console_sequential_consultations_no_orphaned_spinners` — full sequence: `consultation_start("adversarial", ...)` → `consultation_complete("adversarial", ...)` → `consultation_start("critic", ...)` → `consultation_complete("critic", ...)` → `critic_memory_update("13-11")` → verify `has_active_spinners() == false` at end. No orphaned spinners from sequential consultation flow
  - [x] 6.6 `console.rs` — test: `test_console_consultation_start_with_detail` — `consultation_start("review-critic", "13-11", Some("2 decision-needed findings"))` → verify no panic → `consultation_complete("review-critic", 1, Duration::from_secs(20))` → verify spinner cleaned up
  - [x] 6.7 `console.rs` — test: `test_console_critic_memory_update_is_noop` — call `critic_memory_update("13-11")` on both plain and fancy renderers — must not panic and must not affect spinner state

## Dev Notes

### Architecture Compliance

This story implements the architecture enforcement guideline: "Emit UI events for consultation phases via `UiHandle` (consultation_start, consultation_complete, critic_memory_update)." The architecture prescribes these method names. This story adds a 4th method `consultation_error` following the established `phase_error` pattern for error visibility. The `critic_memory_update` method exists in the trait for architecture compliance and future renderer flexibility; in `ConsoleRenderer` the visual is handled by `consultation_complete` appending ", memory updated" for critic types.

The architecture also mandates:
- The `UiRenderer` trait must remain rendering-backend agnostic — no `indicatif` or `console` types in signatures
- Only primitive types: `&str`, `usize`, `u32`, `f64`, `Duration`, `Option<&str>`
- All methods take `&self`, return `()`
- `UiHandle` must be `Send + Sync + Clone`

[Source: `_bmad-output/planning-artifacts/architecture.md` — Enforcement Guidelines, Terminal UI Layer]

### Current UiRenderer API (27 methods → becomes 31)

The trait at `src/ui/renderer.rs:13-106` has 27 methods across 6 categories: Pipeline (6), Phase (3), Session (4), Tool (2), LLM (6), System (6). This story adds a 7th category: Consultation (4). All methods follow the same pattern: `fn method_name(&self, params...)` with `///` doc comment.

### Existing Visual Vocabulary — Follow Exactly

The `ConsoleRenderer` uses these established patterns:
- **Sub-spinners**: `create_spinner(&self, message, sub: true)` — 4-space indent, animated `◉` in fancy mode. Template: `"{indent}{spinner} {msg}"` where `{spinner}` is indicatif's braille animation and `{msg}` starts with `"◉ {message}"` in fancy
- **Resolved sub-items**: `"    ● {text} [{duration}]"` (4-space indent + green `●` + text + duration bracket) — follows `phase_complete` pattern
- **Error sub-items**: `"    ✗ {text} — {error}"` (4-space indent + red `✗` + text + error) — follows `phase_error` pattern
- **Spinner keys**: Stored in `Mutex<HashMap<String, (ProgressBar, bool)>>` — use `"consult:{label}"` to namespace from phase spinners (e.g., `"consult:adversarial"`, `"consult:critic"`)
- **Emoji**: Currently NOT used in the console renderer (existing glyphs are Unicode symbols `●◉✗⚠└`). This story introduces emoji (🔍, 🧠) as visual differentiators for consultation types. **In plain mode, emoji are stripped entirely** — plain mode assumes a non-Unicode terminal where emoji may render as garbled characters. Follow the same pattern as existing glyphs: fancy → emoji, plain → no prefix (just the text)

[Source: `src/ui/console.rs` — `create_spinner()` at line 84, `glyph_ok()` at line 106, `take_spinner()` at line 229, `phase_complete` at line 353]

### Display Name Mapping — `consultation_display_name()`

Raw config labels must be mapped to human-friendly display names and semantic nouns:

| Label | Display Name | Noun (singular) | Noun (plural) |
|-------|-------------|------------------|----------------|
| `"adversarial"` | `"Adversarial review"` | `"finding"` | `"findings"` |
| `"critic"` | `"Story critic"` | `"observation"` | `"observations"` |
| `"review-critic"` | `"Review critic"` | `"observation"` | `"observations"` |
| anything else | Capitalize first char | `"finding"` | `"findings"` |

The resolved line format: `"    ● {display_name}: {count} {noun} [{duration}]"` with optional `", memory updated"` before the duration for critic types.

### Spinner Lifecycle for Consultations

1. `consultation_start("adversarial", "13-11", None)` → `create_spinner("🔍 Consulting Adversarial review...", sub: true)` → stored as key `"consult:adversarial"`. Output while spinning: `    ◉ 🔍 Consulting Adversarial review...`
2. `consultation_complete("adversarial", 7, 72s)` → `take_spinner("consult:adversarial")` → finish spinner → print `    ● Adversarial review: 7 findings [1m 12s]`
3. `consultation_start("critic", "13-11", None)` → stored as `"consult:critic"`. Output: `    ◉ 🧠 Consulting Story critic...`
4. `consultation_complete("critic", 3, 45s)` → print `    ● Story critic: 3 observations, memory updated [45s]` (critic type → append suffix)
5. If `take_spinner` returns `None` (no matching spinner), still print the resolved line — defensive against missing start calls
6. On error: `consultation_error("adversarial", "LLM timeout")` → `take_spinner("consult:adversarial")` → print `    ✗ Adversarial review — LLM timeout`

### Where UI Events Are Emitted — `check_consultation_triggers()` at `src/session/runner.rs:2497`

This is the ONLY place where consultation execution happens. The function:
1. Iterates `consultation_states` looking for regex trigger matches (line 2503-2509)
2. Updates WAL pipeline phase (line 2513-2530) — Story 13.10
3. Logs `tracing::info!` "Consultation trigger matched" (line 2532-2536)
4. Calls `self.consultation_runner.execute(&state.config)` (line 2538)
5. On `Ok(findings)`: logs completion, returns `ResponseAction::Continue { reply: formatted }` (line 2539-2547)
6. On `Err(e)`: logs warning, returns continue with error message (line 2549-2561)

**Insert UI events at:**
- BEFORE line 2538: compute `detail` (regex match count for review-critic), capture `Instant::now()`, emit `self.ui.consultation_start()`
- INSIDE `Ok` arm (line 2539): compute findings count + elapsed, emit `self.ui.consultation_complete()` + conditional `self.ui.critic_memory_update()`
- INSIDE `Err` arm (line 2549): emit `self.ui.consultation_error()` to resolve spinner with error visual

The `self.ui` field is available on `SessionRunner` (used extensively throughout `runner.rs`, e.g., line 1258, 1506, 1507). The `Instant::now()` is declared before the `match` block so both arms can access elapsed time.

### Review-Critic Detail Parameter

The epic spec shows `"Consulting critic for 2 decision-needed findings..."`. The count comes from how many times the trigger regex `r"- \[ \] \[Review\]\[Decision\]"` matched in the response text. In `check_consultation_triggers()`, `state.compiled_regex` is the compiled version of this pattern. Use `state.compiled_regex.find_iter(response).count()` to get the match count, then pass `Some(format!("{count} decision-needed findings"))` as the `detail` parameter. For non-review-critic consultations, `detail` is `None`.

### Consultation Labels (from pipeline.rs)

Three consultation configs exist with these labels:
- `"adversarial"` — built in `build_create_story_consultations()` at `src/pipeline.rs:1480`
- `"critic"` — built in `build_create_story_consultations()` at `src/pipeline.rs:1480`
- `"review-critic"` — built in `build_review_consultations()` at `src/pipeline.rs:1535`

### Critic Memory Visual Strategy

The epic spec shows `"Story critic: 3 observations, memory updated"` as a single line. The architecture prescribes `critic_memory_update` as a separate method. Resolution: `consultation_complete` for critic types appends `", memory updated"` to the resolved line. `critic_memory_update` is a no-op in `ConsoleRenderer` (just `tracing::debug!` for observability). This satisfies both the visual spec and the architecture mandate. Future renderers (e.g., TUI dashboards) can use `critic_memory_update` to trigger separate UI elements.

### Findings Count — Approximate Heuristic

The `findings` string from `ConsultationRunner::execute()` is unstructured LLM text. `findings.lines().filter(|l| !l.trim().is_empty()).count()` is used as an approximation. This count is informational only (displayed in the spinner resolution line) — no programmatic decisions depend on it. The display uses singular/plural: "1 finding" vs "7 findings", "no observations" vs "3 observations".

### UiHandle Propagation — Already Available

`SessionRunner` receives `UiHandle` at construction. The `self.ui` field is accessible in `check_consultation_triggers()` via `&self`. No new wiring needed.

### Previous Story Intelligence

Story 13.10 (WAL Pipeline Phase Tracking) was the last completed story. Key learnings:
- `check_consultation_triggers()` already receives `&mut SessionState` (added in 13.10 Task 3.1)
- The function has access to `self.ui`, `self.consultation_runner`, and all consultation state
- `SessionState` has `story_key` field accessible for the `story_key` parameter
- Test count: 1252 total passing tests (1 pre-existing failure unrelated)
- Pattern: tests inline in `#[cfg(test)] mod tests { ... }` at bottom of each module

### Git Intelligence

Recent commits (all Epic 13):
- Branch naming: `story/13-11-ui-events-new-phases`
- Commit style: `feat(epic-13): ...` with conventional commits
- All stories in Epic 13 follow the same patterns

### Project Structure Notes

Files to modify:
- `src/ui/renderer.rs` — Add 4 trait methods + update TestRenderer + update existing test
- `src/ui/mod.rs` — Add 4 delegate methods on UiHandle + update existing test
- `src/ui/console.rs` — Add `consultation_display_name()` helper + 4 method implementations + 6 tests
- `src/ui/null.rs` — Add 4 no-op implementations
- `src/session/runner.rs` — Add UI event emissions in `check_consultation_triggers()` (3 insertion points: before execute, Ok arm, Err arm)

Files NOT to modify:
- `src/pipeline.rs` — no changes needed; top-level phase events (Create Story, Dev Session, Code Review) already exist
- `src/session/consultation.rs` — no changes needed; consultation execution is internal
- `src/session/state.rs` — no changes needed

### Testing Standards

- Framework: `#[cfg(test)]` + `cargo test` (Rust native)
- Naming: `test_{module}_{behavior}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert
- All tests inline in same file at bottom
- Use `NullRenderer` for non-visual tests, `ConsoleRenderer::new(true, false)` (plain mode) for visual behavior tests
- Zero-warning policy: `#![deny(clippy::all)]`

### References

- [Source: `_bmad-output/planning-artifacts/architecture.md` — Terminal UI Layer (lines 857-891)]
- [Source: `_bmad-output/planning-artifacts/architecture.md` — Enforcement Guidelines (lines 1066-1094)]
- [Source: `_bmad-output/planning-artifacts/epics.md` — Epic 13, Story 13.11]
- [Source: `src/ui/renderer.rs:1-106` — Current UiRenderer trait (27 methods)]
- [Source: `src/ui/mod.rs:1-283` — Current UiHandle with 27 delegate methods]
- [Source: `src/ui/console.rs:84` — create_spinner() with sub-phase indent and ◉ animation]
- [Source: `src/ui/console.rs:229` — take_spinner() for resolving active spinners]
- [Source: `src/ui/console.rs:262` — has_active_spinners() for test assertions]
- [Source: `src/ui/console.rs:353` — phase_complete pattern: indent + glyph_ok + name + duration]
- [Source: `src/ui/console.rs:370` — phase_error pattern: indent + glyph_err + name + error]
- [Source: `src/ui/null.rs` — NullRenderer no-op implementations]
- [Source: `src/session/runner.rs:2497-2566` — check_consultation_triggers() — sole insertion point for UI events]
- [Source: `src/pipeline.rs:1480-1529` — build_create_story_consultations() with labels "adversarial", "critic"]
- [Source: `src/pipeline.rs:1535-1576` — build_review_consultations() with label "review-critic"]
- [Source: `_bmad-output/implementation-artifacts/13-10-wal-pipeline-phase-tracking.md` — Previous story context]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — clean implementation with no blockers.

### Completion Notes List

- Added 4 new `UiRenderer` trait methods under `// ── Consultation events ──` separator: `consultation_start`, `consultation_complete`, `consultation_error`, `critic_memory_update`
- Added 4 delegate methods on `UiHandle` following the existing pattern
- Implemented `ConsoleRenderer` with full visual vocabulary: sub-spinners with emoji (🔍/🧠) in fancy mode, ASCII-only in plain mode, spinner lifecycle management via `consult:{label}` keys
- Added `consultation_display_name()` helper mapping labels to human-friendly display names and semantic nouns (singular/plural)
- `critic_memory_update` is a no-op in ConsoleRenderer (visual handled by ", memory updated" suffix in `consultation_complete`)
- `NullRenderer` gets 4 no-op implementations matching existing pattern
- Emitted UI events in `check_consultation_triggers()` in `runner.rs`: `consultation_start` before execute, `consultation_complete`/`critic_memory_update` on success, `consultation_error` on failure
- Review-critic `detail` parameter computed from regex match count on response text
- Total test count: 1259 passing (1 pre-existing failure unrelated), 8 new tests added (7 in console.rs, 1 in mod.rs)

### Change Log

- 2026-04-25: Implemented Story 13.11 — UI events for consultation pipeline phases (4 trait methods, 3 renderer implementations, runner integration, 8 tests)

### File List

- src/ui/renderer.rs (modified — 4 new trait methods + TestRenderer implementations + test update)
- src/ui/mod.rs (modified — 4 new UiHandle delegate methods + 1 new test)
- src/ui/console.rs (modified — consultation_display_name helper + 4 ConsoleRenderer implementations + 6 new tests)
- src/ui/null.rs (modified — 4 no-op implementations + test update)
- src/session/runner.rs (modified — UI event emissions in check_consultation_triggers)

### Review Findings

- [x] [Review][Decision→Patch] Consultation start spinner text capitalization/wording mismatch with AC2 examples — FIXED: replaced display_name reuse with dedicated lowercase spinner forms matching AC2 examples — AC2 shows lowercase mid-sentence forms ("Consulting adversarial reviewer...", "Consulting story critic...") but the code reuses `consultation_display_name` which returns capitalized forms ("Consulting Adversarial review...", "Consulting Story critic..."). Two issues: (1) mid-sentence capitalization reads awkwardly, (2) AC shows "adversarial reviewer" vs code's "Adversarial review". The Tasks (3.2) prescribe using `display_name` directly, creating an internal spec inconsistency — code follows Tasks, not AC examples. [src/ui/console.rs:consultation_start]
