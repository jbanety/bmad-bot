# Story 5.2: Automated Code Review Session

Status: ready-for-dev

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a developer,
I want an optional automated code review by a separate LLM after the dev session completes,
so that code quality issues are caught and fixed before I review the PR.

## Acceptance Criteria

1. **Given** code review is enabled in `bmad-bot.yaml` configuration
   **When** a development session completes successfully (`SessionOutcome::Completed`)
   **Then** the daemon launches a **new rig agent session** using the review LLM provider/model from `BotConfig.llm.review`
   **And** the session loads the same BMAD dev agent persona (`dev.md`) and sends `"CR"` as the initial command
   **And** the supervisor/analyzer auto-responds to story selection with the story specs file path

2. **Given** the review agent asks how to handle findings (fix automatically / create action items / show details)
   **When** the daemon's response analyzer detects this decision prompt
   **Then** it auto-responds with `"1"` (fix them automatically)
   **And** the review agent applies fixes to the code (but does not commit yet)

3. **Given** the CR workflow completes (step 5 "Review Complete")
   **When** the daemon detects the review completion output
   **Then** the daemon sends a post-review message asking the agent to commit all review fixes with descriptive commit messages referencing the findings, and to provide a complete markdown review report
   **And** the agent commits the fixes in separate commits (distinct from dev agent commits) with full context
   **And** the agent's review report response is captured in `ReviewOutcome::Completed { report }`

4. **Given** a PR is created by the orchestrator after the review
   **When** the `ReviewOutcome::Completed` contains a report
   **Then** the orchestrator posts the review report as a comment on the PR via `GitProvider::add_comment()`
   **And** the review comment includes: summary of findings, severity levels, fixes applied, and any remaining concerns

5. **Given** the review LLM provider is unavailable or errors out
   **When** the review session fails
   **Then** the daemon logs the error, returns `ReviewOutcome::Skipped` with a reason
   **And** the orchestrator proceeds to PR creation without review
   **And** the PR description notes that automated code review was skipped due to an error

6. **Given** code review is disabled in configuration (`code_review_enabled: false`)
   **When** a development session completes
   **Then** the daemon skips the review entirely and proceeds directly to PR creation

## Tasks / Subtasks

### Task 0: Prerequisite Verification

- [ ] Verify Story 5.1 code is present and compilable: `GitProvider` trait, `add_comment()`, `create_provider()` in `src/git_provider/`
- [ ] Verify `BotConfig.llm.review` has `LlmRoleConfig` (provider + model) in `src/config/mod.rs`
- [ ] Verify `BotSecrets` has API key fields for all supported providers in `src/config/mod.rs`
- [ ] Verify `resolve_api_key()` and `ProviderError` exist in `src/session/provider.rs`
- [ ] Verify rig tools exist: `GitTool`, `FsTool`, `TerminalTool` in `src/tools/`
- [ ] Verify `ResponseAnalyzer` with `STORY_SELECTION_PATTERNS` exists in `src/session/analyzer.rs`
- [ ] Verify `SessionOutcome::Completed { story_key, branch, decisions }` in `src/session/mod.rs`
- [ ] Verify `AskSupervisor` and `EscalationSlot` exist in `src/supervisor/`
- [ ] Verify BMAD CR workflow exists at `_bmad/bmm/workflows/4-implementation/code-review/workflow.yaml`
- [ ] Confirm existing skeleton: `src/review/mod.rs` (TODO stub)
- [ ] Verify all needed crates in `Cargo.toml`: `rig-core`, `git2`, `async-trait`, `thiserror`

### Task 1: Add `code_review_enabled` Config Field (`src/config/mod.rs`)

- [ ] Add field to `BotConfig`:
  - `#[serde(default = "default_code_review_enabled")] pub code_review_enabled: bool`
- [ ] Add default function: `fn default_code_review_enabled() -> bool { true }`
- [ ] Update `BotConfig::_test_minimal()` helper to include the new field (with `#[serde(default)]` it will auto-default, but verify VALID_YAML still parses)
- [ ] Add unit test: `test_config_code_review_enabled_defaults_to_true` — parse YAML without field, verify `true`
- [ ] Add unit test: `test_config_code_review_disabled_parses` — parse YAML with `code_review_enabled: false`, verify `false`
- [ ] Update `bmad-bot.yaml.example` — add entry with comment:
  ```yaml
  # Automated code review after dev sessions (optional, default: true)
  # When enabled, a separate LLM runs the BMAD CR workflow to review code before PR creation.
  code_review_enabled: true
  ```

### Task 2: Add Review Patterns to ResponseAnalyzer (`src/session/analyzer.rs`)

- [ ] Add new constant `REVIEW_FIX_PATTERNS`:
  ```rust
  const REVIEW_FIX_PATTERNS: &[&str] = &[
      "fix them automatically",
      "create action items",
      "show me details",
      "choose [1]",
      "choose [2]",
      "choose [3]",
      "[1] fix",
      "[2] create",
      "[3] show",
      "what should i do with these issues",
      "what should i do with these findings",
  ];
  ```
- [ ] Add new constant `REVIEW_COMPLETE_PATTERNS` to detect CR workflow step 5 completion:
  ```rust
  const REVIEW_COMPLETE_PATTERNS: &[&str] = &[
      "review complete",
      "✅ review complete",
      "code review complete",
      "issues fixed:",
      "action items created:",
      "sprint status synced",
  ];
  ```
- [ ] Insert `REVIEW_COMPLETE_PATTERNS` at **priority 1.5** (after Escalation priority 1, BEFORE existing `COMPLETION_SIGNALS` priority 2) in `analyze()`:
  - If `REVIEW_COMPLETE_PATTERNS` matches → `Completed` (triggers post-review phase in `drive_review_session`)
  - Log: `tracing::debug!(action = "response_analysis", result = "review_complete", "CR workflow completion detected")`
  - ⚠️ **No overlap with existing `COMPLETION_SIGNALS`:** The CR workflow outputs "Review Complete!", "Code review complete!" — none of these match the existing dev-session signals ("all tasks completed", "story implementation complete", "story is ready for review", etc.). Verified against `src/session/analyzer.rs` `COMPLETION_SIGNALS` constant.
- [ ] Insert `REVIEW_FIX_PATTERNS` at **priority 5.5** (between YOLO priority 5 and Story Selection priority 6) in `analyze()`:
  - If `REVIEW_FIX_PATTERNS` matches → `Continue { reply: "1".to_string() }` (fix automatically)
  - Log: `tracing::debug!(action = "response_analysis", result = "review_fix_decision", "Review fix decision detected — auto-fixing")`
  - ⚠️ **Must be AFTER `REVIEW_COMPLETE_PATTERNS`:** The step 5 summary contains "Issues Fixed:" which could match `REVIEW_FIX_PATTERNS`. Since `REVIEW_COMPLETE_PATTERNS` is at priority 1.5, it fires first and returns `Completed` — the fix patterns at 5.5 never see the step 5 output.
- [ ] Add unit tests:
  - `test_analyzer_detects_review_fix_decision` — verify "Choose [1], [2], or specify" → replies "1"
  - `test_analyzer_detects_fix_automatically_pattern` — verify "Fix them automatically" → replies "1"
  - `test_analyzer_review_fix_does_not_false_positive` — verify normal text with "fix" doesn't trigger
  - `test_analyzer_detects_review_complete` — verify "✅ Review Complete!" output → triggers Completed
  - `test_analyzer_review_complete_does_not_false_positive` — verify normal text with "complete" doesn't trigger

### Task 3: Define Review Types (`src/review/mod.rs`)

- [ ] Define `ReviewError` enum with `thiserror`:
  - `ProviderInit { reason: String }` — LLM client construction failed
  - `ApiKeyMissing { provider: String, env_var: String }` — review provider API key not set
  - `UnsupportedProvider { provider: String }` — unknown provider name
  - `ChatFailed { turn: usize, reason: String }` — chat turn error
  - `AgentBuildFailed { reason: String }` — rig agent construction failed
  - `PreambleLoadFailed { path: String, reason: String }` — dev.md file read failed
- [ ] Define `ReviewOutcome` enum:
  - `Completed { story_key: String, branch: String, report: String }` — CR workflow finished, story marked done, review report captured for PR comment
  - `Failed { story_key: String, error: String }` — review session crashed (non-blocking)
  - `Skipped { reason: String }` — review was skipped (provider down, config disabled, etc.)
- [ ] Add `///` doc comments on all public items

### Task 4: Implement `ReviewRunner` (`src/review/mod.rs`)

- [ ] Define `ReviewRunner` struct:
  - `config: Arc<BotConfig>` — shared daemon config
  - `secrets: Arc<BotSecrets>` — shared secrets
- [ ] Implement `ReviewRunner::new(config: Arc<BotConfig>, secrets: Arc<BotSecrets>) -> Self`
- [ ] Implement `pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome`:
  - **This method NEVER panics or returns an unhandled error** — all failures → `ReviewOutcome::Skipped` or `ReviewOutcome::Failed`
  - Internally calls `run_inner()` and catches errors:
    ```rust
    pub async fn run(&self, story: &StoryInfo) -> ReviewOutcome {
        match self.run_inner(story).await {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::error!(action = "review_failed", error = %e, story_key = %story.story_key, "Code review failed — skipping");
                ReviewOutcome::Skipped { reason: e.to_string() }
            }
        }
    }
    ```
- [ ] Implement `async fn run_inner(&self, story: &StoryInfo) -> Result<ReviewOutcome, ReviewError>`:
  1. Resolve API key: `resolve_api_key(&self.config.llm.review.provider, &self.secrets)` — map `ProviderError` → `ReviewError`
  2. Load BMAD dev agent preamble: read `{project_root}/_bmad/bmm/agents/dev.md`, append `"\n\nOVERRIDE: communication_language = English"` (same as SessionRunner)
  3. Create tools: `GitTool`, `FsTool`, `TerminalTool` (same as dev session, **plus** `AskSupervisor`)
  4. Build rig agent with review provider/model — follow provider match-arm pattern (anthropic/openai/github-models)
  5. Create shared `EscalationSlot` and `DecisionLog` (same as dev session)
  6. Call `drive_review_session()` with the agent
- [ ] Implement `async fn drive_review_session<A: Chat>(&self, agent: &A, story: &StoryInfo, escalation_slot: EscalationSlot, decision_log: DecisionLog) -> Result<ReviewOutcome, ReviewError>`:
  1. Send `"CR"` as initial message
  2. Initialize `post_review_phase = false`
  3. Enter chat loop (same pattern as `SessionRunner::run_session`):
     - **If `post_review_phase == true`**: Skip the analyzer entirely. The agent's response IS the report. Capture it and return `ReviewOutcome::Completed { story_key, branch, report }`.
     - **If `post_review_phase == false`**: Use `self.analyzer.analyze(response, &escalation_slot, &story_reply)` for each response
       - `story_reply` = **story specs file path** (`story.specs_path.display().to_string()`), NOT the story key — CR workflow needs the file path
       - Handle `ResponseAction::Continue { reply }` → send reply, continue loop
       - Handle `ResponseAction::Escalated` → `ReviewOutcome::Failed { story_key, error: "Review escalated" }`
       - Handle `ResponseAction::Completed`: Set `post_review_phase = true`. Send `POST_REVIEW_MESSAGE`. Continue loop.
     - Max turns: `MAX_REVIEW_TURNS = 100` (safety net)
  4. On chat error → `ReviewOutcome::Failed`
  
  **Why skip the analyzer in post_review_phase?** After `POST_REVIEW_MESSAGE`, the agent commits via GitTool and responds with a markdown report. This response won't contain any `COMPLETION_SIGNALS` or `REVIEW_COMPLETE_PATTERNS` — it's free-form text. Trying to detect completion here would require fragile pattern matching. Instead, we treat the next response as the final report by design. One message in, one report out.
- [ ] Create `ResponseAnalyzer` instance in `ReviewRunner` (same as `SessionRunner`)
- [ ] Add `///` doc comments on all public items

### Task 5: Wire Up Module Exports (`src/review/mod.rs`)

- [ ] Export: `pub use` for `ReviewRunner`, `ReviewOutcome`, `ReviewError`
- [ ] Update module doc comment to describe the BMAD CR workflow approach
- [ ] Ensure `src/main.rs` already has `mod review;` (it does — check it compiles)

### Task 6: Unit Tests

- [ ] Tests in `src/review/mod.rs` `#[cfg(test)] mod tests`:
  - `test_review_error_display_variants` — all error variants produce readable messages
  - `test_review_error_is_send_sync` — compile-time trait check
  - `test_review_outcome_completed_fields` — verify struct fields including `report`
  - `test_review_outcome_skipped_fields` — verify reason stored
  - `test_review_outcome_failed_fields` — verify story_key and error stored
  - `test_review_runner_new_stores_config` — verify construction
  - `test_review_runner_is_send_sync` — compile-time trait check
  - NOTE: No live LLM tests — actual review conversations tested in E2E only
- [ ] Tests in `src/session/analyzer.rs` (added in Task 2):
  - `test_analyzer_detects_review_fix_decision`
  - `test_analyzer_detects_fix_automatically_pattern`
  - `test_analyzer_review_fix_does_not_false_positive`
  - `test_analyzer_detects_review_complete` — verify "Review Complete" output triggers Completed
  - `test_analyzer_review_complete_does_not_false_positive` — verify normal text with "complete" doesn't trigger
- [ ] Tests in `src/config/mod.rs` (added in Task 1):
  - `test_config_code_review_enabled_defaults_to_true`
  - `test_config_code_review_disabled_parses`

### Task 7: Integration Verification

- [ ] `cargo check` — zero new errors
- [ ] `cargo test` — all new tests pass, no regressions on existing 435+ tests
- [ ] `cargo clippy` — zero new warnings
- [ ] `cargo fmt` — all clean
- [ ] Verify all public items have `///` doc comments
- [ ] Verify `review/mod.rs` properly exports all public types

## Dev Notes

### Previous Story Intelligence

**Story 5.1** (Git Provider Trait & GitHub PR Creation) — direct dependency:
- `GitProvider::add_comment(pr_id, body)` — used by the orchestrator (NOT the review module) to post review results on the PR after creation
- The review module does NOT interact with `git_provider` at all — the BMAD CR workflow uses git tools directly for commits

**Story 4.3** (Branch Management):
- Test count: **435 tests**. 82 pre-existing `dead_code` warnings from unconnected modules — expected.
- After the dev session, the working directory is already on the story branch — no branch setup needed for review

**Story 4.2** (Session Runner) — **PRIMARY PATTERN REFERENCE**:
- `SessionRunner::run()` is the template for `ReviewRunner::run()` — same structure, different LLM config and initial command
- `SessionRunner::run_session()` is the chat loop — `drive_review_session()` follows the same pattern
- Provider match-arm pattern (anthropic/openai/github-models) because rig's `Chat` trait is NOT object-safe
- Tools registered via `.tool()` on the agent builder
- `build_preamble()` reads `dev.md` and appends language override — reuse same approach

**Story 4.1** (Rig Tools):
- `GitTool::new(repo_path)` — the CR workflow uses this to commit review fixes
- `FsTool::new(repo_path)` — the CR workflow uses this to read/write code
- `TerminalTool::new(repo_path, timeout_secs)` — the CR workflow runs `cargo check`, `cargo test`

**Story 3.2** (LLM Fallback / Supervisor):
- `AskSupervisor` tool — the CR workflow may ask substantive questions that require supervisor fallback
- Review session MUST include the supervisor tool, same as dev session

### Core Design — BMAD CR Workflow via Daemon

**The daemon does NOT implement code review logic.** The BMAD dev agent already has a complete adversarial code review workflow (`CR` command). The daemon's job is simply to:

1. Launch a **new rig agent session** with the review LLM config
2. Load the **same BMAD dev agent persona** (`dev.md`)
3. Send **`"CR"`** as the initial command (instead of `"DS"`)
4. Let the agent drive the full CR workflow autonomously
5. The `ResponseAnalyzer` handles all interaction patterns automatically

**Why this design:**
- Zero BMAD workflow knowledge in daemon code — the daemon is a launcher, not an executor
- The CR workflow already handles: adversarial review, git diff analysis, fix application, story status updates, sprint-status sync
- The CR workflow does NOT auto-commit — it applies fixes to the filesystem but waits for instruction to commit
- After the CR workflow completes, the daemon sends a post-review message asking the agent to commit and produce a report
- The agent commits with full context (descriptive messages referencing findings) and provides a markdown review report
- The report is captured in `ReviewOutcome::Completed { report }` for the orchestrator to post as a PR comment

**Execution flow (orchestrated by future watcher/main loop):**
1. Dev session completes → `SessionOutcome::Completed { story_key, branch, decisions }`
2. Orchestrator checks `config.code_review_enabled`
3. If disabled → skip to PR creation, story stays in `review` status
4. If enabled → `ReviewRunner::run(story)` launches CR session
5. CR workflow runs: reads story, diffs code, reviews, applies fixes (no commit), updates story status
6. Daemon detects CR completion → sends post-review message asking agent to commit fixes + produce review report
7. Agent commits review fixes in separate commits (distinct from dev work) with descriptive messages
8. Agent produces markdown review report → captured as `report` in `ReviewOutcome::Completed`
9. `ReviewOutcome::Completed { report }` → orchestrator creates PR, then posts `report` as PR comment via `GitProvider::add_comment()`
10. `ReviewOutcome::Skipped/Failed` → orchestrator proceeds to PR creation anyway (non-blocking), notes skip in PR description

**Review runs ONLY on `SessionOutcome::Completed`** — not on Escalated or Failed sessions.

### ResponseAnalyzer: story_reply Parameter

The `analyze()` method takes a `story_key: &str` parameter that is returned for story selection questions. For the **dev session**, this is the story key (e.g., `"5-2-automated-code-review-session"`). For the **review session**, this must be the **story specs file path** (e.g., `"_bmad-output/implementation-artifacts/5-2-automated-code-review-session.md"`) because the CR workflow asks "which story file to review", not "which story key".

The `ReviewRunner` passes `story.specs_path.display().to_string()` as the story_reply parameter.

### ResponseAnalyzer: Review Fix Decision Pattern

The CR workflow (step 4) asks:
```
Choose [1], [2], or specify which issue to examine:
1. Fix them automatically
2. Create action items
3. Show me details
```

The analyzer needs a new pattern set to detect this and auto-respond with `"1"` (fix automatically) for autonomous operation. This is added as a new priority level between YOLO (5) and Story Selection (6).

### ResponseAnalyzer: Review Completion Detection

The CR workflow (step 5) outputs a completion summary:
```
✅ Review Complete!

**Story Status:** done
**Issues Fixed:** 3
**Action Items Created:** 0

Code review complete!
```

The analyzer needs `REVIEW_COMPLETE_PATTERNS` to detect this output and return `ResponseAction::Completed`. In `drive_review_session()`, this triggers the post-review phase:

1. The chat loop checks `post_review_phase` flag **before** calling the analyzer
2. **If `post_review_phase == false`**: Analyze response normally. On `Completed` → set `post_review_phase = true`, send `POST_REVIEW_MESSAGE`, continue loop.
3. **If `post_review_phase == true`**: **Skip the analyzer entirely.** The agent's response IS the report — it's free-form markdown (commit confirmations + review summary). No completion patterns will be present. Capture the response as `report` and return `ReviewOutcome::Completed { story_key, branch, report }`.

**Why skip the analyzer in post_review_phase?** After `POST_REVIEW_MESSAGE`, the agent commits via GitTool and responds with a markdown report. This response won't contain any `COMPLETION_SIGNALS` or `REVIEW_COMPLETE_PATTERNS` — it's free-form text. Trying to detect completion here would require fragile pattern matching. Instead, we treat the next response as the final report by design. One message in, one report out.

This two-phase approach ensures:
- The BMAD agent commits with full context (it knows what it fixed and why → good commit messages)
- The review report is authored by the agent (no parsing needed by the daemon)
- The daemon remains a launcher — the only "intelligence" is the `POST_REVIEW_MESSAGE` constant
- No fragile pattern matching on the report response — it's captured directly

**Priority ordering in `analyze()` — interaction with existing `COMPLETION_SIGNALS`:**

The existing `analyzer.rs` has `COMPLETION_SIGNALS` at priority 2, matching dev-session phrases like "all tasks completed", "story implementation complete", "story is ready for review". The CR workflow's step 5 output ("Review Complete!", "Code review complete!") does **NOT** match any existing `COMPLETION_SIGNALS` — they are dev-session specific.

`REVIEW_COMPLETE_PATTERNS` must be checked **before** both `COMPLETION_SIGNALS` (priority 2) and `REVIEW_FIX_PATTERNS` to avoid the step 5 summary (which contains words like "Issues Fixed") from accidentally triggering the fix-decision auto-response. Suggested priority order:

1. Escalation (existing priority 1) — `escalation_slot` check
2. **Review completion detection** (`REVIEW_COMPLETE_PATTERNS` → `Completed`) — NEW
3. Completion signals (existing priority 2) — `COMPLETION_SIGNALS` (dev-session only)
4. Confirmation/proceed (existing priority 3)
5. Step-by-step (existing priority 4)
6. YOLO (existing priority 5)
7. **Review fix decision** (`REVIEW_FIX_PATTERNS` → `Continue { reply: "1" }`) — NEW
8. Story selection (`STORY_SELECTION_PATTERNS` → `Continue { reply: story_reply }`)
9. Default fallback

### Provider Match-Arm Pattern (from `SessionRunner` and `ArchitectSession`)

rig's `Chat` trait is NOT object-safe. The `ReviewRunner` must follow the same match-arm pattern:

```rust
match provider.as_str() {
    "anthropic" => {
        let client = anthropic::Client::builder().api_key(&api_key).build()
            .map_err(|e| ReviewError::ProviderInit { reason: e.to_string() })?;
        let agent = client.agent(&model).preamble(&preamble)
            .tool(git).tool(fs).tool(terminal).tool(supervisor).build();
        self.drive_review_session(&agent, story, escalation_slot, decision_log).await
    }
    "openai" => { /* same pattern */ }
    "github-models" => { /* openai with base_url = "https://models.inference.ai.azure.com" */ }
    other => Err(ReviewError::UnsupportedProvider { provider: other.into() })
}
```

Reference: `src/supervisor/architect.rs` lines 275-343 (cleanest example), `src/session/runner.rs` lines 210-300 (with tools).

### Chat Loop Pattern

The review chat loop follows `SessionRunner::run_session()` with one key addition — a **post-review phase**:

1. Send initial message (`"CR"` instead of `"DS"`)
2. Initialize `post_review_phase = false`
3. On each agent response, check `post_review_phase` **first**:
   - **If `post_review_phase == true`**: **Skip the analyzer entirely.** The agent's response IS the report (free-form markdown with commit confirmations + review summary). Capture it as `report` and return `ReviewOutcome::Completed { story_key, branch, report }`.
   - **If `post_review_phase == false`**: Analyze response with `ResponseAnalyzer` normally:
     - On `Continue { reply }` → send reply, continue
     - On `Escalated` → return `ReviewOutcome::Failed` (review escalation is treated as failure, non-blocking)
     - On `Completed` → set `post_review_phase = true`, send `POST_REVIEW_MESSAGE`, continue loop
4. Max turns safety net: `MAX_REVIEW_TURNS = 100`
5. On chat error: retry up to 3 times (same as dev session), then `ReviewOutcome::Failed`

**Why skip the analyzer in post_review_phase?** After `POST_REVIEW_MESSAGE`, the agent commits via GitTool and responds with a markdown report. This response won't contain any `COMPLETION_SIGNALS` or `REVIEW_COMPLETE_PATTERNS` — it's free-form text. Trying to detect completion here would require fragile pattern matching. Instead, we treat the next response as the final report by design. One message in, one report out.

**Post-review message constant:**
```rust
const POST_REVIEW_MESSAGE: &str = "Commit all your review fixes with descriptive commit messages \
    that reference the findings. Then provide a complete markdown summary of your code review \
    (findings, severity, fixes applied, remaining concerns) suitable for posting as a PR comment.";
```

**No WAL file for review sessions.** Review is non-critical and non-blocking — if it crashes, it's simply skipped. No crash recovery needed.

### Cross-Module Imports

The review module imports from several other modules:

```rust
use crate::config::{BotConfig, BotSecrets};
use crate::session::provider::{resolve_api_key, ProviderError};
use crate::session::analyzer::{ResponseAction, ResponseAnalyzer};
use crate::supervisor::{AskSupervisor, EscalationSlot};
use crate::supervisor::decisions::DecisionLog;
use crate::tools::{GitTool, FsTool, TerminalTool};
use crate::watcher::StoryInfo;
```

These are all read-only imports — no circular dependencies. The `review` module is a peer of `session`, not a child.

### What the Review Module Does NOT Do

- ❌ Does NOT build a custom review preamble — loads `dev.md` (same as dev session)
- ❌ Does NOT generate diffs — the CR workflow does this internally via git tools
- ❌ Does NOT parse review output — the BMAD CR workflow handles findings and fixes
- ❌ Does NOT commit review fixes directly — the post-review message asks the BMAD agent to commit (the agent has full context for descriptive commit messages)
- ❌ Does NOT post comments on the PR — the orchestrator does this after PR creation using the `report` from `ReviewOutcome::Completed`
- ❌ Does NOT manage branch creation — already on story branch from dev session
- ❌ Does NOT write WAL files — review is non-critical, no crash recovery
- ❌ Does NOT know anything about BMAD workflows — it's just a session launcher with a post-review phase

### Edge Case: No Code Changes After Review

The CR workflow is adversarial (minimum 3-10 issues), but some findings may be non-code (documentation gaps, naming suggestions noted but not auto-fixed, architecture observations). If `POST_REVIEW_MESSAGE` asks the agent to "commit all your review fixes" and there are no staged changes, the agent should respond that there is nothing to commit and still provide the review report. This is a valid `ReviewOutcome::Completed` — the `report` field captures the findings summary regardless of whether commits were made. The orchestrator posts the report as a PR comment either way.

### Error Handling — Non-Blocking Review Failures

**Critical design rule:** `ReviewRunner::run()` NEVER causes PR creation to fail. All errors are caught and returned as `ReviewOutcome::Skipped` or `ReviewOutcome::Failed`. The orchestrator always proceeds to PR creation regardless of review outcome.

The two-method pattern (`run` wraps `run_inner`) ensures clean error handling:
- `run_inner()` → `Result<ReviewOutcome, ReviewError>` (can fail)
- `run()` → `ReviewOutcome` (never fails — catches errors from `run_inner`)

### Files Created/Modified in This Story

| File | Change |
|------|--------|
| `src/review/mod.rs` | **OVERWRITE** — Replace TODO skeleton with `ReviewRunner`, `ReviewOutcome`, `ReviewError`, tests |
| `src/session/analyzer.rs` | **MODIFY** — Add `REVIEW_FIX_PATTERNS` constant + new priority level + 3 tests |
| `src/config/mod.rs` | **MODIFY** — Add `code_review_enabled: bool` field + default + 2 tests |
| `bmad-bot.yaml.example` | **MODIFY** — Add `code_review_enabled` entry with comment |

### Anti-Patterns to Avoid

- ❌ **No `unwrap()` or `expect()` in production code** — only in tests
- ❌ **No `anyhow::Result`** in library modules — use `ReviewError` exclusively
- ❌ **No `println!` or `eprintln!`** — use `tracing` only
- ❌ **No real API calls in unit tests** — mock everything, real calls only in E2E
- ❌ **No logging of API tokens** — never log API key values
- ❌ **Review failure must NEVER block PR creation** — always return `ReviewOutcome::Skipped/Failed` on error
- ❌ **Do NOT build a custom review preamble** — load `dev.md` exactly like the dev session does
- ❌ **Do NOT implement diff generation** — the BMAD CR workflow handles this via git tools
- ❌ **Do NOT parse review output** — the BMAD CR workflow handles findings and fixes
- ❌ **Do NOT commit review fixes from the daemon** — the post-review message asks the BMAD agent to commit (it has the context for good commit messages)
- ❌ **Do NOT call `GitProvider::add_comment()` from the review module** — orchestrator handles PR operations using the `report` field from `ReviewOutcome::Completed`
- ❌ **Do NOT implement the orchestration** — this module provides `ReviewRunner`, the watcher loop integration is future scope
- ❌ **Do NOT add retry logic for LLM calls** — that's Epic 6 (Story 6.2) scope. Chat-level retries (3 attempts) are OK (same as dev session).
- ❌ **Do NOT write WAL files** — review is non-critical, no crash recovery needed

### Scope Boundaries

**IN SCOPE:**
- `ReviewError` thiserror enum (6 variants)
- `ReviewOutcome` enum (3 variants: Completed with `report`, Failed, Skipped)
- `ReviewRunner` struct with `new()` and `run()` methods
- `drive_review_session()` chat loop with post-review phase (commit + report capture)
- `POST_REVIEW_MESSAGE` constant for the post-review instruction
- `REVIEW_FIX_PATTERNS` in `ResponseAnalyzer`
- `REVIEW_COMPLETE_PATTERNS` in `ResponseAnalyzer` (to detect CR step 5 completion)
- `code_review_enabled` config field in `BotConfig`
- Unit tests for all the above

**OUT OF SCOPE:**
- Calling `ReviewRunner` from the watcher/session loop (future orchestration integration)
- Posting review results as PR comment (orchestrator calls `GitProvider::add_comment()` using `ReviewOutcome.report`)
- GitLab support (Story 5.3)
- Retry/resilience for LLM calls (Story 6.2)
- Notifications about review results (Story 6.1)
- WAL/crash recovery for review sessions (non-critical)

### Testing Requirements

All tests follow Arrange → Act → Assert pattern. Test naming: `test_{module}_{behavior}_{scenario}`.

**Unit tests (no network, no API calls):**
- Error enum Display implementations (all 6 variants)
- ReviewOutcome enum construction and field verification (including `report` field on `Completed`)
- ReviewRunner construction
- Send + Sync trait bounds compile check
- ResponseAnalyzer review fix patterns (3 tests in analyzer.rs)
- ResponseAnalyzer review complete patterns (2 tests in analyzer.rs):
  - `test_analyzer_detects_review_complete` — verify "Review Complete" output triggers Completed
  - `test_analyzer_review_complete_does_not_false_positive` — verify normal text with "complete" doesn't trigger
- Config `code_review_enabled` default and parsing (2 tests in config/mod.rs)

**E2E tests (future, gated behind `BMAD_E2E=1`):**
- Real LLM review session with CR workflow → not in this story

### Dev Dependencies Required

No new dependencies needed. All already present in `Cargo.toml`:
- `rig-core = "0.30"` — agent builder and Chat trait
- `thiserror = "2"` — error enum derive
- `tracing = "0.1"` — structured logging
- `tempfile = "3"` (dev-dependency) — available for tests

### Project Structure Notes

After this story, the `src/review/` directory will be:

```
src/review/
└── mod.rs    # ReviewRunner, ReviewOutcome, ReviewError, drive_review_session, tests
```

This aligns with the architecture's Complete Project Directory Structure.

### References

- [Source: src/watcher/mod.rs#StoryInfo] — `specs_path: PathBuf` field used as `story_reply` parameter in `ResponseAnalyzer.analyze()` for CR workflow story selection
- [Source: src/session/analyzer.rs#COMPLETION_SIGNALS] — Existing dev-session completion patterns (priority 2). `REVIEW_COMPLETE_PATTERNS` must not overlap — verified no conflict
- [Source: _bmad/bmm/agents/dev.md#menu] — `[CR] Code Review` menu item triggers `code-review/workflow.yaml`
- [Source: _bmad/bmm/workflows/4-implementation/code-review/workflow.yaml] — Full CR workflow config
- [Source: _bmad/bmm/workflows/4-implementation/code-review/instructions.xml] — CR workflow steps: discover changes, review, fix, update status
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 1: Supervisor Interception Model] — Chat trait not object-safe, match-arm pattern
- [Source: _bmad-output/planning-artifacts/architecture.md#Decision 4: Error Propagation — Layered with Bubble-Up] — Review failures non-blocking
- [Source: _bmad-output/planning-artifacts/architecture.md#Data Flow] — Step 7: Optional review after session end
- [Source: _bmad-output/planning-artifacts/architecture.md#Complete Project Directory Structure] — `src/review/mod.rs`
- [Source: _bmad-output/planning-artifacts/epics.md#Story 5.2] — Acceptance criteria and user story
- [Source: _bmad-output/planning-artifacts/epics.md#Epic 5 Overview] — FRs covered: FR18, FR19, FR20
- [Source: _bmad-output/project-context.md#Code Review] — Optional, configurable, separate LLM, fixes in separate commits
- [Source: _bmad-output/project-context.md#Multi-Provider LLM Config] — Three independent roles: dev, review, supervisor
- [Source: _bmad-output/project-context.md#Daemon Role] — Daemon is a launcher, not an executor
- [Source: src/session/runner.rs#run] — Dev session lifecycle: API key → branch → agent → chat loop → outcome
- [Source: src/session/runner.rs#run_session] — Chat loop pattern: send message, analyze, auto-respond, max turns
- [Source: src/session/runner.rs#build_preamble] — Loads dev.md + language override
- [Source: src/session/runner.rs#build_anthropic_agent] — Agent builder with 4 tools
- [Source: src/session/analyzer.rs#ResponseAnalyzer] — Priority-based pattern matching, story_key reply
- [Source: src/session/analyzer.rs#STORY_SELECTION_PATTERNS] — Story selection → reply with story_key parameter
- [Source: src/session/provider.rs#resolve_api_key] — API key resolution reused by review module
- [Source: src/supervisor/architect.rs#ArchitectSession] — Cleanest match-arm pattern reference
- [Source: src/session/mod.rs#SessionOutcome] — Completed variant triggers review
- [Source: src/tools/] — GitTool, FsTool, TerminalTool reused by review agent
- [Source: src/config/mod.rs#BotConfig] — Where to add `code_review_enabled`
- [Source: src/config/mod.rs#LlmConfig] — `review: LlmRoleConfig` already exists
- [Source: src/review/mod.rs] — Current skeleton (TODO only)
- [Source: bmad-bot.yaml.example] — Where to add `code_review_enabled` entry
- [Source: _bmad-output/implementation-artifacts/5-1-git-provider-trait-github-pr-creation.md] — Story 5.1 context
- [Source: Cargo.toml] — rig-core 0.30, thiserror 2, async-trait 0.1

## Dev Agent Record

### Agent Model Used

{{agent_model_name_version}}

### Debug Log References

### Completion Notes List

### File List