---
type: architect-brief
from: Winston (Architect)
to: Product Owner
date: '2026-02-10'
subject: 'New Epic Request — Surgical Development Tooling Refactor'
related_decision: 'Architecture Decision 7'
status: ready-for-po
---

# Architect Brief: Surgical Development Tooling

## Context

The BMAD Bot agent currently uses a monolithic `FsTool` with a single `write` action that **rewrites entire files** on every edit. This is the #1 blocker to agent productivity — it burns ~8x more tokens than necessary, risks code loss via LLM truncation, and forces blind navigation of the codebase.

Architecture Decision 7 has been documented and committed (`7161267`). It specifies replacing `FsTool` with 5 focused tools modeled on the Claude Code / Zed agent-mode pattern — the proven industry standard for LLM-driven development.

## What Changed in Architecture

- **New Decision 7** added to `architecture.md` — full design specs for each tool
- **`project-context.md`** updated with new tool inventory, design principles, and critical rules
- Both files committed on `main`

## What the PO Needs to Create

A **new Epic 8** (refactoring) with stories to replace the existing `FsTool` implementation. This is not greenfield — the current tools work, but they need to be split and enhanced.

### Suggested Epic

**Epic 8: Surgical Development Tooling**
Replace the monolithic FsTool with focused, Claude Code-style tools to dramatically improve agent token efficiency, code safety, and codebase navigation. After this epic, the dev agent edits files surgically instead of rewriting them, searches code with grep, and navigates with outlines — matching the capability level of modern AI coding assistants.

### Suggested Story Breakdown (5 stories)

The dependency order matters — each story builds on the previous.

#### Story 8.1: ReadFileTool — Partial Reading & Outline Mode

**Why first:** Every other tool depends on the agent being able to read files intelligently. This is the foundation.

**Scope:**
- New `tools/read_file.rs` implementing rig `Tool` trait
- Read with optional `start_line` / `end_line` (1-indexed, inclusive)
- Automatic outline mode for files > 300 lines (regex-based symbol extraction with line numbers)
- Line numbers always included in output
- Project-root security boundary (same as current FsTool)
- Full unit test suite
- Update `tools/mod.rs` to export `ReadFileTool`

**Replaces:** `FsTool` `read` action

**FRs impacted:** FR9 (tools exposed to agent)

---

#### Story 8.2: EditFileTool — Surgical Search-Replace Editing

**Why second:** This is the biggest value unlock — surgical edits instead of full rewrites.

**Scope:**
- New `tools/edit_file.rs` implementing rig `Tool` trait
- Three modes: `edit` (search_replace), `create` (new file), `overwrite` (full rewrite)
- `edit` mode: `Vec<EditOperation>` with `old_text` → `new_text` pairs
- Validation: `old_text` must exist and be unique — clear error messages with line numbers on ambiguity or miss
- `create` mode: fails if file exists, auto-creates parent directories
- `overwrite` mode: requires file to already exist
- Sequential application of multiple edits with offset recalculation
- Returns affected line ranges for verification
- Full unit test suite
- Update `tools/mod.rs` to export `EditFileTool`

**Replaces:** `FsTool` `write` action

**FRs impacted:** FR9

---

#### Story 8.3: GrepTool & FindPathTool — Codebase Search & Navigation

**Why third:** The agent needs to find code before it can edit it. These two are independent but small enough to combine.

**Scope:**
- New `tools/grep.rs`: regex search across file contents with `include_pattern` glob filter, `context_lines`, pagination (`max_results` default 20). Uses the `regex` crate (already a dependency) + `walkdir` or `glob` crate for traversal. Respects `.gitignore`.
- New `tools/find_path.rs`: glob-based file path search with pagination (`max_results` default 50). Respects `.gitignore`.
- Both follow standard rig Tool pattern
- Full unit test suites for both
- Update `tools/mod.rs` to export both
- Add `glob` and/or `walkdir` to `Cargo.toml` if needed

**New tools** (no replacement — these didn't exist before)

**FRs impacted:** FR9

---

#### Story 8.4: ListDirectoryTool & FsTool Removal — Complete Migration

**Why fourth:** Extract the last useful action from FsTool, then remove it entirely.

**Scope:**
- New `tools/list_directory.rs`: list directory contents with types and sizes, project-root security boundary
- Remove `tools/fs.rs` entirely
- Update `tools/mod.rs`: remove `FsTool` export, add `ListDirectoryTool` export
- Update `supervisor/read_tool.rs` to use `ReadFileTool` instead of `FsTool`
- Migrate or delete all `FsTool` unit tests (each new tool already has its own tests from prior stories)
- Verify `cargo test` passes with zero references to `FsTool`

**Replaces:** `FsTool` `list` action + removes `FsTool` `mkdir`, `delete`, `exists` (pushed to TerminalTool)

**FRs impacted:** FR9

---

#### Story 8.5: Agent Integration — Preamble, Registration & Session Update

**Why last:** Wires everything together — the agent can now use the new tools.

**Scope:**
- Update `session/runner.rs` `build_preamble()`: expand tool list and add "Tool Usage Rules" section per Decision 7
- Update all 3 agent builders (`build_anthropic_agent`, `build_openai_agent`, `build_copilot_agent`): register 8 tools instead of 4 (edit_file, read_file, grep, find_path, list_directory, git, terminal, ask_supervisor) + ThinkTool
- Update `review/mod.rs` if it registers tools separately
- Update FR9 references in story acceptance criteria if needed
- Smoke test: verify agent session starts with all 9 tools visible in tool definitions
- Update tool definition descriptions for maximum LLM clarity

**FRs impacted:** FR8, FR9, FR11

---

### Dependencies & Ordering

```
8.1 ReadFileTool ──► 8.2 EditFileTool ──► 8.3 Grep + FindPath ──► 8.4 ListDir + FsTool Removal ──► 8.5 Integration
```

Stories 8.1 and 8.2 are tightly coupled (EditFileTool may use ReadFileTool internally for validation). Stories 8.3 is independent of 8.2 in code but ordered for sprint coherence. Story 8.4 does the cleanup. Story 8.5 is the integration point.

### Impact Estimate

| Metric | Before | After |
|--------|--------|-------|
| Tokens per file edit (500-line file) | ~8,000 | ~900 |
| Risk of code loss (LLM truncation) | High | Near zero |
| Tool calls to find code | 5-10 (list/read loops) | 1-2 (grep → read range) |
| Agent tools registered | 5 | 9 |

### Existing Epics Impacted

- **Epic 4, Story 4.1** ("Rig Tools Implementation") — already implemented. Epic 8 is a refactoring follow-up. The PO should reference Story 4.1 as the baseline.
- **Epic 4, Story 4.2** ("Agent Session Setup & Chat Loop") — preamble changes in Story 8.5. Already implemented, needs update.
- **Epic 7** (Integration Tests) — existing tool integration tests will need updating after Epic 8. Could be handled as a follow-up or within Story 8.4.

### Reference Documents

- `_bmad-output/planning-artifacts/architecture.md` — Decision 7 (full design specs)
- `_bmad-output/project-context.md` — Updated tool inventory and critical rules
- `src/tools/fs.rs` — Current FsTool implementation (to be replaced)
- `src/session/runner.rs` — Current preamble and agent builder (to be updated)