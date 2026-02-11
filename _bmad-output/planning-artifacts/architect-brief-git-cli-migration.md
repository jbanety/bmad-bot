---
type: architect-brief
from: Amelia (Dev Agent)
to: Product Owner
date: '2026-02-11'
subject: 'Architecture Change Request — Migrate git2 to Git CLI'
related_decision: 'Foundation Layer — git2 (libgit2) dependency'
status: ready-for-po
triggered_by: 'Production bug — daemon push authentication failure (SSH agent not available in background process)'
---

# Architect Brief: Migrate from git2 (libgit2) to Git CLI

## Context

The BMAD Bot daemon uses `git2` (Rust bindings for libgit2) for all git operations — both in the agent's `GitTool` (exposed to the LLM) and in daemon internals (branch management, push). This was chosen to avoid an external binary dependency.

**Production incident on 2026-02-10** revealed a fundamental flaw: the daemon runs as a background process and **does not have access to the user's SSH agent**. The `git2` push operation failed with:

```
remote rejected authentication: Failed getting response; class=Ssh (23); code=Auth (-16)
```

A hotfix was applied (commit `eaafa28`) to bypass the SSH remote entirely — constructing an HTTPS URL and using the GitHub token for push auth. While functional, this means the daemon now has **two separate authentication paths**: git2 credentials for agent operations (SSH-based, potentially broken) and HTTPS token auth for pipeline push. This is fragile and inconsistent.

Beyond auth, git2 also **ignores the user's git configuration**: commit signing (GPG/SSH), credential managers (osxkeychain, credential-helper), custom hooks, and editor/diff settings.

## Problem Summary

| Issue | Impact |
|-------|--------|
| SSH agent not available in daemon context | Push fails without HTTPS workaround |
| git2 ignores user's credential manager | Auth strategy varies per operation |
| git2 cannot sign commits | Bot commits appear as "Unverified" on GitHub |
| git2 ignores `.gitconfig` settings | User's git identity, aliases, hooks not respected |
| Two auth paths (git2 SSH vs HTTPS token) | Maintenance burden, inconsistent behavior |
| libgit2 is a heavy C dependency | Increases compile time, binary size, and attack surface |

## Proposed Change

**Replace all `git2` usage with `git` CLI calls** across the entire codebase.

The daemon and agent tools would invoke `git` as a subprocess (`tokio::process::Command` for async, `std::process::Command` for sync contexts) and parse stdout/stderr. This inherits the user's full git configuration automatically.

**New prerequisite:** `git` must be installed on the host system. This is acceptable — the daemon already requires a git repository, and any developer machine will have git installed.

## Scope of Change

Three components use git2 today:

### 1. `GitTool` — Agent-facing rig tool (`src/tools/git.rs`)

The LLM agent's primary git interface. 9 actions: `clone`, `checkout`, `branch_create`, `add`, `commit`, `push`, `diff`, `status`, `log`.

**Migration:** Each action becomes a `tokio::process::Command::new("git")` call with appropriate arguments. Output is captured and returned as-is (git CLI output is already human/LLM-readable). Error handling maps non-zero exit codes to `GitToolError`.

| git2 call | CLI equivalent |
|-----------|---------------|
| `Repository::clone()` | `git clone <url> <path>` |
| `Repository::checkout_tree()` + `set_head()` | `git checkout <branch>` |
| `Branch::new()` from ref | `git checkout -b <branch> [<from>]` |
| `Index::add_path()` + `write()` | `git add <paths...>` |
| `repo.commit()` | `git commit -m <message>` |
| `remote.push()` | `git push <remote> <branch>` |
| `repo.diff_index_to_workdir()` | `git diff [--cached]` |
| `repo.statuses()` | `git status --porcelain` |
| `revwalk` + `find_commit()` | `git log --oneline -<n>` |

### 2. Branch Management — Daemon internal (`src/session/branch.rs`)

Used by the daemon to create/checkout story branches before launching the agent session.

**Functions affected:**
- `determine_base_branch()` — checks if a branch exists (`git branch --list`)
- `ensure_story_branch()` — creates or reuses story branches (`git checkout -b` / `git checkout`)
- `checkout_branch()` — switches HEAD (`git checkout`)

### 3. Pipeline Push — Daemon internal (`src/pipeline.rs`)

The recently-added `push_branch()` method. Currently uses a hybrid HTTPS workaround.

**Migration:** Simplifies to `git push origin <branch>` — inherits whatever auth the user has configured (SSH agent if interactive, credential helper, osxkeychain, etc.).

## Benefits

| Benefit | Detail |
|---------|--------|
| **Auth just works** | Inherits user's credential manager, SSH agent (when available), osxkeychain, git-credential-helper |
| **Commit signing** | `git commit -S` uses user's GPG/SSH signing config automatically |
| **Single auth path** | No more git2 SSH vs HTTPS token split |
| **User's git identity** | Respects `.gitconfig` — name, email, signing key |
| **Simpler code** | CLI calls are ~5 lines each vs 20-40 lines of git2 boilerplate |
| **Smaller binary** | Removes `git2` crate (links libgit2 + libssh2 + OpenSSL) — significant compile-time and binary-size reduction |
| **Better error messages** | Git CLI errors are already human-readable — no need to wrap/translate libgit2 error codes |

## Trade-offs

| Trade-off | Mitigation |
|-----------|------------|
| Requires `git` installed on host | Acceptable — every dev machine has git. Add startup validation check |
| CLI output parsing is fragile | Use `--porcelain` flags where available. Most operations only need exit code + stdout |
| Subprocess overhead per operation | Negligible — git operations are infrequent (seconds between calls) and each is I/O-bound anyway |
| Loss of in-process git object access | Not needed — the agent and daemon only perform standard git workflow operations |

## New Prerequisite Validation

The daemon's `start` command should verify `git` is available and meets a minimum version:

```
git --version  →  parse major.minor  →  require >= 2.30
```

Fail fast with a clear error message if git is missing or too old. This check belongs in `cli/mod.rs::run_start()` alongside the existing config validation.

## Dependency Changes

### Remove from `Cargo.toml`
- `git2 = "0.20"` — entire crate and its transitive C dependencies (libgit2, libssh2)

### No new dependencies needed
- `tokio::process::Command` is already available (tokio `full` feature)
- `std::process::Command` is stdlib

## Suggested Story Breakdown

A single epic is unnecessary — this is a focused refactoring that can be a **standalone story** or a **2-3 story mini-epic** depending on PO preference.

### Option A: Single Story (recommended if team wants fast delivery)

**Story: Migrate all git operations from git2 to Git CLI**

Tasks:
1. Add git version validation to daemon startup
2. Rewrite `GitTool` (9 actions) to use `tokio::process::Command`
3. Rewrite `session/branch.rs` (3 functions) to use `std::process::Command`
4. Rewrite `pipeline.rs::push_branch()` to use `tokio::process::Command`
5. Remove `git2` from `Cargo.toml`
6. Update all unit tests (mock CLI output instead of creating git2 repos)
7. Update `project-context.md` and `architecture.md` references

### Option B: 3 Stories (if PO prefers incremental delivery)

**Story 1: GitTool CLI migration** — Rewrite the agent-facing tool. Highest value — fixes agent auth issues.

**Story 2: Daemon git internals migration** — Rewrite `branch.rs` + `pipeline.rs::push_branch()`. Fixes daemon push auth.

**Story 3: git2 removal + validation** — Remove the crate, add startup git validation, update docs.

## Architecture Document Impact

- **Technical Constraints:** Remove "git2 (libgit2): Embedded, no external git CLI dependency" → Replace with "Git CLI: Requires git >= 2.30 installed on host. All git operations via subprocess."
- **Decision 7 (Surgical Tooling):** `git.rs` description changes from "via git2" to "via git CLI"
- **Project Structure:** `tools/git.rs` description update
- **External Integration Points:** Add "Git CLI (>= 2.30)" as system dependency
- **project-context.md:** Update "Git Operations: git2 (embedded libgit2, no CLI dependency)" line

## Reference

- **Hotfix commit (push):** `62929b2` — Added `push_branch()` with git2 (initial fix)
- **Hotfix commit (HTTPS auth):** `eaafa28` — Switched to HTTPS token auth workaround
- **Affected files:** `src/tools/git.rs`, `src/session/branch.rs`, `src/pipeline.rs`
- **Current git2 usage:** ~600 lines across 3 files
- **Estimated post-migration:** ~250 lines (CLI calls are significantly shorter)