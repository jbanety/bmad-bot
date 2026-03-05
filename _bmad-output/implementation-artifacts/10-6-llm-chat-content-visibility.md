# Story 10.6: LLM Chat Content Visibility

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer observing `bmad-bot start`,
I want to see a preview of what is sent to and received from the LLM,
So that I can understand agent decisions in real-time without reading log files.

## Acceptance Criteria

1. **Given** `ui_verbosity: verbose` in `bmad-bot.yaml` **When** an LLM request is sent **Then** a truncated preview of the last user message is displayed below the `→` event line.

2. **Given** `ui_verbosity: verbose` **When** an LLM response is received **Then** a truncated preview of the response content is displayed below the `←` event line.

3. **Given** `ui_verbosity: normal` (default) **When** LLM exchanges occur **Then** behavior is identical to current (event markers only, no content).

4. **Given** content exceeding the preview limit **When** displayed **Then** it is truncated with `…` and multi-line content is reflowed with `│` prefixes.

5. **Given** the `UiRenderer` trait **When** new methods are added **Then** they have default no-op implementations and `NullRenderer` requires no changes.

6. **Given** plain mode (`ui_mode: plain`) **When** verbose content is displayed **Then** `│` is replaced with `|` and no ANSI colors are used.

7. **Given** all existing tests **When** they run **Then** they pass without modification.

## Not in Scope

- **Streaming display** — showing tokens as they arrive in real-time (e.g., `← dev turn 1 (streaming) ... ▌`) is explicitly deferred to a future enhancement. Response content preview is emitted AFTER the full response is accumulated, not during streaming.
- **Supervisor content visibility** — `supervisor/architect.rs` passes `None` for the `ui` parameter on all `stream_chat()` / `activate_agent()` calls. No content events are emitted for supervisor sessions. This is intentional — supervisor is a background process, not user-facing.

## Tasks / Subtasks

- [ ] Task 1: Add `ui_verbosity` config field to `BotConfig` (AC: #3)
  - [ ] 1.1 Add `ui_verbosity: String` field to `BotConfig` struct in `src/config/mod.rs` with `#[serde(default = "default_ui_verbosity")]` — default value `"normal"`
  - [ ] 1.2 Add `default_ui_verbosity()` function returning `"normal".to_string()`
  - [ ] 1.3 Add validation in `BotConfig::validate()`: accepted values are `"normal"` and `"verbose"` — reject anything else with `ConfigError::InvalidField`
  - [ ] 1.4 Add unit tests: `test_config_default_ui_verbosity_is_normal`, `test_config_ui_verbosity_accepts_valid_values`, `test_config_ui_verbosity_rejects_invalid_value`
  - [ ] 1.5 Update `VALID_YAML` test constant to include `ui_verbosity: normal` (optional — it's defaulted, but explicit is better for test clarity)

- [ ] Task 2: Add 2 new methods with default impls to `UiRenderer` trait (AC: #5)
  - [ ] 2.1 Add `fn llm_request_content(&self, _label: &str, _turn: u32, _preview: &str) {}` to `UiRenderer` trait in `src/ui/renderer.rs` with default no-op implementation
  - [ ] 2.2 Add `fn llm_response_content(&self, _label: &str, _turn: u32, _preview: &str) {}` to `UiRenderer` trait in `src/ui/renderer.rs` with default no-op implementation
  - [ ] 2.3 Add doc comments: `/// Preview of the prompt/message being sent to the LLM (verbose mode only).` and `/// Preview of the LLM response content (verbose mode only).`
  - [ ] 2.4 Verify `NullRenderer` in `src/ui/null.rs` requires NO changes — default impls cover it
  - [ ] 2.5 Verify `TestRenderer` in `renderer.rs` tests requires NO changes — default impls cover it
  - [ ] 2.6 Add both methods to the `test_ui_renderer_all_methods_take_shared_ref` test in `renderer.rs`

- [ ] Task 3: Add delegation methods to `UiHandle` (AC: #5)
  - [ ] 3.1 Add `pub(crate) fn llm_request_content(&self, label: &str, turn: u32, preview: &str)` to `UiHandle` in `src/ui/mod.rs` delegating to `self.0.llm_request_content(label, turn, preview)`
  - [ ] 3.2 Add `pub(crate) fn llm_response_content(&self, label: &str, turn: u32, preview: &str)` to `UiHandle` in `src/ui/mod.rs` delegating to `self.0.llm_response_content(label, turn, preview)`
  - [ ] 3.3 Add both calls to the `test_null_renderer_all_methods_compile_and_succeed` test in `mod.rs`

- [ ] Task 4: Implement `ConsoleRenderer` verbose content rendering (AC: #1, #2, #4, #6)
  - [ ] 4.1 Add `verbose: bool` field to `ConsoleRenderer` struct in `src/ui/console.rs`
  - [ ] 4.2 Update `ConsoleRenderer::new(plain: bool)` signature to `ConsoleRenderer::new(plain: bool, verbose: bool)` — store both fields
  - [ ] 4.3 Add private helper `fn format_content_preview(&self, content: &str, max_chars: usize) -> Vec<String>` that:
      - Truncates content to `max_chars` characters, appending `…` if truncated
      - Splits on newlines, prefixes each line with the content glyph (`│` in fancy mode, `|` in plain mode)
      - Indents at Level 2.5 (5 spaces) to nest under the LLM event line (which is at 4-space indent)
      - Returns a `Vec<String>` of formatted lines ready for `self.println()`
  - [ ] 4.4 Implement `llm_request_content()` on `ConsoleRenderer`: if `!self.verbose` return immediately (no-op). Otherwise call `format_content_preview(preview, 200)` and print each line via `self.println()`. Use `style(...).dim()` for the content glyph and text in fancy mode.
  - [ ] 4.5 Implement `llm_response_content()` on `ConsoleRenderer`: if `!self.verbose` return immediately (no-op). Otherwise call `format_content_preview(preview, 500)` and print each line via `self.println()`. Use `style(...).dim()` for the content glyph and text in fancy mode.
  - [ ] 4.6 Add `glyph_content_pipe(&self) -> &str` helper returning `"│"` in fancy mode, `"|"` in plain mode
  - [ ] 4.7 Add unit tests:
      - `test_format_content_preview_short_single_line` — content under limit stays intact
      - `test_format_content_preview_truncates_long_content` — content over limit gets `…`
      - `test_format_content_preview_multiline_gets_pipe_prefix` — each line prefixed
      - `test_console_renderer_verbose_mode_field_stored` — verify `verbose` field storage
      - `test_console_renderer_llm_content_methods_do_not_panic` — exercise both new methods in verbose + non-verbose mode
      - `test_console_renderer_plain_verbose_uses_ascii_pipe` — plain mode uses `|` not `│`

- [ ] Task 5: Update `UiHandle::console()` to accept verbose flag (AC: #3)
  - [ ] 5.1 Update `UiHandle::console(plain: bool)` to `UiHandle::console(plain: bool, verbose: bool)` in `src/ui/mod.rs`
  - [ ] 5.2 Update `run_start()` in `src/cli/mod.rs`: pass `config.ui_verbosity == "verbose"` as the second argument to `UiHandle::console()`
  - [ ] 5.3 Update ALL existing call sites of `UiHandle::console()` in tests to pass `false` for the verbose parameter (grep for `UiHandle::console(` across the codebase)
  - [ ] 5.4 Update `test_ui_handle_console_creation_does_not_panic` and `test_console_renderer_implements_ui_renderer_trait_object` tests

- [ ] Task 6: Emit content events at caller level alongside existing UI events (AC: #1, #2)

  **⚠️ CRITICAL APPROACH — Caller-level emission, NOT inside `streaming_chat()`:**

  The existing `ui.llm_request()` and `ui.llm_response()` events are already emitted at the **caller level** (in `runner.rs` and `review/mod.rs`), alongside `log_llm_request()` / `log_llm_response()` calls. The prompt text and response text are already available at these call sites. Content preview events MUST be added at the same locations — **DO NOT modify `streaming_chat()`, `BuiltAgent::stream_chat()`, or `activate_agent()` signatures.**

  Use `truncate_summary()` from `session/runner.rs` (L285, already `pub(crate)`) for truncation — 200 chars for request, 500 chars for response.

  - [ ] 6.1 **`src/session/runner.rs` — `drive_activation_and_recover()`** (2 sites):
      - L1083: After `self.ui.llm_request("dev-recovery", ch_turn as u32)`, add `self.ui.llm_request_content("dev-recovery", ch_turn as u32, &truncate_summary(ch_msg, 200))`
      - After the corresponding `self.ui.llm_response(...)` for ch_response, add `self.ui.llm_response_content("dev-recovery", ch_turn as u32, &truncate_summary(&ch_response, 500))`
      - L1128: Same pattern for the ctx ("Load the project context") turn

  - [ ] 6.2 **`src/session/runner.rs` — `run_session()` initial DS message** (L1375):
      - After `self.ui.llm_request("dev-session", activation_turn as u32)`, add `self.ui.llm_request_content("dev-session", activation_turn as u32, &truncate_summary(&initial_message, 200))`
      - L1387: After `self.ui.llm_response("dev-session", 0, r.len())`, add `self.ui.llm_response_content("dev-session", 0, &truncate_summary(&r, 500))`

  - [ ] 6.3 **`src/session/runner.rs` — `run_session()` recovery activation** (L1540):
      - After `self.ui.llm_request("dev-recovery", activation_turn as u32)`, add request content event
      - L1552: After `self.ui.llm_response("dev-recovery", 0, r.len())`, add response content event

  - [ ] 6.4 **`src/session/runner.rs` — `run_session()` recovery resend** (L1607):
      - After `self.ui.llm_request("dev-recovery", turn_offset as u32)`, add `self.ui.llm_request_content("dev-recovery", turn_offset as u32, &truncate_summary(&last_user_msg, 200))`
      - After corresponding response, add response content event

  - [ ] 6.5 **`src/session/runner.rs` — `run_session()` final commit** (L1715):
      - After `self.ui.llm_request("dev-session", turn as u32)`, add request content with `commit_msg`
      - L1728 + L1784: After both `self.ui.llm_response(...)` sites (normal + retry), add response content events

  - [ ] 6.6 **`src/session/runner.rs` — `run_session()` impact analysis** (L1831):
      - After `self.ui.llm_request("dev-session", (turn + 1) as u32)`, add request content with `&impact_prompt`
      - L1900: After `self.ui.llm_response(...)` (including retry site), add response content events

  - [ ] 6.7 **`src/session/runner.rs` — `run_session()` PR summary** (L1979):
      - After `self.ui.llm_request("dev-session", (turn + 2) as u32)`, add request content with `"[pr-summary]"` (or truncated `pr_summary_prompt`)
      - L2059: After `self.ui.llm_response(...)`, add response content events

  - [ ] 6.8 **`src/session/runner.rs` — `run_session()` main chat loop** (L2179):
      - After `self.ui.llm_request("dev-session", turn as u32)`, add `self.ui.llm_request_content("dev-session", turn as u32, &truncate_summary(&reply, 200))`
      - L2192: After `self.ui.llm_response("dev-session", turn as u32, r.len())`, add `self.ui.llm_response_content("dev-session", turn as u32, &truncate_summary(&r, 500))`

  - [ ] 6.9 **`src/review/mod.rs` — `drive_review_session()` initial** (L549):
      - After `self.ui.llm_request("code-review", 1)`, add request content with `&initial_message`
      - L569: After `self.ui.llm_response("code-review", 1, response.len())`, add response content event

  - [ ] 6.10 **`src/review/mod.rs` — `drive_review_session()` chat loop** (L695):
      - After `self.ui.llm_request("code-review", turn as u32)`, add request content with `&reply`
      - L708: After `self.ui.llm_response("code-review", turn as u32, r.len())`, add response content event

  - [ ] 6.11 **Files that need NO changes:**
      - `src/session/agent.rs` — `streaming_chat()` and `activate_agent()` signatures are unchanged
      - `src/llm/agent_factory.rs` — `BuiltAgent::stream_chat()` and `BuiltAgent::activate_agent()` signatures are unchanged
      - `src/supervisor/architect.rs` — passes `None` for `ui`, no content events needed

- [ ] Task 7: Document `ui_verbosity` in README.md (AC: #3)
  - [ ] 7.1 In the `## Terminal Output` section of `README.md` (added in Story 10.5), add a subsection or paragraph describing `ui_verbosity`:
      - `"normal"` (default): event markers only (`→ dev turn 1`, `← dev turn 1 — 4096 bytes`)
      - `"verbose"`: shows truncated content preview for each LLM exchange
  - [ ] 7.2 Add `ui_verbosity: normal` to the `bmad-bot.yaml` config example block in README (after `ui_mode: fancy`)
  - [ ] 7.3 Add a small terminal output example showing verbose mode output with `│` content lines

- [ ] Task 8: Run full test suite and linting (AC: #7)
  - [ ] 8.1 Run `cargo test` — ALL existing tests must pass with zero failures
  - [ ] 8.2 Run `cargo clippy` — zero new warnings
  - [ ] 8.3 Run `cargo fmt --check` — no formatting issues

## Dev Notes

### Architecture Compliance

- **`UiRenderer` trait remains backend-agnostic:** The 2 new methods use only `&str`, `u32` primitives — no `indicatif` or `console` types in signatures. Default no-op impls ensure backward compatibility.
- **UI events are fire-and-forget:** Both new methods follow the existing `&self → ()` pattern. No error propagation from UI to business logic.
- **`NullRenderer` requires zero changes:** Default trait impls provide the no-op behavior automatically.
- **Content truncation happens at the CALLER, not the renderer:** Callers in `runner.rs` and `review/mod.rs` use `truncate_summary()` (already `pub(crate)` in `session/runner.rs`, L285) to truncate content before passing it to the UI. The renderer only handles formatting (pipe prefix, indentation, dim styling).
- **Content events emitted at caller level, NOT inside `streaming_chat()`:** This follows the exact same pattern as existing `ui.llm_request()` / `ui.llm_response()` events which are emitted in `runner.rs` and `review/mod.rs` alongside `log_llm_request()` / `log_llm_response()`.
- **`ConsoleRenderer` writes to stderr** via `MultiProgress` — content preview lines are printed through the same `self.println()` path, so they don't interfere with active spinners or stdout.
- **No `println!`/`eprintln!`** — all output goes through `UiHandle` as per Terminal UI Rules.
- **Verbose no-op when disabled:** `ConsoleRenderer` methods check `self.verbose` and return immediately when `false`. This means callers always emit content events unconditionally — the renderer decides whether to display them. This avoids `if verbose` checks scattered across call sites.

### Aspirational Terminal Output (Verbose Mode)

```
    → dev turn 1
      │ "Read the story file and begin implementing Task 1. Start with failing tests…"
    ← dev turn 1 — 4096 bytes
      │ "I'll start by implementing the `format_duration()` helper. Let me first write
      │  the failing tests:\n\n```rust\n#[test]\nfn test_format_duration_zero…"
      ► edit_file src/ui/console.rs (edit)
        └ 45 lines changed
```

Plain mode equivalent:

```
    -> dev turn 1
      | "Read the story file and begin implementing Task 1. Start with failing tests..."
    <- dev turn 1 — 4096 bytes
      | "I'll start by implementing the `format_duration()` helper. Let me first write
      |  the failing tests:\n\n```rust\n#[test]\nfn test_format_duration_zero..."
      >> edit_file src/ui/console.rs (edit)
        [ok] 45 lines changed
```

### Project Structure Notes

- Alignment with unified project structure — all changes are within existing modules (`ui/`, `config/`, `cli/`, `session/`, `review/`)
- No new files created — this story adds methods and fields to existing structs
- No new dependencies required — uses existing `indicatif`, `console` crates
- `streaming_chat()` and `activate_agent()` signatures are UNCHANGED — zero impact on `agent.rs`, `agent_factory.rs`, `supervisor/architect.rs`

### Technical Requirements

- **Rust edition 2024** — all code follows edition 2024 conventions
- **Error handling:** No `unwrap()` or `expect()` in production code. Config validation uses `ConfigError::InvalidField` for rejected `ui_verbosity` values
- **Linting:** `#![deny(clippy::all)]` — zero new warnings. All public items have `///` doc comments
- **No `unsafe`** code added

### Library & Framework Requirements

- **No new crate dependencies** — story uses only existing `indicatif`, `console`, `serde`, `tracing`
- **`console` crate:** `style(...).dim()` for content glyph and text in fancy mode — consistent with existing LLM event styling
- **`indicatif` crate:** No changes — content preview lines are static println, not spinners

### File Structure Requirements

Files to modify (estimated line counts):

| File | Changes | Lines |
|------|---------|-------|
| `src/config/mod.rs` | +1 field `ui_verbosity`, default fn, validation, tests | ~25 |
| `src/ui/renderer.rs` | +2 methods with default impls, update test | ~12 |
| `src/ui/mod.rs` | +2 delegation methods, update `console()` signature, update tests | ~15 |
| `src/ui/console.rs` | +`verbose` field, `new()` signature, 2 method impls, `format_content_preview()`, `glyph_content_pipe()`, tests | ~80 |
| `src/cli/mod.rs` | Pass `ui_verbosity` to `UiHandle::console()` | ~2 |
| `src/session/runner.rs` | +18 content event calls (9 request + 9 response) alongside existing `ui.llm_request/response` sites | ~20 |
| `src/review/mod.rs` | +4 content event calls (2 request + 2 response) alongside existing `ui.llm_request/response` sites | ~5 |
| `README.md` | Document `ui_verbosity` option, add to config example | ~15 |
| **Total** | | **~174** |

**Files explicitly NOT modified:**
- `src/session/agent.rs` — `streaming_chat()` and `activate_agent()` signatures unchanged
- `src/llm/agent_factory.rs` — `BuiltAgent::stream_chat()` and `BuiltAgent::activate_agent()` signatures unchanged
- `src/supervisor/architect.rs` — passes `None` for ui, no content events

### Testing Requirements

- All tests use `NullRenderer` via `UiHandle::null()` — zero test pollution from verbose content
- Tests calling `ConsoleRenderer::new()` must be updated: `new(false)` → `new(false, false)` (plain, verbose)
- Tests calling `UiHandle::console()` must be updated: `console(false)` → `console(false, false)` (plain, verbose)
- New unit tests for `format_content_preview()` helper — truncation, multi-line, pipe prefix
- New unit tests for `ui_verbosity` config — default, valid values, rejection
- NO changes needed in `streaming_chat()` tests — signatures unchanged
- Run `cargo test` — ALL tests must pass
- Run `cargo clippy` — zero new warnings
- Run `cargo fmt --check` — clean

### Previous Story Intelligence

**Story 10.5 (Polish — Visual Vocabulary, Colors & Final Formatting, `review`):**
- Added `plain_mode: bool` field to `ConsoleRenderer`, changed `new()` to `new(plain: bool)`
- Added 9 glyph helper methods (`glyph_ok`, `glyph_err`, `glyph_warn`, `glyph_progress`, `glyph_sub`, `glyph_arrow_out`, `glyph_arrow_in`, `glyph_tool`, `glyph_url_arrow`)
- Changed `llm_request()` glyph from `►` to `→` (dim), `llm_response()` glyph from `●` to `←` (dim)
- `UiHandle::console(plain: bool)` — this story changes it to `console(plain: bool, verbose: bool)`
- 8 call sites updated for `ConsoleRenderer::new(bool)` (1 production + 7 tests)
- All 1174 tests passed, 0 new clippy warnings
- **Files modified:** `src/ui/console.rs`, `src/ui/mod.rs`, `src/cli/mod.rs`, `README.md`

**Story 10.4 (Review Integration, `review`):**
- Changed `None` → `Some(&self.ui)` for all `stream_chat()`/`activate_agent()` calls in review
- Made `truncate_summary` `pub(crate)` in `session/runner.rs` for cross-module reuse — **reuse this for content truncation**
- **Files modified:** `src/review/mod.rs`, `src/session/runner.rs`, `src/pipeline.rs`

**Story 10.3 (Session Integration, `review`):**
- Added `ui: Option<&UiHandle>` parameter to `streaming_chat()` and `activate_agent()` in `session/agent.rs`
- Tool call interception via `MultiTurnStreamItem` variant inspection in `streaming_chat()`
- Per-tool detail formatting: `edit_file` → `"{path} ({mode})"`, `git` → `"{sub_action} {key_arg}"`, etc.
- `truncate_str()` helper (L498-507 in agent.rs) — private, NOT reusable outside agent.rs. Use `truncate_summary()` from runner.rs instead.
- **Files modified:** `src/session/runner.rs`, `src/session/agent.rs`, `src/llm/agent_factory.rs`, `src/review/mod.rs`

**Story 10.2 (Pipeline Integration, `review`):**
- Wired `ui: UiHandle` into `StoryPipeline`, `SessionRunner`, `ReviewRunner`
- TTY detection + `ui_mode` config → `ConsoleRenderer` or `NullRenderer` selection in `run_start()`
- `console::set_colors_enabled(false)` for plain mode already in `run_start()`
- **Files modified:** `src/cli/mod.rs`, `src/pipeline.rs`, `src/session/runner.rs`, `src/review/mod.rs`, `src/config/mod.rs`

**Story 10.1 (Foundation, `review`):**
- Created `UiRenderer` trait (27 methods, all `&self → ()`), `ConsoleRenderer`, `NullRenderer`, `UiHandle`
- `ConsoleRenderer` uses `MultiProgress` + `console::style()`, spinners for phases, `Mutex<HashMap>` for spinner tracking
- **Files created:** `src/ui/mod.rs`, `src/ui/renderer.rs`, `src/ui/console.rs`, `src/ui/null.rs`

### Git Intelligence

Last 5 implementation commits:
1. `f8a7a01` — `docs(story): add validated story 10.5 — polish visual vocabulary colors & final formatting`
2. `202b846` — `feat(ui): add review integration UI events in code review (Story 10.4)` — modified `review/mod.rs`, `pipeline.rs`, `session/runner.rs`
3. `628fdad` — `feat(session): add UI event emissions for tool calls, chat turns, and LLM lifecycle (Story 10.3)` — modified `session/runner.rs`, `session/agent.rs`, `llm/agent_factory.rs`, `review/mod.rs`
4. `2235fe3` — `feat(ui): wire UiHandle into pipeline, emit lifecycle events (Story 10.2)` — modified `cli/mod.rs`, `pipeline.rs`, `session/runner.rs`, `review/mod.rs`, `config/mod.rs`
5. `bfa0645` — `feat(ui): story 10.1 — UiRenderer trait, ConsoleRenderer, NullRenderer, UiHandle` — created all `ui/` files

### Key Implementation Notes

1. **Content events are emitted at CALLER level — same pattern as existing UI events.** The `ui.llm_request()` and `ui.llm_response()` calls already live in `runner.rs` and `review/mod.rs`, alongside `log_llm_request()` / `log_llm_response()`. Content events go at the same locations, using the same `label`, `turn`, and message variables already in scope. DO NOT modify `streaming_chat()` or `activate_agent()` signatures.

2. **Use `truncate_summary()` from `session/runner.rs` (L285, `pub(crate)`)** for content truncation. It handles Unicode boundary safety and `…` suffix. Do NOT use `truncate_str()` from `session/agent.rs` — it's private (`fn`, not `pub`). In `review/mod.rs`, import via `use crate::session::runner::truncate_summary`.

3. **The `verbose` flag lives in `ConsoleRenderer`, NOT in `UiHandle`.** The renderer decides whether to display content. Callers always emit content events unconditionally — `ConsoleRenderer` returns immediately from `llm_request_content()`/`llm_response_content()` when `self.verbose == false`. This keeps call sites clean (no `if verbose` checks scattered around).

4. **Content preview is NOT streamed.** Response content preview is emitted AFTER `stream_chat()` returns with the accumulated response, not during streaming. This is explicitly deferred to a future enhancement per the architect brief.

5. **Retry loops emit content events on each attempt.** This matches the existing behavior where `ui.llm_request()` is called on every retry attempt. Content events follow the same pattern for consistency.

6. **Call site inventory for content events — 11 request sites + 10 response sites across 2 files:**

   **`src/session/runner.rs`** (9 request + 8 response):
   | Location | Function | Label | Prompt variable |
   |----------|----------|-------|----------------|
   | L1083 | `drive_activation_and_recover()` | `"dev-recovery"` | `ch_msg` |
   | L1128 | `drive_activation_and_recover()` | `"dev-recovery"` | `"Load the project context"` |
   | L1375 | `run_session()` initial DS | `"dev-session"` | `&initial_message` |
   | L1540 | `run_session()` recovery activation | `"dev-recovery"` | `&initial_message` |
   | L1607 | `run_session()` recovery resend | `"dev-recovery"` | `&last_user_msg` |
   | L1715 | `run_session()` final commit | `"dev-session"` | `commit_msg` |
   | L1831 | `run_session()` impact analysis | `"dev-session"` | `&impact_prompt` |
   | L1979 | `run_session()` PR summary | `"dev-session"` | `&pr_summary_prompt` |
   | L2179 | `run_session()` main chat loop | `"dev-session"` | `&reply` |

   **`src/review/mod.rs`** (2 request + 2 response):
   | Location | Function | Label | Prompt variable |
   |----------|----------|-------|----------------|
   | L549 | `drive_review_session()` initial | `"code-review"` | `&initial_message` |
   | L695 | `drive_review_session()` chat loop | `"code-review"` | `&reply` |

### Estimated Scope

This is a **2-point story** (as estimated in the architect brief). Changes span ~174 lines across 8 files. The heaviest change is `ConsoleRenderer` (~80 lines for verbose rendering + tests) and `runner.rs` (~20 lines for content event call sites).

### References

- [Source: _bmad-output/planning-artifacts/architect-brief-llm-chat-visibility.md] — Full architect brief with proposed approach, AC, and scope estimate
- [Source: _bmad-output/planning-artifacts/epics.md#L2281-2711 — Epic 10: Terminal UI] — Epic context, all stories 10.1-10.5, dependency order
- [Source: _bmad-output/planning-artifacts/architecture.md#L980-1055 — Project Structure] — Complete directory structure with ui/ module
- [Source: _bmad-output/project-context.md#L192-205 — Terminal UI Rules] — UiRenderer backend-agnostic, UiHandle propagation, tool call UI at call sites
- [Source: _bmad-output/project-context.md#L121-169 — Code Quality & Style Rules] — Modular structure, doc comments, no dead code
- [Source: _bmad-output/project-context.md#L112-121 — Testing Rules] — Inline tests, descriptive names, no real LLM calls
- [Source: _bmad-output/project-context.md#L30-39 — Rust Rules] — Edition 2024, thiserror, no unwrap in production
- [Source: src/ui/renderer.rs — UiRenderer trait] — 27 existing methods, all `&self → ()`, object-safe, Send + Sync
- [Source: src/ui/console.rs — ConsoleRenderer] — `plain_mode` field, glyph helpers, `MultiProgress` stderr rendering
- [Source: src/ui/console.rs#L371-388 — llm_request/llm_response] — Current event marker rendering with `→`/`←` glyphs at Level 2 (4-space indent)
- [Source: src/ui/mod.rs — UiHandle] — `Arc<dyn UiRenderer>` wrapper, `console(plain: bool)` and `null()` constructors
- [Source: src/session/runner.rs#L285-294 — truncate_summary()] — `pub(crate)`, Unicode-safe truncation with `…` suffix — USE THIS for content truncation
- [Source: src/session/runner.rs#L1228-2347 — run_session()] — All ui.llm_request/response call sites where content events must be added
- [Source: src/session/runner.rs#L1041-1213 — drive_activation_and_recover()] — 2 additional ui.llm_request/response sites
- [Source: src/review/mod.rs#L521-720 — drive_review_session()] — 2 ui.llm_request/response sites for code review
- [Source: src/config/mod.rs#L75-120 — BotConfig] — Existing `ui_mode` field pattern to follow for `ui_verbosity`
- [Source: src/config/mod.rs#L128-130 — default_ui_mode()] — Pattern for default function
- [Source: 10-5 story — Completion Notes] — 1174 tests passing, `ConsoleRenderer::new(plain: bool)` signature, all glyph helpers, 8 call sites updated
- [Source: 10-4 story — Completion Notes] — `truncate_summary` made `pub(crate)` for cross-module reuse

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List