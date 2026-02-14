# Sprint Change Proposal — git2 Reference Cleanup in Epic 7

**Date:** 2026-02-14
**Author:** John (PM Agent)
**Status:** Approved
**Scope:** Minor

---

## Section 1: Issue Summary

### Problem Statement

Story 4.4 (Migrate All Git Operations from git2 to Git CLI) was completed successfully — all production code migrated from `git2` (libgit2) to Git CLI subprocess calls, and `git2` was removed from `Cargo.toml`. However, the downstream impact was not propagated to Epic 7 (Integration Tests) stories that had not yet been implemented.

### Discovery Context

- **Discovered by:** JB (project owner) during review of remaining stories
- **When:** 2026-02-14, during sprint execution
- **How:** Noticed git2 references still present in stories queued for development

### Evidence

| Evidence | Detail |
|----------|--------|
| `git2` not in `Cargo.toml` | `grep -n "git2" Cargo.toml` returns zero matches — crate fully removed by Story 4.4 |
| `determine_base_branch` signature changed | Old: `(story: &StoryInfo, repo: &Repository, default: &str)` → New: `(story: &StoryInfo, repo_path: &Path, default_branch: &str)` |
| `preserve_partial_work` uses Git CLI | Implementation uses `tokio::process::Command::new("git")`, not `git2` |
| Architecture doc already updated | `revisedAt: 2026-02-11` with revision note documenting the git2→CLI migration |

---

## Section 2: Impact Analysis

### Epic Impact

| Epic | Impact | Details |
|------|--------|---------|
| **Epic 7 (Integration Tests)** | ⚠️ Affected | Stories 7.1 and 7.8 contain obsolete git2 references that would cause compilation failures and pattern misalignment |
| Epics 1–6 | ✅ No impact | All stories done — git2 references are historical record only |
| Epic 8 | ✅ No impact | All stories done, no git2 references |

### Story Impact

| Story | Status | git2 References | Risk if Uncorrected |
|-------|--------|----------------|---------------------|
| **7.1** — Integration Test Infrastructure & Fixtures | `ready-for-dev` | 3 locations: Task 6.6 description, Dev Notes code sample, Dependencies list | Dev agent will try to use `git2::Repository::init()` for test fixtures — **will not compile** |
| **7.8** — Branch Management & Git Tools Integration Tests | `blocked` (depends on 7.1) | 8 locations: API reference table, API behavior notes, setup pattern, dependencies, imports, intelligence notes, testing standards, project structure notes | Dev agent will receive **inverted** instructions (told to use `&Repository` when API takes `&Path`) — **will not compile**, wrong patterns throughout |

### Artifact Conflicts

| Artifact | Conflict | Resolution |
|----------|----------|------------|
| `epics.md` (planning) | Integration Test Strategy references "real git2 operations" in 2 places | Updated to "real Git CLI operations" |
| `architecture.md` | None — already updated with `revisionNote` on 2026-02-11 | No action needed |
| PRD | None — defines "what" not "how" | No action needed |
| UI/UX | N/A — daemon CLI project | No action needed |

---

## Section 3: Recommended Approach

**Selected:** Option 1 — Direct Adjustment

### Rationale

- The change is purely documentary — no code modifications required
- No stories need to be added, removed, or resequenced
- The affected stories (7.1, 7.8) have zero implementation work done — clean slate
- Effort is low (targeted text edits in 3 files)
- Risk is low (changes are factual corrections aligned with verified source code)

### Alternatives Considered

| Option | Viable | Why Not |
|--------|--------|---------|
| Rollback (Option 2) | ❌ | Nothing to rollback — the git2→CLI migration (4.4) is correct and desired |
| MVP Review (Option 3) | ❌ | MVP scope is unaffected — this is a documentation inconsistency, not a scope issue |

---

## Section 4: Detailed Change Proposals

### 4.1 — Story 7.1: Integration Test Infrastructure & Fixtures

**File:** `_bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md`

**Change 1a — Task 6.6 description:**

| | Content |
|---|---------|
| **OLD** | `create_test_repo(dir)` — initializes git repo with initial commit via `git2` |
| **NEW** | `create_test_repo(dir)` — initializes git repo with initial commit via Git CLI (`git init`, `git commit`) |

**Change 1b — Dev Notes "Git Repo Initialization" section:**

Replaced entire `git2::Repository::init()` code block with Git CLI equivalent using `std::process::Command`. Added note: "Post Story 4.4: git2 has been removed from the project entirely."

**Change 1c — Dependencies Required:**

| | Content |
|---|---------|
| **OLD** | `git2 = "0.20"` — for test repo creation (already a main dependency) |
| **NEW** | *(line removed)* — git2 is no longer in Cargo.toml |

Also updated References section to remove `git2` from Cargo.toml source reference.

### 4.2 — Story 7.8: Branch Management & Git Tools Integration Tests

**File:** `_bmad-output/implementation-artifacts/7-8-branch-management-git-tools-integration-tests.md`

**Change 2a — Quick API Reference table:**

| | `determine_base_branch` Signature |
|---|---------|
| **OLD** | `(story: &StoryInfo, repo: &Repository, default: &str) -> String` at L89 |
| **NEW** | `(story: &StoryInfo, repo_path: &Path, default_branch: &str) -> String` at L104 |

**Change 2b — API Behavior Notes (CRITICAL — inverted instruction):**

| | Content |
|---|---------|
| **OLD** | `determine_base_branch` takes `&Repository`, NOT `&Path`. The test must open the repo via `git2::Repository::open(path)` before calling. |
| **NEW** | `determine_base_branch` takes `&Path`, NOT `&Repository`. Post git2→CLI migration (Story 4.4), all branch functions operate on paths directly. |

**Change 2c — "Git2 Temp Repo Setup Pattern" section:**

Renamed to "Git CLI Temp Repo Setup Pattern". Replaced entire `git2::Repository::init()` code block with Git CLI equivalent. Added Story 4.4 migration note.

**Change 2d — Dependencies Required:**

Removed `git2 = "0.20"` line.

**Change 2e — Required Imports:**

Added `use std::path::Path;` and `use std::process::Command;` (replacing implicit git2 imports).

**Change 2f — Previous Story Intelligence:**

Replaced "real git2 operations on temp repos" → "real Git CLI operations on temp repos" in two locations.

**Change 2g — Testing Standards:**

Replaced "All tests use real git2 on temp repos" → "All tests use real Git CLI on temp repos". Same for the performance note.

**Change 2h — Project Structure Notes (CRITICAL — false directive):**

| | Content |
|---|---------|
| **OLD** | git2 is preferred over CLI git — consistent with all production code in this project |
| **NEW** | Git CLI is used for all git operations — consistent with production code post Story 4.4 migration (git2 removed entirely from project) |

### 4.3 — epics.md: Planning Artifact

**File:** `_bmad-output/planning-artifacts/epics.md`

**Change 3a — Integration Test Strategy "What We're Testing" table:**

| | Branch Management External Deps |
|---|---------|
| **OLD** | Real git2 on temp repos |
| **NEW** | Real Git CLI on temp repos |

**Change 3b — Integration Test Strategy "Mock Strategy" section:**

| | Git repos line |
|---|---------|
| **OLD** | Git repos: real `git2` operations on temp repos (fast, deterministic) |
| **NEW** | Git repos: real Git CLI operations on temp repos (fast, deterministic) |

---

## Section 5: Implementation Handoff

### Scope Classification: Minor

All changes are documentary corrections to story files and planning artifacts. No code changes, no architecture changes, no new stories.

### Changes Applied

All 13 edits across 3 files have been applied directly as part of this workflow:

- ✅ `_bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md` — 3 changes
- ✅ `_bmad-output/implementation-artifacts/7-8-branch-management-git-tools-integration-tests.md` — 8 changes
- ✅ `_bmad-output/planning-artifacts/epics.md` — 2 changes

### What Was NOT Changed (intentional)

- **Completed stories (1.1, 1.5, 3.3, 4.1, 4.3, 4.4):** These contain git2 references in their Dev Notes and Dev Agent Record sections. These are **historical records** of how those stories were originally implemented and later migrated. Modifying them would erase implementation history.
- **Stories 7.2–7.7, 7.9–7.10:** Verified — no git2 references found in these stories.

### Success Criteria

- ✅ No story in `ready-for-dev` or `blocked` status references `git2` as a current dependency or pattern
- ✅ All API signatures in story Dev Notes match actual source code signatures
- ✅ All "preferred pattern" directives align with post-4.4 architecture (Git CLI)

---

## Workflow Completion

- **Issue addressed:** Stale git2 references in Epic 7 stories post migration (Story 4.4)
- **Change scope:** Minor
- **Artifacts modified:** 3 files (7-1 story, 7-8 story, epics.md)
- **Routed to:** Direct implementation (applied in this session)

✅ Correct Course workflow complete, JB!