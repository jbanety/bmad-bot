---
title: 'Handle GitLab 409 duplicate MR on review resume'
type: 'bugfix'
created: '2026-04-30'
status: 'done'
route: 'one-shot'
context:
  - '_bmad-output/project-context.md'
---

# Handle GitLab 409 duplicate MR on review resume

## Intent

**Problem:** When the daemon resumes a story with status `review`, it tries to create a new MR. If one already exists (GitLab returns HTTP 409), the error is treated as blocking — the story fails and the pipeline moves on without completing the review. The GitHub provider already handles this correctly via `DuplicatePr` detection and `find_open_pr` fallback.

**Approach:** Mirror the GitHub provider pattern in the GitLab provider: map 409 to `DuplicatePr`, add a `find_open_mr` method that queries `GET /merge_requests?source_branch=X&state=opened`, and catch `DuplicatePr` in `create_pr` to return the existing MR info instead of failing.

## Suggested Review Order

1. [gitlab.rs:280-300](../src/git_provider/gitlab.rs) — Error mapping: 409 now produces `DuplicatePr` instead of generic `ApiError`
2. [gitlab.rs:105-115](../src/git_provider/gitlab.rs) — `create_pr` intercept: catches `DuplicatePr` and falls back to `find_open_mr`
3. [gitlab.rs:220-275](../src/git_provider/gitlab.rs) — New `find_open_mr` method with URL-safe query params via `reqwest::Url::parse_with_params`
4. [gitlab.rs:476-490](../src/git_provider/gitlab.rs) — Test: 409 error mapping produces `DuplicatePr`
5. [gitlab.rs:560-590](../src/git_provider/gitlab.rs) — Tests: `ListMrResponse` deserialization (array + empty array)
