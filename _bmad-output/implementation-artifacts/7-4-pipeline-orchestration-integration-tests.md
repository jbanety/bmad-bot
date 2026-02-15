# Story 7.4: Pipeline Orchestration Integration Tests

Status: review

## Story

As a developer,
I want integration tests that verify the full `StoryPipeline.process_story()` flow with mocked dependencies,
So that I'm confident the orchestration logic correctly chains session → PR → review → notification.

## Acceptance Criteria

1. **Given** a `StoryPipeline` constructed with:
   - MockDevRunner returning `SessionOutcome::Completed`
   - MockCodeReviewer returning `ReviewOutcome::Completed { report: "LGTM" }`
   - MockGitProvider returning `Ok(PrInfo { id: "42", url: "https://...", number: 42 })`
   - MockNotifier capturing notifications
   **When** `process_story()` is called with a valid `StoryInfo`
   **Then** the pipeline returns `PipelineResult` with `status: Completed` and `pr_url: Some("https://...")`
   **And** MockGitProvider received a `create_pr` call **before** MockCodeReviewer was called
   **And** MockGitProvider received a `create_pr` call with a title matching `feat({story_key}): ...`
   **And** MockGitProvider received an `add_comment` call with the review report as body
   **And** MockNotifier captured exactly one story notification with the correct story key and PR link

2. **Given** the same setup but MockDevRunner returns `SessionOutcome::Failed { error: "LLM timeout" }`
   **When** `process_story()` is called
   **Then** the pipeline returns `PipelineResult` with `status: Error` and `error_detail` containing "LLM timeout"
   **And** a PR is still created (partial work PR) with title containing `[NEEDS REVIEW]`
   **And** MockNotifier captured a notification with `StoryStatus::Error`

3. **Given** the same setup but MockDevRunner returns `SessionOutcome::Escalated`
   **When** `process_story()` is called
   **Then** the pipeline returns `PipelineResult` with `status: Blocked`
   **And** NO PR is created (`create_pr` not called — escalation skips PR in current code)
   **And** MockNotifier captured a notification with `StoryStatus::Blocked`

4. **Given** a `StoryPipeline` with `code_review_enabled: false` in config
   **When** `process_story()` is called and session succeeds
   **Then** PR is created immediately after push (no review step)
   **And** MockCodeReviewer is NOT called (review skipped)
   **And** MockGitProvider does NOT receive `add_comment` (no review report to post)
   **And** the pipeline result is still `Completed`

5. **Given** a `StoryPipeline` where MockGitProvider's `create_pr` returns an error
   **When** `process_story()` is called and session succeeds
   **Then** the pipeline returns `PipelineResult` with `pr_url: None` and an error detail about PR creation failure
   **And** MockCodeReviewer is NOT called (no PR means no point running review)
   **And** MockNotifier still receives a notification (notification is best-effort, never blocks)

6. **Given** a `StoryPipeline` where MockCodeReviewer returns `ReviewOutcome::Failed`
   **When** `process_story()` is called and session succeeds
   **Then** the PR already exists (created before review ran)
   **And** MockGitProvider does NOT receive `add_comment` (no review report to post)
   **And** the pipeline result is `Completed`

7. **Given** a `StoryPipeline` where MockNotifier returns `Err(NotifierError::...)`
   **When** `process_story()` is called and session succeeds
   **Then** the pipeline still returns `PipelineResult` with `status: Completed`
   **And** the notification failure does not affect the result

## Tasks / Subtasks

- [x] Task 0: Refactor `StoryPipeline` for dependency injection (AC: all)
  - [x] 0.1 Add `use async_trait::async_trait;` import to `src/pipeline.rs`
  - [x] 0.2 Define `DevRunner` async trait in `src/pipeline.rs` with method `async fn run_dev_session(&self, story: &StoryInfo) -> SessionOutcome`
  - [x] 0.3 Define `CodeReviewer` async trait in `src/pipeline.rs` with method `async fn run_review(&self, story: &StoryInfo) -> ReviewOutcome`
  - [x] 0.4 Implement `DevRunner` for `SessionRunner` — delegates to `SessionRunner::run()`
  - [x] 0.5 Implement `CodeReviewer` for `ReviewRunner` — delegates to `ReviewRunner::run()`
  - [x] 0.6 Change `StoryPipeline` struct: replace `session_runner: SessionRunner` with `dev_runner: Box<dyn DevRunner>`, replace `review_runner: ReviewRunner` with `code_reviewer: Box<dyn CodeReviewer>`, add `session_runner_for_recovery: Option<SessionRunner>`
  - [x] 0.7 Update `StoryPipeline::new()` to set all three fields (dev_runner wraps SessionRunner, code_reviewer wraps ReviewRunner, session_runner_for_recovery stores the concrete SessionRunner)
  - [x] 0.8 Add `StoryPipeline::new_with_components()` public constructor — takes `Box<dyn GitProvider>`, `Box<dyn Notifier>`, `Box<dyn DevRunner>`, `Box<dyn CodeReviewer>` — sets `session_runner_for_recovery: None`
  - [x] 0.9 Update all `self.session_runner.run(story)` → `self.dev_runner.run_dev_session(story)` (2 call sites: `process_story()` L182, `process_recovered_session()` is NOT changed — it doesn't call run)
  - [x] 0.10 Update all `self.review_runner.run(story)` → `self.code_reviewer.run_review(story)` (2 call sites: `process_story()` L192, `process_recovered_session()` L471)
  - [x] 0.11 Update `recover_and_process()` to use `self.session_runner_for_recovery.as_ref()?` for `check_and_recover_wal()` and `resume_session()` — returns `None` when recovery unavailable
  - [x] 0.12 Verify `cargo build` succeeds and all existing unit tests pass with `cargo test`

- [x] Task 1: Create `MockDevRunner` and `MockCodeReviewer` in test helpers (AC: all)
  - [x] 1.1 Add `MockDevRunner` implementing `DevRunner` — uses `Mutex<VecDeque<SessionOutcome>>` to support multiple sequential calls
  - [x] 1.2 Add `MockCodeReviewer` implementing `CodeReviewer` — uses `Mutex<VecDeque<ReviewOutcome>>` plus `AtomicUsize` call counter
  - [x] 1.3 Add `MockDevRunner::with_outcome(outcome)` (single call) and `MockDevRunner::with_outcomes(vec)` (multi-call) builders
  - [x] 1.4 Add `MockCodeReviewer::with_outcome(outcome)`, `MockCodeReviewer::never_called()`, and `MockCodeReviewer::call_count()` methods

- [x] Task 2: Create pipeline fixture helper (AC: all)
  - [x] 2.1 Add `PipelineTestBuilder` in `tests/integration/helpers/fixtures.rs`
  - [x] 2.2 `build()` returns `(StoryPipeline, MockNotifierHandle, MockGitProviderHandle)` where handles provide assertion access via shared `Arc<Mutex<Vec<...>>>` internals

- [x] Task 3: Create integration test file `tests/integration/test_pipeline.rs` (AC: #1–#7)
  - [x] 3.1 Add `mod test_pipeline;` declaration in `tests/integration.rs`

- [x] Task 4: Write happy-path test (AC: #1)
  - [x] 4.1 Build pipeline → call `process_story()` → assert Completed, pr_url, no error
  - [x] 4.2 Assert MockNotifier: 1 notification, correct story_key, story_id = "4.1", pr_url present
  - [x] 4.3 Assert MockGitProvider: `create_pr` title starts with `feat(`, `add_comment` body contains "LGTM"

- [x] Task 5: Write session-failure test (AC: #2)
  - [x] 5.1 MockDevRunner returns `Failed { error: "LLM timeout" }` → assert Error, error_detail contains "LLM timeout"
  - [x] 5.2 Assert MockGitProvider: `create_pr` title contains `[NEEDS REVIEW]`
  - [x] 5.3 Assert MockNotifier: notification with `StoryStatus::Error`

- [x] Task 6: Write escalation test (AC: #3)
  - [x] 6.1 MockDevRunner returns `Escalated` → assert Blocked, pr_url is Some (actual code creates PR), error_detail contains "Escalated"
  - [x] 6.2 Assert MockGitProvider: `create_pr` called (call count == 1) — actual code creates escalation PR
  - [x] 6.3 Assert MockNotifier: notification with `StoryStatus::Blocked`

- [x] Task 7: Write review-disabled test (AC: #4)
  - [x] 7.1 Config with `code_review_enabled: false`, MockDevRunner returns `Completed`
  - [x] 7.2 Assert Completed, MockCodeReviewer call_count == 0, MockGitProvider `add_comment` NOT called

- [x] Task 8: Write PR-creation-failure test (AC: #5)
  - [x] 8.1 MockGitProvider returns `Err` for `create_pr` → assert pr_url None, status Error
  - [x] 8.2 Assert MockNotifier still captured 1 notification

- [x] Task 9: Write review-failure-continues test (AC: #6)
  - [x] 9.1 MockCodeReviewer returns `ReviewOutcome::Failed` → assert pipeline still Completed
  - [x] 9.2 Assert MockGitProvider: `create_pr` called, `add_comment` NOT called (no report)

- [x] Task 10: Write notification-failure-non-blocking test (AC: #7)
  - [x] 10.1 MockNotifier returns `Err(NotifierError::HttpRequest { ... })` → assert pipeline still Completed with pr_url

- [x] Task 11: Write `process_eligible_stories` batch test (supplementary)
  - [x] 11.1 MockDevRunner with 3 outcomes via `with_outcomes()`, create 3 `StoryInfo` objects
  - [x] 11.2 Call `process_eligible_stories(stories)` → assert `RunSummary` totals
  - [x] 11.3 Assert MockNotifier captured 3 `notify_story` calls + 1 `notify_run_summary` call

## Dev Notes

### Architecture Compliance

#### 🚨 CRITICAL — Dependency Injection Refactor (Task 0)

**Problem:** `StoryPipeline::new()` internally creates `SessionRunner` and `ReviewRunner` as concrete types. No way to inject mocks.

**Solution:** Define two async traits, implement them for the real types, change `StoryPipeline` to store trait objects.

**Step 1 — Add import to `src/pipeline.rs`:**

```rust
use async_trait::async_trait;  // ADD THIS — not currently imported
```

**Step 2 — Define traits in `src/pipeline.rs`:**

```rust
/// Trait abstraction for dev session execution.
#[async_trait]
pub trait DevRunner: Send + Sync {
    /// Execute a development session for the given story.
    async fn run_dev_session(&self, story: &StoryInfo) -> SessionOutcome;
}

/// Trait abstraction for code review execution.
#[async_trait]
pub trait CodeReviewer: Send + Sync {
    /// Execute a code review for the given story.
    async fn run_review(&self, story: &StoryInfo) -> ReviewOutcome;
}
```

Method names are `run_dev_session`/`run_review` (not `run`) to avoid ambiguity with the concrete types' `run()` methods.

**Step 3 — Implement for real types:**

```rust
#[async_trait]
impl DevRunner for SessionRunner {
    async fn run_dev_session(&self, story: &StoryInfo) -> SessionOutcome {
        self.run(story).await
    }
}

#[async_trait]
impl CodeReviewer for ReviewRunner {
    async fn run_review(&self, story: &StoryInfo) -> ReviewOutcome {
        self.run(story).await
    }
}
```

**Step 4 — Change `StoryPipeline` struct:**

```rust
pub struct StoryPipeline {
    config: Arc<BotConfig>,
    git_provider: Box<dyn GitProvider>,
    notifier: Box<dyn Notifier>,
    dev_runner: Box<dyn DevRunner>,
    code_reviewer: Box<dyn CodeReviewer>,
    /// Concrete session runner for WAL recovery (check_and_recover_wal + resume_session).
    /// Set by new(), None in new_with_components(). Recovery returns None when absent.
    session_runner_for_recovery: Option<SessionRunner>,
}
```

**Step 5 — Update `new()` (preserves existing API):**

```rust
pub fn new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>) -> Result<Self, PipelineError> {
    // ... existing git_provider, notifier factory code unchanged ...

    let session_runner = SessionRunner::new(Arc::clone(&config), Arc::clone(&secrets));
    let review_runner = ReviewRunner::new(Arc::clone(&config), Arc::clone(&secrets));

    Ok(Self {
        config,
        git_provider,
        notifier,
        dev_runner: Box::new(session_runner),       // wraps via DevRunner impl
        code_reviewer: Box::new(review_runner),      // wraps via CodeReviewer impl
        session_runner_for_recovery: Some(SessionRunner::new(Arc::clone(&config), Arc::clone(&secrets))),
    })
}
```

Note: Two `SessionRunner` instances are created — one as `Box<dyn DevRunner>` (consumed by `process_story`), one concrete (consumed by `recover_and_process`). This avoids any downcast gymnastics. Both are cheap to construct (no state, no network).

**Step 6 — Add injectable constructor:**

```rust
/// Construct a pipeline with pre-built dependencies (for integration tests).
pub fn new_with_components(
    config: Arc<BotConfig>,
    git_provider: Box<dyn GitProvider>,
    notifier: Box<dyn Notifier>,
    dev_runner: Box<dyn DevRunner>,
    code_reviewer: Box<dyn CodeReviewer>,
) -> Self {
    Self {
        config,
        git_provider,
        notifier,
        dev_runner,
        code_reviewer,
        session_runner_for_recovery: None,
    }
}
```

**Step 7 — Update call sites (exactly 4 changes):**

| Location | Before | After |
|----------|--------|-------|
| `process_story()` ~L182 | `self.session_runner.run(story).await` | `self.dev_runner.run_dev_session(story).await` |
| `process_story()` ~L192 | `self.review_runner.run(story).await` | `self.code_reviewer.run_review(story).await` |
| `process_recovered_session()` ~L471 | `self.review_runner.run(story).await` | `self.code_reviewer.run_review(story).await` |
| `recover_and_process()` ~L430 | `self.session_runner.check_and_recover_wal()` + `self.session_runner.resume_session()` | `self.session_runner_for_recovery.as_ref()?.check_and_recover_wal()` + `.resume_session()` |

For `recover_and_process()`, the refactored version:

```rust
pub async fn recover_and_process(&self) -> Option<PipelineResult> {
    let runner = self.session_runner_for_recovery.as_ref()?;
    let recovery = runner.check_and_recover_wal().await?;
    // ... rest unchanged, using `runner.resume_session(recovery).await` ...
}
```

When `session_runner_for_recovery` is `None` (test builds), `recover_and_process()` returns `None` — safe no-op.

**Step 8 — Verify:** `cargo build` + `cargo test` must pass with zero regressions. The existing unit test `test_story_pipeline_is_send_sync` validates that the new trait objects maintain `Send + Sync`.

#### Integration Test Location
- All tests: `tests/integration/test_pipeline.rs`
- Declared as `mod test_pipeline;` in `tests/integration.rs`
- Run via `cargo test --test integration`

#### 🚨 Prerequisite: `src/lib.rs` (from Story 7.1 Task 0)
Without `lib.rs`, `use bmad_bot::pipeline::StoryPipeline;` won't compile. Verify `src/lib.rs` exists with `pub mod pipeline;`. If Story 7.1 not implemented, its Task 0 MUST be done first.

### Technical Requirements

#### Key Type Signatures (exact from codebase)

**`SessionOutcome`** (`src/session/mod.rs`) — `#[derive(Debug)]`, does NOT derive `Clone`:
```rust
pub enum SessionOutcome {
    Completed { story_key: String, branch: String, decisions: Vec<DecisionRecord> },
    Escalated { report: EscalationReport, decisions: Vec<DecisionRecord> },
    Failed { story_key: String, error: String, decisions: Vec<DecisionRecord> },
}
```

**`ReviewOutcome`** (`src/review/mod.rs`):
```rust
pub enum ReviewOutcome {
    Completed { story_key: String, branch: String, report: String },
    Failed { story_key: String, error: String },
    Skipped { reason: String },
}
```

**`EscalationReport`** (`src/session/escalation.rs`) — **6 fields, all required:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EscalationReport {
    pub story_key: String,
    pub question: String,
    pub reason: String,
    pub branch_name: String,
    pub partial_work_summary: String,
    pub escalated_at: String,           // ← ISO 8601 timestamp, MUST be provided
}
```

**`DecisionRecord`** (`src/supervisor/decisions.rs`) — **7 fields:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub question: String,
    pub context: Option<String>,
    pub answer: String,
    pub source: DecisionSource,         // enum: RuleEngine{rule_name}, LlmFallback, Escalation
    pub reasoning: String,
    pub alternatives: Vec<String>,
    pub timestamp: String,              // ← ISO 8601 timestamp
}
```

**`PipelineResult`** (`src/pipeline.rs`):
```rust
#[derive(Debug, Clone)]
pub struct PipelineResult {
    pub story_key: String,
    pub status: StoryStatus,
    pub pr_url: Option<String>,
    pub error_detail: Option<String>,
}
```

**`StoryInfo`** (`src/watcher/mod.rs`):
```rust
pub struct StoryInfo {
    pub story_id: String,
    pub story_key: String,
    pub epic_num: u32,
    pub story_num: u32,
    pub label: String,
    pub branch_name: String,
    pub specs_path: PathBuf,
    pub dependencies: Vec<String>,
    pub status: String,
}
```

**`StoryStatus`** (`src/notifier/mod.rs`) — derives `PartialEq`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoryStatus { Completed, Blocked, Error }
```

**`StoryNotification`** (`src/notifier/mod.rs`):
```rust
pub struct StoryNotification {
    pub story_id: String,
    pub story_key: String,
    pub status: StoryStatus,
    pub pr_url: Option<String>,
    pub reason: Option<String>,
}
```

**`RunSummary`** (`src/notifier/mod.rs`):
```rust
pub struct RunSummary {
    pub stories: Vec<StoryNotification>,
    pub total_processed: usize,
    pub completed: usize,
    pub blocked: usize,
    pub errored: usize,
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

**`CreatePrParams`** and **`PrInfo`** (`src/git_provider/mod.rs`):
```rust
pub struct CreatePrParams { pub title: String, pub body: String, pub source_branch: String, pub target_branch: String }
pub struct PrInfo { pub id: String, pub url: String, pub number: u64 }
```

**`Notifier` trait** (`src/notifier/mod.rs`):
```rust
#[async_trait]
pub trait Notifier: Send + Sync {
    async fn notify_story(&self, notification: &StoryNotification) -> Result<(), NotifierError>;
    async fn notify_run_summary(&self, summary: &RunSummary) -> Result<(), NotifierError>;
}
```

**PR title builders** (`src/git_provider/mod.rs`):
```rust
pub fn build_pr_title(story_key: &str, story_title: &str, is_failure: bool) -> String {
    if is_failure { format!("wip({story_key}): {story_title} [NEEDS REVIEW]") }
    else { format!("feat({story_key}): {story_title}") }
}
```

#### `process_story()` Flow — Exact Behavior

1. Calls `self.dev_runner.run_dev_session(story)` → `SessionOutcome`
2. **If `Completed`:**
   a. Pushes story branch to remote via `push_branch()`
   b. Builds PR title: `"feat({story_key}): {title}"`
   c. Calls `self.git_provider.create_pr(params)`
   d. If PR creation fails → returns `{ status: Error, pr_url: None, error_detail: "PR creation failed: ..." }` (review is skipped)
   e. If `config.code_review_enabled` → calls `self.code_reviewer.run_review(story)`
      - `ReviewOutcome::Completed { report }` → stores report, pushes review fix commits via second `push_branch()`
      - `ReviewOutcome::Failed` → logs warning, no report (PR already exists)
      - `ReviewOutcome::Skipped` → logs info, no report (PR already exists)
   f. If review report present → calls `self.git_provider.add_comment(pr_id, report)` (failure logged, non-blocking)
   g. Calls `self.notifier.notify_story()` (failure logged, non-blocking)
   h. Returns `PipelineResult { status: Completed, pr_url: Some(url) }`
3. **If `Escalated`:**
   a. Does **NOT** create a PR (no `create_pr` call)
   b. Calls `self.notifier.notify_story()` with `StoryStatus::Blocked`
   c. Returns `{ status: Blocked, pr_url: None, error_detail: "Escalated: {question} — {reason}" }`
4. **If `Failed`:**
   a. Pushes partial work branch via `push_branch()`
   b. Builds failure PR title: `"wip({story_key}): {title} [NEEDS REVIEW]"`
   c. Calls `self.git_provider.create_pr()` (partial work PR)
   d. Notifies with `StoryStatus::Error`
   e. Returns `{ status: Error, pr_url: Some(url) or None, error_detail: Some(error) }`

#### `story_id` Extraction Logic

The pipeline extracts `story_id` for notifications from `story_key`:
```rust
story_key.split('-').take(2).collect::<Vec<_>>().join(".")
```
Example: `"4-1-rig-tools"` → `"4.1"`. Assert this in notification tests.

#### `process_eligible_stories()` Notification Behavior

After processing all stories, calls `self.notifier.notify_run_summary(&summary)`. So for 3 stories, `MockNotifier` should capture:
- 3 × `notify_story()` calls (one per story, during `process_story`)
- 1 × `notify_run_summary()` call (after all stories)

The mock MUST track both methods separately.

#### MockDevRunner — Multi-Call Support via VecDeque

`SessionOutcome` does NOT derive `Clone`. Use `VecDeque` to support `process_eligible_stories()`:

```rust
use std::collections::VecDeque;
use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};

pub struct MockDevRunner {
    outcomes: Mutex<VecDeque<SessionOutcome>>,
    call_count: AtomicUsize,
}

impl MockDevRunner {
    /// Single-call mock.
    pub fn with_outcome(outcome: SessionOutcome) -> Self {
        let mut q = VecDeque::new();
        q.push_back(outcome);
        Self { outcomes: Mutex::new(q), call_count: AtomicUsize::new(0) }
    }

    /// Multi-call mock — pops outcomes in order per call.
    pub fn with_outcomes(outcomes: Vec<SessionOutcome>) -> Self {
        Self { outcomes: Mutex::new(outcomes.into()), call_count: AtomicUsize::new(0) }
    }

    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl DevRunner for MockDevRunner {
    async fn run_dev_session(&self, _story: &StoryInfo) -> SessionOutcome {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.outcomes.lock().unwrap().pop_front()
            .expect("MockDevRunner: no more outcomes — add more via with_outcomes()")
    }
}
```

#### MockCodeReviewer

```rust
pub struct MockCodeReviewer {
    outcomes: Mutex<VecDeque<ReviewOutcome>>,
    call_count: AtomicUsize,
}

impl MockCodeReviewer {
    pub fn with_outcome(outcome: ReviewOutcome) -> Self {
        let mut q = VecDeque::new();
        q.push_back(outcome);
        Self { outcomes: Mutex::new(q), call_count: AtomicUsize::new(0) }
    }

    pub fn never_called() -> Self {
        Self { outcomes: Mutex::new(VecDeque::new()), call_count: AtomicUsize::new(0) }
    }

    pub fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl CodeReviewer for MockCodeReviewer {
    async fn run_review(&self, _story: &StoryInfo) -> ReviewOutcome {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.outcomes.lock().unwrap().pop_front()
            .expect("MockCodeReviewer: no more outcomes (or never_called() was used)")
    }
}
```

#### PipelineTestBuilder — Correct Mock Sharing Pattern

The mocks must be shared between the pipeline (which consumes them as `Box<dyn Trait>`) and the test (which asserts on captured data). The correct pattern uses **interior `Arc` state**:

- `MockGitProvider` stores captured calls in `Arc<Mutex<Vec<CreatePrParams>>>` etc.
- `MockNotifier` stores captured notifications in `Arc<Mutex<Vec<StoryNotification>>>` and `Arc<Mutex<Vec<RunSummary>>>`
- When building the pipeline, **clone the mock struct** (cloning the inner Arcs) — one copy goes into the `Box<dyn Trait>`, the original stays for assertions.

```rust
pub struct PipelineTestBuilder {
    config: BotConfig,
    session_outcomes: Vec<SessionOutcome>,
    review_outcome: Option<ReviewOutcome>,
    mock_git: MockGitProvider,
    mock_notifier: MockNotifier,
}

impl PipelineTestBuilder {
    pub fn new() -> Self { /* defaults with sensible BotConfig */ }
    pub fn with_code_review(mut self, enabled: bool) -> Self { self.config.code_review_enabled = enabled; self }
    pub fn with_session(mut self, outcome: SessionOutcome) -> Self { self.session_outcomes = vec![outcome]; self }
    pub fn with_sessions(mut self, outcomes: Vec<SessionOutcome>) -> Self { self.session_outcomes = outcomes; self }
    pub fn with_review(mut self, outcome: ReviewOutcome) -> Self { self.review_outcome = Some(outcome); self }
    pub fn with_git_provider(mut self, mock: MockGitProvider) -> Self { self.mock_git = mock; self }
    pub fn with_notifier(mut self, mock: MockNotifier) -> Self { self.mock_notifier = mock; self }

    pub fn build(self) -> (StoryPipeline, MockNotifier, MockGitProvider) {
        let notifier_for_assertions = self.mock_notifier.clone(); // clones inner Arcs
        let git_for_assertions = self.mock_git.clone();           // clones inner Arcs

        let dev_runner: Box<dyn DevRunner> = if self.session_outcomes.len() == 1 {
            Box::new(MockDevRunner::with_outcome(self.session_outcomes.into_iter().next().unwrap()))
        } else {
            Box::new(MockDevRunner::with_outcomes(self.session_outcomes))
        };

        let code_reviewer: Box<dyn CodeReviewer> = match self.review_outcome {
            Some(o) => Box::new(MockCodeReviewer::with_outcome(o)),
            None => Box::new(MockCodeReviewer::never_called()),
        };

        let pipeline = StoryPipeline::new_with_components(
            Arc::new(self.config),
            Box::new(self.mock_git),     // moved into pipeline
            Box::new(self.mock_notifier), // moved into pipeline
            dev_runner,
            code_reviewer,
        );

        (pipeline, notifier_for_assertions, git_for_assertions)
    }
}
```

The key: `MockGitProvider` and `MockNotifier` implement `Clone` by cloning their inner `Arc<Mutex<Vec<...>>>` handles. After `build()`, both the pipeline's copy and the test's copy share the same capture buffers.

If Story 7.1's `MockGitProvider`/`MockNotifier` don't implement `Clone` or don't capture args, extend them in this story.

#### Building Test Outcomes — Complete Examples

**Completed session:**
```rust
SessionOutcome::Completed {
    story_key: "4-1-rig-tools".to_string(),
    branch: "story/4-1-rig-tools".to_string(),
    decisions: vec![],
}
```

**Failed session:**
```rust
SessionOutcome::Failed {
    story_key: "4-1-rig-tools".to_string(),
    error: "LLM timeout".to_string(),
    decisions: vec![],
}
```

**Escalated session (all 6 EscalationReport fields required):**
```rust
SessionOutcome::Escalated {
    report: EscalationReport {
        story_key: "4-1-rig-tools".to_string(),
        question: "What database schema should I use?".to_string(),
        reason: "Not specified in architecture docs".to_string(),
        branch_name: "story/4-1-rig-tools".to_string(),
        partial_work_summary: "Created initial tool stubs".to_string(),
        escalated_at: "2026-02-08T19:00:00+00:00".to_string(), // static for tests
    },
    decisions: vec![],
}
```

**Completed review:**
```rust
ReviewOutcome::Completed {
    story_key: "4-1-rig-tools".to_string(),
    branch: "story/4-1-rig-tools".to_string(),
    report: "LGTM — all tests pass, code follows patterns.".to_string(),
}
```

**Failed review:**
```rust
ReviewOutcome::Failed {
    story_key: "4-1-rig-tools".to_string(),
    error: "Review agent crashed".to_string(),
}
```

#### Building Test `StoryInfo`

Use `make_test_story()` from Story 7.1, or build manually:
```rust
StoryInfo {
    story_id: "4.1".to_string(),
    story_key: "4-1-rig-tools".to_string(),
    epic_num: 4,
    story_num: 1,
    label: "rig-tools-implementation".to_string(),
    branch_name: "story/4-1-rig-tools".to_string(),
    specs_path: PathBuf::from("_bmad-output/implementation-artifacts/4-1-rig-tools.md"),
    dependencies: vec![],
    status: "ready-for-dev".to_string(),
}
```

#### GitProvider Assertion Methods Required

`MockGitProvider` must expose:
- `captured_create_pr_params(&self) -> Vec<CreatePrParams>` — all `create_pr` call args
- `captured_add_comment_calls(&self) -> Vec<(String, String)>` — `(pr_id, body)` pairs
- `create_pr_call_count(&self) -> usize`
- `add_comment_call_count(&self) -> usize`

If Story 7.1's mock lacks these, extend it here.

#### MockNotifier Assertion Methods Required

`MockNotifier` must expose:
- `captured_story_notifications(&self) -> Vec<StoryNotification>` — all `notify_story` calls
- `captured_run_summaries(&self) -> Vec<RunSummary>` — all `notify_run_summary` calls
- `story_notification_count(&self) -> usize`
- `run_summary_count(&self) -> usize`

For AC #7 (notification failure test), `MockNotifier` must support a mode where `notify_story()` returns `Err(NotifierError::HttpRequest { reason: "test error".into() })`.

### Previous Story Intelligence (Stories 7.1, 7.2, 7.3)

- **Cargo test convention:** `tests/integration.rs` is the binary entry point, `tests/integration/` is the submodule directory
- **Fixture imports:** `use crate::helpers::fixtures::{make_test_config, make_test_story};`
- **Mock imports:** `use crate::helpers::mocks::{MockGitProvider, MockNotifier};` + new mocks
- **Test naming:** `test_pipeline_{behavior}_{scenario}` in snake_case
- **Structure:** Arrange → Act → Assert
- **Tracing is a no-op in tests** — silent without a subscriber, no need to install one

### Git Intelligence

- `ca81f83` — `feat(pipeline): implement StoryPipeline orchestrator with full error resilience` — mature with 20+ unit tests (L699-953)
- `2df7229` — `docs(stories): create story 7-3, fix critical lib.rs blocker`
- Pipeline refactor in Task 0 builds on a stable, well-tested foundation

### Dependencies Required

All present — no new crate dependencies:
- `async-trait = "0.1"` — for trait definitions (already in Cargo.toml)
- `tokio = { version = "1", features = ["full"] }` — for `#[tokio::test]`
- `tempfile = "3"` (dev-dependency) — if filesystem fixtures needed

**Prerequisite from Story 7.1:**
- `src/lib.rs` with `pub mod pipeline;` and all module re-exports
- `tests/integration.rs` + `tests/integration/helpers/` structure
- `MockGitProvider`, `MockNotifier`, `make_test_config()`, `make_test_story()`

### File Structure

```
src/
├── pipeline.rs                       ← MODIFIED (add traits, new_with_components, refactor struct)
tests/
├── integration.rs                    # Add: mod test_pipeline;
└── integration/
    ├── helpers/
    │   ├── mod.rs
    │   ├── mocks.rs                  ← MODIFIED (add MockDevRunner, MockCodeReviewer)
    │   └── fixtures.rs               ← MODIFIED (add PipelineTestBuilder)
    └── test_pipeline.rs              ← NEW (this story)
```

### Testing Standards

- `#[tokio::test]` for all tests — `process_story()` is async
- Test names: `test_pipeline_{behavior}_{scenario}`
- Assert on specific field values, not just `is_some()`/`is_none()`
- Each test builds its own pipeline via `PipelineTestBuilder` — no shared mutable state
- Never call real LLM, GitHub, or Telegram APIs

### References

- [Source: src/pipeline.rs — StoryPipeline struct (L113-124), new() (L134-164)]
- [Source: src/pipeline.rs — process_story() (L170-368)]
- [Source: src/pipeline.rs — process_eligible_stories() (L374-394)]
- [Source: src/pipeline.rs — recover_and_process() (L429-451), process_recovered_session() (L456-631)]
- [Source: src/pipeline.rs — PipelineResult (L94-103), PipelineError (L34-86)]
- [Source: src/pipeline.rs — story_title_from_label() (L646-658), build_run_summary() (L661-692)]
- [Source: src/pipeline.rs — unit tests (L699-953) — existing Send+Sync test at L832]
- [Source: src/session/mod.rs — SessionOutcome enum (L98-127), #[derive(Debug)] only, no Clone]
- [Source: src/session/escalation.rs — EscalationReport struct (L46-60), 6 fields including escalated_at]
- [Source: src/session/runner.rs — SessionRunner::run() (L455), check_and_recover_wal() (L155), resume_session() (L192)]
- [Source: src/review/mod.rs — ReviewOutcome enum (L108-130), ReviewRunner::run() (L163-178)]
- [Source: src/git_provider/mod.rs — GitProvider trait (L124-133), CreatePrParams (L95-106), PrInfo (L112-121)]
- [Source: src/git_provider/mod.rs — build_pr_title() (L215-221), build_pr_description() (L190-210)]
- [Source: src/notifier/mod.rs — Notifier trait (L125-131), StoryNotification (L83-96), StoryStatus (L67-76)]
- [Source: src/notifier/mod.rs — RunSummary (L102-115), NotifierError (L25-56)]
- [Source: src/supervisor/decisions.rs — DecisionRecord (L65-81), 7 fields including alternatives and timestamp]
- [Source: src/supervisor/decisions.rs — DecisionSource enum (L31-43), format_pr_decisions_section() (L339)]
- [Source: src/watcher/mod.rs — StoryInfo struct (L72-90)]
- [Source: src/config/mod.rs — BotConfig (L75-108), code_review_enabled (L104-108)]
- [Source: _bmad-output/planning-artifacts/epics.md — Story 7.4 (L973-1020)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Test Mock Pattern (L510-542)]
- [Source: _bmad-output/project-context.md — Testing Rules, Daemon Role]
- [Source: _bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md — Mock Design Pattern, Test Directory Convention]

## Dev Agent Record

### Agent Model Used
Claude claude-sonnet-4-20250514 (via Cursor)

### Debug Log References
- AC #3 discrepancy: Story spec stated "NO PR is created" for escalation, but actual `process_story()` code (L452-535) **does** create an escalation PR via `build_pr_title(..., true)` + `git_provider.create_pr()`. Test verifies actual behavior: PR IS created, status is Blocked.
- `process_story()` requires a working `git push` for the Completed path (push_ok gate at L233). Integration tests use `create_test_repo_with_remote()` to set up a local bare remote.
- `SessionOutcome::Completed` has 6 fields in actual code (includes `pr_context`, `pr_how_to_test`, `pr_additional_info`) vs 3 fields in story Dev Notes.
- `StoryPipeline::new()` takes 3 params (`config`, `secrets`, `shutdown`) vs 2 in story spec.
- `ReviewRunner::new()` takes 4 params in actual code (includes `agent_factory` and `shutdown`).
- `PipelineResult` has a `fatal: bool` field not mentioned in story spec.
- `RunSummary` has a `fatal: bool` field not mentioned in story spec.

### Completion Notes List
- Task 0: Refactored `StoryPipeline` for dependency injection. Added `DevRunner` and `CodeReviewer` traits, implemented for `SessionRunner` and `ReviewRunner`. Changed struct to use trait objects. Added `new_with_components()` constructor. Updated all 4 call sites. `recover_and_process()` uses `session_runner_for_recovery: Option<SessionRunner>`. All 25 existing pipeline unit tests pass. All 66 existing integration tests pass.
- Task 1: Added `MockDevRunner` and `MockCodeReviewer` in `tests/integration/helpers/mocks.rs`. Both use `Mutex<VecDeque<...>>` + `AtomicUsize` for multi-call support and call counting.
- Task 2: Added `PipelineTestBuilder` in fixtures.rs with `build()` and `build_with_config()`. Returns `(StoryPipeline, MockNotifier, MockGitProvider)` with shared `Arc<Mutex<...>>` state for assertions.
- Task 3: Created `tests/integration/test_pipeline.rs`, declared in `tests/integration.rs`.
- Tasks 4-11: Wrote 8 integration tests covering all 7 ACs plus batch processing. All pass. Note: AC #3 tests actual behavior (escalation creates PR) not spec (which incorrectly stated no PR).
- Added `create_test_repo_with_remote()` fixture helper for tests requiring `git push` to succeed.
- Added `captured_create_pr_params()`, `captured_add_comment_calls()`, `create_pr_call_count()`, `add_comment_call_count()` to MockGitProvider.
- Added `story_notification_count()`, `run_summary_count()` to MockNotifier.

### File List
- `src/pipeline.rs` — MODIFIED (added DevRunner/CodeReviewer traits, impl for SessionRunner/ReviewRunner, new_with_components(), refactored struct to trait objects)
- `tests/integration.rs` — MODIFIED (added `mod test_pipeline;` declaration)
- `tests/integration/helpers/mocks.rs` — MODIFIED (added MockDevRunner, MockCodeReviewer, assertion helpers on MockGitProvider/MockNotifier)
- `tests/integration/helpers/fixtures.rs` — MODIFIED (added PipelineTestBuilder, create_test_repo_with_remote())
- `tests/integration/test_pipeline.rs` — NEW (8 integration tests for pipeline orchestration)
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — MODIFIED (7-4 status: in-progress → review)