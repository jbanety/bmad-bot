# Story 10.1: Module `ui/` — Foundation, Trait & Console Renderer

Status: ready-for-dev

## Story

As a daemon developer,
I want a `ui/` module with a `UiRenderer` trait, a `ConsoleRenderer` implementation, and a `NullRenderer`,
So that user-facing terminal output is decoupled from business logic and rendering backends can be swapped without code changes.

## Acceptance Criteria

1. **Given** `Cargo.toml` is updated **When** the project is compiled **Then** `indicatif` (latest stable, currently 0.18.4) and `console` (latest stable, currently 0.16.2) are added as dependencies **And** the project compiles without warnings.

2. **Given** the new `src/ui/` module **When** I inspect the project structure **Then** the following files exist:
   - `src/ui/mod.rs` — `UiHandle` struct (wraps `Arc<dyn UiRenderer>`), convenience methods that delegate to the inner trait, `pub(crate) mod renderer; pub(crate) mod console; pub(crate) mod null;`
   - `src/ui/renderer.rs` — `UiRenderer` trait with all method signatures
   - `src/ui/console.rs` — `ConsoleRenderer` struct implementing `UiRenderer` using `indicatif::MultiProgress` and `console::style()`
   - `src/ui/null.rs` — `NullRenderer` struct implementing `UiRenderer` as no-op
   - `src/main.rs` — `mod ui;` added (with `#[allow(dead_code)]` — removed in Story 10.2 when wired into pipeline)

3. **Given** the `UiRenderer` trait **When** I inspect the method signatures **Then** the following event categories are covered:
   - **Pipeline events:** `story_start(&self, key, title)`, `story_complete(&self, key, pr_url)`, `story_error(&self, key, error)`, `story_escalated(&self, key, reason)`, `batch_start(&self, count)`, `batch_complete(&self, summary)`
   - **Phase events:** `phase_start(&self, phase_name)`, `phase_complete(&self, phase_name, duration)`, `phase_error(&self, phase_name, error)`
   - **Session events:** `chat_turn(&self, turn, summary)`, `activation_start(&self)`, `activation_complete(&self)`, `completion_detected(&self, story_key)`
   - **Tool events:** `tool_call(&self, tool_name, detail)`, `tool_result(&self, tool_name, detail)`
   - **LLM events:** `llm_request(&self, label, turn)`, `llm_response(&self, label, turn, response_len)`, `llm_error(&self, label, turn, error)`, `llm_retry(&self, label, turn, retry_count, delay_secs)`
   - **System events:** `daemon_start(&self, config_summary)`, `poll_cycle(&self, cycle_num)`, `stories_found(&self, count)`, `crash_recovery_start(&self)`, `crash_recovery_complete(&self, story_key)`, `shutdown_requested(&self)`
   - **And** ALL methods take `&self` (never `&mut self`) and return `()` — UI rendering is fire-and-forget
   - **And** the trait is `Send + Sync` (object safe)
   - **And** no `indicatif` or `console` types appear in the trait signature (backend-agnostic)

4. **Given** the `UiHandle` struct **When** I inspect its implementation **Then** it wraps `Arc<dyn UiRenderer>` and implements `Clone`, `Send`, `Sync` **And** it exposes convenience methods that delegate to the inner trait (e.g., `ui.story_start(key, title)` calls `self.0.story_start(key, title)`).

5. **Given** the `ConsoleRenderer` **When** I inspect its implementation **Then** it uses `indicatif::MultiProgress` for managing concurrent spinners **And** it uses `console::style()` for colored and styled text output **And** the visual vocabulary is:
   - `●` (green) — completed action
   - `◉` (cyan, animated) — in-progress action (spinner)
   - `└` — sub-detail / child event
   - `✗` (red) — error
   - `⚠` (yellow) — warning / escalation
   - Indentation: 2 spaces per nesting level (pipeline → phase → tool)

6. **Given** the `NullRenderer` **When** any method is called **Then** it performs no I/O and returns immediately **And** it can be used in unit tests and CI environments.

7. **Given** `UiHandle` is used in tests **When** I create a `UiHandle` with `NullRenderer` **Then** all method calls compile and succeed without side effects.

## Tasks / Subtasks

- [ ] Task 1: Add `indicatif` and `console` dependencies to `Cargo.toml` (AC: #1)
  - [ ] 1.1 Add `indicatif = "0.18"` to `[dependencies]`
  - [ ] 1.2 Add `console = "0.16"` to `[dependencies]`
  - [ ] 1.3 Verify `cargo check` compiles without warnings
- [ ] Task 2: Create `src/ui/renderer.rs` — the `UiRenderer` trait (AC: #3)
  - [ ] 2.1 Define `UiRenderer` trait with all method signatures from AC #3 — all methods take `&self`, return `()`
  - [ ] 2.2 Ensure trait is `Send + Sync` (add supertrait bounds)
  - [ ] 2.3 Ensure object safety — no generics, no `Self: Sized`, no associated types
  - [ ] 2.4 Use only primitive types in signatures (`&str`, `usize`, `u32`, `f64`, `std::time::Duration`, `Option<&str>`)
  - [ ] 2.5 Add `///` doc comments on all methods
- [ ] Task 3: Create `src/ui/null.rs` — `NullRenderer` (AC: #6)
  - [ ] 3.1 Define `NullRenderer` struct (unit struct)
  - [ ] 3.2 Implement `UiRenderer` for `NullRenderer` — all methods are no-ops (empty body, return `()`)
  - [ ] 3.3 Add `///` doc comment
- [ ] Task 4: Create `src/ui/console.rs` — `ConsoleRenderer` (AC: #5)
  - [ ] 4.1 Define `ConsoleRenderer` struct with `MultiProgress` field and `Mutex<HashMap<String, ProgressBar>>` for spinner tracking
  - [ ] 4.2 Implement `ConsoleRenderer::new()` with no parameters — config-based init deferred to Story 10.2
  - [ ] 4.3 Implement `UiRenderer` for `ConsoleRenderer` — all methods take `&self`, use interior mutability
  - [ ] 4.4 Implement visual vocabulary: `●` green, `◉` cyan spinner, `└` gray sub-detail, `✗` red error, `⚠` yellow warning
  - [ ] 4.5 Use `console::style()` for colors and `indicatif::MultiProgress` for spinner management
  - [ ] 4.6 Handle hierarchical indentation (2 spaces per level)
  - [ ] 4.7 Use `MultiProgress::with_draw_target(ProgressDrawTarget::stderr())` to keep stdout clean
  - [ ] 4.8 Handle internal errors gracefully — log via `tracing::debug!` if a spinner op fails, never propagate to caller
- [ ] Task 5: Create `src/ui/mod.rs` — `UiHandle` wrapper (AC: #4)
  - [ ] 5.1 Define `pub(crate) struct UiHandle(Arc<dyn UiRenderer>)`
  - [ ] 5.2 Implement `Clone` (via `Arc::clone`)
  - [ ] 5.3 Add `UiHandle::null()` convenience constructor that wraps `NullRenderer`
  - [ ] 5.4 Add `UiHandle::console()` constructor that wraps `ConsoleRenderer::new()`
  - [ ] 5.5 Implement all convenience delegation methods
  - [ ] 5.6 Add `pub(crate) mod renderer; pub(crate) mod console; pub(crate) mod null;`
  - [ ] 5.7 Add `///` doc comments on struct and all public methods
- [ ] Task 6: Register module in `src/main.rs` (AC: #2)
  - [ ] 6.1 Add `#[allow(dead_code)] mod ui;` to `src/main.rs` (dead_code allow needed because no consumer exists until Story 10.2)
  - [ ] 6.2 Verify `cargo check` passes with zero warnings
- [ ] Task 7: Write unit tests (AC: #7)
  - [ ] 7.1 Test `UiHandle::null()` creation and all method calls compile and succeed
  - [ ] 7.2 Test `UiHandle` is `Send + Sync + Clone`
  - [ ] 7.3 Test `NullRenderer` implements `UiRenderer` (object safety check)
  - [ ] 7.4 Test `ConsoleRenderer` implements `UiRenderer` (object safety check)
  - [ ] 7.5 Test `UiHandle::console()` creation does not panic
  - [ ] 7.6 Run `cargo test` — all existing tests pass, no pollution from UI output

## Dev Notes

### Architecture Compliance

- **All `UiRenderer` methods take `&self` and return `()`** — never `&mut self`, never `Result`. This is critical: `&self` is required for `Arc<dyn UiRenderer>` dispatch without external `Mutex`. Return `()` because UI rendering is fire-and-forget — the `ConsoleRenderer` handles its own errors internally (log via `tracing::debug!` if a spinner operation fails). Never propagate UI errors to business logic.
- **UiRenderer trait MUST be object-safe** — no generics, no `Self: Sized` constraints, no associated types. This is critical because it will be wrapped in `Arc<dyn UiRenderer>`.
- **No `indicatif` or `console` types in trait signature** — the trait is backend-agnostic. Only primitive types, `&str`, `Duration`, `Option<&str>`, `usize`, `u32`, `f64` in method signatures.
- **`UiHandle` wraps `Arc<dyn UiRenderer>`** — must be `Send + Sync + Clone`. It will be propagated like `ShutdownFlag` and `McpManager`: `cli/run_start()` → `StoryPipeline` → `SessionRunner` / `ReviewRunner` (wiring done in Story 10.2).
- **`ConsoleRenderer` uses `MultiProgress::with_draw_target(ProgressDrawTarget::stderr())`** — this keeps stdout clean and avoids interference with the file tracing layer.
- **`NullRenderer` is zero-cost** — all methods are no-ops (empty body). Used in tests and CI.
- **All types use `pub(crate)` visibility** — consistent with other internal modules (`tools/`, `session/`, etc.). Nothing in `ui/` is exposed outside the crate.

### Dead Code Strategy

This is a foundation story — the `ui/` module is created but NOT consumed until Story 10.2 wires `UiHandle` into the pipeline. The crate root has `#![warn(dead_code)]` (note: `warn`, not `deny` — there's a FIXME to change it later; do NOT change this attribute). To prevent warnings on the entire unused module, add `#[allow(dead_code)]` on the `mod ui;` declaration in `src/main.rs`. This annotation will be removed in Story 10.2.

### Project Structure Notes

- New files to create:
  - `src/ui/mod.rs`
  - `src/ui/renderer.rs`
  - `src/ui/console.rs`
  - `src/ui/null.rs`
- These paths align exactly with the architecture document's project directory structure.
- `src/main.rs` needs `#[allow(dead_code)] mod ui;` added. Current modules: `mod auth`, `mod cli`, `mod config`, `mod git_provider`, `mod llm`, `mod mcp`, `mod notifier`, `mod pipeline`, `mod review`, `mod session`, `mod supervisor`, `mod tools`, `mod watcher`.

### Technical Requirements

- **Rust edition 2024** — all code must follow edition 2024 conventions (rustc 1.93+)
- **`#![deny(clippy::all)]`** is enforced at crate root — zero clippy warnings allowed
- **`#![warn(dead_code)]`** is the current crate-root setting (with FIXME to change to deny later) — do NOT change this attribute
- **Error handling:** No `unwrap()` or `expect()` in production code — only allowed in tests
- **Doc comments:** `///` mandatory on all public structs, traits, enums, and functions
- **Testing:** Tests inline in the same file, inside `#[cfg(test)] mod tests { ... }` at the bottom of each module
- **No `println!` / `eprintln!`** in the `ui/` module — use `indicatif` and `console` APIs for output in `ConsoleRenderer`, and nothing in `NullRenderer`

### Library & Framework Requirements

- **`indicatif` 0.18.4** (latest stable) — progress bars, spinners, `MultiProgress`
  - `MultiProgress` is thread-safe — can be shared across async tasks without additional synchronization
  - Key API: `MultiProgress::new()`, `MultiProgress::add()`, `ProgressBar::new_spinner()`, `ProgressStyle::with_template()`, `ProgressBar::finish_with_message()`, `ProgressBar::finish_and_clear()`, `ProgressDrawTarget::stderr()`
- **`console` 0.16.2** (latest stable) — terminal colors and styles
  - Key API: `style()`, `Style::green()`, `Style::red()`, `Style::yellow()`, `Style::cyan()`, `Style::dim()`, `Term::stderr()`
  - TTY detection (`Term::stdout().is_term()`) and color disable (`set_colors_enabled(false)`) will be used in Stories 10.2 and 10.5

### File Structure Requirements

- Follow existing code patterns: `use` imports at top, then structs, then `impl` blocks, then `#[cfg(test)] mod tests` at bottom
- Keep the `ConsoleRenderer` implementation focused — don't over-engineer the visual output. Stories 10.2-10.5 will refine it.

### Testing Requirements

- All tests use `NullRenderer` via `UiHandle::null()` — zero test pollution from terminal output
- Test naming: `test_{module}_{behavior}_{scenario}` in snake_case
- Arrange → Act → Assert pattern
- Compile-time assertions for `Send + Sync` on `UiHandle`:
  ```
  fn assert_send_sync<T: Send + Sync>() {}
  assert_send_sync::<UiHandle>();
  ```

### ConsoleRenderer Implementation Guidance

`ConsoleRenderer::new()` takes no parameters in this story. Config-based initialization (`ui_mode`, TTY detection) is deferred to Story 10.2.

The `ConsoleRenderer` manages an internal `MultiProgress` instance. For this foundation story, the implementation can be relatively simple — later stories (10.2-10.5) will add more sophisticated rendering.

**Interior mutability:** All `UiRenderer` methods take `&self`, so `ConsoleRenderer` must use interior mutability for mutable state. Use `std::sync::Mutex` (not tokio's) for the spinner map since `indicatif` operations are synchronous and fast. The `MultiProgress` itself is already thread-safe.

**Spinner state tracking:** Use a `HashMap<String, ProgressBar>` protected by `Mutex` to track active phase spinners by name, so `phase_complete()` can find and finish the correct spinner.

**Phase spinner pattern:**
- `phase_start()` creates a new `ProgressBar` spinner added to `MultiProgress` with a message like `◉ Dev Session`
- `phase_complete()` finishes the spinner and replaces the line with `● Dev Session [47s]`
- `phase_error()` finishes the spinner and replaces with `✗ Dev Session — error message`

**Error handling inside ConsoleRenderer:** If a `Mutex::lock()` is poisoned or a spinner operation fails, log the issue via `tracing::debug!` and return `()`. Never panic, never propagate errors.

### Previous Story Intelligence

No previous stories in Epic 10 — this is the foundation story. Relevant patterns from implemented epics:

- **Dependency propagation pattern:** `UiHandle` follows the same propagation pattern as `ShutdownFlag` and `Arc<McpManager>` — created in `run_start()`, passed to `StoryPipeline`, then to `SessionRunner` and `ReviewRunner`. Current constructor signatures:
  - `StoryPipeline::new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>, shutdown: ShutdownFlag, mcp_manager: Arc<McpManager>)`
  - `SessionRunner::new(config: Arc<BotConfig>, agent_factory: Arc<AgentFactory>, shutdown: ShutdownFlag, mcp_manager: Arc<McpManager>)`
  - `ReviewRunner::new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>, agent_factory: Arc<AgentFactory>, shutdown: ShutdownFlag, mcp_manager: Arc<McpManager>)`
- A `ui: UiHandle` parameter will be added to each of these constructors in Story 10.2.
- `cli/mod.rs` `run_start()` creates the pipeline — will create `UiHandle` and pass it through in Story 10.2. Currently has `init_tracing()` with a stdout layer that will be conditionally removed in Story 10.2.

The codebase has no `src/ui/` directory yet — this story creates it from scratch.

### References

- [Source: architecture.md — Project Structure & Boundaries] — defines `src/ui/` module layout
- [Source: architecture.md — Tracing Pattern — Terminal UI Layer] — defines `UiRenderer` trait pattern, `ConsoleRenderer`, `NullRenderer`
- [Source: architecture.md — Enforcement Guidelines] — mandates `UiHandle` propagation and `NullRenderer` in tests
- [Source: project-context.md — Terminal UI Rules] — comprehensive rules for the `ui/` module
- [Source: epics.md — Epic 10 — Story 10.1] — full acceptance criteria and dev notes

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List