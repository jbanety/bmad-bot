# Story 7.6: Git Provider & PR Creation Integration Tests

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want integration tests that verify PR creation, commenting, and description building work correctly,
so that I'm confident the daemon produces well-formed PRs on both GitHub and GitLab.

## Acceptance Criteria

1. **Given** a `GitProviderConfig` with `provider: "github"`
   **When** `create_provider()` is called with a valid token
   **Then** a `Box<dyn GitProvider>` is returned containing a `GitHubProvider`

2. **Given** a `GitProviderConfig` with `provider: "gitlab"`
   **When** `create_provider()` is called with a valid token
   **Then** a `Box<dyn GitProvider>` is returned containing a `GitLabProvider`

3. **Given** a `GitProviderConfig` with `provider: "bitbucket"` (unsupported)
   **When** `create_provider()` is called
   **Then** a `GitProviderError::ProviderNotConfigured` error is returned

4. **Given** a `GitLabProvider` constructed with an empty token
   **When** `new()` is called
   **Then** `GitProviderError::AuthenticationFailed` is returned

5. **Given** a successful story with supervisor decisions
   **When** `build_pr_description()` is called with `PrDescriptionParams` including decisions text
   **Then** the generated description contains:
   - Story key and title in the header
   - Outcome summary
   - A "Supervisor Decisions" section with the decisions content
   **And** `build_pr_title()` returns `feat({story_key}): {title}`

6. **Given** a failed story
   **When** `build_pr_description()` is called with failure details
   **Then** the description contains a "⚠️ Failure Details" section
   **And** `build_pr_title()` returns `wip({story_key}): {title} [NEEDS REVIEW]`

## Tasks / Subtasks

- [ ] Task 0: Prerequisites — Verify lib.rs and git_provider visibility (AC: all)
  - [ ] 0.1 Verify `src/lib.rs` exists with `pub mod git_provider;` (created by Story 7.1 Task 0). If missing, create it (see Dev Notes)
  - [ ] 0.2 Verify `bmad_bot::git_provider::{create_provider, build_pr_description, build_pr_title, GitProvider, GitProviderError, CreatePrParams, PrInfo, PrDescriptionParams}` are accessible from integration tests
  - [ ] 0.3 Verify `bmad_bot::git_provider::{GitHubProvider, GitLabProvider}` are accessible (both re-exported via `pub use` in mod.rs)
  - [ ] 0.4 Verify `bmad_bot::supervisor::decisions::{format_pr_decisions_section, DecisionRecord, DecisionSource}` are accessible (cross-module dependency)
  - [ ] 0.5 Run `cargo build` — must succeed

- [ ] Task 1: Create integration test file and module declaration (AC: all)
  - [ ] 1.1 Create `tests/integration/test_git_provider.rs`
  - [ ] 1.2 Add `mod test_git_provider;` in `tests/integration.rs`
  - [ ] 1.3 Add imports from `bmad_bot::git_provider::*`, `bmad_bot::config::GitProviderConfig`, `bmad_bot::supervisor::decisions::*`

- [ ] Task 2: Write provider factory integration tests — public API smoke tests (AC: #1, #2, #3)
  - [ ] 2.1 Happy path: test `create_provider()` with `"github"` + valid token (requires local `install_crypto_provider()` — see Dev Notes) AND `"gitlab"` + valid token → both return `Ok`. Use `#[test]` (factory is synchronous)
  - [ ] 2.2 Error paths: test `create_provider()` with `"bitbucket"` → `Err(ProviderNotConfigured { provider: "bitbucket" })` AND empty string provider → `Err(ProviderNotConfigured)`

- [ ] Task 3: Write GitLab empty token rejection test (AC: #4)
  - [ ] 3.1 Test `GitLabProvider::new(&config, "")` → returns `Err(AuthenticationFailed)` with reason containing "empty"

- [ ] Task 4: Write cross-module PR description integration tests (AC: #5)
  - [ ] 4.1 Build real `DecisionRecord` instances via `DecisionRecord::new()` (from supervisor module)
  - [ ] 4.2 Call `format_pr_decisions_section(&decisions)` to generate the decisions markdown (cross-module: supervisor → git_provider)
  - [ ] 4.3 Pass result into `PrDescriptionParams` and call `build_pr_description()`
  - [ ] 4.4 Assert description contains: story key in header (`## 📋 Story:`), outcome summary (`**Status:**`), "Supervisor Decisions" section with actual decision content, bmad-bot footer
  - [ ] 4.5 Assert `build_pr_title("5-1-git-provider", "Git Provider Trait", false)` → `"feat(5-1-git-provider): Git Provider Trait"`

- [ ] Task 5: Write failure PR description integration test (AC: #6)
  - [ ] 5.1 Build `PrDescriptionParams` with `failure_details: Some("LLM timeout after 3 retries")`
  - [ ] 5.2 Assert description contains "⚠️ Failure Details" section with the failure text
  - [ ] 5.3 Assert `build_pr_title("2-1-polling", "Sprint Polling", true)` → `"wip(2-1-polling): Sprint Polling [NEEDS REVIEW]"`

- [ ] Task 6: Write escalation PR description test (supplementary)
  - [ ] 6.1 Build `PrDescriptionParams` with escalation-style failure_details containing question, reason, and partial work summary
  - [ ] 6.2 Assert description contains all escalation fields

- [ ] Task 7: Write end-to-end factory → trait method chain test (supplementary)
  - [ ] 7.1 Call `create_provider()` for GitLab, then call `get_pr_url("42")` on the returned `Box<dyn GitProvider>`
  - [ ] 7.2 Assert URL matches `"https://gitlab.com/{owner}/{repo}/-/merge_requests/42"`
  - [ ] 7.3 Call `get_pr_url("not-a-number")` → assert `Err(InvalidPrId)`
  - [ ] 7.4 This validates the full factory → trait dispatch → method execution chain through the public API

- [ ] Task 8: Verify all tests pass (AC: all)
  - [ ] 8.1 `cargo test --test integration` — all git provider tests pass
  - [ ] 8.2 `cargo test` — no regressions in 573+ unit tests
  - [ ] 8.3 `cargo clippy` — zero warnings

## Dev Notes

### Cross-Module Integration Value

The `src/git_provider/mod.rs` already contains 20+ unit tests covering every AC individually. **ACs 1-4 are primarily "public API surface smoke tests"** — they re-verify factory and constructor behavior from an external crate perspective. The real integration value is in Tasks 4-7:

1. **Cross-module boundary (supervisor → git_provider) — Tasks 4-6:** The full chain `DecisionRecord::new()` → `format_pr_decisions_section()` → `PrDescriptionParams` → `build_pr_description()` crosses from `supervisor::decisions` into `git_provider`. Unit tests in `mod.rs` use hardcoded decision strings — integration tests use real `DecisionRecord` instances.
2. **Factory → trait dispatch chain — Task 7:** `create_provider()` returns `Box<dyn GitProvider>`. Calling trait methods on the boxed provider validates dynamic dispatch works correctly through the public API.
3. **External crate perspective — All tasks:** Tests import via `bmad_bot::git_provider::*` — exactly how the `tests/` crate sees the library. Any visibility regression breaks immediately.
4. **Octocrab crypto initialization — Task 2:** The GitHub provider requires a rustls crypto provider installed globally. Integration tests verify this works from an external crate context (the unit test helper is NOT accessible — see Dev Notes).

### Architecture Compliance

#### 🚨 CRITICAL — `src/lib.rs` Prerequisite (from Story 7.1 Task 0)

**The project is currently a pure binary crate** — `src/main.rs` only, no `src/lib.rs`. Without `lib.rs`, `use bmad_bot::anything;` will NOT compile.

**If Story 7.1 Task 0 is NOT yet implemented, you MUST do it first:**

Create `src/lib.rs`:
```rust
//! bmad-bot library crate — exposes modules for integration tests.
#![deny(clippy::all)]
#![warn(dead_code)]

pub mod config;
pub mod git_provider;
pub mod notifier;
pub mod pipeline;
pub mod review;
pub mod session;
pub mod supervisor;
pub mod tools;
pub mod watcher;
```

Then update `src/main.rs` — remove all `mod X;` lines (except `mod cli;` which stays binary-only).

**Verify:** `cargo build` + `cargo test` must pass with all 573+ existing unit tests.

#### Module Visibility — Already Correct

The `git_provider` module exports everything needed:

- `pub use github::GitHubProvider;` — re-exported in mod.rs L12
- `pub use gitlab::GitLabProvider;` — re-exported in mod.rs L13
- `pub fn create_provider()` — public factory L150
- `pub fn build_pr_description()` — public L190
- `pub fn build_pr_title()` — public L215
- `pub trait GitProvider` — public L124
- All error/param/result structs — public

The `supervisor::decisions` module exports:
- `pub fn format_pr_decisions_section()` — public L339
- `pub struct DecisionRecord` — public
- `pub enum DecisionSource` — public (RuleEngine, LlmFallback, Escalation variants)

No visibility fixes needed for this story.

#### 🚨 Rustls Crypto Provider — Must Copy Helper Locally

`GitHubProvider::new()` internally builds an `Octocrab` client which requires a rustls crypto provider to be installed globally. The existing unit tests in `github.rs` define an `install_crypto_provider()` helper, but it lives inside `#[cfg(test)] mod tests` — **NOT accessible from integration tests**.

**You MUST define your own copy** in `test_git_provider.rs`:

```rust
/// Install rustls crypto provider for GitHub octocrab client construction.
/// Safe to call multiple times — returns Err if already installed, which we ignore.
fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
```

Call this BEFORE any test that creates a GitHub provider (Task 2.1).

**🚨 `rustls` and `ring` must be in `[dev-dependencies]`** — verify they are available. The production code pulls them transitively via `octocrab`, but `tests/` is a separate crate that may need explicit dev-deps. If compilation fails with "unresolved import `rustls`", add:
```toml
[dev-dependencies]
rustls = { version = "0.23", default-features = false }
ring = "0.17"
```

#### Integration Test Location

- All tests: `tests/integration/test_git_provider.rs`
- Declared as `mod test_git_provider;` in `tests/integration.rs`
- Run via `cargo test --test integration`
- If `tests/integration.rs` doesn't exist yet (Story 7.1 not implemented), create it:
```rust
mod helpers;
mod test_git_provider;
```

### Technical Requirements

#### Key Type Signatures (exact from codebase)

**`GitProviderError`** (`src/git_provider/mod.rs`) — 9 variants:
```rust
#[derive(Debug, thiserror::Error)]
pub enum GitProviderError {
    ApiError { status: u16, message: String },
    AuthenticationFailed { reason: String },
    BranchNotFound { branch: String },
    RateLimited { retry_after_secs: Option<u64> },
    NetworkError { reason: String },
    InvalidResponse { reason: String },
    InvalidPrId { pr_id: String },
    ProviderNotConfigured { provider: String },
    BuildError { reason: String },
}
```

**`GitProvider` trait** (`src/git_provider/mod.rs`):
```rust
#[async_trait]
pub trait GitProvider: Send + Sync {
    async fn create_pr(&self, params: CreatePrParams) -> Result<PrInfo, GitProviderError>;
    async fn add_comment(&self, pr_id: &str, body: &str) -> Result<(), GitProviderError>;
    async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError>;
}
```

**`create_provider()` factory** (`src/git_provider/mod.rs`):
```rust
pub fn create_provider(
    config: &GitProviderConfig,
    token: &str,
) -> Result<Box<dyn GitProvider>, GitProviderError>
```
- `"github"` → `GitHubProvider::new(config, token)?` (requires crypto provider)
- `"gitlab"` → `GitLabProvider::new(config, token)?`
- other → `Err(ProviderNotConfigured { provider })`

**`GitLabProvider::new()`** rejects empty tokens:
```rust
pub fn new(config: &GitProviderConfig, token: &str) -> Result<Self, GitProviderError> {
    if token.is_empty() {
        return Err(GitProviderError::AuthenticationFailed {
            reason: "GitLab token is empty".into(),
        });
    }
    // ... build client
}
```

**`PrDescriptionParams`** (`src/git_provider/mod.rs`):
```rust
#[derive(Debug, Clone)]
pub struct PrDescriptionParams {
    pub story_key: String,
    pub story_title: String,
    pub outcome_summary: String,
    pub decisions_section: String,
    pub failure_details: Option<String>,
}
```

**`build_pr_description()`** output structure:
```
## 📋 Story: {story_key} — {story_title}

**Status:** {outcome_summary}

## ⚠️ Failure Details          ← only if failure_details is Some

{failure_details}

{decisions_section}

---
*Generated by bmad-bot*
```

**`build_pr_title()`**:
- Success: `"feat({story_key}): {story_title}"`
- Failure: `"wip({story_key}): {story_title} [NEEDS REVIEW]"`

**`DecisionRecord::new()`** (`src/supervisor/decisions.rs`):
```rust
pub fn new(
    question: String,
    context: Option<String>,
    answer: String,
    source: DecisionSource,
    reasoning: String,
    alternatives: Vec<String>,
) -> Self
```

**`DecisionSource`** enum (`#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]`):
```rust
pub enum DecisionSource {
    RuleEngine { rule_name: String },
    LlmFallback,                        // ← unit variant, NO fields
    Escalation,
}
```

**🚨 `LlmFallback` has NO fields** — it is a unit variant. Do NOT try to construct it with `{ model: "...", confidence: "..." }`.

**`format_pr_decisions_section()`** (`src/supervisor/decisions.rs`):
- Empty decisions → `"## 🤖 Supervisor Decisions\n\nNo supervisor decisions were made during this session.\n"`
- With decisions → markdown table with columns: `#`, `Source`, `Question`, `Decision`, `Reasoning`

#### `GitProviderConfig` struct (`src/config/mod.rs`):
```rust
pub struct GitProviderConfig {
    pub provider: String,      // "github" or "gitlab"
    pub repo_owner: String,
    pub repo_name: String,
    pub target_branch: String,
}
```

#### GitLab `get_pr_url()` URL Format

```rust
async fn get_pr_url(&self, pr_id: &str) -> Result<String, GitProviderError> {
    let iid = pr_id.parse::<u64>().map_err(|_| GitProviderError::InvalidPrId { ... })?;
    Ok(format!("https://gitlab.com/{}/{}/-/merge_requests/{}", self.owner, self.repo, iid))
}
```

URL pattern: `https://gitlab.com/{owner}/{repo}/-/merge_requests/{iid}`

#### GitHub `get_pr_url()` URL Format

```rust
Ok(format!("https://github.com/{}/{}/pull/{}", self.owner, self.repo, pr_number))
```

URL pattern: `https://github.com/{owner}/{repo}/pull/{number}`

#### GitLabProvider Private Fields

`GitLabProvider` fields (`client`, `base_url`, `project_path`, `token`, `owner`, `repo`) are all **private**. Integration tests cannot inspect them directly. Verify behavior via trait methods (`get_pr_url()` returns the correct URL pattern).

### Previous Story Intelligence (Stories 7.4, 7.5)

**Story 7.5 (Session WAL Crash Recovery):**
- Established the "Cross-Module Integration Value" section pattern — use it here
- Used full struct literal for `BotConfig` instead of `_test_minimal()` — follow same pattern if config needed
- `wal_path()` helper pattern for deriving private internal paths

**Story 7.4 (Pipeline Orchestration):**
- Defines `MockGitProvider` with builder pattern for test setup
- `PipelineTestBuilder` pattern — not needed here (no pipeline interaction)
- Tracing is a no-op in tests — silent without a subscriber

**Existing unit tests in `src/git_provider/mod.rs` (17 tests):**
- `test_create_provider_github_returns_ok` — factory with GitHub
- `test_create_provider_gitlab_returns_ok` — factory with GitLab
- `test_create_provider_unknown_returns_not_configured` — unsupported provider
- `test_pr_description_success_no_failure_details` — success PR body
- `test_pr_description_failure_includes_details` — failure PR body
- `test_pr_description_escalation_includes_details` — escalation PR body
- `test_pr_description_includes_decisions_section` — decisions in body
- `test_build_pr_title_success` / `test_build_pr_title_failure` — title formats
- These use hardcoded strings for `decisions_section` — integration tests use real `DecisionRecord` instances

**Existing unit tests in `src/git_provider/gitlab.rs` (14 tests):**
- `test_gitlab_provider_new_empty_token_fails` — empty token rejection
- `test_get_pr_url_constructs_correct_url` — URL construction
- `test_get_pr_url_invalid_pr_id` — invalid PR ID
- `test_map_gitlab_error_*` — error mapping for each HTTP status

**Existing unit tests in `src/git_provider/github.rs` (4 tests):**
- `test_github_provider_new_success` — construction with crypto provider
- `test_github_provider_new_stores_owner_repo` — field storage
- `test_map_octocrab_error_handles_variants` — error mapping

### Git Intelligence

Recent commits:
- `80e7a09 docs(stories): create story 7-5 session WAL crash recovery integration tests and update sprint status`
- `f1b5f31 docs(stories): create story 7-4 pipeline orchestration integration tests`
- All 573+ unit tests passing on `main`
- Git provider implementation is stable (Stories 5.1, 5.3 completed, in review)

### Dependencies Required

- `rustls` + `ring` — may need explicit `[dev-dependencies]` entries for crypto provider initialization in integration tests (see Dev Notes)
- `tokio` with `macros` + `rt-multi-thread` — for `#[tokio::test]` on async factory test (already available)
- `bmad_bot` — library crate (requires `lib.rs`)

### File Structure

```
tests/
├── integration.rs                           # Entry point (cargo test --test integration)
├── integration/
│   ├── helpers/
│   │   ├── mod.rs                           # Re-exports helper modules
│   │   ├── fixtures.rs                      # Shared fixture builders (from 7.1)
│   │   └── mocks.rs                         # Mock implementations (from 7.1)
│   ├── test_git_provider.rs                 # ← THIS STORY's tests
│   ├── test_session_wal.rs                  # Story 7.5 tests
│   ├── test_pipeline.rs                     # Story 7.4 tests
│   └── ...                                  # Other story test files
```

If Story 7.1 infrastructure is not yet built, create the minimal structure:
```
tests/
├── integration.rs                           # mod test_git_provider;
├── integration/
│   └── test_git_provider.rs                 # This story's tests
```

### Testing Standards

- **Framework:** `#[test]` for most tests — `create_provider()`, `build_pr_description()`, `build_pr_title()`, `GitLabProvider::new()` are all **synchronous** functions. Only use `#[tokio::test]` for Task 7 which calls `get_pr_url().await` (async trait method)
- **Test naming:** `test_git_provider_{behavior}_{scenario}` — e.g., `test_git_provider_factory_github_returns_ok`
- **Structure:** Arrange → Act → Assert
- **Isolation:** No shared mutable state — each test constructs its own config/params
- **No real API calls:** Tests only exercise construction, local methods (`get_pr_url`), and pure functions (`build_pr_description`, `build_pr_title`). Never call `create_pr` or `add_comment` against real GitHub/GitLab APIs
- **Zero warnings:** `cargo clippy` clean

### Project Structure Notes

- Alignment with `tests/integration/` convention from Story 7.1
- `tests/e2e/` reserved for live API tests (gated behind `BMAD_E2E=1`) — do NOT modify
- GitHub/GitLab API calls are E2E scope only — integration tests verify construction and pure logic

### References

- [Source: src/git_provider/mod.rs] — `GitProvider` trait, `GitProviderError`, `create_provider()`, `build_pr_description()`, `build_pr_title()`, `CreatePrParams`, `PrInfo`, `PrDescriptionParams`
- [Source: src/git_provider/mod.rs#L224-496] — 17 existing unit tests (factory, error display, PR description/title builders)
- [Source: src/git_provider/github.rs] — `GitHubProvider`, `map_octocrab_error()`, crypto provider requirement
- [Source: src/git_provider/gitlab.rs] — `GitLabProvider`, empty token rejection, `get_pr_url()` URL format, `map_gitlab_error()`
- [Source: src/git_provider/gitlab.rs#L232-466] — 14 existing unit tests
- [Source: src/supervisor/decisions.rs#L339-349] — `format_pr_decisions_section()` — generates markdown table from `DecisionRecord` vec
- [Source: src/config/mod.rs] — `GitProviderConfig` struct
- [Source: _bmad-output/planning-artifacts/epics.md#L1063-1105] — Story 7.6 acceptance criteria
- [Source: _bmad-output/planning-artifacts/architecture.md#L479-510] — Git Provider Trait Pattern
- [Source: _bmad-output/implementation-artifacts/7-4-pipeline-orchestration-integration-tests.md] — MockGitProvider pattern, PipelineTestBuilder
- [Source: _bmad-output/implementation-artifacts/7-5-session-wal-crash-recovery-integration-tests.md] — Cross-module integration value pattern
- [Source: _bmad-output/project-context.md] — Rust edition 2024, async tokio, test mock pattern

## Dev Agent Record

### Agent Model Used

### Debug Log References

### Completion Notes List

### File List