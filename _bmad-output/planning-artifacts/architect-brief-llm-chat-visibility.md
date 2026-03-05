---
type: architect-brief
from: Winston (Architect)
to: Product Owner
date: '2026-03-05'
subject: 'New Story Request — LLM Chat Content Visibility in Terminal UI'
related_decision: 'Epic 10 — Terminal UI'
status: ready-for-po
---

# Architect Brief: LLM Chat Content Visibility

## Context

Epic 10 delivered a polished terminal UI with a consistent visual vocabulary (`●`, `◉`, `→`, `←`, `►`, etc.). The developer can now see **that** LLM exchanges happen, but not **what** is being exchanged. Current output:

```
    → dev turn 1
    ← dev turn 1 — 4096 bytes
      ► edit_file src/session/runner.rs (edit)
```

The developer has no visibility into the actual prompt sent or the LLM's reasoning/response text. When debugging agent behavior or understanding why the agent made a particular decision, the only option today is to dig through structured log files — a poor developer experience for a tool meant to run in the foreground.

## Problem Statement

During a live `bmad-bot start` session, the developer cannot see:

1. **What the agent receives** — the system prompt, activation preamble, and tool results that form the context
2. **What the LLM responds** — the reasoning, decisions, and text content the model produces
3. **Streaming progress** — whether the LLM is actively generating or stalled

This makes it difficult to:
- Debug agent misbehavior in real-time
- Understand why the agent chose a specific approach
- Detect prompt issues or context truncation early
- Build trust in the autonomous pipeline

## Constraints

- **`UiRenderer` trait is the stable contract** — 27 fire-and-forget methods, all `&self → ()`. Any content visibility must work within this pattern.
- **Content can be massive** — a single LLM response can be 4K–100K+ tokens. Dumping raw content to the terminal is not viable in normal mode.
- **Streaming architecture** — `rig` delivers responses via `StreamingChat` as incremental chunks. The UI could tap into this stream.
- **`ConsoleRenderer` writes to stderr** via `MultiProgress` — must not interfere with active spinners.
- **Tests use `NullRenderer`** — any new methods must have no-op implementations.
- **No `println!`/`eprintln!`** in daemon runtime — all output goes through `UiHandle`.

## Proposed Approach

### New `ui_verbosity` Config Option

Add a verbosity level to `bmad-bot.yaml`:

```yaml
ui_mode: fancy           # existing: fancy | plain | silent
ui_verbosity: normal     # NEW: normal | verbose
```

| Verbosity | Behavior |
|-----------|----------|
| `normal` (default) | Current behavior — event markers only (`→ dev turn 1`, `← dev turn 1 — 4096 bytes`) |
| `verbose` | Show truncated content preview for each LLM exchange + streaming indicator |

### UiRenderer Trait Additions (minimal)

Add 2 new methods with default no-op implementations (backward compatible):

```rust
/// Preview of the prompt/message being sent to the LLM.
fn llm_request_content(&self, _label: &str, _turn: u32, _preview: &str) {}

/// Preview of the LLM response content (truncated).
fn llm_response_content(&self, _label: &str, _turn: u32, _preview: &str) {}
```

Default implementations make this backward compatible — existing `NullRenderer` and any external implementations continue to work unchanged.

### Content Truncation Strategy

- **Request preview**: First 200 chars of the last user message (not the full context), suffixed with `…` if truncated
- **Response preview**: First 500 chars of the LLM response text, suffixed with `…` if truncated
- **Tool call content**: Already visible via existing `tool_call` / `tool_result` events — no change needed

### Terminal Rendering (verbose mode)

```
    → dev turn 1
      │ "Read the story file and begin implementing Task 1. Start with failing tests…"
    ← dev turn 1 — 4096 bytes
      │ "I'll start by implementing the `format_duration()` helper. Let me first write
      │  the failing tests:\n\n```rust\n#[test]\nfn test_format_duration_zero…"
      ► edit_file src/ui/console.rs (edit)
        └ 45 lines changed
```

The `│` (dim) prefix denotes content lines, visually subordinate to the `→`/`←` event markers. Indented at Level 2.5 (5 spaces) to nest under the LLM event.

Plain mode equivalent: `|` (pipe character).

### Emission Points in Session Runner

The `streaming_chat()` and `activate_agent()` methods in `session/runner.rs` already have access to both:
- The messages being sent (available before the API call)
- The accumulated response (available after streaming completes)

These are the two insertion points for emitting the new UI events. No architectural changes needed — just adding `ui.llm_request_content(...)` / `ui.llm_response_content(...)` calls alongside the existing `ui.llm_request(...)` / `ui.llm_response(...)` calls.

### What About Streaming Display?

A future enhancement (not in initial scope) could show tokens as they arrive:

```
    ← dev turn 1 (streaming)
      │ I'll start by implementing the `format_du▌
```

This would require a spinner-like mechanism for the streaming line, updating in place as chunks arrive. The `indicatif` `ProgressBar` message can be updated incrementally, making this technically feasible. However, this adds complexity and should be a separate follow-up if desired.

## Suggested Story

**Story 10.6: LLM Chat Content Visibility**

_As a developer observing `bmad-bot start`,_
_I want to see a preview of what is sent to and received from the LLM,_
_So that I can understand agent decisions in real-time without reading log files._

### Acceptance Criteria

1. **Given** `ui_verbosity: verbose` in `bmad-bot.yaml` **When** an LLM request is sent **Then** a truncated preview of the last user message is displayed below the `→` event line.
2. **Given** `ui_verbosity: verbose` **When** an LLM response is received **Then** a truncated preview of the response content is displayed below the `←` event line.
3. **Given** `ui_verbosity: normal` (default) **When** LLM exchanges occur **Then** behavior is identical to current (event markers only, no content).
4. **Given** content exceeding the preview limit **When** displayed **Then** it is truncated with `…` and multi-line content is reflowed with `│` prefixes.
5. **Given** the `UiRenderer` trait **When** new methods are added **Then** they have default no-op implementations and `NullRenderer` requires no changes.
6. **Given** plain mode **When** verbose content is displayed **Then** `│` is replaced with `|` and no ANSI colors are used.
7. **Given** all existing tests **When** they run **Then** they pass without modification.

### Estimated Scope

- **`src/ui/renderer.rs`**: +2 methods with default impls (~6 lines)
- **`src/ui/console.rs`**: +2 method implementations with truncation + `│` formatting (~40 lines), verbosity field
- **`src/ui/mod.rs`**: +2 delegation methods on `UiHandle` (~10 lines)
- **`src/session/runner.rs`**: +4 call sites (request/response content in `streaming_chat` + `activate_agent`) (~12 lines)
- **`src/config/mod.rs`**: +1 field `ui_verbosity` with default + validation (~8 lines)
- **`src/cli/mod.rs`**: pass verbosity to `ConsoleRenderer` (~2 lines)
- **`README.md`**: document `ui_verbosity` option (~10 lines)
- **Total**: ~90 lines across 7 files — **2-point story**

## Dependencies

- **Epic 10 (Stories 10.1–10.5)**: All done/review — foundation in place
- **No external dependencies**: Uses existing `rig` streaming types and `indicatif`/`console` crates