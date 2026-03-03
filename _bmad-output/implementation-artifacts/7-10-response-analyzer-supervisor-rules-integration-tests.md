# Story 7.10: Response Analyzer & Supervisor Rules Integration Tests

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want integration tests that verify the response analyzer and supervisor rule engine work correctly together,
So that I'm confident the chat loop handles all agent response patterns and the supervisor pipeline (rule engine → LLM fallback → escalation) functions as a cohesive unit.

## Acceptance Criteria

1. **Given** an agent response containing a completion signal (e.g., "Implementation complete. All acceptance criteria met.")
   **When** `ResponseAnalyzer::analyze()` processes it
   **Then** it returns `ResponseAction::Completed`

2. **Given** an agent response asking which story to work on (e.g., "Which story should I implement?")
   **When** `ResponseAnalyzer::analyze()` processes it
   **Then** it returns `ResponseAction::Continue` with the correct story key as the reply

3. **Given** an agent response asking for confirmation (e.g., "Should I proceed with the implementation?")
   **When** the supervisor rule engine processes it
   **Then** a deterministic "Yes, proceed." response is returned without LLM fallback

4. **Given** an agent response with a substantive question that doesn't match any rule
   **When** the supervisor rule engine processes it
   **Then** it falls through to LLM fallback (verified by checking that rules returned `RuleResult::NoMatch`)

5. **Given** an agent response indicating step-by-step detection (e.g., "I'll work through this step by step...")
   **When** `ResponseAnalyzer::analyze()` processes it
   **Then** it returns `ResponseAction::Continue` with a directive to skip step-by-step and execute directly

6. **Given** the `AskSupervisor` tool sets an escalation in the shared `EscalationSlot`
   **When** `ResponseAnalyzer::analyze()` is called with the same slot
   **Then** it returns `ResponseAction::Escalated` (priority 1, highest)

7. **Given** the `AskSupervisor` tool receives a question matching a rule
   **When** `call()` completes
   **Then** a `DecisionRecord` with `DecisionSource::RuleEngine` is appended to the `DecisionLog`

8. **Given** the `AskSupervisor` tool receives a question with no rule match and no architect provider
   **When** `call()` completes
   **Then** `SupervisorError::LlmFallbackNotImplemented` is returned **and** a `DecisionRecord` with `DecisionSource::Escalation` is logged

9. **Given** an agent response containing a review completion signal (e.g., "✅ Review complete")
   **When** `ResponseAnalyzer::analyze()` processes it
   **Then** it returns `ResponseAction::Completed` (priority 1.5, before dev-session completion)

10. **Given** an agent response containing a review fix prompt (e.g., "Fix them automatically")
    **When** `ResponseAnalyzer::analyze()` processes it
    **Then** it returns `ResponseAction::Continue { reply: "1" }` (auto-fix selection)

## Tasks / Subtasks

- [ ] Task 0: Verify `lib.rs` blocker resolved (AC: all)
  - [ ] 0.1 Confirm `src/lib.rs` exists with `pub mod supervisor;` and `pub mod session;`
  - [ ] 0.2 If missing, create `src/lib.rs` per Story 7.1 Task 0 spec (BLOCKER)

- [ ] Task 1: Create integration test file (AC: 1-10)
  - [ ] 1.1 Create `tests/integration/test_response_analyzer_supervisor.rs`
  - [ ] 1.2 Register module in `tests/integration.rs`: `mod test_response_analyzer_supervisor;`

- [ ] Task 2: ResponseAnalyzer completion signal tests (AC: 1)
  - [ ] 2.1 Test each `COMPLETION_SIGNALS` phrase triggers `Completed`
  - [ ] 2.2 Test completion signals are case-insensitive
  - [ ] 2.3 Test non-completion phrases do NOT trigger `Completed` (e.g., "I'll complete the task", "implementation of the feature")

- [ ] Task 3: ResponseAnalyzer story selection tests (AC: 2)
  - [ ] 3.1 Test story selection pattern returns `Continue { reply: story_key }`
  - [ ] 3.2 Test different story keys are passed through correctly
  - [ ] 3.3 Test all `STORY_SELECTION_PATTERNS` phrases

- [ ] Task 4: RuleEngine confirmation tests (AC: 3)
  - [ ] 4.1 Test confirmation patterns return `Matched` with "Yes, proceed."
  - [ ] 4.2 Test permission patterns ("Should I create…") return `Matched`
  - [ ] 4.3 Verify NO architect/LLM call is involved (rule engine only)

- [ ] Task 5: RuleEngine no-match fallthrough tests (AC: 4)
  - [ ] 5.1 Test substantive questions return `RuleResult::NoMatch`
  - [ ] 5.2 Test ambiguous technical questions return `NoMatch`
  - [ ] 5.3 Test empty string returns `NoMatch`

- [ ] Task 6: ResponseAnalyzer step-by-step and YOLO detection tests (AC: 5)
  - [ ] 6.1 Test step-by-step phrases return `Continue` with yolo/execute directive
  - [ ] 6.2 Test YOLO patterns return `Continue` with batch mode directive
  - [ ] 6.3 Test priority ordering: step-by-step (4) vs YOLO (5)

- [ ] Task 7: Cross-module escalation slot integration tests (AC: 6)
  - [ ] 7.1 Create shared `EscalationSlot`, write `EscalationInfo` into it
  - [ ] 7.2 Call `ResponseAnalyzer::analyze()` with the same slot
  - [ ] 7.3 Verify `Escalated` returned regardless of response text content
  - [ ] 7.4 Verify escalation takes priority over completion signals

- [ ] Task 8: AskSupervisor decision logging integration tests (AC: 7, 8)
  - [ ] 8.1 Test rule match records `DecisionSource::RuleEngine` with correct rule name
  - [ ] 8.2 Test no-match-no-architect records `DecisionSource::Escalation`
  - [ ] 8.3 Test multiple calls accumulate decisions in shared `DecisionLog`
  - [ ] 8.4 Verify `DecisionLog::len()` and `DecisionLog::records()` reflect all calls

- [ ] Task 9: Review pattern integration tests (AC: 9, 10)
  - [ ] 9.1 Test `REVIEW_COMPLETE_PATTERNS` phrases trigger `Completed`
  - [ ] 9.2 Test `REVIEW_FIX_PATTERNS` phrases trigger `Continue { reply: "1" }`
  - [ ] 9.3 Test priority: review complete (1.5) takes priority over review fix (5.5) when "Issues Fixed:" appears in review summary
  - [ ] 9.4 Test review complete does NOT false-positive on normal completion signals

- [ ] Task 10: Full pipeline integration tests (AC: 3, 4, 6, 7, 8)
  - [ ] 10.1 Test full flow: agent asks confirmation → rule engine matches → analyzer sees no escalation → Continue
  - [ ] 10.2 Test full flow: agent asks unknown question → rule engine misses → failing `MockAnswerProvider` → supervisor escalates → analyzer sees escalation slot → `Escalated`
  - [ ] 10.3 Test full flow: agent signals completion → analyzer returns Completed (rule engine not involved)

## Dev Notes

### Cross-Module Integration Value

This story validates the **two-layer response handling architecture**:

| Layer | Module | Responsibility | Tested Here |
|-------|--------|----------------|-------------|
| **Chat loop layer** | `session::analyzer` | Workflow-level interactions (completions, confirmations, story selection) | ✅ |
| **Tool layer** | `supervisor::rules` | Substantive questions via `ask_supervisor` rig tool | ✅ |
| **Cross-layer** | `supervisor::mod` → `session::analyzer` | Escalation slot bridges tool errors to chat loop | ✅ |
| **Decision layer** | `supervisor::decisions` | Decision logging accumulation across calls | ✅ |

Unit tests verify each module in isolation. These integration tests verify the **contracts between modules hold** — that the escalation slot written by `AskSupervisor::call()` is correctly read by `ResponseAnalyzer::analyze()`, that pattern coverage doesn't have blind spots, and that the decision logging pipeline works end-to-end.

### Architecture Compliance

#### 🚨🚨 BLOCKER — `src/lib.rs` Must Exist (Task 0)

**The project is currently a pure binary crate** (`src/main.rs` only). Integration tests in `tests/` cannot import from a binary crate. Story 7.1 creates `src/lib.rs`. If 7.1 has not been implemented yet, Task 0 of this story MUST create it first.

**Required `src/lib.rs`:**
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

**Then update `src/main.rs`** — remove `mod config;` through `mod watcher;` declarations (keep `mod cli;` binary-only) and use library imports instead.

**Verify:** `cargo build && cargo test` passes all existing unit tests.

#### Module Visibility — ✅ Confirmed Public

All types needed by this story are already `pub`. Confirmed by reading source:

| Type | Path | Status |
|------|------|--------|
| `ResponseAnalyzer` | `session::analyzer::ResponseAnalyzer` | ✅ `pub struct` |
| `ResponseAction` | `session::analyzer::ResponseAction` | ✅ `pub enum` |
| `RuleEngine` | `supervisor::rules::RuleEngine` | ✅ `pub struct` |
| `RuleResult` | `supervisor::rules::RuleResult` | ✅ `pub enum` |
| `AskSupervisor` | `supervisor::AskSupervisor` | ✅ `pub struct` |
| `AskSupervisorArgs` | `supervisor::AskSupervisorArgs` | ✅ `pub struct`, both fields `pub` |
| `SupervisorError` | `supervisor::SupervisorError` | ✅ `pub enum` |
| `EscalationSlot` | `supervisor::EscalationSlot` | ✅ `pub type` = `Arc<Mutex<Option<EscalationInfo>>>` |
| `EscalationInfo` | `session::escalation::EscalationInfo` | ✅ `pub struct` |
| `DecisionLog` | `supervisor::decisions::DecisionLog` | ✅ `pub struct` |
| `DecisionRecord` | `supervisor::decisions::DecisionRecord` | ✅ `pub struct` |
| `DecisionSource` | `supervisor::decisions::DecisionSource` | ✅ `pub enum` |
| `Rule` | `supervisor::rules::Rule` | ✅ `pub struct` |
| `RulePattern` | `supervisor::rules::RulePattern` | ✅ `pub enum` |
| `AnswerProvider` | `supervisor::architect::AnswerProvider` | ✅ `pub trait` |
| `MockAnswerProvider` | `supervisor::architect::MockAnswerProvider` | ✅ `pub struct`, both fields `pub` |

**✅ `session::analyzer` is `pub mod analyzer;`** in `src/session/mod.rs` (line 21). No visibility adjustment needed.

#### Integration Test Location

```
tests/
├── e2e/
│   └── mod.rs              # (existing — DO NOT TOUCH)
├── integration.rs           ← Cargo test binary entry point (from 7.1 or create if missing)
└── integration/
    ├── helpers/
    │   └── mod.rs           # Re-exports (from 7.1)
    └── test_response_analyzer_supervisor.rs  ← NEW (all Story 7.10 tests)
```

### Technical Requirements

#### Quick API Reference

**`ResponseAnalyzer`** — `src/session/analyzer.rs`

| Method | Signature | Returns |
|--------|-----------|---------|
| `new()` | `pub fn new() -> Self` | `ResponseAnalyzer` |
| `analyze()` | `pub fn analyze(&self, response: &str, escalation_slot: &EscalationSlot, story_key: &str) -> ResponseAction` | `ResponseAction` |

**`ResponseAction`** enum variants:
- `Continue { reply: String }` — send reply, keep looping
- `Completed` — agent signaled workflow completion
- `Escalated` — escalation detected via slot
- `NoReply` — reserved for streaming (not tested here)

**Priority order in `analyze()`:**
1. Escalation slot check (highest)
2. Review completion patterns (`REVIEW_COMPLETE_PATTERNS`) — priority 1.5
3. Dev-session completion signals (`COMPLETION_SIGNALS`) — priority 2
4. Confirmation/proceed (`PROCEED_PATTERNS`) — priority 3
5. Step-by-step detection (`STEP_BY_STEP_PATTERNS`) — priority 4
6. YOLO/batch mode (`YOLO_PATTERNS`) — priority 5
7. Review fix decision (`REVIEW_FIX_PATTERNS`) — priority 5.5
8. Story selection (`STORY_SELECTION_PATTERNS`) — priority 6
9. Default → `Continue { reply: "Continue." }` — priority 7

**`RuleEngine`** — `src/supervisor/rules.rs`

| Method | Signature | Returns |
|--------|-----------|---------|
| `new()` | `pub fn new() -> Self` | `RuleEngine` (with 6 default rules) |
| `evaluate()` | `pub fn evaluate(&self, question: &str) -> RuleResult` | `RuleResult` |
| `add_rule()` | `pub fn add_rule(&mut self, rule: Rule)` | `()` |
| `rule_count()` | `pub fn rule_count(&self) -> usize` | `usize` |

**`RuleResult`** enum variants:
- `Matched { rule_name: String, answer: String }`
- `NoMatch`

**Default rules (in priority order):**
1. `confirmation_proceed` — "should i proceed", "shall i continue", etc. → "Yes, proceed."
2. `permission_action` — "should i create", "should i modify", etc. → "Yes, proceed with the action as described."
3. `step_by_step_detection` — "step by step", "here's my plan", etc. → "Skip the step-by-step breakdown. Execute directly using yolo mode."
4. `story_selection` — "which story", "what story", etc. → "The story file has been provided in context..."
5. `progress_confirmation` — "i've completed", "task complete", etc. → "Acknowledged. Continue to the next task."
6. `stuck_general` — "i'm stuck", "i'm blocked", etc. → "Describe the specific problem..."

**`AskSupervisor`** — `src/supervisor/mod.rs`

| Method | Signature | Returns |
|--------|-----------|---------|
| `new()` | `pub fn new() -> Self` | `AskSupervisor` (rule engine only, no architect) |
| `with_answer_provider(provider)` | `pub fn with_answer_provider(provider: Box<dyn AnswerProvider>) -> Self` | `AskSupervisor` with provider, fresh slot + log |
| `with_answer_provider_and_slot(provider, slot)` | `pub fn with_answer_provider_and_slot(provider: Box<dyn AnswerProvider>, escalation_slot: EscalationSlot) -> Self` | `AskSupervisor` with shared slot |
| `with_all(provider, slot, log)` | `pub fn with_all(provider: Box<dyn AnswerProvider>, escalation_slot: EscalationSlot, decision_log: DecisionLog) -> Self` | Full production constructor |
| `escalation_slot()` | `pub fn escalation_slot(&self) -> EscalationSlot` | **Returns `Arc::clone`** (not a reference) |
| `decision_log()` | `pub fn decision_log(&self) -> DecisionLog` | **Returns clone** (shares inner `Arc<Mutex<Vec>>`) |
| `call()` (Tool trait) | `async fn call(&self, args: AskSupervisorArgs) -> Result<String, SupervisorError>` | Rule answer or error |

**`DecisionLog`** — `src/supervisor/decisions.rs`

| Method | Signature | Returns |
|--------|-----------|---------|
| `new()` | `pub fn new() -> Self` | Empty `DecisionLog` |
| `record(record)` | `pub fn record(&self, record: DecisionRecord)` | `()` — appends to internal `Arc<Mutex<Vec>>` |
| `records()` | `pub fn records(&self) -> Vec<DecisionRecord>` | **Cloned** vec of all recorded decisions |
| `len()` | `pub fn len(&self) -> usize` | Number of recorded decisions |
| `is_empty()` | `pub fn is_empty(&self) -> bool` | `true` if no decisions recorded |

**`AnswerProvider`** trait — `src/supervisor/architect.rs`

```rust
#[async_trait]
pub trait AnswerProvider: Send + Sync + std::fmt::Debug {
    async fn ask(
        &self,
        question: &str,
        context: Option<&str>,
    ) -> Result<String, ArchitectSessionError>;
}
```

**`MockAnswerProvider`** — already exists in `src/supervisor/architect.rs`

```rust
#[derive(Debug)]
pub struct MockAnswerProvider {
    pub response: String,     // returned on success
    pub should_fail: bool,    // if true, ask() returns Err(ChatFailed)
}
```

**`EscalationSlot`** = `Arc<Mutex<Option<EscalationInfo>>>`

**`EscalationInfo`** struct fields:
- `question: String`
- `reason: String`

**`AskSupervisorArgs`** struct — both fields are `pub`, construct directly:
```rust
let args = AskSupervisorArgs {
    question: "Should I proceed with the implementation?".to_string(),
    context: None,
};

// With optional context:
let args = AskSupervisorArgs {
    question: "What database schema should I use?".to_string(),
    context: Some("Working on user authentication module".to_string()),
};
```

#### Key Behavioral Contracts to Test

1. **Escalation slot bridge:** `AskSupervisor::call()` writes `Some(EscalationInfo{...})` to the slot BEFORE returning `Err(SupervisorError::EscalationRequired{...})`. The `ResponseAnalyzer` checks this slot at priority 1. This is the critical cross-module contract.

2. **Pattern overlap consistency:** Both `ResponseAnalyzer` and `RuleEngine` handle "should i proceed" patterns. The analyzer handles it at the chat-loop level (priority 3 → "Yes, proceed."). The rule engine handles it at the tool level (confirmation_proceed → "Yes, proceed."). These are complementary — the analyzer handles agent text responses, the rule engine handles tool-invoked questions. Tests should verify both paths return consistent answers.

3. **Decision log accumulation:** `DecisionLog` uses `Arc<Mutex<Vec<DecisionRecord>>>` internally. `DecisionLog::clone()` shares the same `Arc`, so decisions recorded via the supervisor appear in the clone held by the test. Multiple calls to `AskSupervisor::call()` should accumulate records. Use `decision_log.len()` and `decision_log.records()` to verify.

4. **No false positive completions:** Phrases like "I'll complete the task" or "implementation of the feature" must NOT trigger `ResponseAction::Completed`. Only strong multi-word signals like "all tasks completed" or "implementation is complete" should.

5. **Review complete vs review fix priority:** The review step 5 summary contains "Issues Fixed:" which appears in `REVIEW_FIX_PATTERNS`. But `REVIEW_COMPLETE_PATTERNS` is checked at priority 1.5 (before fix at 5.5), so a full review summary should trigger `Completed`, not a fix response.

#### Cross-Module Escalation Wiring Example (Task 10.2)

This is the most valuable integration test — verifying the escalation slot bridge:

```rust
// 1. Shared escalation slot — same Arc passed to both supervisor and analyzer
let slot: EscalationSlot = Arc::new(Mutex::new(None));

// 2. Failing architect mock — triggers escalation path
let mock = MockAnswerProvider {
    response: String::new(),
    should_fail: true,
};

// 3. Wire AskSupervisor with shared slot
let supervisor = AskSupervisor::with_answer_provider_and_slot(
    Box::new(mock),
    slot.clone(),
);

// 4. Ask a question that doesn't match any rule → architect fails → escalation
let args = AskSupervisorArgs {
    question: "What database schema should I use for user sessions?".to_string(),
    context: None,
};
let result = supervisor.call(args).await;
assert!(matches!(result, Err(SupervisorError::EscalationRequired { .. })));

// 5. Verify the slot was written BEFORE the error was returned
{
    let guard = slot.lock().expect("slot lock");
    assert!(guard.is_some(), "escalation slot should contain EscalationInfo");
}

// 6. ResponseAnalyzer reads the same slot — detects escalation at priority 1
let analyzer = ResponseAnalyzer::new();
let action = analyzer.analyze(
    "Here is some irrelevant response text",  // text doesn't matter
    &slot,
    "7-10-test-story",
);
assert_eq!(action, ResponseAction::Escalated);

// 7. Verify decision was logged
let log = supervisor.decision_log();
assert_eq!(log.len(), 1);
let records = log.records();
assert!(matches!(records[0].source, DecisionSource::Escalation));
```

#### Creating the Shared EscalationSlot

```rust
use std::sync::{Arc, Mutex};
use bmad_bot::session::escalation::EscalationInfo;
use bmad_bot::supervisor::EscalationSlot;

// Empty slot (no escalation)
let slot: EscalationSlot = Arc::new(Mutex::new(None));

// Slot with escalation (simulating supervisor writing to it)
let slot: EscalationSlot = Arc::new(Mutex::new(Some(EscalationInfo {
    question: "What DB schema should I use?".to_string(),
    reason: "Architect session failed: connection timeout".to_string(),
})));
```

### Previous Story Intelligence (Stories 7.1 through 7.9)

1. **`lib.rs` blocker is resolved** — Story 7.1 expanded `src/lib.rs` to 12 `pub mod` declarations. `main.rs` retains `mod X;` (dual-crate compilation). Integration tests import via `bmad_bot::`. Story 7.1 is implemented with 38 passing tests.

2. **Test module registration requires `#[path]` attributes** — e.g., `#[path = "integration/test_response_analyzer_supervisor.rs"] mod test_response_analyzer_supervisor;` in `tests/integration.rs`. Direct `mod` does NOT resolve.

3. **Test file naming convention:** `test_{module_name}.rs`. For this story: `test_response_analyzer_supervisor.rs`.

4. **No mocks needed for ResponseAnalyzer or RuleEngine tests.** Both operate on in-memory data only (strings, pattern matching). No filesystem, HTTP, or git mocking required.

5. **`AskSupervisor::new()` creates a supervisor with rule engine only** (no architect provider). For testing the no-architect-fallback path (AC #8), `AskSupervisor::new()` is sufficient — it returns `LlmFallbackNotImplemented` on miss. For testing the full escalation flow with slot write (Task 10.2), use `AskSupervisor::with_answer_provider_and_slot()` with `MockAnswerProvider { should_fail: true }`.

6. **`MockAnswerProvider` already exists** in `src/supervisor/architect.rs`. Do NOT reinvent it. Import as `bmad_bot::supervisor::architect::MockAnswerProvider`.

7. **Timestamps in DecisionRecords:** Never assert on exact timestamp values. Assert that the timestamp field is non-empty or parse with `chrono::DateTime::parse_from_rfc3339`.

8. **Story 7.9 established pattern:** Confirmed module visibility definitively (✅) and provided Quick API Reference tables. This story follows the same pattern.

9. **`rig::tool::Tool` trait `call()` is async** — use `#[tokio::test]` for any test invoking `AskSupervisor::call()`.

### Git Intelligence

Recent commits (last 5):

```
54d9a6a docs(stories): create story 7-9 CLI lifecycle integration tests and update sprint status
5ef3888 gitignore
508ef2c docs(stories): add story 1-5 git remote auto-detection in init command
81e0064 docs(stories): create story 7-8 branch management git tools integration tests and update sprint status
ad4e6e8 docs: add comprehensive README with architecture, quick start, and CLI reference
```

No Epic 7 implementation code committed yet. All stories 7.1–7.9 are `ready-for-dev` or `review`. Story 7.10 is the last story in Epic 7.

### Dependencies Required

All already present in `Cargo.toml`:
- `serde_json = "1"` — only if needed for edge-case deserialization tests
- `tokio` with `full` features — for `#[tokio::test]` on async `call()` tests
- `async-trait = "0.1"` — already a main dependency (used by `AnswerProvider` trait)

**No new dependencies needed.**

### Required Imports for Test File

```rust
// Core types under test
use bmad_bot::session::analyzer::{ResponseAnalyzer, ResponseAction};
use bmad_bot::session::escalation::EscalationInfo;
use bmad_bot::supervisor::{AskSupervisor, AskSupervisorArgs, EscalationSlot, SupervisorError};
use bmad_bot::supervisor::architect::MockAnswerProvider;
use bmad_bot::supervisor::decisions::{DecisionLog, DecisionSource};
use bmad_bot::supervisor::rules::{RuleEngine, RuleResult};

// Utilities
use rig::tool::Tool;
use std::sync::{Arc, Mutex};
```

### File Structure

```
tests/
├── e2e/
│   └── mod.rs              # (existing — DO NOT TOUCH)
├── integration.rs           ← Ensure `mod test_response_analyzer_supervisor;` is declared
└── integration/
    ├── helpers/
    │   └── mod.rs           # (existing from 7.1 — DO NOT TOUCH)
    └── test_response_analyzer_supervisor.rs  ← NEW (all Story 7.10 tests)
```

### Testing Standards

- **Framework:** Use `#[test]` for sync tests (ResponseAnalyzer, RuleEngine). Use `#[tokio::test]` for async tests (AskSupervisor::call).
- **Isolation:** No shared state between tests. Each test creates its own `ResponseAnalyzer`, `RuleEngine`, `EscalationSlot`, etc.
- **Naming:** `test_{component}_{behavior}_{scenario}` in snake_case.
- **Structure:** Arrange → Act → Assert, always in that order.
- **Assertions:** Use `assert!`, `assert_eq!`, `assert_ne!`. Use `.expect("reason")` for unwraps.
- **No filesystem, HTTP, or git:** All tests operate purely on in-memory structures.
- **All tests must complete in < 2 seconds total** — pattern matching and in-memory operations only.
- **Tracing is a no-op in tests** — `tracing::info!()` and `tracing::debug!()` calls in production code are silent without a subscriber. Do NOT install a tracing subscriber.

### Project Structure Notes

- Alignment with unified project structure: integration tests in `tests/` per `project-context.md` and `architecture.md`
- Existing `tests/e2e/mod.rs` is reserved for live LLM E2E tests (gated behind `BMAD_E2E=1`) — do NOT modify
- This story tests NO external dependencies (no LLM, no HTTP, no filesystem, no git) — purely in-memory module interaction
- The `rig::tool::Tool` trait is used to call `AskSupervisor::call()` directly — no rig agent setup needed

### References

- [Source: _bmad-output/planning-artifacts/epics.md — Epic 7 Overview (L854-864)]
- [Source: _bmad-output/planning-artifacts/epics.md — Integration Test Strategy (L864-898)]
- [Source: _bmad-output/planning-artifacts/epics.md — Story 7.10 (L1254-1287)]
- [Source: _bmad-output/planning-artifacts/epics.md — Epic Summary (L1287-1312)]
- [Source: _bmad-output/planning-artifacts/epics.md — Epic 3: Intelligent Supervision (L436-541)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Decision 1: Supervisor Interception Model (L160-186)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Test Mock Pattern (L510-542)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Project Structure (L561-607)]
- [Source: _bmad-output/planning-artifacts/architecture.md — Rig Tool Implementation Pattern (L376-427)]
- [Source: _bmad-output/project-context.md — Supervisor Hybrid Pattern section]
- [Source: _bmad-output/project-context.md — Testing Rules section]
- [Source: _bmad-output/project-context.md — Critical Don't-Miss Rules section]
- [Source: src/session/analyzer.rs — ResponseAnalyzer (L158), ResponseAction (L19-37)]
- [Source: src/session/analyzer.rs — COMPLETION_SIGNALS (L79-89)]
- [Source: src/session/analyzer.rs — PROCEED_PATTERNS (L92-106)]
- [Source: src/session/analyzer.rs — STEP_BY_STEP_PATTERNS (L109-118)]
- [Source: src/session/analyzer.rs — YOLO_PATTERNS (L121-129)]
- [Source: src/session/analyzer.rs — STORY_SELECTION_PATTERNS (L132-141)]
- [Source: src/session/analyzer.rs — REVIEW_COMPLETE_PATTERNS (L45-52)]
- [Source: src/session/analyzer.rs — REVIEW_FIX_PATTERNS (L60-72)]
- [Source: src/session/analyzer.rs — analyze() priority order (L181-322)]
- [Source: src/session/mod.rs — pub mod analyzer (L21)]
- [Source: src/supervisor/rules.rs — RuleEngine (L103-105), RuleResult (L17-27)]
- [Source: src/supervisor/rules.rs — default_rules() (L155-267) — 6 rules]
- [Source: src/supervisor/rules.rs — RulePattern (L49-57) — Contains, StartsWithAny, AnyOf]
- [Source: src/supervisor/mod.rs — AskSupervisor (L105-126)]
- [Source: src/supervisor/mod.rs — AskSupervisorArgs (L80-88) — both fields pub]
- [Source: src/supervisor/mod.rs — SupervisorError (L49-73)]
- [Source: src/supervisor/mod.rs — EscalationSlot type alias (L40)]
- [Source: src/supervisor/mod.rs — escalation_slot() returns Arc::clone (L209-211)]
- [Source: src/supervisor/mod.rs — decision_log() returns clone (L214-216)]
- [Source: src/supervisor/mod.rs — with_answer_provider_and_slot() (L160-170)]
- [Source: src/supervisor/mod.rs — Tool::call() implementation (L261-379)]
- [Source: src/supervisor/architect.rs — AnswerProvider trait (L91-99)]
- [Source: src/supervisor/architect.rs — MockAnswerProvider (L353-358)]
- [Source: src/supervisor/architect.rs — MockAnswerProvider::ask impl (L361-376)]
- [Source: src/session/escalation.rs — EscalationInfo (L30-35)]
- [Source: src/supervisor/decisions.rs — DecisionLog (L155-158)]
- [Source: src/supervisor/decisions.rs — DecisionLog::records() (L199-211)]
- [Source: src/supervisor/decisions.rs — DecisionLog::len() (L214-216)]
- [Source: src/supervisor/decisions.rs — DecisionRecord (L65-80), DecisionSource (L31-41)]
- [Source: _bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md — lib.rs blocker (L97-134)]
- [Source: _bmad-output/implementation-artifacts/7-1-integration-test-infrastructure-fixtures.md — File Structure (L276-321)]
- [Source: _bmad-output/implementation-artifacts/7-9-cli-lifecycle-integration-tests.md — Previous Story Intelligence (L289-303)]
- [Source: Cargo.toml — dependencies and dev-dependencies]

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List