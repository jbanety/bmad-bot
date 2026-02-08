# Story 6.2: HTTP Retry & Error Resilience

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want all external HTTP calls to be resilient to transient failures,
So that temporary provider outages don't derail overnight runs.

## Acceptance Criteria

1. **Given** all 3 retries are exhausted for an LLM provider call (retry middleware configured in Story 1.1) **When** the final retry fails **Then** the error bubbles up to the session/daemon layer (Layer 3 error propagation) **And** the session commits partial work, creates a PR with failure description, and notifies the human

2. **Given** all 3 retries are exhausted for a GitHub/GitLab API call **When** PR creation or comment posting fails permanently **Then** the error is logged with full context (HTTP status, response body, story_id) **And** the human is notified via Telegram with the failure details and the branch name so they can create the PR manually

3. **Given** a blocking error occurs at any point in the pipeline (session crash, git failure, all LLM providers down) **When** the daemon's Layer 3 error handler catches it **Then** a notification is sent to the human with: story ID, error type, error details, and recovery guidance **And** the daemon moves on to the next eligible story (does not stop the entire run)

## Functional Requirements Covered

- **FR33:** The daemon can handle LLM provider rate limits with retry and exponential backoff
- **FR35:** The daemon can notify the human of any blocking error (session crash, git failure, LLM provider down)
- **FR23:** The daemon can create a PR for blocked/failed stories with partial code and a description of the failure
- **FR24:** When code review is disabled, the daemon proceeds directly to PR creation after the development session
- **NFR-REL1:** Transient LLM errors recovered with exponential backoff, max 3 retries per call
- **NFR-REL4:** All errors logged via `tracing::error!()` with full context

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [x] Verify `SessionRunner::run()` exists and returns `SessionOutcome` (Completed, Escalated, Failed) — `src/session/runner.rs`
- [x] Verify `ReviewRunner::run()` exists and returns `ReviewOutcome` (Completed, Failed, Skipped) — `src/review/mod.rs`
- [x] Verify `GitProvider` trait with `create_pr()`, `add_comment()`, `get_pr_url()` — `src/git_provider/mod.rs`
- [x] Verify `create_provider()` factory returns `Box<dyn GitProvider>` — `src/git_provider/mod.rs`
- [x] Verify `CreatePrParams`, `PrInfo`, `PrDescriptionParams` structs — `src/git_provider/mod.rs`
- [x] Verify `build_pr_description()` and `build_pr_title()` helpers — `src/git_provider/mod.rs`
- [x] Verify `format_pr_decisions_section()` — `src/supervisor/decisions.rs`
- [x] Verify `Notifier` trait with `notify_story()` and `notify_run_summary()` — `src/notifier/mod.rs` (Story 6.1)
- [x] Verify `StoryNotification`, `StoryStatus`, `RunSummary` — `src/notifier/mod.rs` (Story 6.1)
- [x] Verify `create_notifier()` factory — `src/notifier/mod.rs` (Story 6.1)
- [x] Verify `Watcher::poll()` returns eligible `StoryInfo` items — `src/watcher/mod.rs`
- [x] Verify `preserve_partial_work()` in `src/session/cleanup.rs`
- [x] Verify `build_http_client()` in `src/config/mod.rs` already provides 3-retry exponential backoff
- [x] Verify `run_polling_loop()` in `src/cli/mod.rs` has TODO placeholder for session launching
- [x] Verify `BotSecrets` loads all needed tokens (git provider, telegram)

### Task 1: Create Story Pipeline Module (`src/pipeline.rs`)

- [x] Create new file `src/pipeline.rs`
- [x] Add `mod pipeline;` to `src/main.rs`
- [x] Define `PipelineError` enum using `thiserror`:
  - [x] `Init { reason: String }` — pipeline construction failure (git provider init, notifier init) (renamed from `InitFailed` per clippy `enum_variant_names`)
  - [x] `Session { story_key: String, error: String }` — session returned `Failed`
  - [x] `Review { story_key: String, error: String }` — review returned `Failed`
  - [x] `PrCreation { story_key: String, branch: String, reason: String }` — git provider error
  - [x] `PrComment { pr_id: String, reason: String }` — comment posting failed (non-blocking)
  - [x] `Notification { reason: String }` — notification error (always non-blocking)
- [x] Define `PipelineResult` struct:
  - [x] `story_key: String`
  - [x] `status: StoryStatus` (re-use from notifier module)
  - [x] `pr_url: Option<String>`
  - [x] `error_detail: Option<String>`

### Task 2: Implement `StoryPipeline` Struct (`src/pipeline.rs`)

- [x] Define `StoryPipeline` struct:
  - [x] `config: Arc<BotConfig>`
  - [x] `git_provider: Box<dyn GitProvider>`
  - [x] `notifier: Box<dyn Notifier>`
  - [x] `session_runner: SessionRunner`
  - [x] `review_runner: ReviewRunner`
- [x] Implement `StoryPipeline::new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>) -> Result<Self, PipelineError>`
  - [x] Extract git provider token from secrets based on config:
    ```rust
    let token = match config.git_provider.provider.as_str() {
        "github" => secrets.github_token.as_deref().unwrap_or(""),
        "gitlab" => secrets.gitlab_token.as_deref().unwrap_or(""),
        other => return Err(PipelineError::InitFailed {
            reason: format!("Unsupported git provider: {other}"),
        }),
    };
    ```
  - [x] Create git provider via `create_provider(&config.git_provider, token)` — map error to `PipelineError::Init`
  - [x] Create notifier via `create_notifier(&config.notifications, &secrets)` — factory never fails, returns NoopNotifier as fallback
  - [x] Create `SessionRunner::new(config.clone(), secrets.clone())`
  - [x] Create `ReviewRunner::new(config.clone(), secrets.clone())`
  - [x] Store all components

### Task 3: Implement `process_story()` — Core Pipeline Method (`src/pipeline.rs`)

- [x] `pub async fn process_story(&self, story: &StoryInfo) -> PipelineResult`
- [x] This is the main orchestration method implementing the full pipeline for ONE story:

**Phase 1 — Dev Session:**
- [x] Call `self.session_runner.run(story).await`
- [x] Match on `SessionOutcome`:
  - [x] `Completed { story_key, branch, decisions }` → proceed to Phase 2
  - [x] `Escalated { report, decisions }` → notify human (escalation details), return result with `StoryStatus::Blocked`
  - [x] `Failed { story_key, error, decisions }` → proceed to Phase 3 (create failure PR)

**Phase 2 — Code Review (optional):**
- [x] Check `self.config.code_review_enabled`
- [x] If enabled: call `self.review_runner.run(story).await`
- [x] Match on `ReviewOutcome`:
  - [x] `Completed { report, .. }` → store report for PR comment, proceed to Phase 4
  - [x] `Failed { error, .. }` → log error, proceed to Phase 4 WITHOUT review (non-blocking)
  - [x] `Skipped { reason }` → log reason, proceed to Phase 4 WITHOUT review (non-blocking)
- [x] If disabled: proceed directly to Phase 4

**Phase 3 — Failure PR (only on session failure):**
- [x] Build PR title via `build_pr_title(&story.story_key, &story_title, true)` — produces `"wip({key}): {title} [NEEDS REVIEW]"`
- [x] Build decisions section via `format_pr_decisions_section(&decisions)`
- [x] Build PR body via `build_pr_description(&PrDescriptionParams { ..., outcome_summary: "failed", failure_details: Some(error_details) })`
- [x] Build `CreatePrParams` with title, body, source_branch, target_branch
- [x] Call `self.git_provider.create_pr(params).await`
- [x] If PR creation succeeds → store PR URL, notify human with failure + PR link
- [x] If PR creation also fails → log error, notify human with branch name only (AC2)
- [x] Return `PipelineResult` with `StoryStatus::Error`

**Phase 4 — Success PR:**
- [x] Build PR title via `build_pr_title(&story.story_key, &story_title, false)` — produces `"feat({key}): {title}"`
- [x] Build decisions section via `format_pr_decisions_section(&decisions)`
- [x] Build PR body via `build_pr_description(&PrDescriptionParams { ..., outcome_summary: "completed successfully", failure_details: None })`
- [x] Build `CreatePrParams` with title, body, source_branch, target_branch
- [x] Call `self.git_provider.create_pr(params).await`
- [x] If PR creation succeeds:
  - [x] If review report available → call `self.git_provider.add_comment(&pr_info.id, &report).await` (non-blocking: log error if fails)
  - [x] Notify human with success + PR link
  - [x] Return `PipelineResult` with `StoryStatus::Completed`
- [x] If PR creation fails (AC2):
  - [x] Log error with full context (HTTP status, response body, story_id)
  - [x] Notify human with failure details + branch name for manual PR
  - [x] Return `PipelineResult` with `StoryStatus::Error`

### Task 4: Implement `process_eligible_stories()` — Batch Orchestration (`src/pipeline.rs`)

- [x] `pub async fn process_eligible_stories(&self, stories: Vec<StoryInfo>) -> RunSummary`
- [x] Iterate stories sequentially IN THE ORDER received from watcher (dependency-sorted)
- [x] For each story: call `self.process_story(story).await`
- [x] Collect all `PipelineResult` into a `Vec`
- [x] Build `RunSummary` from results:
  - [x] Map each `PipelineResult` → `StoryNotification`
  - [x] Count completed, blocked, errored
- [x] After ALL stories processed: call `self.notifier.notify_run_summary(&summary).await` (non-blocking)
- [x] Return `RunSummary`

### Task 5: Implement Notification & Title Helpers (`src/pipeline.rs`)

- [x] `async fn notify_story_result(&self, result: &PipelineResult)`
  - [x] Build `StoryNotification` from `PipelineResult`
  - [x] Call `self.notifier.notify_story(&notification).await`
  - [x] Swallow errors: `if let Err(e) = ... { tracing::error!(action = "notification_failed", ...) }`
- [x] `fn story_title_from_label(label: &str) -> String`
  - [x] Convert kebab-case label to human-readable title: `"telegram-notifications"` → `"Telegram Notifications"`
  - [x] Split on `-`, capitalize first letter of each word, join with spaces
  - [x] This is needed because `StoryInfo.label` stores the kebab slug, but `PrDescriptionParams.story_title` and `build_pr_title()` expect a human-readable title

### Task 6: Wire Pipeline into Polling Loop (`src/cli/mod.rs`)

- [x] In `run_start()`: create `StoryPipeline` after config/secrets validation
  - [x] Wrap `BotSecrets` in `Arc<BotSecrets>` for sharing
  - [x] Pass `Arc<BotConfig>` and `Arc<BotSecrets>` to `StoryPipeline::new()`
  - [x] Handle `StoryPipeline::new()` failure with `CliError::Init` + return error
- [x] In `run_polling_loop()`: accept `&StoryPipeline` parameter
- [x] Replace the TODO block with actual story processing:
  - [x] Call `pipeline.process_eligible_stories(stories).await`
  - [x] Log run summary results
  - [x] Update daemon state with `record_story_processed()` per story
- [x] Ensure the daemon NEVER stops on a single story failure — always continues to next story (AC3)

### Task 7: Unit Tests

- [x] `test_pipeline_result_completed_fields` — verify PipelineResult construction for success
- [x] `test_pipeline_result_failed_fields` — verify PipelineResult construction for failure
- [x] `test_pipeline_result_blocked_fields` — verify PipelineResult for escalation
- [x] `test_pipeline_error_display_init` — verify Init error message
- [x] `test_pipeline_error_display_session` — verify error message
- [x] `test_pipeline_error_display_pr_creation` — verify branch included in message
- [x] `test_pipeline_error_display_notification` — verify error message
- [x] `test_pipeline_error_display_review` — verify review error message
- [x] `test_pipeline_error_display_pr_comment` — verify PR comment error message
- [x] `test_pipeline_error_is_send_sync` — verify PipelineError is Send + Sync
- [x] `test_story_pipeline_is_send_sync` — verify StoryPipeline is Send + Sync (critical for async context)
- [x] `test_story_title_from_label_simple` — `"telegram-notifications"` → `"Telegram Notifications"`
- [x] `test_story_title_from_label_single_word` — `"scaffolding"` → `"Scaffolding"`
- [x] `test_story_title_from_label_multi_word` — `"http-retry-error-resilience"` → `"Http Retry Error Resilience"`
- [x] `test_story_title_from_label_empty` — `""` → `""`
- [x] `test_run_summary_from_pipeline_results` — verify correct counting
- [x] `test_run_summary_all_completed` — verify all-success case
- [x] `test_run_summary_empty` — verify empty input handling
- [x] `test_run_summary_story_id_extraction` — verify `"6-1-..."` → `"6.1"` conversion
- [x] All tests use mocked data — NO real API calls, NO real sessions

### Task 8: Integration Verification

- [x] `cargo check` — 0 errors
- [x] `cargo test` — all existing + new tests pass, 0 regressions (525 total: 506 existing + 19 new)
- [x] `cargo clippy` — 0 new warnings (renamed enum variants per `enum_variant_names`, collapsed nested `if let` per `collapsible_if`)
- [x] `cargo fmt` — clean
- [x] All public items have `///` doc comments

## Dev Notes

### Previous Story Intelligence

**From Story 6.1 (Telegram Notifications) — immediate predecessor:**
- Defines `Notifier` trait, `TelegramNotifier`, `NoopNotifier`, `create_notifier()` factory
- `create_notifier()` signature: `pub fn create_notifier(config: &NotificationConfig, secrets: &BotSecrets) -> Box<dyn Notifier>`
- Defines `StoryNotification`, `StoryStatus` (Completed, Blocked, Error), `RunSummary`
- `NotifierError` uses `{ reason: String }` fields — no `#[from]` wrappers
- All notification failures are non-blocking — swallow errors with `tracing::error!()`

**From Story 5.3 (GitLab MR Support) — most recent implemented story:**
- Agent model: Claude Opus 4.6
- `GitProvider` trait: `create_pr()`, `add_comment()`, `get_pr_url()`
- `create_provider(config: &GitProviderConfig, token: &str)` — takes raw `&str` token, NOT `BotSecrets`
- `CreatePrParams { title, body, source_branch, target_branch }` → `PrInfo { id, url, number }`
- Error type uses `{ reason: String }` fields mapped via `.map_err()`
- 488 existing tests passed

**From Story 4.2 (Agent Session Setup & Chat Loop):**
- `SessionRunner::new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>)` — takes Arc-wrapped args
- `SessionRunner::run(story: &StoryInfo) -> SessionOutcome`
- `SessionOutcome::Completed { story_key, branch, decisions }`
- `SessionOutcome::Escalated { report: EscalationReport, decisions }`
- `SessionOutcome::Failed { story_key, error, decisions }`
- `preserve_partial_work()` already called by SessionRunner on failure
- `mark_story_needs_clarification()` already called by SessionRunner on escalation
- `decisions: Vec<DecisionRecord>` flows through all outcomes

**From Story 5.2 (Automated Code Review):**
- `ReviewRunner::new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>)` — takes Arc-wrapped args
- `ReviewRunner::run(story: &StoryInfo) -> ReviewOutcome`
- `ReviewOutcome::Completed { story_key, branch, report }` — report is markdown string for PR comment
- `ReviewOutcome::Failed { story_key, error }` — non-blocking, continue to PR
- `ReviewOutcome::Skipped { reason }` — non-blocking, continue to PR
- Review failures NEVER block PR creation

### Git Intelligence (Last 5 Commits)

1. `97b7c80` docs(stories): create story 6-1 telegram notifications and update sprint status
2. `cdc25c3` feat(git-provider): implement GitLabProvider with full GitProvider trait support
3. `dea1232` feat(review): implement automated code review session runner
4. `cf29058` feat(git-provider): implement GitProvider trait and GitHub PR creation
5. `deb6639` docs(stories): create story 5-3 GitLab merge request support

### Core Design — StoryPipeline Orchestrator

This story creates the **daemon orchestration layer** (Architecture Decision 4, Layer 3). It is NOT about HTTP retry itself — that already works via `build_http_client()` with reqwest-middleware (Layer 1). This story is about **what happens when retries are exhausted** and errors reach the daemon.

The `StoryPipeline` struct encapsulates the full story processing pipeline:

```
StoryPipeline
├── SessionRunner     → runs dev session, returns SessionOutcome
├── ReviewRunner      → runs code review (optional), returns ReviewOutcome
├── GitProvider       → creates PRs, posts comments
└── Notifier          → sends Telegram notifications
```

**Pipeline flow per story:**
```
poll() → eligible stories
  └── for each story (sequential, in watcher order):
      ├── session_runner.run(story) → SessionOutcome
      │   ├── Completed → [optional review] → create PR → notify ✅
      │   ├── Escalated → notify ⚠️ (story already marked needs-clarification)
      │   └── Failed → create failure PR → notify ❌
      ├── [if review enabled] review_runner.run(story) → ReviewOutcome
      │   ├── Completed → report stored for PR comment
      │   ├── Failed → logged, continue (non-blocking)
      │   └── Skipped → logged, continue (non-blocking)
      ├── git_provider.create_pr(params)  ← uses existing build_pr_title() + build_pr_description()
      │   ├── Ok(pr_info) → [post review comment] → notify with PR link
      │   └── Err(e) → log error → notify with branch name (manual PR)
      └── notifier.notify_story(notification)
          └── Err(e) → tracing::error!() only (non-blocking)
```

**After all stories:**
```
notifier.notify_run_summary(summary)  → non-blocking
```

### Error Handling Philosophy — Layer 3

The pipeline implements the **"never stop the run"** principle:

| Error Type | Response | Stops Story? | Stops Run? |
|---|---|---|---|
| Session failure (LLM down, tool crash) | Commit WIP, create failure PR, notify | ✅ Yes | ❌ No |
| Session escalation (needs human) | Notify with question context | ✅ Yes | ❌ No |
| Review failure | Log, skip review, continue to PR | ❌ No | ❌ No |
| PR creation failure | Log, notify with branch name | ✅ Yes (no PR) | ❌ No |
| PR comment failure | Log only | ❌ No | ❌ No |
| Notification failure | Log only | ❌ No | ❌ No |
| Pipeline construction failure | Fatal — daemon cannot start | N/A | ✅ Yes (startup) |

**Key invariant:** The polling loop ALWAYS moves to the next story. No single failure halts the daemon.

### Known Limitation: Graceful Shutdown During Processing

⚠️ When `process_eligible_stories()` is running (potentially hours for multiple stories), the `tokio::select!` in `run_polling_loop()` is inside the `sleep` arm handler — SIGTERM/SIGINT signals will NOT be caught until processing completes and the loop iterates.

**This is acceptable for MVP because:**
- `SessionRunner` already handles failure preservation internally (WIP commit on any error)
- Story 6.3 (Crash Recovery via WAL) handles daemon restart after unexpected termination
- The agent's chat loop persists state to WAL after each turn
- Future improvement: wrap `process_eligible_stories()` in `tokio::select!` with signal handlers for immediate graceful shutdown

### Architecture Compliance

| Constraint | Implementation |
|---|---|
| New module | `src/pipeline.rs` — daemon orchestration layer |
| Error handling | `PipelineError` enum via `thiserror` — no `anyhow` |
| Error field pattern | `{ reason: String }` — matches project convention |
| HTTP retry | Already handled by `build_http_client()` Layer 1 — NOT reimplemented here |
| Sequential execution | One story at a time, in watcher order (dependency-sorted). No parallelism |
| Non-blocking notifications | All `notifier` calls wrapped with error swallowing |
| Non-blocking review | Review failures logged, never block PR creation |
| PR for failures | Failed stories get `wip()` PR with partial work + error description (FR23) |
| PR title/body | Built via existing `build_pr_title()` + `build_pr_description()` — NOT reimplemented |
| Supervisor decisions in PR | Via existing `format_pr_decisions_section()` (FR22) |
| Logging | `tracing` only — structured fields with `story_id`, `action` |
| Doc comments | `///` on all public structs, traits, enums, functions |
| Tests | Inline `#[cfg(test)] mod tests` — mock data only |

### Existing Code to Reuse (DO NOT Reinvent)

| Component | Location | What to use |
|---|---|---|
| `SessionRunner` | `src/session/runner.rs` | `.new(Arc<BotConfig>, Arc<BotSecrets>)`, `.run(story) → SessionOutcome` |
| `SessionOutcome` | `src/session/mod.rs` | `Completed`, `Escalated`, `Failed` — all carry `decisions: Vec<DecisionRecord>` |
| `ReviewRunner` | `src/review/mod.rs` | `.new(Arc<BotConfig>, Arc<BotSecrets>)`, `.run(story) → ReviewOutcome` |
| `ReviewOutcome` | `src/review/mod.rs` | `Completed { report }`, `Failed`, `Skipped` |
| `preserve_partial_work()` | `src/session/cleanup.rs` | Already called internally by SessionRunner — do NOT call from pipeline |
| `mark_story_needs_clarification()` | `src/session/cleanup.rs` | Already called internally by SessionRunner — do NOT call from pipeline |
| `GitProvider` trait | `src/git_provider/mod.rs` | `create_pr()`, `add_comment()`, `get_pr_url()` |
| `create_provider()` | `src/git_provider/mod.rs` | `create_provider(config: &GitProviderConfig, token: &str) → Box<dyn GitProvider>` |
| `CreatePrParams` | `src/git_provider/mod.rs` | `{ title, body, source_branch, target_branch }` |
| `PrInfo` | `src/git_provider/mod.rs` | `{ id, url, number }` |
| `PrDescriptionParams` | `src/git_provider/mod.rs` | `{ story_key, story_title, outcome_summary, decisions_section, failure_details }` |
| `build_pr_description()` | `src/git_provider/mod.rs` L190 | Builds structured markdown PR body from `PrDescriptionParams` |
| `build_pr_title()` | `src/git_provider/mod.rs` L215 | `build_pr_title(key, title, is_failure)` → conventional commit title |
| `format_pr_decisions_section()` | `src/supervisor/decisions.rs` L339 | `format_pr_decisions_section(&[DecisionRecord]) → String` — markdown table |
| `DecisionRecord` | `src/supervisor/decisions.rs` | `{ question, context, answer, source, reasoning, alternatives, timestamp }` |
| `Notifier` trait | `src/notifier/mod.rs` | `notify_story()`, `notify_run_summary()` |
| `create_notifier()` | `src/notifier/mod.rs` | `create_notifier(config: &NotificationConfig, secrets: &BotSecrets) → Box<dyn Notifier>` |
| `StoryNotification` | `src/notifier/mod.rs` | `{ story_id, story_key, status, pr_url, reason }` |
| `RunSummary` | `src/notifier/mod.rs` | `{ stories, total_processed, completed, blocked, errored }` |
| `StoryStatus` | `src/notifier/mod.rs` | `Completed`, `Blocked`, `Error` |
| `Watcher::poll()` | `src/watcher/mod.rs` | Returns `Vec<StoryInfo>` (dependency-sorted) |
| `StoryInfo` | `src/watcher/mod.rs` | `{ story_id, story_key, epic_num, story_num, label, branch_name, specs_path, dependencies, status }` |
| `BotConfig` | `src/config/mod.rs` | `code_review_enabled`, `git_provider`, `notifications`, etc. |
| `BotSecrets` | `src/config/mod.rs` | `github_token`, `gitlab_token`, `telegram_bot_token` — all `Option<String>` |

⚠️ **Do NOT reimplement any of these.** Import and use them directly. In particular, do NOT create custom PR description or title formatting — use `build_pr_description()`, `build_pr_title()`, and `format_pr_decisions_section()`.

### StoryInfo.label → Story Title Mapping

`StoryInfo.label` stores the kebab-case slug (e.g., `"telegram-notifications"`), but `PrDescriptionParams.story_title` and `build_pr_title()` expect a human-readable title (e.g., `"Telegram Notifications"`).

Provide a simple helper:
```rust
fn story_title_from_label(label: &str) -> String {
    label
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
```

### Token Extraction for Git Provider — CRITICAL

`create_provider()` takes `&str` token, NOT `&BotSecrets`. The pipeline must extract the correct token:

```rust
let token = match config.git_provider.provider.as_str() {
    "github" => secrets.github_token.as_deref().unwrap_or(""),
    "gitlab" => secrets.gitlab_token.as_deref().unwrap_or(""),
    other => return Err(PipelineError::InitFailed {
        reason: format!("Unsupported git provider: {other}"),
    }),
};
let git_provider = create_provider(&config.git_provider, token)
    .map_err(|e| PipelineError::InitFailed { reason: e.to_string() })?;
```

Note: `BotSecrets::validate_for_config()` already ensures the required token is present at startup, so `unwrap_or("")` is a defensive fallback that should never trigger in practice.

### Wiring Changes to `src/cli/mod.rs`

The `run_start()` function currently creates `Arc<BotConfig>` and `Watcher`. It must additionally:

1. Wrap `BotSecrets` in `Arc<BotSecrets>`
2. Create `StoryPipeline::new(config.clone(), secrets.clone())`
3. Pass `&pipeline` to `run_polling_loop()`

The `run_polling_loop()` signature changes from:
```rust
async fn run_polling_loop(
    config: &Arc<BotConfig>,
    watcher: &Watcher,
    daemon_state: &mut DaemonState,
    state_path: &Path,
) -> Result<(), CliError>
```

To:
```rust
async fn run_polling_loop(
    config: &Arc<BotConfig>,
    watcher: &Watcher,
    pipeline: &StoryPipeline,
    daemon_state: &mut DaemonState,
    state_path: &Path,
) -> Result<(), CliError>
```

The TODO block inside the `Ok(stories)` match arm:
```rust
Ok(stories) => {
    tracing::info!(
        eligible_count = stories.len(),
        "Found eligible stories — session launching not yet implemented (Epic 4)"
    );
    // TODO: Epic 4 — Launch dev session for first eligible story
}
```

Becomes:
```rust
Ok(stories) => {
    tracing::info!(
        eligible_count = stories.len(),
        "Found eligible stories — launching pipeline"
    );
    let summary = pipeline.process_eligible_stories(stories).await;
    tracing::info!(
        total = summary.total_processed,
        completed = summary.completed,
        blocked = summary.blocked,
        errored = summary.errored,
        "Pipeline run complete"
    );
}
```

### Library & Framework Requirements

| Dependency | Version | Purpose | Already in Cargo.toml |
|---|---|---|---|
| `std::sync::Arc` | stdlib | Shared config/secrets | ✅ Yes |
| `thiserror` | 2 | Typed error enums | ✅ Yes |
| `tracing` | 0.1 | Structured logging | ✅ Yes |
| `async-trait` | 0.1 | Async trait methods (if needed) | ✅ Yes |

**No new dependencies needed.** Everything is already available.

### File Structure Requirements

**Files to create:**
- `src/pipeline.rs` — **NEW** — Full `StoryPipeline` implementation + `PipelineError` + `PipelineResult` + tests

**Files to modify:**
- `src/main.rs` — **MODIFY** — Add `mod pipeline;` declaration
- `src/cli/mod.rs` — **MODIFY** — Wire `StoryPipeline` into `run_start()` and `run_polling_loop()`

**Files NOT to touch:**
- `src/session/` — Already handles failure preservation internally
- `src/review/` — Already handles failure gracefully (returns Skipped/Failed)
- `src/git_provider/` — Used as-is via trait + helper functions
- `src/supervisor/` — Used as-is via `format_pr_decisions_section()` + `DecisionRecord`
- `src/notifier/` — Used as-is via trait (Story 6.1)
- `src/config/` — Config/secrets already have everything needed
- `src/watcher/` — Used as-is via `poll()`
- `Cargo.toml` — No new dependencies
- Anything under `_bmad/` — Read-only, sacred

### Testing Requirements

All tests inline in `#[cfg(test)] mod tests` at the bottom of `src/pipeline.rs`:
- Use `#[test]` for synchronous tests (PipelineError display, PipelineResult construction)
- Use `#[tokio::test]` for async tests if needed
- Naming convention: `test_{module}_{behavior}_{scenario}` in snake_case
- Structure: Arrange → Act → Assert
- Mock all external dependencies — NO real API calls, NO real LLM sessions
- `story_title_from_label()` tests are pure functions — no mocking needed
- `process_story()` and `process_eligible_stories()` are hard to unit test without mocking SessionRunner/ReviewRunner — focus tests on:
  - Error types and display
  - PipelineResult construction
  - `story_title_from_label()` conversion
  - RunSummary building from PipelineResults
  - Send+Sync assertions for both `PipelineError` and `StoryPipeline`

### Anti-Patterns to Avoid

- ❌ Do NOT reimplement HTTP retry logic — `build_http_client()` already handles Layer 1
- ❌ Do NOT reimplement PR description/title formatting — use `build_pr_description()`, `build_pr_title()`, `format_pr_decisions_section()` from existing code
- ❌ Do NOT call `preserve_partial_work()` from the pipeline — `SessionRunner` already does this internally
- ❌ Do NOT call `mark_story_needs_clarification()` from the pipeline — `SessionRunner` already does this on escalation
- ❌ Do NOT propagate notification errors — always swallow with `tracing::error!()`
- ❌ Do NOT propagate review failures — always continue to PR creation
- ❌ Do NOT stop the daemon run on any single story failure — always move to next story
- ❌ Do NOT use `unwrap()` or `expect()` in production code
- ❌ Do NOT use `println!` or `eprintln!` — use `tracing` only
- ❌ Do NOT use `anyhow` in `pipeline.rs` — `thiserror` only
- ❌ Do NOT add new dependencies — everything needed is already available
- ❌ Do NOT make `StoryPipeline` generic over its components for now — concrete types with `Box<dyn>` for trait objects is fine for MVP

### Scope Boundaries

**In scope:**
- `StoryPipeline` orchestrator struct with `process_story()` and `process_eligible_stories()`
- `PipelineError` typed error enum (with `InitFailed` variant)
- `PipelineResult` struct for per-story outcomes
- `story_title_from_label()` helper for kebab-to-title conversion
- `notify_story_result()` helper with error swallowing
- Wiring into `run_polling_loop()` replacing the TODO placeholder
- Per-story notifications and run summary notification
- Unit tests for error types, result construction, title conversion, and Send+Sync

**Out of scope:**
- HTTP retry logic (already exists in Layer 1 via `build_http_client()`)
- PR description/title formatting (already exists in `git_provider/mod.rs` and `supervisor/decisions.rs`)
- Session failure handling internals (already in `SessionRunner`)
- Review failure handling internals (already in `ReviewRunner`)
- Notifier implementation (Story 6.1)
- Git provider implementation (Story 5.1/5.3)
- Crash recovery via WAL (Story 6.3)
- Context window limit recovery (Story 6.4)
- Full graceful shutdown during processing (known limitation, see section above)
- Sprint-status updates on completion/failure (daemon is read-only per Decision 2 — agent handles mutations)

### Project Structure Notes

After this story, the project gains a new module:
```
src/
├── pipeline.rs         # NEW — StoryPipeline orchestrator
├── main.rs             # MODIFIED — add mod pipeline
├── cli/
│   └── mod.rs          # MODIFIED — wire pipeline into polling loop
├── session/            # UNCHANGED — used via SessionRunner
├── review/             # UNCHANGED — used via ReviewRunner
├── git_provider/       # UNCHANGED — used via GitProvider trait + build_pr_*() helpers
├── notifier/           # UNCHANGED — used via Notifier trait (Story 6.1)
├── watcher/            # UNCHANGED — used via Watcher::poll()
├── config/             # UNCHANGED — provides Arc<BotConfig>, Arc<BotSecrets>
├── supervisor/         # UNCHANGED — DecisionRecord + format_pr_decisions_section()
└── tools/              # UNCHANGED
```

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Story 6.2 (L722-L745), FR23/FR24/FR33/FR35]
- [Source: _bmad-output/planning-artifacts/architecture.md — Decision 4: Error Propagation Layered (L261-L284), Data Flow (L660-L673)]
- [Source: _bmad-output/project-context.md — Resilience Rules, Sequential Execution, No Silent Failures]
- [Source: src/session/mod.rs — SessionOutcome enum, SessionError enum]
- [Source: src/session/runner.rs — SessionRunner::new() (L63-L72), ::run() (L80-L300)]
- [Source: src/session/cleanup.rs — preserve_partial_work(), mark_story_needs_clarification()]
- [Source: src/review/mod.rs — ReviewRunner::new() (L151-L157), ::run() (L163-L178), ReviewOutcome enum]
- [Source: src/git_provider/mod.rs — GitProvider trait (L124-L133), create_provider() (L150-L167), PrDescriptionParams (L172-L184), build_pr_description() (L190-L210), build_pr_title() (L215-L221)]
- [Source: src/supervisor/decisions.rs — DecisionRecord (L65-L82), format_pr_decisions_section() (L339+)]
- [Source: src/cli/mod.rs — run_start() (L944-L1001), run_polling_loop() with TODO (L1011-L1076)]
- [Source: src/watcher/mod.rs — StoryInfo (L66-L86), Watcher::poll()]
- [Source: _bmad-output/implementation-artifacts/6-1-telegram-notifications.md — Notifier trait, create_notifier(), StoryNotification, RunSummary, StoryStatus]

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — clean implementation, one compilation fix (`record_story_processed` takes no args).

### Completion Notes List

- **Task 0:** All 15 prerequisites verified — SessionRunner, ReviewRunner, GitProvider, Notifier, Watcher, helper functions, config structs all confirmed present and matching expected signatures.
- **Task 1:** Created `src/pipeline.rs` with `PipelineError` enum (6 variants: `Init`, `Session`, `Review`, `PrCreation`, `PrComment`, `Notification`) and `PipelineResult` struct. Added `mod pipeline;` to `src/main.rs`. Variant names shortened from `*Failed` suffix per clippy `enum_variant_names` lint.
- **Task 2:** `StoryPipeline` struct with `config`, `git_provider`, `notifier`, `session_runner`, `review_runner`. Constructor extracts git token from `BotSecrets` based on provider config, creates all components. `secrets` field not stored on struct (only needed during construction).
- **Task 3:** `process_story()` implements full 4-phase pipeline: (1) dev session, (2) optional code review, (3) failure PR on session failure, (4) success PR on completion. All error paths handled with logging and notification. PR comment posting is non-blocking.
- **Task 4:** `process_eligible_stories()` iterates stories sequentially, collects results, builds `RunSummary`, sends summary notification (non-blocking).
- **Task 5:** `notify_story_result()` builds `StoryNotification` and swallows errors. `story_title_from_label()` converts kebab-case to Title Case. `build_run_summary()` helper maps `PipelineResult` vec to `RunSummary` with correct counts.
- **Task 6:** Wired pipeline into `src/cli/mod.rs`: `run_start()` wraps `BotSecrets` in `Arc`, creates `StoryPipeline`, passes to `run_polling_loop()`. `run_polling_loop()` now accepts `&StoryPipeline` parameter. Replaced TODO block with `pipeline.process_eligible_stories(stories).await` + summary logging + daemon state updates via `record_story_processed()`.
- **Task 7:** 19 unit tests covering: `PipelineResult` construction (completed/failed/blocked), `PipelineError` display (all 6 variants), `Send+Sync` for both `PipelineError` and `StoryPipeline`, `story_title_from_label` (simple/single/multi/empty), `build_run_summary` (mixed/all-completed/empty/story-id extraction). All mock data, no real API calls.
- **Task 8:** `cargo check` 0 errors, `cargo test` 525 passed (506 existing + 19 new, 0 regressions), `cargo clippy` 0 new errors, `cargo fmt` clean, all public items have `///` doc comments.
- **Clippy fixes:** Renamed enum variants to drop common `Failed` suffix. Collapsed nested `if let Some` + `if let Err` using `let chains` syntax.

### Change Log

- 2026-02-08: Implemented Story 6.2 — HTTP Retry & Error Resilience / StoryPipeline orchestrator (all 9 tasks complete, 19 new tests, 525 total passing)

### File List

- `src/pipeline.rs` — **NEW** — StoryPipeline orchestrator, PipelineError, PipelineResult, process_story(), process_eligible_stories(), story_title_from_label(), build_run_summary(), notify_story_result(), 19 unit tests
- `src/main.rs` — **MODIFIED** — Added `mod pipeline;` declaration
- `src/cli/mod.rs` — **MODIFIED** — Wired StoryPipeline into run_start() and run_polling_loop(), replaced TODO with pipeline execution