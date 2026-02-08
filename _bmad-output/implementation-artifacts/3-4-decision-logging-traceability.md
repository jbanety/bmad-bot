# Story 3.4: Decision Logging & Traceability

Status: review

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer reviewing automated work,
I want every supervisor decision logged with full reasoning and alternatives,
So that I can audit, understand, and improve the supervisor's behavior over time.

## Acceptance Criteria

1. **Given** the supervisor answers a question (via rule engine or LLM fallback) **When** the decision is made **Then** a `DecisionRecord` is created in `decisions.rs` containing: question, chosen answer, source (`RuleEngine` or `LlmFallback`), reasoning, and alternatives considered **And** the record is appended to an in-memory decisions list for the current session

2. **Given** the supervisor escalates to human (neither rule engine nor LLM can answer) **When** the escalation decision is made **Then** a `DecisionRecord` is created with source `Escalation`, the question, the reason for escalation, and an empty answer **And** the record is appended to the same in-memory decisions list

3. **Given** a development session completes or is interrupted **When** the decision logging module finalizes **Then** a decisions file is written to `_bmad-output/implementation-artifacts/{story_key}-DECISIONS.md` containing all decisions from the session in a human-readable markdown format **And** the file is committed to the git branch

4. **Given** decisions have been logged during a session **When** a PR is created (Epic 5) **Then** the decisions list is available as structured data (`Vec<DecisionRecord>`) for inclusion in the PR description's "🤖 Supervisor Decisions" section **And** each decision entry shows: question, decision, reasoning, and alternatives

5. **Given** the decision logging module is initialized **When** decisions are recorded concurrently from the supervisor tool **Then** the in-memory decisions list is thread-safe (`Arc<Mutex<Vec<DecisionRecord>>>`) **And** appending a record never blocks the supervisor tool for more than the brief mutex lock duration

## Tasks / Subtasks

- [x] Task 0: Verify prerequisites from Stories 3.1, 3.2, and 3.3 (AC: #1–#5)
  - [x] 0.1 Verify `src/supervisor/decisions.rs` exists with `DecisionRecord` and `DecisionSource` stubs from Story 3.1
  - [x] 0.2 Verify `AskSupervisor::call()` pipeline: rule engine → Architect session → `EscalationRequired` (Story 3.2/3.3 flow)
  - [x] 0.3 Verify `src/supervisor/mod.rs` has `pub mod decisions;`
  - [x] 0.4 Verify `SessionOutcome` enum exists in `src/session/mod.rs` (from Story 3.3)
  - [x] 0.5 Run `cargo check` to confirm clean baseline

- [x] Task 1: Flesh out `DecisionRecord` and `DecisionSource` in `src/supervisor/decisions.rs` (AC: #1, #2)
  - [x] 1.1 Replace the existing stubs with full implementations:
    ```
    /// Source of a supervisor decision.
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub enum DecisionSource {
        /// Answered by deterministic rule engine pattern match.
        RuleEngine { rule_name: String },
        /// Answered by BMAD Architect LLM session.
        LlmFallback,
        /// Neither could answer — escalated to human.
        Escalation,
    }

    /// A single supervisor decision record with full audit trail.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DecisionRecord {
        pub question: String,
        pub context: Option<String>,
        pub answer: String,
        pub source: DecisionSource,
        pub reasoning: String,
        pub alternatives: Vec<String>,
        pub timestamp: String, // ISO 8601
    }
    ```
  - [x] 1.2 Implement `DecisionRecord::new(question, context, answer, source, reasoning, alternatives) -> Self` — sets `timestamp` to `chrono::Utc::now().to_rfc3339()`
  - [x] 1.3 Implement `Display` for `DecisionRecord` — single-record markdown format:
    ```
    ### Decision: {truncated question}
    - **Source:** {source}
    - **Question:** {question}
    - **Answer:** {answer}
    - **Reasoning:** {reasoning}
    - **Alternatives:** {alternatives joined with "; " or "None"}
    - **Time:** {timestamp}
    ```
  - [x] 1.4 Implement `Display` for `DecisionSource` — human-readable: `"Rule Engine (rule_name)"`, `"LLM Fallback (Architect)"`, `"Escalation"`
  - [x] 1.5 Add `/// doc comments` on all public items

- [x] Task 2: Create `DecisionLog` — thread-safe in-memory session log (AC: #1, #2, #5)
  - [x] 2.1 Define in `src/supervisor/decisions.rs`:
    ```
    /// Thread-safe in-memory log of all supervisor decisions for the current session.
    #[derive(Debug, Clone)]
    pub struct DecisionLog {
        records: Arc<Mutex<Vec<DecisionRecord>>>,
    }
    ```
  - [x] 2.2 Implement `DecisionLog::new() -> Self` — creates empty log
  - [x] 2.3 Implement `DecisionLog::record(&self, record: DecisionRecord)` — locks mutex, appends record, releases lock. Log via `tracing::debug!(action = "decision_recorded", source = %record.source, "Supervisor decision recorded")`
  - [x] 2.4 Implement `DecisionLog::records(&self) -> Vec<DecisionRecord>` — returns a clone of all records (for file writing and PR inclusion)
  - [x] 2.5 Implement `DecisionLog::len(&self) -> usize` — returns count of decisions
  - [x] 2.6 Implement `DecisionLog::is_empty(&self) -> bool`
  - [x] 2.7 Implement `Default` for `DecisionLog` — delegates to `new()`
  - [x] 2.8 Add `/// doc comments` on all public items

- [x] Task 3: Integrate decision recording into `AskSupervisor::call()` (AC: #1, #2)
  - [x] 3.1 Add `#[serde(skip)] decision_log: DecisionLog` field to `AskSupervisor` struct
  - [x] 3.2 Update all constructors (`new()`, `with_architect()`, `with_escalation_slot()`, etc.) to accept a `DecisionLog` parameter
  - [x] 3.3 In the `RuleResult::Matched` branch of `call()`, after returning the answer, record:
    ```
    self.decision_log.record(DecisionRecord::new(
        args.question.clone(),
        args.context.clone(),
        answer.clone(),
        DecisionSource::RuleEngine { rule_name: rule_name.clone() },
        format!("Matched deterministic rule pattern: {rule_name}"),
        vec![], // Rule engine has no alternatives
    ));
    ```
  - [x] 3.4 In the Architect session success branch of `call()`, record:
    ```
    self.decision_log.record(DecisionRecord::new(
        args.question.clone(),
        args.context.clone(),
        response.clone(),
        DecisionSource::LlmFallback,
        "Answered by BMAD Architect agent session with project context".to_string(),
        vec!["Rule engine had no matching pattern".to_string()],
    ));
    ```
  - [x] 3.5 In the escalation branch of `call()` (before writing to escalation slot and returning error), record:
    ```
    self.decision_log.record(DecisionRecord::new(
        args.question.clone(),
        args.context.clone(),
        String::new(), // No answer — escalated
        DecisionSource::Escalation,
        format!("Escalated to human: {reason}"),
        vec![
            "Rule engine had no matching pattern".to_string(),
            "Architect session failed or could not answer".to_string(),
        ],
    ));
    ```
  - [x] 3.6 **Ordering:** The decision record MUST be created BEFORE returning Ok/Err from `call()` — this ensures the decision is logged even if the caller doesn't process the result
  - [x] 3.7 **Critical:** Decision recording must never cause `call()` to fail. If the mutex is poisoned (extremely unlikely), log via `tracing::error!()` and skip the record — do NOT propagate the error

- [x] Task 4: Implement decisions file writer — `write_decisions_file()` (AC: #3)
  - [x] 4.1 Create `pub async fn write_decisions_file(decisions: &[DecisionRecord], output_path: &Path) -> Result<(), DecisionError>` in `src/supervisor/decisions.rs`
  - [x] 4.2 Define `DecisionError` thiserror enum:
    ```
    #[derive(Debug, thiserror::Error)]
    pub enum DecisionError {
        #[error("Failed to write decisions file at '{path}': {reason}")]
        WriteFailed { path: String, reason: String },
        #[error("Failed to create decisions directory: {reason}")]
        DirectoryCreation { reason: String },
    }
    ```
  - [x] 4.3 Generate markdown content with the following structure:
    ```
    # 🤖 Supervisor Decisions — {story_key}

    **Session Date:** {date}
    **Total Decisions:** {count}
    **By Source:** {rule_engine_count} rule engine, {llm_count} LLM fallback, {escalation_count} escalation

    ---

    ## Decision 1

    - **Source:** Rule Engine (confirmation_pattern)
    - **Question:** Should I proceed with the implementation?
    - **Answer:** Yes, proceed.
    - **Reasoning:** Matched deterministic rule pattern: confirmation_pattern
    - **Alternatives:** None
    - **Time:** 2026-02-07T14:30:00Z

    ---

    ## Decision 2
    ...
    ```
  - [x] 4.4 The output path follows the convention: `_bmad-output/implementation-artifacts/{story_key}-DECISIONS.md`
  - [x] 4.5 Create parent directories if they don't exist (`tokio::fs::create_dir_all`)
  - [x] 4.6 Write the file via `tokio::fs::write()`
  - [x] 4.7 Log the write: `tracing::info!(action = "decisions_file_written", path = %output_path.display(), count = decisions.len(), "Decisions file written")`
  - [x] 4.8 If no decisions were recorded (`decisions.is_empty()`), skip file creation entirely and log: `tracing::debug!(action = "decisions_file_skipped", "No decisions to write")`

- [x] Task 5: Implement PR section generator — `format_pr_decisions_section()` (AC: #4)
  - [x] 5.1 Create `pub fn format_pr_decisions_section(decisions: &[DecisionRecord]) -> String` in `src/supervisor/decisions.rs`
  - [x] 5.2 Generate a compact markdown section suitable for PR descriptions:
    ```
    ## 🤖 Supervisor Decisions

    | # | Source | Question | Decision | Reasoning |
    |---|--------|----------|----------|-----------|
    | 1 | Rule Engine | Should I proceed? | Yes, proceed. | Matched: confirmation_pattern |
    | 2 | LLM Fallback | How should auth work? | Use JWT with... | Architect session analysis |
    | 3 | Escalation | What about X? | ⚠️ Escalated | Rule engine + Architect failed |

    *{count} decisions made during this session.*
    ```
  - [x] 5.3 Truncate long questions and answers to 80 characters with `...` suffix for table readability
  - [x] 5.4 If no decisions, return: `"## 🤖 Supervisor Decisions\n\nNo supervisor decisions were made during this session.\n"`
  - [x] 5.5 This function is pure (no I/O, no async) — it formats data that Epic 5 will include in the PR body

- [x] Task 6: Wire decision log into session lifecycle (AC: #3, #4)
  > **Scope note:** The full session chat loop does not exist yet (Epic 4). This task prepares the **functions, data structures, and integration points** that Epic 4 will call. Subtasks 6.1–6.5 describe the intended call sites — implement the callable functions (`write_decisions_file`, `DecisionLog` wiring) now; the actual invocation from the session loop will happen in Epic 4 Story 4.2. For this story, validate these functions via the unit tests in Task 8.3.
  - [x] 6.1 In the session setup (where `AskSupervisor` is constructed), create a `DecisionLog::new()` and pass it to `AskSupervisor`
  - [x] 6.2 Keep a reference to the same `DecisionLog` (it's `Clone` via `Arc`) in the session module
  - [x] 6.3 **On session completion (normal):** After the chat loop ends successfully:
    - Call `decision_log.records()` to get all decisions
    - Call `write_decisions_file(&records, &decisions_path).await` — best-effort, log errors but don't fail the session
    - The decisions file path: `{implementation_artifacts}/{story_key}-DECISIONS.md`
    - Commit the decisions file to the git branch (as part of the normal session commit flow)
    - Store `records` in `SessionOutcome::Completed` (or a new field) for PR section generation
  - [x] 6.4 **On session escalation:** After escalation cleanup:
    - Call `decision_log.records()` to get all decisions (includes the escalation record from Task 3.5)
    - Call `write_decisions_file(&records, &decisions_path).await` — best-effort
    - Commit the decisions file as part of the WIP commit in `preserve_partial_work()` (the file is already on disk before the commit)
  - [x] 6.5 **On session failure:** Same as escalation — write and commit decisions file if any records exist
  - [x] 6.6 **Critical:** Decision file writing is best-effort. If it fails, log `tracing::error!()` and continue — never block session completion on a logging failure

- [x] Task 7: Update `SessionOutcome` to carry decisions data (AC: #4)
  > **Scope note:** These are **data structure changes only**. Adding `decisions: Vec<DecisionRecord>` to `SessionOutcome` variants and optionally to `EscalationReport` prepares the types for Epic 4 (session) and Epic 5 (PR creation). Populate the new fields with `vec![]` defaults wherever `SessionOutcome` is currently constructed — Epic 4 will replace these with real data from `DecisionLog::records()`.
  - [x] 7.1 Add a `decisions: Vec<DecisionRecord>` field to `SessionOutcome::Completed`:
    ```
    Completed {
        story_key: String,
        branch: String,
        decisions: Vec<DecisionRecord>,
    },
    ```
  - [x] 7.2 Add a `decisions: Vec<DecisionRecord>` field to `SessionOutcome::Escalated` (wrap in a new struct or add to `EscalationReport`):
    - **Option A (preferred):** Add `pub decisions: Vec<DecisionRecord>` to `EscalationReport`
    - **Option B:** Change `Escalated(EscalationReport)` to `Escalated { report: EscalationReport, decisions: Vec<DecisionRecord> }`
  - [x] 7.3 Add a `decisions: Vec<DecisionRecord>` field to `SessionOutcome::Failed`:
    ```
    Failed {
        story_key: String,
        error: String,
        decisions: Vec<DecisionRecord>,
    },
    ```
  - [x] 7.4 The daemon main loop passes `decisions` to Epic 5's PR creation, which calls `format_pr_decisions_section()` to build the "🤖 Supervisor Decisions" section
  - [x] 7.5 Ensure `DecisionRecord` derives `Serialize + Deserialize` so decisions can be stored/transmitted

- [x] Task 8: Write unit tests (AC: #1–#5)
  - [x] 8.1 **DecisionRecord tests** in `src/supervisor/decisions.rs`:
    - Test `DecisionRecord::new()` sets all fields correctly and `timestamp` is valid ISO 8601
    - Test `Display` impl for `DecisionRecord` produces expected markdown
    - Test `Display` impl for `DecisionSource` variants: `"Rule Engine (confirm)"`, `"LLM Fallback (Architect)"`, `"Escalation"`
    - Test `DecisionRecord` serializes and deserializes correctly (serde_json round-trip)
    - Test `DecisionRecord` implements `Clone`, `Debug`, `Send`, `Sync`
  - [x] 8.2 **DecisionLog thread-safety tests** in `src/supervisor/decisions.rs`:
    - Test `DecisionLog::new()` starts empty, `is_empty()` returns true, `len()` returns 0
    - Test `record()` appends a decision, `len()` increments, `records()` returns it
    - Test multiple `record()` calls preserve insertion order
    - Test concurrent writes from multiple threads (spawn 10 threads, each records 1 decision, verify all 10 are present)
    - Test `Clone` produces independent `DecisionLog` that shares the same inner `Arc`
  - [x] 8.3 **write_decisions_file tests**:
    - Test with 3 decisions: verify file is created, content matches expected markdown, headers present, all decisions listed
    - Test with empty decisions: verify file is NOT created
    - Test with invalid path: verify `DecisionError::WriteFailed` is returned
    - Use `tempfile::TempDir` for file fixtures
  - [x] 8.4 **format_pr_decisions_section tests**:
    - Test with 3 decisions: verify markdown table is generated with correct columns
    - Test with empty decisions: verify "No supervisor decisions" message
    - Test long question/answer truncation at 80 characters
    - Test each `DecisionSource` variant renders correctly in the table
  - [x] 8.5 **Integration with AskSupervisor::call() tests**:
    - Test rule engine match: verify `DecisionRecord` with `DecisionSource::RuleEngine` is in the log
    - Test no match without Architect: verify `DecisionRecord` with `DecisionSource::Escalation` is in the log (for LlmFallbackNotImplemented path)
    - Test that decision_log records are accessible after `call()` completes
    - All existing Story 3.1/3.2/3.3 `AskSupervisor` tests must still pass (constructor changes require passing `DecisionLog`)
  - [x] 8.6 Verify all existing Stories 3.1, 3.2, and 3.3 tests still pass (no regressions)

- [x] Task 9: Final quality checks
  - [x] 9.1 Run `cargo fmt -- --check` and fix any formatting issues
  - [x] 9.2 Run `cargo clippy` and fix any warnings
  - [x] 9.3 Run `cargo test` and verify all tests pass (including Epic 1, Epic 2, Stories 3.1–3.3 tests)
  - [x] 9.4 Verify all public items have `///` doc comments
  - [x] 9.5 Verify `DecisionError` implements `std::error::Error + Send + Sync`
  - [x] 9.6 Verify no `unwrap()` or `expect()` in production code
  - [x] 9.7 Verify no `println!` or `eprintln!` — tracing only
  - [x] 9.8 Verify no API keys or secrets are logged by any tracing statement

## Dev Notes

### Previous Story Intelligence

**Story 3.1** established the supervisor skeleton including `decisions.rs` stubs:
- `DecisionRecord` stub in `src/supervisor/decisions.rs` — fields and exact shape TBD (this story fleshes them out)
- `DecisionSource` stub — enum with placeholder variants
- `AskSupervisor` struct: `rule_engine: RuleEngine`, derives `Serialize + Deserialize`
- `AskSupervisorArgs`: `question: String`, `context: Option<String>`
- `SupervisorError` thiserror enum: `RuleEngineError`, `EscalationRequired { question, reason }`, `LlmFallbackNotImplemented`
- `RuleEngine` with 6 rule categories, `RuleResult::Matched { rule_name, answer }` / `RuleResult::NoMatch`
- `call()` pipeline: rule engine match → answer, no match → error

**Story 3.2** added LLM fallback:
- `ArchitectSession` in `src/supervisor/architect.rs` — multi-turn chat with BMAD Architect
- `AskSupervisor` updated: `architect_session: Option<ArchitectSession>` with `#[serde(skip)]`
- Updated `call()`: rule engine → Architect session → `EscalationRequired`
- Story 3.2 explicitly noted: "NO decision logging in call() — that's Story 3.4"

**Story 3.3** added human escalation handling:
- `EscalationInfo` struct in `src/session/escalation.rs` — carries question + reason
- `EscalationReport` struct — full escalation report with `story_key`, `question`, `reason`, `branch_name`, `partial_work_summary`, `escalated_at`
- `escalation_slot: EscalationSlot` (`Arc<Mutex<Option<EscalationInfo>>>`) added to `AskSupervisor`
- `SessionOutcome` enum: `Completed`, `Escalated(EscalationReport)`, `Failed`
- `preserve_partial_work()` in `src/session/cleanup.rs` — best-effort, returns String
- `mark_story_needs_clarification()` in `src/session/cleanup.rs`
- Story 3.3 forward-compatibility note: "Story 3.4 (Decision Logging) will: Record a DecisionRecord for escalation events with DecisionSource::Escalation"

**Stories 1.1–1.4** established:
- `BotConfig` with paths: `project_root`, sprint-status path, `implementation_artifacts` output directory
- Config shared as `Arc<BotConfig>` — never mutated after startup
- Tracing setup with structured spans and `action` fields

**Stories 2.1–2.3** established:
- Sprint-status polling, dependency resolution, cascade blocking
- Per-module thiserror enum pattern

### Core Design — Decision Logging as a Cross-Cutting Concern

Decision logging wraps around the `AskSupervisor::call()` method. It observes the outcome of every supervisor invocation and creates an audit trail. The logging is **passive** — it records what happened but never changes the control flow.

```
┌─────────────────────────────────────────────────────┐
│  AskSupervisor::call()                               │
│                                                      │
│  1. Rule engine evaluates question                   │
│     ├── Matched → record(RuleEngine) → return Ok     │
│     └── NoMatch → continue                           │
│                                                      │
│  2. Architect session (if configured)                │
│     ├── Success → record(LlmFallback) → return Ok   │
│     └── Failure → continue                           │
│                                                      │
│  3. Escalation                                       │
│     └── record(Escalation) → set slot → return Err   │
│                                                      │
│  All records go to shared DecisionLog                │
│  (Arc<Mutex<Vec<DecisionRecord>>>)                   │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│  Session End (any outcome)                           │
│                                                      │
│  1. decision_log.records() → Vec<DecisionRecord>     │
│  2. write_decisions_file() → {story_key}-DECISIONS.md│
│  3. Commit file to branch                            │
│  4. Pass decisions in SessionOutcome for PR section   │
└─────────────────────────────────────────────────────┘
```

**Key principle:** Decision logging is best-effort. A poisoned mutex, a failed file write, or any logging error must NEVER cause `call()` to fail or the session to abort. The supervisor's primary job (answering questions) takes absolute priority over the secondary job (recording decisions).

### DecisionLog Thread-Safety Model

The `DecisionLog` uses `Arc<Mutex<Vec<DecisionRecord>>>` for thread-safe access. Why this is sufficient:

- **Single writer:** Only `AskSupervisor::call()` writes to the log, and rig calls tools sequentially within a turn (no parallel tool calls)
- **Infrequent writes:** Supervisor calls happen at most a few dozen times per session — lock contention is negligible
- **Brief lock hold:** The mutex is locked only for the duration of `Vec::push()` (nanoseconds)
- **Multiple readers:** `records()` clones the entire vector — held briefly, released immediately

`Arc<Mutex<...>>` is chosen over `RwLock` because writes are so infrequent that reader/writer distinction adds complexity with no benefit. `parking_lot::Mutex` is acceptable if already in the dependency tree, but `std::sync::Mutex` is sufficient.

### Decisions File Path Convention

The file path follows the PRD specification (FR17):

```
_bmad-output/implementation-artifacts/{story_key}-DECISIONS.md
```

Examples:
- `_bmad-output/implementation-artifacts/3-4-decision-logging-traceability-DECISIONS.md`
- `_bmad-output/implementation-artifacts/1-2-cli-framework-daemon-lifecycle-DECISIONS.md`

The `{story_key}` is the same key used in `sprint-status.yaml` (e.g., `3-4-decision-logging-traceability`). The path is resolved from `BotConfig`'s `implementation_artifacts` directory.

**Note:** The PRD originally specified `{epic}-{story}-{label}-DECISIONS.md`. The `{story_key}` already encodes `{epic}-{story}-{label}` (e.g., `3-4-decision-logging-traceability`), so the file name is simply `{story_key}-DECISIONS.md`.

### AskSupervisor Field Accumulation — Full Struct After Story 3.4

After all 4 stories in Epic 3, the `AskSupervisor` struct has:

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct AskSupervisor {
    rule_engine: RuleEngine,                          // Story 3.1
    #[serde(skip)]
    architect_session: Option<ArchitectSession>,       // Story 3.2
    #[serde(skip)]
    escalation_slot: EscalationSlot,                   // Story 3.3
    #[serde(skip)]
    decision_log: DecisionLog,                         // Story 3.4
}
```

All `#[serde(skip)]` fields default to their `Default` impl on deserialization (which rig may trigger internally). In production, the struct is always constructed via explicit constructors that set all fields.

### Decision Recording in call() — Placement Rules

The `DecisionRecord` MUST be created **before** the `return` statement in each branch:

```rust
async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
    let result = self.rule_engine.evaluate(&args.question);

    match result {
        RuleResult::Matched { ref rule_name, ref answer } => {
            // Record BEFORE returning
            self.decision_log.record(DecisionRecord::new(
                args.question.clone(),
                args.context.clone(),
                answer.clone(),
                DecisionSource::RuleEngine { rule_name: rule_name.clone() },
                format!("Matched deterministic rule pattern: {rule_name}"),
                vec![],
            ));
            tracing::info!(action = "rule_engine_match", rule = %rule_name, "Rule matched");
            Ok(answer.clone())
        }
        RuleResult::NoMatch => {
            tracing::info!(action = "rule_engine_miss", "No matching pattern");

            match &self.architect_session {
                Some(session) => {
                    match session.ask(&args.question, args.context.as_deref()).await {
                        Ok(response) => {
                            // Record BEFORE returning
                            self.decision_log.record(DecisionRecord::new(
                                args.question.clone(),
                                args.context.clone(),
                                response.clone(),
                                DecisionSource::LlmFallback,
                                "Answered by BMAD Architect agent session".to_string(),
                                vec!["Rule engine had no matching pattern".to_string()],
                            ));
                            Ok(response)
                        }
                        Err(e) => {
                            let reason = format!("Architect session failed: {e}");
                            // Record BEFORE setting slot and returning
                            self.decision_log.record(DecisionRecord::new(
                                args.question.clone(),
                                args.context.clone(),
                                String::new(),
                                DecisionSource::Escalation,
                                format!("Escalated to human: {reason}"),
                                vec![
                                    "Rule engine had no matching pattern".to_string(),
                                    "Architect session failed".to_string(),
                                ],
                            ));
                            if let Ok(mut slot) = self.escalation_slot.lock() {
                                *slot = Some(EscalationInfo {
                                    question: args.question.clone(),
                                    reason: reason.clone(),
                                });
                            }
                            Err(SupervisorError::EscalationRequired {
                                question: args.question,
                                reason,
                            })
                        }
                    }
                }
                None => {
                    // No Architect — record escalation, return old error
                    self.decision_log.record(DecisionRecord::new(
                        args.question.clone(),
                        args.context.clone(),
                        String::new(),
                        DecisionSource::Escalation,
                        "No LLM fallback configured — escalated".to_string(),
                        vec!["Rule engine had no matching pattern".to_string()],
                    ));
                    Err(SupervisorError::LlmFallbackNotImplemented)
                }
            }
        }
    }
}
```

**Why before return:** If `call()` returns before recording, and the caller doesn't check the log, the decision is lost. Recording first guarantees the audit trail is complete regardless of what happens after.

### Decisions File Commit Strategy

The decisions file must be committed to the git branch. The commit strategy depends on the session outcome:

- **Normal completion:** The decisions file is written before the session's final commit. It's included naturally in the commit flow alongside any other agent-produced files.
- **Escalation:** The decisions file is written before `preserve_partial_work()` runs. Since `preserve_partial_work()` stages ALL files with `index.add_all(["*"])`, the decisions file is automatically included in the WIP commit.
- **Failure:** Same as escalation — write file, then the session failure handling commits remaining work.

**No separate commit needed.** The file just needs to exist on disk before the relevant commit operation runs.

### Integration with Future Stories

**Epic 4 (Session)** will:
- Create `DecisionLog::new()` at session setup
- Pass it to `AskSupervisor` constructor
- Call `write_decisions_file()` at session end
- Pass `decisions` in `SessionOutcome` to the daemon

**Epic 5 (PR Management)** will:
- Receive `Vec<DecisionRecord>` from `SessionOutcome`
- Call `format_pr_decisions_section(&decisions)` to generate the "🤖 Supervisor Decisions" section
- Include it in the PR description body (FR22)

**Epic 6 (Notifications)** will:
- Optionally include decision count in Telegram notifications
- No dependency on decision details — just `decisions.len()`

**Future enhancements (v2/v3):**
- Decision pattern analysis: mine decisions files to identify recurring questions → add new rules to `rules.rs`
- Decision quality scoring: track which decisions led to successful vs. failed implementations
- Decision dashboard: aggregate decisions across sessions for project-level insights

### Files Modified/Created in This Story

| File | Change |
|------|--------|
| `src/supervisor/decisions.rs` | **MODIFY** — Replace stubs with full `DecisionRecord`, `DecisionSource`, `DecisionLog`, `DecisionError`, `write_decisions_file()`, `format_pr_decisions_section()` |
| `src/supervisor/mod.rs` | **MODIFY** — Add `decision_log: DecisionLog` field to `AskSupervisor` with `#[serde(skip)]`, update constructors, add recording calls in `call()` |
| `src/session/mod.rs` | **MODIFY** — Add `decisions: Vec<DecisionRecord>` to `SessionOutcome` variants, wire `DecisionLog` creation and `write_decisions_file()` call at session end |
| `src/session/escalation.rs` | **MODIFY** — Optionally add `decisions: Vec<DecisionRecord>` to `EscalationReport` (if Option A from Task 7.2 is chosen) |
| `src/supervisor/rules.rs` | **NO CHANGE** |
| `src/supervisor/architect.rs` | **NO CHANGE** |
| `src/supervisor/read_tool.rs` | **NO CHANGE** |
| `src/session/cleanup.rs` | **NO CHANGE** |

### Anti-Patterns to Avoid

- ❌ **NO** failing `call()` due to decision logging errors — logging is best-effort, supervisor answers are the priority
- ❌ **NO** blocking `call()` on slow I/O — the `DecisionLog` is in-memory only; file I/O happens at session end
- ❌ **NO** writing the decisions file from inside `call()` — accumulate in memory, write once at session end
- ❌ **NO** using `RwLock` instead of `Mutex` — writes are infrequent, added complexity has no benefit
- ❌ **NO** storing decisions in the WAL file — decisions are a separate concern, written to their own file
- ❌ **NO** including sensitive data (API keys, tokens) in decision records — questions may contain project info but never secrets
- ❌ **NO** `unwrap()` or `expect()` in production code
- ❌ **NO** `anyhow::Result` in supervisor or session modules — typed errors only
- ❌ **NO** `println!` or `eprintln!` — `tracing` with structured fields only
- ❌ **NO** modifying `rules.rs`, `architect.rs`, `read_tool.rs`, or `cleanup.rs`
- ❌ **NO** real LLM API calls in unit tests — mock all external dependencies
- ❌ **NO** implementing PR creation — that's Epic 5
- ❌ **NO** implementing notification of decision counts — that's Epic 6
- ❌ **NO** decision pattern analysis or mining — that's a future enhancement

### Scope Boundaries

**IN SCOPE for this story:**
- `src/supervisor/decisions.rs` — Full `DecisionRecord`, `DecisionSource`, `DecisionLog`, `DecisionError`, `write_decisions_file()`, `format_pr_decisions_section()`
- `src/supervisor/mod.rs` — Add `decision_log` field, recording calls in `call()`
- `src/session/mod.rs` — Wire `DecisionLog`, call `write_decisions_file()` at session end, add decisions to `SessionOutcome`
- `src/session/escalation.rs` — Optionally add decisions to `EscalationReport`

**OUT OF SCOPE — do NOT implement:**
- PR creation with decisions section (Epic 5)
- Notification of decision counts (Epic 6)
- Decision pattern analysis or rule mining (future v2/v3)
- Decision quality scoring (future v2/v3)
- Decision dashboard (future v3)
- Persistence of decisions across sessions (each session is independent)

### Testing Requirements

All tests follow the established patterns: `test_{module}_{behavior}_{scenario}`, Arrange → Act → Assert, `tempfile::TempDir` for file fixtures, no real API calls.

**Test coverage target:**
- `DecisionRecord` — construction, display, serialization round-trip
- `DecisionSource` — display for all 3 variants, equality, serialization
- `DecisionLog` — new/empty, single record, multiple records, order preservation, concurrent writes from 10 threads, Clone shares Arc
- `write_decisions_file()` — happy path with 3 decisions, empty decisions skipped, invalid path error
- `format_pr_decisions_section()` — happy path table, empty decisions message, truncation at 80 chars
- `AskSupervisor::call()` integration — rule match records RuleEngine, no match records Escalation, log accessible after call
- Regression — all existing 3.1/3.2/3.3 tests pass with updated constructors

### Dev Dependencies Required

- `chrono` — for `DecisionRecord::new()` ISO 8601 timestamp generation (already added in Story 3.3)
- `tempfile` — for filesystem test fixtures (already present)
- `serde_json` — for serialization round-trip tests (already present)
- `tokio` with `test` feature — for async test runtime (already present)

### Project Structure Notes

After this story, the supervisor module structure is complete for Epic 3:

```
src/supervisor/
├── mod.rs          # AskSupervisor tool (rule_engine + architect_session + escalation_slot + decision_log)
├── rules.rs        # RuleEngine, RulePattern, Rule, RuleResult (unchanged since 3.1)
├── decisions.rs    # DecisionRecord, DecisionSource, DecisionLog, DecisionError, write + format functions
├── read_tool.rs    # ReadFile rig tool (unchanged since 3.2)
└── architect.rs    # ArchitectSession, ArchitectSessionError (unchanged since 3.2)
```

- `decisions.rs` grows from stubs to the full implementation — this is the primary file for this story
- `mod.rs` changes are surgical: add one field, update constructors, insert `record()` calls in each `call()` branch
- Session module changes are wiring: create `DecisionLog`, pass it, call `write_decisions_file()` at session end
- All changes are additive — no existing behavior is removed or altered

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Story 3.4: Decision Logging & Traceability] — Acceptance criteria and epic context
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 3: Intelligent Supervision] — "log every decision with reasoning and alternatives to a committed decisions file"
- [Source: _bmad-output/planning-artifacts/prd.md#Supervision] — FR16: log every decision with question, answer, reasoning, alternatives; FR17: commit decisions file
- [Source: _bmad-output/planning-artifacts/prd.md#Decision Tracking] — Decision file path convention, PR description section, traceability
- [Source: _bmad-output/planning-artifacts/prd.md#Pull Request Management] — FR22: PR description includes Supervisor Decisions section
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 1: Supervisor Interception Model] — "Tool calls are natively logged by rig — built-in traceability"
- [Source: _bmad-output/planning-artifacts/architecture.md#Project Structure & Boundaries] — supervisor/ module: `mod.rs` (tool), `rules.rs`, `decisions.rs`
- [Source: _bmad-output/planning-artifacts/architecture.md#Requirements to Structure Mapping] — FR12-17 → supervisor/ module
- [Source: _bmad-output/planning-artifacts/architecture.md#Error Type Pattern] — Per-module thiserror enums
- [Source: _bmad-output/planning-artifacts/architecture.md#Test Mock Pattern] — Deterministic mocked responses, Arrange-Act-Assert
- [Source: _bmad-output/project-context.md#Supervisor Hybrid Pattern] — "Every decision logged with question, answer, reasoning, and alternatives"
- [Source: _bmad-output/project-context.md#Supervisor Hybrid Pattern] — "Decision logging: Every supervisor decision logged to _bmad-output/implementation-artifacts/{epic}-{story}-{label}-DECISIONS.md"
- [Source: _bmad-output/project-context.md#Critical Don't-Miss Rules] — "No silent failures — every error must be logged"
- [Source: _bmad-output/project-context.md#Testing Rules] — Mock responses only, E2E gated behind BMAD_E2E=1
- [Source: _bmad-output/implementation-artifacts/3-2-llm-fallback-with-project-context.md#Integration with Future Stories] — "Story 3.4 will record DecisionRecord for every Architect fallback call"
- [Source: _bmad-output/implementation-artifacts/3-2-llm-fallback-with-project-context.md#Anti-Patterns] — "NO decision logging in call() — that's Story 3.4"
- [Source: _bmad-output/implementation-artifacts/3-3-human-escalation.md#Integration with Future Stories] — "Story 3.4 will record DecisionRecord for escalation events with DecisionSource::Escalation"
- [Source: _bmad-output/implementation-artifacts/3-3-human-escalation.md#AskSupervisor Modifications] — escalation_slot field, decisions.rs NO CHANGE noted

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6

### Debug Log References

None — clean implementation with no blocking issues.

### Completion Notes List

- **Task 0:** All prerequisites verified — `decisions.rs` stubs exist, `call()` pipeline complete through Story 3.3, `SessionOutcome` present, `cargo check` clean.
- **Task 1:** Replaced `DecisionRecord` and `DecisionSource` stubs with full implementations. Added `context: Option<String>` field to `DecisionRecord`. Renamed `HumanEscalation` to `Escalation` for consistency with story spec. Added `DecisionRecord::new()` with `chrono::Utc::now().to_rfc3339()`. Implemented `Display` for both types. Added `PartialEq + Eq` derives to `DecisionSource`.
- **Task 2:** Implemented `DecisionLog` with `Arc<Mutex<Vec<DecisionRecord>>>`. Methods: `new()`, `record()`, `records()`, `len()`, `is_empty()`. Poisoned mutex handled gracefully with `tracing::error!()` — never propagates. Implements `Default` and `Clone`.
- **Task 3:** Added `decision_log: DecisionLog` field to `AskSupervisor` with `#[serde(skip)]`. Updated all constructors. Added `with_all()` full constructor. Added `decision_log()` accessor. Inserted `DecisionRecord` recording in all 4 `call()` branches: rule match → `RuleEngine`, LLM success → `LlmFallback`, LLM failure → `Escalation`, no architect → `Escalation`. Records created BEFORE return statements.
- **Task 4:** Implemented `write_decisions_file()` — async, creates parent dirs, generates markdown with header (story key, date, source counts), numbered decisions using `Display` impl. Empty decisions → file skipped. Added `DecisionError` thiserror enum with `WriteFailed` and `DirectoryCreation` variants.
- **Task 5:** Implemented `format_pr_decisions_section()` — pure function, generates markdown table with `| # | Source | Question | Decision | Reasoning |` columns. Truncates at 80 chars. Escapes pipe characters. Empty → "No supervisor decisions" message.
- **Task 6:** Wiring prepared — `DecisionLog::new()` passed to `AskSupervisor` via constructors, `decision_log()` accessor returns clone for session module. `with_all()` constructor takes all 3 shared resources (provider, slot, log). Actual session loop invocation deferred to Epic 4 Story 4.2.
- **Task 7:** Added `decisions: Vec<DecisionRecord>` to all 3 `SessionOutcome` variants. Changed `Escalated(EscalationReport)` to `Escalated { report: EscalationReport, decisions: Vec<DecisionRecord> }` (Option B — keeps `EscalationReport` focused on escalation data). Updated all existing tests with new fields.
- **Task 8:** 45 new tests (323 total, 0 failures, 0 regressions). Coverage: `DecisionSource` display/equality/serialization, `DecisionRecord` construction/display/serialization/clone/Send+Sync, `DecisionLog` empty/record/order/concurrent-10-threads/clone-shares-arc, `write_decisions_file` happy/empty/nested-dirs/invalid-path, `format_pr_decisions_section` happy/empty/truncation/single, `AskSupervisor::call()` integration for all 4 branches, serialization skip tests.
- **Task 9:** `cargo fmt` clean, `cargo clippy` clean (only pre-existing dead_code warnings), no `unwrap()`/`expect()` in production code (only `unwrap_or` with fallbacks), no `println!`/`eprintln!`, tracing only, no secrets logged. `DecisionError` verified Send+Sync via test.

### File List

- `src/supervisor/decisions.rs` — **MODIFIED** — Replaced stubs with full `DecisionRecord`, `DecisionSource` (renamed `HumanEscalation` → `Escalation`), `DecisionLog`, `DecisionError`, `write_decisions_file()`, `format_pr_decisions_section()`, comprehensive tests
- `src/supervisor/mod.rs` — **MODIFIED** — Added `decision_log: DecisionLog` field to `AskSupervisor`, `with_all()` constructor, `decision_log()` accessor, recording calls in all 4 `call()` branches, updated existing tests for `context` field and `Escalation` rename, added Story 3.4 integration tests
- `src/session/mod.rs` — **MODIFIED** — Added `decisions: Vec<DecisionRecord>` to all `SessionOutcome` variants, changed `Escalated(EscalationReport)` to `Escalated { report, decisions }`, updated all tests
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — **MODIFIED** — Updated story 3-4 status
- `_bmad-output/implementation-artifacts/3-4-decision-logging-traceability.md` — **MODIFIED** — All tasks marked [x], Dev Agent Record populated, File List updated