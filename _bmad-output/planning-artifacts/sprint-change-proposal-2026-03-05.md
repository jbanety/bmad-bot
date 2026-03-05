---
type: sprint-change-proposal
date: 2026-03-05
author: John (PM)
status: approved
scope: moderate
trigger: user-experience-gap
epic: 10
---

# Sprint Change Proposal — Epic 10: Terminal UI & Developer Experience

## 1. Issue Summary

### Problem Statement

The `bmad-bot start` daemon running in foreground mode (tmux/screen) outputs raw `tracing_subscriber::fmt` logs to stdout — structured debug lines with timestamps, log levels, targets, and key-value fields. This makes it impossible for the user to follow in real-time what the daemon is doing: which story is being processed, which pipeline phase is active, which tools the agent is calling, or whether the LLM is responding.

### Discovery Context

Discovered during daily usage of `bmad-bot start` in a tmux session. The daemon is designed as a foreground process (Architecture Decision 6), and the primary usage mode is tmux/screen — not background daemonization. The current stdout output was designed for debugging, not for operational monitoring.

### Evidence

- `init_tracing()` in `cli/mod.rs` L165-216: stdout layer is a raw `fmt::layer()` or JSON — no user-facing formatting
- ~100+ `tracing::info!/warn!/error!` calls across the codebase — all oriented toward debug, not UX
- Zero TUI dependencies in `Cargo.toml` (no `indicatif`, `console`, `crossterm`, `colored`)
- The Architecture Tracing Pattern (L705-741) explicitly states "NEVER use println! — tracing only" — there is no mechanism for user-facing terminal output
- Tool calls (`edit_file`, `read_file`, `git commit`, `terminal`) are logged as structured tracing events but not surfaced in a readable way

### Desired State

A terminal output similar to GitHub Copilot CLI / Claude Code:

```
● Pipeline: epic-4/story-4.2 — "Agent Session Setup & Chat Loop"
  ◉ Dev Session [turn 3/50]
    ● read_file src/session/runner.rs
      └ 3567 lines (outline)
    ● edit_file src/session/runner.rs (mode: edit)
    ● git commit "feat(session): add context limit recovery"
    ● Terminal: cargo test session::tests
  ● Phase Complete — Dev Session [47s]
  ◉ Push Branch...
```

---

## 2. Impact Analysis

### Epic Impact

| Epic | Impact | Details |
|---|---|---|
| Epic 1-6 | **Retroactive modification** | Existing code in pipeline, session, review modules will receive `ui.emit()` calls |
| Epic 7 (Integration Tests) | **Minor** | Tests must use `NullRenderer` to avoid terminal dependency |
| Epic 8 (Surgical Tooling) | **None** | Tools are independent — UI events emitted from tool call sites, not tool internals |
| Epic 9 (MCP) | **None** | No impact |
| **Epic 10 (NEW)** | **New epic** | Terminal UI & Developer Experience — 5 stories |

No existing epics are obsoleted. No resequencing required. Epic 10 can execute in parallel with Epics 7-9.

### Artifact Conflicts

#### PRD (`prd.md`)

| Section | Change Required |
|---|---|
| Success Criteria / User Success (L37-44) | Add: "The user can follow daemon progress in real-time without reading raw debug logs" |
| Journey 4 — Operations (L155-163) | Add scenario: user monitors tmux terminal, sees structured output with story phases, tool calls, and progress indicators |
| CLI & Configuration (L298-310) | Add **FR43**: "The daemon displays structured, user-facing terminal output (spinners, pipeline phases, tool actions) separate from debug logs which remain in the log file only" |
| Developer Tool Requirements (L197-225) | Mention that `bmad-bot start` in foreground produces a terminal UI (hierarchical actions, progress indicators) |
| Decision 6 reference | Note that foreground mode (tmux/screen) is the primary usage mode and benefits from a dedicated UI |

**MVP scope unchanged** — this is a quality-of-life addition, not a scope reduction.

#### Architecture (`architecture.md`)

| Section | Change Required |
|---|---|
| Tracing Pattern (L705-741) | Amend: stdout no longer receives raw tracing logs when ConsoleRenderer is active. File layer remains JSON. The "NEVER use println!" rule stays for business code, but `ui/` module is the authorized exception |
| Decision 6 — Deployment Model (L379-394) | Amend: foreground mode now has structured terminal UI, not just raw tracing |
| Project Directory Structure (L938-1006) | Add `src/ui/` module with `mod.rs`, `console.rs`, `renderer.rs`, `null.rs` |
| Data Flow (L1101-1116) | Add UI event emission at each pipeline step |
| Module Communication Map | Add `ui/` — consumed by `pipeline.rs`, `session/runner.rs`, `review/mod.rs`, `cli/mod.rs` |
| Requirements to Structure Mapping | Add FR43 → `ui/` row |

#### Project Context (`project-context.md`)

Add rule: "The `ui/` module is the sole user-facing terminal output point. All other modules use `tracing` for debug logging only. The `UiRenderer` trait abstracts the rendering backend — `ConsoleRenderer` for foreground mode, `NullRenderer` for tests and silent mode."

#### Cargo.toml

New dependencies:
- `indicatif` — spinners, progress bars, `MultiProgress` for hierarchical display
- `console` — terminal colors, styles, TTY detection, unicode width

Both crates are from the same author family as `dialoguer` (already in deps). Battle-tested (10M+ and 16M+ downloads/month respectively).

### Technical Impact

- New module `src/ui/` (~500-800 lines estimated)
- `UiHandle` (`Arc<dyn UiRenderer>`) propagated into `StoryPipeline`, `SessionRunner`, `ReviewRunner` structs
- ~30-50 `ui.emit()` insertion points across pipeline, session, review, and CLI modules
- Stdout tracing layer removed when `ConsoleRenderer` is active (file-only logging)
- No changes to tool implementations — UI events emitted at call sites in session/pipeline

---

## 3. Recommended Approach

### Selected Path: Direct Adjustment — New Epic 10

**Classification:** Direct Adjustment — add new epic with stories, update existing artifacts.

### Options Evaluated

| Option | Description | Verdict |
|---|---|---|
| **A — Custom Tracing Layer** | Replace stdout layer with a custom `tracing::Layer` that reformats events | ❌ Not viable — no spinners, no hierarchy, fragile field-name coupling, limited expressivity |
| **B — Module `ui/` with Event Channel** | New module with `UiEvent` enum, `UiRenderer` trait, `indicatif` + `console` | ✅ **Selected** — full control, compile-time safe, testable, extensible |
| **C — Hybrid (Layer + Events)** | Custom layer for simple events + channel for spinners | ❌ Rejected — dual system confusion, maintenance burden |
| **D — `tracing-indicatif`** | Tracing layer that auto-creates progress bars from spans | ❌ Not viable — only handles spans, not events. Tool calls, LLM responses, chat turns are all events, invisible to this approach |

### Rationale

| Criterion | Assessment |
|---|---|
| Implementation effort | Medium — 5 stories, ~1-2 weeks |
| Timeline impact | None — no blocking dependencies, parallelizable with Epics 7-9 |
| Technical risk | Low — `indicatif` + `console` are battle-tested crates |
| Maintenance complexity | Low — single system, compile-time checked via enum/trait |
| Long-term sustainability | Excellent — `UiRenderer` trait enables future migration to `iocraft` (React-like declarative TUI) or `ratatui` (full TUI framework) without modifying business code |
| Team morale | Positive — finally see what the bot does in real-time |

### Effort Estimate

- **Total**: Medium (~5 stories)
- **Risk**: Low
- **Dependencies**: None blocking

---

## 4. Detailed Change Proposals

### 4.1 New Epic: Epic 10 — Terminal UI & Developer Experience

#### Story 10.1: Module `ui/` — Foundation, Trait & Console Renderer

**Goal:** Create the `src/ui/` module with the `UiRenderer` trait, `ConsoleRenderer` implementation (indicatif + console), and `NullRenderer` (tests/CI).

**Tasks:**
- Add `indicatif` and `console` to `Cargo.toml`
- Create `src/ui/mod.rs` — public exports, `UiHandle` (Arc wrapper with convenience methods)
- Create `src/ui/renderer.rs` — `UiRenderer` trait with all method signatures
- Create `src/ui/console.rs` — `ConsoleRenderer` using `indicatif::MultiProgress` for spinners and `console::style()` for colors
- Create `src/ui/null.rs` — `NullRenderer` (no-op implementation for tests and silent/daemon mode)
- Unit tests for `NullRenderer` and basic formatting

**Acceptance Criteria:**
- `UiRenderer` trait covers: pipeline events (story start/complete/error), phase events (start/complete with duration), tool events (call/result), session events (chat turn, activation, completion), LLM events (request/response/error), and system events (poll cycle, shutdown, crash recovery)
- `ConsoleRenderer` displays spinners via `indicatif` and colored text via `console`
- `NullRenderer` compiles and performs no I/O
- `UiHandle` is `Send + Sync + Clone`
- Visual vocabulary defined: `●` completed action, `◉` in-progress (spinner), `└` sub-detail, green/yellow/red/gray color scheme

**Depends on:** None

---

#### Story 10.2: Pipeline Integration — UI Events in Story Lifecycle

**Goal:** Wire `UiHandle` into `StoryPipeline` and emit UI events at each pipeline phase.

**Tasks:**
- Add `UiHandle` field to `StoryPipeline` struct and `new()` constructor
- Emit in `process_story()`: story start, phase start/complete for each phase (dev session, push, PR creation, code review, notification, sprint-status update), story complete/error/escalated with PR URL
- Emit in `process_eligible_stories()`: batch start/end, run summary
- Emit in `recover_and_process()`: crash recovery start/complete
- Pass `UiHandle` from `run_start()` in `cli/mod.rs`
- Modify `init_tracing()`: remove stdout layer when `ConsoleRenderer` is active (logs go to file only); keep stdout layer for `NullRenderer` mode (backward compat)

**Acceptance Criteria:**
- Terminal displays the full lifecycle of each story with clear phase progression
- Spinners animate during active phases, resolve to `●` on completion
- Raw `tracing` logs no longer appear on stdout when UI is active
- All existing tests pass with `NullRenderer` injected
- Polling cycles show a quiet status line (not noisy)

**Depends on:** Story 10.1

---

#### Story 10.3: Session Integration — Tool Calls & Chat Turns Visible

**Goal:** Wire `UiHandle` into `SessionRunner` to surface agent tool calls and chat turns in real-time.

**Tasks:**
- Add `UiHandle` field to `SessionRunner` struct
- Emit in `run_session()`: activation start/complete, each chat turn (turn number + truncated first line of response), completion detected, final commit phase, impact analysis phase, PR summary phase
- Emit tool call events: add `UiHandle` parameter to tool `call()` methods or emit from the tool call logging sites in tools (edit_file, read_file, grep, find_path, list_directory, git, terminal) — each tool call shows tool name + key argument (e.g., `● read_file src/main.rs`, `● git commit "feat: ..."`, `● Terminal: cargo test`)
- Emit LLM events: request sent (spinner start), response received (spinner resolve + response size), error (red)
- Surface retries visibly: retry count, backoff duration
- Surface token refresh events

**Acceptance Criteria:**
- Every tool call by the agent appears in real-time under the active phase spinner
- Chat turns display a compact summary (turn number + truncated response)
- LLM request/response cycle is visible (thinking spinner → response received)
- Retries and token refreshes are visible with counts
- Agent activation sequence is visible (loading agent file, loading config)

**Depends on:** Story 10.2

---

#### Story 10.4: Review Integration — UI Events in Code Review

**Goal:** Wire `UiHandle` into `ReviewRunner` to surface the review cycle.

**Tasks:**
- Add `UiHandle` field to `ReviewRunner` struct
- Emit: review start, review chat turns, fix applications, review complete/failed/skipped
- Reuse patterns established in Story 10.3

**Acceptance Criteria:**
- Review cycle is visible as a sub-phase of the pipeline
- Review fix commits are visible
- Review skip/failure reasons are displayed

**Depends on:** Story 10.3

---

#### Story 10.5: Polish — Visual Vocabulary, Colors & Final Formatting

**Goal:** Refine terminal rendering for a professional, consistent result.

**Tasks:**
- Finalize visual vocabulary (symbols, colors, indentation levels)
- Add TTY detection: non-TTY (pipes, CI) automatically uses `NullRenderer`
- Add config option `ui_mode` in `bmad-bot.yaml`: `"fancy"` (default, ConsoleRenderer), `"plain"` (no colors/spinners), `"silent"` (NullRenderer)
- Add elapsed time display on completed phases (e.g., `● Dev Session [47s]`)
- Test on multiple terminals (tmux, iTerm2, Terminal.app, VS Code integrated terminal)
- Update README with terminal output documentation and screenshots

**Acceptance Criteria:**
- Rendering is consistent and professional across terminals
- TTY detection works correctly (pipes and CI = no fancy output)
- Config option `ui_mode` is respected
- Elapsed times shown on completed phases
- README documents the terminal output format

**Depends on:** Story 10.4

---

### 4.2 PRD Updates

```
Section: Functional Requirements > CLI & Configuration

ADD after FR42:

- **FR43:** The daemon displays structured, user-facing terminal output in foreground
  mode — progress indicators, pipeline phase transitions, agent tool calls, and LLM
  interaction status — separate from debug logs which are written to the log file only.
  The UI is powered by a `UiRenderer` trait with `ConsoleRenderer` (fancy terminal with
  spinners and colors) and `NullRenderer` (silent/test mode) implementations. Terminal
  output mode is configurable via `ui_mode` in `bmad-bot.yaml`.
```

```
Section: Success Criteria > User Success

ADD:

- Developer can follow daemon progress in real-time via structured terminal output
  without reading raw debug logs
```

```
Section: Journey 4 — Operations

AMEND rising action to include:

JB glances at his tmux pane — the daemon shows a clean hierarchical view: story
epic-4/story-4.2 in progress, Dev Session phase active with a spinning indicator,
recent tool calls listed beneath (read_file, edit_file, git commit). No need to
parse log lines — the status is immediately clear.
```

### 4.3 Architecture Updates

```
Section: Tracing Pattern (L705-741)

ADD after existing content:

**Terminal UI Layer (`ui/` module):**
In foreground mode, the stdout tracing layer is replaced by a `ConsoleRenderer` that
displays structured, user-facing output via `indicatif` (spinners, progress) and
`console` (colors, styles). Debug logs are written exclusively to the JSON file layer.
The `UiRenderer` trait abstracts the rendering backend:
- `ConsoleRenderer` — rich terminal output with spinners, hierarchy, colors
- `NullRenderer` — no-op for tests, CI, and silent mode

The "NEVER use println!" rule applies to all business logic modules. The `ui/` module
is the sole authorized terminal output point for user-facing information.

Modules emit UI events via `UiHandle` (an `Arc<dyn UiRenderer>` wrapper) passed through
pipeline, session, and review structs. This is separate from tracing — tracing remains
for debug/operational logging to file.
```

```
Section: Project Directory Structure

ADD:

src/ui/
├── mod.rs          # UiHandle wrapper, public API, spawn logic
├── renderer.rs     # UiRenderer trait definition
├── console.rs      # ConsoleRenderer — indicatif + console
└── null.rs         # NullRenderer — no-op for tests/CI
```

### 4.4 Project Context Update

```
Section: Critical Implementation Rules

ADD new subsection:

### Terminal UI Rules
- The `ui/` module is the SOLE user-facing terminal output point during daemon execution
- All other modules use `tracing` for debug logging only (file layer)
- Never use `println!`/`eprintln!` in daemon runtime code — use `UiHandle` methods
- The `UiRenderer` trait must remain rendering-backend agnostic (no indicatif types in the trait signature)
- `UiHandle` must be `Send + Sync + Clone` — it is shared across async tasks
- Tool call UI events are emitted at call sites (session/pipeline level), not inside tool implementations
- Exception: `cli/mod.rs` interactive commands (init, status, copilot-login) may use `println!` directly as they run before the daemon loop
```

---

## 5. Implementation Handoff

### Change Scope Classification: **Moderate**

Backlog addition (new epic) + document updates. No fundamental replan required.

### Handoff Plan

| Action | Responsible Agent | Priority | Notes |
|---|---|---|---|
| Update PRD with FR43 + Journey/Success Criteria changes | PM (John) | High | Before story execution begins |
| Update Architecture (Tracing Pattern, Structure, Data Flow, Decision 6) | Architect (Winston) | High | Before story execution begins |
| Update Project Context with Terminal UI rules | Dev Agent (Amelia) | Medium | Can be done during Story 10.1 |
| Add Epic 10 to `epics.md` with all 5 stories | SM (Bob) | High | Prerequisite for dev execution |
| Update `sprint-status.yaml` with Epic 10 stories | SM (Bob) | High | After epics.md updated |
| Execute Story 10.1 (Foundation) | Dev Agent (Amelia) | — | First implementation story |
| Execute Stories 10.2-10.5 sequentially | Dev Agent (Amelia) | — | Each depends on previous |

### Success Criteria

- [ ] PRD updated with FR43
- [ ] Architecture updated with `ui/` module documentation
- [ ] Project Context updated with Terminal UI rules
- [ ] Epic 10 added to `epics.md` with 5 stories
- [ ] `sprint-status.yaml` updated
- [ ] All 5 stories implemented and passing tests
- [ ] Terminal output in foreground mode shows structured, readable pipeline progress
- [ ] Raw tracing logs no longer appear on stdout when UI is active
- [ ] Existing tests unaffected (NullRenderer injected)

---

## Summary

| Field | Value |
|---|---|
| **Issue** | Raw tracing logs on stdout — unreadable in foreground mode |
| **Scope** | Moderate — new epic, document updates, no replan |
| **Approach** | New `ui/` module with `indicatif` + `console`, `UiRenderer` trait |
| **Epic** | Epic 10 — Terminal UI & Developer Experience (5 stories) |
| **Artifacts Modified** | PRD, Architecture, Project Context, Cargo.toml, epics.md, sprint-status.yaml |
| **Dependencies** | None blocking — parallelizable with Epics 7-9 |
| **Future Path** | `UiRenderer` trait enables migration to `iocraft` or `ratatui` for full Copilot CLI-level rendering |
| **Approved** | ✅ 2026-03-05 by JB |

---

✅ Correct Course workflow complete, JB!