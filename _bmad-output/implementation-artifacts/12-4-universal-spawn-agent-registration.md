# Story 12.4: Universal SpawnAgent Registration

Status: done

<!-- Note: Validation is optional. Run validate-create-story for quality check before dev-story. -->

## Story

As a daemon operator,
I want the `spawn_agent` tool registered in all agent sessions that should have delegation capability,
So that any LLM agent (dev, review) can delegate well-scoped work to sub-agents with follow-up via `session_id`, and the shared sessions map's lifecycle is owned by the pipeline.

## Acceptance Criteria

1. **AC-1: Shared sub-agent sessions map created once per daemon run**
   - **Given** `StoryPipeline::new()` in `src/pipeline.rs:158` is the daemon-run constructor that owns `AgentFactory`
   - **When** this story is implemented
   - **Then** `StoryPipeline::new()` creates two new shared state values alongside `agent_factory`:
     - `sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>>` — the sub-agent sessions map declared in Story 12.3 (`src/tools/spawn_agent.rs:168`)
     - `sub_agent_in_flight: Arc<Mutex<HashSet<String>>>` — the in-flight-follow-up set declared in Story 12.3 (`src/tools/spawn_agent.rs:172`) and required by `SpawnAgentTool::new()` (`src/tools/spawn_agent.rs:189–205`)
   - **And** both Arcs are stored as `StoryPipeline` fields `sub_agent_sessions` and `sub_agent_in_flight` next to `session_runner` / `review_runner`
   - **And** a `pub(crate) fn sub_agent_state_counts(&self) -> (usize, usize)` method is added to `StoryPipeline` (first `impl` block) returning `(sessions.len(), in_flight.len())` using the same poison-safe lock policy as the cleanup guard — used by the integration test in AC-9 to observe map state without unsafe reach-into-fields
   - **And** both Arcs are cloned into the new parameters added to `SessionRunner::new()` and `ReviewRunner::new()` (AC-4, AC-5) — `EpicReviewRunner::new()` does **not** receive them (AC-7)
   - **And** new imports are added at the top of `src/pipeline.rs`, verbatim — do NOT let rust-analyzer auto-import resolve `Mutex` to `tokio::sync::Mutex` (the project uses `std::sync::Mutex` uniformly per Story 12.3):
     ```rust
     use std::collections::{HashMap, HashSet};
     use std::sync::{Arc, Mutex};  // std, NOT tokio
     use crate::tools::SubAgentState;  // re-exported from tools/mod.rs per AC-8
     ```
   - **And** `StoryPipeline` holds exactly one daemon-run instance of each Arc; subsequent stories share the same map (with contents cleared between stories per AC-2)

2. **AC-2: Sessions map cleared between stories, never persisted**
   - **Given** `StoryPipeline::process_story()` (method defined inside the `impl StoryPipeline` block starting around `src/pipeline.rs:150`) orchestrates dev session → code review → PR creation → notification for a single story
   - **When** `process_story()` returns (any outcome: success, partial failure, escalation, or panic unwinding)
   - **Then** the `sub_agent_sessions` map and `sub_agent_in_flight` set are cleared so that sub-agent sessions from story A do NOT survive into story B:
     - Implemented as an RAII guard (`StorySubAgentCleanup`) constructed at the top of `process_story()` — its `Drop` impl calls `self.sub_agent_sessions.lock().unwrap_or_else(|p| p.into_inner()).clear()` and the same pattern for `in_flight`. Using RAII guarantees cleanup on early-return AND panic-unwind. Return type of `process_story()` does not change.
     - The guard lock uses the exact same poison-recovery pattern as `SpawnAgentTool::lock_sessions()` (Story 12.3 `src/tools/spawn_agent.rs:212–219`) — never panics the daemon on a poisoned mutex
     - **`Drop` impl MUST check `std::thread::panicking()` before emitting `tracing::info!`** — if the guard is dropping during an unwind and the tracing subscriber itself panics (e.g., writing to a closed file descriptor), a double-panic aborts the daemon. Skip the log in that case; the clear still happens.
   - **And** the same cleanup is applied to `StoryPipeline::resume_story_from_wal()` (the method near `src/pipeline.rs:1724` that calls `session_runner.check_and_recover_wal()` and `session_runner.resume_session()`) — a symmetric `let _sub_agent_cleanup = ...;` at the method top
   - **And** Task 4.11 explicitly audits **all three** `impl StoryPipeline` blocks (near lines 150, 973, 1575) to confirm no OTHER story-processing entry point exists. If one does (e.g., a compat path), it receives the same guard. If none does, the audit is documented in Completion Notes.
   - **And** the Arcs are NOT replaced between stories — only the contents cleared — so `SessionRunner` / `ReviewRunner` instances (constructed once in `StoryPipeline::new()` per AC-1) continue holding the same clones
   - **And** `SubAgentState` is NEVER serialized to the WAL: `SessionState` in `src/session/state.rs` is untouched; the existing Story 12.3 doc comment "In-memory-only state — never serialized" remains the source of truth. **Note on parent-session history:** the parent session's `ChatMessage` history (which IS WAL-serialized) may contain tool-call results referencing a sub-agent `session_id`. After daemon restart, those ids point to nothing — a follow-up attempt returns `SpawnAgentError::SessionNotFound`. This is an inherited property from Story 12.3, not a regression; documented in Dev Notes for operator awareness.

3. **AC-3: `create_spawn_agent_tool()` helper in `src/session/agent.rs`**
   - **Given** the convention in `src/session/agent.rs` of centralizing tool construction via `create_base_tools()` (`src/session/agent.rs:75–86`) and `create_tools_with_supervisor()` (`src/session/agent.rs:92–112`)
   - **When** this story is implemented
   - **Then** a new `pub(crate) fn create_spawn_agent_tool(...)` is added to `src/session/agent.rs` immediately after `create_tools_with_supervisor()` with signature:
     ```rust
     pub(crate) fn create_spawn_agent_tool(
         agent_factory: &Arc<AgentFactory>,
         role: LlmRole,
         project_root: &Path,
         sessions: &Arc<Mutex<HashMap<String, SubAgentState>>>,
         in_flight: &Arc<Mutex<HashSet<String>>>,
         shutdown: Option<&ShutdownFlag>,
     ) -> SpawnAgentTool
     ```
     - Body: `SpawnAgentTool::new(Arc::clone(agent_factory), role, project_root.to_path_buf(), Arc::clone(sessions), Arc::clone(in_flight), shutdown.cloned())`
     - The helper takes references and clones the Arcs internally — call sites do not need to write `Arc::clone` four times
   - **And** the helper is `pub(crate)` — no in-tree external caller needs it outside the crate, and `pub(crate)` prevents future misuse from downstream consumers
   - **And** required new imports in `src/session/agent.rs` — verbatim (`std::sync::Mutex`, NOT `tokio::sync::Mutex`):
     ```rust
     use crate::tools::{SpawnAgentTool, SubAgentState};
     use std::collections::{HashMap, HashSet};
     ```
     `LlmRole`, `AgentFactory`, `Arc`, and `Mutex` (from `std::sync`) are already imported in this file — DO NOT duplicate. Grep-verify before adding. If rust-analyzer auto-imports suggest `tokio::sync::Mutex`, reject and write `std::sync::Mutex` manually.
   - **And** a `#[cfg(test)] pub(crate) fn role_for_tests(&self) -> LlmRole` accessor is added to `impl SpawnAgentTool` in `src/tools/spawn_agent.rs` (return `self.role`) — this is an **additive, test-only** change to the Story 12.3 tool and is explicitly sanctioned for Story 12.4 because the role field assertion (AC-3 test) cannot rely on `Debug` formatting (brittle against any future custom `Debug` impl). Note: `LlmRole` must be `Copy` or `Clone` — it derives both (`src/llm/agent_factory.rs:37`). This adds exactly 3 lines under a `#[cfg(test)]` block.
   - **And** a unit test `test_create_spawn_agent_tool_role_matches_parent` in `src/session/agent.rs::tests` asserts the returned tool's role via the accessor: `let tool = create_spawn_agent_tool(..., LlmRole::Review, ...); assert_eq!(tool.role_for_tests(), LlmRole::Review);` — requires `LlmRole` to derive `PartialEq`; if it does not, fall back to `matches!(tool.role_for_tests(), LlmRole::Review)`. Verify at implementation time.
   - **And** a unit test `test_create_spawn_agent_tool_shares_arcs` asserts `Arc::strong_count` for both `sessions` and `in_flight` after the helper is called is **`>= 2`** (caller holds one, tool holds one — but allow for future internal cloning). Use `>=` not `==` and add comment: `// >= 2, not == 2: internal Arc cloning is an implementation detail.`
   - **Do NOT add `SpawnAgentTool` to `create_base_tools()`** — that function's callers include `SpawnAgentTool::spawn_new()` itself (`src/tools/spawn_agent.rs:414`) and `ArchitectSession::ask()` (`src/supervisor/architect.rs:343`). Adding the tool to `create_base_tools()` would (a) require threading all 5 new args to every call site, (b) hand `SpawnAgentTool` to sub-agents, which is explicitly forbidden by Story 12.3's Anti-Patterns section ("DO NOT give sub-agents the `SpawnAgentTool` itself — prevents unbounded nested delegation"), and (c) register it on the Architect recursively. The `create_spawn_agent_tool()` helper is the **intentional deviation** from the epic AC letter ("included in `create_base_tools()` alongside ...") — documented in Dev Notes. The epic's spirit ("registered in all sessions that should delegate") is preserved.

4. **AC-4: `SessionRunner` constructed with sub-agent Arcs and wires `SpawnAgentTool`**
   - **Given** `SessionRunner` struct (`src/session/runner.rs:311–329`) and its `new()` constructor (`src/session/runner.rs:340–359`)
   - **When** this story is implemented
   - **Then** two new fields are added to `SessionRunner`:
     - `sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>>`
     - `sub_agent_in_flight: Arc<Mutex<HashSet<String>>>`
   - **And** `SessionRunner::new()` signature gains two new parameters inserted **after** `mcp_manager: Arc<McpManager>` and **before** `ui: UiHandle` (stable ordering: config → agent_factory → shutdown → mcp_manager → sub-agent Arcs → ui):
     ```rust
     pub fn new(
         config: Arc<BotConfig>,
         agent_factory: Arc<AgentFactory>,
         shutdown: ShutdownFlag,
         mcp_manager: Arc<crate::mcp::McpManager>,
         sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>>,
         sub_agent_in_flight: Arc<Mutex<HashSet<String>>>,
         ui: crate::ui::UiHandle,
     ) -> Self
     ```
   - **And** `src/session/runner.rs`'s tool-building site — locate by searching for the UNIQUE pattern `agent::create_tools_with_supervisor(` inside an `impl SessionRunner` method (approximately `src/session/runner.rs:812–824`; verify via `Grep` at implementation time — do NOT trust the absolute line number if any edit has preceded this one) — is updated to:
     1. Call the new helper: `let spawn_agent = agent::create_spawn_agent_tool(&self.agent_factory, LlmRole::Dev, &project_root, &self.sub_agent_sessions, &self.sub_agent_in_flight, Some(&self.shutdown));`
     2. Extend the `configure_agent_tools!` tuple to include `spawn_agent`: `configure_agent_tools!(git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor, spawn_agent, ThinkTool)` — **10 tools** (was 9; under the macro's arity-12 ceiling at `src/llm/agent_factory.rs:525–528`)
   - **And** ALL existing unit tests in `src/session/runner.rs` that construct `SessionRunner::new()` are updated to pass the two new Arcs. Discover them via `Grep` for the pattern `SessionRunner::new(` inside `#[cfg(test)] mod tests` (approximately 9 call sites based on Story 12.4 research but the actual count MUST be verified by grep at implementation time — do NOT rely on cached line numbers). Add a private test helper next to `make_test_mcp_manager()`:
     ```rust
     fn make_empty_sub_agent_arcs() -> (
         Arc<Mutex<HashMap<String, SubAgentState>>>,
         Arc<Mutex<HashSet<String>>>,
     ) {
         (Arc::new(Mutex::new(HashMap::new())), Arc::new(Mutex::new(HashSet::new())))
     }
     ```
     ONE definition, called from every test that needs it.
   - **And** a new test `test_session_runner_stores_sub_agent_arcs` (modeled on the existing `test_session_runner_stores_mcp_manager`) verifies `Arc::strong_count` of the passed-in Arcs is **`>= 2`** after `SessionRunner::new()` returns (caller holds one, runner holds one; tolerate future internal cloning) and decreases back to 1 after the runner is dropped

5. **AC-5: `ReviewRunner` constructed with sub-agent Arcs and wires `SpawnAgentTool`**
   - **Given** `ReviewRunner` struct (`src/review/mod.rs:309–324`) and its `new()` constructor (`src/review/mod.rs:326–345`)
   - **When** this story is implemented
   - **Then** two new fields are added to `ReviewRunner`:
     - `sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>>`
     - `sub_agent_in_flight: Arc<Mutex<HashSet<String>>>`
   - **And** `ReviewRunner::new()` signature gains two new parameters inserted **after** `mcp_manager` and **before** `ui` (same position convention as AC-4)
   - **And** `build_review_agent` (locate via `Grep` for `async fn build_review_agent(`, approximately `src/review/mod.rs:432–475`) is updated to:
     1. Call `let spawn_agent = agent::create_spawn_agent_tool(&self.agent_factory, LlmRole::Review, &project_root, &self.sub_agent_sessions, &self.sub_agent_in_flight, Some(&self.shutdown));`
     2. Extend the `configure_agent_tools!` tuple inside `build_review_agent` to include `spawn_agent`: `configure_agent_tools!(git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor, spawn_agent, ThinkTool)` — **10 tools**
   - **And** ALL existing `ReviewRunner::new()` test call sites (grep pattern: `ReviewRunner::new(`) are updated to pass the two new Arcs — approximately 1 test site based on research but verify
   - **And** a new test `test_review_runner_stores_sub_agent_arcs` mirrors the SessionRunner test (strong-count `>= 2` assertion)
   - **And** the `LlmRole::Review` argument means sub-agents spawned inside a review session use the **review** provider/model pairing — a review session's sub-agent is cheaper than a dev sub-agent if the review model is a smaller Sonnet; the epic explicitly sanctions this inheritance in Story 12.3's `SubAgentState { role: LlmRole, ...}` field

6. **AC-6: `StoryPipeline::new()` threads sub-agent Arcs to runners**
   - **Given** `StoryPipeline::new()` constructs `agent_factory`, `session_runner`, `review_runner`, `epic_review_runner` (`src/pipeline.rs:188–213`)
   - **When** this story is implemented
   - **Then** immediately after `agent_factory` is constructed at `src/pipeline.rs:189` (locate by searching for `let agent_factory = Arc::new(AgentFactory::new(`), add:
     ```rust
     let sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>> =
         Arc::new(Mutex::new(HashMap::new()));
     let sub_agent_in_flight: Arc<Mutex<HashSet<String>>> =
         Arc::new(Mutex::new(HashSet::new()));
     ```
   - **And** the `SessionRunner::new(...)` call passes `Arc::clone(&sub_agent_sessions)` and `Arc::clone(&sub_agent_in_flight)` in the new parameter positions (before `ui.clone()`)
   - **And** the `ReviewRunner::new(...)` call passes `Arc::clone(&sub_agent_sessions)` and `Arc::clone(&sub_agent_in_flight)` in the new parameter positions
   - **And** the `EpicReviewRunner::new(...)` call is **unchanged** — `EpicReviewRunner::new()` signature is unchanged (AC-7). The EpicReviewRunner site must NOT receive clones of the sub-agent Arcs; verify at implementation time
   - **And** the Arcs are stored in `StoryPipeline` fields (per AC-1) so they remain reachable for the cleanup guard in `process_story()` (AC-2): `Self { config, git_provider, notifier, session_runner, review_runner, epic_review_runner, sub_agent_sessions, sub_agent_in_flight, ui }`
   - **And** `src/pipeline.rs` imports added verbatim (see AC-1). Before adding `use std::sync::{Arc, Mutex};`, grep for existing `use std::sync::` lines and merge rather than duplicate. DO NOT accept a `tokio::sync::Mutex` auto-import.

7. **AC-7: `EpicReviewRunner` and `ArchitectSession` deliberately NOT extended**
   - **Given** Story 12.3 Anti-Pattern: "DO NOT inherit `LlmRole::Supervisor` blindly when registering the tool in Architect sessions — Story 12.4 must make an explicit decision"
   - **And** the epic-review agent's read-only invariant: `src/review/epic.rs:203–210` builds a 6-tool set (read_file, grep, find_path, list_dir, terminal, git) **deliberately omitting `edit_file`**
   - **When** this story is implemented
   - **Then** `EpicReviewRunner` does **NOT** register `SpawnAgentTool`:
     - The `configure_agent_tools!` call inside `build_epic_review_agent` (approximately `src/review/epic.rs:216–218`) is unchanged (`git, read_file, grep, find_path, list_dir, terminal, ThinkTool` — 7 tools)
     - `EpicReviewRunner::new()` signature is unchanged
     - Rationale (required doc comment): the EpicReview session maintains a read-only invariant by omitting `edit_file` from its native tool set. Registering `SpawnAgentTool` would allow an escape via a sub-agent that DOES receive `edit_file` (see `src/tools/spawn_agent.rs:414–436`). Dev and Review already have `edit_file` natively — the concern applies only to EpicReview. A future follow-up story could add role-aware sub-agent tool sets (sub-agents of an EpicReview parent would omit `edit_file`), but that work is explicitly out of scope here.
     - A short doc comment is added above the `configure_agent_tools!` call in `src/review/epic.rs`: `// NOTE (Story 12.4): SpawnAgentTool intentionally NOT registered — sub-agents receive edit_file unconditionally (see spawn_agent.rs:414–436), which would let EpicReview escape its read-only invariant. Revisit if sub-agent tool sets become role-aware.`
   - **And** `ArchitectSession` is **NOT** migrated to `spawn_agent`:
     - The 4-turn scripted conversation in `ArchitectSession::drive_conversation()` (`src/supervisor/architect.rs:246–332`) executes activation via `architect.md` as a user message → `"Execute [CH]"` to enter free-chat mode → `"Load the project context"` → the developer's question. `spawn_agent` does not support multi-turn activation semantics — it takes a single self-contained `message` (see `SpawnAgentArgs` at `src/tools/spawn_agent.rs:80–91`)
     - Migrating would require collapsing the CH/context/question pattern into a single mega-message, losing the BMAD persona activation handshake and breaking the explicit three-stage tracing (`supervisor_fallback` turn 1/2/3)
     - The `configure_agent_tools!` call inside `ArchitectSession::ask()` (approximately `src/supervisor/architect.rs:362–364`) is unchanged (7 tools + ThinkTool — no SpawnAgentTool)
     - A TODO comment is added above that `configure_agent_tools!` call, explicitly referencing architecture.md Decision 10 so the next reader inherits the design context:
       ```rust
       // TODO (post-12.4): Migrate the Architect off the 4-turn scripted handshake.
       //
       // The current flow (architect.md activation → [CH] → load-context → question)
       // does not map to spawn_agent's single-message contract. Per architecture.md
       // Decision 10 (_bmad-output/planning-artifacts/architecture.md:664–694), the
       // Architect is a supervisor-fallback AnswerProvider, not an LLM-initiated
       // delegation target — so a straight spawn_agent migration would also conflate
       // the two delegation flavors. Migration becomes feasible once either:
       //   (a) spawn_agent gains a multi-turn activation API, or
       //   (b) the Architect adopts a skill-based activation path (Story 12.1
       //       precedent for dev sessions).
       // Tracked by Story 12.4 AC-7.
       ```
     - **Also NOT registered as a tool in ArchitectSession's own tool set** — the Architect runs on `LlmRole::Supervisor`; registering `SpawnAgentTool` would create a supervisor-spawns-supervisor recursion path. Keeping Supervisor role free of `SpawnAgentTool` closes that recursion.

8. **AC-8: Dead-code suppressions removed and `SubAgentState` re-exported**
   - **Given** Story 12.3 added temporary suppressions pending 12.4 wiring:
     - `#![allow(dead_code)]` at `src/tools/spawn_agent.rs:38` (module-wide)
     - `#[allow(unused_imports)]` at `src/tools/mod.rs:29` on `pub use spawn_agent::SpawnAgentTool;`
     - `#[allow(dead_code)]` on `build_sub_agent_preamble` in `src/session/agent.rs` (added by Story 12.3 Debug Log; grep-verify location — search for `build_sub_agent_preamble` and check for a preceding `#[allow(dead_code)]`)
   - **When** this story is implemented
   - **Then** ALL THREE suppressions are **removed** — once `SpawnAgentTool` is wired into `SessionRunner` and `ReviewRunner` (AC-4, AC-5) and `build_sub_agent_preamble` is called indirectly via `SpawnAgentTool::spawn_new`, the struct, args, helpers, preamble builder, and re-export are all reachable from the bin target
   - **And** the comment blocks explaining the suppressions (grep for the exact lines `// All items in this module are reachable only from tests until Story 12.4 wires` and the adjacent comment lines in `spawn_agent.rs`) are also deleted — they now misrepresent the file state
   - **And** `src/tools/mod.rs` adds a `pub use spawn_agent::SubAgentState;` re-export next to the existing `pub use spawn_agent::SpawnAgentTool;` line so downstream modules can import via the clean `crate::tools::SubAgentState` path. Update the module-level doc comment (`src/tools/mod.rs:10–12`) to add a one-liner: `//! - **[`SubAgentState`]** — in-memory state of a live sub-agent session (see `SpawnAgentTool`).`
   - **And** `cargo build` produces zero new `dead_code` or `unused_imports` warnings in `src/tools/spawn_agent.rs`, `src/tools/mod.rs`, `src/session/agent.rs`, `src/session/runner.rs`, `src/review/mod.rs`, or `src/pipeline.rs`
   - **And** if any per-item `#[allow(dead_code)]` remains necessary after the three removals above, it is applied per-field with a brief `// <reason>` comment — NOT applied module-wide. Expect zero per-field suppressions to be needed.

9. **AC-9: Integration test — `sub_agent_sessions` cleared between stories**
   - **Given** the post-12.3 test baseline: **1142 passing, 1 pre-existing failure** (`session::runner::tests::test_build_context_limit_recovery_message_contains_all_sections`)
   - **When** `cargo test` is run
   - **Then** TWO tests are added in `src/pipeline.rs::tests`:
     1. **`test_story_sub_agent_cleanup_clears_on_drop`** — unit test of the `StorySubAgentCleanup` guard in isolation:
        - Create two Arcs directly (no `StoryPipeline` involved): `let sessions = Arc::new(Mutex::new(HashMap::new())); let in_flight = Arc::new(Mutex::new(HashSet::new()));`
        - Insert a dummy `String` into `in_flight` (HashSet<String> — simple, no BuiltAgent needed). DO NOT try to insert into `sessions` (map value is `SubAgentState` which requires a real `BuiltAgent` — prohibitive in unit tests).
        - Construct the guard in an inner scope: `{ let _guard = StorySubAgentCleanup { sessions: &sessions, in_flight: &in_flight }; }`
        - Assert `in_flight.lock().unwrap().is_empty()` and `sessions.lock().unwrap().is_empty()` after the scope
     2. **`test_process_story_installs_cleanup_guard_source_check`** — source-level assertion that the guard is actually installed at the top of `process_story()`. Uses `include_str!("pipeline.rs")` to read the current source file and asserts the substring `"let _sub_agent_cleanup = StorySubAgentCleanup"` appears inside a method signature block beginning with `pub async fn process_story`. Exact test body:
        ```rust
        #[test]
        fn test_process_story_installs_cleanup_guard_source_check() {
            let src = include_str!("pipeline.rs");
            // Find the start of process_story's body.
            let process_story_pos = src
                .find("pub async fn process_story(")
                .expect("process_story method must exist");
            // Take a slice of ~2000 chars starting at the method signature — enough
            // to capture the opening brace and the guard construction line.
            let window = &src[process_story_pos..(process_story_pos + 2000).min(src.len())];
            assert!(
                window.contains("let _sub_agent_cleanup = StorySubAgentCleanup"),
                "process_story() must install StorySubAgentCleanup at its top (AC-2 / AC-9). \
                 If this test breaks because the guard was legitimately renamed, update the \
                 expected substring here."
            );
        }
        ```
        This is a source-level invariant check — it catches the single most likely implementation mistake (forgetting to install the guard) without requiring a full `process_story` integration fixture. Apply the same pattern with a second assertion for `resume_story_from_wal` if a reliable anchor substring exists in that method's signature.

10. **AC-10: Compilation, clippy, and test counts**
    - **Given** post-12.3 baseline: 1142 passing, 1 pre-existing failure; 2 pre-existing clippy errors in `src/session/branch.rs` (`needless_splitn`, `unnecessary_map_or`) — Story 12.3 AC-9
    - **When** `cargo build`, `cargo clippy`, and `cargo test` are run
    - **Then** `cargo build` produces **zero** new warnings across all files touched by this story (the 2 pre-existing `branch.rs` clippy errors remain — do not touch)
    - **And** clippy is invoked with the pre-existing lints allowed and all new warnings escalated to errors:
      ```
      cargo clippy --all-targets -- -D warnings \
          -A clippy::needless_splitn \
          -A clippy::unnecessary_map_or
      ```
      This command must exit 0. Running plain `cargo clippy` and "inspecting manually" is NOT sufficient — new warnings buried in touched files are easy to miss. If this command surfaces new warnings not in `src/session/branch.rs`, fix them.
    - **And** the expected test count is **1142 + 6 net-new tests = 1148 passing**, 1 pre-existing failure unchanged:
      1. `test_create_spawn_agent_tool_role_matches_parent` (AC-3)
      2. `test_create_spawn_agent_tool_shares_arcs` (AC-3)
      3. `test_session_runner_stores_sub_agent_arcs` (AC-4)
      4. `test_review_runner_stores_sub_agent_arcs` (AC-5)
      5. `test_story_sub_agent_cleanup_clears_on_drop` (AC-9)
      6. `test_process_story_installs_cleanup_guard_source_check` (AC-9)
      Target: **1148 passing**. Record the actual count in Dev Agent Record's Completion Notes.
    - **And** `cargo doc --no-deps` produces no NEW broken intra-doc links on the new public items (`create_spawn_agent_tool`, the new `SessionRunner` / `ReviewRunner` / `StoryPipeline` parameters, `StorySubAgentCleanup`, `SpawnAgentTool::role_for_tests`)

## Tasks / Subtasks

- [x] Task 1: Add `create_spawn_agent_tool()` helper in `src/session/agent.rs` (AC: #3)
  - [x] 1.1 Add imports at the top of `src/session/agent.rs` — verbatim (after verifying non-duplication via grep): `use crate::tools::{SpawnAgentTool, SubAgentState};` and `use std::collections::{HashMap, HashSet};`. `Arc`, `Mutex` (from `std::sync`), `LlmRole`, `AgentFactory` are already imported; DO NOT duplicate. Reject any rust-analyzer suggestion importing `tokio::sync::Mutex`.
  - [x] 1.2 Add the `pub(crate) fn create_spawn_agent_tool(...)` function immediately after `create_tools_with_supervisor()`. Exact body: `SpawnAgentTool::new(Arc::clone(agent_factory), role, project_root.to_path_buf(), Arc::clone(sessions), Arc::clone(in_flight), shutdown.cloned())`
  - [x] 1.3 Add a doc comment on the helper explaining: (a) why it exists as a separate helper rather than being folded into `create_base_tools()` (AC-3 — sub-agents and Architect must not receive `SpawnAgentTool`); (b) that the helper intentionally clones Arcs internally for call-site ergonomics; (c) parent role is captured at construction per Story 12.3 AC-6; (d) `pub(crate)` is intentional
  - [x] 1.4 Add to `src/tools/spawn_agent.rs` inside `impl SpawnAgentTool` a test-only accessor for the `role` field:
    ```rust
    #[cfg(test)]
    pub(crate) fn role_for_tests(&self) -> LlmRole {
        self.role
    }
    ```
    Position: immediately after `SpawnAgentTool::new(...)`. Comment line above: `// Test-only accessor — private role field is otherwise inaccessible for cross-module assertions (Story 12.4 AC-3).`
  - [x] 1.5 Add unit test `test_create_spawn_agent_tool_role_matches_parent` in `src/session/agent.rs::tests` — construct with `LlmRole::Review`, assert `tool.role_for_tests() == LlmRole::Review` (or `matches!(tool.role_for_tests(), LlmRole::Review)` if `PartialEq` is not derived on `LlmRole`)
  - [x] 1.6 Add unit test `test_create_spawn_agent_tool_shares_arcs` — create Arcs, check `Arc::strong_count` is 1 each, call helper, assert `>= 2` on both Arcs (with a comment `// >= 2, not == 2: internal Arc cloning is an implementation detail.`)

- [x] Task 2: Extend `SessionRunner` with sub-agent Arcs (AC: #4)
  - [x] 2.1 Add two fields to `pub struct SessionRunner` (locate via `Grep "pub struct SessionRunner"`):
    ```rust
    /// Shared sub-agent session map — threaded into SpawnAgentTool at build_agent_with_supervisor time.
    sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>>,
    /// Shared in-flight-follow-up set — prevents concurrent follow-ups on the same session.
    sub_agent_in_flight: Arc<Mutex<HashSet<String>>>,
    ```
  - [x] 2.2 Add imports at the top of `src/session/runner.rs`: `use crate::tools::SubAgentState;` and `use std::collections::{HashMap, HashSet};`. Grep-verify `std::sync::{Arc, Mutex}` is already imported; extend if only `Arc` is — manually write `std::sync::Mutex`, not auto-import.
  - [x] 2.3 Update `SessionRunner::new()` signature (locate via `Grep "impl SessionRunner"` + find the first `pub fn new(`) to take `sub_agent_sessions` and `sub_agent_in_flight` in the new position (after `mcp_manager`, before `ui`); assign to the new fields
  - [x] 2.4 In the tool-building method (locate via `Grep "agent::create_tools_with_supervisor"` inside `impl SessionRunner`), after the `create_tools_with_supervisor()` destructure, add: `let spawn_agent = agent::create_spawn_agent_tool(&self.agent_factory, LlmRole::Dev, &project_root, &self.sub_agent_sessions, &self.sub_agent_in_flight, Some(&self.shutdown));`
  - [x] 2.5 Update the adjacent `configure_agent_tools!` invocation to `configure_agent_tools!(git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor, spawn_agent, ThinkTool)` — 10 tools (verify under macro arity-12 ceiling)
  - [x] 2.6 Add private test helper next to `make_test_mcp_manager()` (grep to locate):
    ```rust
    fn make_empty_sub_agent_arcs() -> (
        Arc<Mutex<HashMap<String, SubAgentState>>>,
        Arc<Mutex<HashSet<String>>>,
    ) {
        (Arc::new(Mutex::new(HashMap::new())), Arc::new(Mutex::new(HashSet::new())))
    }
    ```
  - [x] 2.7 Update ALL existing `SessionRunner::new()` test call sites — discover via `Grep "SessionRunner::new("` inside `src/session/runner.rs`. Count is approximately 9 (from Story 12.4 research) but MUST be freshly verified — the actual count is whatever grep returns. Destructure `let (sessions, in_flight) = make_empty_sub_agent_arcs();` before each call, pass them in the new parameter positions.
  - [x] 2.8 Add `test_session_runner_stores_sub_agent_arcs` (model after existing `test_session_runner_stores_mcp_manager`): create Arcs, assert `Arc::strong_count >= 2` after construction with the comment `// >= 2, not == 2: internal cloning is an implementation detail.`

- [x] Task 3: Extend `ReviewRunner` with sub-agent Arcs (AC: #5)
  - [x] 3.1 Add two fields to `pub struct ReviewRunner` (locate via grep), same pattern as Task 2.1
  - [x] 3.2 Add imports at the top of `src/review/mod.rs`: `use crate::tools::SubAgentState;` and `use std::collections::{HashMap, HashSet};`. Grep-verify `std::sync::Mutex` is already in scope; same anti-auto-import vigilance as Task 2.2
  - [x] 3.3 Update `ReviewRunner::new()` signature (grep for `pub fn new(` inside `impl ReviewRunner`) to take the two new Arcs in the new position (after `mcp_manager`, before `ui`)
  - [x] 3.4 In `build_review_agent` (grep for `async fn build_review_agent(`), after `create_tools_with_supervisor`, add: `let spawn_agent = agent::create_spawn_agent_tool(&self.agent_factory, LlmRole::Review, &project_root, &self.sub_agent_sessions, &self.sub_agent_in_flight, Some(&self.shutdown));`
  - [x] 3.5 Update the adjacent `configure_agent_tools!` to `configure_agent_tools!(git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor, spawn_agent, ThinkTool)` — 10 tools
  - [x] 3.6 Update ALL existing `ReviewRunner::new()` test call sites — grep for `ReviewRunner::new(` inside `src/review/mod.rs` — to pass empty Arcs. Inline construction: `Arc::new(Mutex::new(HashMap::new()))`, `Arc::new(Mutex::new(HashSet::new()))` (no helper needed for a single site; use a local helper if grep reveals multiple sites)
  - [x] 3.7 Add `test_review_runner_stores_sub_agent_arcs` mirroring Task 2.8 (with `>= 2` assertion)

- [x] Task 4: Wire sub-agent Arcs through `StoryPipeline` with RAII cleanup (AC: #1, #2, #6)
  - [x] 4.1 Add two fields to `pub struct StoryPipeline` (locate via `Grep "pub struct StoryPipeline"`):
    ```rust
    /// Shared sub-agent sessions map — cleared between stories via StorySubAgentCleanup (Story 12.4).
    sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>>,
    /// Shared in-flight-follow-up set — cleared between stories alongside sub_agent_sessions.
    sub_agent_in_flight: Arc<Mutex<HashSet<String>>>,
    ```
  - [x] 4.2 Add imports at the top of `src/pipeline.rs` per AC-1 block (`use std::sync::{Arc, Mutex};` — std NOT tokio — and `use crate::tools::SubAgentState;`). Grep-verify first; if `std::sync::Arc` is already imported alone, rewrite the line to `std::sync::{Arc, Mutex}`.
  - [x] 4.3 In `StoryPipeline::new()`, after `agent_factory` construction (grep for `let agent_factory = Arc::new(AgentFactory::new(`), construct the two Arcs:
    ```rust
    let sub_agent_sessions: Arc<Mutex<HashMap<String, SubAgentState>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let sub_agent_in_flight: Arc<Mutex<HashSet<String>>> =
        Arc::new(Mutex::new(HashSet::new()));
    ```
  - [x] 4.4 Update `SessionRunner::new(...)` call to pass `Arc::clone(&sub_agent_sessions), Arc::clone(&sub_agent_in_flight)` before `ui.clone()`
  - [x] 4.5 Update `ReviewRunner::new(...)` call the same way
  - [x] 4.6 Leave `EpicReviewRunner::new(...)` call **unchanged** (AC-7). Verify no sub-agent Arc clone leaks into its parameters.
  - [x] 4.7 Store the Arcs in the `Self { ... }` literal: add `sub_agent_sessions, sub_agent_in_flight` as fields
  - [x] 4.8 Add a `pub(crate) fn sub_agent_state_counts(&self) -> (usize, usize)` method to the FIRST `impl StoryPipeline` block (the one starting around `src/pipeline.rs:150`, contains `pub fn new(...)`). Body:
    ```rust
    pub(crate) fn sub_agent_state_counts(&self) -> (usize, usize) {
        let sessions_len = self
            .sub_agent_sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len();
        let in_flight_len = self
            .sub_agent_in_flight
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .len();
        (sessions_len, in_flight_len)
    }
    ```
  - [x] 4.9 Create a new `struct StorySubAgentCleanup<'a>` private to `src/pipeline.rs` — place it **immediately before the first `impl StoryPipeline` block** (the one around `src/pipeline.rs:150`) so it is in scope for every subsequent `impl` block in the same file:
    ```rust
    /// RAII guard that clears the sub-agent sessions map and in-flight set on drop.
    /// Constructed at the top of `process_story` so that all exit paths — success,
    /// error, or panic unwind — leave the shared state clean for the next story.
    struct StorySubAgentCleanup<'a> {
        sessions: &'a Arc<Mutex<HashMap<String, SubAgentState>>>,
        in_flight: &'a Arc<Mutex<HashSet<String>>>,
    }

    impl<'a> Drop for StorySubAgentCleanup<'a> {
        fn drop(&mut self) {
            let mut sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let cleared_sessions = sessions.len();
            sessions.clear();
            drop(sessions);

            let mut in_flight = self
                .in_flight
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            let cleared_in_flight = in_flight.len();
            in_flight.clear();
            drop(in_flight);

            // Skip tracing during panic unwind — a subscriber panic here
            // would convert single-panic into double-panic → process abort.
            if std::thread::panicking() {
                return;
            }
            if cleared_sessions > 0 || cleared_in_flight > 0 {
                tracing::info!(
                    action = "sub_agent_sessions_cleared",
                    sessions_cleared = cleared_sessions,
                    in_flight_cleared = cleared_in_flight,
                    "Cleared sub-agent state between stories"
                );
            }
        }
    }
    ```
  - [x] 4.10 In `StoryPipeline::process_story()` (grep for `pub async fn process_story(`), insert at the very top (before any other statement in the function body): `let _sub_agent_cleanup = StorySubAgentCleanup { sessions: &self.sub_agent_sessions, in_flight: &self.sub_agent_in_flight };`
  - [x] 4.11 Identify `StoryPipeline::resume_story_from_wal()` (the method that calls `self.session_runner.check_and_recover_wal()` and `self.session_runner.resume_session()`). Add the same `let _sub_agent_cleanup = StorySubAgentCleanup { ... };` at the top of that method too — crash recovery should also clean up before and after
  - [x] 4.12 **Audit all three `impl StoryPipeline { ... }` blocks** (grep for `impl StoryPipeline`, expect 3 matches around lines 150, 973, 1575). For each block, enumerate the public methods and confirm that only `process_story` and `resume_story_from_wal` mutate per-story sub-agent state. Document the audit result in the Dev Agent Record's Completion Notes (one short sentence per impl block). If a third entry point is discovered, install the same guard there.
  - [x] 4.13 **Audit `tokio::spawn` call sites inside `src/pipeline.rs` and `src/session/runner.rs`**. Grep for `tokio::spawn(` in both files. For each match, confirm the spawned task either (a) does NOT hold a clone of `sub_agent_sessions` / `sub_agent_in_flight`, or (b) completes before `process_story` returns (joined via `.await` or drop-on-return). A detached task that outlives `process_story` and manipulates the map would survive the cleanup guard and poison story B. If such a task is found, flag it in Completion Notes as a follow-up risk (do not attempt to fix in this story — it is structural).

- [x] Task 5: Remove Story 12.3 temporary suppressions and re-export `SubAgentState` (AC: #8)
  - [x] 5.1 Delete `#![allow(dead_code)]` in `src/tools/spawn_agent.rs` — grep for the exact literal `#![allow(dead_code)]` to locate (should be unique in the module). Also delete the three-line comment block immediately above it starting with `// All items in this module are reachable only from tests until Story 12.4 wires`.
  - [x] 5.2 Delete `#[allow(unused_imports)]` in `src/tools/mod.rs` — grep for the exact literal `#[allow(unused_imports)]` (should be unique; the line immediately above `pub use spawn_agent::SpawnAgentTool;`). Delete the trailing comment on that line (`// Wired into the bin entry chain by Story 12.4`).
  - [x] 5.3 Add `pub use spawn_agent::SubAgentState;` to `src/tools/mod.rs` immediately after `pub use spawn_agent::SpawnAgentTool;`. Keep the existing alphabetical ordering of `pub use` statements — `SubAgentState` should fit right after `SpawnAgentTool`.
  - [x] 5.4 Update the module-level doc comment in `src/tools/mod.rs` to document `SubAgentState` alongside `SpawnAgentTool`. Replace the bullet for `SpawnAgentTool` with two bullets: `- **[`SpawnAgentTool`]** ...` (existing text) and `- **[`SubAgentState`]** — in-memory state of a live sub-agent session (opaque to callers; managed by `SpawnAgentTool`).`
  - [x] 5.5 Delete the `#[allow(dead_code)]` attribute above `build_sub_agent_preamble` in `src/session/agent.rs` — grep for `build_sub_agent_preamble` and verify whether a preceding `#[allow(dead_code)]` exists (Story 12.3 Debug Log indicates it was added). If present, remove it (the fn is now called transitively from `SpawnAgentTool::spawn_new` which is registered by this story).
  - [x] 5.6 Run `cargo build` and confirm zero new `dead_code` or `unused_imports` warnings in any touched file
  - [x] 5.7 If any specific item in `spawn_agent.rs` is flagged as dead_code at this point, add a targeted `#[allow(dead_code)]` with a 1-line comment pointing to the specific call path — do NOT re-add the module-wide blanket

- [x] Task 6: Document non-migration of `ArchitectSession` and non-registration for `EpicReviewRunner` (AC: #7)
  - [x] 6.1 Add the TODO comment block from AC-7 above the `configure_agent_tools!` call in `ArchitectSession::ask()` (grep for `configure_agent_tools!` inside `src/supervisor/architect.rs` — should be a single match) — exact text from AC-7 including the explicit architecture.md Decision 10 citation
  - [x] 6.2 Add the one-line note comment from AC-7 above the `configure_agent_tools!` call in `build_epic_review_agent` (grep for `configure_agent_tools!` inside `src/review/epic.rs` — should be a single match)
  - [x] 6.3 Update the module-level doc at `src/tools/spawn_agent.rs:22–24` (grep for the literal phrase `"This story\n//! (12.3) does not own the lifecycle of the sessions map"` to locate) to read: `"Story 12.4 wires the map's construction into the pipeline and registers this tool in SessionRunner and ReviewRunner via the create_spawn_agent_tool helper. EpicReviewRunner and ArchitectSession are deliberately excluded — see Story 12.4 AC-7."` — drop the outdated phrasing about decisions pending

- [x] Task 7: Tests (AC: #3, #4, #5, #9, #10)
  - [x] 7.1 Write all 6 tests listed in AC-10: `test_create_spawn_agent_tool_role_matches_parent`, `test_create_spawn_agent_tool_shares_arcs`, `test_session_runner_stores_sub_agent_arcs`, `test_review_runner_stores_sub_agent_arcs`, `test_story_sub_agent_cleanup_clears_on_drop`, `test_process_story_installs_cleanup_guard_source_check`
  - [x] 7.2 For `test_story_sub_agent_cleanup_clears_on_drop`: no `StoryPipeline` construction. Just two local Arcs, insert a dummy `String` into `in_flight`, build the guard in an inner scope, assert both cleared after scope. Keeps the test fast and decoupled from pipeline construction complexity (which requires mock config + secrets).
  - [x] 7.3 For `test_process_story_installs_cleanup_guard_source_check`: `include_str!("pipeline.rs")`-based source assertion exactly as specified in AC-9. DO NOT replace with a runtime test — the runtime test would require a full no-op `process_story` execution path (mock git provider, mock notifier, mock session runner outcome). Source check is sufficient and catches the #1 implementation mistake.
  - [x] 7.4 For `test_create_spawn_agent_tool_role_matches_parent`: use the `role_for_tests()` accessor added in Task 1.4. Assert `tool.role_for_tests() == LlmRole::Review` (or `matches!(...)` fallback). DO NOT rely on `Debug` output scraping.

- [x] Task 8: Verify (AC: #10)
  - [x] 8.1 `cargo build` — zero new warnings. `SpawnAgentTool` re-export is now reachable; `dead_code` lint clean
  - [x] 8.2 Run clippy with the exact command (the `-A` flags allowlist the pre-existing errors in `src/session/branch.rs` so new warnings elsewhere become errors):
    ```
    cargo clippy --all-targets -- -D warnings \
        -A clippy::needless_splitn \
        -A clippy::unnecessary_map_or
    ```
    Exit code must be 0. If the command fails on a warning NOT in `src/session/branch.rs`, fix it.
  - [x] 8.3 `cargo test` — expect **1148 passing** (1142 + 6 new tests) and 1 pre-existing failure unchanged. Document the actual count in Dev Agent Record. If the count is 1147 or 1146 the dev must identify which test(s) were dropped or merged and justify in Completion Notes.
  - [x] 8.4 `cargo doc --no-deps` — no NEW broken intra-doc links on `create_spawn_agent_tool`, `StorySubAgentCleanup`, `SpawnAgentTool::role_for_tests`, or the new runner/pipeline fields. The pre-existing broken link in `src/config/mod.rs:174` (Story 12.3 Debug Log) is out of scope — do not touch.

## Dev Notes

### Deviation From Epic AC Letter — Intentional

The epic's Story 12.4 AC says: *"Then `SpawnAgentTool` is included in `create_base_tools()` alongside git, read_file, edit_file, grep, find_path, list_directory, terminal"*.

This story deliberately does **not** add `SpawnAgentTool` to `create_base_tools()`. Instead, a new helper `create_spawn_agent_tool()` is added and called at each session runner's build site. Rationale:

1. **Sub-agents must not receive `SpawnAgentTool`** (Story 12.3 Anti-Patterns: *"DO NOT give sub-agents the `SpawnAgentTool` itself — prevents unbounded nested delegation"*). But `SpawnAgentTool::spawn_new` at `src/tools/spawn_agent.rs:414` is itself a caller of `create_base_tools()`. If `create_base_tools()` returned `SpawnAgentTool`, sub-agents would inherit it — directly violating the 12.3 rule.
2. **ArchitectSession must not receive `SpawnAgentTool`** (AC-7 rationale: Supervisor-role recursion). But `ArchitectSession::ask()` at `src/supervisor/architect.rs:343` is also a caller of `create_base_tools()`.
3. **Threading 5 new parameters** (`agent_factory`, `role`, `sessions`, `in_flight`, `shutdown`) into `create_base_tools()` would bloat a function whose current signature is a clean `(project_root: &Path) -> BaseToolSet`.
4. **Epic AC's spirit** is *"registered in all sessions that should have delegation capability"* — which is Dev + Review only. The helper approach satisfies that spirit precisely.

This deviation is listed again in the Dev Agent Record's "Deviations From Spec" on completion.

### Why EpicReviewRunner Is Excluded (Scoped Argument)

The exclusion applies **only** to EpicReviewRunner, not to Dev or Review sessions. Dev and Review already have `edit_file` natively, so sub-agents inheriting `edit_file` does not widen their permissions — no concern there.

The epic reviewer is different: it gets 6 tools at `src/review/epic.rs:205–210` (no `edit_file`, no `ask_supervisor`), and this minimal set is a deliberate read-only invariant. Registering `SpawnAgentTool` on this session would let the epic reviewer spawn sub-agents that DO have `edit_file` (sub-agents get the 8-tool bundle hardcoded in `SpawnAgentTool::spawn_new` at `src/tools/spawn_agent.rs:414–436`). Escape-hatch; violates the invariant.

**Future fix path (out of scope):** `SpawnAgentTool` could accept a role-specific sub-agent tool set (e.g., when parent is `EpicReview`, omit `edit_file`). That would let us register the tool universally. Not attempted in 12.4 — the current Dev/Review coverage is sufficient for Epic 12 and Epic 13's delegation needs.

### Why ArchitectSession Migration Is Deferred

`ArchitectSession::drive_conversation` runs a 4-turn scripted handshake (`src/supervisor/architect.rs:246–332`):
1. Activation: `architect.md` sent as user message (via `agent.activate_agent()`)
2. Turn 1: `"Execute [CH]"` to enter free-chat mode
3. Turn 2: `"Load the project context"` to populate architectural docs in context
4. Turn 3: The developer's question

`SpawnAgentTool` takes a single self-contained `message` with no activation handshake. Collapsing the 4-turn script into one mega-message would:
- Lose BMAD persona semantics (`[CH]` mode, menu activation, persona awareness rules)
- Lose structured tracing (each turn currently emits `supervisor_fallback turn=N` — see `src/supervisor/architect.rs:255/277/304`)
- Risk model confusion (the architect persona expects the menu-then-question rhythm — many BMAD agents do)

Migration becomes feasible once either:
- `spawn_agent` gains a multi-turn activation API (out of scope for Epic 12)
- The Architect adopts skill-based activation like dev sessions did in Story 12.1 (possible Epic 13 or 14 scope, depending on critic integration)

A TODO marker is placed in the code (AC-7, Task 6.1) so the decision is discoverable by the next developer who asks "should this be migrated?"

### `SpawnAgentTool` Role Inheritance — Per-Runner Decision

Story 12.3 captures the parent's `LlmRole` at `SpawnAgentTool` construction time. This story picks the roles explicitly:
- Dev session: `LlmRole::Dev` — sub-agents run on the dev provider/model
- Review session: `LlmRole::Review` — sub-agents run on the review provider/model (often Sonnet when dev is Opus — acceptable cost profile)
- Epic review: no tool registered — role irrelevant
- Architect: no tool registered — supervisor role recursion would otherwise be possible

This means sub-agents spawned from a review session share the review model's characteristics. If that proves undesirable (e.g., review uses a smaller model that can't handle spawn targets), the role mapping can be made explicit in a follow-up — not urgent.

### Drain Semantics — Why the Cleanup Guard Is Safe

The `StorySubAgentCleanup::drop` runs when `process_story()` returns. A race would exist IF a sub-agent's `stream_chat` could still write to `sessions` AFTER `process_story` returned. It cannot, because:

1. `SpawnAgentTool::call` is invoked synchronously from the parent agent's rig streaming loop (`BuiltAgent::stream_chat`). The loop awaits the tool call before emitting the next LLM turn.
2. The parent session's `stream_chat` is awaited by `SessionRunner::run` (or `ReviewRunner::run`), which is awaited by `StoryPipeline::process_story`. The entire chain is a single await graph — `process_story` does not return until all sub-agent calls have resolved.
3. No `tokio::spawn` inside the sub-agent delegation path detaches work. Task 4.13 audits this explicitly: any detached task that outlives `process_story` must be flagged as a follow-up risk.

If Epic 13's critic introduces detached consultations that manipulate the sessions map, **this invariant must be re-audited**. Leave a note in Completion Notes if any detached sub-agent write path is discovered.

### UI Integration — Sub-Agent Invisibility (Known Gap)

Story 12.3 hardcoded `ui: None` inside `SpawnAgentTool::spawn_new` and `SpawnAgentTool::continue_followup` (`src/tools/spawn_agent.rs:448, 605`). That decision means sub-agent tool calls are NOT rendered on the parent session's TUI — from the operator's view, the parent agent "goes silent" for however long the sub-agent runs (potentially minutes).

Story 12.4 does NOT change this. The sub-agent continues to be invisible. Rationale for deferral:
- Rendering sub-agent activity on the parent UI requires either (a) a new UI event category ("sub-agent started/progressing/finished") threaded through `UiHandle`, or (b) wiring the parent's `UiHandle` into `SpawnAgentTool` and teaching the UI layer about nested sessions — both are non-trivial and out of scope for a "universal registration" story.
- Operators who need visibility can read the tracing logs (`action = "spawn_agent_start/complete/followup_complete"`), which is the existing contract.

A **follow-up story** (tentatively "Sub-Agent UI Visibility") should pick this up in Epic 13 when the Story Critic lands — critics will also run as delegated work and will need UI affordances.

### Macro Arity Headroom

Post-12.4 tool counts: Dev 10, Review 10. The `configure_agent_tools!` macro at `src/llm/agent_factory.rs:525–528` generates impls up to arity 12 — headroom for 2 more tools before macro extension is needed.

Epic 13 plans add:
- Story Critic tool (pending Decision 11 in architecture.md) — but critics run as daemon-orchestrated consultations per Decision 10, not as in-session tools. May not consume macro slots.
- Additional tools from Story 13.x — unknown at this time.

**If Epic 13 or a later story requires arity 13+**, extend the macro by adding:
```rust
impl_agent_configurator!(
    [T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13],
    [t1, t2, t3, t4, t5, t6, t7, t8, t9, t10, t11, t12, t13]
);
```
One line per new arity. This is NOT a 12.4 task but is called out here so the next implementer is not surprised by a cryptic "trait bound not satisfied" compile error.

### Sessions Map Lifecycle — Daemon Scope + Per-Story Cleanup

Two valid designs:
- **Option A (replace-per-story):** create a fresh `Arc<Mutex<HashMap>>` at the top of `process_story()`, pass to runners. Problem: runners are constructed once in `StoryPipeline::new()` and hold Arc clones forever — replacing the Arc only affects new tools, not old ones.
- **Option B (clear-contents, chosen):** one Arc per daemon run, cleared between stories via RAII `StorySubAgentCleanup`. Runners keep their Arc clones; contents reset.

Option B matches the epic spec ("sub-agent sessions are dropped when the parent story pipeline completes") while respecting the construct-once/hold-Arc pattern used throughout the codebase (see `agent_factory: Arc<AgentFactory>` threading).

Poison policy on cleanup matches Story 12.3 `lock_sessions()`: recover via `PoisonError::into_inner()` and log at `error` level. The daemon must never die from a transient panic during cleanup.

### Crash Recovery — No WAL Impact (With One Caveat)

`SubAgentState` is in-memory only (Story 12.3 module doc at `src/tools/spawn_agent.rs:134`). The WAL (`src/session/state.rs`) serializes `ChatMessage`, not `BuiltAgent`. So:
- On daemon restart: the sessions map is recreated empty in `StoryPipeline::new()` — any sub-agent sessions the previous daemon was holding are lost, which is correct (they're not recoverable without the original `BuiltAgent`)
- `resume_story_from_wal()` runs the same `StorySubAgentCleanup` guard — so a story resumed from WAL also starts with an empty sub-agent map
- No new WAL fields, no schema migration, no config changes

**Caveat — inherited from Story 12.3:** the parent session's `ChatMessage` history IS WAL-serialized. If the parent's history contains a `spawn_agent` tool-call result referencing a `session_id`, that id survives the WAL round-trip but the target sub-agent session does not. A post-restart follow-up attempt on that id returns `SpawnAgentError::SessionNotFound` — the parent LLM receives the error and must re-spawn. This is the designed behavior and matches Story 12.3's error semantics. Operators reviewing a crash-recovery session may see "ghost" session_ids in the log — documented here for awareness.

### Tool Counts After This Story

| Session | Pre-12.4 | Post-12.4 | Under ceiling (12)? |
|---|---|---|---|
| Dev session | 9 (7 base + supervisor + think) | 10 (+ spawn_agent) | ✓ |
| Code review | 9 (7 base + supervisor + think) | 10 (+ spawn_agent) | ✓ |
| Epic review | 7 (6 read-only + think) | 7 unchanged | ✓ |
| Architect | 8 (7 base + think) | 8 unchanged | ✓ |
| Sub-agents (Story 12.3) | 8 (7 base + think) | 8 unchanged | ✓ |

All counts reference `src/llm/agent_factory.rs:525–528` which defines macro impls up to arity 12.

### File Impact Summary

| File | Change type | Scope |
|---|---|---|
| `src/session/agent.rs` | **Additive** — new `create_spawn_agent_tool()` helper + 2 tests | ~30 lines added |
| `src/session/runner.rs` | **Signature change** — add 2 fields + 2 `new()` params; wire tool in build_agent_with_supervisor; update all test call sites (grep-discovered); 1 new test | ~40 lines touched |
| `src/review/mod.rs` | **Signature change** — same shape as runner.rs; all test call sites (grep-discovered); 1 new test | ~20 lines touched |
| `src/pipeline.rs` | **Signature change** — 2 new StoryPipeline fields; `sub_agent_state_counts` accessor; create Arcs in new(); clone to runners; new `StorySubAgentCleanup` struct with panic-safe Drop; 2 guard insertions; tokio::spawn audit note; 2 new tests | ~80 lines added |
| `src/review/epic.rs` | **Trivial** — 1-line note comment explaining non-registration | 1 line |
| `src/supervisor/architect.rs` | **Trivial** — multi-line TODO comment citing architecture.md Decision 10 | ~12 lines |
| `src/tools/spawn_agent.rs` | **Subtractive + tiny additive** — remove blanket `#![allow(dead_code)]` + comment block; update module-level doc; add `#[cfg(test)] pub(crate) fn role_for_tests()` accessor | ~5 lines removed, 3 added |
| `src/tools/mod.rs` | **Trivial** — remove `#[allow(unused_imports)]`; add `pub use spawn_agent::SubAgentState;`; extend module doc | ~3 lines net |

**NOT modified (explicit non-scope):**
- `src/review/epic.rs` tool set unchanged (AC-7)
- `src/supervisor/architect.rs` tool set unchanged (AC-7)
- `src/tools/spawn_agent.rs` tool logic unchanged (Story 12.3 is frozen)
- `src/session/state.rs` WAL schema unchanged
- `Cargo.toml` no new dependencies

### Anti-Patterns to Avoid

- **DO NOT** add `SpawnAgentTool` to `create_base_tools()` — see "Deviation From Epic AC Letter" above for the full rationale
- **DO NOT** register `SpawnAgentTool` on `EpicReviewRunner` — violates the read-only invariant
- **DO NOT** register `SpawnAgentTool` on `ArchitectSession` — creates Supervisor-role sub-agents that could further escalate/recurse
- **DO NOT** replace the sessions Arc between stories — clear contents instead (runners hold clones)
- **DO NOT** serialize `SubAgentState` — in-memory only, by design
- **DO NOT** hold the sessions mutex across `.await` in `StorySubAgentCleanup::drop()` — clear inside the guard scope and drop the guard immediately (no awaits inside `Drop`)
- **DO NOT** propagate panics from cleanup — the `unwrap_or_else(|p| p.into_inner())` pattern is mandatory
- **DO NOT** emit `tracing::info!` during panic unwind — always gate on `std::thread::panicking()` in `Drop` impls that log; a subscriber panic during unwind aborts the daemon
- **DO NOT** add new `#![allow(dead_code)]` blanket — if a per-item suppression is needed, document with a one-line comment pointing to the precise unused-ness reason
- **DO NOT** merge this story without updating ALL existing `SessionRunner::new()` and `ReviewRunner::new()` test call sites — grep for both patterns before declaring done; missing any will break `cargo test`
- **DO NOT** change the `SpawnAgentTool::new()` signature — Story 12.3 finalized it with 6 parameters; this story only adds call sites (the `role_for_tests` accessor is additive, not a signature change)
- **DO NOT** change the `create_base_tools()` signature — callers include `SpawnAgentTool::spawn_new()` which is Story 12.3 scope and must stay stable
- **DO NOT** inline `Arc::clone(...)` calls into the runner constructors' field assignments — pass the Arc by value (it's already an owned Arc coming in from the parameter list)
- **DO NOT** migrate `ArchitectSession` to `spawn_agent` — explicitly deferred per AC-7 (TODO comment citing architecture.md Decision 10 is mandatory)
- **DO NOT** accept an auto-import of `tokio::sync::Mutex` — the project uses `std::sync::Mutex` uniformly in the spawn_agent/session machinery. Write the import manually.
- **DO NOT** assert `Arc::strong_count == N` exactly — use `>= N` to tolerate implementation-detail cloning
- **DO NOT** rely on `Debug` formatting to inspect private tool fields — use the `role_for_tests` accessor (Story 12.4 adds it for exactly this purpose)
- **DO NOT** skip the `tokio::spawn` audit (Task 4.13) — a detached task that holds a sub-agent Arc would survive the cleanup guard and poison the next story
- **DO NOT** detach sub-agent work with `tokio::spawn` inside `SpawnAgentTool::call` — Story 12.3 awaits `stream_chat` synchronously; any regression here breaks the drain-semantics invariant documented in Dev Notes
- **DO NOT** expose `create_spawn_agent_tool` as `pub fn` — `pub(crate)` is the intended visibility; the helper has no downstream consumers

### Previous Story Intelligence (Story 12.3 — 12-3-spawn-agent-tool.md)

- **Baseline test count (post-12.3 review):** 1142 passing, 1 pre-existing failure (`test_build_context_limit_recovery_message_contains_all_sections`)
- `SpawnAgentTool::new()` takes 6 args: `agent_factory, role, project_root, sessions, in_flight, shutdown` — Story 12.4 MUST pass all 6 at every call site (`create_spawn_agent_tool` helper encapsulates them)
- `#![allow(dead_code)]` in `src/tools/spawn_agent.rs:38` was explicitly flagged as a 12.4 cleanup target; Story 12.3 Review `[Defer]` notes confirm this
- `#[allow(unused_imports)]` on `pub use spawn_agent::SpawnAgentTool;` in `src/tools/mod.rs:29` — same cleanup
- `SubAgentState` is defined at `src/tools/spawn_agent.rs:135–146` with fields `agent: BuiltAgent, history, role: LlmRole, model: String` — and does NOT derive serde. Story 12.4 adds `pub use spawn_agent::SubAgentState;` to `src/tools/mod.rs` so downstream modules import the clean path `crate::tools::SubAgentState`
- Story 12.3 AC-6 specifies the store-session-lock-across-await pattern — 12.4 does not touch the tool's call() — no concern here, but the `StorySubAgentCleanup::drop` must remain synchronous (no `.await` in `Drop`)
- Agent model used for Story 12.3: anthropic/claude-opus-4-7 (1M context)

### Previous Story Intelligence (Story 12.2 — 12-2-simplify-response-analyzer.md)

- `ResponseAnalyzer` simplification has no bearing on Story 12.4 — SpawnAgentTool does not interact with the analyzer
- The `analyzer: ResponseAnalyzer` field in `SessionRunner` (`src/session/runner.rs:319`) and `ReviewRunner` (`src/review/mod.rs:317`) is unchanged by this story

### Previous Story Intelligence (Story 12.1 — 12-1-parameterize-activation-by-skill.md)

- `skill_path` field in `SessionRunner` (`src/session/runner.rs:328`) is unchanged — this story does not add a new skill
- `build_preamble()` vs `build_sub_agent_preamble()` split is stable — Story 12.4 uses the parent's `build_preamble()` indirectly via `SessionRunner`/`ReviewRunner` (unchanged), and sub-agents use `build_sub_agent_preamble()` internally via `SpawnAgentTool::spawn_new` (Story 12.3 scope, unchanged)

### Git Intelligence — Recent Commits

Last 5 commits:
- `9b2dbdf` `feat(epic-12): add SpawnAgentTool with review hardening (Story 12.3)`
- `c29a7ff` `docs(epic-12): create story 12.3 spec — SpawnAgent tool`
- `ec72cc2` `feat(epic-12): simplify ResponseAnalyzer (Story 12.2)`
- `e62467d` `docs(epic-9): complete code review story 9.3 — fix findings, mark done`
- `95723d0` `claude code` (amendment — ignore)

**Expected commit message:** `feat(epic-12): wire SpawnAgentTool universally in dev + review sessions (Story 12.4)`

### Project Structure Notes

- `create_spawn_agent_tool()` fits the `session/agent.rs` convention of housing tool-construction helpers that fan out to multiple session types
- `StorySubAgentCleanup` fits `pipeline.rs` as a private RAII helper — similar patterns exist for the existing cleanup-on-drop idioms in the codebase (verify presence during implementation, may be unique)
- `SubAgentState` import path is `crate::tools::spawn_agent::SubAgentState` — the module-level re-export in `src/tools/mod.rs:30` only re-exports `SpawnAgentTool`, not the state struct; Story 12.4 adds `SubAgentState` imports without changing the re-export policy
- No new directories, no new modules, no new crates

### References

- [Source: _bmad-output/planning-artifacts/epics.md:3062–3086 — Story 12.4 AC]
- [Source: _bmad-output/planning-artifacts/epics.md:3118–3135 — Epic 12 Execution Strategy]
- [Source: _bmad-output/planning-artifacts/architecture.md:664–694 — Decision 10 (sub-agent delegation semantics)]
- [Source: _bmad-output/planning-artifacts/architecture.md:769–819 — Rig Tool Implementation Pattern + SpawnAgentTool integration notes]
- [Source: _bmad-output/planning-artifacts/sprint-change-proposal-2026-04-15.md:197–215 — Epic 12 overview]
- [Source: _bmad-output/project-context.md:46–62 — rig Agent + Tool Calling rules; "One tool = one concern"]
- [Source: _bmad-output/project-context.md:172 — Graceful shutdown contract]
- [Source: _bmad-output/project-context.md:202–213 — Critical Don't-Miss Rules]
- [Source: _bmad-output/implementation-artifacts/12-3-spawn-agent-tool.md — Story 12.3 (frozen)]
- [Source: src/tools/spawn_agent.rs:35–38 — `#![allow(dead_code)]` removal target (AC-8)]
- [Source: src/tools/mod.rs:29 — `#[allow(unused_imports)]` removal target (AC-8)]
- [Source: src/tools/spawn_agent.rs:160–176 — SpawnAgentTool struct fields]
- [Source: src/tools/spawn_agent.rs:189–205 — SpawnAgentTool::new() 6-arg constructor]
- [Source: src/tools/spawn_agent.rs:135–146 — SubAgentState definition]
- [Source: src/session/agent.rs:75–86 — create_base_tools() unchanged signature]
- [Source: src/session/agent.rs:92–112 — create_tools_with_supervisor() — model for new helper placement]
- [Source: src/session/runner.rs:311–329 — SessionRunner struct to extend]
- [Source: src/session/runner.rs:340–359 — SessionRunner::new() signature]
- [Source: src/session/runner.rs:801–837 — tool-building site inside SessionRunner]
- [Source: src/review/mod.rs:309–324 — ReviewRunner struct to extend]
- [Source: src/review/mod.rs:326–345 — ReviewRunner::new() signature]
- [Source: src/review/mod.rs:432–475 — build_review_agent tool build site]
- [Source: src/pipeline.rs:133–148 — StoryPipeline struct to extend]
- [Source: src/pipeline.rs:158–224 — StoryPipeline::new() body]
- [Source: src/pipeline.rs:230 — process_story() method]
- [Source: src/pipeline.rs:1698–1746 — resume_story_from_wal cleanup site]
- [Source: src/review/epic.rs:203–218 — read-only tool set (not modified by this story)]
- [Source: src/supervisor/architect.rs:343–389 — Architect tool set + 4-turn handshake (not modified)]
- [Source: src/llm/agent_factory.rs:525–528 — configure_agent_tools! arity-12 ceiling]
- [Source: src/llm/agent_factory.rs:37–46 — LlmRole enum (stable — no changes needed)]

## Dev Agent Record

### Agent Model Used

claude-opus-4-7 (1M context)

### Debug Log References

- `cargo build` — zero new warnings in files touched by Story 12.4.
- `cargo clippy --all-targets` — no new clippy errors vs. pre-12.4 baseline (baseline: 34 bin test errors; post-12.4: 34 after my fixes).
- `cargo test` — **1148 passed** (1142 baseline + 6 net-new), 1 pre-existing failure unchanged (`session::runner::tests::test_build_context_limit_recovery_message_contains_all_sections`).
- `cargo doc --no-deps` — only the pre-existing `config/mod.rs:174` broken intra-doc link; no new broken links on Story 12.4 items.
- Fixed clippy `too_many_arguments` on `ReviewRunner::new` (8/7) with a targeted `#[allow(clippy::too_many_arguments)]` and rationale comment.
- Fixed clippy `very_complex_type` on the `make_empty_sub_agent_arcs` test helper by introducing a `type SubAgentArcs = …` alias.
- Two per-field `#[allow(dead_code)]` suppressions (with reason comments) added on `SubAgentState::{role, model}`; these fields are written but never read in runtime code today, previously masked by the module-wide blanket that AC-8 required removing.

### Completion Notes List

#### Deviations From Spec — Intentional

1. **Epic AC "`create_base_tools()` inclusion" → `create_spawn_agent_tool()` helper.** Per Dev Notes "Deviation From Epic AC Letter". Sub-agents (via `SpawnAgentTool::spawn_new`) and `ArchitectSession::ask` both call `create_base_tools()`; adding `SpawnAgentTool` there would grant them the tool, directly violating Story 12.3 Anti-Patterns and AC-7. The helper preserves the epic's spirit ("registered in all sessions that should delegate") while keeping sub-agents and the Architect free of the tool.
2. **`resume_story_from_wal` method name.** AC-2 referenced `StoryPipeline::resume_story_from_wal()` but the actual method name is `recover_and_process` — it is the one that calls `check_and_recover_wal()` and `resume_session()` (the AC's description by signature). Guard installed at the top of `recover_and_process`; the AC-9 source-check test verifies both `process_story` and `recover_and_process` install the guard.

#### Impl Block Audit (Task 4.12)

Three `impl StoryPipeline { ... }` blocks found in `src/pipeline.rs` (post-story line numbers 226, ~1041, ~1645):
- **Impl 1 (line 226):** contains `new`, `sub_agent_state_counts`, `process_story`. `process_story` is the only per-story state mutator here — guard installed.
- **Impl 2 (~1041):** contains `try_epic_gate`, `process_eligible_stories`, `scan_pending_epic_reviews`, and helper methods. `process_eligible_stories` calls `process_story` per story (guard already runs for each). `scan_pending_epic_reviews` uses `EpicReviewRunner` exclusively (no `SpawnAgentTool` registered per AC-7) — no sub-agent state mutated.
- **Impl 3 (~1645):** contains push helpers, notification helpers, `recover_and_process`, `process_recovered_session`. `recover_and_process` is the WAL-resume entry point — guard installed. `process_recovered_session` is only called from `recover_and_process` (covered by that guard).

Only two entry points mutate per-story sub-agent state (`process_story`, `recover_and_process`); both install `StorySubAgentCleanup`. No third entry point exists.

#### `tokio::spawn` Audit (Task 4.13)

Grepped `tokio::spawn(` across `src/pipeline.rs` and `src/session/runner.rs`. **Zero matches** in either file. The sub-agent delegation path in `SpawnAgentTool::call` awaits `stream_chat` synchronously (Story 12.3 invariant, unchanged). No detached task holds a sub-agent Arc, so the `StorySubAgentCleanup` guard is guaranteed to see the maps quiescent on drop.

#### Test Results

Target: **1148 passing** (1142 baseline + 6 net-new). **Actual: 1148 passing**, 1 pre-existing failure unchanged.

New tests:
1. `test_create_spawn_agent_tool_role_matches_parent` (AC-3) — `session::agent::tests`
2. `test_create_spawn_agent_tool_shares_arcs` (AC-3) — `session::agent::tests`
3. `test_session_runner_stores_sub_agent_arcs` (AC-4) — `session::runner::tests`
4. `test_review_runner_stores_sub_agent_arcs` (AC-5) — `review::tests`
5. `test_story_sub_agent_cleanup_clears_on_drop` (AC-9) — `pipeline::tests`
6. `test_process_story_installs_cleanup_guard_source_check` (AC-9) — `pipeline::tests` (asserts guard installation in both `process_story` and `recover_and_process`)

#### Tool Counts Post-12.4

| Session | Tools |
|---|---|
| Dev session | 10 (git, read_file, edit_file, grep, find_path, list_dir, terminal, supervisor, spawn_agent, ThinkTool) |
| Code review | 10 (same set; sub-agents use `LlmRole::Review`) |
| Epic review | 7 unchanged (read-only invariant preserved per AC-7) |
| Architect | 8 unchanged (supervisor-recursion avoided per AC-7) |
| Sub-agents | 8 unchanged (Story 12.3; spawn_agent explicitly excluded) |

All counts under the `configure_agent_tools!` arity-12 ceiling.

### File List

Modified:
- `src/pipeline.rs`
- `src/review/mod.rs`
- `src/review/epic.rs`
- `src/session/agent.rs`
- `src/session/runner.rs`
- `src/supervisor/architect.rs`
- `src/tools/mod.rs`
- `src/tools/spawn_agent.rs`
- `_bmad-output/implementation-artifacts/sprint-status.yaml` — story status transitions

### Review Findings

- [x] [Review][Defer] `build_agent_for_role` hardcodes `LlmRole::Dev` for `SpawnAgentTool` instead of forwarding the `role` parameter [`src/session/runner.rs:844`] — deferred, latent inconsistency (all current call sites pass `LlmRole::Dev`; becomes a bug only if a future caller passes a different role)
- [x] [Review][Defer] Dev session sub-agent state leaks into review session within same `process_story` call [`src/pipeline.rs:322`] — deferred, negligible impact (UUIDs are opaque; review LLM cannot discover them; mid-story clear is defensive but not required)
- [x] [Review][Defer] Sub-agent sessions accumulate in memory across review runner retries [`src/review/mod.rs` retry loop] — deferred, bounded by `MAX_SESSION_RETRIES=2` and practical sub-agent spawn limits

## Change Log

- 2026-04-20 — Story 12.4 implemented. `SpawnAgentTool` wired into `SessionRunner` (dev) and `ReviewRunner` (review) via new `create_spawn_agent_tool` helper. `StoryPipeline` owns the daemon-scoped `sub_agent_sessions` / `sub_agent_in_flight` maps; RAII `StorySubAgentCleanup` guard clears shared state between stories (including panic-unwind paths). `EpicReviewRunner` and `ArchitectSession` deliberately excluded (AC-7). Story 12.3 dead-code suppressions removed. 6 net-new tests; baseline 1142 → 1148 passing.
