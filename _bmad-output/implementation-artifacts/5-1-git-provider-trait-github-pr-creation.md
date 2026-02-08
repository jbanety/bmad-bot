# Story 5.1: Git Provider Trait & GitHub PR Creation

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer using GitHub,
I want the daemon to create Pull Requests with comprehensive descriptions after each story session,
so that I wake up to reviewable PRs with full context on what was done and why.

## Acceptance Criteria

1. **Given** the `git_provider` module is initialized
   **When** the `GitProvider` trait is defined
   **Then** it exposes async methods: `create_pr(params: CreatePrParams) -> Result<PrInfo, GitProviderError>`, `add_comment(pr_id: &str, body: &str) -> Result<(), GitProviderError>`, `get_pr_url(pr_id: &str) -> Result<String, GitProviderError>`
   **And** `CreatePrParams`, `PrInfo`, and `GitProviderError` are dedicated structs/enums following the git provider trait pattern
   **And** the provider is selected via `bmad-bot.yaml` config (`git_provider: github | gitlab`)

2. **Given** a development session has completed successfully
   **When** the daemon creates a PR via the GitHub implementation (octocrab)
   **Then** the PR is created with: agent-written title and description body, source branch (`story/{epic}-{story}`), target branch (configured base branch)
   **And** the PR description includes a dedicated "🤖 Supervisor Decisions" section listing all decisions from the session (question, decision, reasoning, alternatives)

3. **Given** a development session has been blocked or failed
   **When** the daemon creates a PR for the failed story
   **Then** a PR is still created with partial code committed to the branch
   **And** the PR description includes a clear failure/blockage description explaining what happened, where it stopped, and all decisions made before the failure

4. **Given** code review is disabled in configuration
   **When** the development session completes
   **Then** the daemon proceeds directly to PR creation without launching a review session
   _(Note: The `review.enabled` config flag does not exist yet — it will be added by Story 5.2. AC4 validates the provider layer works independently of the review module. The orchestration layer that checks this flag is out of scope for this story.)_

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [x] Verify Epic 4 stories (4-1, 4-2, 4-3) code and types are present and compilable
- [x] Verify `SessionOutcome` enum exists in `src/session/mod.rs` with `Completed`, `Escalated`, `Failed` variants containing `decisions: Vec<DecisionRecord>`
- [x] Verify `EscalationReport` struct exists in `src/session/escalation.rs` with fields: `story_key`, `question`, `reason`, `branch_name`, `partial_work_summary`, `escalated_at`
- [x] Verify `format_pr_decisions_section()` exists in `src/supervisor/decisions.rs`
- [x] Verify `BotSecrets.github_token` field exists in `src/config/mod.rs`
- [x] Verify `GitProviderConfig` struct exists with `provider`, `repo_owner`, `repo_name`, `target_branch` fields
- [x] Verify `octocrab = "0.49"` and `async-trait = "0.1"` are in `Cargo.toml`
- [x] Confirm existing skeleton files: `src/git_provider/mod.rs`, `src/git_provider/github.rs`, `src/git_provider/gitlab.rs`

### Task 1: Define `GitProvider` Trait & Shared Types (`src/git_provider/mod.rs`)

- [x] Define `GitProviderError` enum with `thiserror`:
  - `ApiError { status: u16, message: String }` — HTTP-level failures after retries exhausted
  - `AuthenticationFailed { reason: String }` — token missing/invalid (401/403)
  - `BranchNotFound { branch: String }` — source branch doesn't exist on remote (often 422 from GitHub — likely means branch was never pushed)
  - `RateLimited { retry_after_secs: Option<u64> }` — rate limit exceeded (429)
  - `NetworkError { reason: String }` — connection/DNS failures
  - `InvalidResponse { reason: String }` — unexpected response format
  - `InvalidPrId { pr_id: String }` — `pr_id` string could not be parsed as `u64` (for `add_comment`/`get_pr_url`)
  - `ProviderNotConfigured { provider: String }` — factory called with unsupported provider
  - `BuildError { reason: String }` — octocrab client construction failed
- [x] Define `CreatePrParams` struct:
  - `title: String`
  - `body: String`
  - `source_branch: String`
  - `target_branch: String`
- [x] Define `PrInfo` struct:
  - `id: String`
  - `url: String`
  - `number: u64`
- [x] Define `#[async_trait] pub trait GitProvider: Send + Sync`:
  - `async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError>`
  - `async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError>`
  - `async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError>`
- [x] Define factory function: `pub fn create_provider(config: &GitProviderConfig, token: &str) -> Result<Box<dyn GitProvider>, GitProviderError>`
  - Match on `config.provider`:
    - `"github"` → `GitHubProvider::new(config, token).map(|p| Box::new(p) as Box<dyn GitProvider>)`
    - `"gitlab"` → `Err(GitProviderError::ProviderNotConfigured { provider: "gitlab (not yet implemented)".into() })`
    - other → `Err(GitProviderError::ProviderNotConfigured { provider: other.into() })`
- [x] Re-export `GitHubProvider` from `github` submodule
- [x] Add `///` doc comments on all public items

### Task 2: Implement `GitHubProvider` (`src/git_provider/github.rs`)

- [x] Define `GitHubProvider` struct:
  - `octocrab: Octocrab` — authenticated octocrab instance
  - `owner: String` — repo owner from config
  - `repo: String` — repo name from config
- [x] Implement `GitHubProvider::new(config: &GitProviderConfig, token: &str) -> Result<Self, GitProviderError>`:
  - Build `Octocrab::builder().personal_token(token.to_string()).build()`
  - Map `octocrab::Error` → `GitProviderError::BuildError { reason: e.to_string() }`
  - Store `owner` and `repo` from config
  - ⚠️ **MUST return `Result<Self, GitProviderError>`** — `Octocrab::builder().build()` is fallible
- [x] Implement `#[async_trait] GitProvider for GitHubProvider`:
  - **`create_pr`**:
    - Use `self.octocrab.pulls(&self.owner, &self.repo).create(params.title, params.source_branch, params.target_branch).body(params.body).send().await`
    - Map `octocrab::Error` → `GitProviderError` using `map_octocrab_error()` helper (see Error Matching section below)
    - Extract URL: `pr.html_url.map(|u| u.to_string()).unwrap_or_else(|| format!("https://github.com/{}/{}/pull/{}", self.owner, self.repo, pr.number))`
    - Return `PrInfo { id: pr.number.to_string(), url, number: pr.number }`
    - Log via `tracing::info!(action = "pr_created", pr_number = %pr.number, url = %url, "Pull request created")`
  - **`add_comment`**:
    - Parse `pr_id` → `u64` via `pr_id.parse::<u64>().map_err(|_| GitProviderError::InvalidPrId { pr_id: pr_id.to_string() })`
    - Use `self.octocrab.issues(&self.owner, &self.repo).create_comment(pr_number, body).await`
    - Map errors via `map_octocrab_error()`
    - Log via `tracing::info!(action = "pr_comment_added", pr_id = %pr_id, "Comment added to PR")`
  - **`get_pr_url`**:
    - Parse `pr_id` → `u64` (same pattern as `add_comment`)
    - Construct URL deterministically: `format!("https://github.com/{}/{}/pull/{}", self.owner, self.repo, pr_number)` — no API call needed
    - Return the URL string
- [x] Implement private helper `fn map_octocrab_error(e: octocrab::Error) -> GitProviderError` (see Error Matching section below)
- [x] Add `///` doc comments on all public items

### Task 3: Implement PR Description Builder (helper in `src/git_provider/mod.rs`)

- [x] Define `pub struct PrDescriptionParams`:
  - `story_key: String`
  - `story_title: String`
  - `outcome_summary: String` — "completed successfully" / "failed" / "escalated — needs clarification"
  - `decisions_section: String` — output of `format_pr_decisions_section()`
  - `failure_details: Option<String>` — only for failed/escalated stories
- [x] Implement `pub fn build_pr_description(params: &PrDescriptionParams) -> String`:
  - Build markdown body with sections:
    - `## 📋 Story: {story_key} — {story_title}`
    - `**Status:** {outcome_summary}`
    - If `failure_details.is_some()` → `## ⚠️ Failure Details\n{details}`
    - Append `decisions_section` (already formatted by `format_pr_decisions_section`)
    - Footer: `---\n*Generated by bmad-bot*`
- [x] Implement `pub fn build_pr_title(story_key: &str, story_title: &str, is_failure: bool) -> String`:
  - Success: `"feat({story_key}): {story_title}"`
  - Failure: `"wip({story_key}): {story_title} [NEEDS REVIEW]"`

### Task 4: Unit Tests

- [x] Tests in `src/git_provider/mod.rs` `#[cfg(test)] mod tests`:
  - `test_create_provider_github_returns_ok` — factory with `provider: "github"` and valid token succeeds
  - `test_create_provider_gitlab_returns_not_configured` — factory with `provider: "gitlab"` returns `ProviderNotConfigured`
  - `test_create_provider_unknown_returns_not_configured` — factory with `provider: "bitbucket"` returns error
  - `test_git_provider_error_display_variants` — all error variants produce readable messages
  - `test_git_provider_error_display_invalid_pr_id` — `InvalidPrId` variant includes the bad ID in message
  - `test_git_provider_error_display_build_error` — `BuildError` variant includes reason
  - `test_pr_description_success_no_failure_details` — verify markdown output structure
  - `test_pr_description_failure_includes_details` — verify failure section present
  - `test_pr_description_escalation_includes_details` — verify escalation context
  - `test_pr_description_includes_decisions_section` — verify decisions section appended
  - `test_pr_description_includes_footer` — verify bmad-bot footer
  - `test_build_pr_title_success` — verify `feat(...)` format
  - `test_build_pr_title_failure` — verify `wip(...)` format
  - `test_create_pr_params_fields` — verify struct construction
  - `test_pr_info_fields` — verify struct construction
- [x] Tests in `src/git_provider/github.rs` `#[cfg(test)] mod tests`:
  - `test_github_provider_new_success` — verify constructor returns `Ok` with valid token
  - `test_github_provider_new_stores_owner_repo` — verify struct fields after construction
  - `test_github_provider_is_send_sync` — compile-time trait check: `fn assert_send_sync<T: Send + Sync>() {}; assert_send_sync::<GitHubProvider>();`
  - `test_map_octocrab_error_handles_variants` — verify error mapping helper covers key cases (Other variant; GitHub-specific status mapping via `#[non_exhaustive]` GitHubError is E2E-tested)
  - NOTE: No live API tests here — octocrab calls are tested in E2E only

### Task 5: Integration Verification

- [x] `cargo check` — zero new errors
- [x] `cargo test` — all 454 tests pass (21 new + 433 existing), no regressions
- [x] `cargo clippy` — zero new warnings (pre-existing dead_code warnings from unconnected modules)
- [x] `cargo fmt` — all clean
- [x] Verify `#![deny(clippy::all)]` is respected (no new warnings)
- [x] Verify all public items have `///` doc comments

## Dev Notes

### Previous Story Intelligence

**Story 4.3** (latest completed):
- Test count: **435 tests** (421 existing + 14 new). 82 pre-existing `dead_code` warnings from unconnected modules — expected.
- `StoryInfo.branch_name` is pre-computed as `format!("story/{key}")` — Single Source of Truth for branch naming. This is the value to use for `CreatePrParams.source_branch`.

**Story 4.2** (Session Runner):
- `SessionRunner::run(&self, story: &StoryInfo) -> SessionOutcome` — this is the upstream trigger for PR creation.
- `SessionOutcome` carries `decisions: Vec<DecisionRecord>` in all three variants.

**Story 3.4** (Decision Logging):
- `format_pr_decisions_section(decisions: &[DecisionRecord]) -> String` — **already exists in `src/supervisor/decisions.rs`**, returns formatted markdown table. Call this to populate `PrDescriptionParams.decisions_section`.

### SessionOutcome → PR Params Mapping

The orchestration layer (future scope) will use this mapping. The provider layer must support all three paths:

| `SessionOutcome` variant | `story_key` source | `source_branch` source | `is_failure` | `failure_details` |
|---|---|---|---|---|
| `Completed { story_key, branch, decisions }` | `story_key` | `branch` | `false` | `None` |
| `Escalated { report, decisions }` | `report.story_key` | `report.branch_name` | `true` | `Some(format!("**Question:** {}\n**Reason:** {}\n**Partial work:** {}", report.question, report.reason, report.partial_work_summary))` |
| `Failed { story_key, error, decisions }` | `story_key` | derive from story_key: `format!("story/{story_key}")` | `true` | `Some(error.clone())` |

**`EscalationReport` fields** (from `src/session/escalation.rs`):
- `story_key: String` — story being developed
- `question: String` — unanswered supervisor question
- `reason: String` — why escalation was necessary
- `branch_name: String` — git branch with partial work
- `partial_work_summary: String` — what was preserved
- `escalated_at: String` — ISO 8601 timestamp

### Core Design — GitProvider Trait + GitHub Implementation

The architecture mandates the **Git Provider Trait Pattern**:
- Async trait methods (`#[async_trait]`)
- Dedicated param/return structs (never loose primitives)
- Per-module `thiserror` error enum
- Implementations in separate files (github.rs, gitlab.rs)

This story implements the **provider layer only**. The orchestration (calling the provider after session completion) happens in the watcher/main loop which is NOT in scope. The integration point: `create_provider()` returns a `Box<dyn GitProvider>` ready to be called.

### octocrab API Usage (v0.49)

**Creating a PR:**
```rust
let pr = octocrab
    .pulls("owner", "repo")
    .create("title", "source_branch", "target_branch")
    .body("description body")
    .send()
    .await?;
// pr.number: u64
// pr.html_url: Option<Url> — use .map(|u| u.to_string()) with fallback
```

**Adding a comment (PRs use the issues API in GitHub):**
```rust
let comment = octocrab
    .issues("owner", "repo")
    .create_comment(pr_number_u64, "comment body")
    .await?;
```

**Authentication:**
```rust
let octocrab = Octocrab::builder()
    .personal_token(token.to_string())
    .build()?;  // Returns Result<Octocrab, octocrab::Error>
```
Token comes from `BotSecrets.github_token`, loaded from `.env` via `GITHUB_TOKEN` env var.

### octocrab Error Matching (v0.49) — CRITICAL

`octocrab::Error` is an enum. The key variants for error mapping:

```rust
fn map_octocrab_error(e: octocrab::Error) -> GitProviderError {
    match e {
        octocrab::Error::GitHub { source, .. } => {
            // source is octocrab::models::GitHubError with:
            //   source.message: String — error description
            //   source.status_code: StatusCode — HTTP status
            let status = source.status_code.as_u16();
            match status {
                401 | 403 => GitProviderError::AuthenticationFailed {
                    reason: source.message.clone(),
                },
                404 => GitProviderError::ApiError {
                    status,
                    message: source.message.clone(),
                },
                422 => GitProviderError::BranchNotFound {
                    branch: source.message.clone(),  // Often "No commits between X and Y"
                },
                429 => GitProviderError::RateLimited { retry_after_secs: None },
                _ => GitProviderError::ApiError {
                    status,
                    message: source.message.clone(),
                },
            }
        }
        // Network/connection errors
        other => GitProviderError::NetworkError {
            reason: other.to_string(),
        },
    }
}
```

⚠️ **Verify `octocrab::Error` variant names and `GitHubError` field names** against the actual v0.49 source at compile time. The above is based on octocrab docs — field names may differ slightly (e.g., `status_code` vs `status`). Use `cargo doc --open` on octocrab to confirm exact types. If the internal structure differs, adapt the match arms accordingly but preserve the same mapping semantics.

### Push Assumption

`create_pr` will fail with HTTP 422 ("No commits between base and head") if the source branch was never pushed to the remote. The dev agent pushes via the `GitTool` (from Story 4.1) during its session. The `BranchNotFound` error variant should include a helpful message: the 422 from GitHub on PR creation almost always means the branch doesn't exist on the remote yet. Log this clearly so debugging is straightforward.

### Error Mapping Strategy

Three-tier error propagation (architecture Decision 4):
- **Layer 1 (HTTP transport):** octocrab manages its own HTTP client. No retry logic in this story — that's FR33 in Epic 6 (Story 6.2).
- **Layer 2 (Git Provider):** Map `octocrab::Error` → `GitProviderError` with descriptive variants. All errors logged via `tracing::error!()`.
- **Layer 3 (Session/Daemon):** Caller handles `GitProviderError` — if PR creation fails, it should still notify the human (Epic 6 scope).

### Files Created/Modified in This Story

| File | Change |
|------|--------|
| `src/git_provider/mod.rs` | **OVERWRITE** — Replace TODO skeleton with trait, types, factory, description builder, unit tests |
| `src/git_provider/github.rs` | **OVERWRITE** — Replace TODO skeleton with `GitHubProvider` implementation + unit tests |
| `src/git_provider/gitlab.rs` | **UNCHANGED** — Leave as-is (TODO stub for Story 5.3) |

### Anti-Patterns to Avoid

- ❌ **No `unwrap()` or `expect()` in production code** — only in tests
- ❌ **No `anyhow::Result`** in library modules — use `GitProviderError` exclusively
- ❌ **No `println!` or `eprintln!`** — use `tracing` only
- ❌ **No real API calls in unit tests** — mock everything, real calls only in E2E
- ❌ **No loose primitives as function params** — use dedicated structs
- ❌ **No logging of API tokens** — never log the GitHub token value
- ❌ **Do NOT implement retry logic** — that's Epic 6 (Story 6.2) scope
- ❌ **Do NOT implement GitLab** — that's Story 5.3 scope. Return `ProviderNotConfigured` from factory
- ❌ **Do NOT modify `src/session/runner.rs`** to call the provider — orchestration integration is future scope
- ❌ **Do NOT parse `pr_id` with `unwrap()`** — use `InvalidPrId` error variant for parse failures

### Scope Boundaries

**IN SCOPE:**
- `GitProvider` trait definition with all three methods
- `GitHubProvider` implementation using octocrab (returns `Result` from constructor)
- `GitProviderError` thiserror enum (9 variants including `InvalidPrId`, `BuildError`)
- `CreatePrParams`, `PrInfo` structs
- `PrDescriptionParams` and `build_pr_description()` helper
- `build_pr_title()` helper
- `create_provider()` factory function
- `map_octocrab_error()` private helper
- Unit tests for all the above

**OUT OF SCOPE:**
- GitLab implementation (Story 5.3)
- Code review session and `review.enabled` config flag (Story 5.2)
- Retry/resilience for HTTP calls (Story 6.2)
- Calling the provider from the watcher/session loop (future integration)
- Notifications about PR creation (Story 6.1)

### Testing Requirements

All tests follow Arrange → Act → Assert pattern. Test naming: `test_{module}_{behavior}_{scenario}`.

**Unit tests (no network, no API calls):**
- Factory function routing (github/gitlab/unknown)
- Constructor success and field verification
- Error enum Display implementations (all 9 variants)
- PR description builder output structure (success, failure, escalation)
- PR title builder (success vs failure format)
- Struct field construction
- Send + Sync trait bounds compile check
- `map_octocrab_error` helper (if testable without live errors — otherwise document as E2E only)

**E2E tests (future, gated behind `BMAD_E2E=1`):**
- Real GitHub API PR creation → not in this story

### Dev Dependencies Required

No new dependencies needed. All already present in `Cargo.toml`:
- `octocrab = "0.49"` — GitHub API client
- `async-trait = "0.1"` — async trait methods
- `thiserror = "2"` — error enum derive
- `tracing = "0.1"` — structured logging
- `tempfile = "3"` (dev-dependency) — already available for tests

### Project Structure Notes

After this story, the `src/git_provider/` directory will be:

```
src/git_provider/
├── mod.rs       # GitProvider trait, shared types, factory, description builder, tests
├── github.rs    # GitHubProvider implementation + tests
└── gitlab.rs    # TODO stub (Story 5.3)
```

This aligns exactly with the architecture's Complete Project Directory Structure.

### References

- [Source: _bmad-output/planning-artifacts/architecture.md#Git Provider Trait Pattern — Params as Structs] — Trait signature, CreatePrParams, PrInfo structs
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 4: Error Propagation — Layered with Bubble-Up] — Three-tier error handling
- [Source: _bmad-output/planning-artifacts/architecture.md#Rig Tool Implementation Pattern] — Standard structure pattern (adapted for provider)
- [Source: _bmad-output/planning-artifacts/architecture.md#Test Mock Pattern] — Testing approach
- [Source: _bmad-output/planning-artifacts/architecture.md#Complete Project Directory Structure] — File layout
- [Source: _bmad-output/planning-artifacts/epics.md#Story 5.1] — Acceptance criteria and user story
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 5 Overview] — FRs covered: FR18, FR19, FR20, FR21, FR22, FR23, FR24
- [Source: _bmad-output/project-context.md#Git Provider Trait Pattern] — Params as structs, async trait, thiserror
- [Source: _bmad-output/project-context.md#Testing Rules] — Inline tests, mock LLM, E2E gated
- [Source: _bmad-output/project-context.md#Code Quality & Style Rules] — rustfmt, clippy, doc comments, no dead code
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — No unwrap, no anyhow in lib, no silent failures
- [Source: src/config/mod.rs#GitProviderConfig] — Config struct with provider, repo_owner, repo_name, target_branch
- [Source: src/config/mod.rs#BotSecrets] — github_token field loaded from .env
- [Source: src/supervisor/decisions.rs#format_pr_decisions_section] — Already implemented, ready to use
- [Source: src/supervisor/decisions.rs#DecisionRecord] — Struct used in SessionOutcome.decisions
- [Source: src/session/mod.rs#SessionOutcome] — Completed/Escalated/Failed with decisions vec
- [Source: src/session/escalation.rs#EscalationReport] — story_key, question, reason, branch_name, partial_work_summary, escalated_at
- [Source: src/git_provider/mod.rs] — Current skeleton (TODO only)
- [Source: src/git_provider/github.rs] — Current skeleton (TODO only)
- [Source: Cargo.toml] — octocrab 0.49, async-trait 0.1, thiserror 2

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (via Cursor)

### Debug Log References

- octocrab v0.49.5 `GitHubError` is `#[non_exhaustive]` — cannot construct from outside crate in tests. `map_octocrab_error` GitHub-specific status code mapping (401→AuthenticationFailed, 422→BranchNotFound, 429→RateLimited) tested via `Other` variant only in unit tests; full status mapping verified at E2E level.
- octocrab `Octocrab::builder().build()` requires a Tokio runtime (tower buffer service) — tests using `GitHubProvider::new()` use `#[tokio::test]`.
- rustls CryptoProvider must be explicitly installed since both `ring` and `aws-lc-rs` features are enabled transitively — added `install_crypto_provider()` test helper.

### Completion Notes List

- ✅ Task 0: All 8 prerequisites verified — Epic 4 code compiles, all required types/functions present
- ✅ Task 1: `GitProvider` trait, `GitProviderError` (9 variants), `CreatePrParams`, `PrInfo`, `create_provider()` factory, re-export of `GitHubProvider`, full doc comments
- ✅ Task 2: `GitHubProvider` with `create_pr`, `add_comment`, `get_pr_url`, `map_octocrab_error` helper, tracing instrumentation
- ✅ Task 3: `PrDescriptionParams`, `build_pr_description()`, `build_pr_title()` — markdown body with story info, failure details, decisions section, bmad-bot footer
- ✅ Task 4: 21 unit tests (16 in mod.rs, 4+1 in github.rs) — all pass
- ✅ Task 5: cargo check/test/clippy/fmt all clean. 454 total tests, zero regressions.
- Added `snafu`, `http`, `rustls` as dev-dependencies for test infrastructure (constructing octocrab error types in tests)

### Change Log

- 2026-02-08: Story 5.1 implementation complete — GitProvider trait, GitHubProvider, PR description builder, 21 tests

### File List

| File | Change |
|------|--------|
| `src/git_provider/mod.rs` | **OVERWRITTEN** — GitProvider trait, GitProviderError, CreatePrParams, PrInfo, create_provider factory, PrDescriptionParams, build_pr_description, build_pr_title, 16 unit tests |
| `src/git_provider/github.rs` | **OVERWRITTEN** — GitHubProvider struct, new(), create_pr, add_comment, get_pr_url, map_octocrab_error, 5 unit tests |
| `src/git_provider/gitlab.rs` | **UNCHANGED** — TODO stub for Story 5.3 |
| `Cargo.toml` | **MODIFIED** — Added dev-dependencies: snafu, http, rustls (for test infrastructure) |