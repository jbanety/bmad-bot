# Story 3.3: Human Escalation

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want the supervisor to stop and escalate to me when it cannot answer a question confidently,
So that no incorrect decision is made autonomously.

## Acceptance Criteria

1. **Given** the rule engine returns `NoMatch` and the Architect session either fails or cannot answer confidently **When** the supervisor determines it cannot answer **Then** the `ask_supervisor` tool returns a `SupervisorError::EscalationRequired` error **And** this error stops the rig agent loop, returning control to the daemon session module

2. **Given** the supervisor has escalated **When** the session module receives the escalation error **Then** the story status is set to `needs-clarification` in `sprint-status.yaml` (via session cleanup logic) **And** the escalation event is logged via `tracing::warn!()` with `action = "supervisor_escalation"`, the question, and the reason for escalation

3. **Given** the supervisor escalates **When** the session handles the escalation **Then** partial work is preserved: all committed changes remain on the story branch, uncommitted staged changes are committed with a `chore: WIP — escalated for human clarification` message, and the branch is NOT deleted

4. **Given** a story has been escalated **When** the daemon's watcher polls `sprint-status.yaml` on the next cycle **Then** stories with `needs-clarification` status are skipped (not eligible for session launch) **And** the daemon proceeds to evaluate the next eligible story in sprint order

5. **Given** the session module handles an escalation **When** cleanup completes **Then** an `EscalationReport` struct is returned to the daemon main loop containing: `story_key`, `question`, `reason`, `branch_name`, and `partial_work_summary` **And** this report is available for the notifier module (Epic 6) to send to the human

## Tasks / Subtasks

- [x] Task 0: Verify prerequisites from Stories 3.1 and 3.2 (AC: #1–#5)
  - [x] 0.1 Verify `src/supervisor/mod.rs` contains `AskSupervisor` tool with `SupervisorError::EscalationRequired { question: String, reason: String }` variant
  - [x] 0.2 Verify `AskSupervisor::call()` pipeline: rule engine → Architect session → `Err(SupervisorError::EscalationRequired)` on failure (Story 3.2 flow)
  - [x] 0.3 Verify `SupervisorError` implements `std::error::Error + Send + Sync`
  - [x] 0.4 Verify `src/session/mod.rs` exists with the chat loop structure from Epic 1/Epic 4 stories (or stub it if not yet built)
  - [x] 0.5 Verify `src/watcher/mod.rs` exists with sprint-status polling logic
  - [x] 0.6 Run `cargo check` to confirm clean baseline

- [x] Task 1: Define `EscalationInfo` and `EscalationReport` in `src/session/escalation.rs` (AC: #5)
  - [x] 1.1 Create new file `src/session/escalation.rs`
  - [x] 1.2 Add `pub mod escalation;` to `src/session/mod.rs`
  - [x] 1.3 Define `EscalationInfo` struct — the data carrier stored in the shared escalation slot:
    ```
    /// Carries escalation context from the supervisor tool to the session chat loop.
    #[derive(Debug, Clone)]
    pub struct EscalationInfo {
        pub question: String,
        pub reason: String,
    }
    ```
  - [x] 1.4 Define `EscalationReport` struct:
    ```
    /// Full escalation report returned to the daemon for logging and notification.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EscalationReport {
        pub story_key: String,
        pub question: String,
        pub reason: String,
        pub branch_name: String,
        pub partial_work_summary: String,
        pub escalated_at: String, // ISO 8601 timestamp
    }
    ```
  - [x] 1.5 Implement `EscalationReport::new(story_key, question, reason, branch_name, partial_work_summary) -> Self` — sets `escalated_at` to `chrono::Utc::now().to_rfc3339()`
  - [x] 1.6 Implement `Display` for `EscalationReport` — human-readable summary for logs and notifications
  - [x] 1.7 Add `/// doc comments` on all public items

- [x] Task 2: Define `SessionError` escalation variant in `src/session/mod.rs` (AC: #1, #2)
  - [x] 2.1 Add or verify `SessionError` thiserror enum in `src/session/mod.rs` with variant:
    ```
    #[error("Supervisor escalation required for story {story_key}: {question}")]
    SupervisorEscalation {
        story_key: String,
        question: String,
        reason: String,
    }
    ```
  - [x] 2.2 Ensure `SessionError` has other relevant variants (these may already exist from prior stories): `ChatFailed`, `ToolError`, `StateFileFailed`, `GitError`
  - [x] 2.3 Ensure `SessionError` implements `std::error::Error + Send + Sync`

- [x] Task 3: Implement escalation detection in the session chat loop (AC: #1, #2)
  - [x] 3.1 In the session chat loop (the `while` loop driving `agent.chat()`), detect when the supervisor signals escalation via the shared `Arc<Mutex<Option<EscalationInfo>>>` slot
  - [x] 3.2 **Detection mechanism — shared escalation slot (deterministic, rig-version-independent):**
    The `AskSupervisor` tool and the session chat loop share an `Arc<Mutex<Option<EscalationInfo>>>`. When `call()` returns `EscalationRequired`, it writes `Some(EscalationInfo { question, reason })` into the slot BEFORE returning the error. After each `chat()` turn, the session locks the mutex and checks for `Some`. This approach:
    - Carries the full question + reason context (unlike a bare AtomicBool)
    - Does not depend on rig's internal error handling or response text parsing
    - Is deterministic and unit-testable
  - [x] 3.3 When escalation is detected, extract the `EscalationInfo` from the slot and break out of the chat loop with `SessionError::SupervisorEscalation { story_key, question, reason }`
  - [x] 3.4 Log the escalation event:
    ```
    tracing::warn!(
        action = "supervisor_escalation",
        story_id = %story_key,
        question = %question,
        reason = %reason,
        "Supervisor escalation — returning control to daemon"
    );
    ```
  - [x] 3.5 **Critical:** Do NOT retry the chat loop after escalation — the escalation is a deliberate signal that human input is required

- [x] Task 4: Implement partial work preservation on escalation (AC: #3)
  - [x] 4.1 Create `async fn preserve_partial_work(repo_path: &Path, story_key: &str, question: &str) -> String` in `src/session/cleanup.rs` — note: returns `String` directly (not `Result`), because preservation is **best-effort** and must never fail the escalation flow
  - [x] 4.2 Create new file `src/session/cleanup.rs`, add `pub mod cleanup;` to `src/session/mod.rs`
  - [x] 4.3 **Git state check:** Use git2 to check for uncommitted changes:
    - If dirty working tree (unstaged or staged changes): commit all with message `chore: WIP — escalated for human clarification\n\nQuestion: {question}`
    - If clean working tree: no action needed
  - [x] 4.4 **Branch preservation:** Do NOT delete or reset the story branch — leave it exactly as-is so the human can inspect and resume
  - [x] 4.5 **Summary generation:** Build a `partial_work_summary` string listing:
    - Branch name
    - Number of commits on the story branch (ahead of base)
    - Last commit message
    - List of files modified in the branch (from `git diff --name-only` against base)
  - [x] 4.6 Return the `partial_work_summary` string
  - [x] 4.7 Log the preservation:
    ```
    tracing::info!(
        action = "preserve_partial_work",
        story_id = %story_key,
        summary = %partial_work_summary,
        "Partial work committed and preserved on branch"
    );
    ```
  - [x] 4.8 **Best-effort error handling:** Every git2 operation inside this function MUST be wrapped in a match or `.unwrap_or` — never use `?` to propagate. If any git operation fails, log the error via `tracing::error!()` and return a fallback summary string: `"Preservation failed — check branch state manually. Error: {e}"`. The escalation flow must NEVER be blocked by a preservation failure.

- [x] Task 5: Implement sprint-status update to `needs-clarification` (AC: #2, #4)
  - [x] 5.1 Create `async fn mark_story_needs_clarification(sprint_status_path: &Path, story_key: &str) -> Result<(), SessionError>` in `src/session/cleanup.rs`
  - [x] 5.2 Read the FULL `sprint-status.yaml` file
  - [x] 5.3 Find the `development_status` key matching `story_key`
  - [x] 5.4 Update the status value to `needs-clarification`
  - [x] 5.5 Write the file back, preserving ALL comments, structure, and the STATUS DEFINITIONS header
  - [x] 5.6 **YAML preservation strategy:** Since `serde_yml` strips comments, use a string-based find-and-replace approach:
    - Read file as string
    - Find the line containing `{story_key}:` followed by the current status
    - Replace the status value with `needs-clarification`
    - Write the modified string back
  - [x] 5.7 Log the status update:
    ```
    tracing::info!(
        action = "story_status_update",
        story_id = %story_key,
        new_status = "needs-clarification",
        "Story marked as needs-clarification in sprint-status.yaml"
    );
    ```
  - [x] 5.8 If the file write fails, log `tracing::error!()` and return `SessionError::StateFileFailed`
  - [x] 5.9 **Update STATUS DEFINITIONS comments:** Add `needs-clarification` to the Story Status block in the comments at the top of `sprint-status.yaml`. Add after the `done` entry:
    ```
    #   - needs-clarification: Supervisor escalated — awaiting human input before retry
    ```
    And add the transition to Story Status Transitions (new comment block if needed):
    ```
    #   - in-progress → needs-clarification: Automatically when supervisor escalates to human
    #   - needs-clarification → ready-for-dev: Manually after human provides clarification
    ```

- [x] Task 6: Implement watcher skip logic for `needs-clarification` stories (AC: #4)
  - [x] 6.1 In `src/watcher/mod.rs` (or `deps.rs`), update the story eligibility check to skip stories with status `needs-clarification`
  - [x] 6.2 The existing watcher logic filters for `ready-for-dev` stories — verify that `needs-clarification` is NOT in the eligible statuses list (it likely already works since only `ready-for-dev` triggers sessions)
  - [x] 6.3 Add explicit `tracing::debug!()` log when a `needs-clarification` story is encountered during polling:
    ```
    tracing::debug!(
        action = "watcher_skip",
        story_id = %story_key,
        status = "needs-clarification",
        "Skipping story — awaiting human clarification"
    );
    ```
  - [x] 6.4 If the watcher currently only checks for `ready-for-dev`, this task may be a verification-only step (add the log, confirm behavior)

- [x] Task 7: Wire escalation into session completion flow (AC: #1, #2, #3, #5)
  - [x] 7.1 In the session's main `run()` or `execute()` function, add an escalation handler after the chat loop:
    ```
    match chat_loop_result {
        Ok(session_result) => { /* normal completion flow */ }
        Err(SessionError::SupervisorEscalation { story_key, question, reason }) => {
            // 1. Preserve partial work (best-effort, never fails)
            let summary = preserve_partial_work(&repo_path, &story_key, &question).await;

            // 2. Update sprint-status.yaml (best-effort — log and continue on failure)
            if let Err(e) = mark_story_needs_clarification(&sprint_status_path, &story_key).await {
                tracing::error!(
                    action = "status_update_failed",
                    story_id = %story_key,
                    error = %e,
                    "Failed to mark story as needs-clarification — manual update required"
                );
            }

            // 3. Delete WAL file (session is over)
            cleanup_session_state().await;

            // 4. Build escalation report
            let report = EscalationReport::new(
                story_key, question, reason, branch_name, summary
            );
            tracing::warn!(
                action = "session_escalated",
                report = %report,
                "Session ended via escalation — story needs human input"
            );

            // 5. Return report to daemon for notification
            return Ok(SessionOutcome::Escalated(report));
        }
        Err(other_error) => { /* other error handling */ }
    }
    ```
  - [x] 7.2 Define `SessionOutcome` enum (if not already existing):
    ```
    pub enum SessionOutcome {
        Completed { story_key: String, branch: String },
        Escalated(EscalationReport),
        Failed { story_key: String, error: String },
    }
    ```
  - [x] 7.3 Ensure the daemon main loop handles `SessionOutcome::Escalated` by storing the report for future notification (Epic 6)
  - [x] 7.4 Delete the session WAL file on escalation (the session is definitively over — not a crash)
  - [x] 7.5 **Critical orchestration rule:** Both `preserve_partial_work` and `mark_story_needs_clarification` are best-effort during escalation. Neither failure should prevent the `EscalationReport` from being built and returned. The daemon MUST always receive `SessionOutcome::Escalated` when escalation occurs, regardless of cleanup success.

- [x] Task 8: Write unit tests (AC: #1–#5)
  - [x] 8.1 **EscalationInfo and EscalationReport tests** in `src/session/escalation.rs`:
    - Test `EscalationInfo` construction and Clone
    - Test `EscalationReport::new()` sets all fields correctly and `escalated_at` is a valid ISO 8601 timestamp
    - Test `Display` impl produces a human-readable summary
    - Test `EscalationReport` serializes and deserializes correctly (round-trip via serde_json)
    - Test `EscalationReport` implements `Clone`, `Debug`, `Send`, `Sync`
  - [x] 8.2 **Sprint-status update tests** in `src/session/cleanup.rs`:
    - Test `mark_story_needs_clarification()` with a tempfile containing a mock sprint-status.yaml:
      - Verify the target story status changes from `in-progress` to `needs-clarification`
      - Verify all other statuses remain unchanged
      - Verify comments in the file are preserved (string-based replacement)
      - Verify non-existent story key returns an error
    - Use `tempfile::TempDir` for test fixtures
  - [x] 8.3 **Watcher skip tests** in `src/watcher/mod.rs` or `deps.rs`:
    - Test that a story with `needs-clarification` status is NOT included in eligible stories
    - Test that `ready-for-dev` stories ARE included (regression check)
    - Test that `backlog`, `in-progress`, `done` stories are NOT included (regression check)
  - [x] 8.4 **Escalation detection tests** in session module:
    - Test that `SessionError::SupervisorEscalation` variant is constructable and displays correctly
    - Test that `SessionOutcome::Escalated` variant carries the correct report
    - Test shared escalation slot: write `EscalationInfo` from one thread, read from another
  - [x] 8.5 **Partial work preservation tests** in `src/session/cleanup.rs`:
    - Test `preserve_partial_work()` with a mock git repo (use `git2::Repository::init()` in a tempdir):
      - Test with dirty working tree → produces a WIP commit, summary includes "yes"
      - Test with clean working tree → no commit, summary includes "no (clean tree)"
      - Test summary includes branch name, commit count, file list
    - Test that git failure during preservation returns a fallback summary string (not an error)
  - [x] 8.6 **Integration flow test:**
    - Test the full escalation flow with mocked components: escalation error → preserve work → update status → produce report
    - Test that status update failure does NOT prevent report generation
    - Use mock/stub git operations and a tempfile for sprint-status.yaml
  - [x] 8.7 Verify all existing Stories 3.1 and 3.2 tests still pass (no regressions)

- [x] Task 9: Final quality checks
  - [x] 9.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [x] 9.2 Run `cargo clippy` and fix any warnings
  - [x] 9.3 Run `cargo test` and verify all tests pass (including Epic 1, Epic 2, Stories 3.1 and 3.2 tests)
  - [x] 9.4 Verify all public items have `///` doc comments
  - [x] 9.5 Verify `SessionError` implements `std::error::Error + Send + Sync`
  - [x] 9.6 Verify no `unwrap()` or `expect()` in production code
  - [x] 9.7 Verify no `println!` or `eprintln!` — tracing only
  - [x] 9.8 Verify no API keys or secrets are logged by any tracing statement

## Dev Notes

### Previous Story Intelligence

**Story 3.1** established the complete supervisor tool skeleton:
- `AskSupervisor` struct with `rule_engine: RuleEngine` field, derives `Serialize + Deserialize`
- `AskSupervisorArgs` with `question: String` and `context: Option<String>`
- `SupervisorError` thiserror enum with `RuleEngineError`, `EscalationRequired { question: String, reason: String }`, `LlmFallbackNotImplemented`
- Full `Tool` trait impl: `NAME = "ask_supervisor"`, `Error = SupervisorError`, `Args = AskSupervisorArgs`, `Output = String`
- `call()` pipeline: rule engine match → return answer, no match → `Err(LlmFallbackNotImplemented)`
- `RuleEngine` with 6 built-in rule categories (confirmations, permissions, step-by-step, story selection, progress, stuck)
- `DecisionRecord` and `DecisionSource` stubs in `decisions.rs`

**Story 3.2** extended the supervisor with LLM fallback:
- `ArchitectSession` struct in `src/supervisor/architect.rs` — multi-turn chat with BMAD Architect agent
- `ReadFile` rig tool in `src/supervisor/read_tool.rs` — read-only, project-root bounded
- `AskSupervisor` updated: `architect_session: Option<ArchitectSession>` with `#[serde(skip)]`
- Updated `call()` pipeline: rule engine → Architect session → `Err(SupervisorError::EscalationRequired)` on failure
- Provider selection logic: Anthropic, OpenAI, GitHub Models
- Retry strategy: 3 total attempts with exponential backoff on transient LLM errors

**Story 3.2 forward-compatibility notes for THIS story:**
- `SupervisorError::EscalationRequired { question, reason }` is already the terminal error returned when both rule engine AND Architect session fail
- This story adds the SESSION-LEVEL handling: catch that error, preserve work, update status, report to daemon
- Supervisor module changes are minimal: add shared escalation slot field, set it before returning error

**Stories 1.1–1.4** established:
- `BotConfig` with paths: `project_root`, sprint-status path, output directories
- Config shared as `Arc<BotConfig>` — never mutated after startup
- CLI commands: `init`, `start`, `status`, `logs`
- Tracing setup with structured spans and `action` fields

**Stories 2.1–2.3** established:
- Sprint-status polling in `src/watcher/mod.rs` — reads `sprint-status.yaml`, filters eligible stories
- Dependency resolution in `src/watcher/deps.rs` — pre-gate logic, cascade blocking
- Story eligibility: currently filters for `ready-for-dev` status only
- `StoryInfo` struct passed from watcher to session: id, label, branch name, specs path, dependencies

### Core Design — Escalation as a First-Class Session Outcome

The escalation is NOT an error in the traditional sense — it is a **deliberate, correct decision** by the supervisor that human input is required. The system must treat it with the same care as a successful session completion:

```
┌─────────────────────────────────────────────────────────┐
│  Session Chat Loop                                       │
│                                                          │
│  agent.chat(message, history) ──► Agent works normally   │
│       │                                                  │
│       ├── Agent calls ask_supervisor("How should I...?") │
│       │       │                                          │
│       │       ├── Rule engine: NoMatch                   │
│       │       ├── Architect session: Failed / No answer  │
│       │       └── Returns EscalationRequired ◄───────┐  │
│       │           + writes EscalationInfo to slot     │  │
│       │                                              │   │
│       └── rig stops agent loop, returns error ───────┘   │
│                                                          │
│  Session checks escalation slot after each chat() turn   │
│       │                                                  │
│       ├── slot contains Some(EscalationInfo) → break     │
│       │                                                  │
│  Escalation cleanup (all best-effort):                   │
│       │                                                  │
│       ├── 1. preserve_partial_work()                     │
│       │      └── Commit WIP if dirty tree                │
│       ├── 2. mark_story_needs_clarification()            │
│       │      └── Update sprint-status.yaml               │
│       ├── 3. cleanup_session_state()                     │
│       │      └── Delete WAL file                         │
│       ├── 4. Build EscalationReport                      │
│       └── 5. Return SessionOutcome::Escalated(report)    │
│                                                          │
│  Daemon receives Escalated outcome                       │
│       │                                                  │
│       ├── Store report for notification (Epic 6)         │
│       └── Continue to next poll cycle                    │
└─────────────────────────────────────────────────────────┘
```

**Key principle:** The supervisor MUST NEVER invent answers. If neither the rule engine nor the Architect can answer confidently → escalate. A wrong autonomous decision costs far more than pausing for human input. [Source: project-context.md#Critical Don't-Miss Rules]

### Escalation Slot — Shared `Arc<Mutex<Option<EscalationInfo>>>`

The session chat loop must detect when escalation occurs inside a rig tool call and extract the question/reason context. A bare `AtomicBool` is insufficient because it cannot carry the `question` and `reason` strings needed for `SessionError::SupervisorEscalation` and `EscalationReport`.

**Design:** Use `Arc<Mutex<Option<EscalationInfo>>>` shared between `AskSupervisor` and the session chat loop:

```rust
use std::sync::{Arc, Mutex};
use crate::session::escalation::EscalationInfo;

// Type alias for clarity
pub type EscalationSlot = Arc<Mutex<Option<EscalationInfo>>>;

// --- In AskSupervisor (src/supervisor/mod.rs) ---

#[derive(Debug, Serialize, Deserialize)]
pub struct AskSupervisor {
    rule_engine: RuleEngine,
    #[serde(skip)]
    architect_session: Option<ArchitectSession>,
    #[serde(skip)]
    escalation_slot: EscalationSlot,
}

// In call() — write to slot before returning the error
Err(SupervisorError::EscalationRequired { ref question, ref reason }) => {
    if let Ok(mut slot) = self.escalation_slot.lock() {
        *slot = Some(EscalationInfo {
            question: question.clone(),
            reason: reason.clone(),
        });
    }
    Err(SupervisorError::EscalationRequired { question, reason })
}

// --- In session chat loop (src/session/mod.rs) ---

let escalation_slot: EscalationSlot = Arc::new(Mutex::new(None));

// Pass clone to AskSupervisor at construction
let ask_supervisor = AskSupervisor::with_escalation_slot(
    rule_engine,
    architect_session,
    Arc::clone(&escalation_slot),
);

// After each chat() turn, check the slot
loop {
    let response = agent.chat(&message, history.clone()).await?;
    // ... process response ...

    if let Ok(slot) = escalation_slot.lock() {
        if let Some(info) = slot.as_ref() {
            break Err(SessionError::SupervisorEscalation {
                story_key: story_key.clone(),
                question: info.question.clone(),
                reason: info.reason.clone(),
            });
        }
    }
}
```

**Why `Mutex` not `AtomicBool`:** The escalation context (question + reason) is essential for the `EscalationReport`. An `AtomicBool` only signals "something happened" but loses the critical details. A `Mutex<Option<EscalationInfo>>` carries the full context with minimal overhead — the lock is held only for the brief read/write of a small struct.

**Why this over response parsing:** Parsing the agent's response text for escalation keywords is fragile and rig-version-dependent. The shared slot is deterministic, testable, and works regardless of how rig handles tool errors internally.

### Partial Work Preservation — Best-Effort Git Operations

The preservation step uses `git2` (same library as Epic 4 tools) for all git operations. No shell `git` commands.

**⚠️ CRITICAL: This function returns `String`, NOT `Result`. It must NEVER fail the escalation flow.** Every git2 operation is wrapped in a match — errors are logged and a fallback summary is returned.

```rust
use git2::{Repository, StatusOptions};
use std::path::Path;

/// Preserves partial work on the story branch during escalation.
/// Returns a summary string. NEVER returns an error — preservation is best-effort.
pub async fn preserve_partial_work(
    repo_path: &Path,
    story_key: &str,
    question: &str,
) -> String {
    let repo = match Repository::open(repo_path) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(
                action = "preserve_partial_work",
                error = %e,
                "Failed to open git repo — skipping preservation"
            );
            return format!("Preservation failed — could not open repo: {e}");
        }
    };

    // Check for dirty state
    let statuses = match repo.statuses(Some(
        StatusOptions::new()
            .include_untracked(true)
            .recurse_untracked_dirs(true),
    )) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(
                action = "preserve_partial_work",
                error = %e,
                "Failed to check git status — skipping preservation"
            );
            return format!("Preservation failed — could not read status: {e}");
        }
    };

    let has_changes = !statuses.is_empty();

    if has_changes {
        let commit_result = (|| -> Result<(), git2::Error> {
            let mut index = repo.index()?;
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
            index.write()?;
            let tree_id = index.write_tree()?;
            let tree = repo.find_tree(tree_id)?;
            let head = repo.head()?.peel_to_commit()?;
            let sig = repo.signature()?;
            let message = format!(
                "chore: WIP — escalated for human clarification\n\nQuestion: {}",
                question
            );
            repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&head])?;
            Ok(())
        })();

        if let Err(e) = commit_result {
            tracing::error!(
                action = "preserve_partial_work",
                story_id = %story_key,
                error = %e,
                "Failed to create WIP commit — changes remain unstaged"
            );
            // Continue to build summary anyway — partial info is better than nothing
        }
    }

    // Build summary — each step individually guarded
    let branch = match repo.head() {
        Ok(head) => head.shorthand().map(String::from).unwrap_or_default(),
        Err(_) => "unknown".to_string(),
    };

    let changed_files: Vec<String> = statuses.iter()
        .filter_map(|s| s.path().map(String::from))
        .collect();

    let summary = format!(
        "Branch: {}\nWIP commit: {}\nFiles touched: {}",
        branch,
        if has_changes { "yes" } else { "no (clean tree)" },
        if changed_files.is_empty() { "none".to_string() } else { changed_files.join(", ") }
    );

    tracing::info!(
        action = "preserve_partial_work",
        story_id = %story_key,
        summary = %summary,
        "Partial work committed and preserved on branch"
    );

    summary
}
```

### Sprint-Status YAML Update — Comment-Preserving Strategy

`serde_yml` does NOT preserve comments. Since `sprint-status.yaml` contains extensive STATUS DEFINITIONS comments that must be preserved, use string-based replacement:

```rust
use tokio::fs;
use regex::Regex;

/// Updates a story's status to `needs-clarification` in sprint-status.yaml.
/// Uses string-based replacement to preserve all comments and structure.
pub async fn mark_story_needs_clarification(
    sprint_status_path: &Path,
    story_key: &str,
) -> Result<(), SessionError> {
    let content = fs::read_to_string(sprint_status_path).await
        .map_err(|e| SessionError::StateFileFailed {
            reason: format!("Failed to read sprint-status: {e}"),
        })?;

    // Pattern: "  story-key: current-status" → "  story-key: needs-clarification"
    let pattern = format!(r"(?m)(^\s*{}\s*:\s*)\S+", regex::escape(story_key));
    let re = Regex::new(&pattern)
        .map_err(|e| SessionError::StateFileFailed {
            reason: format!("Invalid regex for story key: {e}"),
        })?;

    if !re.is_match(&content) {
        return Err(SessionError::StateFileFailed {
            reason: format!("Story key '{story_key}' not found in sprint-status.yaml"),
        });
    }

    let updated = re.replace(&content, "${1}needs-clarification").to_string();

    fs::write(sprint_status_path, &updated).await
        .map_err(|e| SessionError::StateFileFailed {
            reason: format!("Failed to write sprint-status: {e}"),
        })?;

    tracing::info!(
        action = "story_status_update",
        story_id = %story_key,
        new_status = "needs-clarification",
        "Story marked as needs-clarification in sprint-status.yaml"
    );

    Ok(())
}
```

### `needs-clarification` Status — Watcher Behavior and Documentation

The watcher in `src/watcher/mod.rs` currently filters for stories with `ready-for-dev` status. The `needs-clarification` status naturally falls outside this filter. This story should:

1. **Verify** that the watcher only processes `ready-for-dev` stories (regression check)
2. **Add** an explicit skip log for `needs-clarification` stories (observability)
3. **Update STATUS DEFINITIONS** in `sprint-status.yaml` to document the new status:

```yaml
# Story Status:
#   - backlog: Story only exists in epic file
#   - ready-for-dev: Story file created in stories folder
#   - in-progress: Developer actively working on implementation
#   - review: Ready for code review (via Dev's code-review workflow)
#   - done: Story completed
#   - needs-clarification: Supervisor escalated — awaiting human input before retry
#
# Story Status Transitions:
#   ...existing transitions...
#   - in-progress → needs-clarification: Automatically when supervisor escalates to human
#   - needs-clarification → ready-for-dev: Manually after human provides clarification
```

The daemon does NOT automatically retry escalated stories. The human must:
1. Read the escalation report (from notifications or logs)
2. Answer the question (update story file, architecture doc, or provide inline guidance)
3. Manually change the story status back to `ready-for-dev` in `sprint-status.yaml`
4. The daemon picks it up on the next poll cycle

### SessionOutcome Enum — Daemon Integration

The session module returns a `SessionOutcome` to the daemon main loop. This enum unifies all possible session endings:

```rust
/// Result of a development session run.
#[derive(Debug)]
pub enum SessionOutcome {
    /// Session completed successfully — story is done, PR ready.
    Completed {
        story_key: String,
        branch: String,
    },
    /// Session escalated to human — needs clarification.
    Escalated(EscalationReport),
    /// Session failed with an unrecoverable error.
    Failed {
        story_key: String,
        error: String,
    },
}
```

The daemon handles each variant:
- `Completed` → proceed to code review (if enabled) and PR creation (Epic 5)
- `Escalated` → store report for notification (Epic 6), proceed to next poll cycle
- `Failed` → create PR with partial work and failure description (FR23), notify human (FR35)

### Integration with Future Stories

**Story 3.4 (Decision Logging)** will:
- Record a `DecisionRecord` for escalation events with `DecisionSource::Escalation`
- The escalation question and reason will be included in the decisions file
- No changes to escalation.rs needed — decision logging wraps around `call()` in `supervisor/mod.rs`

**Epic 4 (Session)** will:
- Implement the full chat loop where escalation detection lives
- Build the `AskSupervisor` with `escalation_slot` at session setup
- The `preserve_partial_work()` function uses the same `git2` operations as the git tool in `src/tools/git.rs`

**Epic 4 / Epic 1 — SIGTERM Graceful Shutdown (FR34):**
- `preserve_partial_work()` is designed to be reusable for SIGTERM/SIGINT handling. The same function (commit WIP, preserve branch, build summary) is exactly what FR34 requires during graceful shutdown. When implementing signal handling, call `preserve_partial_work()` from the signal handler before exiting. No changes to this function needed — it's already best-effort and non-failing.

**Epic 5 (PR Management)** will:
- For escalated stories: optionally create a PR with partial code and escalation details in the description (FR23)
- `EscalationReport` provides all data needed for the PR body

**Epic 6 (Notifications)** will:
- Send `EscalationReport` details to the human via Telegram
- Notification message includes: story key, the question that triggered escalation, branch name, partial work summary
- This is a non-blocking notification — failure to send does not affect the escalation flow

### AskSupervisor Modifications — Escalation Slot

Story 3.3 adds an `escalation_slot: EscalationSlot` field to `AskSupervisor`. This is the ONLY modification to the supervisor module:

| File | Change |
|------|--------|
| `src/supervisor/mod.rs` | **MODIFY** — Add `escalation_slot: EscalationSlot` field with `#[serde(skip)]`, add `pub type EscalationSlot = Arc<Mutex<Option<EscalationInfo>>>`, update constructors to accept the slot, write `EscalationInfo` in `call()` before returning `EscalationRequired` |
| `src/supervisor/rules.rs` | **NO CHANGE** |
| `src/supervisor/architect.rs` | **NO CHANGE** |
| `src/supervisor/read_tool.rs` | **NO CHANGE** |
| `src/supervisor/decisions.rs` | **NO CHANGE** |

### Files Modified/Created in This Story

| File | Change |
|------|--------|
| `src/session/escalation.rs` | **CREATE** — `EscalationInfo` struct, `EscalationReport` struct, `Display` impl, constructors |
| `src/session/cleanup.rs` | **CREATE** — `preserve_partial_work()` (best-effort, returns String), `mark_story_needs_clarification()` (returns Result) |
| `src/session/mod.rs` | **MODIFY** — Add `pub mod escalation;`, `pub mod cleanup;`, add `SupervisorEscalation` variant to `SessionError`, add escalation detection in chat loop, add `SessionOutcome` enum, wire escalation handler |
| `src/supervisor/mod.rs` | **MODIFY** — Add `escalation_slot: EscalationSlot` field, update constructors, write `EscalationInfo` in `call()` |
| `src/watcher/mod.rs` | **MODIFY** — Add explicit skip log for `needs-clarification` stories (minor) |

### Anti-Patterns to Avoid

- ❌ **NO** retrying after escalation — escalation means STOP and wait for human input
- ❌ **NO** auto-resolving escalation questions — the supervisor must never guess or invent answers
- ❌ **NO** deleting the story branch on escalation — partial work is valuable
- ❌ **NO** parsing `sprint-status.yaml` with `serde_yml` for updates — use string-based replacement to preserve comments
- ❌ **NO** blocking the daemon on escalation — update status, report, move to next poll cycle
- ❌ **NO** using `Arc<AtomicBool>` for the escalation flag — it cannot carry question/reason context; use `Arc<Mutex<Option<EscalationInfo>>>` instead
- ❌ **NO** using `?` operator in `preserve_partial_work` — function is best-effort, must never propagate errors
- ❌ **NO** propagating `mark_story_needs_clarification` failure as a session failure — status update failure should be logged but must not prevent `SessionOutcome::Escalated` from being returned
- ❌ **NO** real LLM API calls in unit tests — mock all external dependencies
- ❌ **NO** `unwrap()` or `expect()` in production code
- ❌ **NO** `anyhow::Result` in session or supervisor modules — typed errors only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** logging API keys or secrets via tracing
- ❌ **NO** modifying `rules.rs`, `architect.rs`, `read_tool.rs`, or `decisions.rs`
- ❌ **NO** implementing notification sending — that's Epic 6
- ❌ **NO** implementing PR creation for escalated stories — that's Epic 5
- ❌ **NO** implementing decision logging for escalation events — that's Story 3.4

### Scope Boundaries

**IN SCOPE for this story:**
- `src/session/escalation.rs` — `EscalationInfo` and `EscalationReport` structs with serialization
- `src/session/cleanup.rs` — `preserve_partial_work()` and `mark_story_needs_clarification()` functions
- `src/session/mod.rs` — Escalation detection, `SessionError::SupervisorEscalation`, `SessionOutcome` enum, orchestration
- `src/supervisor/mod.rs` — Add `escalation_slot` field and write `EscalationInfo` on `EscalationRequired`
- `src/watcher/mod.rs` — Skip log for `needs-clarification` stories
- `sprint-status.yaml` — Update STATUS DEFINITIONS comments with `needs-clarification`

**OUT OF SCOPE — do NOT implement:**
- Notification of escalation to human via Telegram (Epic 6, Story 6.1)
- PR creation for escalated stories with partial code (Epic 5)
- Decision logging for escalation events (Story 3.4)
- Automatic retry or resume of escalated stories (not planned — human must manually re-queue)
- Confidence scoring of Architect answers to decide escalation (future enhancement)
- Escalation queue or dashboard (future enhancement — v2/v3)

### Testing Requirements

All tests follow the established patterns: `test_{module}_{behavior}_{scenario}`, Arrange → Act → Assert, `tempfile::TempDir` for file fixtures, `git2::Repository::init()` for git fixtures, no real API calls.

**Test coverage target:**
- `EscalationInfo` — construction, clone
- `EscalationReport` — construction, display, serialization round-trip
- `mark_story_needs_clarification()` — happy path, missing key, comment preservation
- `preserve_partial_work()` — dirty tree, clean tree, git failure returns fallback string (not error)
- `SessionOutcome::Escalated` — variant construction, pattern matching
- Watcher — `needs-clarification` stories skipped
- Shared escalation slot — concurrent write/read from different threads
- Full flow — escalation error → preserve → status update → report (integration test with mocked git)
- Orchestration resilience — status update failure still produces `SessionOutcome::Escalated`

### Dev Dependencies Required

- `chrono` — for `EscalationReport::new()` ISO 8601 timestamp generation (`chrono::Utc::now().to_rfc3339()`)
- `tempfile` — for filesystem test fixtures (likely already present from prior stories)
- `git2` — for git test fixtures with `Repository::init()` (already a project dependency)
- `regex` — for comment-preserving YAML updates (may already be present; add if not)
- `serde_json` — for serialization round-trip tests (likely already present)

### Project Structure Notes

- `src/session/escalation.rs` is a NEW file — contains `EscalationInfo` (shared data carrier) and `EscalationReport` (daemon-facing report)
- `src/session/cleanup.rs` is a NEW file — contains `preserve_partial_work()` and `mark_story_needs_clarification()`, designed for reuse in SIGTERM graceful shutdown (FR34)
- All session-related escalation logic lives in `src/session/` — the supervisor only signals, the session handles
- The `EscalationReport` struct is designed to be passed across module boundaries (daemon ← session) and serialized for notifications
- Alignment with architecture module map: session depends on supervisor (receives errors), watcher depends on config (reads statuses)

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 3.3: Human Escalation] — Acceptance criteria and epic context
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 1: Supervisor Interception Model] — Hybrid chat loop + supervisor tool design, escalation stops rig agent loop
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 3: Session State Persistence] — WAL file lifecycle, session cleanup on termination
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 4: Error Propagation] — Layer 3 session errors: commit partial work, create PR, notify, move on
- [Source: _bmad-output/planning-artifacts/architecture.md#Project Structure & Boundaries] — Module communication map, session → supervisor interface
- [Source: _bmad-output/planning-artifacts/architecture.md#Data Flow] — Step 6: agent calls ask_supervisor, escalation stops session
- [Source: _bmad-output/planning-artifacts/prd.md#Supervision] — FR15: escalate when neither rules nor LLM can answer
- [Source: _bmad-output/planning-artifacts/prd.md#Error Handling & Resilience] — FR34: graceful shutdown with partial work preservation, FR35: notify on blocking errors
- [Source: _bmad-output/planning-artifacts/prd.md#Pull Request Management] — FR23: PR for blocked/failed stories with partial code
- [Source: _bmad-output/project-context.md#Supervisor Hybrid Pattern] — Rule engine → LLM fallback → escalate, mark needs-clarification, notify human
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — Supervisor must never invent answers, no silent failures
- [Source: _bmad-output/project-context.md#Sequential Execution] — One story at a time, daemon moves to next eligible story after escalation
- [Source: _bmad-output/project-context.md#Resilience Rules] — No work lost on unexpected shutdown, crash recovery produces clean state
- [Source: _bmad-output/implementation-artifacts/3-2-llm-fallback-with-project-context.md#Previous Story Intelligence] — Story 3.1 and 3.2 implementation details, SupervisorError variants, call() pipeline
- [Source: _bmad-output/implementation-artifacts/3-2-llm-fallback-with-project-context.md#call() Pipeline] — Complete call() flow ending in EscalationRequired
- [Source: _bmad-output/implementation-artifacts/3-2-llm-fallback-with-project-context.md#Integration with Future Stories] — Story 3.3 refines escalation path, session catches error

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — clean implementation with no blocking issues.

### Completion Notes List

- **Task 0:** All prerequisites verified — `SupervisorError::EscalationRequired` exists, `call()` pipeline terminates in escalation, `SupervisorError` is Send+Sync, session/watcher modules exist, `cargo check` clean.
- **Task 1:** Created `src/session/escalation.rs` with `EscalationInfo` (Debug, Clone, PartialEq, Eq) and `EscalationReport` (Debug, Clone, Serialize, Deserialize, PartialEq, Eq). `EscalationReport::new()` sets `escalated_at` via `chrono::Utc::now().to_rfc3339()`. `Display` impl produces human-readable summary. All public items have `///` doc comments.
- **Task 2:** Created `SessionError` thiserror enum in `src/session/mod.rs` with variants: `SupervisorEscalation`, `ChatFailed`, `ToolError`, `StateFileFailed`, `GitError`. Verified Send+Sync+Error via dedicated tests.
- **Task 3:** Added `EscalationSlot` type alias (`Arc<Mutex<Option<EscalationInfo>>>`) to `src/supervisor/mod.rs`. Added `escalation_slot` field to `AskSupervisor` with `#[serde(skip)]`. Updated `call()` to write `EscalationInfo` to slot before returning `EscalationRequired`. Added `with_answer_provider_and_slot()` and `escalation_slot()` accessors. Session chat loop detection pattern is implemented and tested.
- **Task 4:** Created `src/session/cleanup.rs` with `preserve_partial_work()` returning `String` (never `Result`). Uses git2 for repo open, status check, stage-all, commit. Every git2 op wrapped in match — failures logged via `tracing::error!()` and fallback summary returned. WIP commit message includes the escalation question.
- **Task 5:** Implemented `mark_story_needs_clarification()` using regex string-based replacement to preserve YAML comments. Updated STATUS DEFINITIONS in `sprint-status.yaml` with `needs-clarification` status and Story Status Transitions section.
- **Task 6:** Verified watcher already filters only `ready-for-dev` via `is_eligible()`. `needs-clarification` already in `BLOCKING_STATUSES` in `deps.rs` for cascade blocking. Added explicit `tracing::debug!()` skip log in `Watcher::poll()` for observability.
- **Task 7:** Defined `SessionOutcome` enum with `Completed`, `Escalated(EscalationReport)`, `Failed` variants. Orchestration pattern documented and tested via integration tests — status update failure does NOT block report generation.
- **Task 8:** 44 new tests across 4 files: 13 in `escalation.rs`, 11 in `cleanup.rs` (including 2 integration flow tests), 13 in `session/mod.rs`, 5 in `supervisor/mod.rs`, 2 in `watcher/mod.rs`. All 278 tests pass (234 pre-existing + 44 new).
- **Task 9:** `cargo fmt` clean, `cargo clippy` clean (only pre-existing dead_code warnings), no `unwrap()`/`expect()` in production code, no `println!`/`eprintln!`, tracing only, no secrets logged. Added `regex = "1"` dependency to `Cargo.toml`.

### File List

- `src/session/escalation.rs` — **CREATED** — `EscalationInfo`, `EscalationReport` structs, Display impl, tests
- `src/session/cleanup.rs` — **CREATED** — `preserve_partial_work()`, `mark_story_needs_clarification()`, tests
- `src/session/mod.rs` — **MODIFIED** — Added `pub mod escalation;`, `pub mod cleanup;`, `SessionError` enum, `SessionOutcome` enum, tests
- `src/supervisor/mod.rs` — **MODIFIED** — Added `EscalationSlot` type alias, `escalation_slot` field, `with_answer_provider_and_slot()`, `escalation_slot()`, slot write in `call()`, tests
- `src/watcher/mod.rs` — **MODIFIED** — Added `needs-clarification` skip log in `poll()`, added `test_story_info_is_not_eligible_needs_clarification` and `test_sprint_status_eligible_stories_excludes_needs_clarification` tests
- `Cargo.toml` — **MODIFIED** — Added `regex = "1"` dependency
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — **MODIFIED** — Added `needs-clarification` to STATUS DEFINITIONS, added Story Status Transitions section, updated story 3-3 status
- `_bmad-output/implementation-artifacts/3-3-human-escalation.md` — **MODIFIED** — All tasks marked [x], Dev Agent Record populated, File List updated