# Story 1.5: Git Remote Auto-Detection in Init Command

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer setting up BMAD Bot for the first time,
I want the `bmad-bot init` command to auto-detect my git provider, repository owner, repository name, and default branch from the local `.git` configuration,
So that I can complete the setup faster with fewer manual inputs and zero risk of typos on repository information.

## Acceptance Criteria

1. **Given** I am in a directory with an initialized git repository that has an `origin` remote configured **When** I run `bmad-bot init` **Then** the git provider, repo owner, repo name, and target branch are auto-detected from the `origin` remote URL **And** a summary of detected values is displayed for confirmation before proceeding

2. **Given** the auto-detected git settings are displayed **When** I confirm them (default: Yes) **Then** the init command skips the individual git provider/owner/repo/branch prompts and uses the detected values

3. **Given** the auto-detected git settings are displayed **When** I decline them **Then** the init command falls back to the standard manual prompts for git provider, repo owner, repo name, and target branch (existing Story 1.3 behavior)

4. **Given** I am in a directory without a `.git` directory, without any remote configured, or with a remote whose URL is malformed/unparseable **When** I run `bmad-bot init` **Then** the init command silently skips auto-detection and falls back to manual prompts without any error message

5. **Given** the `origin` remote URL uses SSH format (`git@github.com:owner/repo.git`) **When** auto-detection runs **Then** the provider, owner, and repo name are correctly parsed

6. **Given** the `origin` remote URL uses HTTPS format (`https://github.com/owner/repo.git`) **When** auto-detection runs **Then** the provider, owner, and repo name are correctly parsed

7. **Given** the `origin` remote URL points to an unrecognized host (not `github.com` or `gitlab.com`) **When** auto-detection runs **Then** the owner and repo name are still pre-filled **And** the git provider prompt falls back to manual selection with a note that the host was not recognized

8. **Given** the repository has multiple remotes but no `origin` **When** auto-detection runs **Then** the available remote names are listed and the user is prompted to select one

9. **Given** auto-detection successfully identifies git settings **When** the final `bmad-bot.yaml` is generated **Then** the generated config contains the correct git_provider, repo_owner, repo_name, and target_branch values identical to what was confirmed by the user

10. **Given** the repository has exactly one remote that is NOT named `origin` **When** auto-detection runs **Then** that single remote is used automatically for auto-detection (same flow as if it were `origin`)

## Tasks / Subtasks

- [x] Task 0: Implement git remote detection utility (AC: #1, #4, #5, #6, #7, #8, #10)
  - [x] 0.1 Create a `GitRemoteInfo` struct with fields: `provider: Option<String>`, `host: String`, `owner: String`, `repo_name: String`, `default_branch: String`, `remote_name: String` (see Dev Notes for full struct definition with doc comments)
  - [x] 0.2 Create `fn detect_git_remote(project_path: &Path) -> Option<GitRemoteInfo>` that opens the repo via `git2::Repository::discover(project_path)`
  - [x] 0.3 If repo open fails → return `None` (no error, silent fallback)
  - [x] 0.4 Look for `origin` remote first. If not found, collect all remote names.
  - [x] 0.5 If no `origin` and exactly one remote exists → use that single remote automatically (AC #10)
  - [x] 0.6 If no `origin` and multiple remotes exist → return a `GitDetectionResult::MultipleRemotes(Vec<String>)` variant so the caller can prompt the user to choose
  - [x] 0.7 If no remotes at all → return `None`
  - [x] 0.8 Parse the remote URL (handle both SSH and HTTPS formats) to extract host, owner, and repo name. If URL is malformed/unparseable → return `None` (silent fallback, AC #4)
  - [x] 0.9 Map host to provider: `github.com` → `"github"`, `gitlab.com` → `"gitlab"`, anything else → `None`
  - [x] 0.10 Strip `.git` suffix from repo name if present
  - [x] 0.11 Detect default branch: use `repo.head()` to get current branch name as default, fallback to `"main"` if HEAD is detached or unborn

- [x] Task 1: Implement URL parsing for SSH and HTTPS formats (AC: #5, #6)
  - [x] 1.1 Create `fn parse_git_remote_url(url: &str) -> Option<(String, String, String)>` returning `(host, owner, repo_name)`
  - [x] 1.2 Handle SSH format: `git@<host>:<owner>/<repo>.git` — split on `:`, then split path on `/`
  - [x] 1.3 Handle HTTPS format: `https://<host>/<owner>/<repo>.git` — standard URL path parsing
  - [x] 1.4 Handle SSH with `ssh://` prefix: `ssh://git@<host>/<owner>/<repo>.git`
  - [x] 1.5 Handle edge cases: trailing slashes, missing `.git` suffix, port numbers in URL

- [x] Task 2: Integrate auto-detection into `collect_config_interactively()` (AC: #1, #2, #3, #4, #8, #10)
  - [x] 2.1 At the start of the "Git Provider" section, call `detect_git_remote(".")`
  - [x] 2.2 If `GitDetectionResult::MultipleRemotes` → prompt user to select a remote via `dialoguer::Select`, then re-detect with the chosen remote
  - [x] 2.3 If detection succeeds → display a summary block:
        ```
        🔍 Git repository detected!
           Provider:  github
           Owner:     jean-baptiste-music
           Repo:      bmad-bot
           Branch:    main

        ✔ Use these settings? (Y/n)
        ```
  - [x] 2.4 If provider is `None` (unrecognized host) → display partial summary (owner + repo + branch) with a note: `"⚠ Git host not recognized — provider must be selected manually"`
  - [x] 2.5 If user confirms → use detected values, skip manual git prompts entirely
  - [x] 2.6 If user declines → fall through to existing manual prompts (unchanged Story 1.3 behavior)
  - [x] 2.7 If detection returns `None` → skip auto-detection silently, proceed with manual prompts

- [x] Task 3: Write unit tests (AC: #1–#10)
  - [x] 3.1 Test `parse_git_remote_url` with SSH format `git@github.com:owner/repo.git` → `("github.com", "owner", "repo")`
  - [x] 3.2 Test `parse_git_remote_url` with HTTPS format `https://github.com/owner/repo.git` → `("github.com", "owner", "repo")`
  - [x] 3.3 Test `parse_git_remote_url` with SSH `ssh://` prefix format → correct parsing
  - [x] 3.4 Test `parse_git_remote_url` with GitLab URL → `("gitlab.com", "owner", "repo")`
  - [x] 3.5 Test `parse_git_remote_url` with self-hosted URL → `("git.company.com", "owner", "repo")`
  - [x] 3.6 Test `parse_git_remote_url` strips `.git` suffix correctly
  - [x] 3.7 Test `parse_git_remote_url` works without `.git` suffix
  - [x] 3.8 Test `parse_git_remote_url` with trailing slash
  - [x] 3.9 Test `parse_git_remote_url` with port number in SSH URL
  - [x] 3.10 Test `parse_git_remote_url` returns `None` for malformed URLs
  - [x] 3.11 Test provider mapping: `github.com` → `Some("github")`, `gitlab.com` → `Some("gitlab")`, `other.com` → `None`
  - [x] 3.12 Test `detect_git_remote` returns `None` when no `.git` directory exists (using a temp dir)
  - [x] 3.13 Test `detect_git_remote` with a real git2 repo initialized in a temp dir with a configured remote → returns correct `GitRemoteInfo`
  - [x] 3.14 Test `detect_git_remote` with a single remote named `upstream` (not `origin`) → auto-detects from that remote without prompting (AC #10)
  - [x] 3.15 Test `detect_git_remote` returns `NotAvailable` when remote URL is malformed/unparseable (AC #4)

- [x] Task 4: Final quality checks
  - [x] 4.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [x] 4.2 Run `cargo clippy` and fix any warnings
  - [x] 4.3 Run `cargo test` and verify all tests pass (including all Story 1.1–1.4 tests — no regressions)
  - [x] 4.4 Verify all new public items have `///` doc comments
  - [ ] 4.5 Manual integration test: run `cargo run -- init` in a git repo with an origin remote, verify auto-detection displays correct values and confirmation works
  - [ ] 4.6 Manual integration test: run `cargo run -- init` in a directory without git, verify fallback to manual prompts with no errors

## Dev Agent Record

### Implementation Plan

- Created new module `src/cli/git_detect.rs` for all git remote detection logic, keeping it cleanly separated from `cli/mod.rs`
- Implemented `GitRemoteInfo` struct and `GitDetectionResult` enum exactly as specified in Dev Notes
- `parse_git_remote_url()` handles SSH SCP-like, HTTPS, HTTP, and `ssh://` scheme formats including port numbers, trailing slashes, and missing `.git` suffixes
- `detect_git_remote()` implements the full discovery chain: origin → single-remote → multiple-remotes → not-available
- `detect_git_remote_with_name()` added for re-detection after user selects a remote from the multi-remote prompt
- `map_host_to_provider()` maps github.com/gitlab.com to known providers, case-insensitive
- `attempt_git_auto_detection()` helper in `cli/mod.rs` orchestrates the full UX flow: detection → summary display → confirmation → fallback
- Integrated into `collect_config_interactively()` at the start of the "Git Provider" section, preserving all existing manual prompts as fallback
- 26 unit tests written inline in `git_detect.rs` covering all Task 3 subtasks (3.1–3.15)

### Completion Notes

- All 599 tests pass (26 new + 573 existing, zero regressions)
- `cargo fmt -- --check` clean
- `cargo clippy` zero new warnings (15 pre-existing from other stories' scaffolding)
- All public items have `///` doc comments
- No new dependencies added (`git2` already in Cargo.toml from Story 1.1)
- Subtasks 4.5 and 4.6 are manual integration tests requiring interactive terminal — left for human verification

## File List

- `src/cli/git_detect.rs` — NEW: Git remote auto-detection module (structs, URL parsing, detection, 26 tests)
- `src/cli/mod.rs` — MODIFIED: Added `git_detect` submodule, `attempt_git_auto_detection()` helper, integrated auto-detection into `collect_config_interactively()`
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — MODIFIED: Story status updated to in-progress
- `_bmad-output/implementation-artifacts/1-5-git-remote-auto-detection-init.md` — MODIFIED: Tasks marked complete, Dev Agent Record added

## Change Log

- 2026-02-09: Implemented git remote auto-detection for `bmad-bot init` command — URL parsing (SSH/HTTPS/ssh-scheme), remote discovery (origin/single/multi), provider mapping, interactive confirmation UX, 26 unit tests, full integration into init wizard

## Dev Notes

### Previous Story Intelligence

This story modifies the `cli/mod.rs` file created in Story 1.3. The key function to modify is `collect_config_interactively()` — the auto-detection logic should be inserted at the beginning of the "Git Provider" section (before the existing `dialoguer::Select` for git provider).

**Key existing structures from Story 1.1 / 1.3:**
- `GitProviderConfig { provider, repo_owner, repo_name, target_branch }` — the struct we're populating
- `GIT_PROVIDERS: &[&str] = &["github", "gitlab"]` — used for manual selection fallback
- `CliError` enum — already has `Init { reason: String }` variant for init-specific errors

**Dependency:** `git2` is already in `Cargo.toml` (added in Story 1.1). No new dependencies needed.

### URL Parsing Reference

Common git remote URL formats to handle:

| Format | Example |
|--------|---------|
| SSH (SCP-like) | `git@github.com:owner/repo.git` |
| HTTPS | `https://github.com/owner/repo.git` |
| SSH with scheme | `ssh://git@github.com/owner/repo.git` |
| SSH with port | `ssh://git@github.com:22/owner/repo.git` |
| HTTPS without .git | `https://github.com/owner/repo` |

### Detection Result Enum

```rust
/// Result of attempting to auto-detect git remote information.
pub enum GitDetectionResult {
    /// Successfully detected remote info from a single remote.
    Detected(GitRemoteInfo),
    /// Multiple remotes found but no `origin` — user must choose.
    MultipleRemotes(Vec<String>),
    /// No git repo or no remotes found — silent fallback.
    NotAvailable,
}

/// Git remote information extracted from the local repository.
pub struct GitRemoteInfo {
    /// Git hosting provider if recognized ("github" or "gitlab"), None for unknown hosts.
    pub provider: Option<String>,
    /// The hostname extracted from the remote URL (e.g., "github.com").
    pub host: String,
    /// Repository owner (organization or user).
    pub owner: String,
    /// Repository name (without .git suffix).
    pub repo_name: String,
    /// Default branch name detected from HEAD.
    pub default_branch: String,
    /// Name of the remote used for detection (typically "origin").
    pub remote_name: String,
}
```

### Integration Point in collect_config_interactively()

The auto-detection block should be inserted right after `println!("── Git Provider ──");` and before the existing `dialoguer::Select` for git provider. The flow is:

1. Attempt `detect_git_remote(".")`
2. If `Detected` with known provider → show summary, ask confirm → if yes, build `GitProviderConfig` directly and skip to LLM section
3. If `Detected` with unknown provider → show partial summary (owner/repo/branch pre-filled), ask manual provider select, then ask confirm
4. If `MultipleRemotes` → prompt user to select remote, re-detect, then follow Detected flow
5. If `NotAvailable` → fall through to existing manual prompts unchanged

### Anti-Patterns to Avoid

- **Do NOT crash or error on detection failure** — this is a convenience feature, always fallback gracefully
- **Do NOT modify any git state** — read-only operations only (open repo, read remotes, read HEAD)
- **Do NOT add new dependencies** — `git2` is already available
- **Do NOT change the generated `bmad-bot.yaml` format** — the output is identical regardless of whether values were auto-detected or manually entered
- **Do NOT remove or modify existing manual prompt code** — it serves as the fallback path and must remain functional

### Scope Boundaries

**IN scope:**
- Auto-detection of git provider, owner, repo name, and default branch from local `.git`
- Confirmation UX with summary display
- Fallback to manual prompts on decline or detection failure
- Multi-remote selection when no `origin` exists
- Handling of unrecognized hosts (self-hosted Git)
- Unit tests for URL parsing and detection logic

**OUT of scope:**
- Self-hosted GitLab/GitHub Enterprise provider support (auto-detect fills owner/repo but provider is manual)
- Detecting git credentials or tokens
- Any write operations to git
- Changes to other CLI commands (start, status, logs)

### References

- Story 1.1 (`1-1-project-scaffolding-configuration-validation.md`) — BotConfig struct definitions, git2 dependency
- Story 1.3 (`1-3-interactive-init-command.md`) — `collect_config_interactively()` implementation, `generate_config_yaml()`, `CliError` enum
- `git2` crate docs — `Repository::discover()`, `Repository::remotes()`, `Remote::url()`
