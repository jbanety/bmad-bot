# Story 5.3: GitLab Merge Request Support

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer using GitLab,
I want the daemon to create Merge Requests with the same comprehensive descriptions as GitHub PRs,
so that I get the same experience regardless of my git provider.

## Acceptance Criteria

1. **Given** the `bmad-bot.yaml` config specifies `git_provider: gitlab`
   **When** the GitLab implementation of `GitProvider` is initialized
   **Then** it uses the shared `build_http_client()` (reqwest-middleware with retry) to call the GitLab REST API (v4) with the token loaded from `.env` (`GITLAB_TOKEN`)
   **And** it implements all trait methods: `create_pr` (creates a Merge Request), `add_comment` (posts a note on the MR), `get_pr_url` (returns the MR web URL)

2. **Given** a development session has completed
   **When** the daemon creates a Merge Request via the GitLab implementation
   **Then** the MR is created with: agent-written title and description, source branch, target branch
   **And** the MR description includes the "🤖 Supervisor Decisions" section, identical in format to the GitHub implementation

3. **Given** code review is enabled and completes
   **When** the review is posted on GitLab
   **Then** the review is posted as a note (comment) on the Merge Request via the GitLab notes API (`POST /projects/:id/merge_requests/:merge_request_iid/notes`)
   **And** the format and content are consistent with the GitHub PR comment implementation

4. **Given** the GitLab API returns rate limit or transient errors
   **When** the git_provider makes API calls
   **Then** errors are handled by the reqwest-middleware retry layer (exponential backoff, max 3 retries) via `build_http_client()`
   **And** permanent failures return a descriptive `GitProviderError` with the HTTP status and response body

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [ ] Verify Story 5.1 types are present and compilable: `GitProvider` trait, `GitProviderError`, `CreatePrParams`, `PrInfo`, `create_provider()` factory in `src/git_provider/mod.rs`
- [ ] Verify `GitHubProvider` exists in `src/git_provider/github.rs` as implementation reference
- [ ] Verify `build_http_client()` exists in `src/config/mod.rs` returning `reqwest_middleware::ClientWithMiddleware` with retry
- [ ] Verify `BotSecrets.gitlab_token` field exists in `src/config/mod.rs`
- [ ] Verify `GitProviderConfig` struct has `provider`, `repo_owner`, `repo_name`, `target_branch` fields
- [ ] Verify `reqwest = "0.13"` (with `json` feature), `reqwest-middleware = "0.5"`, `reqwest-retry = "0.9"`, `async-trait = "0.1"`, `serde = "1"` (with `derive`), `serde_json = "1"` are in `Cargo.toml`
- [ ] Confirm existing skeleton file: `src/git_provider/gitlab.rs` (currently TODO stub)

### Task 1: Define GitLab API Response Types (`src/git_provider/gitlab.rs`)

- [ ] Define private response structs for serde deserialization:
  - `#[derive(serde::Deserialize)] struct CreateMrResponse`:
    - `iid: u64` — merge request **internal** ID scoped to the project (⚠️ NOT `id` which is the global database ID — see "GitLab iid vs id" section in Dev Notes)
    - `web_url: String` — browser URL for the MR
  - These are internal to `gitlab.rs`, not part of the public API
  - Only deserialize the fields we need (use `#[serde(default)]` or just let serde ignore extra fields — `Deserialize` ignores unknown fields by default)
  - GitLab API already uses snake_case — no `#[serde(rename_all)]` needed

### Task 2: Implement `GitLabProvider` (`src/git_provider/gitlab.rs`)

- [ ] Define `GitLabProvider` struct:
  - `client: ClientWithMiddleware` — shared HTTP client from `build_http_client()`
  - `base_url: String` — GitLab API base URL, defaults to `"https://gitlab.com/api/v4"` (constructed in `new()`)
  - `project_path: String` — URL-encoded `"{repo_owner}/{repo_name}"` for API paths
  - `token: String` — GitLab personal access token from `.env`
  - `owner: String` — repo owner (for web URL construction)
  - `repo: String` — repo name (for web URL construction)
- [ ] Implement `GitLabProvider::new(config: &GitProviderConfig, token: &str) -> Result<Self, GitProviderError>`:
  - Validate token is not empty → `GitProviderError::AuthenticationFailed { reason: "GitLab token is empty".into() }`
  - Build HTTP client via `crate::config::build_http_client()`
  - URL-encode project path: `format!("{}%2F{}", config.repo_owner, config.repo_name)` (GitLab API requires URL-encoded path for `:id` parameter)
  - Store `base_url` as `"https://gitlab.com/api/v4"` (hardcoded for MVP; could be made configurable for self-hosted GitLab in a future story)
  - Store owner, repo for web URL construction
  - Return `Ok(Self { ... })`
- [ ] Implement `#[async_trait] GitProvider for GitLabProvider` — follow the HTTP request pattern documented in "reqwest-middleware Request Pattern" section of Dev Notes for all three methods:
  - **`create_pr`**:
    - POST `{base_url}/projects/{project_path}/merge_requests`
    - Header: `PRIVATE-TOKEN: {self.token}`
    - JSON body: `{ "source_branch": params.source_branch, "target_branch": params.target_branch, "title": params.title, "description": params.body }`
    - On success (201): deserialize response as `CreateMrResponse`, extract `iid` and `web_url`
    - Return `PrInfo { id: iid.to_string(), url: web_url, number: iid }`
    - Log via `tracing::info!(action = "mr_created", mr_iid = %iid, url = %web_url, "Merge request created")`
  - **`add_comment`**:
    - Parse `pr_id` → `u64` via `pr_id.parse::<u64>().map_err(|_| GitProviderError::InvalidPrId { pr_id: pr_id.to_string() })`
    - POST `{base_url}/projects/{project_path}/merge_requests/{mr_iid}/notes`
    - Header: `PRIVATE-TOKEN: {self.token}`
    - JSON body: `{ "body": body }`
    - On success (201): return `Ok(())`
    - Log via `tracing::info!(action = "mr_comment_added", pr_id = %pr_id, "Note added to merge request")`
  - **`get_pr_url`**:
    - Parse `pr_id` → `u64` (same pattern as `add_comment`)
    - Construct URL deterministically: `format!("https://gitlab.com/{}/{}/-/merge_requests/{}", self.owner, self.repo, mr_iid)` — no API call needed
    - Return the URL string
- [ ] Implement private helper `fn map_gitlab_error(status: reqwest::StatusCode, body: String) -> GitProviderError`:
  - `401 | 403` → `GitProviderError::AuthenticationFailed { reason: body }`
  - `404` → `GitProviderError::ApiError { status: status.as_u16(), message: body }`
  - `422` → `GitProviderError::BranchNotFound { branch: body }` (GitLab returns 422 when source branch doesn't exist or has no diff from target)
  - `429` → `GitProviderError::RateLimited { retry_after_secs: None }` (note: transient 429s are already retried by reqwest-middleware, so this only fires after retries exhausted)
  - `_` → `GitProviderError::ApiError { status: status.as_u16(), message: body }`
- [ ] Add `///` doc comments on all public items

### Task 3: Update Factory Function (`src/git_provider/mod.rs`)

- [ ] Update `create_provider()` factory function:
  - Change the `"gitlab"` match arm from:
    `Err(GitProviderError::ProviderNotConfigured { provider: "gitlab (not yet implemented)".into() })`
  - To:
    `"gitlab" => GitLabProvider::new(config, token).map(|p| Box::new(p) as Box<dyn GitProvider>)`
- [ ] Add `pub use gitlab::GitLabProvider;` re-export in `mod.rs`
- [ ] Update the `test_create_provider_gitlab_returns_not_configured` test → rename to `test_create_provider_gitlab_returns_ok` and verify it succeeds with a valid token

### Task 4: Unit Tests

- [ ] Tests in `src/git_provider/gitlab.rs` `#[cfg(test)] mod tests`:
  - `test_gitlab_provider_new_success` — constructor returns `Ok` with valid (non-empty) token
  - `test_gitlab_provider_new_empty_token_fails` — constructor returns `AuthenticationFailed` for empty token
  - `test_gitlab_provider_new_stores_fields` — verify struct fields (project_path encoding, owner, repo) after construction
  - `test_gitlab_provider_project_path_encoding` — verify `repo_owner/repo_name` is correctly URL-encoded as `repo_owner%2Frepo_name`
  - `test_gitlab_provider_is_send_sync` — compile-time trait check: `fn assert_send_sync<T: Send + Sync>() {}; assert_send_sync::<GitLabProvider>();`
  - `test_map_gitlab_error_401_authentication` — verify 401 maps to `AuthenticationFailed`
  - `test_map_gitlab_error_403_authentication` — verify 403 maps to `AuthenticationFailed`
  - `test_map_gitlab_error_404_api_error` — verify 404 maps to `ApiError`
  - `test_map_gitlab_error_422_branch_not_found` — verify 422 maps to `BranchNotFound`
  - `test_map_gitlab_error_429_rate_limited` — verify 429 maps to `RateLimited`
  - `test_map_gitlab_error_500_api_error` — verify 500 maps to `ApiError` with status and body
  - `test_get_pr_url_constructs_correct_url` — verify URL format: `https://gitlab.com/{owner}/{repo}/-/merge_requests/{iid}`
  - `test_get_pr_url_invalid_pr_id` — verify non-numeric pr_id returns `InvalidPrId` error
  - `test_add_comment_invalid_pr_id` — verify non-numeric pr_id returns `InvalidPrId` error
  - `test_create_mr_response_deserializes_from_gitlab_json` — verify `CreateMrResponse` correctly deserializes from a realistic GitLab API JSON response (use `serde_json::from_str` with a hardcoded JSON string matching the GitLab response format, confirming `iid` and `web_url` are extracted and extra fields are ignored)
  - NOTE: No live API tests here — reqwest calls are tested in E2E only
- [ ] Tests in `src/git_provider/mod.rs` `#[cfg(test)] mod tests`:
  - Update `test_create_provider_gitlab_returns_not_configured` → `test_create_provider_gitlab_returns_ok` — factory with `provider: "gitlab"` and valid token succeeds

### Task 5: Integration Verification

- [ ] `cargo check` — zero new errors
- [ ] `cargo test` — all new tests pass, no regressions
- [ ] `cargo clippy` — zero new warnings
- [ ] `cargo fmt` — all clean
- [ ] Verify `#![deny(clippy::all)]` is respected (no new warnings)
- [ ] Verify all public items have `///` doc comments

## Dev Notes

### Previous Story Intelligence

**Story 5.1** (Git Provider Trait & GitHub PR Creation) — direct dependency:
- `GitProvider` trait, `GitProviderError`, `CreatePrParams`, `PrInfo` all defined in `src/git_provider/mod.rs`
- `create_provider()` factory currently returns `ProviderNotConfigured` for `"gitlab"` — must be updated
- `GitHubProvider` in `github.rs` is the structural reference — `GitLabProvider` follows the same pattern but with reqwest instead of octocrab
- `PrDescriptionParams` and `build_pr_description()` / `build_pr_title()` helpers already exist in `mod.rs` — no changes needed, these are provider-agnostic
- `map_octocrab_error()` pattern in `github.rs` → adapt as `map_gitlab_error()` for HTTP status codes
- `GitProviderError` already has all needed variants (9 total): `ApiError`, `AuthenticationFailed`, `BranchNotFound`, `RateLimited`, `NetworkError`, `InvalidResponse`, `InvalidPrId`, `ProviderNotConfigured`, `BuildError`

**Story 5.2** (Automated Code Review Session):
- `ReviewOutcome::Completed { report }` → orchestrator posts `report` as MR comment via `GitProvider::add_comment()` — the GitLab `add_comment` implementation must handle this correctly
- Code review module does NOT interact with `git_provider` directly — the orchestrator calls `add_comment` after PR/MR creation

**Story 4.3** (Branch Management):
- Test count: **435 tests** (from 5.1 story notes). Expect more from 5.1/5.2 implementations.
- `StoryInfo.branch_name` is pre-computed as `format!("story/{key}")` — this is the value used for `CreatePrParams.source_branch`

### Core Design — GitLabProvider via reqwest-middleware

The architecture mandates: "GitLab impl via reqwest" in a separate file (`gitlab.rs`). Key design decisions:

1. **HTTP Client**: Use `crate::config::build_http_client()` which returns `reqwest_middleware::ClientWithMiddleware` with automatic retry (exponential backoff, max 3 retries). This satisfies AC4 directly — no retry logic needed in the provider itself.

2. **Authentication**: GitLab REST API v4 uses `PRIVATE-TOKEN` header with personal access token. Token loaded from `BotSecrets.gitlab_token` (env var: `GITLAB_TOKEN`). The header value is the raw token string — no `Bearer` prefix, no encoding.

3. **Project Identification**: GitLab API uses `:id` parameter which accepts either numeric project ID or URL-encoded project path (`owner%2Frepo`). Since `GitProviderConfig` provides `repo_owner` and `repo_name`, we URL-encode the path with manual `%2F` (sufficient for simple owner/repo names).

4. **MR Identification**: GitLab uses `iid` (internal ID, scoped to project) for merge requests. This maps to `PrInfo.id` (as String) and `PrInfo.number` (as u64). All subsequent operations (`add_comment`, `get_pr_url`) use the `iid`.

5. **Web URL Pattern**: `https://gitlab.com/{owner}/{repo}/-/merge_requests/{iid}` — note the `/-/` separator which is GitLab's standard URL format.

### GitLab `iid` vs `id` — CRITICAL

⚠️ **GitLab returns two ID fields in every MR response:**

| Field | Type | Meaning | Example |
|-------|------|---------|---------|
| `id` | u64 | **Global** database ID across all projects | `52` |
| `iid` | u64 | **Internal** ID scoped to the project (the number in the URL) | `34` |

**You MUST use `iid` everywhere:**
- `iid` is what appears in MR URLs: `/-/merge_requests/34`
- `iid` is what the notes API requires: `/merge_requests/:merge_request_iid/notes`
- `iid` is what humans see and reference

The `id` field is useless for our purposes — it's a global counter across all GitLab projects. **Never use `id` where `iid` is needed.** The `CreateMrResponse` struct intentionally only deserializes `iid` and `web_url`.

### reqwest-middleware Request Pattern — CRITICAL

`ClientWithMiddleware` from `reqwest-middleware 0.5` wraps `reqwest::Client` and exposes the same builder API: `.get(url)`, `.post(url)`, `.put(url)`, etc. These return `reqwest_middleware::RequestBuilder` (not `reqwest::RequestBuilder`) which supports `.header()`, `.json()`, `.send()`.

**Error type**: `.send().await` returns `Result<reqwest::Response, reqwest_middleware::Error>` — **NOT** `reqwest::Error`.

`reqwest_middleware::Error` is an enum with two variants:
- `Error::Middleware(anyhow::Error)` — middleware chain failure
- `Error::Reqwest(reqwest::Error)` — underlying HTTP client error

Both should map to `GitProviderError::NetworkError { reason: e.to_string() }`.

**Complete HTTP request + response handling pattern for every API call:**

```rust
// 1. Build and send request — handle network/middleware errors
let response = self.client
    .post(format!("{}/projects/{}/merge_requests", self.base_url, self.project_path))
    .header("PRIVATE-TOKEN", &self.token)
    .json(&serde_json::json!({
        "source_branch": params.source_branch,
        "target_branch": params.target_branch,
        "title": params.title,
        "description": params.body,
    }))
    .send()
    .await
    .map_err(|e| GitProviderError::NetworkError { reason: e.to_string() })?;

// 2. Check HTTP status — handle API errors
if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    return Err(map_gitlab_error(status, body));
}

// 3. Deserialize success response — handle parse errors
let mr: CreateMrResponse = response
    .json()
    .await
    .map_err(|e| GitProviderError::InvalidResponse {
        reason: format!("Failed to parse GitLab MR response: {e}"),
    })?;
```

**Required imports:**

```rust
use async_trait::async_trait;
use reqwest_middleware::ClientWithMiddleware;
use crate::config::build_http_client;
use super::{CreatePrParams, GitProvider, GitProviderError, PrInfo};
// Note: reqwest_middleware::Error is handled inline via .map_err(), no direct import needed
```

### GitLab REST API v4 — Endpoints Used

**Create Merge Request:**
```
POST /api/v4/projects/:id/merge_requests
Headers: PRIVATE-TOKEN: <token>
Content-Type: application/json (set automatically by .json())
Body: { "source_branch": "...", "target_branch": "...", "title": "...", "description": "..." }
Response (201): { "id": 52, "iid": 34, "web_url": "https://gitlab.com/owner/repo/-/merge_requests/34", ... }
```

**Add Note (Comment) to Merge Request:**
```
POST /api/v4/projects/:id/merge_requests/:merge_request_iid/notes
Headers: PRIVATE-TOKEN: <token>
Content-Type: application/json
Body: { "body": "..." }
Response (201): { "id": 302, "body": "...", ... }
```

**Get MR URL** — No API call needed. Constructed deterministically:
`https://gitlab.com/{owner}/{repo}/-/merge_requests/{iid}`

### GitLab API Error Mapping

GitLab REST API returns standard HTTP status codes. Mapping to `GitProviderError`:

| HTTP Status | GitProviderError Variant | Notes |
|-------------|--------------------------|-------|
| 401 | `AuthenticationFailed` | Invalid or expired token |
| 403 | `AuthenticationFailed` | Insufficient permissions |
| 404 | `ApiError` | Project or MR not found |
| 409 | `ApiError` | Conflict (e.g., MR already exists for this branch pair) |
| 422 | `BranchNotFound` | Source branch doesn't exist or no diff between source/target |
| 429 | `RateLimited` | Rate limit exceeded (after reqwest-middleware retries exhausted) |
| 5xx | `ApiError` | Server errors (after reqwest-middleware retries exhausted for 500/503) |

⚠️ **Important**: Transient errors (429, 500, 503) are automatically retried by the `reqwest-middleware` retry layer before the provider ever sees them. The error mapping only fires for permanent failures or exhausted retries.

### Structural Comparison: GitHubProvider vs GitLabProvider

| Aspect | GitHubProvider | GitLabProvider |
|--------|---------------|----------------|
| HTTP Client | `octocrab::Octocrab` (internal reqwest) | `reqwest_middleware::ClientWithMiddleware` via `build_http_client()` |
| Auth | `personal_token()` on builder | `PRIVATE-TOKEN` header on each request |
| Create PR/MR | `octocrab.pulls().create().body().send()` | `client.post(url).header().json().send()` |
| Add Comment | `octocrab.issues().create_comment()` | `client.post(notes_url).header().json().send()` |
| Get URL | Deterministic format string | Deterministic format string |
| Error Source | `octocrab::Error` enum | `reqwest_middleware::Error` + HTTP status codes |
| Error Mapping | `map_octocrab_error(e: octocrab::Error)` | `map_gitlab_error(status: StatusCode, body: String)` |
| Retry | None (octocrab has no built-in retry) | Built-in via `reqwest-middleware` |
| MR/PR ID type | `pr.number` (u64) | `mr.iid` (u64) — internal ID, NOT global `id` |

### Self-Hosted GitLab Consideration

The `base_url` is hardcoded to `"https://gitlab.com/api/v4"` for the MVP. The `get_pr_url` web URL is also hardcoded to `https://gitlab.com/...`. For self-hosted GitLab instances, a future enhancement could add an optional `gitlab_url` field to `GitProviderConfig` and derive both `base_url` and web URL from it. This is explicitly **out of scope** for this story but documented for awareness.

### Files Created/Modified in This Story

| File | Change |
|------|--------|
| `src/git_provider/gitlab.rs` | **OVERWRITE** — Replace TODO stub with `GitLabProvider` implementation + `CreateMrResponse` type + error mapping + unit tests |
| `src/git_provider/mod.rs` | **MODIFY** — Update factory `create_provider()` to instantiate `GitLabProvider` for `"gitlab"`, add `pub use gitlab::GitLabProvider` re-export, update related test |

### Anti-Patterns to Avoid

- ❌ **No `unwrap()` or `expect()` in production code** — only in tests
- ❌ **No `anyhow::Result`** in library modules — use `GitProviderError` exclusively
- ❌ **No `println!` or `eprintln!`** — use `tracing` only
- ❌ **No real API calls in unit tests** — mock everything, real calls only in E2E
- ❌ **No logging of API tokens** — never log the GitLab token value
- ❌ **No custom retry logic** — `build_http_client()` handles retries via reqwest-middleware
- ❌ **Do NOT create a new `reqwest::Client`** — use `build_http_client()` exclusively
- ❌ **Do NOT use `reqwest::Error`** — `.send().await` returns `reqwest_middleware::Error`, map it via `.map_err(|e| GitProviderError::NetworkError { reason: e.to_string() })`
- ❌ **Do NOT use GitLab's `id` field** — always use `iid` (internal ID scoped to project)
- ❌ **Do NOT modify `GitProviderError`** — all needed variants already exist from Story 5.1
- ❌ **Do NOT modify `PrDescriptionParams` or builders** — they are provider-agnostic
- ❌ **Do NOT modify `src/git_provider/github.rs`** — GitHub implementation is complete
- ❌ **Do NOT modify `src/config/mod.rs`** — GitLab config and secrets already fully supported
- ❌ **Do NOT implement self-hosted GitLab URL configuration** — out of scope, hardcode `gitlab.com`
- ❌ **Do NOT implement orchestration** — this story implements the provider layer only
- ❌ **Do NOT parse `pr_id` with `unwrap()`** — use `InvalidPrId` error variant for parse failures

### Scope Boundaries

**IN SCOPE:**
- `GitLabProvider` struct with `new()` constructor
- `GitProvider` trait implementation for `GitLabProvider` (all three methods)
- `CreateMrResponse` private serde response struct with `#[derive(Deserialize)]`
- `map_gitlab_error()` private helper
- Update `create_provider()` factory to support `"gitlab"`
- `pub use gitlab::GitLabProvider` re-export
- Unit tests for all the above (16 tests in gitlab.rs + 1 updated in mod.rs)

**OUT OF SCOPE:**
- Self-hosted GitLab URL configuration (future enhancement)
- GitHub implementation changes (already complete in Story 5.1)
- Code review session (Story 5.2 — already complete)
- Retry/resilience beyond `build_http_client()` (handled by reqwest-middleware)
- Calling the provider from the watcher/session loop (future integration)
- Notifications about MR creation (Story 6.1)
- Orchestration logic (future scope)

### Testing Requirements

All tests follow Arrange → Act → Assert pattern. Test naming: `test_{module}_{behavior}_{scenario}`.

**Unit tests (no network, no API calls):**
- Constructor success and failure (empty token)
- Field storage verification (project_path encoding, owner, repo)
- URL-encoded project path construction
- Send + Sync trait bounds compile check
- Error mapping for all relevant HTTP status codes (401, 403, 404, 422, 429, 500)
- `get_pr_url` deterministic URL construction
- `get_pr_url` / `add_comment` invalid `pr_id` handling
- `CreateMrResponse` deserialization from realistic GitLab JSON (verifies field names match API)
- Factory function updated to accept `"gitlab"`

**E2E tests (future, gated behind `BMAD_E2E=1`):**
- Real GitLab API MR creation → not in this story

### Dev Dependencies Required

No new dependencies needed. All already present in `Cargo.toml`:
- `reqwest = "0.13"` (with `json` feature) — HTTP client
- `reqwest-middleware = "0.5"` — retry middleware (provides `ClientWithMiddleware`, `RequestBuilder`)
- `reqwest-retry = "0.9"` — retry policy
- `async-trait = "0.1"` — async trait methods
- `thiserror = "2"` — error enum derive (already used by `GitProviderError`)
- `serde = "1"` (with `derive`) — response deserialization
- `serde_json = "1"` — JSON body construction and test deserialization
- `tracing = "0.1"` — structured logging
- `tempfile = "3"` (dev-dependency) — available for tests

### Project Structure Notes

After this story, the `src/git_provider/` directory will be:

```
src/git_provider/
├── mod.rs       # GitProvider trait, shared types, factory (now routes gitlab), description builder, tests
├── github.rs    # GitHubProvider implementation + tests (unchanged)
└── gitlab.rs    # GitLabProvider implementation + CreateMrResponse + error mapping + tests (NEW)
```

This aligns exactly with the architecture's Complete Project Directory Structure.

### References

- [Source: _bmad-output/planning-artifacts/architecture.md#Git Provider Trait Pattern — Params as Structs] — Trait signature, mandatory rules for implementations in separate files
- [Source: _bmad-output/planning-artifacts/architecture.md#Error Type Pattern — Per-Module thiserror Enums] — GitProviderError already defined, reuse all variants
- [Source: _bmad-output/planning-artifacts/architecture.md#Complete Project Directory Structure] — `src/git_provider/gitlab.rs` placement
- [Source: _bmad-output/planning-artifacts/architecture.md#External Integration Points] — GitLab API: HTTPS via reqwest, token from `.env`
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 4: Error Propagation — Layered with Bubble-Up] — Three-tier error handling
- [Source: _bmad-output/planning-artifacts/epics.md#Story 5.3] — Acceptance criteria and user story
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 5 Overview] — FRs covered: FR21, FR22, FR23, FR24
- [Source: _bmad-output/project-context.md#Git Provider Trait Pattern] — Params as structs, async trait, thiserror, separate files
- [Source: _bmad-output/project-context.md#Testing Rules] — Inline tests, mock responses, E2E gated
- [Source: _bmad-output/project-context.md#Code Quality & Style Rules] — rustfmt, clippy, doc comments, no dead code
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — No unwrap, no anyhow in lib, no silent failures
- [Source: _bmad-output/project-context.md#Resilience Rules] — Retry with backoff, max 3 retries per call
- [Source: _bmad-output/implementation-artifacts/5-1-git-provider-trait-github-pr-creation.md] — Full Story 5.1 context: trait definition, GitHubProvider patterns, error mapping strategy, factory function
- [Source: _bmad-output/implementation-artifacts/5-1-git-provider-trait-github-pr-creation.md#Error Mapping Strategy] — Three-tier error propagation pattern
- [Source: _bmad-output/implementation-artifacts/5-1-git-provider-trait-github-pr-creation.md#Anti-Patterns to Avoid] — Comprehensive anti-pattern list (applicable to GitLab too)
- [Source: _bmad-output/implementation-artifacts/5-2-automated-code-review-session.md] — Review session context: `ReviewOutcome::Completed { report }` posted via `add_comment`
- [Source: src/git_provider/mod.rs] — Current stub with `mod github; mod gitlab;` declarations
- [Source: src/git_provider/github.rs] — Current stub (TODO — implemented in story branch)
- [Source: src/git_provider/gitlab.rs] — Current stub (TODO — to be implemented by this story)
- [Source: src/config/mod.rs#GitProviderConfig] — Config struct with `provider`, `repo_owner`, `repo_name`, `target_branch`
- [Source: src/config/mod.rs#BotSecrets] — `gitlab_token: Option<String>` field loaded from `GITLAB_TOKEN` env var
- [Source: src/config/mod.rs#build_http_client] — Returns `reqwest_middleware::ClientWithMiddleware` with exponential backoff retry (max 3)
- [Source: src/config/mod.rs#VALID_GIT_PROVIDERS] — `&["github", "gitlab"]` — already validates gitlab
- [Source: Cargo.toml] — reqwest 0.13, reqwest-middleware 0.5, reqwest-retry 0.9, async-trait 0.1, serde 1, serde_json 1
- [Source: GitLab REST API v4 Docs] — POST `/projects/:id/merge_requests` (201 response with `iid`, `web_url`), POST `/projects/:id/merge_requests/:iid/notes` (201 response)
- [Source: reqwest-middleware 0.5 source] — `ClientWithMiddleware` exposes `.post()/.get()/.header()/.json()/.send()` API; `.send()` returns `Result<reqwest::Response, reqwest_middleware::Error>`; `Error` enum: `Middleware(anyhow::Error) | Reqwest(reqwest::Error)`

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List